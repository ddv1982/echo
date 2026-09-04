use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::catalog::{ComponentId, ComponentSpec};
use super::sha256_file_cancellable;
use super::types::{InstallError, InstallPhase, InstallProgress, OperationId};

const BUFFER_SIZE: usize = 64 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);
const RECEIVE_BODY_POLL_INTERVAL: Duration = Duration::from_millis(100);
const RECEIVE_BODY_STALL_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CATALOG_ARTIFACT_BYTES: u64 = 574_041_195;
const TRANSPORT_BODY_LIMIT: u64 = MAX_CATALOG_ARTIFACT_BYTES + 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadSpec {
    pub component: ComponentId,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub installed_bytes: u64,
}

impl From<&ComponentSpec> for DownloadSpec {
    fn from(spec: &ComponentSpec) -> Self {
        Self {
            component: spec.id,
            url: spec.url.to_string(),
            size: spec.artifact_size,
            sha256: spec.artifact_sha256.to_string(),
            installed_bytes: spec.installed_bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

pub struct HttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Box<dyn Read + Send>,
}

pub trait HttpTransport: Send + Sync {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, InstallError>;

    fn body_stall_timeout(&self) -> Duration {
        RECEIVE_BODY_STALL_TIMEOUT
    }
}

pub trait DiskSpace: Send + Sync {
    fn available_bytes(&self, path: &Path) -> Result<Option<u64>, InstallError>;
}

#[derive(Default)]
pub struct SystemDisk;

impl DiskSpace for SystemDisk {
    fn available_bytes(&self, path: &Path) -> Result<Option<u64>, InstallError> {
        fs2::available_space(path)
            .map(Some)
            .map_err(InstallError::Io)
    }
}

pub struct UreqTransport {
    agent: ureq::Agent,
    body_stall_timeout: Duration,
}

impl Default for UreqTransport {
    fn default() -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .https_only(true)
            .timeout_resolve(Some(Duration::from_secs(30)))
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_recv_body(Some(RECEIVE_BODY_POLL_INTERVAL))
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build()
            .new_agent();
        Self {
            agent,
            body_stall_timeout: RECEIVE_BODY_STALL_TIMEOUT,
        }
    }
}

impl HttpTransport for UreqTransport {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, InstallError> {
        let mut builder = self.agent.get(&request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let response = builder
            .call()
            .map_err(|error| InstallError::Http(error.to_string()))?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
            })
            .collect();
        let body = response
            .into_body()
            .into_with_config()
            .limit(TRANSPORT_BODY_LIMIT)
            .reader();
        Ok(HttpResponse {
            status,
            headers,
            body: Box::new(body),
        })
    }

