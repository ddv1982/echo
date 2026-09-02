use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::engine::RunDetail;
use crate::inject::InjectReport;
use crate::paths::{history_path, set_aside_corrupt, write_atomic_private, PrivateDir};
use crate::types::EngineId;

const HISTORY_CAP: usize = 2000;
static HISTORY_WRITES: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryRow {
    pub id: String,
    pub text: String,
    pub raw: String,
    pub engine: EngineId,
    pub started_at: u64,
    pub infer_ms: u64,
    pub inject: InjectReport,
    /// What the engine reported about the run. Absent on rows written before
    /// the field existed.
    #[serde(default)]
    pub detail: RunDetail,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HistoryFile {
    rows: Vec<HistoryRow>,
}

#[derive(Debug, Clone)]
pub struct History {
    rows: Vec<HistoryRow>,
    path: PathBuf,
}

impl History {
    pub fn load() -> Result<Self, String> {
        Self::load_from(history_path())
    }

    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Ok(Self {
                rows: Vec::new(),
                path,
            });
        }
        let raw = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        let rows = match serde_json::from_str::<HistoryFile>(&raw) {
            Ok(file) => file.rows,
            Err(error) => {
                set_aside_corrupt(&path);
                return Err(format!(
                    "History file {} contains invalid JSON and was not loaded: {error}",
                    path.display()
                ));
            }
        };
        Ok(Self { rows, path })
    }

    pub fn load_read_only() -> Result<Self, String> {
        Self::load_from_read_only(history_path())
    }

    pub fn load_from_read_only(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Ok(Self {
                rows: Vec::new(),
                path,
            });
        }
        let raw = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        let rows = serde_json::from_str::<HistoryFile>(&raw)
            .map(|file| file.rows)
            .map_err(|error| {
                format!(
                    "History file {} contains invalid JSON and was not loaded: {error}",
                    path.display()
                )
            })?;
        Ok(Self { rows, path })
    }

    pub fn append_default(row: HistoryRow) -> Result<(), String> {
        Self::update_locked(history_path(), move |history| history.append(row))
    }

    pub fn remove_default(id: &str) -> Result<bool, String> {
        Self::update_locked(history_path(), |history| history.remove(id))
    }

    pub fn clear_default() -> Result<usize, String> {
        Self::update_locked(history_path(), Self::clear)
    }

    #[must_use]
    pub fn rows(&self) -> &[HistoryRow] {
        &self.rows
    }

    pub fn append(&mut self, row: HistoryRow) -> Result<(), String> {
        self.rows.push(row);
        if self.rows.len() > HISTORY_CAP {
            let drop = self.rows.len() - HISTORY_CAP;
            self.rows.drain(..drop);
        }
        self.save()
    }

    pub fn remove(&mut self, id: &str) -> Result<bool, String> {
        let Some(index) = self.rows.iter().position(|row| row.id == id) else {
            return Ok(false);
        };
        let mut rows = self.rows.clone();
        rows.remove(index);
        self.save_rows(&rows)?;
        self.rows = rows;
        Ok(true)
    }

    pub fn clear(&mut self) -> Result<usize, String> {
        let count = self.rows.len();
        if count == 0 {
            return Ok(0);
        }
        let rows = Vec::new();
        self.save_rows(&rows)?;
        self.rows = rows;
        Ok(count)
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_rows(&self.rows)
    }

    fn save_rows(&self, rows: &[HistoryRow]) -> Result<(), String> {
        let file = HistoryFile {
            rows: rows.to_vec(),
        };
        let raw = serde_json::to_string_pretty(&file).map_err(|err| err.to_string())?;
        write_atomic_private(&self.path, raw.as_bytes())
    }

    fn update_locked<T>(
        path: impl AsRef<Path>,
        update: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let _process_guard = HISTORY_WRITES.lock().map_err(|_| {
            "History writes are unavailable because the history lock is poisoned.".to_string()
        })?;
        let path = path.as_ref();
        let _file_guard = lock_history_file(path)?;
        let mut history = Self::load_from(path)?;
        update(&mut history)
    }
}

