pub mod catalog;
mod download;
mod extract;
mod filesystem;
mod payload;
mod store;
mod types;

pub use catalog::{ComponentId, SetupPlanId};
pub use download::required_free_bytes;
pub use download::{
    DiskSpace, HttpRequest, HttpResponse, HttpTransport, SystemDisk, UreqTransport,
};
pub use store::ManagedStore;
pub use types::{
    ActivationRecord, ComponentLease, InstallError, InstallPhase, InstallProgress, InstalledFile,
    ManagedComponentState, ManagedPath, OperationId,
};

#[cfg(test)]
pub(crate) use payload::trust_payload_fixture;

mod installer;
pub use installer::{CommandRuntimeProbe, Installer, RuntimeProbe};
#[cfg(test)]
mod tests;

fn sha256_file_cancellable(
    path: &std::path::Path,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<String, InstallError> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    use std::sync::atomic::Ordering;

    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
            return Err(InstallError::Cancelled);
        }
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
mod hashing_tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn shared_file_sha256_honours_cancellation_and_hashes_when_active() {
        let path = std::env::temp_dir().join(format!("echo-shared-sha256-{}", std::process::id()));
        let body = b"shared install digest";
        std::fs::write(&path, body).unwrap();

        assert!(matches!(
            sha256_file_cancellable(&path, Some(&AtomicBool::new(true))),
            Err(InstallError::Cancelled)
        ));
        assert_eq!(
            sha256_file_cancellable(&path, None).unwrap(),
            format!("{:x}", Sha256::digest(body))
        );
        let _ = std::fs::remove_file(path);
    }
}
