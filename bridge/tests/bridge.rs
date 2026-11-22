//! Bridge tests drive the REAL engine through the same dispatch the desktop app uses.
//! Fixtures are disposable Git repositories under the system temp dir.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use tako_bridge::{Bridge, BridgeError};

static N: AtomicU32 = AtomicU32::new(0);

struct Fx {
    root: PathBuf,
    project: PathBuf,
    bridge: Bridge,
}

fn sh(dir: &Path, cmd: &str) {
    let o = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{cmd}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

impl Fx {
    fn new() -> Fx {
        let root = std::env::temp_dir().join(format!(
            "tako-bridge-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        sh(&project, "git init -q -b main && git config user.name t && git config user.email t@t && printf 'base\\n' > README.md && git add -A && git commit -qm init");
        let bridge = Bridge::open(root.join("data")).unwrap();
        Fx {
            root,
            project,
            bridge,
        }
    }
    fn call(&self, cmd: &str, args: Value) -> Result<Value, BridgeError> {
        self.bridge.dispatch(cmd, args)
    }
    fn ok(&self, cmd: &str, args: Value) -> Value {
        self.call(cmd, args)
            .unwrap_or_else(|e| panic!("{cmd} failed: {} ({})", e.message, e.code))
    }
    fn err(&self, cmd: &str, args: Value) -> BridgeError {
        match self.call(cmd, args) {
            Ok(v) => panic!("{cmd} unexpectedly succeeded: {v}"),
            Err(e) => e,
        }
    }
    fn p(&self) -> String {
        self.project.to_string_lossy().into_owned()
    }
    fn mission(&self, checks: &[&str]) -> String {
        let v = self.ok("create_mission", json!({"project_path": self.p(), "goal": "write out.txt", "criteria": ["out.txt says hello"], "checks": checks}));
        v["mission"]["id"].as_str().unwrap().to_string()
    }
    fn plan(&self, id: &str, script: &str) -> Value {
        self.ok(
            "define_plan",
            json!({"mission_id": id, "script": script, "expected_files": ["out.txt"]}),
        )
    }
    fn approve(&self, id: &str, plan: &Value, granted: &[&str]) -> Result<Value, BridgeError> {
        let p = &plan["plan"];
        self.call("approve_plan", json!({"mission_id": id, "version": p["version"], "plan_hash": p["hash"], "granted": granted}))
    }
    fn approved(&self, checks: &[&str], script: &str) -> String {
        let id = self.mission(checks);
        let plan = self.plan(&id, script);
        self.approve(&id, &plan, &["fs_write_workspace", "exec_command:sh"])
            .unwrap();
        id
    }
    fn view(&self, id: &str) -> Value {
        self.ok("get_mission", json!({"mission_id": id}))
    }
    fn wait_idle(&self, id: &str) -> Value {
        let t = Instant::now();
        loop {
            let v = self.view(id);
            let st = v["mission"]["state"].as_str().unwrap();
            if !v["job"]["running"].as_bool().unwrap() && st != "executing" && st != "verifying" {
                return v;
            }
            assert!(
                t.elapsed() < Duration::from_secs(60),
                "timed out in state {st}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    fn run_and_verify(&self, id: &str) -> Value {
        self.ok(
            "start_execution",
            json!({"mission_id": id, "timeout_s": 30}),
        );
        let v = self.wait_idle(id);
        assert_eq!(v["mission"]["state"], "sealed", "{v}");
        self.ok(
            "start_verification",
            json!({"mission_id": id, "timeout_s": 30}),
        );
        self.wait_idle(id)
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        if self.root.starts_with(std::env::temp_dir())
            && self.root.to_string_lossy().contains("tako-bridge-test-")
        {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

const HELLO: &str = "printf 'hello\\n' > out.txt; echo agent-ran";
const CHECK: &str = "grep -q hello out.txt";

fn actions(v: &Value) -> Vec<String> {
    v["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect()
}

// ---- lifecycle ------------------------------------------------------------------------------

#[test]
fn full_lifecycle_through_the_bridge_ends_in_accept() {
    let fx = Fx::new();
    let proj = fx.ok("open_project", json!({"path": fx.p()}));
    assert_eq!(proj["project"]["branch"], "main");
    assert_eq!(proj["isolation"]["level"], "L0_worktree");
    assert_eq!(proj["isolation"]["is_security_sandbox"], false);

    let id = fx.mission(&[CHECK]);
    let v = fx.view(&id);
    assert_eq!(actions(&v), ["define_plan"]);
    let plan = fx.plan(&id, HELLO);
    assert_eq!(actions(&plan), ["approve_plan", "define_plan"]);
    assert_eq!(
        plan["mission"]["approval"],
        Value::Null,
        "nothing is pre-approved"
    );
    let perms: Vec<&str> = plan["plan"]["body"]["requested_permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert_eq!(perms, ["fs_write_workspace", "exec_command:sh"]);

    let a = fx
        .approve(&id, &plan, &["fs_write_workspace", "exec_command:sh"])
        .unwrap();
    assert_eq!(a["mission"]["state"], "approved");
    assert_eq!(actions(&a), ["start_execution", "define_plan"]);

    fx.ok("start_execution", json!({"mission_id": id}));
    let v = fx.wait_idle(&id);
    assert_eq!(v["mission"]["state"], "sealed");
    assert_eq!(v["execution"]["status"], "succeeded");
    assert!(
        v["execution"]["stdout_tail"]
            .as_str()
            .unwrap()
            .contains("agent-ran"),
        "output must come from the real run"
    );
    assert_eq!(v["execution"]["files_changed"], json!(["out.txt"]));
    assert!(v["diff"]["text"].as_str().unwrap().contains("+hello"));
    assert!(v["evidence"].is_null(), "no evidence before verification");
    assert_eq!(actions(&v), ["start_verification", "reject"]);

    fx.ok("start_verification", json!({"mission_id": id}));
    let v = fx.wait_idle(&id);
    assert_eq!(v["mission"]["state"], "evidence_ready");
    assert_eq!(v["evidence"]["result"], "pass");
    assert_eq!(v["evidence"]["acceptable"], true);
    assert_eq!(v["evidence"]["checks"][0]["exit_code"], 0);
    assert!(actions(&v).contains(&"accept".to_string()));
    assert!(
        !fx.project.join("out.txt").exists(),
        "project untouched before accept"
    );

    let done = fx.ok("accept", json!({"mission_id": id, "note": "looks good"}));
    assert_eq!(done["mission"]["state"], "accepted");
    assert_eq!(done["decision"]["decision"], "accept");
    assert_eq!(done["decision"]["result_tree_matches_verified"], true);
    assert!(actions(&done).is_empty());
    assert_eq!(
        fs::read_to_string(fx.project.join("out.txt")).unwrap(),
        "hello\n"
    );
    // the project's mission list reflects reality
    let list = fx.ok("list_missions", json!({"project_path": fx.p()}));
    assert_eq!(list[0]["state"], "accepted");
    assert_eq!(
        fx.err("accept", json!({"mission_id": id})).code,
        "already_decided"
    );
}

#[test]
fn reject_keeps_evidence_and_leaves_project_unchanged() {
    let fx = Fx::new();
    let before = git_head(&fx.project);
    let id = fx.approved(&[CHECK], HELLO);
    let v = fx.run_and_verify(&id);
    assert_eq!(v["evidence"]["acceptable"], true);
    let done = fx.ok("reject", json!({"mission_id": id, "note": "no thanks"}));
    assert_eq!(done["mission"]["state"], "rejected");
    assert_eq!(done["decision"]["decision"], "reject");
    assert_eq!(
        done["evidence"]["result"], "pass",
        "evidence survives rejection"
    );
    assert!(
        done["diff"]["text"].as_str().unwrap().contains("+hello"),
        "diff stays inspectable after reject"
    );
    assert_eq!(git_head(&fx.project), before);
    assert!(!fx.project.join("out.txt").exists());
}

fn git_head(p: &Path) -> String {
    let o = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(p)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

// ---- approval workflow --------------------------------------------------------------------------

#[test]
fn approval_is_explicit_and_never_expanded_silently() {
    let fx = Fx::new();
    let id = fx.mission(&[CHECK]);
    let plan = fx.plan(&id, HELLO);
    // executing before approval is refused
    assert_eq!(
        fx.err("start_execution", json!({"mission_id": id})).code,
        "not_approved"
    );
    // a grant the plan did not request is refused
    let e = fx.approve(&id, &plan, &["exec_command:rm"]).unwrap_err();
    assert_eq!(e.code, "unauthorized");
    // wrong hash is refused
    let e = fx
        .call(
            "approve_plan",
            json!({"mission_id": id, "version": 1, "plan_hash": "deadbeef", "granted": []}),
        )
        .unwrap_err();
    assert_eq!(e.code, "plan_hash_mismatch");
    // approving with an empty grant set succeeds but then nothing may run
    fx.approve(&id, &plan, &[]).unwrap();
    let e = fx.err("start_execution", json!({"mission_id": id}));
    let v = fx.view(&id);
    assert!(
        e.code == "unauthorized" || v["job"]["error"]["code"] == "unauthorized",
        "{e:?} {v}"
    );
    assert_eq!(v["mission"]["state"], "approved");
    assert!(
        !Path::new(v["mission"]["workspace_path"].as_str().unwrap()).exists(),
        "no workspace without grants"
    );
}

#[test]
fn changing_the_plan_voids_approval_and_the_approved_script_is_what_runs() {
    let fx = Fx::new();
    let id = fx.mission(&[CHECK]);
    let p1 = fx.plan(&id, HELLO);
    fx.approve(&id, &p1, &["fs_write_workspace", "exec_command:sh"])
        .unwrap();
    let p2 = fx.plan(&id, "printf 'other\\n' > other.txt");
    assert_eq!(p2["mission"]["approval"], Value::Null);
    assert_eq!(p2["mission"]["state"], "plan_ready");
    assert_eq!(
        fx.err("start_execution", json!({"mission_id": id})).code,
        "not_approved"
    );
    fx.approve(&id, &p2, &["fs_write_workspace", "exec_command:sh"])
        .unwrap();
    fx.ok("start_execution", json!({"mission_id": id}));
    let v = fx.wait_idle(&id);
    assert_eq!(
        v["execution"]["files_changed"],
        json!(["other.txt"]),
        "only the approved (v2) script ran"
    );
}

#[test]
fn granted_env_is_passed_only_when_requested_and_granted() {
    let fx = Fx::new();
    let id = fx.mission(&[CHECK]);
    let plan = fx.ok("define_plan", json!({"mission_id": id, "script": "printf 'hello\\n' > out.txt; env > envdump.txt", "extra_permissions": ["read_env:HOME_DIR_TEST"]}));
    let perms = plan["plan"]["body"]["requested_permissions"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(perms, 3);
    fx.approve(&id, &plan, &["fs_write_workspace", "exec_command:sh"])
        .unwrap(); // env NOT granted
    fx.ok("start_execution", json!({"mission_id": id}));
    let v = fx.wait_idle(&id);
    assert_eq!(v["mission"]["state"], "sealed");
    let ws = v["mission"]["workspace_path"].as_str().unwrap();
    let dump = fs::read_to_string(Path::new(ws).join("envdump.txt")).unwrap();
    assert!(!dump.contains("HOME_DIR_TEST"));
}

// ---- cancellation -------------------------------------------------------------------------------

#[test]
fn cancel_stops_a_running_execution() {
    let fx = Fx::new();
    let id = fx.approved(&[CHECK], "printf 'x' > partial.txt; sleep 300");
    fx.ok(
        "start_execution",
        json!({"mission_id": id, "timeout_s": 120}),
    );
    let v = fx.view(&id);
    assert_eq!(v["job"]["running"], true);
    assert_eq!(
        actions(&v),
        ["cancel"],
        "only cancel is valid while running"
    );
    fx.ok("cancel", json!({"mission_id": id}));
    let v = fx.wait_idle(&id);
    assert_eq!(v["mission"]["state"], "cancelled");
    assert_eq!(v["execution"]["status"], "cancelled");
    assert_eq!(v["execution"]["reaped"], true);
    assert_eq!(actions(&v), ["reject"]);
    fx.ok("reject", json!({"mission_id": id}));
}

#[test]
fn cancel_stops_a_running_verification() {
    let fx = Fx::new();
    let id = fx.approved(&["sleep 300"], HELLO);
    fx.ok("start_execution", json!({"mission_id": id}));
    fx.wait_idle(&id);
    fx.ok(
        "start_verification",
        json!({"mission_id": id, "timeout_s": 120}),
    );
    let t = Instant::now();
    while fx.view(&id)["mission"]["state"] != "verifying" {
        assert!(t.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(25));
    }
    fx.ok("cancel", json!({"mission_id": id}));
    let v = fx.wait_idle(&id);
    assert_eq!(
        v["mission"]["state"], "sealed",
        "state restored; no evidence recorded"
    );
    assert!(v["evidence"].is_null());
    assert_eq!(v["job"]["error"]["code"], "cancelled");
}

#[test]
fn cancel_is_refused_when_nothing_runs() {
    let fx = Fx::new();
    let id = fx.approved(&[CHECK], HELLO);
    assert_eq!(
        fx.err("cancel", json!({"mission_id": id})).code,
        "wrong_state"
    );
}

// ---- busy / duplicates --------------------------------------------------------------------------

#[test]
fn busy_missions_refuse_duplicate_and_conflicting_submissions() {
    let fx = Fx::new();
    let id = fx.approved(&[CHECK], "sleep 300");
    fx.ok(
        "start_execution",
        json!({"mission_id": id, "timeout_s": 120}),
    );
    assert_eq!(
        fx.err("start_execution", json!({"mission_id": id})).code,
        "busy",
        "duplicate submission"
    );
    assert_eq!(
        fx.err("start_verification", json!({"mission_id": id})).code,
        "busy"
    );
    assert_eq!(
        fx.err("define_plan", json!({"mission_id": id, "script": "true"}))
            .code,
        "busy"
    );
    assert_eq!(fx.err("accept", json!({"mission_id": id})).code, "busy");
    assert_eq!(fx.err("reject", json!({"mission_id": id})).code, "busy");
    assert_eq!(fx.err("recover", json!({"mission_id": id})).code, "busy");
    assert_eq!(fx.view(&id)["mission"]["state"], "executing");
    fx.ok("cancel", json!({"mission_id": id}));
    fx.wait_idle(&id);
}

// ---- unsafe repositories -------------------------------------------------------------------------

#[test]
fn unsafe_repository_is_surfaced_with_details() {
    let fx = Fx::new();
    sh(
        &fx.project,
        "git config --local filter.evil.clean 'touch SENTINEL'",
    );
    let e = fx.err("open_project", json!({"path": fx.p()}));
    assert_eq!(e.code, "unsafe_repository");
    assert_eq!(e.details, vec!["local:filter.evil.clean".to_string()]);
    assert_eq!(
        fx.err(
            "create_mission",
            json!({"project_path": fx.p(), "goal": "g", "criteria": ["c"], "checks": ["true"]})
        )
        .code,
        "unsafe_repository"
    );
}

// ---- provider refusal ----------------------------------------------------------------------------

#[test]
fn ai_providers_are_refused_and_nothing_starts() {
    let fx = Fx::new();
    let id = fx.approved(&[CHECK], HELLO);
    let e = fx.err(
        "start_execution",
        json!({"mission_id": id, "kind": "provider"}),
    );
    assert_eq!(e.code, "isolation_required");
    let v = fx.view(&id);
    assert_eq!(v["mission"]["state"], "approved");
    assert_eq!(v["job"]["running"], false);
    assert_eq!(v["isolation"]["provider_execution"], "disabled");
    assert!(!Path::new(v["mission"]["workspace_path"].as_str().unwrap()).exists());
    assert_eq!(
        fx.err("start_execution", json!({"mission_id": id, "kind": "gpt"}))
            .code,
        "invalid_input"
    );
}

// ---- evidence ------------------------------------------------------------------------------------

#[test]
fn failed_checks_and_integrity_violations_are_reported_separately_and_block_accept() {
    let fx = Fx::new();
    let id = fx.approved(&["exit 1"], HELLO);
    let v = fx.run_and_verify(&id);
    assert_eq!(v["evidence"]["failed_checks"], json!([0]));
    assert_eq!(v["evidence"]["integrity_violations"], json!([]));
    assert!(!actions(&v).contains(&"accept".to_string()));
    assert_eq!(
        fx.err("accept", json!({"mission_id": id})).code,
        "evidence_not_acceptable"
    );

    let id2 = fx.approved(&[CHECK, "echo x > new.txt"], HELLO);
    let v2 = fx.run_and_verify(&id2);
    assert_eq!(v2["evidence"]["result"], "pass");
    assert_eq!(
        v2["evidence"]["integrity_violations"],
        json!(["subject_modified"])
    );
    assert!(!actions(&v2).contains(&"accept".to_string()));
    assert_eq!(
        fx.err("accept", json!({"mission_id": id2})).code,
        "evidence_not_acceptable"
    );
    assert_eq!(git_head(&fx.project), git_head(&fx.project));
    assert!(!fx.project.join("out.txt").exists());
}

#[test]
fn a_failed_agent_has_no_evidence_and_cannot_be_accepted() {
    let fx = Fx::new();
    let id = fx.approved(&[CHECK], "echo boom >&2; exit 3");
    fx.ok("start_execution", json!({"mission_id": id}));
    let v = fx.wait_idle(&id);
    assert_eq!(v["mission"]["state"], "execution_failed");
    assert_eq!(v["execution"]["exit_code"], 3);
    assert!(v["execution"]["stderr_tail"]
        .as_str()
        .unwrap()
        .contains("boom"));
    assert!(v["evidence"].is_null());
    assert_eq!(actions(&v), ["reject"]);
    assert_eq!(
        fx.err("accept", json!({"mission_id": id})).code,
        "wrong_state"
    );
}

// ---- recovery ------------------------------------------------------------------------------------

/// Child half of the crash test; does nothing unless launched by it.
#[test]
fn helper_bridge_process() {
    let (Ok(data), Ok(id)) = (
        std::env::var("TAKO_TEST_DATA"),
        std::env::var("TAKO_TEST_MISSION"),
    ) else {
        return;
    };
    let b = Bridge::open(data).unwrap();
    b.dispatch(
        "start_execution",
        json!({"mission_id": id, "timeout_s": 600}),
    )
    .unwrap();
    std::thread::sleep(Duration::from_secs(600));
}

#[test]
fn startup_recovers_missions_interrupted_by_a_crash() {
    let fx = Fx::new();
    let id = fx.approved(&[CHECK], "echo $$ > \"$HOME/pid\"; sleep 300");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "helper_bridge_process",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("TAKO_TEST_DATA", fx.root.join("data"))
        .env("TAKO_TEST_MISSION", &id)
        .spawn()
        .unwrap();
    let t = Instant::now();
    while fx.view(&id)["mission"]["state"] != "executing"
        || fx.view(&id)["mission"]["active_run"]["pid"].is_null()
    {
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "child never started the run"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // a second bridge sees a live engine and must not touch it
    let during = Bridge::open(fx.root.join("data")).unwrap();
    assert_eq!(during.startup_report().busy, vec![id.clone()]);
    assert!(during.startup_report().recovered.is_empty());
    assert_eq!(fx.view(&id)["mission"]["state"], "executing");
    // the UI-visible offer: recover is listed because no job runs in THIS process
    assert!(actions(&fx.view(&id)).contains(&"recover".to_string()));

    child.kill().unwrap();
    child.wait().unwrap();
    let after = Bridge::open(fx.root.join("data")).unwrap();
    let rep = after.startup_report();
    assert_eq!(rep.recovered.len(), 1, "{rep:?}");
    assert_eq!(rep.recovered[0].mission_id, id);
    let v = after.view(&id).unwrap();
    assert_eq!(
        serde_json::to_value(&v.mission.state).unwrap(),
        "interrupted"
    );
    assert_eq!(v.next_actions, vec!["reject"]);
    after.dispatch("reject", json!({"mission_id": id})).unwrap();
}

#[test]
fn startup_report_is_empty_on_a_clean_data_dir() {
    let fx = Fx::new();
    let r = fx.ok("startup_report", Value::Null);
    assert_eq!(r, json!({"recovered": [], "busy": [], "errors": []}));
}

// ---- protocol ------------------------------------------------------------------------------------

#[test]
fn unknown_commands_and_bad_arguments_are_rejected() {
    let fx = Fx::new();
    assert_eq!(fx.err("format_disk", json!({})).code, "invalid_input");
    assert_eq!(
        fx.err("open_project", json!({"wrong": 1})).code,
        "invalid_input"
    );
    assert_eq!(
        fx.err("get_mission", json!({"mission_id": "../etc"})).code,
        "invalid_input"
    );
    assert_eq!(
        fx.err("get_mission", json!({"mission_id": "nope"})).code,
        "not_found"
    );
    assert_eq!(
        fx.err(
            "create_mission",
            json!({"project_path": fx.p(), "goal": "", "criteria": ["c"], "checks": ["true"]})
        )
        .code,
        "invalid_input"
    );
    assert_eq!(
        fx.err("open_project", json!({"path": fx.root.to_string_lossy()}))
            .code,
        "internal",
        "non-repository is reported, not crashed"
    );
}

#[test]
fn every_view_carries_the_isolation_notice() {
    let fx = Fx::new();
    let id = fx.mission(&[CHECK]);
    let v = fx.view(&id);
    assert_eq!(v["isolation"]["level"], "L0_worktree");
    assert!(v["isolation"]["notice"]
        .as_str()
        .unwrap()
        .contains("NOT a security sandbox"));
    assert_eq!(v["isolation"]["is_security_sandbox"], false);
}

#[test]
#[ignore = "requires an explicitly configured live gateway; deterministic coverage is in evidence_integrity.rs"]
fn backend_status_and_projects_live_integration() {
    let mut fx = Fx::new();
    fx.bridge = Bridge::open_with_client(
        fx.root.join("data"),
        tako_bridge::client::TakoBackendClient::default(),
    )
    .unwrap();
    let status_res = fx.bridge.dispatch("backend_status", Value::Null).unwrap();
    assert_eq!(status_res["online"], true);
    assert_eq!(
        status_res["operational_mode"],
        "READ_ONLY_INTEGRATION_READY"
    );

    let projects_res = fx.bridge.dispatch("backend_projects", Value::Null).unwrap();
    assert!(projects_res["total"].as_u64().unwrap() >= 1);
    assert!(projects_res["projects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["project_id"] == "era1c-confkit"));

    let gov_res = fx
        .bridge
        .dispatch("backend_governance", Value::Null)
        .unwrap();
    assert_eq!(gov_res["limits"]["spending_limit_usd"], 0.0);
    assert!(gov_res["approvals"].as_array().unwrap().len() >= 1);
}

#[test]
fn backend_t_project_fails_closed_on_mutation() {
    let fx = Fx::new();
    let err = fx.err(
        "create_mission",
        json!({
            "project_path": "/workspace/projects/t-master-sandboxes/era1c-confkit",
            "goal": "Attempt mutation",
            "criteria": ["fail"],
            "checks": ["true"]
        }),
    );
    assert_eq!(err.code, "authorization_required");
}

#[test]
fn http_client_security_and_parsing_guards() {
    use tako_bridge::client::{decode_chunked_body, parse_http_response, TakoBackendClient};

    // 1. Valid Content-Length parsing
    let resp = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\n\r\n{\"status\":\"ok\"}";
    let (code, body) = parse_http_response(resp).unwrap();
    assert_eq!(code, 200);
    assert_eq!(body, b"{\"status\":\"ok\"}");

    // 2. Premature EOF detection (< Content-Length)
    let truncated = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{\"short\":1}";
    let err = parse_http_response(truncated).unwrap_err();
    assert!(err.contains("Premature EOF"));

    // 3. Chunked transfer encoding decoder
    let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n6\r\npedia \r\n0\r\n\r\n";
    let (code, body) = parse_http_response(chunked).unwrap();
    assert_eq!(code, 200);
    assert_eq!(String::from_utf8(body).unwrap(), "Wikipedia ");

    // 4. Malformed chunked stream
    let bad_chunk = b"4\r\nWiki"; // missing trailing CRLF & 0 chunk
    assert!(decode_chunked_body(bad_chunk).is_err());

    // 5. Malformed HTTP status line
    let bad_status = b"INVALID STATUS\r\n\r\n";
    assert!(parse_http_response(bad_status)
        .unwrap_err()
        .contains("Malformed HTTP status line"));

    // 6. Security violation: non-loopback address rejected
    let remote_client = TakoBackendClient::unauthenticated("8.8.8.8", 80);
    let err = remote_client.raw_get("/test", false).unwrap_err();
    assert!(err.contains("Security violation: Only loopback addresses"));
}

#[test]
#[ignore = "requires an explicitly configured live gateway"]
fn workstations_discovery_live_integration() {
    let client = tako_bridge::client::TakoBackendClient::default();
    let res = client
        .list_workstations()
        .expect("list_workstations failed");
    assert!(
        res.total >= 2,
        "Expected at least 2 configured workstations"
    );

    // Workstation (Linux CachyOS)
    let ws_linux = res
        .workstations
        .iter()
        .find(|w| w.workstation_id == "workstation-linux-cachyos");
    assert!(ws_linux.is_some(), "Linux workstation must be present");
    let ws_linux = ws_linux.unwrap();
    assert_eq!(ws_linux.os, "linux");
    assert!(ws_linux.is_local);
    assert_eq!(ws_linux.connectivity_status, "connected");

    // Mac (macOS ITJ-1060)
    let ws_mac = res
        .workstations
        .iter()
        .find(|w| w.workstation_id == "workstation-mac-itj1060");
    assert!(ws_mac.is_some(), "Mac workstation must be present");
    let ws_mac = ws_mac.unwrap();
    assert_eq!(ws_mac.os, "macOS");
    assert!(!ws_mac.is_local);
    assert_eq!(ws_mac.transport, "tailscale-ssh");

    // Test get_workstation single item lookup
    let single = client
        .get_workstation("workstation-mac-itj1060")
        .expect("get_workstation failed");
    assert_eq!(single.workstation_id, "workstation-mac-itj1060");
    assert_eq!(single.display_name, "MacBook Pro (ITJ-1060)");
}

// TAKO-006: persisted counters/state never substitute for supporting records.
#[test]
fn missing_or_modified_evidence_cannot_be_displayed_as_verified_or_accepted() {
    for accepted in [false, true] {
        let fx = Fx::new();
        let id = fx.approved(&[CHECK], HELLO);
        fx.run_and_verify(&id);
        if accepted {
            fx.ok("accept", json!({"mission_id": id}));
        }
        let path = fx
            .root
            .join("data/missions")
            .join(&id)
            .join("evidence-1.json");
        let original = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(
            fx.err("get_mission", json!({"mission_id": id})).code,
            "evidence_unavailable"
        );
        assert_eq!(
            fx.err("list_missions", json!({"project_path": fx.p()}))
                .code,
            "evidence_unavailable"
        );
        fs::write(&path, b"{}").unwrap();
        assert_eq!(
            fx.err("get_mission", json!({"mission_id": id})).code,
            "evidence_unverified"
        );
        fs::write(&path, original).unwrap();
        assert!(fx.view(&id)["evidence"]["valid"].as_bool().unwrap());
    }
}

#[test]
fn accepted_state_requires_matching_decision_and_evidence() {
    let fx = Fx::new();
    let id = fx.approved(&[CHECK], HELLO);
    fx.run_and_verify(&id);
    fx.ok("accept", json!({"mission_id": id}));
    let dir = fx.root.join("data/missions").join(&id);
    fs::remove_file(dir.join("decision.json")).unwrap();
    assert_eq!(
        fx.err("get_mission", json!({"mission_id": id})).code,
        "evidence_unverified"
    );
    let path = dir.join("mission.json");
    let mut mission: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    mission["evidence_count"] = json!(0);
    fs::write(path, serde_json::to_vec(&mission).unwrap()).unwrap();
    assert_eq!(
        fx.err("get_mission", json!({"mission_id": id})).code,
        "evidence_unavailable"
    );
}

#[test]
fn unreadable_backend_prefixed_records_do_not_fall_back_to_success() {
    let fx = Fx::new();
    for id in ["PROG-corrupt", "GOAL-corrupt", "T-corrupt"] {
        let dir = fx.root.join("data/missions").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("mission.json"), b"not json").unwrap();
        assert_eq!(
            fx.err("get_mission", json!({"mission_id": id})).code,
            "internal"
        );
    }
}

#[test]
fn unsupported_isolation_record_is_never_shown_as_sandboxed() {
    let fx = Fx::new();
    let id = fx.approved(&[CHECK], HELLO);
    fx.run_and_verify(&id);
    let path = fx
        .root
        .join("data/missions")
        .join(&id)
        .join("exec-1/execution.json");
    let mut execution: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    execution["isolation_level"] = json!("L1_bubblewrap");
    fs::write(path, serde_json::to_vec(&execution).unwrap()).unwrap();
    assert_eq!(
        fx.err("get_mission", json!({"mission_id": id})).code,
        "evidence_unverified"
    );
}
