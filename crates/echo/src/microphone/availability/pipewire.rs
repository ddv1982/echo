use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use pipewire as pw;
use pw::{
    proxy::ProxyT,
    spa::{
        param::{ParamInfoFlags, ParamType},
        pod::{deserialize::PodDeserializer, Object, Value, ValueArray},
        sys,
        utils::Direction,
    },
    types::ObjectType,
};

use super::{
    BackendHealth, EndpointMetadata, EndpointRole, MicrophoneId, NativeSnapshot, RouteAvailability,
};

type Global = pw::registry::GlobalObject<pw::properties::PropertiesBox>;

#[derive(Debug, Default)]
struct Routes {
    info_received: bool,
    query_complete: bool,
    readable: Vec<ParamType>,
    active: Vec<Route>,
    candidates: Vec<Route>,
    error: Option<String>,
}

#[derive(Debug)]
struct Route {
    direction: Direction,
    device: Option<i32>,
    devices: Vec<i32>,
    availability: RouteAvailability,
}

fn parse_route(value: Value) -> Result<Route, String> {
    let Value::Object(Object {
        type_,
        id,
        properties,
    }) = value
    else {
        return Err("PipeWire route is not an object".into());
    };
    if type_ != sys::SPA_TYPE_OBJECT_ParamRoute {
        return Err("PipeWire parameter is not a route".into());
    }
    let mut route = Route {
        direction: Direction::Input,
        device: None,
        devices: Vec::new(),
        availability: RouteAvailability::Unknown,
    };
    let mut has_devices = false;
    let mut has_direction = false;
    for property in properties {
        match (property.key, property.value) {
            (sys::SPA_PARAM_ROUTE_direction, Value::Id(id))
                if id.0 == sys::SPA_DIRECTION_INPUT || id.0 == sys::SPA_DIRECTION_OUTPUT =>
            {
                has_direction = true;
                route.direction = Direction::from_raw(id.0);
            }
            (sys::SPA_PARAM_ROUTE_device, Value::Int(device)) if device >= 0 => {
                route.device = Some(device)
            }
            (sys::SPA_PARAM_ROUTE_devices, Value::ValueArray(ValueArray::Int(devices)))
                if devices.iter().all(|device| *device >= 0) =>
            {
                has_devices = true;
                route.devices = devices
            }
            (sys::SPA_PARAM_ROUTE_available, Value::Id(id)) => {
                route.availability = match id.0 {
                    sys::SPA_PARAM_AVAILABILITY_unknown => RouteAvailability::Unknown,
                    sys::SPA_PARAM_AVAILABILITY_no => RouteAvailability::Unavailable,
                    sys::SPA_PARAM_AVAILABILITY_yes => RouteAvailability::Available,
                    _ => return Err("PipeWire route has invalid availability".into()),
                };
            }
            (
                sys::SPA_PARAM_ROUTE_direction
                | sys::SPA_PARAM_ROUTE_device
                | sys::SPA_PARAM_ROUTE_devices
                | sys::SPA_PARAM_ROUTE_available,
                _,
            ) => {
                return Err("PipeWire route has malformed join or availability data".into());
            }
            _ => {}
        }
    }
    if !has_direction {
        return Err("PipeWire route has no direction".into());
    }
    if (id == ParamType::Route.as_raw() && route.device.is_none())
        || (id == ParamType::EnumRoute.as_raw() && !has_devices)
    {
        return Err("PipeWire route has no profile-device join".into());
    }
    Ok(route)
}

fn route_availability(routes: &Routes, profile_device: i32) -> Result<RouteAvailability, String> {
    if !routes.info_received {
        return Err("PipeWire device information did not arrive".into());
    }
    let active = routes
        .active
        .iter()
        .find(|route| route.direction == Direction::Input && route.device == Some(profile_device));
    if active.is_some_and(|route| route.availability == RouteAvailability::Unavailable) {
        return Ok(RouteAvailability::Unavailable);
    }
    if let Some(error) = &routes.error {
        return Err(error.clone());
    }
    if let Some(active) = active {
        return Ok(active.availability);
    }
    let candidates: Vec<_> = routes
        .candidates
        .iter()
        .filter(|route| {
            route.direction == Direction::Input && route.devices.contains(&profile_device)
        })
        .map(|route| route.availability)
        .collect();
    Ok(
        if !candidates.is_empty()
            && candidates
                .iter()
                .all(|value| *value == RouteAvailability::Unavailable)
        {
            RouteAvailability::Unavailable
        } else if candidates.contains(&RouteAvailability::Available) {
            RouteAvailability::Available
        } else {
            RouteAvailability::Unknown
        },
    )
}

