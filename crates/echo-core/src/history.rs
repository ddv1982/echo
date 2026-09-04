use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::engine::RunDetail;
use crate::inject::InjectReport;
use crate::paths::{
    history_path, read_private, set_aside_corrupt, write_atomic_private, PrivateDir,
};
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

#[derive(Debug, Clone, Default, Serialize)]
struct HistoryFile {
    rows: Vec<HistoryRow>,
}

impl<'de> Deserialize<'de> for HistoryFile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct CurrentHistoryFile {
            rows: Vec<HistoryRow>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum HistoryFileWire {
            Current(CurrentHistoryFile),
            Other(std::collections::BTreeMap<String, serde::de::IgnoredAny>),
        }

        match HistoryFileWire::deserialize(deserializer)? {
            HistoryFileWire::Current(file) => Ok(Self { rows: file.rows }),
            HistoryFileWire::Other(fields) if fields.is_empty() => Ok(Self::default()),
            HistoryFileWire::Other(_) => Err(serde::de::Error::custom(
                "history object is missing its rows field",
            )),
        }
    }
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
        let Some(raw) = read_private(&path)? else {
            return Ok(Self {
                rows: Vec::new(),
                path,
            });
        };
        let rows = match serde_json::from_slice::<HistoryFile>(&raw) {
            Ok(file) => file.rows,
            Err(error) => {
                set_aside_corrupt(&path)?;
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
        let Some(raw) = read_private(&path)? else {
            return Ok(Self {
                rows: Vec::new(),
                path,
            });
        };
        let rows = serde_json::from_slice::<HistoryFile>(&raw)
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
        Self::mutate_path_locked(history_path(), move |rows| {
            Self::append_loaded(rows, row);
            ((), true)
        })
        .map(|_| ())
    }

    pub fn remove_default(id: &str) -> Result<bool, String> {
        Self::mutate_path_locked(history_path(), |rows| {
            let removed = Self::remove_loaded(rows, id);
            (removed, removed)
        })
        .map(|(result, _)| result)
    }

    pub fn clear_default() -> Result<usize, String> {
        Self::mutate_path_locked(history_path(), |rows| {
            let count = Self::clear_loaded(rows);
            (count, count > 0)
        })
        .map(|(result, _)| result)
    }

    #[must_use]
    pub fn rows(&self) -> &[HistoryRow] {
        &self.rows
    }

    pub fn append(&mut self, row: HistoryRow) -> Result<(), String> {
        let (_, rows) = Self::mutate_path_locked(&self.path, move |rows| {
            Self::append_loaded(rows, row);
            ((), true)
        })?;
        self.rows = rows;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Result<bool, String> {
        let (removed, rows) = Self::mutate_path_locked(&self.path, |rows| {
            let removed = Self::remove_loaded(rows, id);
            (removed, removed)
        })?;
        self.rows = rows;
        Ok(removed)
    }

    pub fn clear(&mut self) -> Result<usize, String> {
        let (count, rows) = Self::mutate_path_locked(&self.path, |rows| {
            let count = Self::clear_loaded(rows);
            (count, count > 0)
        })?;
        self.rows = rows;
        Ok(count)
    }

    pub fn save(&self) -> Result<(), String> {
        let _process_guard = history_process_guard()?;
        let _file_guard = lock_history_file(&self.path)?;
        let rows = match Self::read_rows_read_only(&self.path)? {
            Some(rows) => rows,
            None => self.rows.clone(),
        };
        Self::save_rows_at(&self.path, &rows)
    }

    fn save_rows_at(path: &Path, rows: &[HistoryRow]) -> Result<(), String> {
        let file = HistoryFile {
            rows: rows.to_vec(),
        };
        let raw = serde_json::to_string_pretty(&file).map_err(|err| err.to_string())?;
        write_atomic_private(path, raw.as_bytes())
    }

    fn mutate_path_locked<T>(
        path: impl AsRef<Path>,
        update: impl FnOnce(&mut Vec<HistoryRow>) -> (T, bool),
    ) -> Result<(T, Vec<HistoryRow>), String> {
        let _process_guard = history_process_guard()?;
        let path = path.as_ref();
        let _file_guard = lock_history_file(path)?;
        let mut rows = Self::read_rows_read_only(path)?.unwrap_or_default();
        let (result, changed) = update(&mut rows);
        if changed {
            Self::save_rows_at(path, &rows)?;
        }
        Ok((result, rows))
    }

    fn read_rows_read_only(path: &Path) -> Result<Option<Vec<HistoryRow>>, String> {
        let Some(raw) = read_private(path)? else {
            return Ok(None);
        };
        serde_json::from_slice::<HistoryFile>(&raw)
            .map(|file| Some(file.rows))
            .map_err(|error| {
                format!(
                    "History file {} contains invalid JSON and was not loaded: {error}",
                    path.display()
                )
            })
    }

    fn append_loaded(rows: &mut Vec<HistoryRow>, row: HistoryRow) {
        rows.push(row);
        if rows.len() > HISTORY_CAP {
            let drop = rows.len() - HISTORY_CAP;
            rows.drain(..drop);
        }
    }

    fn remove_loaded(rows: &mut Vec<HistoryRow>, id: &str) -> bool {
        let Some(index) = rows.iter().position(|row| row.id == id) else {
            return false;
        };
        rows.remove(index);
        true
    }

    fn clear_loaded(rows: &mut Vec<HistoryRow>) -> usize {
        let count = rows.len();
        rows.clear();
        count
    }
}

fn history_process_guard() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    HISTORY_WRITES.lock().map_err(|_| {
        "History writes are unavailable because the history lock is poisoned.".to_string()
    })
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
    match lock_file.try_lock_exclusive() {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            #[cfg(test)]
            report_history_lock_contention();
            lock_file.lock_exclusive().map_err(|err| {
                format!("Could not lock history file {}: {err}", lock_path.display())
            })?;
        }
        Err(err) => {
            return Err(format!(
                "Could not lock history file {}: {err}",
                lock_path.display()
            ));
        }
    }
    Ok(lock_file)
}

