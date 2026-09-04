use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Component, Path, PathBuf};

use rustix::fs::{AtFlags, Mode, OFlags};

#[must_use]
pub fn data_dir() -> PathBuf {
    try_data_dir().unwrap_or_else(|error| panic!("{error}"))
}

pub fn try_data_dir() -> Result<PathBuf, String> {
    resolve_data_dir(
        env::var_os("ECHO_DATA_DIR").map(PathBuf::from),
        env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
    )
}

#[must_use]
pub fn config_dir() -> PathBuf {
    try_config_dir().unwrap_or_else(|error| panic!("{error}"))
}

pub fn try_config_dir() -> Result<PathBuf, String> {
    resolve_config_dir(
        env::var_os("ECHO_CONFIG_DIR").map(PathBuf::from),
        env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
    )
}

fn resolve_data_dir(
    explicit: Option<PathBuf>,
    xdg: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf, String> {
    resolve_dir(
        explicit,
        xdg,
        home,
        "ECHO_DATA_DIR",
        "XDG_DATA_HOME",
        &[".local", "share", "echo"],
        "data",
    )
}

fn resolve_config_dir(
    explicit: Option<PathBuf>,
    xdg: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf, String> {
    resolve_dir(
        explicit,
        xdg,
        home,
        "ECHO_CONFIG_DIR",
        "XDG_CONFIG_HOME",
        &[".config", "echo"],
        "configuration",
    )
}

fn resolve_dir(
    explicit: Option<PathBuf>,
    xdg: Option<PathBuf>,
    home: Option<PathBuf>,
    explicit_variable: &str,
    xdg_variable: &str,
    under_home: &[&str],
    description: &str,
) -> Result<PathBuf, String> {
    if let Some(dir) = explicit {
        if dir.is_absolute() {
            return Ok(dir);
        }
        let value = if dir.as_os_str().is_empty() {
            "it is empty".to_string()
        } else {
            format!("got {}", dir.display())
        };
        return Err(format!(
            "{explicit_variable} must be an absolute path ({value})"
        ));
    }
    if let Some(xdg) = xdg.filter(|dir| dir.is_absolute()) {
        return Ok(xdg.join("echo"));
    }
    if let Some(home) = home.filter(|dir| dir.is_absolute()) {
        let mut dir = home;
        for part in under_home {
            dir.push(part);
        }
        return Ok(dir);
    }
    Err(format!(
        "could not resolve the Echo {description} directory: {xdg_variable} and HOME are unset, empty, or relative; set {explicit_variable} to an absolute path"
    ))
}

#[must_use]
pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

#[must_use]
pub fn dictionary_path() -> PathBuf {
    data_dir().join("dictionary.json")
}

#[must_use]
pub fn history_path() -> PathBuf {
    data_dir().join("history.json")
}

#[must_use]
pub fn status_path() -> PathBuf {
    data_dir().join("status")
}

/// Move an unparseable store aside so the app can start fresh without
/// destroying the evidence.
pub(crate) fn set_aside_corrupt(path: &Path) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or_else(|| format!("{} has no private parent directory", path.display()))?;
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?;
    PrivateDir::open(parent)
        .and_then(|directory| directory.set_aside_corrupt(name))
        .map(|backup| parent.join(backup))
        .map_err(|error| error.to_string())
}

pub(crate) fn read_private(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let parent = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or_else(|| format!("{} has no private parent directory", path.display()))?;
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?;
    let directory = PrivateDir::open(parent).map_err(|error| error.to_string())?;
    match directory.read(name) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// Write via a same-directory temp file plus rename, so a crash mid-write
/// never corrupts the previous contents and readers never see a partial file.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    write_atomic_with_dir_sync(path, contents, |parent| {
        fs::File::open(parent).and_then(|directory| directory.sync_all())
    })
}