    fn body_stall_timeout(&self) -> Duration {
        self.body_stall_timeout
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PartialMetadata {
    component: ComponentId,
    url: String,
    expected_bytes: u64,
    expected_sha256: String,
    etag: Option<String>,
    last_modified: Option<String>,
}

#[must_use]
pub fn required_free_bytes(spec: &DownloadSpec, partial_bytes: u64) -> u64 {
    let remaining = spec.size.saturating_sub(partial_bytes);
    let working = remaining.saturating_add(spec.installed_bytes);
    let margin = (working / 10).max(256 * 1024 * 1024);
    working.saturating_add(margin)
}

fn part_paths(root: &Path, spec: &DownloadSpec) -> (PathBuf, PathBuf) {
    let stem = format!("{}-{}", spec.component.as_str(), &spec.sha256[..12]);
    let downloads = root.join("managed").join("downloads");
    (
        downloads.join(format!("{stem}.part")),
        downloads.join(format!("{stem}.json")),
    )
}

fn load_metadata(path: &Path) -> Option<PartialMetadata> {
    fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
}

fn save_metadata(path: &Path, metadata: &PartialMetadata) -> Result<(), InstallError> {
    let raw = serde_json::to_vec_pretty(metadata)
        .map_err(|error| InstallError::State(error.to_string()))?;
    echo_core::write_atomic(path, &raw).map_err(InstallError::IoMessage)
}

fn parse_content_range(value: Option<&str>) -> Option<(u64, u64)> {
    let value = value?.strip_prefix("bytes ")?;
    let (range, total) = value.split_once('/')?;
    let (start, _) = range.split_once('-')?;
    Some((start.parse().ok()?, total.parse().ok()?))
}

fn stream_body(
    body: Box<dyn Read + Send>,
    accepted_bytes: u64,
    stall_timeout: Duration,
    cancel: &AtomicBool,
    mut consume: impl FnMut(&[u8]) -> Result<(), InstallError>,
) -> Result<(), InstallError> {
    let mut body = body.take(accepted_bytes);
    let mut buffer = [0_u8; BUFFER_SIZE];
    let mut last_progress = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }
        let read = match body.read(&mut buffer) {
            Ok(read) => read,
            Err(error) => {
                if cancel.load(Ordering::Relaxed) {
                    return Err(InstallError::Cancelled);
                }
                if receive_body_poll_timed_out(&error) && last_progress.elapsed() < stall_timeout {
                    continue;
                }
                return Err(InstallError::Http(error.to_string()));
            }
        };
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }
        if read == 0 {
            return Ok(());
        }
        last_progress = Instant::now();
        consume(&buffer[..read])?;
    }
}

fn receive_body_poll_timed_out(error: &std::io::Error) -> bool {
    error.get_ref().is_some_and(|source| {
        source
            .downcast_ref::<ureq::Error>()
            .is_some_and(|error| matches!(error, ureq::Error::Timeout(ureq::Timeout::RecvBody)))
    })
}

