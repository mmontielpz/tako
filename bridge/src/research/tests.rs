//! Test-only faults and real crash processes. Never compiled into runtime controls.
use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "tako-s1-fault-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
// Other parallel library tests fork/exec while descriptors are live. A child can
// briefly retain a CLOEXEC lock before exec. Busy is a supported result, so this
// test caller retries explicitly; a stuck lock still fails within a fixed bound.
fn call(root: &Path, cmd: &str, args: Value) -> Result<Value> {
    for _ in 0..200 {
        match crate::research::dispatch(root, cmd, args.clone()) {
            Err(e) if e.code == "research_busy" => {
                std::thread::sleep(std::time::Duration::from_millis(2))
            }
            result => return result,
        }
    }
    Err(error("busy"))
}
fn op(n: u64) -> String {
    format!("{n:032x}")
}
fn create_args(n: u64) -> Value {
    serde_json::json!({"schema_version":1,"operation_id":op(n),"metadata":{"title":"Research","primary_type":null,"type_unknown_reason":"unknown","secondary_types":[]},"goal":"Question?"})
}
fn update_args(pid: &str) -> Value {
    serde_json::json!({"schema_version":1,"operation_id":op(9),"project_id":pid,"expected_project_version":1,"metadata":{"title":"Changed","primary_type":null,"type_unknown_reason":"unknown","secondary_types":[]}})
}
fn project_dir(root: &Path, pid: &str) -> PathBuf {
    let owner: Value =
        serde_json::from_slice(&std::fs::read(root.join("research/v1/owner-v1.json")).unwrap())
            .unwrap();
    root.join("research/v1")
        .join(owner["owner_id"].as_str().unwrap())
        .join(pid)
}