fn endpoint_role(media_class: Option<&str>) -> EndpointRole {
    match media_class {
        Some(
            "Audio/Source"
            | "Audio/Source/Virtual"
            | "Audio/Source/Internal"
            | "Audio/Duplex"
            | "Audio/Duplex/Internal",
        ) => EndpointRole::Source,
        Some("Audio/Sink" | "Audio/Sink/Internal") => EndpointRole::Playback,
        _ => EndpointRole::Other,
    }
}

fn source_route(
    props: Option<&pw::spa::utils::dict::DictRef>,
    devices: &HashMap<u32, Rc<RefCell<Routes>>>,
) -> Result<RouteAvailability, String> {
    let props = props.ok_or("PipeWire source node information did not arrive")?;
    let Some(device) = props.get("device.id") else {
        return if props.get("card.profile.device").is_some()
            || matches!(props.get("device.api"), Some("alsa" | "bluez5"))
        {
            Err("PipeWire hardware source has no device identity".into())
        } else {
            Ok(RouteAvailability::Unknown)
        };
    };
    let device = device
        .parse::<u32>()
        .map_err(|_| "PipeWire source has an invalid device identity")?;
    let routes = devices
        .get(&device)
        .ok_or("PipeWire source device metadata is missing")?
        .borrow();
    let Some(profile_device) = props.get("card.profile.device") else {
        if let Some(error) = &routes.error {
            return Err(error.clone());
        }
        return if routes.info_received {
            Ok(RouteAvailability::Unknown)
        } else {
            Err("PipeWire device information did not arrive".into())
        };
    };
    let profile_device = profile_device
        .parse::<i32>()
        .ok()
        .filter(|value| *value >= 0)
        .ok_or("PipeWire source has an invalid profile-device identity")?;
    route_availability(&routes, profile_device)
}

