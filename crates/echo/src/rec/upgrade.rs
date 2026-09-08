use std::path::Path;
use std::sync::Mutex;

use super::lease::{LockAcquisition, ToggleSession};

static COMMITTED_TAKEOVER: Mutex<Option<ToggleSession>> = Mutex::new(None);
pub struct TakeoverReservation(Option<ToggleSession>);

impl TakeoverReservation {
    pub(super) fn commit(mut self) {
        let session = self.0.take().expect("takeover reservation");
        *COMMITTED_TAKEOVER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(session);
    }
}

#[derive(Debug)]
pub enum UpgradeTakeover {
    Deferred,
    Spawned,
    SpawnFailed(std::io::Error),
}

/// Reserve the final idle decision, check the cross-process recording lock,
/// and spawn the replacement. A failed spawn reopens local recording; a
/// successful spawn leaves it blocked until this process exits.
pub fn attempt_upgrade_takeover(spawn: impl FnOnce() -> std::io::Result<()>) -> UpgradeTakeover {
    attempt_upgrade_takeover_in(&echo_core::data_dir(), spawn)
}

pub(super) fn attempt_upgrade_takeover_in(
    dir: &Path,
    spawn: impl FnOnce() -> std::io::Result<()>,
) -> UpgradeTakeover {
    let reservation = match reserve_upgrade_takeover_in(dir) {
        Ok(reservation) => reservation,
        Err(_) => return UpgradeTakeover::Deferred,
    };
    match spawn() {
        Ok(()) => {
            reservation.commit();
            UpgradeTakeover::Spawned
        }
        Err(err) => UpgradeTakeover::SpawnFailed(err),
    }
}

pub fn reserve_upgrade_takeover() -> Result<TakeoverReservation, String> {
    reserve_upgrade_takeover_in(&echo_core::data_dir())
}

pub(super) fn reserve_upgrade_takeover_in(dir: &Path) -> Result<TakeoverReservation, String> {
    match ToggleSession::acquire_in(dir)? {
        LockAcquisition::Started(session) => Ok(TakeoverReservation(Some(session))),
        LockAcquisition::Busy(_) => Err("recording is active".to_string()),
    }
}