pub fn download_verified(
    root: &Path,
    spec: &DownloadSpec,
    transport: &dyn HttpTransport,
    disk: &dyn DiskSpace,
    operation: &OperationId,
    cancel: &AtomicBool,
    mut progress: impl FnMut(InstallProgress),
) -> Result<PathBuf, InstallError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(InstallError::Cancelled);
    }
    let (part, metadata_path) = part_paths(root, spec);
    if let Some(parent) = part.parent() {
        fs::create_dir_all(parent)?;
    }
    let expected_metadata = |etag, last_modified| PartialMetadata {
        component: spec.component,
        url: spec.url.clone(),
        expected_bytes: spec.size,
        expected_sha256: spec.sha256.clone(),
        etag,
        last_modified,
    };
    let mut metadata =
        load_metadata(&metadata_path).unwrap_or_else(|| expected_metadata(None, None));
    let metadata_matches = metadata.component == spec.component
        && metadata.url == spec.url
        && metadata.expected_bytes == spec.size
        && metadata.expected_sha256 == spec.sha256;
    if !metadata_matches {
        let _ = fs::remove_file(&part);
        metadata = expected_metadata(None, None);
    }
    let mut partial_bytes = fs::metadata(&part).map(|value| value.len()).unwrap_or(0);
    if partial_bytes > spec.size {
        let _ = fs::remove_file(&part);
        partial_bytes = 0;
    }
    let required = required_free_bytes(spec, partial_bytes);
    if let Some(available) = disk.available_bytes(root)? {
        if available < required {
            return Err(InstallError::InsufficientSpace {
                required,
                available,
            });
        }
    }
    progress(InstallProgress::new(
        operation,
        spec.component,
        InstallPhase::CheckingDisk,
        partial_bytes,
        spec.size,
        partial_bytes,
    ));
    save_metadata(&metadata_path, &metadata)?;

    if partial_bytes < spec.size {
        let mut headers = BTreeMap::from([("accept-encoding".to_string(), "identity".to_string())]);
        if partial_bytes > 0 {
            headers.insert("range".to_string(), format!("bytes={partial_bytes}-"));
            if let Some(value) = metadata.etag.as_ref().or(metadata.last_modified.as_ref()) {
                headers.insert("if-range".to_string(), value.clone());
            }
        }
        let response = transport.get(&HttpRequest {
            url: spec.url.clone(),
            headers,
        })?;
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }
        let append = match response.status {
            200 => {
                partial_bytes = 0;
                false
            }
            206 => {
                let (start, total) =
                    parse_content_range(response.headers.get("content-range").map(String::as_str))
                        .ok_or_else(|| InstallError::Range("missing Content-Range".to_string()))?;
                if start != partial_bytes || total != spec.size {
                    return Err(InstallError::Range(format!(
                        "server resumed at {start} of {total}, expected {partial_bytes} of {}",
                        spec.size
                    )));
                }
                true
            }
            416 if partial_bytes == spec.size => true,
            status => return Err(InstallError::Http(format!("HTTP {status}"))),
        };
        metadata.etag = response.headers.get("etag").cloned().or(metadata.etag);
        metadata.last_modified = response
            .headers
            .get("last-modified")
            .cloned()
            .or(metadata.last_modified);
        save_metadata(&metadata_path, &metadata)?;
        if response.status != 416 {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .append(append)
                .truncate(!append)
                .open(&part)?;
            let resumed_from = partial_bytes;
            let accepted_bytes = spec.size.saturating_sub(partial_bytes).saturating_add(1);
            stream_body(
                response.body,
                accepted_bytes,
                transport.body_stall_timeout(),
                cancel,
                |bytes| {
                    partial_bytes = partial_bytes.saturating_add(bytes.len() as u64);
                    if partial_bytes > spec.size {
                        let _ = fs::remove_file(&part);
                        let _ = fs::remove_file(&metadata_path);
                        return Err(InstallError::Http(
                            "response exceeds the pinned size".to_string(),
                        ));
                    }
                    file.write_all(bytes)?;
                    progress(InstallProgress::new(
                        operation,
                        spec.component,
                        InstallPhase::Downloading,
                        partial_bytes,
                        spec.size,
                        resumed_from,
                    ));
                    Ok(())
                },
            )?;
            file.flush()?;
        }
    }
    if partial_bytes != spec.size {
        return Err(InstallError::Interrupted {
            received: partial_bytes,
            expected: spec.size,
        });
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(InstallError::Cancelled);
    }
    progress(InstallProgress::new(
        operation,
        spec.component,
        InstallPhase::Verifying,
        spec.size,
        spec.size,
        partial_bytes,
    ));
    fs::File::options().write(true).open(&part)?.sync_all()?;
    let actual = sha256_file_cancellable(&part, Some(cancel))?;
    if cancel.load(Ordering::Relaxed) {
        return Err(InstallError::Cancelled);
    }
    if actual != spec.sha256 {
        let _ = fs::remove_file(&part);
        let _ = fs::remove_file(&metadata_path);
        return Err(InstallError::Sha256Mismatch {
            expected: spec.sha256.clone(),
            actual,
        });
    }
    Ok(part)
}

