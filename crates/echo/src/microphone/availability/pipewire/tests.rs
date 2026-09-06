use super::*;
use pw::spa::{pod::Property, utils::Id};

fn route(device: Option<i32>, devices: &[i32], availability: RouteAvailability) -> Route {
    Route {
        direction: Direction::Input,
        device,
        devices: devices.to_vec(),
        availability,
    }
}

fn routes(active: Vec<Route>, candidates: Vec<Route>) -> Routes {
    Routes {
        info_received: true,
        active,
        candidates,
        ..Routes::default()
    }
}

fn object(mut properties: Vec<Property>) -> Value {
    properties.insert(
        0,
        Property::new(
            sys::SPA_PARAM_ROUTE_direction,
            Value::Id(Id(sys::SPA_DIRECTION_INPUT)),
        ),
    );
    Value::Object(Object {
        type_: sys::SPA_TYPE_OBJECT_ParamRoute,
        id: ParamType::Route.as_raw(),
        properties,
    })
}

#[test]
fn laptop_mic2_is_unavailable_and_mic1_has_unknown_jack_status() {
    let metadata = routes(
        vec![
            route(Some(3), &[3], RouteAvailability::Unknown),
            route(Some(5), &[5], RouteAvailability::Unknown),
        ],
        vec![
            route(None, &[4], RouteAvailability::Unavailable),
            route(None, &[5], RouteAvailability::Unknown),
        ],
    );
    assert_eq!(
        route_availability(&metadata, 4),
        Ok(RouteAvailability::Unavailable)
    );
    assert_eq!(
        route_availability(&metadata, 5),
        Ok(RouteAvailability::Unknown)
    );
}

#[test]
fn active_route_takes_precedence_over_candidate_routes() {
    for (active, candidate) in [
        (RouteAvailability::Unavailable, RouteAvailability::Available),
        (RouteAvailability::Unknown, RouteAvailability::Unavailable),
        (RouteAvailability::Available, RouteAvailability::Unavailable),
    ] {
        let metadata = routes(
            vec![route(Some(4), &[4], active)],
            vec![route(None, &[4], candidate)],
        );
        assert_eq!(route_availability(&metadata, 4), Ok(active));
    }
}

#[test]
fn candidates_are_unavailable_only_when_every_matching_route_says_no() {
    let mut metadata = routes(
        Vec::new(),
        vec![
            route(None, &[3, 4], RouteAvailability::Unavailable),
            route(None, &[4], RouteAvailability::Unavailable),
        ],
    );
    assert_eq!(
        route_availability(&metadata, 4),
        Ok(RouteAvailability::Unavailable)
    );
    metadata
        .candidates
        .push(route(None, &[4], RouteAvailability::Unknown));
    assert_eq!(
        route_availability(&metadata, 4),
        Ok(RouteAvailability::Unknown)
    );
    metadata
        .candidates
        .push(route(None, &[4], RouteAvailability::Available));
    assert_eq!(
        route_availability(&metadata, 4),
        Ok(RouteAvailability::Available)
    );
}

#[test]
fn missing_routes_and_profile_changes_do_not_reuse_a_different_device() {
    let metadata = routes(
        vec![route(Some(5), &[5], RouteAvailability::Unavailable)],
        Vec::new(),
    );
    assert_eq!(
        route_availability(&metadata, 4),
        Ok(RouteAvailability::Unknown)
    );
    assert_eq!(
        route_availability(&routes(Vec::new(), Vec::new()), 4),
        Ok(RouteAvailability::Unknown)
    );
}

#[test]
fn failed_and_missing_device_information_is_not_unknown_availability() {
    assert!(route_availability(&Routes::default(), 4).is_err());
    let mut metadata = routes(Vec::new(), Vec::new());
    metadata.error = Some("query timed out".into());
    assert_eq!(
        route_availability(&metadata, 4),
        Err("query timed out".into())
    );
}

#[test]
fn route_parser_joins_profile_device_not_route_index() {
    let parsed = parse_route(object(vec![
        Property::new(sys::SPA_PARAM_ROUTE_index, Value::Int(6)),
        Property::new(sys::SPA_PARAM_ROUTE_device, Value::Int(5)),
        Property::new(
            sys::SPA_PARAM_ROUTE_devices,
            Value::ValueArray(ValueArray::Int(vec![5])),
        ),
        Property::new(
            sys::SPA_PARAM_ROUTE_available,
            Value::Id(Id(sys::SPA_PARAM_AVAILABILITY_unknown)),
        ),
    ]))
    .unwrap();
    assert_eq!(parsed.device, Some(5));
    assert_eq!(parsed.devices, vec![5]);
    assert_eq!(parsed.availability, RouteAvailability::Unknown);
}

#[test]
fn malformed_route_fields_are_rejected() {
    for property in [
        Property::new(sys::SPA_PARAM_ROUTE_direction, Value::Id(Id(99))),
        Property::new(sys::SPA_PARAM_ROUTE_direction, Value::Int(0)),
        Property::new(sys::SPA_PARAM_ROUTE_device, Value::String("4".into())),
        Property::new(sys::SPA_PARAM_ROUTE_device, Value::Int(-1)),
        Property::new(
            sys::SPA_PARAM_ROUTE_devices,
            Value::ValueArray(ValueArray::Int(vec![-1])),
        ),
        Property::new(sys::SPA_PARAM_ROUTE_available, Value::Id(Id(99))),
        Property::new(sys::SPA_PARAM_ROUTE_available, Value::Int(1)),
    ] {
        assert!(parse_route(object(vec![property])).is_err());
    }
    assert!(parse_route(Value::Int(4)).is_err());
}

