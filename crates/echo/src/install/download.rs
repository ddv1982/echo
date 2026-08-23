use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::catalog::{ComponentId, ComponentSpec};
use super::{InstallError, InstallPhase, InstallProgress, OperationId};

const BUFFER_SIZE: usize = 64 * 1024;

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
}

impl Default for UreqTransport {
    fn default() -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .new_agent();
        Self { agent }
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
            .limit(u64::MAX)
            .reader();
        Ok(HttpResponse {
            status,
            headers,
            body: Box::new(body),
        })
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
    let raw = serde_json::to_vec_pretty(metadata).map_err(|error| InstallError::State(error.to_string()))?;
    echo_core::write_atomic(path, &raw).map_err(InstallError::IoMessage)
}

fn sha256_file(path: &Path) -> Result<String, InstallError> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; BUFFER_SIZE];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn parse_content_range(value: Option<&str>) -> Option<(u64, u64)> {
    let value = value?.strip_prefix("bytes ")?;
    let (range, total) = value.split_once('/')?;
    let (start, _) = range.split_once('-')?;
    Some((start.parse().ok()?, total.parse().ok()?))
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
    let mut metadata = load_metadata(&metadata_path).unwrap_or_else(|| expected_metadata(None, None));
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
            return Err(InstallError::InsufficientSpace { required, available });
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
        let append = match response.status {
            200 => {
                partial_bytes = 0;
                false
            }
            206 => {
                let (start, total) = parse_content_range(
                    response.headers.get("content-range").map(String::as_str),
                )
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
            let mut body = response.body;
            let resumed_from = partial_bytes;
            let mut buffer = [0u8; BUFFER_SIZE];
            loop {
                if cancel.load(Ordering::Relaxed) {
                    file.flush()?;
                    return Err(InstallError::Cancelled);
                }
                let read = body.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                partial_bytes = partial_bytes.saturating_add(read as u64);
                if partial_bytes > spec.size {
                    let _ = fs::remove_file(&part);
                    let _ = fs::remove_file(&metadata_path);
                    return Err(InstallError::Http("response exceeds the pinned size".to_string()));
                }
                file.write_all(&buffer[..read])?;
                progress(InstallProgress::new(
                    operation,
                    spec.component,
                    InstallPhase::Downloading,
                    partial_bytes,
                    spec.size,
                    resumed_from,
                ));
            }
            file.flush()?;
        }
    }
    if partial_bytes != spec.size {
        return Err(InstallError::Interrupted {
            received: partial_bytes,
            expected: spec.size,
        });
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
    let actual = sha256_file(&part)?;
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
    use std::collections::VecDeque;
    use std::io::Cursor;
    use std::sync::Mutex;

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

    fn scratch(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("echo-install-download-{label}-{}", std::process::id()));
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
        let error = download_verified(&root, &spec, &refused, &FakeDisk(Some(1)), &OperationId::fixture("1"), &AtomicBool::new(false), |_| {}).unwrap_err();
        assert!(matches!(error, InstallError::InsufficientSpace { .. }));
        assert!(refused.requests.lock().unwrap().is_empty());

        let transport = FakeTransport::new(vec![(200, BTreeMap::new(), body.to_vec())]);
        let path = download_verified(&root, &spec, &transport, &FakeDisk(None), &OperationId::fixture("2"), &AtomicBool::new(false), |_| {}).unwrap();
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
            download_verified(&root, &spec, &first, &FakeDisk(None), &OperationId::fixture("1"), &AtomicBool::new(false), |_| {}),
            Err(InstallError::Interrupted { received: 7, .. })
        ));
        let second = FakeTransport::new(vec![(
            206,
            BTreeMap::from([("content-range".to_string(), format!("bytes 7-{}/{}", body.len() - 1, body.len()))]),
            body[7..].to_vec(),
        )]);
        download_verified(&root, &spec, &second, &FakeDisk(None), &OperationId::fixture("2"), &AtomicBool::new(false), |_| {}).unwrap();
        let request = &second.requests.lock().unwrap()[0];
        assert_eq!(request.headers.get("range").map(String::as_str), Some("bytes=7-"));
        assert_eq!(request.headers.get("if-range").map(String::as_str), Some("v1"));
    }

    #[test]
    fn cancellation_keeps_partial_and_checksum_failure_starts_clean() {
        let body = vec![9u8; BUFFER_SIZE + 4];
        let fixture_spec = spec(&body);
        let root = scratch("cancel");
        let transport = FakeTransport::new(vec![(200, BTreeMap::new(), body.clone())]);
        let cancel = AtomicBool::new(false);
        let error = download_verified(&root, &fixture_spec, &transport, &FakeDisk(None), &OperationId::fixture("1"), &cancel, |progress| {
            if progress.phase == InstallPhase::Downloading {
                cancel.store(true, Ordering::Relaxed);
            }
        }).unwrap_err();
        assert!(matches!(error, InstallError::Cancelled));
        let (part, _) = part_paths(&root, &fixture_spec);
        assert!(fs::metadata(&part).unwrap().len() > 0);

        let bad = spec(b"right");
        let bad_transport = FakeTransport::new(vec![(200, BTreeMap::new(), b"wrong".to_vec())]);
        let error = download_verified(&scratch("hash"), &bad, &bad_transport, &FakeDisk(None), &OperationId::fixture("2"), &AtomicBool::new(false), |_| {}).unwrap_err();
        assert!(matches!(error, InstallError::Sha256Mismatch { .. }));
    }

    #[test]
    fn ignored_range_restarts_and_bad_content_range_refuses_append() {
        let body = b"range fixture";
        let spec = spec(body);
        let root = scratch("range");
        let first = FakeTransport::new(vec![(200, BTreeMap::new(), body[..3].to_vec())]);
        let _ = download_verified(&root, &spec, &first, &FakeDisk(None), &OperationId::fixture("1"), &AtomicBool::new(false), |_| {});
        let restart = FakeTransport::new(vec![(200, BTreeMap::new(), body.to_vec())]);
        download_verified(&root, &spec, &restart, &FakeDisk(None), &OperationId::fixture("2"), &AtomicBool::new(false), |_| {}).unwrap();

        let root = scratch("bad-range");
        let first = FakeTransport::new(vec![(200, BTreeMap::new(), body[..3].to_vec())]);
        let _ = download_verified(&root, &spec, &first, &FakeDisk(None), &OperationId::fixture("1"), &AtomicBool::new(false), |_| {});
        let bad = FakeTransport::new(vec![(206, BTreeMap::from([("content-range".to_string(), format!("bytes 2-{}/{}", body.len() - 1, body.len()))]), body[3..].to_vec())]);
        assert!(matches!(download_verified(&root, &spec, &bad, &FakeDisk(None), &OperationId::fixture("2"), &AtomicBool::new(false), |_| {}), Err(InstallError::Range(_))));
    }
}