fn lock_history_file(path: &Path) -> Result<fs::File, String> {
    let parent = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let directory = PrivateDir::open(parent).map_err(|err| {
        format!(
            "Could not create history lock directory {}: {err}",
            parent.display()
        )
    })?;
    let mut lock_name = path
        .file_name()
        .ok_or_else(|| format!("Could not derive a lock file for {}", path.display()))?
        .to_os_string();
    lock_name.push(".lock");
    let lock_path = parent.join(lock_name);
    let lock_file = directory
        .open_or_create(lock_path.file_name().expect("lock path has a file name"))
        .map_err(|err| format!("Could not open history lock {}: {err}", lock_path.display()))?;
    lock_file
        .lock_exclusive()
        .map_err(|err| format!("Could not lock history file {}: {err}", lock_path.display()))?;
    Ok(lock_file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inject::InjectBackend;
    use std::process::{Command, Stdio};
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::{Duration, Instant};

    fn row(id: &str, text: &str) -> HistoryRow {
        HistoryRow {
            id: id.into(),
            text: text.into(),
            raw: text.into(),
            engine: EngineId::Whisper {
                model: "fake".into(),
            },
            started_at: 1,
            infer_ms: 2,
            inject: InjectReport::Typed {
                backend: InjectBackend::Xdotool,
            },
            detail: RunDetail::default(),
        }
    }

    #[test]
    fn persists_across_reload() {
        let dir = std::env::temp_dir().join(format!("echo-hist-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        let mut store = History::load_from(&path).unwrap();
        store.append(row("1", "hello")).unwrap();
        let reloaded = History::load_from(&path).unwrap();
        assert_eq!(reloaded.rows().len(), 1);
        assert_eq!(reloaded.rows()[0].text, "hello");
    }

    #[cfg(unix)]
    #[test]
    fn history_lock_secures_its_directory_and_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "echo-history-private-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let path = dir.join("history.json");
        let lock = lock_history_file(&path).unwrap();

        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(dir.join("history.json.lock"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        drop(lock);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_file_is_set_aside_and_reported() {
        let dir = std::env::temp_dir().join(format!("echo-hist-corrupt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        let original = "{\"rows\": [{\"id\": \"truncat";
        fs::write(&path, original).unwrap();

        let error = History::load_from(&path).unwrap_err();
        assert!(error.contains("invalid JSON"), "{error}");
        assert!(error.contains(path.to_str().unwrap()), "{error}");
        assert!(!path.exists(), "corrupt file should be moved aside");
        assert_eq!(
            fs::read_to_string(dir.join("history.json.corrupt")).unwrap(),
            original
        );
    }

    #[test]
    fn read_only_malformed_history_returns_error_without_moving_or_rewriting_it() {
        let dir = std::env::temp_dir().join(format!(
            "echo-hist-read-only-corrupt-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        let original = b"{\"rows\": [{\"id\": \"truncated";
        fs::write(&path, original).unwrap();

        let error = History::load_from_read_only(&path).unwrap_err();

        assert!(error.contains("invalid JSON"), "{error}");
        assert!(error.contains(path.to_str().unwrap()), "{error}");
        assert!(path.exists(), "read-only load must leave history in place");
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!dir.join("history.json.corrupt").exists());
    }

    #[test]
    fn read_only_load_then_locked_append_still_reports_malformed_history() {
        let dir = std::env::temp_dir().join(format!(
            "echo-hist-read-only-then-append-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        let original = b"{\"rows\": [{\"id\": \"existing";
        fs::write(&path, original).unwrap();

        let read_error = History::load_from_read_only(&path).unwrap_err();
        assert!(read_error.contains("invalid JSON"), "{read_error}");
        assert_eq!(fs::read(&path).unwrap(), original);

        let append_error = History::update_locked(&path, |history| {
            history.append(row("fresh", "must not replace existing history"))
        })
        .unwrap_err();

        assert!(append_error.contains("invalid JSON"), "{append_error}");
        assert!(!path.exists(), "no fresh history should be written");
        assert_eq!(
            fs::read(dir.join("history.json.corrupt")).unwrap(),
            original
        );
    }

    #[test]
    fn locked_append_does_not_replace_malformed_history() {
        let dir =
            std::env::temp_dir().join(format!("echo-hist-corrupt-append-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        let original = "{\"rows\": [{\"id\": \"existing";
        fs::write(&path, original).unwrap();

        let error = History::update_locked(&path, |history| {
            history.append(row("fresh", "must not replace existing history"))
        })
        .unwrap_err();

        assert!(error.contains("invalid JSON"), "{error}");
        assert!(!path.exists(), "no replacement history should be written");
        assert_eq!(
            fs::read_to_string(dir.join("history.json.corrupt")).unwrap(),
            original
        );
    }

    #[test]
    fn locked_clear_does_not_silently_reset_malformed_history() {
        let dir =
            std::env::temp_dir().join(format!("echo-hist-corrupt-clear-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        let original = "{\"rows\": [{\"id\": \"existing";
        fs::write(&path, original).unwrap();

        let error = History::update_locked(&path, History::clear).unwrap_err();

        assert!(error.contains("invalid JSON"), "{error}");
        assert!(!path.exists(), "no reset history should be written");
        assert_eq!(
            fs::read_to_string(dir.join("history.json.corrupt")).unwrap(),
            original
        );
    }

    #[test]
    fn removes_one_row_and_leaves_unknown_ids_unchanged() {
        let dir = std::env::temp_dir().join(format!("echo-hist-remove-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        let mut store = History::load_from(&path).unwrap();
        store.append(row("first", "remove me")).unwrap();
        store.append(row("second", "keep me")).unwrap();

        assert!(store.remove("first").unwrap());
        assert_eq!(store.rows(), &[row("second", "keep me")]);
        assert_eq!(History::load_from(&path).unwrap().rows(), store.rows());

        let persisted = fs::read_to_string(&path).unwrap();
        assert!(!store.remove("unknown").unwrap());
        assert_eq!(store.rows(), &[row("second", "keep me")]);
        assert_eq!(fs::read_to_string(&path).unwrap(), persisted);
        assert_eq!(History::load_from(&path).unwrap().rows(), store.rows());
    }

    #[test]
    fn clears_all_rows_and_reports_the_prior_count() {
        let dir = std::env::temp_dir().join(format!("echo-hist-clear-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        let mut store = History::load_from(&path).unwrap();
        store.append(row("first", "one")).unwrap();
        store.append(row("second", "two")).unwrap();

        assert_eq!(store.clear().unwrap(), 2);
        assert!(store.rows().is_empty());
        assert!(History::load_from(&path).unwrap().rows().is_empty());

        let persisted = fs::read_to_string(&path).unwrap();
        assert_eq!(store.clear().unwrap(), 0);
        assert!(store.rows().is_empty());
        assert_eq!(fs::read_to_string(&path).unwrap(), persisted);
        assert!(History::load_from(&path).unwrap().rows().is_empty());
    }

    #[test]
    fn locked_updates_preserve_serial_remove_then_append_order() {
        let dir = std::env::temp_dir().join(format!("echo-hist-serialized-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        History::load_from(&path)
            .unwrap()
            .append(row("old", "remove me"))
            .unwrap();

        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let first_path = path.clone();
        let first = std::thread::spawn(move || {
            History::update_locked(&first_path, |history| {
                first_entered_tx.send(()).unwrap();
                release_first_rx.recv().unwrap();
                history.remove("old")
            })
        });
        first_entered_rx.recv().unwrap();

        let start_second = Arc::new(Barrier::new(2));
        let second_start = Arc::clone(&start_second);
        let (second_attempting_tx, second_attempting_rx) = mpsc::channel();
        let (second_entered_tx, second_entered_rx) = mpsc::channel();
        let second_path = path.clone();
        let second = std::thread::spawn(move || {
            second_start.wait();
            second_attempting_tx.send(()).unwrap();
            History::update_locked(&second_path, |history| {
                second_entered_tx.send(()).unwrap();
                history.append(row("new", "keep me"))
            })
        });
        start_second.wait();
        second_attempting_rx.recv().unwrap();

        let entered_while_first_held = second_entered_rx
            .recv_timeout(Duration::from_millis(100))
            .is_ok();
        release_first_tx.send(()).unwrap();
        assert!(first.join().unwrap().unwrap());
        second.join().unwrap().unwrap();

        assert!(!entered_while_first_held);
        assert_eq!(
            History::load_from(&path).unwrap().rows(),
            &[row("new", "keep me")]
        );
    }

    #[test]
    fn cross_process_update_helper() {
        let Some(path) = std::env::var_os("ECHO_TEST_HISTORY_CHILD_PATH") else {
            return;
        };
        let marker = std::env::var_os("ECHO_TEST_HISTORY_ATTEMPT_MARKER")
            .expect("child attempt marker path should be set");
        fs::write(marker, b"attempting update").unwrap();
        History::update_locked(PathBuf::from(path), |history| {
            history.append(row("child", "written second"))
        })
        .unwrap();
    }

    #[test]
    fn locked_updates_are_serialized_across_processes() {
        let dir =
            std::env::temp_dir().join(format!("echo-hist-cross-process-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        let marker = dir.join("child-attempted");

        let lock_file = lock_history_file(&path).unwrap();
        History::load_from(&path)
            .unwrap()
            .append(row("parent", "written first"))
            .unwrap();

        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("history::tests::cross_process_update_helper")
            .env("ECHO_TEST_HISTORY_CHILD_PATH", &path)
            .env("ECHO_TEST_HISTORY_ATTEMPT_MARKER", &marker)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let marker_deadline = Instant::now() + Duration::from_secs(5);
        let marker_observed = loop {
            if marker.exists() {
                break true;
            }
            if child.try_wait().unwrap().is_some() || Instant::now() >= marker_deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        std::thread::sleep(Duration::from_millis(150));
        let blocked_while_parent_held_lock = child.try_wait().unwrap().is_none();

        FileExt::unlock(&lock_file).unwrap();
        let child_deadline = Instant::now() + Duration::from_secs(5);
        let child_status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if Instant::now() >= child_deadline {
                child.kill().unwrap();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };

        assert!(marker_observed, "child did not reach the update attempt");
        assert!(
            blocked_while_parent_held_lock,
            "child completed while the parent held the advisory lock"
        );
        assert!(child_status.is_some_and(|status| status.success()));
        assert_eq!(
            History::load_from(&path).unwrap().rows(),
            &[
                row("parent", "written first"),
                row("child", "written second")
            ]
        );
    }

    #[test]
    fn failed_remove_keeps_memory_and_persisted_history_unchanged() {
        let dir =
            std::env::temp_dir().join(format!("echo-hist-remove-failure-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let original_path = dir.join("history.json");
        let mut store = History::load_from(&original_path).unwrap();
        store.append(row("first", "one")).unwrap();
        store.append(row("second", "two")).unwrap();
        let original_rows = store.rows().to_vec();
        let original_file = fs::read_to_string(&original_path).unwrap();
        let invalid_parent = dir.join("regular-file");
        fs::write(&invalid_parent, "not a directory").unwrap();
        store.path = invalid_parent.join("history.json");

        assert!(store.remove("first").is_err());
        assert_eq!(store.rows(), original_rows);
        assert_eq!(fs::read_to_string(&original_path).unwrap(), original_file);
    }

    #[test]
    fn failed_clear_keeps_memory_and_persisted_history_unchanged() {
        let dir =
            std::env::temp_dir().join(format!("echo-hist-clear-failure-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let original_path = dir.join("history.json");
        let mut store = History::load_from(&original_path).unwrap();
        store.append(row("first", "one")).unwrap();
        store.append(row("second", "two")).unwrap();
        let original_rows = store.rows().to_vec();
        let original_file = fs::read_to_string(&original_path).unwrap();
        let invalid_parent = dir.join("regular-file");
        fs::write(&invalid_parent, "not a directory").unwrap();
        store.path = invalid_parent.join("history.json");

        assert!(store.clear().is_err());
        assert_eq!(store.rows(), original_rows);
        assert_eq!(fs::read_to_string(&original_path).unwrap(), original_file);
    }
}