#[test]
fn virtual_sources_without_hardware_routes_remain_supported() {
    for class in ["Audio/Source", "Audio/Source/Virtual"] {
        let props = pw::properties::properties! { "media.class" => class, "node.name" => "virtual-microphone", "node.virtual" => "true" };
        assert_eq!(
            source_route(Some(props.as_ref()), &HashMap::new()),
            Ok(RouteAvailability::Unknown)
        );
    }
}

#[test]
fn invalid_and_missing_hardware_joins_do_not_claim_unknown_jack_status() {
    for props in [
        pw::properties::properties! { "device.id" => "not-an-id" },
        pw::properties::properties! { "device.id" => "50", "card.profile.device" => "4" },
        pw::properties::properties! { "card.profile.device" => "4" },
        pw::properties::properties! { "media.class" => "Audio/Source", "device.api" => "alsa" },
        pw::properties::properties! { "media.class" => "Audio/Source", "device.api" => "bluez5" },
    ] {
        assert!(source_route(Some(props.as_ref()), &HashMap::new()).is_err());
    }
    let devices = HashMap::from([(50, Rc::new(RefCell::new(routes(Vec::new(), Vec::new()))))]);
    let props =
        pw::properties::properties! { "device.id" => "50", "card.profile.device" => "invalid" };
    assert!(source_route(Some(props.as_ref()), &devices).is_err());
}

#[test]
fn missing_bound_node_info_is_not_unknown_jack_detection() {
    assert!(source_route(None, &HashMap::new()).is_err());
    assert!(parse_route(object(Vec::new())).is_err());
}

#[test]
fn stalled_handshakes_time_out_and_close_every_connection() {
    use std::{io::Read, os::unix::net::UnixStream};
    pw::init();
    for _ in 0..100 {
        let (client, mut server) = UnixStream::pair().unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mainloop = pw::main_loop::MainLoopRc::new(None).unwrap();
        let context = pw::context::ContextRc::new(&mainloop, None).unwrap();
        let core = context.connect_fd_rc(client.into(), None).unwrap();
        let started = Instant::now();
        let result = roundtrip(
            &core,
            &mainloop,
            started + Duration::from_millis(2),
            &RefCell::new(None),
        );
        assert!(result.unwrap_err().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(core);
        drop(context);
        drop(mainloop);
        server.read_to_end(&mut Vec::new()).unwrap();
    }
}

#[test]
fn source_role_includes_duplex_and_internal_capture_nodes() {
    for class in [
        "Audio/Source",
        "Audio/Source/Virtual",
        "Audio/Source/Internal",
        "Audio/Duplex",
        "Audio/Duplex/Internal",
    ] {
        assert_eq!(endpoint_role(Some(class)), EndpointRole::Source, "{class}");
    }
    for class in ["Audio/Sink", "Audio/Sink/Internal"] {
        assert_eq!(
            endpoint_role(Some(class)),
            EndpointRole::Playback,
            "{class}"
        );
    }
    for class in [
        "Stream/Input/Audio",
        "Stream/Output/Audio",
        "Video/Source",
        "Audio/Device",
    ] {
        assert_eq!(endpoint_role(Some(class)), EndpointRole::Other, "{class}");
    }
    assert_eq!(endpoint_role(None), EndpointRole::Other);
}

#[test]
fn duplex_input_availability_does_not_follow_output_routes() {
    let mut output = route(Some(4), &[4], RouteAvailability::Available);
    output.direction = Direction::Output;
    let metadata = routes(
        vec![output, route(Some(4), &[4], RouteAvailability::Unavailable)],
        Vec::new(),
    );
    assert_eq!(
        route_availability(&metadata, 4),
        Ok(RouteAvailability::Unavailable)
    );
    let mut output = route(None, &[4], RouteAvailability::Available);
    output.direction = Direction::Output;
    let metadata = routes(
        Vec::new(),
        vec![output, route(None, &[4], RouteAvailability::Unavailable)],
    );
    assert_eq!(
        route_availability(&metadata, 4),
        Ok(RouteAvailability::Unavailable)
    );
}

#[test]
fn partial_route_failure_preserves_known_active_negative_and_rejects_uncertainty() {
    let mut metadata = routes(
        vec![
            route(Some(4), &[4], RouteAvailability::Unavailable),
            route(Some(5), &[5], RouteAvailability::Available),
        ],
        vec![route(None, &[6], RouteAvailability::Unavailable)],
    );
    metadata.error = Some("another route could not be decoded".into());
    assert_eq!(
        route_availability(&metadata, 4),
        Ok(RouteAvailability::Unavailable)
    );
    assert!(route_availability(&metadata, 5).is_err());
    assert!(route_availability(&metadata, 6).is_err());
    assert!(route_availability(&metadata, 7).is_err());
    let devices = HashMap::from([(50, Rc::new(RefCell::new(metadata)))]);
    let props = pw::properties::properties! { "device.id" => "50", "card.profile.device" => "4" };
    assert_eq!(
        source_route(Some(props.as_ref()), &devices),
        Ok(RouteAvailability::Unavailable)
    );
}
