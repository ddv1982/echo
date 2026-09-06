use std::ffi::CStr;
use std::io::{self, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use pulseaudio::protocol::{self, port_info::PortAvailable, Command, SourceInfo};

use super::{
    BackendHealth, EndpointMetadata, EndpointRole, MicrophoneId, NativeSnapshot, RouteAvailability,
};

const QUERY_BUDGET: Duration = Duration::from_secs(1);

pub(super) fn collect() -> NativeSnapshot {
    let deadline = Instant::now() + QUERY_BUDGET;
    let Some(path) = pulseaudio::socket_path_from_env() else {
        return failed(
            BackendHealth::Unreachable,
            "PulseAudio server socket is unavailable".into(),
        );
    };
    let socket = match connect(&path, deadline) {
        Ok(socket) => socket,
        Err(error) => return failed(BackendHealth::Unreachable, error.to_string()),
    };
    let cookie = pulseaudio::cookie_path_from_env()
        .and_then(|path| std::fs::read(path).ok())
        .unwrap_or_default();
    collect_connected(socket, cookie, deadline)
}

fn failed(health: BackendHealth, warning: String) -> NativeSnapshot {
    NativeSnapshot {
        warning: Some(warning),
        ..NativeSnapshot::empty(health)
    }
}

fn connect(path: &Path, deadline: Instant) -> io::Result<UnixStream> {
    use rustix::net::{AddressFamily, SocketAddrUnix, SocketFlags, SocketType};
    let socket = rustix::net::socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
        None,
    )?;
    let address = SocketAddrUnix::new(path)?;
    loop {
        remaining(deadline)?;
        match rustix::net::connect(&socket, &address) {
            Ok(()) => break,
            Err(rustix::io::Errno::AGAIN) => {
                std::thread::sleep(remaining(deadline)?.min(Duration::from_millis(5)));
            }
            Err(rustix::io::Errno::INPROGRESS) => {
                let timeout = rustix::event::Timespec::try_from(remaining(deadline)?)
                    .map_err(io::Error::other)?;
                let mut events = [rustix::event::PollFd::new(
                    &socket,
                    rustix::event::PollFlags::OUT,
                )];
                if rustix::event::poll(&mut events, Some(&timeout))? == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "PulseAudio connection timed out",
                    ));
                }
                break;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let socket = UnixStream::from(socket);
    if let Some(error) = socket.take_error()? {
        return Err(error);
    }
    socket.set_nonblocking(false)?;
    Ok(socket)
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "PulseAudio metadata query timed out",
            )
        })
}

struct DeadlineSocket {
    socket: UnixStream,
    deadline: Instant,
}

impl Read for DeadlineSocket {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.socket
            .set_read_timeout(Some(remaining(self.deadline)?))?;
        self.socket.read(bytes)
    }
}

impl Write for DeadlineSocket {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.socket
            .set_write_timeout(Some(remaining(self.deadline)?))?;
        self.socket.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn roundtrip<T: protocol::CommandReply>(
    socket: &mut BufReader<DeadlineSocket>,
    command: Command,
    sequence: u32,
    version: u16,
) -> Result<T, String> {
    remaining(socket.get_ref().deadline).map_err(|error| error.to_string())?;
    protocol::write_command_message(socket.get_mut(), sequence, &command, version)
        .map_err(|error| format!("PulseAudio request failed: {error}"))?;
    let (reply_sequence, reply) = protocol::read_reply_message(socket, version)
        .map_err(|error| format!("PulseAudio reply failed: {error}"))?;
    if reply_sequence != sequence {
        return Err("PulseAudio reply sequence did not match the request".into());
    }
    Ok(reply)
}

fn collect_connected(socket: UnixStream, cookie: Vec<u8>, deadline: Instant) -> NativeSnapshot {
    let mut snapshot = NativeSnapshot::empty(BackendHealth::Reachable);
    let mut socket = BufReader::new(DeadlineSocket { socket, deadline });
    let result = (|| -> Result<(), String> {
        let auth: protocol::AuthReply = roundtrip(
            &mut socket,
            Command::Auth(protocol::AuthParams {
                version: protocol::MAX_VERSION,
                supports_shm: false,
                supports_memfd: false,
                cookie,
            }),
            0,
            protocol::MAX_VERSION,
        )?;
        let version = auth.version.min(protocol::MAX_VERSION);
        if version < protocol::MIN_VERSION {
            return Err("PulseAudio protocol version is unsupported".into());
        }
        let mut props = protocol::Props::new();
        props.set(
            protocol::Prop::ApplicationName,
            c"Echo microphone inventory",
        );
        let _: protocol::SetClientNameReply =
            roundtrip(&mut socket, Command::SetClientName(props), 1, version)?;
        let sources: protocol::SourceInfoList =
            roundtrip(&mut socket, Command::GetSourceInfoList, 2, version)?;
        for source in sources {
            match microphone_id(&source.name) {
                Ok(id) => {
                    snapshot.endpoints.insert(id, source_metadata(&source));
                }
                Err(error) => {
                    snapshot.warning.get_or_insert(error);
                }
            }
        }
        let info: protocol::ServerInfo =
            roundtrip(&mut socket, Command::GetServerInfo, 3, version)?;
        snapshot.default_source = info
            .default_source_name
            .as_deref()
            .map(microphone_id)
            .transpose()?;
        Ok(())
    })();
    if let Err(error) = result {
        snapshot.warning.get_or_insert(error);
    }
    snapshot
}

fn microphone_id(name: &CStr) -> Result<MicrophoneId, String> {
    let name = name
        .to_str()
        .map_err(|_| "PulseAudio source name is not UTF-8")?;
    if name.is_empty() {
        return Err("PulseAudio source name is empty".into());
    }
    MicrophoneId::parse(format!("pulseaudio:{name}"))
}

fn source_metadata(source: &SourceInfo) -> EndpointMetadata {
    let role = if source.monitor_of_sink_index.is_some() {
        EndpointRole::Playback
    } else if source.driver.as_deref() == Some(c"module-null-source.c") {
        EndpointRole::Other
    } else {
        EndpointRole::Source
    };
    // pulseaudio 0.3.1 substitutes index zero for an absent or unmatched active-port name.
    let port = if source.active_port > 0 || source.ports.len() == 1 {
        source.ports.get(source.active_port)
    } else {
        None
    };
    let route = match port.map(|port| port.available) {
        Some(PortAvailable::No) => RouteAvailability::Unavailable,
        Some(PortAvailable::Yes) => RouteAvailability::Available,
        Some(PortAvailable::Unknown) => RouteAvailability::Unknown,
        None if !source.ports.is_empty()
            && source
                .ports
                .iter()
                .all(|port| port.available == PortAvailable::No) =>
        {
            RouteAvailability::Unavailable
        }
        None => RouteAvailability::Unknown,
    };
    EndpointMetadata {
        role,
        route: Ok(route),
    }
}

#[cfg(test)]
mod tests;