#[test]
fn cj1_vectors_duplicate_keys_integer_limits_depth_and_unicode() {
    let v = json::decode(br#"{"z":-0,"a":"\u00e9\ud83d\udc19\u007f\n"}"#).unwrap();
    assert_eq!(
        json::canonical(&v).unwrap(),
        r#"{"a":"\u00e9\ud83d\udc19\u007f\n","z":0}"#
    );
    for bytes in [
        br#"{"x":1,"x":2}"#.as_slice(),
        br#"{"x":1e0}"#,
        br#"{"x":-0.0}"#,
        br#"{"x":9007199254740992}"#,
        br#"{"x":"\ud800"}"#,
        br#"{"\u00e9":1}"#,
    ] {
        assert!(json::decode(bytes).is_err(), "{:?}", bytes)
    }
    let mut v = Value::Null;
    for _ in 0..34 {
        v = serde_json::json!([v]);
    }
    assert!(json::canonical(&v).is_err());
}

#[test]
fn injected_write_failures_preserve_previous_commits_and_reconcile_retries() {
    for phase in [
        Phase::BeforeWrite,
        Phase::BeforePublish,
        Phase::AfterPublish,
        Phase::BeforeAck,
    ] {
        let f = Fixture::new();
        let a = call(&f.0, "research_project_create", create_args(1)).unwrap();
        let pid = a["project_id"].as_str().unwrap();
        let dir = project_dir(&f.0, pid);
        let first = std::fs::read(dir.join("commits/00000000000000000001.json")).unwrap();
        FAULT.with(|v| v.set(Some((phase, false))));
        let e = call(&f.0, "research_project_update", update_args(pid)).unwrap_err();
        assert_eq!(e.code, "research_storage_limit");
        let read = call(
            &f.0,
            "research_project_get",
            serde_json::json!({"schema_version":1,"project_id":pid}),
        )
        .unwrap();
        assert_eq!(
            read["project"]["meta"]["version"],
            if matches!(phase, Phase::AfterPublish | Phase::BeforeAck) {
                2
            } else {
                1
            }
        );
        let r = call(&f.0, "research_project_update", update_args(pid)).unwrap();
        assert_eq!(r["project_version"], 2);
        assert_eq!(
            call(&f.0, "research_project_update", update_args(pid)).unwrap(),
            r
        );
        assert_eq!(
            std::fs::read(dir.join("commits/00000000000000000001.json")).unwrap(),
            first
        );
        if matches!(phase, Phase::BeforeWrite | Phase::BeforePublish) {
            assert!(read["recovery"]["orphan_files"].as_u64().unwrap() > 0)
        }
    }
}

#[test]
fn abrupt_exit_at_publication_boundaries_recovers_in_fresh_processes() {
    const ENV: &str = "TAKO_TEST_S1_CRASH";
    let phases = [
        Phase::AfterReservation,
        Phase::BeforeWrite,
        Phase::BeforePublish,
        Phase::AfterPublish,
        Phase::BeforeAck,
    ];
    if let Ok(raw) = std::env::var(ENV) {
        let (phase, root) = raw.split_once('|').unwrap();
        let phase = phases[phase.parse::<usize>().unwrap()];
        let root = Path::new(root);
        FAULT.with(|v| v.set(Some((phase, true))));
        if phase == Phase::AfterReservation {
            let _ = call(root, "research_project_create", create_args(2));
        } else {
            let list = call(
                root,
                "research_project_list",
                serde_json::json!({"schema_version":1,"cursor":null,"limit":256}),
            )
            .unwrap();
            let pid = list["items"][0]["project_id"].as_str().unwrap();
            let _ = call(root, "research_project_update", update_args(pid));
        }
        panic!("fault was not reached");
    }
    for (n, phase) in phases.into_iter().enumerate() {
        let f = Fixture::new();
        let a = call(&f.0, "research_project_create", create_args(1)).unwrap();
        let pid = a["project_id"].as_str().unwrap();
        let out=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","research::store::tests::abrupt_exit_at_publication_boundaries_recovers_in_fresh_processes"])
            .env_clear().env(ENV,format!("{n}|{}",f.0.display())).output().unwrap();
        assert_eq!(out.status.code(), Some(93), "{out:?}");
        if phase == Phase::AfterReservation {
            let status = call(
                &f.0,
                "research_operation_status",
                serde_json::json!({"schema_version":1,"project_id":null,"operation_id":op(2)}),
            )
            .unwrap();
            assert_eq!(status["status"], "pending");
            let reserved = status["project_id"].clone();
            let page = call(
                &f.0,
                "research_project_list",
                serde_json::json!({"schema_version":1,"cursor":null,"limit":256}),
            )
            .unwrap();
            assert!(page["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["status"] == "pending_creation"));
            let pending_pid = reserved.as_str().unwrap();
            assert!(!project_dir(&f.0, pending_pid).exists());
            let e = call(
                &f.0,
                "research_project_get",
                serde_json::json!({"schema_version":1,"project_id":pending_pid}),
            )
            .unwrap_err();
            assert_eq!(e.code, "research_incomplete_creation");
            assert!(!project_dir(&f.0, pending_pid).exists());
            let completed = std::thread::scope(|s| {
                let reader = s.spawn(|| {
                    for _ in 0..20 {
                        let page = call(
                            &f.0,
                            "research_project_list",
                            serde_json::json!({"schema_version":1,"cursor":null,"limit":256}),
                        )
                        .unwrap();
                        let rows: Vec<_> = page["items"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter(|r| r["project_id"] == pending_pid)
                            .collect();
                        assert_eq!(rows.len(), 1);
                        assert!(
                            rows[0]["status"] == "pending_creation"
                                || rows[0]["status"] == "available"
                        );
                    }
                });
                let writer =
                    s.spawn(|| call(&f.0, "research_project_create", create_args(2)).unwrap());
                reader.join().unwrap();
                writer.join().unwrap()
            });
            assert_eq!(completed["project_id"], reserved);
            assert_eq!(
                call(&f.0, "research_project_create", create_args(2)).unwrap(),
                completed
            );
        } else {
            let p = call(
                &f.0,
                "research_project_get",
                serde_json::json!({"schema_version":1,"project_id":pid}),
            )
            .unwrap();
            assert_eq!(
                p["project"]["meta"]["version"],
                if matches!(phase, Phase::AfterPublish | Phase::BeforeAck) {
                    2
                } else {
                    1
                }
            );
            assert_eq!(
                call(&f.0, "research_project_update", update_args(pid)).unwrap()["project_version"],
                2
            );
        }
    }
}

#[test]
fn quota_exhaustion_blocks_writes_but_preserves_inspection_and_orphan_bytes() {
    use std::os::unix::fs::OpenOptionsExt;
    let f = Fixture::new();
    let a = call(&f.0, "research_project_create", create_args(1)).unwrap();
    let pid = a["project_id"].as_str().unwrap();
    let dir = project_dir(&f.0, pid);
    for i in 0..16 {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(dir.join("blobs").join(format!("orphan-{i}")))
            .unwrap();
        file.set_len(32 * 1024 * 1024).unwrap();
    }
    let e = call(&f.0, "research_project_update", update_args(pid)).unwrap_err();
    assert_eq!(e.code, "research_storage_limit");
    let read = call(
        &f.0,
        "research_project_get",
        serde_json::json!({"schema_version":1,"project_id":pid}),
    )
    .unwrap();
    assert_eq!(read["project"]["meta"]["version"], 1);
    assert_eq!(read["recovery"]["orphan_files"], 16);
    assert_eq!(std::fs::read_dir(dir.join("blobs")).unwrap().count(), 16);
}

#[test]
fn publication_cannot_replace_existing_file_and_unsupported_files_stay_untouched() {
    let f = Fixture::new();
    let root = Dir::root(&f.0).unwrap();
    let d = root.child("private", true).unwrap();
    d.publish("record", b"original", Role::Metadata).unwrap();
    assert!(d.publish("record", b"replacement", Role::Metadata).is_err());
    assert_eq!(d.bytes("record", 100).unwrap(), b"original");
}

#[test]
fn unavailable_clock_is_explicit_and_does_not_disrupt_sequence_order() {
    let st = State::default();
    let mut s = seeds(&st).unwrap();
    s.clock = Clock {
        at_ms: None,
        reason: Some("clock unavailable".into()),
    };
    let m = Mutation::Create(state::parse(create_args(1)).unwrap());
    let commit = state::build(&native_id().unwrap(), &native_id().unwrap(), &st, &m, s).unwrap();
    let Record::Project(p) = &commit.records[0] else {
        panic!("project")
    };
    assert_eq!(p.meta.created_at_ms, None);
    assert_eq!(p.meta.clock_status, "uncertain");
    assert_eq!(commit.sequence, 1);
}

// Keep a real forked child before exec while it holds inherited CLOEXEC descriptors.
// The handshake, rather than scheduler timing or sleeps, fixes the reproduction order.
struct ForkPin {
    pid: libc::pid_t,
    _socket: std::os::unix::net::UnixStream,
}
impl ForkPin {
    fn new(drop_in_child: Option<*mut Dir>) -> Self {
        use std::io::Read;
        use std::os::fd::AsRawFd;
        let (mut parent, child) = std::os::unix::net::UnixStream::pair().unwrap();
        parent
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap(); // Deadlock watchdog only; not a contention retry or sleep.
        let parent_fd = parent.as_raw_fd();
        let child_fd = child.as_raw_fd();
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0);
        if pid == 0 {
            // Only stack operations and async-signal-safe syscalls before _exit.
            unsafe {
                libc::close(parent_fd);
                if let Some(dir) = drop_in_child {
                    // Dir contains only a descriptor and scalar lock ownership.
                    // Its destructor must not unlock a different process's request.
                    std::ptr::drop_in_place(dir);
                }
                if libc::write(child_fd, b"R".as_ptr().cast(), 1) != 1 {
                    libc::_exit(111);
                }
                let mut byte = 0u8;
                loop {
                    let n = libc::read(child_fd, (&mut byte as *mut u8).cast(), 1);
                    if n >= 0 || *libc::__errno_location() != libc::EINTR {
                        break;
                    }
                }
                libc::_exit(0);
            }
        }
        let pin = Self {
            pid,
            _socket: parent.try_clone().unwrap(),
        };
        drop(child);
        let mut ready = [0u8];
        parent.read_exact(&mut ready).unwrap();
        assert_eq!(ready, [b'R']);
        pin
    }
}
impl Drop for ForkPin {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.pid, libc::SIGKILL);
            let mut status = 0;
            while libc::waitpid(self.pid, &mut status, 0) < 0 {
                if *libc::__errno_location() != libc::EINTR {
                    break;
                }
            }
        }
    }
}

