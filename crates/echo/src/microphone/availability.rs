use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::{AudioHost, MicrophoneId};

mod pipewire;
mod pulse;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EndpointRole {
    Source,
    Playback,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RouteAvailability {
    Available,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct EndpointMetadata {
    pub role: EndpointRole,
    pub route: Result<RouteAvailability, String>,
}

impl EndpointMetadata {
    pub fn rejection(&self) -> Option<&str> {
        match (&self.role, &self.route) {
            (EndpointRole::Playback | EndpointRole::Other, _) => Some("not a microphone source"),
            (_, Ok(RouteAvailability::Unavailable)) => Some("microphone route is unavailable"),
            (_, Err(reason)) => Some(reason),
            (_, Ok(RouteAvailability::Available | RouteAvailability::Unknown)) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendHealth {
    Unreachable,
    Reachable,
    Disconnected,
}

#[derive(Debug, Clone, Serialize)]
pub struct NativeSnapshot {
    pub health: BackendHealth,
    pub endpoints: HashMap<MicrophoneId, EndpointMetadata>,
    pub default_source: Option<MicrophoneId>,
    pub warning: Option<String>,
}

impl NativeSnapshot {
    pub fn empty(health: BackendHealth) -> Self {
        Self {
            health,
            endpoints: HashMap::new(),
            default_source: None,
            warning: None,
        }
    }
}

#[derive(Default)]
struct CollectionState {
    collecting: bool,
    generation: u64,
    last: Option<Arc<NativeSnapshot>>,
    ever_reachable: bool,
}

#[derive(Default)]
struct Collector {
    state: Mutex<CollectionState>,
    completed: Condvar,
}

fn failed_snapshot(message: &str) -> NativeSnapshot {
    let mut snapshot = NativeSnapshot::empty(BackendHealth::Unreachable);
    snapshot.warning = Some(message.to_owned());
    snapshot
}

impl Collector {
    fn read(&self, collect: impl FnOnce() -> NativeSnapshot) -> Arc<NativeSnapshot> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.collecting {
            let generation = state.generation;
            while state.generation == generation {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    let mut failed = failed_snapshot("microphone metadata refresh timed out");
                    if state.ever_reachable {
                        failed.health = BackendHealth::Disconnected;
                    }
                    return Arc::new(failed);
                }
                state = self
                    .completed
                    .wait_timeout(state, remaining)
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .0;
            }
            return state
                .last
                .clone()
                .expect("completed collection publishes a snapshot");
        }
        state.collecting = true;
        drop(state);
        let mut snapshot = std::panic::catch_unwind(std::panic::AssertUnwindSafe(collect))
            .unwrap_or_else(|_| failed_snapshot("microphone metadata collection failed"));
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.ever_reachable |= snapshot.health == BackendHealth::Reachable;
        if state.ever_reachable && snapshot.health == BackendHealth::Unreachable {
            snapshot.health = BackendHealth::Disconnected;
        }
        let snapshot = Arc::new(snapshot);
        state.last = Some(snapshot.clone());
        state.generation += 1;
        state.collecting = false;
        self.completed.notify_all();
        snapshot
    }
}

pub fn snapshot(host: AudioHost) -> Option<Arc<NativeSnapshot>> {
    static PIPEWIRE: std::sync::LazyLock<Collector> = std::sync::LazyLock::new(Collector::default);
    static PULSE: std::sync::LazyLock<Collector> = std::sync::LazyLock::new(Collector::default);
    match host {
        AudioHost::PipeWire => Some(PIPEWIRE.read(pipewire::collect)),
        AudioHost::PulseAudio => Some(PULSE.read(pulse::collect)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_jack_status_is_not_a_failed_query() {
        let source = EndpointMetadata {
            role: EndpointRole::Source,
            route: Ok(RouteAvailability::Unknown),
        };
        assert_eq!(source.rejection(), None);
        let failed = EndpointMetadata {
            route: Err("route query failed".into()),
            ..source
        };
        assert_eq!(failed.rejection(), Some("route query failed"));
    }

    #[test]
    fn playback_and_unavailable_routes_are_excluded() {
        for endpoint in [
            EndpointMetadata {
                role: EndpointRole::Playback,
                route: Ok(RouteAvailability::Available),
            },
            EndpointMetadata {
                role: EndpointRole::Other,
                route: Ok(RouteAvailability::Unknown),
            },
            EndpointMetadata {
                role: EndpointRole::Source,
                route: Ok(RouteAvailability::Unavailable),
            },
        ] {
            assert!(endpoint.rejection().is_some());
        }
    }

    #[test]
    fn a_server_failure_does_not_reenable_raw_hardware_fallback() {
        let collector = Collector::default();
        assert_eq!(
            collector
                .read(|| NativeSnapshot::empty(BackendHealth::Unreachable))
                .health,
            BackendHealth::Unreachable
        );
        assert_eq!(
            collector
                .read(|| NativeSnapshot::empty(BackendHealth::Reachable))
                .health,
            BackendHealth::Reachable
        );
        assert_eq!(
            collector
                .read(|| NativeSnapshot::empty(BackendHealth::Unreachable))
                .health,
            BackendHealth::Disconnected
        );
    }

    #[test]
    fn a_failed_collection_can_be_retried() {
        let collector = Collector::default();
        let failed = collector.read(|| panic!("simulated backend panic"));
        assert!(failed.warning.is_some());
        let recovered = collector.read(|| NativeSnapshot::empty(BackendHealth::Reachable));
        assert!(recovered.warning.is_none());
    }
}