pub fn forget_partial(root: &Path, spec: &DownloadSpec) {
    let (part, metadata) = part_paths(root, spec);
    let _ = fs::remove_file(part);
    let _ = fs::remove_file(metadata);
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::collections::VecDeque;
    use std::io::Cursor;
    use std::net::TcpListener;
    use std::sync::{mpsc, Arc, Mutex};

    struct FakeDisk(Option<u64>);

    impl DiskSpace for FakeDisk {
        fn available_bytes(&self, _: &Path) -> Result<Option<u64>, InstallError> {
            Ok(self.0)
        }
    }

    type FixtureResponse = (u16, BTreeMap<String, String>, Vec<u8>);

    struct FakeTransport {
        responses: Mutex<VecDeque<FixtureResponse>>,
        requests: Mutex<Vec<HttpRequest>>,
    }

    impl FakeTransport {
        fn new(responses: Vec<FixtureResponse>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl HttpTransport for FakeTransport {
        fn get(&self, request: &HttpRequest) -> Result<HttpResponse, InstallError> {
            self.requests.lock().unwrap().push(request.clone());
            let (status, headers, body) = self.responses.lock().unwrap().pop_front().unwrap();
            Ok(HttpResponse {
                status,
                headers,
                body: Box::new(Cursor::new(body)),
            })
        }
    }

    #[derive(Default)]
    struct BodyObservation {
        read_threads: Vec<std::thread::ThreadId>,
        largest_buffer: usize,
        dropped: bool,
    }

    struct ObservedBody {
        bytes: Cursor<Vec<u8>>,
        observation: Arc<Mutex<BodyObservation>>,
    }

    impl Read for ObservedBody {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let mut observation = self.observation.lock().unwrap();
            observation.read_threads.push(std::thread::current().id());
            observation.largest_buffer = observation.largest_buffer.max(buffer.len());
            drop(observation);
            self.bytes.read(buffer)
        }
    }

    impl Drop for ObservedBody {
        fn drop(&mut self) {
            self.observation.lock().unwrap().dropped = true;
        }
    }

    struct ObservedTransport {
        body: Mutex<Option<ObservedBody>>,
    }

    impl HttpTransport for ObservedTransport {
        fn get(&self, _: &HttpRequest) -> Result<HttpResponse, InstallError> {
            Ok(HttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: Box::new(self.body.lock().unwrap().take().unwrap()),
            })
        }
    }

    struct PollingTimedOutBody {
        entered: Option<mpsc::Sender<()>>,
        poll_interval: Duration,
    }

    impl Read for PollingTimedOutBody {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            if let Some(entered) = self.entered.take() {
                entered.send(()).unwrap();
            }
            std::thread::sleep(self.poll_interval);
            Err(ureq::Error::Timeout(ureq::Timeout::RecvBody).into_io())
        }
    }

    struct TimedOutBodyTransport {
        body: Mutex<Option<PollingTimedOutBody>>,
    }

    impl HttpTransport for TimedOutBodyTransport {
        fn get(&self, _: &HttpRequest) -> Result<HttpResponse, InstallError> {
            Ok(HttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: Box::new(self.body.lock().unwrap().take().unwrap()),
            })
        }
    }

    fn scratch(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "echo-install-download-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn spec(body: &[u8]) -> DownloadSpec {
        DownloadSpec {
            component: ComponentId::SileroVad,
            url: "https://fixture.invalid/artifact".to_string(),
            size: body.len() as u64,
            sha256: format!("{:x}", Sha256::digest(body)),
            installed_bytes: body.len() as u64,
        }
    }

    #[test]
    fn fresh_download_verifies_and_disk_refusal_makes_no_request() {
        let body = b"verified artifact";
        let spec = spec(body);
        let root = scratch("fresh");
        let refused = FakeTransport::new(vec![]);
        let error = download_verified(
            &root,
            &spec,
            &refused,
            &FakeDisk(Some(1)),
            &OperationId::fixture("1"),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, InstallError::InsufficientSpace { .. }));
        assert!(refused.requests.lock().unwrap().is_empty());

        let transport = FakeTransport::new(vec![(200, BTreeMap::new(), body.to_vec())]);
        let path = download_verified(
            &root,
            &spec,
            &transport,
            &FakeDisk(None),
            &OperationId::fixture("2"),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        assert_eq!(fs::read(path).unwrap(), body);
    }

    #[test]
    fn interrupted_transfer_resumes_with_range_and_if_range() {
        let body = b"resume this artifact";
        let spec = spec(body);
        let root = scratch("resume");
        let first = FakeTransport::new(vec![(
            200,
            BTreeMap::from([("etag".to_string(), "v1".to_string())]),
            body[..7].to_vec(),
        )]);
        assert!(matches!(
            download_verified(
                &root,
                &spec,
                &first,
                &FakeDisk(None),
                &OperationId::fixture("1"),
                &AtomicBool::new(false),
                |_| {}
            ),
            Err(InstallError::Interrupted { received: 7, .. })
        ));
        let second = FakeTransport::new(vec![(
            206,
            BTreeMap::from([(
                "content-range".to_string(),
                format!("bytes 7-{}/{}", body.len() - 1, body.len()),
            )]),
            body[7..].to_vec(),
        )]);
        download_verified(
            &root,
            &spec,
            &second,
            &FakeDisk(None),
            &OperationId::fixture("2"),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        let request = &second.requests.lock().unwrap()[0];
        assert_eq!(
            request.headers.get("range").map(String::as_str),
            Some("bytes=7-")
        );
        assert_eq!(
            request.headers.get("if-range").map(String::as_str),
            Some("v1")
        );
    }

    #[test]
    fn cancellation_keeps_partial_and_checksum_failure_starts_clean() {
        let body = vec![9u8; BUFFER_SIZE + 4];
        let fixture_spec = spec(&body);
        let root = scratch("cancel");
        let transport = FakeTransport::new(vec![(200, BTreeMap::new(), body.clone())]);
        let cancel = AtomicBool::new(false);
        let error = download_verified(
            &root,
            &fixture_spec,
            &transport,
            &FakeDisk(None),
            &OperationId::fixture("1"),
            &cancel,
            |progress| {
                if progress.phase == InstallPhase::Downloading {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
        )
        .unwrap_err();
        assert!(matches!(error, InstallError::Cancelled));
        let (part, _) = part_paths(&root, &fixture_spec);
        assert!(fs::metadata(&part).unwrap().len() > 0);

        let bad = spec(b"right");
        let bad_transport = FakeTransport::new(vec![(200, BTreeMap::new(), b"wrong".to_vec())]);
        let error = download_verified(
            &scratch("hash"),
            &bad,
            &bad_transport,
            &FakeDisk(None),
            &OperationId::fixture("2"),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, InstallError::Sha256Mismatch { .. }));
    }

    #[test]
    fn cancellation_interrupts_a_stalled_body_read_after_one_poll() {
        let root = scratch("cancel-during-timed-out-read");
        let fixture_spec = spec(b"expected body");
        let (entered_tx, entered_rx) = mpsc::channel();
        let transport = TimedOutBodyTransport {
            body: Mutex::new(Some(PollingTimedOutBody {
                entered: Some(entered_tx),
                poll_interval: RECEIVE_BODY_POLL_INTERVAL,
            })),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let (result_tx, result_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = download_verified(
                &root,
                &fixture_spec,
                &transport,
                &FakeDisk(None),
                &OperationId::fixture("1"),
                worker_cancel.as_ref(),
                |_| {},
            );
            result_tx.send(result).unwrap();
        });

        entered_rx.recv().unwrap();
        cancel.store(true, Ordering::Relaxed);

        let error = result_rx
            .recv_timeout(RECEIVE_BODY_POLL_INTERVAL * 3)
            .expect("cancellation was not observed after one body-read poll")
            .unwrap_err();
        worker.join().unwrap();
        assert!(
            matches!(error, InstallError::Cancelled),
            "cancellation during a stalled body read must win over its timeout error, got {error:?}"
        );
    }

    #[test]
    fn body_is_size_pinned_and_read_without_an_owned_reader_thread() {
        let root = scratch("bounded-body");
        let spec = spec(b"fit");
        let observation = Arc::new(Mutex::new(BodyObservation::default()));
        let transport = ObservedTransport {
            body: Mutex::new(Some(ObservedBody {
                bytes: Cursor::new(b"fits".to_vec()),
                observation: observation.clone(),
            })),
        };
        let caller = std::thread::current().id();
        let error = download_verified(
            &root,
            &spec,
            &transport,
            &FakeDisk(None),
            &OperationId::fixture("1"),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, InstallError::Http(message) if message.contains("pinned size")));

        let observation = observation.lock().unwrap();
        assert!(observation.dropped, "body must be dropped before return");
        assert!(observation
            .read_threads
            .iter()
            .all(|thread| thread == &caller));
        assert_eq!(observation.largest_buffer, spec.size as usize + 1);
    }

    #[test]
    fn ureq_requests_have_a_finite_total_deadline_and_catalog_body_limit() {
        let transport = UreqTransport::default();
        assert_eq!(
            transport.agent.config().timeouts().global,
            Some(REQUEST_TIMEOUT)
        );
        assert_eq!(
            transport.agent.config().timeouts().recv_body,
            Some(RECEIVE_BODY_POLL_INTERVAL)
        );
        assert_eq!(transport.body_stall_timeout, RECEIVE_BODY_STALL_TIMEOUT);
        assert!(REQUEST_TIMEOUT >= Duration::from_secs(60 * 60));
        assert_eq!(
            TRANSPORT_BODY_LIMIT,
            super::super::catalog::COMPONENTS
                .iter()
                .map(|spec| spec.artifact_size)
                .max()
                .unwrap()
                + 1
        );
    }

    #[test]
    fn stalled_response_body_times_out_before_the_server_closes() {
        const TEST_BODY_POLL_INTERVAL: Duration = Duration::from_millis(25);
        const TEST_BODY_STALL_TIMEOUT: Duration = Duration::from_millis(100);
        const SERVER_FALLBACK: Duration = Duration::from_secs(2);

        let expected_body = b"partial body that never finishes";
        let partial_body = b"partial body";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (body_started_tx, body_started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (server_closed_tx, server_closed_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 256];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                assert!(read > 0, "client closed before sending request headers");
                request.extend_from_slice(&buffer[..read]);
            }

            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                expected_body.len()
            )
            .unwrap();
            stream.write_all(partial_body).unwrap();
            stream.flush().unwrap();
            body_started_tx.send(()).unwrap();

            let _ = release_rx.recv_timeout(SERVER_FALLBACK);
            drop(stream);
            server_closed_tx.send(()).unwrap();
        });

        let transport = UreqTransport {
            agent: ureq::Agent::config_builder()
                .http_status_as_error(false)
                .https_only(false)
                .proxy(None)
                .timeout_recv_body(Some(TEST_BODY_POLL_INTERVAL))
                .timeout_global(Some(Duration::from_secs(5)))
                .build()
                .new_agent(),
            body_stall_timeout: TEST_BODY_STALL_TIMEOUT,
        };
        let mut fixture_spec = spec(expected_body);
        fixture_spec.url = format!("http://{address}/artifact");
        let root = scratch("stalled-response-body");

        let started = Instant::now();
        let result = download_verified(
            &root,
            &fixture_spec,
            &transport,
            &FakeDisk(None),
            &OperationId::fixture("1"),
            &AtomicBool::new(false),
            |_| {},
        );
        let elapsed = started.elapsed();
        let body_started = body_started_rx.recv_timeout(Duration::from_secs(1));
        let server_closed_before_release = server_closed_rx.try_recv().is_ok();
        let _ = release_tx.send(());
        server.join().unwrap();

        assert!(
            body_started.is_ok(),
            "server did not start the response body"
        );
        assert!(
            !server_closed_before_release,
            "server closed before the client returned"
        );
        assert!(
            elapsed >= TEST_BODY_STALL_TIMEOUT,
            "body timeout returned before the no-progress deadline: {elapsed:?}"
        );
        let error = result.unwrap_err();
        assert!(
            matches!(
                &error,
                InstallError::Http(message)
                    if message.contains("timeout") || message.contains("timed out")
            ),
            "expected an HTTP body timeout, got {error:?}"
        );
        let (part, _) = part_paths(&root, &fixture_spec);
        assert_eq!(fs::read(part).unwrap(), partial_body);
    }

    #[test]
    fn global_timeout_bounds_a_response_body_that_keeps_making_progress() {
        const GLOBAL_TIMEOUT: Duration = Duration::from_millis(150);
        const RECEIVE_BODY_TIMEOUT: Duration = Duration::from_secs(1);
        const CHUNK_INTERVAL: Duration = Duration::from_millis(10);
        const SERVER_FALLBACK: Duration = Duration::from_secs(3);
        const BODY_LENGTH: usize = 1024 * 1024;

        let expected_body = vec![b'x'; BODY_LENGTH];
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (body_started_tx, body_started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (server_closed_tx, server_closed_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 256];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                assert!(read > 0, "client closed before sending request headers");
                request.extend_from_slice(&buffer[..read]);
            }

            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {BODY_LENGTH}\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            stream.write_all(b"progress").unwrap();
            stream.flush().unwrap();
            body_started_tx.send(()).unwrap();

            let started = std::time::Instant::now();
            while started.elapsed() < SERVER_FALLBACK {
                match release_rx.recv_timeout(CHUNK_INTERVAL) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let _ = stream.write_all(b"progress");
                        let _ = stream.flush();
                    }
                }
            }
            drop(stream);
            let _ = server_closed_tx.send(());
        });

        let transport = UreqTransport {
            agent: ureq::Agent::config_builder()
                .http_status_as_error(false)
                .https_only(false)
                .proxy(None)
                .timeout_recv_body(Some(RECEIVE_BODY_TIMEOUT))
                .timeout_global(Some(GLOBAL_TIMEOUT))
                .build()
                .new_agent(),
            body_stall_timeout: RECEIVE_BODY_TIMEOUT,
        };
        let mut fixture_spec = spec(&expected_body);
        fixture_spec.url = format!("http://{address}/artifact");
        let root = scratch("progressing-response-global-timeout");

        let result = download_verified(
            &root,
            &fixture_spec,
            &transport,
            &FakeDisk(None),
            &OperationId::fixture("1"),
            &AtomicBool::new(false),
            |_| {},
        );
        let body_started = body_started_rx.recv_timeout(Duration::from_secs(1));
        let server_closed_before_release = server_closed_rx.try_recv().is_ok();
        let _ = release_tx.send(());
        server.join().unwrap();

        assert!(
            body_started.is_ok(),
            "server did not start the response body"
        );
        assert!(
            !server_closed_before_release,
            "server closed before the global timeout returned"
        );
        let error = result.unwrap_err();
        assert!(
            matches!(
                &error,
                InstallError::Http(message)
                    if message.to_ascii_lowercase().contains("timeout")
                        || message.to_ascii_lowercase().contains("timed out")
            ),
            "expected an HTTP global timeout while the body was progressing, got {error:?}"
        );
    }

    #[test]
    fn ignored_range_restarts_and_bad_content_range_refuses_append() {
        let body = b"range fixture";
        let spec = spec(body);
        let root = scratch("range");
        let first = FakeTransport::new(vec![(200, BTreeMap::new(), body[..3].to_vec())]);
        let _ = download_verified(
            &root,
            &spec,
            &first,
            &FakeDisk(None),
            &OperationId::fixture("1"),
            &AtomicBool::new(false),
            |_| {},
        );
        let restart = FakeTransport::new(vec![(200, BTreeMap::new(), body.to_vec())]);
        download_verified(
            &root,
            &spec,
            &restart,
            &FakeDisk(None),
            &OperationId::fixture("2"),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();

        let root = scratch("bad-range");
        let first = FakeTransport::new(vec![(200, BTreeMap::new(), body[..3].to_vec())]);
        let _ = download_verified(
            &root,
            &spec,
            &first,
            &FakeDisk(None),
            &OperationId::fixture("1"),
            &AtomicBool::new(false),
            |_| {},
        );
        let bad = FakeTransport::new(vec![(
            206,
            BTreeMap::from([(
                "content-range".to_string(),
                format!("bytes 2-{}/{}", body.len() - 1, body.len()),
            )]),
            body[3..].to_vec(),
        )]);
        assert!(matches!(
            download_verified(
                &root,
                &spec,
                &bad,
                &FakeDisk(None),
                &OperationId::fixture("2"),
                &AtomicBool::new(false),
                |_| {}
            ),
            Err(InstallError::Range(_))
        ));
    }
}
