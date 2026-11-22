//! Native S2 fixtures only; no providers, research engine or fabricated science.
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tako_bridge::{Bridge, BridgeError};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "tako-s2-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn bridge(&self) -> Bridge {
        Bridge::open(&self.0).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn call(b: &Bridge, cmd: &str, args: Value) -> Result<Value, BridgeError> {
    for _ in 0..200 {
        match b.dispatch(cmd, args.clone()) {
            Err(e) if e.code == "research_busy" => {
                std::thread::sleep(std::time::Duration::from_millis(2))
            }
            r => return r,
        }
    }
    panic!("persistent Busy")
}
fn op(n: u64) -> String {
    format!("{n:032x}")
}
fn create(b: &Bridge) -> String {
    call(b,"research_project_create",json!({"schema_version":1,"operation_id":op(1),"metadata":{"title":"Local test fixture","primary_type":null,"type_unknown_reason":"fixture","secondary_types":[]},"goal":"Local fixture; no research performed"})).unwrap()["project_id"].as_str().unwrap().into()
}
fn prepare(b: &Bridge, pid: &str) -> Value {
    call(b,"research_prepare_evidence_store",json!({"schema_version":2,"project_id":pid,"operation_id":op(2),"expected_project_version":1})).unwrap()
}
fn packet(pid: &str, n: u64, version: u64) -> Value {
    json!({"schema_version":2,"project_id":pid,"operation_id":op(n),"expected_project_version":version,
    "previous":null,"revision_reason":"fixture registration","claim":"Supplied fixture note","stance":"unknown","limitations":["No scientific conclusion"],"origin":"local_input",
    "text":"hello world","artifact":null,"source":{"title":"Supplied metadata fixture","authors":[],"date":null,"url":"https://invalid.example/never-fetch","doi":null,"attribution":"Reported by test author","previous":null},
    "source_ref":null,"question":null,"plan_revision":null,"association_reason":"project-only fixture"})
}
fn reference(result: &Value, kind: &str) -> Value {
    result["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == kind)
        .unwrap()
        .clone()
}
fn get(b: &Bridge, pid: &str, r: Value) -> Value {
    call(
        b,
        "research_record_get",
        json!({"schema_version":2,"project_id":pid,"reference":r}),
    )
    .unwrap()
}
fn project(root: &Path, pid: &str) -> PathBuf {
    let owner: Value =
        serde_json::from_slice(&fs::read(root.join("research/v1/owner-v1.json")).unwrap()).unwrap();
    root.join("research/v1")
        .join(owner["owner_id"].as_str().unwrap())
        .join(pid)
}
fn snapshot(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, p: &Path, out: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(p).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out)
            } else {
                out.insert(p.strip_prefix(root).unwrap().into(), fs::read(p).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(root, root, &mut out);
    out
}
#[test]
fn records_sources_integrity_restart_and_s1_edits_after_transition() {
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b);
    let old = call(
        &b,
        "research_project_get",
        json!({"schema_version":1,"project_id":pid}),
    )
    .unwrap();
    let q = old["project"]["active_question"].clone();
    let initial = snapshot(&f.0);
    let prepared = prepare(&b, &pid);
    assert_eq!(prepared["project_version"], 2);
    assert_eq!(prepare(&b, &pid), prepared);
    let p = packet(&pid, 3, 2);
    let saved = call(&b, "research_evidence_record", p.clone()).unwrap();
    assert_eq!(saved["project_version"], 3);
    assert_eq!(call(&b, "research_evidence_record", p).unwrap(), saved);
    let evidence = get(&b, &pid, reference(&saved, "evidence"));
    assert_eq!(evidence["record"]["scientific_assessment"], "not_assessed");
    assert_eq!(evidence["inspection"]["integrity"], "matches");
    let artifact = get(&b, &pid, reference(&saved, "artifact"));
    assert_eq!(
        artifact["record"]["digest"]["value"],
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
    let text = call(
        &b,
        "research_artifact_text_get",
        json!({"schema_version":2,"project_id":pid,"reference":reference(&saved,"artifact")}),
    )
    .unwrap();
    assert_eq!(text["text"], "hello world");
    let update = json!({"schema_version":1,"project_id":pid,"operation_id":op(4),"expected_project_version":3,"metadata":{"title":"Changed after transition","primary_type":null,"type_unknown_reason":"fixture","secondary_types":[]}});
    let changed = call(&b, "research_project_update", update.clone()).unwrap();
    assert_eq!(changed["project_version"], 4);
    assert_eq!(
        call(&b, "research_project_update", update).unwrap(),
        changed
    );
    let question=call(&b,"research_question_save",json!({"schema_version":1,"project_id":pid,"operation_id":op(5),"expected_project_version":4,"previous":q,"body":{"question":"Revised fixture question","objectives":[],"hypotheses":[],"evaluation_questions":[],"constraints":[]},"revision_reason":"after evidence"})).unwrap();
    assert_eq!(question["project_version"], 5);
    drop(b);
    let b = f.bridge();
    assert_eq!(get(&b, &pid, reference(&saved, "evidence")), evidence);
    assert_eq!(
        call(
            &b,
            "research_project_get",
            json!({"schema_version":1,"project_id":pid})
        )
        .unwrap()["project"]["metadata"]["title"],
        "Changed after transition"
    );
    let now = snapshot(&f.0);
    for (path, bytes) in initial {
        assert_eq!(now[&path], bytes, "legacy bytes changed: {path:?}");
    }
    let events = call(
        &b,
        "research_events_page",
        json!({"schema_version":2,"project_id":pid,"cursor":null,"limit":256}),
    )
    .unwrap();
    assert_eq!(events["items"].as_array().unwrap().len(), 5);
    for (i, event) in events["items"].as_array().unwrap().iter().enumerate() {
        assert_eq!(event["project_sequence"], i + 1);
        assert_eq!(event["event_index"], 0);
    }
}
#[test]
fn corrected_sources_and_artifacts_preserve_history_and_mark_dependencies() {
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b);
    prepare(&b, &pid);
    let first = call(&b, "research_evidence_record", packet(&pid, 3, 2)).unwrap();
    let old = get(&b, &pid, reference(&first, "evidence"));
    let mut p = packet(&pid, 4, 3);
    p["previous"] = reference(&first, "evidence");
    p["source"]["previous"] = reference(&first, "source");
    p["source"]["title"] = json!("Corrected supplied metadata");
    p["text"] = json!("changed text\r\n");
    let second = call(&b, "research_evidence_correct", p).unwrap();
    let old_now = get(&b, &pid, reference(&first, "evidence"));
    assert_eq!(old_now["record"], old["record"]);
    assert_eq!(old_now["inspection"]["review_state"], "needs_review");
    let corrected = get(&b, &pid, reference(&second, "evidence"));
    assert_eq!(corrected["record"]["meta"]["version"], 2);
    assert_eq!(corrected["inspection"]["integrity"], "matches");
    assert_ne!(
        reference(&first, "artifact")["id"],
        reference(&second, "artifact")["id"]
    );
    let mut p = packet(&pid, 5, 4);
    p["previous"] = reference(&second, "evidence");
    p["text"] = Value::Null;
    p["artifact"] = reference(&second, "artifact");
    p["source"] = Value::Null;
    p["source_ref"] = reference(&second, "source");
    let third = call(&b, "research_evidence_correct", p).unwrap();
    assert_eq!(
        get(&b, &pid, reference(&third, "evidence"))["inspection"]["integrity"],
        "matches"
    );
}
#[test]
fn missing_tampered_material_is_explicit_and_never_reconstructed() {
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b);
    prepare(&b, &pid);
    let p = packet(&pid, 3, 2);
    let saved = call(&b, "research_evidence_record", p.clone()).unwrap();
    let rf = reference(&saved, "artifact");
    let blob = project(&f.0, &pid)
        .join("blobs")
        .join(rf["id"].as_str().unwrap());
    fs::write(&blob, b"tampered").unwrap();
    let before = snapshot(&f.0);
    assert_eq!(
        get(&b, &pid, reference(&saved, "evidence"))["inspection"]["integrity"],
        "mismatch"
    );
    assert_eq!(
        call(
            &b,
            "research_artifact_text_get",
            json!({"schema_version":2,"project_id":pid,"reference":rf})
        )
        .unwrap_err()
        .code,
        "research_integrity_mismatch"
    );
    assert_eq!(
        call(&b, "research_evidence_record", p.clone())
            .unwrap_err()
            .code,
        "research_integrity_mismatch"
    );
    assert_eq!(snapshot(&f.0), before);
    fs::rename(&blob, f.0.join("quarantined-text")).unwrap();
    let before = snapshot(&f.0);
    drop(b);
    let b = f.bridge();
    assert_eq!(
        get(&b, &pid, reference(&saved, "evidence"))["inspection"]["availability"],
        "unavailable"
    );
    assert_eq!(
        call(&b, "research_evidence_record", p).unwrap_err().code,
        "research_missing_evidence"
    );
    assert!(!blob.exists());
    assert_eq!(snapshot(&f.0), before);
}
#[test]
fn schema2_requires_explicit_preparation_and_rejects_unknown_authority_and_cross_project_refs() {
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b);
    let before = snapshot(&f.0);
    assert_eq!(
        call(&b, "research_evidence_record", packet(&pid, 3, 1))
            .unwrap_err()
            .code,
        "research_unsupported_schema"
    );
    assert_eq!(snapshot(&f.0), before);
    prepare(&b, &pid);
    let saved = call(&b, "research_evidence_record", packet(&pid, 3, 2)).unwrap();
    for (field, value) in [
        ("scientific_assessment", json!("verified")),
        ("path", json!("../../secret")),
        ("execute", json!(true)),
    ] {
        let mut p = packet(&pid, 4, 3);
        p[field] = value;
        let before = snapshot(&f.0);
        assert_eq!(
            call(&b, "research_evidence_record", p).unwrap_err().code,
            "research_invalid_input"
        );
        assert_eq!(snapshot(&f.0), before);
    }
    let mut p = packet(&pid, 4, 3);
    p["text"] = json!("x".repeat(65537));
    assert_eq!(
        call(&b, "research_evidence_record", p).unwrap_err().code,
        "research_invalid_input"
    );
    let other=call(&b,"research_project_create",json!({"schema_version":1,"operation_id":op(8),"metadata":{"title":"Other","primary_type":null,"type_unknown_reason":"fixture","secondary_types":[]},"goal":"Other local fixture"})).unwrap()["project_id"].as_str().unwrap().to_owned();
    prepare(&b, &other);
    assert!(call(
        &b,
        "research_record_get",
        json!({"schema_version":2,"project_id":other,"reference":reference(&saved,"evidence")})
    )
    .is_err());
}
#[test]
fn missing_snapshots_fail_closed_and_acknowledged_project_identity_survives() {
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b);
    prepare(&b, &pid);
    let owner: Value =
        serde_json::from_slice(&fs::read(f.0.join("research/v1/owner-v1.json")).unwrap()).unwrap();
    let snap =
        f.0.join("research/s2-snapshots/v1")
            .join(owner["owner_id"].as_str().unwrap())
            .join(&pid)
            .join(op(2))
            .join("manifest.json");
    fs::rename(&snap, f.0.join("quarantined-manifest")).unwrap();
    let before = snapshot(&f.0);
    assert_eq!(
        call(
            &b,
            "research_project_get",
            json!({"schema_version":1,"project_id":pid})
        )
        .unwrap_err()
        .code,
        "research_corrupt_store"
    );
    assert_eq!(snapshot(&f.0), before);
    let p = project(&f.0, &pid);
    fs::rename(p, f.0.join("quarantined-project")).unwrap();
    let list = call(
        &b,
        "research_project_list",
        json!({"schema_version":1,"cursor":null,"limit":256}),
    )
    .unwrap();
    assert_eq!(list["items"][0]["project_id"], pid);
    assert_eq!(list["items"][0]["reason"], "research_missing_project_data");
    assert!(list["items"][0]["project"].is_null());
}
#[test]
fn concurrent_writers_and_pagination_are_deterministic() {
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b);
    prepare(&b, &pid);
    let result = std::thread::scope(|s| {
        let a = s.spawn(|| call(&f.bridge(), "research_evidence_record", packet(&pid, 3, 2)));
        let c = s.spawn(|| call(&f.bridge(), "research_evidence_record", packet(&pid, 4, 2)));
        vec![a.join().unwrap(), c.join().unwrap()]
    });
    assert_eq!(result.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        result
            .iter()
            .filter(|r| r
                .as_ref()
                .err()
                .is_some_and(|e| e.code == "research_conflict"))
            .count(),
        1
    );
    let mut cursor = Value::Null;
    let mut order = Vec::new();
    loop {
        let p = call(
            &b,
            "research_events_page",
            json!({"schema_version":2,"project_id":pid,"cursor":cursor,"limit":1}),
        )
        .unwrap();
        order.extend(
            p["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["project_sequence"].as_u64().unwrap()),
        );
        cursor = p["next_cursor"].clone();
        if cursor.is_null() {
            break;
        }
    }
    assert_eq!(order, vec![1, 2, 3]);
    assert_eq!(
        call(
            &b,
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

#[test]
fn exact_legacy_plan_revision_association_survives_transition_and_rejects_wrong_question() {
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b);
    let question = call(
        &b,
        "research_project_get",
        json!({"schema_version":1,"project_id":pid}),
    )
    .unwrap()["project"]["active_question"]
        .clone();
    let plan=call(&b,"research_plan_revision_save",json!({"schema_version":1,"project_id":pid,"operation_id":op(10),"expected_project_version":1,"plan_id":null,"parent":null,"question":question,"body":{"objective":"Local fixture draft","methodology":"","milestones":[],"technical_requirements":[],"evidence_criteria":[],"risks":[],"assumptions":[],"required_human_decisions":[],"expected_deliverables":[]},"revision_reason":"fixture"})).unwrap();
    let logical = call(
        &b,
        "research_record_get",
        json!({"schema_version":1,"project_id":pid,"reference":reference(&plan,"plan")}),
    )
    .unwrap();
    let revision = logical["record"]["draft_revision"].clone();
    call(&b,"research_prepare_evidence_store",json!({"schema_version":2,"project_id":pid,"operation_id":op(2),"expected_project_version":2})).unwrap();
    let mut p = packet(&pid, 3, 3);
    p["question"] = question.clone();
    p["plan_revision"] = revision.clone();
    p["association_reason"] = Value::Null;
    let saved = call(&b, "research_evidence_record", p.clone()).unwrap();
    assert_eq!(
        get(&b, &pid, reference(&saved, "evidence"))["record"]["plan_revision"],
        revision
    );
    let updated=call(&b,"research_question_save",json!({"schema_version":1,"project_id":pid,"operation_id":op(11),"expected_project_version":4,"previous":question,"body":{"question":"Later fixture question","objectives":[],"hypotheses":[],"evaluation_questions":[],"constraints":[]},"revision_reason":"fixture revision"})).unwrap();
    p["operation_id"] = json!(op(4));
    p["expected_project_version"] = json!(5);
    p["question"] = reference(&updated, "question");
    let before = snapshot(&f.0);
    assert_eq!(
        call(&b, "research_evidence_record", p.clone())
            .unwrap_err()
            .code,
        "research_invalid_input"
    );
    assert_eq!(snapshot(&f.0), before);
    p["question"] = question;
    p["origin"] = json!("ai_generated_reported");
    let saved = call(&b, "research_evidence_record", p).unwrap();
    let view = get(&b, &pid, reference(&saved, "evidence"));
    assert_eq!(view["record"]["scientific_assessment"], "not_assessed");
    assert_eq!(
        view["record"]["independent_verification"],
        "not_established"
    );
}

#[test]
fn canonical_journal_tampering_and_snapshot_symlinks_are_refused_without_repair() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b);
    prepare(&b, &pid);
    let saved = call(&b, "research_evidence_record", packet(&pid, 3, 2)).unwrap();
    let path = project(&f.0, &pid).join("commits/00000000000000000003.json");
    let mut frame: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let artifact = frame["records"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["kind"] == "artifact")
        .unwrap();
    artifact["record"]["digest"]["value"] = json!("0".repeat(64));
    fs::write(&path, serde_json::to_vec(&frame).unwrap()).unwrap();
    let before = snapshot(&f.0);
    assert_eq!(
        call(
            &b,
            "research_record_get",
            json!({"schema_version":2,"project_id":pid,"reference":reference(&saved,"evidence")})
        )
        .unwrap_err()
        .code,
        "research_corrupt_store"
    );
    assert_eq!(snapshot(&f.0), before);
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b);
    prepare(&b, &pid);
    let owner: Value =
        serde_json::from_slice(&fs::read(f.0.join("research/v1/owner-v1.json")).unwrap()).unwrap();
    let path =
        f.0.join("research/s2-snapshots/v1")
            .join(owner["owner_id"].as_str().unwrap())
            .join(&pid)
            .join(op(2));
    let quarantined = f.0.join("quarantined-snapshot");
    fs::rename(&path, &quarantined).unwrap();
    symlink(&quarantined, &path).unwrap();
    assert_eq!(
        call(
            &b,
            "research_project_get",
            json!({"schema_version":1,"project_id":pid})
        )
        .unwrap_err()
        .code,
        "research_corrupt_store"
    );
    assert!(path.is_symlink());
    assert!(quarantined.join("manifest.json").exists());
}

