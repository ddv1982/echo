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
