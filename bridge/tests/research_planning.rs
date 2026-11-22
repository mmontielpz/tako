//! Real local native storage only. No reviewers, AI responses or science are simulated.
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
            "tako-s3-{}-{}",
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
fn op(n: u64) -> String {
    format!("{n:032x}")
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
fn create(b: &Bridge, n: u64) -> String {
    call(b,"research_project_create",json!({"schema_version":1,"operation_id":op(n),"metadata":{"title":"S3 fixture","primary_type":null,"type_unknown_reason":"fixture","secondary_types":[]},"goal":"Local fixture; no research performed"})).unwrap()["project_id"].as_str().unwrap().into()
}
fn input(b: &Bridge, pid: &str) -> Value {
    let mut p: Value =
        serde_json::from_str(include_str!("fixtures/research-plan-v3.json")).unwrap();
    p["question"] = call(
        b,
        "research_project_get",
        json!({"schema_version":1,"project_id":pid}),
    )
    .unwrap()["project"]["active_question"]
        .clone();
    p
}
fn draft(pid: &str, input: Value, n: u64, version: u64) -> Value {
    json!({"schema_version":3,"project_id":pid,"operation_id":op(n),"expected_project_version":version,"plan_id":null,"parent_revision_id":null,"input":input,"revision_reason":"fixture"})
}
fn read(b: &Bridge, pid: &str, result: &Value) -> Value {
    call(
        b,
        "read_plan",
        json!({"schema_version":3,"project_id":pid,"plan_id":result["plan_id"],"revision_id":null}),
    )
    .unwrap()
}
fn target(pid: &str, result: &Value, n: u64) -> Value {
    json!({"schema_version":3,"project_id":pid,"plan_id":result["plan_id"],"revision_id":result["revision_id"],"expected_revision_digest":result["revision_digest"],"operation_id":op(n),"expected_project_version":result["project_version"]})
}
fn files(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, p: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for e in fs::read_dir(p).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(base, &p, out)
            } else {
                out.insert(
                    p.strip_prefix(base).unwrap().to_string_lossy().into(),
                    fs::read(p).unwrap(),
                );
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(root, root, &mut out);
    out
}
fn area(root: &Path, pid: &str, name: &str) -> PathBuf {
    let owner: Value =
        serde_json::from_slice(&fs::read(root.join("research/v1/owner-v1.json")).unwrap()).unwrap();
    root.join("research")
        .join(name)
        .join("v3")
        .join(owner["owner_id"].as_str().unwrap())
        .join(pid)
}
#[test]
fn all_four_types_validate_without_forcing_experiments_or_hypotheses() {
    for (kind, methods) in [
        (
            "literature_review",
            vec!["selection", "appraisal", "synthesis"],
        ),
        (
            "experimental_research",
            vec!["design", "measurement", "controls", "sampling", "analysis"],
        ),
        (
            "computational_research",
            vec!["inputs", "baseline", "analysis", "reproducibility"],
        ),
        (
            "technical_feasibility",
            vec!["constraints", "alternatives", "evaluation"],
        ),
    ] {
        let f = Fixture::new();
        let b = f.bridge();
        let pid = create(&b, 1);
        let mut p = input(&b, &pid);
        p["body"]["research_type"] = json!(kind);
        p["body"]["method_requirements"] = json!(methods
            .into_iter()
            .map(|m| json!({"kind":m,"description":"Fixture method requirement"}))
            .collect::<Vec<_>>());
        let report=call(&b,"validate_plan_revision",json!({"schema_version":3,"project_id":pid,"plan_id":null,"revision_id":null,"proposal_json":p.to_string()})).unwrap();
        assert_eq!(report["reviewable"], true, "{report}");
        assert_eq!(report["structural_only"], true);
        let saved = call(&b, "create_plan_draft", draft(&pid, p, 2, 1)).unwrap();
        let submitted = call(&b, "submit_plan_for_review", target(&pid, &saved, 3)).unwrap();
        assert_eq!(submitted["review_state"], "REVIEW_REQUIRED");
        assert_eq!(
            read(&b, &pid, &saved)["revision"]["payload"]["input"]["body"]["hypotheses"],
            json!([])
        );
    }
}
#[test]
fn immutable_revisions_restart_history_and_legacy_bytes_are_preserved() {
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b, 1);
    let original = files(&f.0);
    let p = input(&b, &pid);
    let a = draft(&pid, p.clone(), 2, 1);
    let saved = call(&b, "create_plan_draft", a.clone()).unwrap();
    assert_eq!(call(&b, "create_plan_draft", a).unwrap(), saved);
    let before = read(&b, &pid, &saved)["revision"].clone();
    let submitted = call(&b, "submit_plan_for_review", target(&pid, &saved, 3)).unwrap();
    let mut args = draft(&pid, p, 4, 3);
    args["plan_id"] = saved["plan_id"].clone();
    args["parent_revision_id"] = saved["revision_id"].clone();
    args["input"]["body"]["objective"] = json!("Revised fixture question scope");
    let revised = call(&b, "create_plan_revision", args.clone()).unwrap();
    assert_eq!(revised["review_state"], "DRAFT");
    assert_eq!(revised["project_version"], 4);
    drop(b);
    let b = f.bridge();
    assert_eq!(read(&b, &pid, &revised)["revision"]["revision_number"], 2);
    let old=call(&b,"read_plan",json!({"schema_version":3,"project_id":pid,"plan_id":saved["plan_id"],"revision_id":saved["revision_id"]})).unwrap();
    assert_eq!(old["revision"], before);
    assert_eq!(old["review_state"], submitted["review_state"]);
    let page=call(&b,"list_plan_revisions",json!({"schema_version":3,"project_id":pid,"plan_id":saved["plan_id"],"cursor":0,"limit":1})).unwrap();
    assert_eq!(page["next_cursor"], 1);
    let history=call(&b,"read_plan_review_history",json!({"schema_version":3,"project_id":pid,"plan_id":saved["plan_id"],"cursor":0,"limit":256})).unwrap();
    assert_eq!(
        history["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["CREATED", "SUBMITTED_REVIEW", "REVISED"]
    );
    assert_eq!(history["decisions"], json!([]));
    let after = files(&f.0);
    for (name, bytes) in original {
        assert_eq!(after[&name], bytes);
    }
    assert_eq!(call(&b, "create_plan_revision", args).unwrap(), revised);
}
#[test]
fn human_review_is_unavailable_and_never_forged_or_executed() {
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b, 1);
    let saved = call(&b, "create_plan_draft", draft(&pid, input(&b, &pid), 2, 1)).unwrap();
    let submitted = call(&b, "submit_plan_for_review", target(&pid, &saved, 3)).unwrap();
    let mut a = target(&pid, &submitted, 4);
    a["human_review_intent"] = json!(true);
    a["review_rationale"] = json!("Explicit fixture request is not authenticated authority");
    let frozen = files(&f.0);
    for cmd in ["approve_plan_revision", "reject_plan_revision"] {
        assert_eq!(
            call(&b, cmd, a.clone()).unwrap_err().code,
            "research_review_authority_unavailable"
        );
        let mut forged = a.clone();
        forged["actor_id"] = json!(op(999));
        assert_eq!(
            call(&b, cmd, forged).unwrap_err().code,
            "research_invalid_input"
        );
        let mut implicit = a.clone();
        implicit["human_review_intent"] = json!(false);
        assert_eq!(
            call(&b, cmd, implicit).unwrap_err().code,
            "research_invalid_input"
        );
    }
    assert_eq!(files(&f.0), frozen);
    assert_eq!(read(&b, &pid, &saved)["review_state"], "REVIEW_REQUIRED");
}
#[test]
fn concurrent_and_conflicting_operations_preserve_cas_and_global_operation_identity() {
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b, 1);
    let p = input(&b, &pid);
    let a = draft(&pid, p.clone(), 2, 1);
    let c = draft(&pid, p, 3, 1);
    let outcomes = std::thread::scope(|s| {
        let x = s.spawn(|| call(&f.bridge(), "create_plan_draft", a.clone()));
        let y = s.spawn(|| call(&f.bridge(), "create_plan_draft", c.clone()));
        vec![x.join().unwrap(), y.join().unwrap()]
    });
    assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| r
                .as_ref()
                .err()
                .is_some_and(|e| e.code == "research_conflict"))
            .count(),
        1
    );
    let winner = outcomes.iter().position(|r| r.is_ok()).unwrap();
    let result = outcomes[winner].as_ref().unwrap();
    let mut args = if winner == 0 { a } else { c };
    args["input"]["body"]["objective"] = json!("Different intent");
    assert_eq!(
        call(&b, "create_plan_draft", args).unwrap_err().code,
        "research_idempotency_conflict"
    );
    let metadata = call(
        &b,
        "research_project_get",
        json!({"schema_version":1,"project_id":pid}),
    )
    .unwrap()["project"]["metadata"]
        .clone();
    assert_eq!(call(&b,"research_project_update",json!({"schema_version":1,"project_id":pid,"operation_id":result["operation_id"],"expected_project_version":1,"metadata":metadata})).unwrap_err().code,"research_idempotency_conflict");
    call(&b,"research_project_update",json!({"schema_version":1,"project_id":pid,"operation_id":op(9),"expected_project_version":1,"metadata":metadata})).unwrap();
    assert_eq!(read(&b, &pid, result)["project_version"], 3);
    assert_eq!(
        call(&b, "submit_plan_for_review", target(&pid, result, 10))
            .unwrap_err()
            .code,
        "research_conflict"
    );
}
#[test]
fn malformed_proposals_unknowns_contradictions_and_cross_project_refs_fail_safely() {
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b, 1);
    let other = create(&b, 20);
    let p = input(&b, &pid);
    let original = files(&f.0);
    for raw in [
        "{bad".to_owned(),
        "{\"question\":null,\"question\":null}".into(),
        {
            let mut forged = p.clone();
            forged["approved"] = json!(true);
            forged.to_string()
        },
    ] {
        assert!(call(&b,"validate_plan_revision",json!({"schema_version":3,"project_id":pid,"plan_id":null,"revision_id":null,"proposal_json":raw})).is_err());
    }
    let mut wrong = p.clone();
    wrong["question"] = input(&b, &other)["question"].clone();
    assert_eq!(
        call(&b, "create_plan_draft", draft(&pid, wrong, 2, 1))
            .unwrap_err()
            .code,
        "research_not_found"
    );
    let mut contradiction = p.clone();
    contradiction["body"]["methodology"] = json!("");
    assert_eq!(
        call(
            &b,
            "create_plan_draft",
            draft(&pid, contradiction.clone(), 2, 1)
        )
        .unwrap_err()
        .code,
        "research_invalid_input"
    );
    contradiction["body"]["unknowns"] = json!([{"field":"methodology","reason":"Not yet known"}]);
    contradiction["body"]["intent"] = json!("confirmatory");
    assert_eq!(files(&f.0), original);
    let saved = call(&b, "create_plan_draft", draft(&pid, contradiction, 2, 1)).unwrap();
    let view = read(&b, &pid, &saved);
    assert_eq!(view["validation"]["reviewable"], false);
    assert_eq!(
        call(&b, "submit_plan_for_review", target(&pid, &saved, 3))
            .unwrap_err()
            .code,
        "research_invalid_input"
    );
}
#[test]
fn lost_acknowledged_frame_and_corrupt_data_are_not_reconstructed() {
    for corrupt in [false, true] {
        let f = Fixture::new();
        let b = f.bridge();
        let pid = create(&b, 1);
        let a = draft(&pid, input(&b, &pid), 2, 1);
        let saved = call(&b, "create_plan_draft", a.clone()).unwrap();
        let path = area(&f.0, &pid, "s3").join("commits/00000000000000000001.json");
        if corrupt {
            let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            value["revision"]["payload_digest"]["value"] = json!("0".repeat(64));
            fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        } else {
            fs::rename(&path, f.0.join("quarantined-frame")).unwrap();
        }
        let frozen = files(&f.0);
        drop(b);
        let b = f.bridge();
        assert_eq!(call(&b,"read_plan",json!({"schema_version":3,"project_id":pid,"plan_id":saved["plan_id"],"revision_id":null})).unwrap_err().code,"research_corrupt_store");
        assert!(call(&b, "create_plan_draft", a).is_err());
        assert_eq!(files(&f.0), frozen);
        assert!(call(
            &b,
            "research_project_get",
            json!({"schema_version":1,"project_id":pid})
        )
        .is_ok());
    }
}
#[test]
fn s2_material_status_is_explicit_and_missing_content_never_becomes_verified() {
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b, 1);
    call(&b,"research_prepare_evidence_store",json!({"schema_version":2,"project_id":pid,"operation_id":op(2),"expected_project_version":1})).unwrap();
    let ev=call(&b,"research_evidence_record",json!({"schema_version":2,"project_id":pid,"operation_id":op(3),"expected_project_version":2,"previous":null,"revision_reason":"fixture","claim":"Supplied assertion","stance":"unknown","limitations":["Unverified"],"origin":"local_input","text":"fixture note","artifact":null,"source":null,"source_ref":null,"question":null,"plan_revision":null,"association_reason":"project note"})).unwrap();
    let evidence = ev["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "evidence")
        .unwrap()
        .clone();
    let artifact = ev["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "artifact")
        .unwrap();
    let mut p = input(&b, &pid);
    p["selected_evidence"] = json!([evidence]);
    let a = draft(&pid, p, 4, 3);
    let saved = call(&b, "create_plan_draft", a).unwrap();
    assert_eq!(
        read(&b, &pid, &saved)["current_materials"][0]["integrity"],
        "matches"
    );
    let owner: Value =
        serde_json::from_slice(&fs::read(f.0.join("research/v1/owner-v1.json")).unwrap()).unwrap();
    let blob =
        f.0.join("research/v1")
            .join(owner["owner_id"].as_str().unwrap())
            .join(&pid)
            .join("blobs")
            .join(artifact["id"].as_str().unwrap());
    fs::rename(&blob, f.0.join("quarantined-note")).unwrap();
    let view = read(&b, &pid, &saved);
    assert_eq!(view["current_materials"][0]["availability"], "unavailable");
    assert_eq!(
        view["revision"]["payload"]["materials"][0]["integrity"],
        "matches"
    );
    assert!(!blob.exists());
}
#[test]
fn s3_links_and_retained_namespace_cannot_escape_or_reset_the_owner() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b, 1);
    let saved = call(&b, "create_plan_draft", draft(&pid, input(&b, &pid), 2, 1)).unwrap();
    let path = area(&f.0, &pid, "s3").join("commits/00000000000000000001.json");
    let held = f.0.join("held-frame");
    fs::rename(&path, &held).unwrap();
    symlink(&held, &path).unwrap();
    assert_eq!(call(&b,"read_plan",json!({"schema_version":3,"project_id":pid,"plan_id":saved["plan_id"],"revision_id":null})).unwrap_err().code,"research_unsafe_store");
    assert!(path.is_symlink());
    fs::rename(f.0.join("research/v1"), f.0.join("held-v1")).unwrap();
    assert_eq!(call(&b,"research_project_create",json!({"schema_version":1,"operation_id":op(4),"metadata":{"title":"No adoption","primary_type":null,"type_unknown_reason":"test","secondary_types":[]},"goal":"Refused"})).unwrap_err().code,"research_corrupt_store");
    assert!(!f.0.join("research/v1").exists());
}
#[test]
fn retained_s3_orphans_count_toward_shared_quota_without_erasing_readable_plans() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let b = f.bridge();
    let pid = create(&b, 1);
    let saved = call(&b, "create_plan_draft", draft(&pid, input(&b, &pid), 2, 1)).unwrap();
    let commits = area(&f.0, &pid, "s3").join("commits");
    for n in 1000..1512 {
        let file = fs::File::create(commits.join(format!(".tmp-{}", op(n)))).unwrap();
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .unwrap();
        file.set_len(1024 * 1024).unwrap();
    }
    assert_eq!(
        call(&b, "submit_plan_for_review", target(&pid, &saved, 3))
            .unwrap_err()
            .code,
        "research_storage_limit"
    );
    assert_eq!(read(&b, &pid, &saved)["review_state"], "DRAFT");
    assert_eq!(fs::read_dir(commits).unwrap().count(), 513);
    let query = call(
        &b,
        "research_project_get",
        json!({"schema_version":1,"project_id":pid}),
    )
    .unwrap();
    assert_eq!(call(&b,"research_project_update",json!({"schema_version":1,"project_id":pid,"operation_id":op(4),"expected_project_version":1,"metadata":query["project"]["metadata"]})).unwrap_err().code,"research_storage_limit");
}
