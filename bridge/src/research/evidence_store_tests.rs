//! Abrupt child exits. The environment/fault controls exist only in this test binary.
use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "tako-s2-crash-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn op(n: u64) -> String {
    format!("{n:032x}")
}
fn call(root: &Path, cmd: &str, args: Value) -> Result<Value> {
    for _ in 0..200 {
        match crate::research::dispatch(root, cmd, args.clone()) {
            Err(e) if e.code == "research_busy" => {
                std::thread::sleep(std::time::Duration::from_millis(2))
            }
            r => return r,
        }
    }
    panic!("persistent Busy")
}
fn create(root: &Path) -> String {
    call(root,"research_project_create",json!({"schema_version":1,"operation_id":op(1),"metadata":{"title":"Crash fixture","primary_type":null,"type_unknown_reason":"test","secondary_types":[]},"goal":"Local crash fixture"})).unwrap()["project_id"].as_str().unwrap().into()
}
fn prepare_args(pid: &str, operation: u64, version: u64) -> Value {
    json!({"schema_version":2,"project_id":pid,"operation_id":op(operation),"expected_project_version":version})
}
fn packet(pid: &str) -> Value {
    json!({"schema_version":2,"project_id":pid,"operation_id":op(3),"expected_project_version":2,"previous":null,"revision_reason":"test","claim":"Test fixture note","stance":"unknown","limitations":[],"origin":"local_input","text":"fixture bytes","artifact":null,"source":null,"source_ref":null,"question":null,"plan_revision":null,"association_reason":"local fixture"})
}
fn child(root: &Path, cmd: &str, args: Value, phase: Option<Phase>) -> std::process::Output {
    let case = json!({"root":root,"cmd":cmd,"args":args,"phase":phase.map(|p|format!("{p:?}"))});
    std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "research::store::v2::tests::abrupt_s2_boundaries_recover_in_fresh_processes",
        ])
        .env_clear()
        .env("TAKO_TEST_S2_CRASH", case.to_string())
        .output()
        .unwrap()
}
#[test]
fn sha256_known_answers_and_streaming_chunk_boundaries() {
    use sha2::{Digest, Sha256};
    let vectors = [
        (
            vec![],
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"abc".to_vec(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq".to_vec(),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
        (
            vec![b'a'; 1_000_000],
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
        ),
    ];
    for (bytes, expected) in vectors {
        assert_eq!(e::digest(&bytes).value, expected);
        for size in [1, 7, 64, 4096] {
            let mut hash = Sha256::new();
            for block in bytes.chunks(size) {
                hash.update(block);
            }
            assert_eq!(
                hash.finalize()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
                expected
            );
        }
    }
}
#[test]
fn abrupt_s2_boundaries_recover_in_fresh_processes() {
    let phases = [
        Phase::S2AfterHeader,
        Phase::S2BeforeTransition,
        Phase::S2AfterIntent,
        Phase::S2AfterBlob,
        Phase::BeforeWrite,
        Phase::BeforePublish,
        Phase::AfterPublish,
        Phase::BeforeAck,
    ];
    if let Ok(raw) = std::env::var("TAKO_TEST_S2_CRASH") {
        let case: Value = serde_json::from_str(&raw).unwrap();
        if let Some(name) = case["phase"].as_str() {
            let phase = *phases.iter().find(|p| format!("{p:?}") == name).unwrap();
            FAULT.with(|fault| fault.set(Some((phase, true))));
        }
        call(
            Path::new(case["root"].as_str().unwrap()),
            case["cmd"].as_str().unwrap(),
            case["args"].clone(),
        )
        .unwrap();
        assert!(case["phase"].is_null(), "fault not reached");
        return;
    }
    for phase in [
        Phase::S2AfterHeader,
        Phase::S2BeforeTransition,
        Phase::BeforeWrite,
        Phase::BeforePublish,
        Phase::AfterPublish,
        Phase::BeforeAck,
    ] {
        let f = Fixture::new();
        let pid = create(&f.0);
        let args = prepare_args(&pid, 2, 1);
        let out = child(
            &f.0,
            "research_prepare_evidence_store",
            args.clone(),
            Some(phase),
        );
        assert_eq!(out.status.code(), Some(93), "{out:?}");
        assert_eq!(
            call(
                &f.0,
                "research_project_get",
                json!({"schema_version":1,"project_id":pid})
            )
            .unwrap()["project"]["meta"]["version"],
            1
        );
        assert_eq!(
            call(
                &f.0,
                "research_operation_status",
                json!({"schema_version":2,"project_id":pid,"operation_id":op(2)})
            )
            .unwrap()["status"],
            "pending_preparation"
        );
        assert!(
            child(&f.0, "research_prepare_evidence_store", args.clone(), None)
                .status
                .success()
        );
        assert_eq!(
            call(&f.0, "research_prepare_evidence_store", args).unwrap()["project_version"],
            2
        );
    }
    for phase in [Phase::AfterPublish, Phase::BeforeAck] {
        let f = Fixture::new();
        let pid = create(&f.0);
        let args = prepare_args(&pid, 2, 1);
        assert_eq!(
            child(
                &f.0,
                "research_prepare_evidence_store",
                args.clone(),
                Some(Phase::S2BeforeTransition)
            )
            .status
            .code(),
            Some(93)
        );
        assert_eq!(
            child(
                &f.0,
                "research_prepare_evidence_store",
                args.clone(),
                Some(phase)
            )
            .status
            .code(),
            Some(93)
        );
        assert_eq!(
            call(
                &f.0,
                "research_project_get",
                json!({"schema_version":1,"project_id":pid})
            )
            .unwrap()["project"]["meta"]["version"],
            2
        );
        assert!(child(&f.0, "research_prepare_evidence_store", args, None)
            .status
            .success());
    }
    for phase in [
        Phase::S2AfterIntent,
        Phase::S2AfterBlob,
        Phase::BeforeWrite,
        Phase::BeforePublish,
        Phase::AfterPublish,
        Phase::BeforeAck,
    ] {
        let f = Fixture::new();
        let pid = create(&f.0);
        call(
            &f.0,
            "research_prepare_evidence_store",
            prepare_args(&pid, 2, 1),
        )
        .unwrap();
        let args = packet(&pid);
        let out = child(&f.0, "research_evidence_record", args.clone(), Some(phase));
        assert_eq!(out.status.code(), Some(93), "{out:?}");
        assert_eq!(
            call(
                &f.0,
                "research_operation_status",
                json!({"schema_version":2,"project_id":pid,"operation_id":op(3)})
            )
            .unwrap()["status"],
            "pending"
        );
        let mut other = args.clone();
        other["operation_id"] = json!(op(4));
        assert_eq!(
            call(&f.0, "research_evidence_record", other)
                .unwrap_err()
                .code,
            "research_pending_operation"
        );
        assert!(child(&f.0, "research_evidence_record", args.clone(), None)
            .status
            .success());
        assert_eq!(
            call(&f.0, "research_evidence_record", args).unwrap()["project_version"],
            3
        );
        assert_eq!(
            call(
                &f.0,
                "research_evidence_list",
                json!({"schema_version":2,"project_id":pid,"cursor":null,"limit":256})
            )
            .unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    for phase in [
        Phase::BeforeWrite,
        Phase::BeforePublish,
        Phase::AfterPublish,
        Phase::BeforeAck,
    ] {
        let f = Fixture::new();
        let pid = create(&f.0);
        call(
            &f.0,
            "research_prepare_evidence_store",
            prepare_args(&pid, 2, 1),
        )
        .unwrap();
        let initial = call(&f.0, "research_evidence_record", packet(&pid)).unwrap();
        let refs = initial["records"].as_array().unwrap();
        let mut args = packet(&pid);
        args["operation_id"] = json!(op(4));
        args["expected_project_version"] = json!(3);
        args["previous"] = refs
            .iter()
            .find(|r| r["kind"] == "evidence")
            .unwrap()
            .clone();
        args["text"] = Value::Null;
        args["artifact"] = refs
            .iter()
            .find(|r| r["kind"] == "artifact")
            .unwrap()
            .clone();
        assert_eq!(
            child(&f.0, "research_evidence_correct", args.clone(), Some(phase))
                .status
                .code(),
            Some(93)
        );
        let expected = if matches!(phase, Phase::AfterPublish | Phase::BeforeAck) {
            "complete"
        } else {
            "pending"
        };
        assert_eq!(
            call(
                &f.0,
                "research_operation_status",
                json!({"schema_version":2,"project_id":pid,"operation_id":op(4)})
            )
            .unwrap()["status"],
            expected
        );
        assert!(child(&f.0, "research_evidence_correct", args.clone(), None)
            .status
            .success());
        assert_eq!(
            call(&f.0, "research_evidence_correct", args).unwrap()["project_version"],
            4
        );
    }
}
#[test]
fn stale_snapshot_and_failed_publication_preserve_prior_state() {
    let f = Fixture::new();
    let pid = create(&f.0);
    let args = prepare_args(&pid, 2, 1);
    assert_eq!(
        child(
            &f.0,
            "research_prepare_evidence_store",
            args.clone(),
            Some(Phase::S2BeforeTransition)
        )
        .status
        .code(),
        Some(93)
    );
    call(&f.0,"research_project_update",json!({"schema_version":1,"project_id":pid,"operation_id":op(8),"expected_project_version":1,"metadata":{"title":"Old writer advanced","primary_type":null,"type_unknown_reason":"test","secondary_types":[]}})).unwrap();
    assert_eq!(
        call(&f.0, "research_prepare_evidence_store", args)
            .unwrap_err()
            .code,
        "research_conflict"
    );
    call(
        &f.0,
        "research_prepare_evidence_store",
        prepare_args(&pid, 9, 2),
    )
    .unwrap();
    let mut p = packet(&pid);
    p["expected_project_version"] = json!(3);
    FAULT.with(|fault| fault.set(Some((Phase::BeforePublish, false))));
    assert_eq!(
        call(&f.0, "research_evidence_record", p.clone())
            .unwrap_err()
            .code,
        "research_storage_limit"
    );
    assert_eq!(
        call(
            &f.0,
            "research_project_get",
            json!({"schema_version":1,"project_id":pid})
        )
        .unwrap()["project"]["metadata"]["title"],
        "Old writer advanced"
    );
    call(&f.0, "research_evidence_record", p).unwrap();
}
