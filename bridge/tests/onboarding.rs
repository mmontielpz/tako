//! Real native dispatch and disposable filesystem; no provider or T connections.
#![cfg(unix)]
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tako_bridge::Bridge;

static NEXT: AtomicU64 = AtomicU64::new(0);
const NAME: &str = "onboarding-v1.json";
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "tako-onboarding-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn bridge(&self) -> Bridge {
        Bridge::open(self.0.join("data")).unwrap()
    }
    fn marker(&self) -> PathBuf {
        self.0.join("data").join(NAME)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn state(b: &Bridge) -> Value {
    b.dispatch("onboarding_state", Value::Null).unwrap()
}
fn open(b: &Bridge) {
    assert_eq!(
        b.dispatch(
            "onboarding_open_workspace",
            json!({"workspace":"engineering"})
        )
        .unwrap(),
        json!({"workspace":"engineering","opened":true})
    );
}
fn complete(b: &Bridge) -> Value {
    b.dispatch("onboarding_complete", json!({"workspace":"engineering"}))
        .unwrap()
}
fn completed() -> Value {
    json!({"status":"completed","last_workspace":"engineering"})
}
fn record() -> Value {
    json!({"schema_version":1,"intro_version":1,"completed":true,"last_workspace":"engineering"})
}

#[test]
fn missing_reads_and_native_open_do_not_complete_or_create_a_project() {
    let f = Fixture::new();
    let b = f.bridge();
    let before: Vec<_> = fs::read_dir(f.0.join("data"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(state(&b), json!({"status":"first_run"}));
    assert_eq!(state(&b), json!({"status":"first_run"}));
    assert_eq!(
        b.dispatch("onboarding_complete", json!({"workspace":"engineering"}))
            .unwrap_err()
            .code,
        "onboarding_workspace_not_open"
    );
    open(&b);
    assert_eq!(state(&b), json!({"status":"first_run"}));
    assert!(!f.marker().exists());
    let after: Vec<_> = fs::read_dir(f.0.join("data"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(before, after);
    assert_eq!(
        b.dispatch("integration_status", Value::Null).unwrap()["availability"],
        "disabled"
    );
}

#[test]
fn completion_is_private_exact_idempotent_and_persists_across_bridge_reopen() {
    let f = Fixture::new();
    let b = f.bridge();
    open(&b);
    assert_eq!(complete(&b), completed());
    let bytes = fs::read(f.marker()).unwrap();
    let meta = fs::metadata(f.marker()).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), record());
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    assert_eq!(complete(&b), completed());
    assert_eq!(meta.ino(), fs::metadata(f.marker()).unwrap().ino());
    assert_eq!(bytes, fs::read(f.marker()).unwrap());
    drop(b);
    let reopened = f.bridge();
    assert_eq!(state(&reopened), completed());
    // Persistence survives, but opening in the prior session is not a fresh open.
    assert_eq!(
        reopened
            .dispatch("onboarding_complete", json!({"workspace":"engineering"}))
            .unwrap_err()
            .code,
        "onboarding_workspace_not_open"
    );
    open(&reopened);
    assert_eq!(complete(&reopened), completed());
}

#[test]
fn incomplete_session_restarts_first_run_and_does_not_sweep_orphan_files() {
    let f = Fixture::new();
    let b = f.bridge();
    open(&b);
    drop(b);
    let orphan = f.0.join("data/.onboarding-v1.old.tmp");
    fs::write(&orphan, b"partial").unwrap();
    let b = f.bridge();
    assert_eq!(state(&b), json!({"status":"first_run"}));
    assert_eq!(fs::read(&orphan).unwrap(), b"partial");
    assert!(!f.marker().exists());
}

#[test]
fn app_data_roots_are_isolated() {
    let a = Fixture::new();
    let b = Fixture::new();
    let ba = a.bridge();
    let bb = b.bridge();
    open(&ba);
    complete(&ba);
    assert_eq!(state(&bb), json!({"status":"first_run"}));
    assert!(!b.marker().exists());
    assert_eq!(
        bb.dispatch("onboarding_complete", json!({"workspace":"engineering"}))
            .unwrap_err()
            .code,
        "onboarding_workspace_not_open"
    );
}

#[test]
fn invalid_arguments_and_research_never_open_or_complete() {
    let f = Fixture::new();
    let b = f.bridge();
    for args in [
        Value::Null,
        json!({}),
        json!({"workspace":"research"}),
        json!({"workspace":"unknown"}),
        json!({"workspace":true}),
        json!({"workspace":"engineering","path":"../sensitive"}),
        json!({"workspace":"engineering","completed":true}),
    ] {
        for cmd in ["onboarding_open_workspace", "onboarding_complete"] {
            let e = b.dispatch(cmd, args.clone()).unwrap_err();
            assert_eq!(e.code, "invalid_input");
            assert!(!e.message.contains("sensitive"));
            assert!(e.details.is_empty());
        }
    }
    for args in [json!({}), json!({"path":"../sensitive"}), json!(true)] {
        assert_eq!(
            b.dispatch("onboarding_state", args).unwrap_err().code,
            "invalid_input"
        );
    }
    assert!(!f.marker().exists());
    assert_eq!(
        b.dispatch("onboarding_complete", json!({"workspace":"engineering"}))
            .unwrap_err()
            .code,
        "onboarding_workspace_not_open"
    );
}

#[test]
fn malformed_oversize_duplicate_and_incomplete_metadata_are_preserved() {
    let f = Fixture::new();
    let b = f.bridge();
    open(&b);
    let mut cases = vec![
        b"{".to_vec(),
        vec![0xff],
        vec![b' '; 4097],
        b"null".to_vec(),
    ];
    for value in [
        json!({}),
        json!({"schema_version":1,"intro_version":1,"completed":false,"last_workspace":"engineering"}),
        json!({"schema_version":1,"intro_version":1,"completed":true,"last_workspace":"research"}),
        json!({"schema_version":1,"intro_version":1,"completed":"true","last_workspace":"engineering"}),
    ] {
        cases.push(serde_json::to_vec(&value).unwrap());
    }
    let mut extra = record();
    extra["secret"] = json!("PRIVATE_SENTINEL");
    cases.push(serde_json::to_vec(&extra).unwrap());
    cases.push(br#"{"schema_version":1,"intro_version":1,"completed":false,"completed":true,"last_workspace":"engineering"}"#.to_vec());
    for bytes in cases {
        fs::write(f.marker(), &bytes).unwrap();
        assert_eq!(
            state(&b),
            json!({"status":"unavailable","reason":"invalid_data"})
        );
        let e = b
            .dispatch("onboarding_complete", json!({"workspace":"engineering"}))
            .unwrap_err();
        assert_eq!(e.code, "onboarding_state_unavailable");
        assert!(!e.message.contains("PRIVATE_SENTINEL"));
        assert_eq!(fs::read(f.marker()).unwrap(), bytes);
        assert_eq!(
            b.dispatch("startup_report", Value::Null).unwrap()["busy"],
            json!([])
        );
    }
}

#[test]
fn version_refusal_and_exact_size_boundary() {
    let f = Fixture::new();
    let b = f.bridge();
    open(&b);
    for key in ["schema_version", "intro_version"] {
        let mut v = record();
        v[key] = json!(2);
        let bytes = serde_json::to_vec(&v).unwrap();
        fs::write(f.marker(), &bytes).unwrap();
        assert_eq!(
            state(&b),
            json!({"status":"unavailable","reason":"unsupported_version"})
        );
        assert_eq!(
            b.dispatch("onboarding_complete", json!({"workspace":"engineering"}))
                .unwrap_err()
                .code,
            "onboarding_state_unavailable"
        );
        assert_eq!(fs::read(f.marker()).unwrap(), bytes);
    }
    let mut bytes = serde_json::to_vec(&record()).unwrap();
    bytes.resize(4096, b' ');
    fs::write(f.marker(), bytes).unwrap();
    assert_eq!(state(&b), completed());
}

#[test]
fn symlink_dangling_symlink_directory_and_fifo_are_refused_without_following() {
    let f = Fixture::new();
    let b = f.bridge();
    open(&b);
    let target = f.0.join("sentinel");
    fs::write(&target, b"PRIVATE_SENTINEL").unwrap();
    for target in [&target, &f.0.join("missing")] {
        symlink(target, f.marker()).unwrap();
        assert_eq!(
            state(&b),
            json!({"status":"unavailable","reason":"unsafe_file"})
        );
        assert_eq!(
            b.dispatch("onboarding_complete", json!({"workspace":"engineering"}))
                .unwrap_err()
                .code,
            "onboarding_state_unavailable"
        );
        assert!(fs::symlink_metadata(f.marker())
            .unwrap()
            .file_type()
            .is_symlink());
        fs::remove_file(f.marker()).unwrap();
    }
    assert_eq!(fs::read(&target).unwrap(), b"PRIVATE_SENTINEL");
    fs::create_dir(f.marker()).unwrap();
    assert_eq!(
        state(&b),
        json!({"status":"unavailable","reason":"unsafe_file"})
    );
    fs::remove_dir(f.marker()).unwrap();
    let path = std::ffi::CString::new(f.marker().as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    assert_eq!(
        state(&b),
        json!({"status":"unavailable","reason":"unsafe_file"})
    );
}

#[test]
fn concurrent_bridge_instances_publish_one_complete_record() {
    let f = Fixture::new();
    let bridges: Vec<_> = (0..8).map(|_| f.bridge()).collect();
    let handles: Vec<_> = bridges
        .into_iter()
        .map(|b| {
            std::thread::spawn(move || {
                open(&b);
                complete(&b)
            })
        })
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap(), completed());
    }
    assert_eq!(state(&f.bridge()), completed());
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(f.marker()).unwrap()).unwrap(),
        record()
    );
    assert!(fs::read_dir(f.0.join("data")).unwrap().all(|e| !e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".tmp")));
}