#[test]
fn request_lock_release_is_not_extended_by_unrelated_fork() {
    for exclusive in [true, false] {
        let f = Fixture::new();
        let made = call(&f.0, "research_project_create", create_args(1)).unwrap();
        let pid = made["project_id"].as_str().unwrap();
        let query = serde_json::json!({"schema_version":1,"project_id":pid});
        let held = Context::open(&f.0, false).unwrap();
        held._v1.lock(exclusive).unwrap();
        let (held_project, _, _) = held.ready(pid, exclusive).unwrap();
        assert_ne!(
            unsafe { libc::fcntl(held._v1.0.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        let pin = ForkPin::new(None);
        if exclusive {
            assert_eq!(
                crate::research::dispatch(&f.0, "research_project_get", query.clone())
                    .unwrap_err()
                    .code,
                "research_busy"
            );
        }
        assert_eq!(
            crate::research::dispatch(&f.0, "research_project_update", update_args(pid))
                .unwrap_err()
                .code,
            "research_busy"
        );
        drop(held_project);
        drop(held);
        // Child still holds the duplicated open description. Request completion
        // must release our lock without waiting for that unrelated child to exec.
        crate::research::dispatch(&f.0, "research_project_get", query).unwrap();
        crate::research::dispatch(&f.0, "research_project_update", update_args(pid)).unwrap();
        drop(pin);
    }
}

#[test]
fn fork_child_drop_cannot_unlock_parent_request() {
    let f = Fixture::new();
    let made = call(&f.0, "research_project_create", create_args(1)).unwrap();
    let pid = made["project_id"].as_str().unwrap();
    let mut held = Context::open(&f.0, false).unwrap();
    held._v1.lock(true).unwrap();
    let pin = ForkPin::new(Some(&mut held._v1));
    assert_eq!(
        crate::research::dispatch(&f.0, "research_project_update", update_args(pid))
            .unwrap_err()
            .code,
        "research_busy"
    );
    drop(held);
    crate::research::dispatch(&f.0, "research_project_update", update_args(pid)).unwrap();
    drop(pin);
}

#[test]
fn dropping_one_shared_request_preserves_the_other_reader_lock() {
    let f = Fixture::new();
    let made = call(&f.0, "research_project_create", create_args(1)).unwrap();
    let pid = made["project_id"].as_str().unwrap();
    let first = Context::open(&f.0, false).unwrap();
    let (first_project, _, _) = first.ready(pid, false).unwrap();
    let second = Context::open(&f.0, false).unwrap();
    let (second_project, _, _) = second.ready(pid, false).unwrap();
    drop(first_project);
    drop(first);
    assert_eq!(
        crate::research::dispatch(&f.0, "research_project_update", update_args(pid))
            .unwrap_err()
            .code,
        "research_busy"
    );
    drop(second_project);
    drop(second);
    crate::research::dispatch(&f.0, "research_project_update", update_args(pid)).unwrap();
}
