#![cfg(target_os = "linux")]
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tako_bridge::{Bridge, BridgeError};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tako-research-s1-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        Self { root }
    }
    fn bridge(&self) -> Bridge {
        Bridge::open(&self.root).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
fn call(b: &Bridge, cmd: &str, args: Value) -> Result<Value, BridgeError> {
    for _ in 0..200 {
        match b.dispatch(cmd, args.clone()) {
            Err(e) if e.code == "research_busy" => {
                std::thread::sleep(std::time::Duration::from_millis(2))
            }
            result => return result,
        }
    }
    Err(BridgeError {
        code: "research_busy".into(),
        message: "test retry bound".into(),
        details: vec![],
    })
}
fn op(n: u64) -> String {
    format!("{n:032x}")
}
fn metadata(title: &str) -> Value {
    json!({"title":title,"primary_type":null,"type_unknown_reason":"not selected yet","secondary_types":[]})
}
fn create(b: &Bridge, n: u64) -> Value {
    call(&b, "research_project_create",json!({"schema_version":1,"operation_id":op(n),"metadata":metadata("Research"),"goal":"What is known?"})).unwrap()
}
fn get(b: &Bridge, p: &str) -> Value {
    call(
        &b,
        "research_project_get",
        json!({"schema_version":1,"project_id":p}),
    )
    .unwrap()
}
fn list(b: &Bridge) -> Value {
    call(
        &b,
        "research_project_list",
        json!({"schema_version":1,"cursor":null,"limit":256}),
    )
    .unwrap()
}
fn commit_path(root: &Path, p: &str, seq: u64) -> PathBuf {
    let owner: Value =
        serde_json::from_slice(&std::fs::read(root.join("research/v1/owner-v1.json")).unwrap())
            .unwrap();
    root.join("research/v1")
        .join(owner["owner_id"].as_str().unwrap())
        .join(p)
        .join("commits")
        .join(format!("{seq:020}.json"))
}
fn err(r: Result<Value, BridgeError>, code: &str) {
    let e = r.unwrap_err();
    assert_eq!(e.code, format!("research_{code}"), "{e:?}");
    assert!(e.details.is_empty());
}
fn draft() -> Value {
    json!({"objective":"Review supplied material","methodology":"","milestones":[],"technical_requirements":[],"evidence_criteria":[],"risks":[],"assumptions":[],"required_human_decisions":[],"expected_deliverables":[]})
}

#[test]
fn records_roundtrip_metadata_question_and_immutable_plan_history() {
    let fx = Fixture::new();
    let b = fx.bridge();
    assert!(list(&b)["items"].as_array().unwrap().is_empty());
    assert!(!fx.root.join("research").exists());
    let c = create(&b, 1);
    let pid = c["project_id"].as_str().unwrap();
    let first_bytes = std::fs::read(commit_path(&fx.root, pid, 1)).unwrap();
    let initial = get(&b, pid);
    let qref = initial["project"]["active_question"].clone();
    assert_eq!(initial["project"]["metadata"]["title"], "Research");
    let update = json!({"schema_version":1,"operation_id":op(2),"project_id":pid,"expected_project_version":1,"metadata":metadata("Updated")});
    let saved = b
        .dispatch("research_project_update", update.clone())
        .unwrap();
    assert_eq!(saved["project_version"], 2);
    assert_eq!(call(&b, "research_project_update", update).unwrap(), saved);
    let old=call(&b, "research_record_get",json!({"schema_version":1,"project_id":pid,"reference":{"kind":"project","id":pid,"version":1}})).unwrap();
    assert_eq!(old["record"]["metadata"]["title"], "Research");
    assert_eq!(get(&b, pid)["project"]["metadata"]["title"], "Updated");
    let q=call(&b, "research_question_save",json!({"schema_version":1,"operation_id":op(3),"project_id":pid,"expected_project_version":2,"previous":qref,"body":{"question":"Refined question?","objectives":[],"hypotheses":[],"evaluation_questions":[],"constraints":[]},"revision_reason":"clarify"})).unwrap();
    assert_eq!(q["project_version"], 3);
    let qref = get(&b, pid)["project"]["active_question"].clone();
    let plan=call(&b, "research_plan_revision_save",json!({"schema_version":1,"operation_id":op(4),"project_id":pid,"expected_project_version":3,"plan_id":null,"parent":null,"question":qref,"body":draft(),"revision_reason":"initial draft"})).unwrap();
    assert_eq!(plan["project_version"], 4);
    let pref = get(&b, pid)["project"]["active_plan"].clone();
    let p = b
        .dispatch(
            "research_record_get",
            json!({"schema_version":1,"project_id":pid,"reference":pref}),
        )
        .unwrap();
    let parent = p["record"]["draft_revision"].clone();
    let mut body = draft();
    body["objective"] = json!("Revised review scope");
    call(&b, "research_plan_revision_save",json!({"schema_version":1,"operation_id":op(5),"project_id":pid,"expected_project_version":4,"plan_id":pref["id"],"parent":parent,"question":qref,"body":body,"revision_reason":"narrow scope"})).unwrap();
    let prior = b
        .dispatch(
            "research_record_get",
            json!({"schema_version":1,"project_id":pid,"reference":parent}),
        )
        .unwrap();
    assert_eq!(prior["record"]["revision_number"], 1);
    assert_eq!(prior["record"]["digest"], Value::Null);
    assert_eq!(prior["record"]["identity_status"], "not_computed");
    assert_eq!(
        std::fs::read(commit_path(&fx.root, pid, 1)).unwrap(),
        first_bytes
    );
    drop(b);
    let b = fx.bridge();
    assert_eq!(get(&b, pid)["project"]["meta"]["version"], 5);
    assert_eq!(list(&b)["items"].as_array().unwrap().len(), 1);
    let events = b
        .dispatch(
            "research_events_page",
            json!({"schema_version":1,"project_id":pid,"cursor":0,"limit":2}),
        )
        .unwrap();
    assert_eq!(events["items"].as_array().unwrap().len(), 2);
    assert_eq!(events["next_cursor"], 2);
    assert_eq!(events["items"][0]["kind"], "local_metadata_saved");
}

#[test]
fn duplicate_creation_and_conflicting_operations_never_create_second_project() {
    let fx = Fixture::new();
    let b = fx.bridge();
    let a = create(&b, 1);
    assert_eq!(create(&b, 1), a);
    let pid = a["project_id"].as_str().unwrap();
    err(
        call(
            &b,
            "research_project_create",
            json!({"schema_version":1,"operation_id":op(1),"metadata":metadata("changed"),"goal":"other"}),
        ),
        "idempotency_conflict",
    );
    let status = b
        .dispatch(
            "research_operation_status",
            json!({"schema_version":1,"project_id":null,"operation_id":op(1)}),
        )
        .unwrap();
    assert_eq!(status["result"], a);
    err(
        call(
            &b,
            "research_project_update",
            json!({"schema_version":1,"operation_id":op(2),"project_id":pid,"expected_project_version":2,"metadata":metadata("stale")}),
        ),
        "conflict",
    );
    assert_eq!(list(&b)["items"].as_array().unwrap().len(), 1);
    assert_eq!(get(&b, pid)["project"]["meta"]["version"], 1);
}

#[test]
fn invalid_inputs_fail_before_writes_and_unavailable_commands_never_grant_authority() {
    let fx = Fixture::new();
    let b = fx.bridge();
    let mut a =
        json!({"schema_version":1,"operation_id":op(1),"metadata":metadata("x"),"goal":"question"});
    a["path"] = json!("../private");
    err(call(&b, "research_project_create", a), "invalid_input");
    err(
        call(
            &b,
            "research_project_get",
            json!({"schema_version":1,"project_id":"../outside"}),
        ),
        "invalid_input",
    );
    err(
        call(
            &b,
            "research_project_list",
            json!({"schema_version":1,"cursor":null,"limit":257}),
        ),
        "invalid_input",
    );
    err(
        call(
            &b,
            "research_project_create",
            json!({"schema_version":2,"operation_id":op(1),"metadata":metadata("x"),"goal":"q"}),
        ),
        "unsupported_schema",
    );
    err(
        call(
            &b,
            "research_project_create",
            json!({"schema_version":1,"operation_id":op(1),"metadata":metadata("x"),"goal":"é".repeat(2049)}),
        ),
        "invalid_input",
    );
    for name in [
        "research_approve",
        "research_start",
        "research_verify",
        "research_archive",
    ] {
        err(call(&b, name, json!({})), "invalid_input")
    }
    assert!(!fx.root.join("research").exists());
    assert_eq!(
        call(&b, "integration_status", Value::Null).unwrap()["availability"],
        "disabled"
    );
    assert!(b
        .dispatch("onboarding_open_workspace", json!({"workspace":"research"}))
        .is_err());
}

#[test]
fn project_and_profile_isolation_and_pagination() {
    let fx = Fixture::new();
    let b = fx.bridge();
    let a = create(&b, 1);
    let z = create(&b, 2);
    let p = a["project_id"].as_str().unwrap();
    let other = z["project_id"].as_str().unwrap();
    let qr = get(&b, p)["project"]["active_question"].clone();
    err(
        call(
            &b,
            "research_record_get",
            json!({"schema_version":1,"project_id":other,"reference":qr}),
        ),
        "not_found",
    );
    let fresh = Fixture::new();
    assert!(list(&fresh.bridge())["items"]
        .as_array()
        .unwrap()
        .is_empty());
    let page = b
        .dispatch(
            "research_project_list",
            json!({"schema_version":1,"cursor":null,"limit":1}),
        )
        .unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert!(page["next_cursor"].is_string());
    let second = b
        .dispatch(
            "research_project_list",
            json!({"schema_version":1,"cursor":page["next_cursor"],"limit":1}),
        )
        .unwrap();
    assert_ne!(
        page["items"][0]["project_id"],
        second["items"][0]["project_id"]
    );
}

#[test]
fn corrupt_project_is_reported_and_other_project_remains_usable() {
    let fx = Fixture::new();
    let b = fx.bridge();
    let a = create(&b, 1);
    let c = create(&b, 2);
    let pid = a["project_id"].as_str().unwrap();
    let path = commit_path(&fx.root, pid, 1);
    std::fs::write(&path, b"{broken").unwrap();
    let broken = std::fs::read(&path).unwrap();
    err(
        call(
            &b,
            "research_project_get",
            json!({"schema_version":1,"project_id":pid}),
        ),
        "corrupt_store",
    );
    let pages = list(&b);
    assert!(pages["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["status"] == "unavailable"));
    assert_eq!(
        get(&b, c["project_id"].as_str().unwrap())["project"]["meta"]["version"],
        1
    );
    assert_eq!(std::fs::read(&path).unwrap(), broken);
}

#[test]
fn unsafe_files_hardlinks_symlinks_and_permission_changes_fail_closed() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    for attack in 0..3 {
        let fx = Fixture::new();
        let b = fx.bridge();
        let a = create(&b, 1);
        let pid = a["project_id"].as_str().unwrap();
        let path = commit_path(&fx.root, pid, 1);
        let original = std::fs::read(&path).unwrap();
        let target = fx.root.join("private");
        std::fs::write(&target, b"private sentinel").unwrap();
        match attack {
            0 => {
                std::fs::remove_file(&path).unwrap();
                symlink(&target, &path).unwrap()
            }
            1 => std::fs::hard_link(&path, fx.root.join("alias")).unwrap(),
            _ => std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap(),
        }
        err(
            call(
                &b,
                "research_project_get",
                json!({"schema_version":1,"project_id":pid}),
            ),
            "unsafe_store",
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"private sentinel");
        if attack != 0 {
            assert_eq!(std::fs::read(&path).unwrap(), original)
        }
    }
    // Orphan-shaped names cannot hide links or directories in owner journals.
    for location in 0..3 {
        let fx = Fixture::new();
        let b = fx.bridge();
        let a = create(&b, 1);
        let pid = a["project_id"].as_str().unwrap();
        let owner = get(&b, pid)["project"]["meta"]["owner_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let base = if location == 1 {
            fx.root.join("research/v1").join(owner).join("creates")
        } else {
            fx.root.join("research/v1")
        };
        let suspect = base.join(format!(".tmp-{}", op(88)));
        let target = fx.root.join("private-target");
        std::fs::write(&target, b"private sentinel").unwrap();
        if location == 2 {
            std::fs::create_dir(&suspect).unwrap();
        } else {
            symlink(&target, &suspect).unwrap();
        }
        err(
            call(
                &b,
                "research_project_list",
                json!({"schema_version":1,"cursor":null,"limit":256}),
            ),
            "unsafe_store",
        );
        assert_eq!(std::fs::read(target).unwrap(), b"private sentinel");
    }
}

#[test]
fn semantic_tampering_gaps_unknown_versions_and_duplicate_keys_are_detected() {
    for attack in 0..4 {
        let fx = Fixture::new();
        let b = fx.bridge();
        let a = create(&b, 1);
        let pid = a["project_id"].as_str().unwrap();
        let path = commit_path(&fx.root, pid, 1);
        let mut v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        match attack {
            0 => {
                v["records"][0]["record"]["metadata"]["title"] = json!("forged");
                std::fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap()
            }
            1 => std::fs::rename(&path, path.with_file_name("00000000000000000002.json")).unwrap(),
            2 => {
                v["schema_version"] = json!(2);
                std::fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap()
            }
            _ => std::fs::write(&path, b"{\"schema_version\":1,\"schema_version\":1}").unwrap(),
        }
        let e = b
            .dispatch(
                "research_project_get",
                json!({"schema_version":1,"project_id":pid}),
            )
            .unwrap_err();
        assert_eq!(
            e.code,
            if attack == 2 {
                "research_unsupported_schema"
            } else {
                "research_corrupt_store"
            }
        );
    }
}

#[test]
fn missing_owner_never_adopts_existing_records_or_blocks_engineering() {
    let fx = Fixture::new();
    let b = fx.bridge();
    create(&b, 1);
    std::fs::remove_file(fx.root.join("research/v1/owner-v1.json")).unwrap();
    err(
        call(
            &b,
            "research_project_list",
            json!({"schema_version":1,"cursor":null,"limit":1}),
        ),
        "corrupt_store",
    );
    err(
        call(
            &b,
            "research_project_create",
            json!({"schema_version":1,"operation_id":op(2),"metadata":metadata("x"),"goal":"q"}),
        ),
        "corrupt_store",
    );
    assert!(b
        .dispatch(
            "onboarding_open_workspace",
            json!({"workspace":"engineering"})
        )
        .is_ok());
}

// Reconciliation reads real journals/receipts. Quarantine simulates loss without
// discarding fixture bytes or providing any automatic reconstruction mechanism.
fn project_directory(root: &Path, pid: &str) -> PathBuf {
    commit_path(root, pid, 1)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}
fn file_snapshot(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let kind = std::fs::symlink_metadata(&path).unwrap().file_type();
            if kind.is_dir() {
                visit(root, &path, files);
            } else if kind.is_file() {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    std::fs::read(path).unwrap(),
                );
            } else if kind.is_symlink() {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    std::fs::read_link(path)
                        .unwrap()
                        .as_os_str()
                        .as_encoded_bytes()
                        .to_vec(),
                );
            }
        }
    }
    let mut files = std::collections::BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn acknowledged_missing_directory_retains_identity_without_fabricating_history() {
    let fx = Fixture::new();
    let b = fx.bridge();
    let made = create(&b, 1);
    let pid = made["project_id"].as_str().unwrap();
    call(&b, "research_project_update", json!({"schema_version":1,"operation_id":op(2),"project_id":pid,"expected_project_version":1,"metadata":metadata("Acknowledged revision 2")})).unwrap();
    let dir = project_directory(&fx.root, pid);
    let saved = fx.root.join("quarantined");
    std::fs::rename(&dir, &saved).unwrap();
    let before = file_snapshot(&fx.root);
    for _ in 0..3 {
        let page = list(&b);
        let items = page["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["project_id"], pid);
        assert_eq!(items[0]["status"], "unavailable");
        assert_eq!(items[0]["project"], Value::Null);
        assert_eq!(items[0]["reason"], "research_missing_project_data");
        err(
            call(
                &b,
                "research_project_get",
                json!({"schema_version":1,"project_id":pid}),
            ),
            "missing_project_data",
        );
        err(
            call(
                &b,
                "research_operation_status",
                json!({"schema_version":1,"project_id":null,"operation_id":op(1)}),
            ),
            "missing_project_data",
        );
        err(
            call(
                &b,
                "research_operation_status",
                json!({"schema_version":1,"project_id":pid,"operation_id":op(2)}),
            ),
            "missing_project_data",
        );
        err(
            call(
                &b,
                "research_project_create",
                json!({"schema_version":1,"operation_id":op(1),"metadata":metadata("Research"),"goal":"What is known?"}),
            ),
            "missing_project_data",
        );
        assert!(!dir.exists());
        assert_eq!(file_snapshot(&fx.root), before);
    }
    // A new project cannot make the retained old identity disappear.
    let new = create(&b, 3);
    assert_ne!(new["project_id"], pid);
    assert_eq!(list(&b)["items"].as_array().unwrap().len(), 2);
    assert!(!dir.exists());
    assert!(saved.join("commits/00000000000000000002.json").is_file());
}

#[test]
fn missing_and_corrupt_ids_stay_in_deterministic_pages_across_restart() {
    let fx = Fixture::new();
    let b = fx.bridge();
    let a = create(&b, 1);
    let c = create(&b, 2);
    let healthy = create(&b, 3);
    let lost = a["project_id"].as_str().unwrap();
    let corrupt = c["project_id"].as_str().unwrap();
    std::fs::rename(
        project_directory(&fx.root, lost),
        fx.root.join("quarantined"),
    )
    .unwrap();
    std::fs::write(commit_path(&fx.root, corrupt, 1), b"{corrupt").unwrap();
    let before = file_snapshot(&fx.root);
    let pages = |b: &Bridge| {
        let mut cursor = Value::Null;
        let mut items = Vec::new();
        loop {
            let page = call(
                b,
                "research_project_list",
                json!({"schema_version":1,"cursor":cursor,"limit":1}),
            )
            .unwrap();
            items.extend(page["items"].as_array().unwrap().iter().cloned());
            cursor = page["next_cursor"].clone();
            if cursor.is_null() {
                break;
            }
        }
        items
    };
    let first = pages(&b);
    assert_eq!(first.len(), 3);
    assert!(first
        .iter()
        .any(|r| r["project_id"] == lost && r["reason"] == "research_missing_project_data"));
    assert!(first
        .iter()
        .any(|r| r["project_id"] == corrupt && r["reason"] == "research_corrupt_store"));
    assert!(first
        .iter()
        .any(|r| r["project_id"] == healthy["project_id"] && r["status"] == "available"));
    drop(b);
    let b = fx.bridge();
    assert_eq!(pages(&b), first);
    assert_eq!(pages(&b), first);
    // Even when every acknowledged directory is absent, discovery is not empty.
    std::fs::rename(
        project_directory(&fx.root, corrupt),
        fx.root.join("corrupt-copy"),
    )
    .unwrap();
    std::fs::rename(
        project_directory(&fx.root, healthy["project_id"].as_str().unwrap()),
        fx.root.join("healthy-copy"),
    )
    .unwrap();
    let all_missing = list(&b);
    assert_eq!(all_missing["items"].as_array().unwrap().len(), 3);
    assert!(all_missing["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["reason"] == "research_missing_project_data" && r["project"].is_null()));
    // No prior bytes were overwritten by the first reconciliation/restart.
    for (path, bytes) in before {
        if fx.root.join(&path).is_file() {
            assert_eq!(std::fs::read(fx.root.join(path)).unwrap(), bytes);
        }
    }
}

#[test]
fn corrupt_directories_require_explicit_external_restore_not_native_repair() {
    use std::os::unix::fs::{symlink, DirBuilderExt};
    for kind in 0..3 {
        let fx = Fixture::new();
        let b = fx.bridge();
        let a = create(&b, 1);
        let pid = a["project_id"].as_str().unwrap();
        let original = get(&b, pid);
        let dir = project_directory(&fx.root, pid);
        let saved = fx.root.join("saved-copy");
        std::fs::rename(&dir, &saved).unwrap();
        match kind {
            0 => std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap(),
            1 => std::fs::write(&dir, b"not a directory").unwrap(),
            _ => symlink(&saved, &dir).unwrap(),
        }
        let before = file_snapshot(&fx.root);
        let row = list(&b)["items"][0].clone();
        assert_eq!(row["project_id"], pid);
        assert_eq!(row["status"], "unavailable");
        assert_eq!(
            row["reason"],
            if kind == 0 {
                "research_corrupt_store"
            } else {
                "research_unsafe_store"
            }
        );
        err(
            call(
                &b,
                "research_project_create",
                json!({"schema_version":1,"operation_id":op(1),"metadata":metadata("Research"),"goal":"What is known?"}),
            ),
            if kind == 0 {
                "corrupt_store"
            } else {
                "unsafe_store"
            },
        );
        assert_eq!(file_snapshot(&fx.root), before);
        // Fixture operator explicitly supplies the intact copy; TAKO never does this.
        std::fs::rename(&dir, fx.root.join("bad-path")).unwrap();
        std::fs::rename(&saved, &dir).unwrap();
        assert_eq!(get(&b, pid), original);
        assert_eq!(list(&b)["items"][0]["status"], "available");
    }
}

#[test]
fn unregistered_directory_is_unavailable_not_recoverable_creation() {
    use std::os::unix::fs::DirBuilderExt;
    let fx = Fixture::new();
    let b = fx.bridge();
    let made = create(&b, 1);
    let parent = project_directory(&fx.root, made["project_id"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let unknown = op(999);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(parent.join(&unknown))
        .unwrap();
    let before = file_snapshot(&fx.root);
    let page = list(&b);
    let row = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["project_id"] == unknown)
        .unwrap();
    assert_eq!(row["status"], "unavailable");
    assert_eq!(row["reason"], "research_corrupt_store");
    assert_eq!(std::fs::read_dir(parent.join(unknown)).unwrap().count(), 0);
    assert_eq!(file_snapshot(&fx.root), before);
}