fn roundtrip(
    core: &pw::core::CoreRc,
    mainloop: &pw::main_loop::MainLoopRc,
    deadline: Instant,
    fatal: &RefCell<Option<String>>,
) -> Result<(), String> {
    let pending = core.sync(0).map_err(|error| error.to_string())?;
    let done = Rc::new(Cell::new(false));
    let observed = done.clone();
    let _listener = core
        .add_listener_local()
        .done(move |id, sequence| {
            if id == pw::core::PW_ID_CORE && sequence == pending {
                observed.set(true);
            }
        })
        .register();
    while !done.get() {
        if let Some(error) = fatal.borrow().as_ref() {
            return Err(error.clone());
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("PipeWire microphone metadata query timed out")?;
        if mainloop.loop_().iterate(pw::loop_::Timeout::Finite(
            remaining.min(Duration::from_millis(20)),
        )) < 0
        {
            return Err("PipeWire microphone metadata loop failed".into());
        }
    }
    Ok(())
}

pub(super) fn collect() -> NativeSnapshot {
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut snapshot = NativeSnapshot::empty(BackendHealth::Unreachable);
    if let Err(error) = collect_into(&mut snapshot, deadline) {
        snapshot.warning = Some(error);
    }
    snapshot
}

fn collect_into(snapshot: &mut NativeSnapshot, deadline: Instant) -> Result<(), String> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(|error| error.to_string())?;
    let context =
        pw::context::ContextRc::new(&mainloop, None).map_err(|error| error.to_string())?;
    let core = context
        .connect_rc(None)
        .map_err(|error| error.to_string())?;
    let reached = Rc::new(Cell::new(false));
    let observed = reached.clone();
    let fatal = Rc::new(RefCell::new(None));
    let failure = fatal.clone();
    let object_errors = Rc::new(RefCell::new(HashMap::new()));
    let errors = object_errors.clone();
    let _core_listener = core
        .add_listener_local()
        .info(move |_| observed.set(true))
        .error(move |id, _, _, message| {
            if id == pw::core::PW_ID_CORE {
                *failure.borrow_mut() = Some(message.to_owned());
            } else {
                errors.borrow_mut().insert(id, message.to_owned());
            }
        })
        .register();
    let registry = core.get_registry().map_err(|error| error.to_string())?;
    let globals = Rc::new(RefCell::new(HashMap::<u32, Global>::new()));
    let added = globals.clone();
    let removed = globals.clone();
    let _registry_listener = registry
        .add_listener_local()
        .global(move |global| {
            added.borrow_mut().insert(global.id, global.to_owned());
        })
        .global_remove(move |id| {
            removed.borrow_mut().remove(&id);
        })
        .register();
    let initial = roundtrip(&core, &mainloop, deadline, &fatal);
    if reached.get() {
        snapshot.health = BackendHealth::Reachable;
    }
    initial?;
    snapshot.health = BackendHealth::Reachable;

    let mut nodes = HashMap::<u32, Rc<RefCell<Option<pw::properties::PropertiesBox>>>>::new();
    let mut node_proxies = Vec::new();
    let mut node_listeners = Vec::new();
    let mut devices = HashMap::<u32, Rc<RefCell<Routes>>>::new();
    let mut device_proxies = Vec::new();
    let mut device_listeners = Vec::new();
    let mut query_listeners = Vec::new();
    let mut metadata_proxies = Vec::new();
    let mut metadata_listeners = Vec::new();
    let default_name = Rc::new(RefCell::new(None));
    let metadata_error = Rc::new(RefCell::new(None));
    for global in globals.borrow().values() {
        if global.type_ == ObjectType::Node
            && endpoint_role(
                global
                    .props
                    .as_ref()
                    .and_then(|props| props.get("media.class")),
            ) == EndpointRole::Source
        {
            let state = Rc::new(RefCell::new(None));
            nodes.insert(global.id, state.clone());
            let node = match registry.bind::<pw::node::Node, _>(global) {
                Ok(node) => node,
                Err(error) => {
                    snapshot.warning.get_or_insert_with(|| error.to_string());
                    continue;
                }
            };
            let info_state = state.clone();
            let listener = node
                .add_listener_local()
                .info(move |info| {
                    if let Some(props) = info.props() {
                        *info_state.borrow_mut() =
                            Some(pw::properties::PropertiesBox::from_dict(props));
                    }
                })
                .register();
            node_listeners.push(listener);
            node_proxies.push(node);
        } else if global.type_ == ObjectType::Device {
            let state = Rc::new(RefCell::new(Routes::default()));
            devices.insert(global.id, state.clone());
            let device = match registry.bind::<pw::device::Device, _>(global) {
                Ok(device) => device,
                Err(error) => {
                    state.borrow_mut().error = Some(error.to_string());
                    continue;
                }
            };
            let info_state = state.clone();
            let param_state = state.clone();
            let listener = device
                .add_listener_local()
                .info(move |info| {
                    let mut state = info_state.borrow_mut();
                    state.info_received = true;
                    if info.params().iter().any(|param| {
                        (param.id() == ParamType::Route || param.id() == ParamType::EnumRoute)
                            && !param.flags().contains(ParamInfoFlags::READ)
                    }) {
                        state.error = Some("PipeWire device route metadata is not readable".into());
                    }
                    state.readable = info
                        .params()
                        .iter()
                        .filter(|param| param.flags().contains(ParamInfoFlags::READ))
                        .map(|param| param.id())
                        .collect();
                })
                .param(move |_, kind, _, _, pod| {
                    if kind != ParamType::Route && kind != ParamType::EnumRoute {
                        return;
                    }
                    let mut state = param_state.borrow_mut();
                    let parsed = pod
                        .ok_or_else(|| "PipeWire returned an empty route parameter".to_string())
                        .and_then(|pod| {
                            PodDeserializer::deserialize_any_from(pod.as_bytes())
                                .map(|(_, value)| value)
                                .map_err(|error| format!("PipeWire route decode failed: {error:?}"))
                        })
                        .and_then(parse_route);
                    match parsed {
                        Ok(route) if kind == ParamType::Route && route.device.is_none() => {
                            state.error =
                                Some("PipeWire active route has no profile-device identity".into());
                        }
                        Ok(route) if kind == ParamType::Route => state.active.push(route),
                        Ok(route) => state.candidates.push(route),
                        Err(error) => state.error = Some(error),
                    }
                })
                .register();
            device_listeners.push(listener);
            device_proxies.push((device, state));
        } else if global.type_ == ObjectType::Metadata
            && global
                .props
                .as_ref()
                .and_then(|props| props.get("metadata.name"))
                == Some("default")
        {
            let metadata = match registry.bind::<pw::metadata::Metadata, _>(global) {
                Ok(metadata) => metadata,
                Err(error) => {
                    *metadata_error.borrow_mut() = Some(error.to_string());
                    continue;
                }
            };
            let target = default_name.clone();
            let failure = metadata_error.clone();
            let listener = metadata
                .add_listener_local()
                .property(move |subject, key, _, value| {
                    if subject == pw::core::PW_ID_CORE && key == Some("default.audio.source") {
                        match value {
                            None => *target.borrow_mut() = None,
                            Some(value) => match serde_json::from_str::<serde_json::Value>(value)
                                .ok()
                                .and_then(|value| {
                                    value
                                        .get("name")
                                        .and_then(|name| name.as_str())
                                        .map(str::to_owned)
                                }) {
                                Some(name) => *target.borrow_mut() = Some(name),
                                None => {
                                    *failure.borrow_mut() =
                                        Some("PipeWire default source metadata is malformed".into())
                                }
                            },
                        }
                    }
                    0
                })
                .register();
            metadata_listeners.push(listener);
            metadata_proxies.push(metadata);
        }
    }
    let bound = roundtrip(&core, &mainloop, deadline, &fatal);
    if bound.is_ok() {
        for (device, state) in &device_proxies {
            let mut queried = false;
            for kind in [ParamType::Route, ParamType::EnumRoute] {
                if state.borrow().readable.contains(&kind) {
                    queried = true;
                    device.enum_params(0, Some(kind), 0, u32::MAX);
                }
            }
            if queried {
                match core.sync(0) {
                    Ok(pending) => {
                        let completed = state.clone();
                        query_listeners.push(
                            core.add_listener_local()
                                .done(move |id, sequence| {
                                    if id == pw::core::PW_ID_CORE && sequence == pending {
                                        completed.borrow_mut().query_complete = true;
                                    }
                                })
                                .register(),
                        );
                    }
                    Err(error) => state.borrow_mut().error = Some(error.to_string()),
                }
            } else {
                let mut state = state.borrow_mut();
                state.query_complete = state.info_received;
            }
        }
    }
    let complete = bound.and_then(|()| roundtrip(&core, &mainloop, deadline, &fatal));
    for (device, state) in &device_proxies {
        let error = object_errors
            .borrow()
            .get(&device.upcast_ref().id())
            .cloned()
            .or_else(|| {
                (!state.borrow().query_complete)
                    .then(|| complete.as_ref().err().cloned())
                    .flatten()
            });
        if let Some(error) = error {
            state.borrow_mut().error = Some(error);
        }
    }
    devices.retain(|id, _| globals.borrow().contains_key(id));
    for global in globals
        .borrow()
        .values()
        .filter(|global| global.type_ == ObjectType::Node)
    {
        let Some(props) = global.props.as_ref() else {
            continue;
        };
        let Some(name) = props.get("node.name") else {
            continue;
        };
        let Ok(id) = MicrophoneId::parse(format!("pipewire:{name}")) else {
            continue;
        };
        let role = nodes
            .get(&global.id)
            .and_then(|state| {
                state
                    .borrow()
                    .as_ref()
                    .map(|props| endpoint_role(props.get("media.class")))
            })
            .unwrap_or_else(|| endpoint_role(props.get("media.class")));
        let route = if role == EndpointRole::Source {
            match nodes.get(&global.id) {
                Some(state) => source_route(state.borrow().as_ref().map(AsRef::as_ref), &devices),
                None => Err("PipeWire source node metadata is missing".into()),
            }
        } else {
            Ok(RouteAvailability::Unknown)
        };
        if let Err(error) = &route {
            snapshot.warning.get_or_insert_with(|| error.clone());
        }
        snapshot
            .endpoints
            .insert(id, EndpointMetadata { role, route });
    }
    if complete.is_ok() {
        snapshot.default_source = default_name
            .borrow()
            .as_ref()
            .and_then(|name| MicrophoneId::parse(format!("pipewire:{name}")).ok());
    }
    snapshot.warning = snapshot
        .warning
        .take()
        .or_else(|| metadata_error.borrow().clone());
    complete
}

#[cfg(test)]
mod tests;