#[cfg(test)]
fn report_history_lock_contention() {
    let Some(address) = std::env::var_os("ECHO_TEST_HISTORY_CONTENTION_ADDRESS") else {
        return;
    };
    let mut stream = std::net::TcpStream::connect(address.to_string_lossy().as_ref())
        .expect("history contention observer should accept a connection");
    std::io::Write::write_all(&mut stream, b"history lock contended")
        .expect("history contention marker should be written");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inject::InjectBackend;
    use crate::paths::{fail_next_private_write, PrivateWriteFailure};
    use std::io::Read;
    use std::net::TcpListener;
    use std::process::{Command, Stdio};
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::Duration;

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

    fn assert_no_history_temp_file(path: &Path) {
        let prefix = format!(".{}.tmp-", path.file_name().unwrap().to_string_lossy());
        let residual = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .find(|name| name.to_string_lossy().starts_with(&prefix));
        assert!(
            residual.is_none(),
            "residual history temp file: {residual:?}"
        );
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
    fn empty_object_loads_as_empty_history_without_quarantine() {
        let dir = std::env::temp_dir().join(format!("echo-hist-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        fs::write(&path, "{}").unwrap();

        assert!(History::load_from(&path).unwrap().rows().is_empty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
        assert!(!dir.join("history.json.corrupt").exists());
    }

    #[test]
    fn nonempty_object_without_rows_is_still_rejected() {
        let dir = std::env::temp_dir().join(format!(
            "echo-hist-incompatible-empty-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        fs::write(&path, r#"{"unexpected":true}"#).unwrap();

        assert!(History::load_from(&path).is_err());
        assert!(!path.exists());
        assert!(dir.join("history.json.corrupt").exists());
    }

    #[test]
    fn repeated_corruption_preserves_every_backup() {
        let dir = std::env::temp_dir().join(format!(
            "echo-hist-repeated-corruption-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        fs::write(&path, "first corrupt value").unwrap();
        History::load_from(&path).unwrap_err();
        fs::write(&path, "second corrupt value").unwrap();
        History::load_from(&path).unwrap_err();

        assert_eq!(
            fs::read_to_string(dir.join("history.json.corrupt")).unwrap(),
            "first corrupt value"
        );
        assert_eq!(
            fs::read_to_string(dir.join("history.json.corrupt-1")).unwrap(),
            "second corrupt value"
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

        let mut history = History {
            rows: vec![row("fresh", "must not replace existing history")],
            path: path.clone(),
        };
        let append_error = history
            .append(row("another", "must not replace existing history"))
            .unwrap_err();

        assert!(append_error.contains("invalid JSON"), "{append_error}");
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!dir.join("history.json.corrupt").exists());
        assert_eq!(
            history.rows(),
            &[row("fresh", "must not replace existing history")]
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

        let mut history = History {
            rows: Vec::new(),
            path: path.clone(),
        };
        let error = history
            .append(row("fresh", "must not replace existing history"))
            .unwrap_err();

        assert!(error.contains("invalid JSON"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert!(!dir.join("history.json.corrupt").exists());
        assert!(history.rows().is_empty());
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

        let mut history = History {
            rows: vec![row("stale", "stale")],
            path: path.clone(),
        };
        let error = history.clear().unwrap_err();

        assert!(error.contains("invalid JSON"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert!(!dir.join("history.json.corrupt").exists());
        assert_eq!(history.rows(), &[row("stale", "stale")]);
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
    fn stale_instances_reload_locked_state_before_every_mutation() {
        let dir =
            std::env::temp_dir().join(format!("echo-hist-stale-mutations-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        let mut first = History::load_from(&path).unwrap();
        first.append(row("first", "one")).unwrap();
        let mut stale = History::load_from(&path).unwrap();

        first.append(row("second", "two")).unwrap();
        assert!(stale.remove("first").unwrap());
        assert_eq!(stale.rows(), &[row("second", "two")]);

        first.append(row("third", "three")).unwrap();
        stale.append(row("fourth", "four")).unwrap();
        assert_eq!(
            stale.rows(),
            &[
                row("second", "two"),
                row("third", "three"),
                row("fourth", "four")
            ]
        );
        assert_eq!(History::load_from(&path).unwrap().rows(), stale.rows());
    }

    #[test]
    fn stale_save_does_not_clobber_newer_history() {
        let dir = std::env::temp_dir().join(format!("echo-hist-stale-save-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        let mut current = History::load_from(&path).unwrap();
        current.append(row("first", "one")).unwrap();
        let stale = History::load_from(&path).unwrap();
        current.append(row("second", "two")).unwrap();

        stale.save().unwrap();

        assert_eq!(
            History::load_from(&path).unwrap().rows(),
            &[row("first", "one"), row("second", "two")]
        );
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
            History::mutate_path_locked(&first_path, |rows| {
                first_entered_tx.send(()).unwrap();
                release_first_rx.recv().unwrap();
                let removed = History::remove_loaded(rows, "old");
                (removed, removed)
            })
            .map(|(result, _)| result)
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
            History::mutate_path_locked(&second_path, |rows| {
                second_entered_tx.send(()).unwrap();
                History::append_loaded(rows, row("new", "keep me"));
                ((), true)
            })
            .map(|_| ())
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
        History::load_from(PathBuf::from(path))
            .unwrap()
            .append(row("child", "written second"))
            .unwrap();
    }

    #[test]
    fn locked_updates_are_serialized_across_processes() {
        let dir =
            std::env::temp_dir().join(format!("echo-hist-cross-process-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        History::load_from(&path)
            .unwrap()
            .append(row("parent", "written first"))
            .unwrap();
        let lock_file = lock_history_file(&path).unwrap();
        let contention_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let contention_address = contention_listener.local_addr().unwrap();

        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("history::tests::cross_process_update_helper")
            .env("ECHO_TEST_HISTORY_CHILD_PATH", &path)
            .env(
                "ECHO_TEST_HISTORY_CONTENTION_ADDRESS",
                contention_address.to_string(),
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let (mut contention, _) = contention_listener.accept().unwrap();
        let mut marker = [0_u8; 22];
        contention.read_exact(&mut marker).unwrap();
        assert_eq!(&marker, b"history lock contended");
        assert_eq!(
            History::load_from_read_only(&path).unwrap().rows(),
            &[row("parent", "written first")]
        );
        assert!(
            child.try_wait().unwrap().is_none(),
            "child completed while the parent held the advisory lock"
        );

        FileExt::unlock(&lock_file).unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(
            History::load_from(&path).unwrap().rows(),
            &[
                row("parent", "written first"),
                row("child", "written second")
            ]
        );
    }

    #[test]
    fn failed_append_keeps_memory_and_persisted_history_unchanged() {
        for failure in [PrivateWriteFailure::Write, PrivateWriteFailure::Sync] {
            let dir = std::env::temp_dir().join(format!(
                "echo-hist-append-failure-{}-{failure:?}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("history.json");
            let mut store = History::load_from(&path).unwrap();
            store.append(row("first", "one")).unwrap();
            let original_rows = store.rows().to_vec();
            let original_file = fs::read(&path).unwrap();

            fail_next_private_write(failure);
            assert!(store.append(row("second", "two")).is_err());
            assert_eq!(store.rows(), original_rows);
            assert_eq!(fs::read(&path).unwrap(), original_file);
            assert_no_history_temp_file(&path);
        }
    }

    #[test]
    fn failed_remove_keeps_memory_and_persisted_history_unchanged() {
        for failure in [PrivateWriteFailure::Write, PrivateWriteFailure::Sync] {
            let dir = std::env::temp_dir().join(format!(
                "echo-hist-remove-failure-{}-{failure:?}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("history.json");
            let mut store = History::load_from(&path).unwrap();
            store.append(row("first", "one")).unwrap();
            store.append(row("second", "two")).unwrap();
            let original_rows = store.rows().to_vec();
            let original_file = fs::read(&path).unwrap();

            fail_next_private_write(failure);
            assert!(store.remove("first").is_err());
            assert_eq!(store.rows(), original_rows);
            assert_eq!(fs::read(&path).unwrap(), original_file);
            assert_no_history_temp_file(&path);
        }
    }

    #[test]
    fn failed_clear_keeps_memory_and_persisted_history_unchanged() {
        for failure in [PrivateWriteFailure::Write, PrivateWriteFailure::Sync] {
            let dir = std::env::temp_dir().join(format!(
                "echo-hist-clear-failure-{}-{failure:?}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("history.json");
            let mut store = History::load_from(&path).unwrap();
            store.append(row("first", "one")).unwrap();
            store.append(row("second", "two")).unwrap();
            let original_rows = store.rows().to_vec();
            let original_file = fs::read(&path).unwrap();

            fail_next_private_write(failure);
            assert!(store.clear().is_err());
            assert_eq!(store.rows(), original_rows);
            assert_eq!(fs::read(&path).unwrap(), original_file);
            assert_no_history_temp_file(&path);
        }
    }

    #[test]
    fn failed_save_keeps_memory_and_persisted_history_unchanged_without_temp_files() {
        for failure in [PrivateWriteFailure::Write, PrivateWriteFailure::Sync] {
            let dir = std::env::temp_dir().join(format!(
                "echo-hist-save-failure-{}-{failure:?}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("history.json");
            let mut store = History::load_from(&path).unwrap();
            store.append(row("first", "one")).unwrap();
            let original_rows = store.rows().to_vec();
            let original_file = fs::read(&path).unwrap();

            fail_next_private_write(failure);
            let error = store.save().unwrap_err();

            assert!(error.contains("injected private"), "{error}");
            assert_eq!(store.rows(), original_rows);
            assert_eq!(fs::read(&path).unwrap(), original_file);
            assert_no_history_temp_file(&path);
        }
    }
}
