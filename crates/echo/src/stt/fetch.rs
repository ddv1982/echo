use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use sha1::{Digest, Sha1};

/// A curated offer, not the full catalog. Anyone wanting a different model
/// drops it in the directory and the scanner finds it. Sizes and hashes are
/// the published ones from huggingface.co/ggerganov/whisper.cpp and
/// ggml-org/whisper-vad, verified August 2026; the silero hash was measured
/// on the published file because upstream publishes no SHA-1 for it.
pub struct ModelOffer {
    pub id: &'static str,
    pub label: &'static str,
    pub filename: &'static str,
    pub url: &'static str,
    pub sha1: &'static str,
    pub size_bytes: u64,
    /// Resident memory while inferring, where upstream publishes a figure.
    pub runtime_mb: Option<u32>,
    pub multilingual: bool,
}

pub const OFFERS: &[ModelOffer] = &[
    ModelOffer {
        id: "base-en-q5_1",
        label: "Fast, English",
        filename: "ggml-base.en-q5_1.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en-q5_1.bin",
        sha1: "d26d7ce5a1b6e57bea5d0431b9c20ae49423c94a",
        size_bytes: 59_721_011,
        runtime_mb: Some(388),
        multilingual: false,
    },
    ModelOffer {
        id: "small",
        label: "Balanced, multilingual",
        filename: "ggml-small.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        sha1: "55356645c2b361a969dfd0ef2c5a50d530afd8d5",
        size_bytes: 487_601_967,
        runtime_mb: Some(852),
        multilingual: true,
    },
    ModelOffer {
        id: "large-v3-turbo-q5_0",
        label: "Best, multilingual",
        filename: "ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        sha1: "e050f7970618a659205450ad97eb95a18d69c9ee",
        size_bytes: 574_041_195,
        runtime_mb: None,
        multilingual: true,
    },
    ModelOffer {
        id: "silero-vad",
        label: "Silence detection",
        filename: "ggml-silero-v6.2.0.bin",
        url: "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin",
        sha1: "470e5d9d094ddba2f0a512cecc3732a252188abd",
        size_bytes: 885_098,
        runtime_mb: None,
        multilingual: false,
    },
];

#[must_use]
pub fn offer(id: &str) -> Option<&'static ModelOffer> {
    OFFERS.iter().find(|offer| offer.id == id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadStage {
    Downloading,
    /// A 547 MiB file's hash check is not instant; a bar parked at 100% with
    /// no state change reads as hung.
    Verifying,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadProgress {
    pub received: u64,
    pub total: u64,
    pub stage: DownloadStage,
}

#[derive(Debug)]
pub enum FetchError {
    Http(String),
    Io(io::Error),
    HashMismatch { expected: String, actual: String },
    Cancelled,
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(detail) => write!(f, "download failed: {detail}"),
            Self::Io(err) => write!(f, "disk error: {err}"),
            Self::HashMismatch { expected, actual } => write!(
                f,
                "integrity check failed: expected sha1 {expected}, got {actual}"
            ),
            Self::Cancelled => f.write_str("download cancelled"),
        }
    }
}

impl std::error::Error for FetchError {}

impl From<io::Error> for FetchError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// Download an offer into `dir`, verifying the SHA-1 before the file lands
/// under its final name. A completed download is a no-op. A partial or
/// corrupt temp file is deleted, never left where the scanner could find it.
pub fn download(
    offer: &ModelOffer,
    dir: &Path,
    mut progress: impl FnMut(DownloadProgress),
    cancel: &AtomicBool,
) -> Result<PathBuf, FetchError> {
    let dest = dir.join(offer.filename);
    if dest.is_file()
        && fs::metadata(&dest).map(|meta| meta.len()).unwrap_or(0) == offer.size_bytes
        && sha1_file(&dest)? == offer.sha1
    {
        progress(DownloadProgress {
            received: offer.size_bytes,
            total: offer.size_bytes,
            stage: DownloadStage::Done,
        });
        return Ok(dest);
    }
    fs::create_dir_all(dir)?;
    let temp = dir.join(format!("{}.part-{}", offer.filename, std::process::id()));
    let outcome = stream_to_temp(offer, &temp, &mut progress, cancel)
        .and_then(|()| verify_and_rename(offer, &temp, &dest, &mut progress));
    if outcome.is_err() {
        let _ = fs::remove_file(&temp);
    }
    outcome.map(|()| dest)
}

fn stream_to_temp(
    offer: &ModelOffer,
    temp: &Path,
    progress: &mut impl FnMut(DownloadProgress),
    cancel: &AtomicBool,
) -> Result<(), FetchError> {
    let response = ureq::get(offer.url)
        .call()
        .map_err(|err| FetchError::Http(err.to_string()))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(offer.size_bytes);
    let mut reader = response
        .into_body()
        .into_with_config()
        .limit(u64::MAX)
        .reader();
    let mut file = fs::File::create(temp)?;
    let mut received = 0u64;
    let mut chunk = [0u8; 65_536];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(FetchError::Cancelled);
        }
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        file.write_all(&chunk[..read])?;
        received += read as u64;
        progress(DownloadProgress {
            received,
            total,
            stage: DownloadStage::Downloading,
        });
    }
    file.flush()?;
    Ok(())
}