pub fn write_atomic_private(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or_else(|| format!("{} has no private parent directory", path.display()))?;
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?;
    PrivateDir::open(parent)
        .and_then(|directory| directory.write_atomic(name, contents))
        .map_err(|err| err.to_string())
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivateWriteFailure {
    Write,
    Sync,
}

#[cfg(test)]
thread_local! {
    static NEXT_PRIVATE_WRITE_FAILURE: std::cell::Cell<Option<PrivateWriteFailure>> = const {
        std::cell::Cell::new(None)
    };
}

#[cfg(test)]
pub(crate) fn fail_next_private_write(failure: PrivateWriteFailure) {
    NEXT_PRIVATE_WRITE_FAILURE.with(|next| next.set(Some(failure)));
}

#[cfg(test)]
fn fail_private_write_at(point: PrivateWriteFailure) -> io::Result<()> {
    NEXT_PRIVATE_WRITE_FAILURE.with(|next| {
        if next.get() == Some(point) {
            next.set(None);
            Err(io::Error::other(format!(
                "injected private {point:?} failure"
            )))
        } else {
            Ok(())
        }
    })
}

fn write_atomic_with_dir_sync(
    path: &Path,
    contents: &[u8],
    sync_directory: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<(), String> {
    // Pid alone is not unique enough: two threads of the desktop app can
    // write the same file concurrently, so each call gets its own counter.
    static WRITE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = WRITE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let parent = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{} has no file name", path.display()))?;
    let tmp = parent.join(format!(".{name}.tmp-{}-{seq}", std::process::id()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        let mut file = options.open(&tmp).map_err(|err| err.to_string())?;
        file.write_all(contents).map_err(|err| err.to_string())?;
        file.sync_all().map_err(|err| err.to_string())?;
        drop(file);
        fs::rename(&tmp, path).map_err(|err| err.to_string())?;
        let _ = sync_directory(parent);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[derive(Debug)]
pub struct PrivateDir {
    handle: fs::File,
}

impl PrivateDir {
    pub fn open(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("private directory {} is not absolute", path.display()),
            ));
        }

        let components = path
            .components()
            .filter_map(|component| match component {
                Component::RootDir => None,
                Component::Normal(name) => Some(Ok(name)),
                _ => Some(Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("private directory {} is not normalized", path.display()),
                ))),
            })
            .collect::<io::Result<Vec<_>>>()?;
        if components.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the filesystem root cannot be a private directory",
            ));
        }

        let mut directory = fs::File::open("/")?;
        for (index, name) in components.iter().enumerate() {
            let final_component = index + 1 == components.len();
            let path_flags = OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
            let (path_fd, secure_before_open) =
                match rustix::fs::openat(&directory, *name, path_flags, Mode::empty()) {
                    Ok(fd) => (fd, false),
                    Err(err) if err == rustix::io::Errno::NOENT => {
                        match rustix::fs::mkdirat(&directory, *name, Mode::RWXU) {
                            Ok(()) => {}
                            Err(err) if err == rustix::io::Errno::EXIST => {}
                            Err(err) => return Err(err.into()),
                        }
                        let fd = rustix::fs::openat(&directory, *name, path_flags, Mode::empty())
                            .map_err(io::Error::from)?;
                        (fd, true)
                    }
                    Err(err) => return Err(err.into()),
                };

            if secure_before_open || final_component {
                secure_path_fd(&path_fd, Mode::RWXU)?;
            }

            let next = rustix::fs::openat(
                &directory,
                *name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(io::Error::from)?;
            let next = fs::File::from(next);
            if secure_before_open || final_component {
                rustix::fs::fchmod(&next, Mode::RWXU).map_err(io::Error::from)?;
            }
            directory = next;
        }

        Ok(Self { handle: directory })
    }

    pub fn create_new(&self, name: &std::ffi::OsStr) -> io::Result<fs::File> {
        self.open_file(name, OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL)
    }

    pub fn open_or_create(&self, name: &std::ffi::OsStr) -> io::Result<fs::File> {
        self.open_file(name, OFlags::RDWR | OFlags::CREATE)
    }

    pub fn read_to_string(&self, name: &std::ffi::OsStr) -> io::Result<String> {
        let bytes = self.read(name)?;
        String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn read(&self, name: &std::ffi::OsStr) -> io::Result<Vec<u8>> {
        let name = private_file_name(name)?;
        let fd = rustix::fs::openat(
            &self.handle,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(io::Error::from)?;
        let mut file = fs::File::from(fd);
        rustix::fs::fchmod(&file, Mode::RUSR | Mode::WUSR).map_err(io::Error::from)?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        Ok(contents)
    }

    pub fn hard_link(&self, existing: &std::ffi::OsStr, new: &std::ffi::OsStr) -> io::Result<()> {
        let existing = private_file_name(existing)?;
        let new = private_file_name(new)?;
        rustix::fs::linkat(&self.handle, existing, &self.handle, new, AtFlags::empty())
            .map_err(io::Error::from)
    }

    pub fn remove_file(&self, name: &std::ffi::OsStr) -> io::Result<()> {
        let name = private_file_name(name)?;
        rustix::fs::unlinkat(&self.handle, name, AtFlags::empty()).map_err(io::Error::from)
    }

    fn open_file(&self, name: &std::ffi::OsStr, flags: OFlags) -> io::Result<fs::File> {
        let name = private_file_name(name)?;
        let fd = rustix::fs::openat(
            &self.handle,
            name,
            flags | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(io::Error::from)?;
        let file = fs::File::from(fd);
        rustix::fs::fchmod(&file, Mode::RUSR | Mode::WUSR).map_err(io::Error::from)?;
        Ok(file)
    }

    fn write_atomic(&self, name: &std::ffi::OsStr, contents: &[u8]) -> io::Result<()> {
        static WRITE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let name = private_file_name(name)?;
        let seq = WRITE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut tmp = std::ffi::OsString::from(".");
        tmp.push(name);
        tmp.push(format!(".tmp-{}-{seq}", std::process::id()));

        let result = (|| {
            let mut file = self.create_new(&tmp)?;
            #[cfg(test)]
            fail_private_write_at(PrivateWriteFailure::Write)?;
            file.write_all(contents)?;
            #[cfg(test)]
            fail_private_write_at(PrivateWriteFailure::Sync)?;
            file.sync_all()?;
            drop(file);
            rustix::fs::renameat(&self.handle, &tmp, &self.handle, name)
                .map_err(io::Error::from)?;
            let _ = self.handle.sync_all();
            Ok(())
        })();
        if result.is_err() {
            let _ = self.remove_file(&tmp);
        }
        result
    }

    fn set_aside_corrupt(&self, name: &std::ffi::OsStr) -> io::Result<PathBuf> {
        use std::os::unix::fs::MetadataExt;

        let name = private_file_name(name)?;
        let source = rustix::fs::openat(
            &self.handle,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map(fs::File::from)
        .map_err(io::Error::from)?;
        let source_metadata = source.metadata()?;
        if !source_metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "corrupt store is not a regular file",
            ));
        }

        for attempt in 0_u32..1_000 {
            let mut candidate = name.to_os_string();
            candidate.push(if attempt == 0 {
                ".corrupt".to_string()
            } else {
                format!(".corrupt-{attempt}")
            });
            match self.hard_link(name, &candidate) {
                Ok(()) => {
                    let linked = rustix::fs::openat(
                        &self.handle,
                        &candidate,
                        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map(fs::File::from)
                    .map_err(io::Error::from)?;
                    let linked_metadata = linked.metadata()?;
                    if linked_metadata.dev() != source_metadata.dev()
                        || linked_metadata.ino() != source_metadata.ino()
                    {
                        let _ = self.remove_file(&candidate);
                        return Err(io::Error::other("corrupt store changed during backup"));
                    }
                    self.remove_file(name)?;
                    return Ok(PathBuf::from(candidate));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a corrupt backup name",
        ))
    }
}

fn private_file_name(name: &std::ffi::OsStr) -> io::Result<&std::ffi::OsStr> {
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(name)), None) => Ok(name),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "private file name must be one normal path component",
        )),
    }
}

