use super::*;
use protocol::port_info::{PortDirection, PortInfo, PortType};

fn source(availability: &[PortAvailable], active_port: usize) -> SourceInfo {
    SourceInfo {
        name: c"microphone".into(),
        active_port,
        ports: availability
            .iter()
            .enumerate()
            .map(|(index, available)| PortInfo {
                name: std::ffi::CString::new(format!("port-{index}")).unwrap(),
                port_type: PortType::Mic,
                description: None,
                dir: PortDirection::Output,
                priority: 0,
                available: *available,
                availability_group: None,
            })
            .collect(),
        ..SourceInfo::default()
    }
}

#[test]
fn source_inventory_classifies_ports_independently_of_parser_direction() {
    let endpoint = source_metadata(&source(&[PortAvailable::Unknown], 0));
    assert_eq!(endpoint.role, EndpointRole::Source);
    assert_eq!(endpoint.route, Ok(RouteAvailability::Unknown));
    assert!(endpoint.rejection().is_none());
}

#[test]
fn unavailable_headset_is_excluded_but_digital_mic_without_jack_detection_remains() {
    assert_eq!(
        source_metadata(&source(&[PortAvailable::No], 0)).route,
        Ok(RouteAvailability::Unavailable)
    );
    assert_eq!(
        source_metadata(&source(&[PortAvailable::Unknown], 0)).route,
        Ok(RouteAvailability::Unknown)
    );
}

#[test]
fn missing_or_ambiguous_active_port_is_unknown_unless_every_route_is_unavailable() {
    for source in [
        source(&[], 0),
        source(&[PortAvailable::No, PortAvailable::Yes], 0),
        source(&[PortAvailable::Yes, PortAvailable::No], 0),
        source(&[PortAvailable::Yes], 9),
    ] {
        assert_eq!(
            source_metadata(&source).route,
            Ok(RouteAvailability::Unknown)
        );
    }
    assert_eq!(
        source_metadata(&source(&[PortAvailable::No, PortAvailable::No], 0)).route,
        Ok(RouteAvailability::Unavailable)
    );
}

#[test]
fn unambiguous_active_port_takes_precedence() {
    assert_eq!(
        source_metadata(&source(&[PortAvailable::No, PortAvailable::Yes], 1)).route,
        Ok(RouteAvailability::Available)
    );
    assert_eq!(
        source_metadata(&source(&[PortAvailable::Yes, PortAvailable::No], 1)).route,
        Ok(RouteAvailability::Unavailable)
    );
}

#[test]
fn monitors_and_null_generators_are_excluded_and_muted_virtual_sources_remain() {
    let monitor = SourceInfo {
        monitor_of_sink_index: Some(0),
        ..source(&[], 0)
    };
    assert_eq!(source_metadata(&monitor).role, EndpointRole::Playback);
    let null = SourceInfo {
        driver: Some(c"module-null-source.c".into()),
        ..source(&[], 0)
    };
    assert_eq!(source_metadata(&null).role, EndpointRole::Other);
    let virtual_mic = SourceInfo {
        muted: true,
        state: protocol::SourceState::Suspended,
        ..source(&[], 0)
    };
    assert_eq!(source_metadata(&virtual_mic).role, EndpointRole::Source);
    assert_eq!(source_metadata(&virtual_mic).rejection(), None);
}

#[test]
fn repeated_stalled_handshakes_close_the_socket_without_background_workers() {
    let started = Instant::now();
    for _ in 0..100 {
        let (client, mut server) = UnixStream::pair().unwrap();
        server
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let snapshot = collect_connected(
            client,
            Vec::new(),
            Instant::now() + Duration::from_millis(2),
        );
        assert_eq!(snapshot.health, BackendHealth::Reachable);
        assert!(snapshot.warning.is_some());
        let mut bytes = Vec::new();
        server.read_to_end(&mut bytes).unwrap();
        assert!(!bytes.is_empty());
    }
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn handshake_deadline_does_not_restart_when_bytes_keep_arriving() {
    let (client, mut server) = UnixStream::pair().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..20 {
            if server.write_all(&[0]).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let started = Instant::now();
    let snapshot = collect_connected(client, Vec::new(), started + Duration::from_millis(35));
    assert!(snapshot.warning.is_some());
    assert!(started.elapsed() < Duration::from_millis(150));
    server.join().unwrap();
}

fn inventory_reply(sources: Vec<SourceInfo>, info: Option<protocol::ServerInfo>) -> NativeSnapshot {
    let (client, server) = UnixStream::pair().unwrap();
    let server = std::thread::spawn(move || {
        let version = 24;
        let mut socket = BufReader::new(server);
        let (sequence, _) =
            protocol::read_command_message(&mut socket, protocol::MAX_VERSION).unwrap();
        protocol::write_reply_message(
            socket.get_mut(),
            sequence,
            &protocol::AuthReply {
                version,
                use_shm: false,
                use_memfd: false,
            },
            protocol::MAX_VERSION,
        )
        .unwrap();
        let (sequence, _) = protocol::read_command_message(&mut socket, version).unwrap();
        protocol::write_reply_message(
            socket.get_mut(),
            sequence,
            &protocol::SetClientNameReply { client_id: 1 },
            version,
        )
        .unwrap();
        let (sequence, _) = protocol::read_command_message(&mut socket, version).unwrap();
        protocol::write_reply_message(socket.get_mut(), sequence, &sources, version).unwrap();
        let (sequence, command) = protocol::read_command_message(&mut socket, version).unwrap();
        assert_eq!(command, Command::GetServerInfo);
        if let Some(info) = info {
            protocol::write_reply_message(socket.get_mut(), sequence, &info, version).unwrap();
        }
    });
    let snapshot = collect_connected(client, Vec::new(), Instant::now() + QUERY_BUDGET);
    server.join().unwrap();
    snapshot
}

#[test]
fn partial_source_inventory_survives_failure_reading_the_default() {
    let snapshot = inventory_reply(vec![source(&[PortAvailable::No], 0)], None);
    assert_eq!(snapshot.health, BackendHealth::Reachable);
    assert!(snapshot.warning.is_some());
    assert_eq!(
        snapshot.endpoints[&microphone_id(c"microphone").unwrap()].route,
        Ok(RouteAvailability::Unavailable)
    );
}

#[test]
fn malformed_sources_do_not_hide_later_sources_or_skip_the_default_query() {
    for name in [c"", c"\xff"] {
        let malformed = SourceInfo {
            name: name.into(),
            ..source(&[], 0)
        };
        let snapshot = inventory_reply(
            vec![malformed, source(&[PortAvailable::Unknown], 0)],
            Some(protocol::ServerInfo {
                default_source_name: Some(c"microphone".into()),
                ..protocol::ServerInfo::default()
            }),
        );
        let valid_id = microphone_id(c"microphone").unwrap();
        assert_eq!(snapshot.endpoints.len(), 1);
        assert_eq!(
            snapshot.endpoints[&valid_id].route,
            Ok(RouteAvailability::Unknown)
        );
        assert_eq!(snapshot.default_source, Some(valid_id));
        assert_eq!(snapshot.warning, Some(microphone_id(name).unwrap_err()));
    }
}

#[test]
fn source_names_must_be_present_and_valid_utf8() {
    assert!(microphone_id(c"").is_err());
    assert!(microphone_id(c"\xff").is_err());
}
