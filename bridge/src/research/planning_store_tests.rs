//! Real abrupt test-build child exits, production filesystem/replay implementation.
use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "tako-s3-crash-{}-{}",
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
fn op(n: u64) -> String {
    format!("{n:032x}")
}
fn setup(root: &Path) -> Value {
    let created=crate::research::dispatch(root,"research_project_create",json!({"schema_version":1,"operation_id":op(1),"metadata":{"title":"S3 crash fixture","primary_type":null,"type_unknown_reason":"test","secondary_types":[]},"goal":"No research performed"})).unwrap();
    let pid = created["project_id"].clone();
    let q = crate::research::dispatch(
        root,
        "research_project_get",
        json!({"schema_version":1,"project_id":pid}),
    )
    .unwrap()["project"]["active_question"]
        .clone();
    let mut input: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/research-plan-v3.json")).unwrap();
    input["question"] = q;
    json!({"schema_version":3,"project_id":pid,"operation_id":op(2),"expected_project_version":1,"plan_id":null,"parent_revision_id":null,"input":input,"revision_reason":"test"})
}
fn child(root: &Path, args: Value, phase: Option<&str>) -> std::process::Output {
    std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","research::store::v3::tests::abrupt_s3_publication_recovers_without_duplicate_revisions"])
        .env_clear().env("TAKO_TEST_S3_CRASH",json!({"root":root,"args":args,"phase":phase}).to_string()).output().unwrap()
}
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, p: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for e in std::fs::read_dir(p).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out)
            } else {
                out.insert(
                    p.strip_prefix(root).unwrap().into(),
                    std::fs::read(&p).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}
#[test]
fn abrupt_s3_publication_recovers_without_duplicate_revisions() {
    let phases = [
        ("S3AfterIntent", Phase::S3AfterIntent),
        ("BeforeWrite", Phase::BeforeWrite),
        ("BeforePublish", Phase::BeforePublish),
        ("AfterPublish", Phase::AfterPublish),
        ("BeforeAck", Phase::BeforeAck),
        ("S3AfterFrame", Phase::S3AfterFrame),
        ("S3AfterDone", Phase::S3AfterDone),
    ];
    if let Ok(raw) = std::env::var("TAKO_TEST_S3_CRASH") {
        let case: Value = serde_json::from_str(&raw).unwrap();
        if let Some(name) = case["phase"].as_str() {
            let phase = phases.iter().find(|(s, _)| *s == name).unwrap().1;
            FAULT.with(|f| f.set(Some((phase, true))));
        }
        dispatch(
            Path::new(case["root"].as_str().unwrap()),
            "create_plan_draft",
            case["args"].clone(),
        )
        .unwrap();
        assert!(case["phase"].is_null(), "fault not reached");
        return;
    }
    for (name, _) in phases {
        let f = Fixture::new();
        let args = setup(&f.0);
        let out = child(&f.0, args.clone(), Some(name));
        assert_eq!(out.status.code(), Some(93), "{out:?}");
        let ctx = Context::open(&f.0, false).unwrap();
        let pid = args["project_id"].as_str().unwrap();
        let (_, legacy, _) = ctx.ready(pid, false).unwrap();
        let s = load(&ctx, pid, &legacy).unwrap();
        if name == "S3AfterDone" {
            assert!(s.pending.is_empty());
            assert_eq!(s.revisions.len(), 1);
        } else {
            assert_eq!(s.pending.len(), 1);
        }
        let frozen = snapshot(&f.0);
        drop(ctx);
        let query = crate::research::dispatch(
            &f.0,
            "research_project_get",
            json!({"schema_version":1,"project_id":pid}),
        )
        .unwrap();
        assert_eq!(query["project"]["meta"]["version"], 1);
        assert_eq!(snapshot(&f.0), frozen);
        if name != "S3AfterDone" {
            let mut other = args.clone();
            other["operation_id"] = json!(op(3));
            assert_eq!(
                dispatch(&f.0, "create_plan_draft", other).unwrap_err().code,
                "research_pending_operation"
            );
            assert_eq!(crate::research::dispatch(&f.0,"research_project_update",json!({"schema_version":1,"project_id":pid,"operation_id":op(4),"expected_project_version":1,"metadata":{"title":"No bypass","primary_type":null,"type_unknown_reason":"test","secondary_types":[]}})).unwrap_err().code,"research_pending_operation");
        }
        assert!(child(&f.0, args.clone(), None).status.success());
        let saved = dispatch(&f.0, "create_plan_draft", args.clone()).unwrap();
        assert_eq!(saved["project_version"], 2);
        let frozen = snapshot(&f.0);
        assert_eq!(
            dispatch(&f.0, "create_plan_draft", args.clone()).unwrap(),
            saved
        );
        assert_eq!(snapshot(&f.0), frozen);
        let page=dispatch(&f.0,"list_plan_revisions",json!({"schema_version":3,"project_id":pid,"plan_id":saved["plan_id"],"cursor":0,"limit":256})).unwrap();
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
    }
}
#[test]
fn failed_write_is_explicit_and_retry_preserves_the_reserved_identity() {
    let f = Fixture::new();
    let args = setup(&f.0);
    FAULT.with(|fault| fault.set(Some((Phase::S3AfterIntent, false))));
    assert_eq!(
        dispatch(&f.0, "create_plan_draft", args.clone())
            .unwrap_err()
            .code,
        "research_storage_limit"
    );
    let ctx = Context::open(&f.0, false).unwrap();
    let pid = args["project_id"].as_str().unwrap();
    let (_, legacy, _) = ctx.ready(pid, false).unwrap();
    let s = load(&ctx, pid, &legacy).unwrap();
    assert_eq!(s.pending.len(), 1);
    let reserved = s.pending[0].revision.as_ref().unwrap().revision_id.clone();
    drop(ctx);
    let saved = dispatch(&f.0, "create_plan_draft", args).unwrap();
    assert_eq!(saved["revision_id"], reserved);
}