#[test]
fn failed_metadata_io_is_redacted_and_does_not_invalidate_cached_local_core() {
    let f = Fixture::new();
    let b = f.bridge();
    open(&b);
    fs::rename(f.0.join("data"), f.0.join("saved-data")).unwrap();
    fs::write(f.0.join("data"), b"PRIVATE_SENTINEL").unwrap();
    for (cmd, args, code) in [
        ("onboarding_state", Value::Null, "onboarding_read_failed"),
        (
            "onboarding_complete",
            json!({"workspace":"engineering"}),
            "onboarding_write_failed",
        ),
    ] {
        let e = b.dispatch(cmd, args).unwrap_err();
        assert_eq!(e.code, code);
        assert!(!e.message.contains("PRIVATE_SENTINEL"));
        assert!(e.details.is_empty());
    }
    assert_eq!(
        b.dispatch("startup_report", Value::Null).unwrap()["recovered"],
        json!([])
    );
    assert_eq!(
        b.dispatch("integration_status", Value::Null).unwrap()["availability"],
        "disabled"
    );
}

#[test]
fn failed_native_workspace_entry_does_not_enable_completion() {
    let f = Fixture::new();
    let b = f.bridge();
    fs::rename(f.0.join("data/missions"), f.0.join("saved-missions")).unwrap();
    fs::write(f.0.join("data/missions"), b"PRIVATE_SENTINEL").unwrap();
    let e = b
        .dispatch(
            "onboarding_open_workspace",
            json!({"workspace":"engineering"}),
        )
        .unwrap_err();
    assert_eq!(e.code, "onboarding_workspace_open_failed");
    assert!(!e.message.contains("PRIVATE_SENTINEL"));
    assert!(e.details.is_empty());
    assert_eq!(
        b.dispatch("onboarding_complete", json!({"workspace":"engineering"}))
            .unwrap_err()
            .code,
        "onboarding_workspace_not_open"
    );
    assert!(!f.marker().exists());
}