#[test]
fn retained_orphans_count_toward_quota_and_never_erase_readable_records() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b);
    prepare(&b, &pid);
    let dir = project(&f.0, &pid).join("blobs");
    for n in 100..116 {
        let path = dir.join(op(n));
        let file = fs::File::create(&path).unwrap();
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .unwrap();
        file.set_len(32 * 1024 * 1024).unwrap();
    }
    assert_eq!(
        call(&b, "research_evidence_record", packet(&pid, 3, 2))
            .unwrap_err()
            .code,
        "research_storage_limit"
    );
    assert_eq!(
        call(
            &b,
            "research_project_get",
            json!({"schema_version":1,"project_id":pid})
        )
        .unwrap()["project"]["meta"]["version"],
        2
    );
    assert_eq!(fs::read_dir(dir).unwrap().count(), 16);
}

#[test]
fn lost_v1_namespace_never_hides_retained_s2_or_reinitializes_owner() {
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b);
    prepare(&b, &pid);
    fs::rename(f.0.join("research/v1"), f.0.join("quarantined-v1")).unwrap();
    let before = snapshot(&f.0);
    assert_eq!(
        call(
            &b,
            "research_project_list",
            json!({"schema_version":1,"cursor":null,"limit":256})
        )
        .unwrap_err()
        .code,
        "research_corrupt_store"
    );
    assert_eq!(call(&b,"research_project_create",json!({"schema_version":1,"operation_id":op(10),"metadata":{"title":"Must not initialize","primary_type":null,"type_unknown_reason":"fixture","secondary_types":[]},"goal":"Refused"})).unwrap_err().code,"research_corrupt_store");
    assert!(!f.0.join("research/v1").exists());
    assert_eq!(snapshot(&f.0), before);
}