fn verify_and_rename(
    offer: &ModelOffer,
    temp: &Path,
    dest: &Path,
    progress: &mut impl FnMut(DownloadProgress),
) -> Result<(), FetchError> {
    progress(DownloadProgress {
        received: offer.size_bytes,
        total: offer.size_bytes,
        stage: DownloadStage::Verifying,
    });
    let actual = sha1_file(temp)?;
    if actual != offer.sha1 {
        return Err(FetchError::HashMismatch {
            expected: offer.sha1.to_string(),
            actual,
        });
    }
    fs::rename(temp, dest)?;
    progress(DownloadProgress {
        received: offer.size_bytes,
        total: offer.size_bytes,
        stage: DownloadStage::Done,
    });
    Ok(())
}

fn sha1_file(path: &Path) -> Result<String, FetchError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha1::new();
    let mut chunk = [0u8; 65_536];
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicBool;

    /// A one-request-at-a-time HTTP server returning a fixed body, for
    /// transport tests without the network.
    struct FixtureServer {
        url: String,
        hits: ArcCounter,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    type ArcCounter = std::sync::Arc<std::sync::atomic::AtomicUsize>;

    impl FixtureServer {
        fn serve(body: &'static [u8]) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let hits = ArcCounter::default();
            let counter = hits.clone();
            let handle = std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { break };
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    // Read the request line and headers; respond regardless.
                    loop {
                        let mut line = String::new();
                        match reader.read_line(&mut line) {
                            Ok(0) | Err(_) => break,
                            Ok(_) if line == "\r\n" => break,
                            Ok(_) => {}
                        }
                    }
                    counter.fetch_add(1, Ordering::Relaxed);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.write_all(body);
                    let _ = stream.flush();
                }
            });
            Self {
                url: format!("http://127.0.0.1:{port}/model.bin"),
                hits,
                handle: Some(handle),
            }
        }
    }

    impl Drop for FixtureServer {
        fn drop(&mut self) {
            // The listener thread blocks on incoming(); let it die with the
            // test process rather than joining it.
            self.handle.take();
        }
    }

    fn test_offer(url: &str, sha1: &str, size: u64) -> ModelOffer {
        ModelOffer {
            id: "test",
            label: "Test",
            filename: "ggml-test.bin",
            url: Box::leak(url.to_string().into_boxed_str()),
            sha1: Box::leak(sha1.to_string().into_boxed_str()),
            size_bytes: size,
            runtime_mb: None,
            multilingual: true,
        }
    }

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("echo-fetch-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    static BODY: &[u8] = b"echo fixture model body, standing in for a ggml file";

    fn fixture_offer(server: &FixtureServer) -> ModelOffer {
        test_offer(&server.url, &hex(&Sha1::digest(BODY)), BODY.len() as u64)
    }

    #[test]
    fn offer_urls_follow_the_published_patterns() {
        assert_eq!(
            offer("small").unwrap().url,
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"
        );
        assert_eq!(
            offer("silero-vad").unwrap().url,
            "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin"
        );
        assert!(offer("nope").is_none());
        assert_eq!(OFFERS.len(), 4);
    }

    #[test]
    fn download_lands_verified_and_rerun_is_a_noop() {
        let server = FixtureServer::serve(BODY);
        let offer = fixture_offer(&server);
        let dir = scratch_dir("happy");
        let cancel = AtomicBool::new(false);
        let mut stages = Vec::new();
        let dest = download(
            &offer,
            &dir,
            |progress| stages.push(progress.stage),
            &cancel,
        )
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), BODY);
        assert!(stages.contains(&DownloadStage::Downloading));
        assert!(stages.contains(&DownloadStage::Verifying));
        assert_eq!(stages.last(), Some(&DownloadStage::Done));

        let hits = server.hits.load(Ordering::Relaxed);
        download(&offer, &dir, |_| {}, &cancel).unwrap();
        assert_eq!(server.hits.load(Ordering::Relaxed), hits, "rerun re-downloads");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hash_mismatch_deletes_the_temp_file_and_names_the_error() {
        let server = FixtureServer::serve(BODY);
        let mut offer = fixture_offer(&server);
        offer.sha1 = "0000000000000000000000000000000000000000";
        let dir = scratch_dir("corrupt");
        let err = download(&offer, &dir, |_| {}, &AtomicBool::new(false)).unwrap_err();
        match err {
            FetchError::HashMismatch { expected, actual } => {
                assert_eq!(expected, "0000000000000000000000000000000000000000");
                assert_eq!(actual, hex(&Sha1::digest(BODY)));
            }
            other => panic!("expected HashMismatch, got {other:?}"),
        }
        let left: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert!(left.is_empty(), "nothing may remain in the model directory");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_leaves_no_partial_file() {
        let server = FixtureServer::serve(BODY);
        let offer = fixture_offer(&server);
        let dir = scratch_dir("cancel");
        let cancel = AtomicBool::new(false);
        let err = download(
            &offer,
            &dir,
            |_| cancel.store(true, Ordering::Relaxed),
            &cancel,
        )
        .unwrap_err();
        assert!(matches!(err, FetchError::Cancelled));
        let left: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert!(left.is_empty(), "no partial file where the scanner looks");
        let _ = fs::remove_dir_all(&dir);
    }
}