fn secure_path_fd(fd: &impl AsRawFd, mode: Mode) -> io::Result<()> {
    // O_PATH can open a mode-000 directory created under a restrictive umask.
    // Its procfs link names the pinned inode, not a replaceable path component.
    let path = PathBuf::from("/proc/self/fd").join(fd.as_raw_fd().to_string());
    rustix::fs::chmod(path, mode).map_err(io::Error::from)
}

pub fn ensure_private_dir(path: &Path) -> Result<(), String> {
    PrivateDir::open(path).map(drop).map_err(|err| {
        format!(
            "could not secure private directory {}: {err}",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn atomic_writes_keep_private_directory_and_file_modes() {
        use std::os::unix::fs::PermissionsExt;

        const CHILD_ENV: &str = "ECHO_PRIVATE_WRITE_UMASK_CHILD";
        if std::env::var_os(CHILD_ENV).is_none() {
            for mask in ["000", "777"] {
                let status = std::process::Command::new("sh")
                    .args(["-c", "umask \"$1\"; shift; exec \"$@\"", "sh", mask])
                    .arg(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "paths::tests::atomic_writes_keep_private_directory_and_file_modes",
                        "--nocapture",
                    ])
                    .env(CHILD_ENV, "1")
                    .status()
                    .unwrap();
                assert!(status.success(), "private write failed under umask {mask}");
            }
            return;
        }

        let root = std::env::temp_dir().join(format!(
            "echo-private-write-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let dir = root.join("private");
        let path = dir.join("store.json");

        write_atomic_private(&path, b"first").unwrap();
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
        write_atomic_private(&path, b"replacement").unwrap();
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read(&path).unwrap(), b"replacement");

        let ordinary = root.join("ordinary");
        fs::create_dir(&ordinary).unwrap();
        fs::set_permissions(&ordinary, fs::Permissions::from_mode(0o755)).unwrap();
        write_atomic(&ordinary.join("output.txt"), b"output").unwrap();
        assert_eq!(
            fs::metadata(&ordinary).unwrap().permissions().mode() & 0o777,
            0o755
        );

        let concurrent = root.join("concurrent");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let writers = (0..8)
            .map(|index| {
                let barrier = std::sync::Arc::clone(&barrier);
                let path = concurrent.join(format!("{index}.txt"));
                std::thread::spawn(move || {
                    barrier.wait();
                    write_atomic_private(&path, b"private")
                })
            })
            .collect::<Vec<_>>();
        for writer in writers {
            writer.join().unwrap().unwrap();
        }
        assert_eq!(
            fs::metadata(&concurrent).unwrap().permissions().mode() & 0o777,
            0o700
        );

        let external = root.join("external");
        fs::create_dir(&external).unwrap();
        let symlink = root.join("private-link");
        std::os::unix::fs::symlink(&external, &symlink).unwrap();
        assert!(ensure_private_dir(&symlink).is_err());
        assert_ne!(
            fs::metadata(&external).unwrap().permissions().mode() & 0o777,
            0o700
        );

        fs::set_permissions(&external, fs::Permissions::from_mode(0o755)).unwrap();
        let external_nested = external.join("nested");
        fs::create_dir(&external_nested).unwrap();
        let intermediate_symlink = root.join("intermediate-link");
        std::os::unix::fs::symlink(&external, &intermediate_symlink).unwrap();
        assert!(ensure_private_dir(&intermediate_symlink.join("nested")).is_err());
        assert_ne!(
            fs::metadata(&external_nested).unwrap().permissions().mode() & 0o777,
            0o700
        );

        let pinned_path = root.join("pinned");
        let pinned = PrivateDir::open(&pinned_path).unwrap();
        let moved_path = root.join("pinned-moved");
        fs::rename(&pinned_path, &moved_path).unwrap();
        std::os::unix::fs::symlink(&external, &pinned_path).unwrap();
        pinned
            .write_atomic("anchored.txt".as_ref(), b"private")
            .unwrap();
        assert_eq!(
            fs::read(moved_path.join("anchored.txt")).unwrap(),
            b"private"
        );
        assert!(!external.join("anchored.txt").exists());

        assert!(pinned.create_new("../outside".as_ref()).is_err());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn absolute_xdg_config_home_wins_over_home() {
        let got = resolve_config_dir(
            None,
            Some(PathBuf::from("/xdg/config")),
            Some(PathBuf::from("/home/tester")),
        )
        .unwrap();
        assert_eq!(got, PathBuf::from("/xdg/config/echo"));
    }

    #[test]
    fn absolute_explicit_directories_win() {
        assert_eq!(
            resolve_data_dir(
                Some(PathBuf::from("/echo/data")),
                Some(PathBuf::from("/xdg/data")),
                Some(PathBuf::from("/home/tester")),
            )
            .unwrap(),
            PathBuf::from("/echo/data")
        );
        assert_eq!(
            resolve_config_dir(
                Some(PathBuf::from("/echo/config")),
                Some(PathBuf::from("/xdg/config")),
                Some(PathBuf::from("/home/tester")),
            )
            .unwrap(),
            PathBuf::from("/echo/config")
        );
    }

    #[test]
    fn invalid_explicit_data_directory_is_a_hard_error() {
        for explicit in [PathBuf::new(), PathBuf::from("relative/echo")] {
            let error = resolve_data_dir(
                Some(explicit),
                Some(PathBuf::from("/valid/xdg")),
                Some(PathBuf::from("/home/tester")),
            )
            .unwrap_err();
            assert!(error.contains("ECHO_DATA_DIR"), "{error}");
            assert!(error.contains("absolute path"), "{error}");
        }
    }

    #[test]
    fn invalid_explicit_config_directory_is_a_hard_error() {
        for explicit in [PathBuf::new(), PathBuf::from("relative/echo")] {
            let error = resolve_config_dir(
                Some(explicit),
                Some(PathBuf::from("/valid/xdg")),
                Some(PathBuf::from("/home/tester")),
            )
            .unwrap_err();
            assert!(error.contains("ECHO_CONFIG_DIR"), "{error}");
            assert!(error.contains("absolute path"), "{error}");
        }
    }

    #[test]
    fn invalid_xdg_values_fall_back_only_to_absolute_home() {
        for xdg in [PathBuf::new(), PathBuf::from("relative/xdg")] {
            assert_eq!(
                resolve_data_dir(None, Some(xdg.clone()), Some(PathBuf::from("/home/tester")),)
                    .unwrap(),
                PathBuf::from("/home/tester/.local/share/echo")
            );
            assert_eq!(
                resolve_config_dir(None, Some(xdg), Some(PathBuf::from("/home/tester")),).unwrap(),
                PathBuf::from("/home/tester/.config/echo")
            );
        }
    }

    #[test]
    fn missing_or_invalid_home_without_absolute_xdg_is_actionable() {
        for home in [
            None,
            Some(PathBuf::new()),
            Some(PathBuf::from("relative/home")),
        ] {
            for xdg in [None, Some(PathBuf::from("relative/xdg"))] {
                let data_error = resolve_data_dir(None, xdg.clone(), home.clone()).unwrap_err();
                assert!(data_error.contains("ECHO_DATA_DIR"), "{data_error}");
                assert!(data_error.contains("absolute path"), "{data_error}");
                assert!(!data_error.contains("/tmp/"), "{data_error}");

                let config_error = resolve_config_dir(None, xdg, home.clone()).unwrap_err();
                assert!(config_error.contains("ECHO_CONFIG_DIR"), "{config_error}");
                assert!(config_error.contains("absolute path"), "{config_error}");
                assert!(!config_error.contains("/tmp/"), "{config_error}");
            }
        }
    }

    #[test]
    fn directory_sync_failure_after_rename_does_not_report_write_failure() {
        let dir =
            std::env::temp_dir().join(format!("echo-atomic-post-rename-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("store.json");
        fs::write(&path, b"old contents").unwrap();

        let result = write_atomic_with_dir_sync(&path, b"replacement contents", |parent| {
            assert_eq!(parent, dir);
            Err(std::io::Error::other("injected directory sync failure"))
        });

        assert!(result.is_ok());
        assert_eq!(fs::read(&path).unwrap(), b"replacement contents");
    }
}
