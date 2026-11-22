//! Navigation metadata only. No project, consent, provider or execution authority.
use crate::BridgeError;
use serde::{Deserialize, Serialize};
use std::path::Path;

const NAME: &str = "onboarding-v1.json";
const MAX_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Workspace {
    Engineering,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkspaceArgs {
    pub workspace: Workspace,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum State {
    FirstRun,
    Completed { last_workspace: Workspace },
    Unavailable { reason: Reason },
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reason {
    InvalidData,
    UnsupportedVersion,
    UnsafeFile,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema_version: u64,
    intro_version: u64,
    completed: bool,
    last_workspace: Workspace,
}

fn completed() -> State {
    State::Completed {
        last_workspace: Workspace::Engineering,
    }
}

fn error(code: &str) -> BridgeError {
    let message = match code {
        "onboarding_read_failed" => "Could not read the local start preference.",
        "onboarding_state_unavailable" => "The local start preference cannot be safely updated.",
        _ => "Could not save the local start preference. Local work remains available.",
    };
    BridgeError::new(code, message)
}

#[cfg(unix)]
pub(crate) use native::{complete, state};

#[cfg(not(unix))]
pub(crate) fn state(_: &Path) -> Result<State, BridgeError> {
    Err(error("onboarding_read_failed"))
}
#[cfg(not(unix))]
pub(crate) fn complete(_: &Path) -> Result<State, BridgeError> {
    Err(error("onboarding_write_failed"))
}

#[cfg(unix)]
mod native {
    use super::*;
    use std::ffi::CString;
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Directory(File);

    impl Directory {
        fn open(root: &Path, exclusive: bool) -> io::Result<Self> {
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(root)?;
            // Same-directory readers/publishers cooperate across bridge processes. Holding
            // the descriptor also pins all relative operations to this native trust root.
            if unsafe {
                libc::flock(
                    file.as_raw_fd(),
                    if exclusive {
                        libc::LOCK_EX
                    } else {
                        libc::LOCK_SH
                    },
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(file))
        }

        fn read(&self) -> Result<State, BridgeError> {
            let name = CString::new(NAME).unwrap();
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                let e = io::Error::last_os_error();
                return match e.raw_os_error() {
                    Some(libc::ENOENT) => Ok(State::FirstRun),
                    Some(libc::ELOOP | libc::ENXIO) => Ok(State::Unavailable {
                        reason: Reason::UnsafeFile,
                    }),
                    _ => Err(error("onboarding_read_failed")),
                };
            }
            let mut file = unsafe { File::from_raw_fd(fd) };
            let meta = file
                .metadata()
                .map_err(|_| error("onboarding_read_failed"))?;
            // A crash immediately after link publication can leave our temporary hard
            // link alive. This navigation marker carries no authority; validate its
            // exact bytes instead of mistaking that recoverable state for corruption.
            if !meta.is_file() {
                return Ok(State::Unavailable {
                    reason: Reason::UnsafeFile,
                });
            }
            let invalid = || State::Unavailable {
                reason: Reason::InvalidData,
            };
            if meta.len() > MAX_BYTES as u64 {
                return Ok(invalid());
            }
            let mut bytes = Vec::new();
            (&mut file)
                .take((MAX_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| error("onboarding_read_failed"))?;
            if bytes.len() > MAX_BYTES {
                return Ok(invalid());
            }
            // Deserialize directly from bytes: serde rejects duplicate struct fields,
            // unlike parsing to Value and silently keeping the last duplicate key.
            let record: Record = match serde_json::from_slice(&bytes) {
                Ok(v) => v,
                Err(_) => return Ok(invalid()),
            };
            if record.schema_version != 1 || record.intro_version != 1 {
                return Ok(State::Unavailable {
                    reason: Reason::UnsupportedVersion,
                });
            }
            if !record.completed {
                return Ok(invalid());
            }
            Ok(completed())
        }

        fn sync_record(&self) -> io::Result<()> {
            let name = CString::new(NAME).unwrap();
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let file = unsafe { File::from_raw_fd(fd) };
            if !file.metadata()?.is_file() {
                return Err(io::Error::other("nonregular metadata"));
            }
            file.sync_all()?;
            self.0.sync_all()
        }
    }

    pub(crate) fn state(root: &Path) -> Result<State, BridgeError> {
        Directory::open(root, false)
            .map_err(|_| error("onboarding_read_failed"))?
            .read()
    }

    struct Temporary<'a> {
        dir: &'a Directory,
        name: CString,
        present: bool,
    }
    impl Temporary<'_> {
        fn remove(&mut self) -> io::Result<()> {
            let removed = unsafe { libc::unlinkat(self.dir.0.as_raw_fd(), self.name.as_ptr(), 0) };
            if removed != 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::NotFound {
                    self.present = false;
                }
                return Err(error);
            }
            self.present = false;
            Ok(())
        }
    }
    impl Drop for Temporary<'_> {
        fn drop(&mut self) {
            // Only this exclusively created temporary name; never scan or reset metadata.
            if self.present {
                let _ = self.remove();
            }
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Phase {
        BeforeWrite,
        BeforePublish,
        AfterPublish,
        BeforeSync,
    }

    pub(crate) fn complete(root: &Path) -> Result<State, BridgeError> {
        publish(root, |_| Ok(()))
    }

    fn publish(root: &Path, fault: impl Fn(Phase) -> io::Result<()>) -> Result<State, BridgeError> {
        let dir = Directory::open(root, true).map_err(|_| error("onboarding_write_failed"))?;
        match dir.read()? {
            State::Completed { .. } => {
                // Also acknowledge retry after a publish that succeeded but whose sync failed.
                dir.sync_record()
                    .map_err(|_| error("onboarding_write_failed"))?;
                return Ok(completed());
            }
            State::Unavailable { .. } => return Err(error("onboarding_state_unavailable")),
            State::FirstRun => {}
        }
        let name = CString::new(format!(
            ".onboarding-v1.{}.{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
        .unwrap();
        let fd = unsafe {
            libc::openat(
                dir.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(error("onboarding_write_failed"));
        }
        let mut temporary = Temporary {
            dir: &dir,
            name,
            present: true,
        };
        let mut file = unsafe { File::from_raw_fd(fd) };
        let record = Record {
            schema_version: 1,
            intro_version: 1,
            completed: true,
            last_workspace: Workspace::Engineering,
        };
        let final_name = CString::new(NAME).unwrap();
        let result = (|| -> io::Result<()> {
            fault(Phase::BeforeWrite)?;
            file.write_all(&serde_json::to_vec(&record)?)?;
            file.sync_all()?;
            fault(Phase::BeforePublish)?;
            let linked = unsafe {
                libc::linkat(
                    dir.0.as_raw_fd(),
                    temporary.name.as_ptr(),
                    dir.0.as_raw_fd(),
                    final_name.as_ptr(),
                    0,
                )
            };
            if linked != 0 {
                return Err(io::Error::last_os_error());
            }
            fault(Phase::AfterPublish)?;
            temporary.remove()?;
            fault(Phase::BeforeSync)?;
            dir.0.sync_all()
        })();
        drop(temporary);
        if result.is_err() {
            return Err(error("onboarding_write_failed"));
        }
        Ok(completed())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn noncooperating_publish_race_preserves_the_other_file() {
            let root = std::env::temp_dir().join(format!(
                "tako-onboarding-race-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            let e = publish(&root, |phase| {
                if phase == Phase::BeforePublish {
                    std::fs::write(root.join(NAME), b"foreign metadata")?;
                }
                Ok(())
            })
            .unwrap_err();
            assert_eq!(e.code, "onboarding_write_failed");
            assert_eq!(std::fs::read(root.join(NAME)).unwrap(), b"foreign metadata");
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
            std::fs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn abrupt_process_exit_after_publication_recovers_without_sweeping_orphans() {
            const CHILD_ROOT: &str = "TAKO_TEST_ONBOARDING_CRASH_ROOT";
            if let Some(root) = std::env::var_os(CHILD_ROOT) {
                let _ = publish(Path::new(&root), |phase| {
                    if phase == Phase::AfterPublish {
                        std::process::exit(93); // Deliberately bypass destructors.
                    }
                    Ok(())
                });
                panic!("child did not reach publication");
            }
            let root = std::env::temp_dir().join(format!(
                "tako-onboarding-crash-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "onboarding::native::tests::abrupt_process_exit_after_publication_recovers_without_sweeping_orphans"])
                .env_clear().env(CHILD_ROOT, &root).output().unwrap();
            assert_eq!(child.status.code(), Some(93), "{child:?}");
            assert_eq!(state(&root).unwrap(), completed());
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
            assert_eq!(complete(&root).unwrap(), completed());
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
            std::fs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn failed_writes_and_publish_sync_are_retryable_without_partial_final_json() {
            for phase in [
                Phase::BeforeWrite,
                Phase::BeforePublish,
                Phase::AfterPublish,
                Phase::BeforeSync,
            ] {
                let root = std::env::temp_dir().join(format!(
                    "tako-onboarding-fault-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                std::fs::create_dir(&root).unwrap();
                let e = publish(&root, |p| {
                    if p == phase {
                        Err(io::Error::other("injected private path / sentinel"))
                    } else {
                        Ok(())
                    }
                })
                .unwrap_err();
                assert_eq!(e.code, "onboarding_write_failed");
                assert!(!e.message.contains("sentinel"));
                assert_eq!(
                    state(&root).unwrap(),
                    if matches!(phase, Phase::BeforeWrite | Phase::BeforePublish) {
                        State::FirstRun
                    } else {
                        completed()
                    }
                );
                assert!(std::fs::read_dir(&root).unwrap().all(|e| !e
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")));
                assert_eq!(complete(&root).unwrap(), completed());
                std::fs::remove_dir_all(root).unwrap();
            }
        }
    }
}
