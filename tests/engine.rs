//! Integration tests for the TAKO engine. All fixtures are disposable Git repos
//! under the system temp dir; no real project is ever touched.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tako_engine::git;
use tako_engine::*;

static N: AtomicU32 = AtomicU32::new(0);
const HUMAN: fn() -> Actor = || Actor::Human("tester".into());

struct Fx {
    root: PathBuf,
    project: PathBuf,
    engine: Engine,
}

impl Fx {
    fn new() -> Fx {
        Self::with_env(BTreeMap::new())
    }
    fn with_env(env: BTreeMap<String, String>) -> Fx {
        let root = std::env::temp_dir().join(format!(
            "tako-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        sh(&project, "git init -q -b main && git config user.name t && git config user.email t@t && printf 'base\\n' > README.md && git add -A && git commit -qm init");
        let engine = Engine::with_env(root.join("data"), env).unwrap();
        Fx {
            root,
            project,
            engine,
        }
    }
    fn mission(&self, checks: Vec<Vec<&str>>) -> Mission {
        let p = self.engine.open_project(&self.project).unwrap();
        self.engine
            .create_mission(
                &p,
                "add out.txt",
                vec!["out.txt exists".into()],
                checks
                    .into_iter()
                    .map(|c| c.into_iter().map(String::from).collect())
                    .collect(),
            )
            .unwrap()
    }
    /// Plan requesting standard grants; returns plan.
    fn plan(&self, id: &str, extra: &[&str]) -> Plan {
        let mut perms = vec![
            "fs_write_workspace".to_string(),
            "exec_command:sh".to_string(),
        ];
        perms.extend(extra.iter().map(|s| s.to_string()));
        self.engine
            .submit_plan(
                id,
                PlanBody {
                    summary: "write out.txt".into(),
                    steps: vec!["write".into()],
                    files_expected: vec!["out.txt".into()],
                    requested_permissions: perms,
                    risks: vec![],
                },
            )
            .unwrap()
    }
    fn approve_all(&self, plan: &Plan) -> Approval {
        self.engine
            .approve_plan(
                &plan.mission_id,
                plan.version,
                &plan.hash,
                plan.body.requested_permissions.clone(),
                &HUMAN(),
            )
            .unwrap()
    }
    fn approved(&self, checks: Vec<Vec<&str>>) -> Mission {
        let m = self.mission(checks);
        let p = self.plan(&m.id, &[]);
        self.approve_all(&p);
        m
    }
    fn run(&self, id: &str, script: &str, timeout_s: u64) -> Execution {
        self.engine
            .run_execution(id, &agent(script), Duration::from_secs(timeout_s))
            .unwrap()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        if self.root.starts_with(std::env::temp_dir())
            && self.root.to_string_lossy().contains("tako-test-")
        {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

fn agent(script: &str) -> AgentSpec {
    AgentSpec {
        program: "sh".into(),
        args: vec!["-c".into(), script.into()],
        env: vec![],
        kind: AgentKind::Scripted,
    }
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
fn pass_check() -> Vec<&'static str> {
    vec!["sh", "-c", "grep -q hello out.txt"]
}
const GOOD: &str = "printf 'hello\\n' > out.txt";

/// Everything observable about the project except .git internals that legitimately grow.
fn project_state(p: &Path) -> (String, String, String, String) {
    let h = git::head(p).unwrap();
    let refs = git::git_ok(p, &["for-each-ref", "--format=%(refname) %(objectname)"]).unwrap();
    let wt = git::git_ok(p, &["worktree", "list", "--porcelain"])
        .unwrap()
        .lines()
        .filter(|l| l.starts_with("worktree "))
        .count()
        .to_string();
    let files = Command::new("sh")
        .arg("-c")
        .arg("find . -path ./.git -prune -o -type f -print | sort | xargs sha256sum")
        .current_dir(p)
        .output()
        .unwrap();
    (
        format!("{h}\n{refs}"),
        String::from_utf8_lossy(&files.stdout).into_owned(),
        git::porcelain(p).unwrap(),
        wt,
    )
}
fn raw_snapshot(p: &Path) -> String {
    let o = Command::new("sh")
        .arg("-c")
        .arg("find . -type f | sort | xargs sha256sum")
        .current_dir(p)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned()
}

// ---- AC1: open project is read-only ----------------------------------------------------
#[test]
fn ac01_open_project_is_byte_identical() {
    let fx = Fx::new();
    fs::write(fx.project.join("dirty.txt"), "x").unwrap();
    let before = raw_snapshot(&fx.project);
    let p = fx.engine.open_project(&fx.project).unwrap();
    assert!(p.dirty);
    assert_eq!(p.branch, "main");
    assert_eq!(before, raw_snapshot(&fx.project));
}

// ---- AC2: no execution without approved plan + grants ----------------------------------
#[test]
fn ac02_no_execution_without_approval() {
    let fx = Fx::new();
    let m = fx.mission(vec![pass_check()]);
    let e = fx
        .engine
        .run_execution(&m.id, &agent(GOOD), Duration::from_secs(5))
        .unwrap_err();
    assert!(matches!(e, EngineError::NotApproved(_)), "{e}");
    let _p = fx.plan(&m.id, &[]); // planned but not approved
    let e = fx
        .engine
        .run_execution(&m.id, &agent(GOOD), Duration::from_secs(5))
        .unwrap_err();
    assert!(matches!(e, EngineError::NotApproved(_)), "{e}");
    assert!(
        !PathBuf::from(&m.workspace_path).exists(),
        "no workspace may exist before approval"
    );
}

#[test]
fn unauthorized_capability_fails_closed() {
    let fx = Fx::new();
    let m = fx.mission(vec![pass_check()]);
    let p = fx.plan(&m.id, &[]);
    // grant only a subset: no exec_command:sh
    fx.engine
        .approve_plan(
            &m.id,
            p.version,
            &p.hash,
            vec!["fs_write_workspace".into()],
            &HUMAN(),
        )
        .unwrap();
    let e = fx
        .engine
        .run_execution(&m.id, &agent(GOOD), Duration::from_secs(5))
        .unwrap_err();
    assert!(matches!(e, EngineError::Unauthorized(_)), "{e}");
    assert!(!PathBuf::from(&m.workspace_path).exists());
    // granting something the plan never requested is refused
    let m2 = fx.mission(vec![pass_check()]);
    let p2 = fx.plan(&m2.id, &[]);
    let e = fx
        .engine
        .approve_plan(
            &m2.id,
            p2.version,
            &p2.hash,
            vec!["exec_command:rm".into()],
            &HUMAN(),
        )
        .unwrap_err();
    assert!(matches!(e, EngineError::Unauthorized(_)), "{e}");
    // network cannot be enforced at L0: refused at plan time
    let e = fx
        .engine
        .submit_plan(
            &m2.id,
            PlanBody {
                summary: "s".into(),
                steps: vec![],
                files_expected: vec![],
                requested_permissions: vec!["network".into()],
                risks: vec![],
            },
        )
        .unwrap_err();
    assert!(matches!(e, EngineError::Unenforceable(_)), "{e}");
    // env var without grant
    let m3 = fx.approved(vec![pass_check()]);
    let mut a = agent(GOOD);
    a.env = vec!["SECRET".into()];
    let e = fx
        .engine
        .run_execution(&m3.id, &a, Duration::from_secs(5))
        .unwrap_err();
    assert!(matches!(e, EngineError::Unauthorized(_)), "{e}");
}

// ---- AC3: editing the plan clears approval ---------------------------------------------
#[test]
fn ac03_plan_change_voids_approval() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    assert_eq!(fx.engine.load(&m.id).unwrap().state, MissionState::Approved);
    let v2 = fx.plan(&m.id, &[]);
    assert_eq!(v2.version, 2);
    let after = fx.engine.load(&m.id).unwrap();
    assert!(after.approval.is_none());
    assert_eq!(after.state, MissionState::PlanReady);
    let e = fx
        .engine
        .run_execution(&m.id, &agent(GOOD), Duration::from_secs(5))
        .unwrap_err();
    assert!(matches!(e, EngineError::NotApproved(_)));
    // stale version / wrong hash cannot be approved
    let e = fx
        .engine
        .approve_plan(&m.id, 1, &v2.hash, vec![], &HUMAN())
        .unwrap_err();
    assert!(matches!(e, EngineError::InvalidInput(_)), "{e}");
    let e = fx
        .engine
        .approve_plan(&m.id, 2, "deadbeef", vec![], &HUMAN())
        .unwrap_err();
    assert!(matches!(e, EngineError::PlanHashMismatch), "{e}");
}

#[test]
fn plan_file_tampered_after_approval_is_refused() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    let path = PathBuf::from(&m.workspace_path)
        .parent()
        .unwrap()
        .join("plan-v1.json");
    let t = fs::read_to_string(&path)
        .unwrap()
        .replace("write out.txt", "do something else");
    fs::write(&path, t).unwrap();
    let e = fx
        .engine
        .run_execution(&m.id, &agent(GOOD), Duration::from_secs(5))
        .unwrap_err();
    assert!(matches!(e, EngineError::PlanHashMismatch), "{e}");
}

#[test]
fn agent_cannot_approve_or_decide() {
    let fx = Fx::new();
    let m = fx.mission(vec![pass_check()]);
    let p = fx.plan(&m.id, &[]);
    let bot = Actor::Agent("tako".into());
    assert!(matches!(
        fx.engine.approve_plan(&m.id, 1, &p.hash, vec![], &bot),
        Err(EngineError::NotHuman)
    ));
    assert!(matches!(
        fx.engine.accept(&m.id, &bot, ""),
        Err(EngineError::NotHuman)
    ));
    assert!(matches!(
        fx.engine.reject(&m.id, &bot, ""),
        Err(EngineError::NotHuman)
    ));
}

// ---- AC4: agent edits only inside the worktree -----------------------------------------
#[test]
fn ac04_execution_is_confined_to_worktree() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    let before = project_state(&fx.project);
    let x = fx.run(&m.id, GOOD, 10);
    assert!(matches!(x.status, ExecStatus::Succeeded));
    assert_eq!(x.exit_code, Some(0));
    assert_eq!(x.files_changed, vec!["out.txt".to_string()]);
    assert!(!x.project_touched);
    assert_eq!(x.isolation_level, "L0_worktree");
    assert!(PathBuf::from(&m.workspace_path).join("out.txt").exists());
    assert!(!fx.project.join("out.txt").exists());
    let mut after = project_state(&fx.project);
    // only the new mission branch and worktree entry may differ
    assert_eq!(before.1, after.1, "project files unchanged");
    assert_eq!(before.2, after.2, "project status unchanged");
    assert_eq!(git::head(&fx.project).unwrap(), m.base_commit);
    after.3 = before.3.clone();
    assert_eq!(fx.engine.load(&m.id).unwrap().state, MissionState::Sealed);
}

#[test]
fn project_touch_is_detected_best_effort() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    let script = format!("{GOOD}; echo bad > {}/escaped.txt", fx.project.display());
    let x = fx.run(&m.id, &script, 10);
    assert!(x.project_touched);
    assert!(x.note.unwrap().contains("project working tree"));
}

#[test]
fn failed_agent_execution() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    let x = fx.run(
        &m.id,
        "printf 'partial\\n' > out.txt; echo oops >&2; exit 3",
        10,
    );
    assert!(matches!(x.status, ExecStatus::Failed));
    assert_eq!(x.exit_code, Some(3));
    assert!(fs::read_to_string(&x.stderr_path).unwrap().contains("oops"));
    assert_eq!(
        fx.engine.load(&m.id).unwrap().state,
        MissionState::ExecutionFailed
    );
    // verification refuses a failed execution
    assert!(matches!(
        fx.engine.verify(&m.id, Duration::from_secs(5)),
        Err(EngineError::WrongState { .. })
    ));
    // agent that does nothing is also not sealed
    let m2 = fx.approved(vec![pass_check()]);
    fx.run(&m2.id, "true", 10);
    assert_eq!(
        fx.engine.load(&m2.id).unwrap().state,
        MissionState::ExecutionFailed
    );
}

// ---- AC5: timeout kills the whole process group incl. grandchild ----------------------
#[test]
fn ac05_timeout_kills_process_group() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    let started = std::time::Instant::now();
    let x = fx.run(&m.id, "sleep 300 & echo $! > \"$HOME/bg.pid\"; wait", 1);
    assert!(matches!(x.status, ExecStatus::Timeout));
    assert!(started.elapsed() < Duration::from_secs(20));
    let pid: u32 = fs::read_to_string(
        PathBuf::from(&m.workspace_path)
            .parent()
            .unwrap()
            .join("home/bg.pid"),
    )
    .unwrap()
    .trim()
    .parse()
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        !tako_engine::runner::pid_alive(pid),
        "grandchild {pid} survived"
    );
    assert_eq!(fx.engine.load(&m.id).unwrap().state, MissionState::TimedOut);
}

// ---- AC6: environment allowlist ---------------------------------------------------------
#[test]
fn ac06_environment_allowlist() {
    let mut env = BTreeMap::new();
    env.insert("CANARY_SECRET".to_string(), "s3cr3t-canary".to_string());
    env.insert("ALLOWED_VAR".to_string(), "visible".to_string());
    let fx = Fx::with_env(env);
    let m = fx.mission(vec![vec!["sh", "-c", "true"]]);
    let p = fx.plan(&m.id, &["read_env:ALLOWED_VAR"]);
    fx.approve_all(&p);
    let mut a = agent("env > env_dump.txt");
    a.env = vec!["ALLOWED_VAR".into()];
    fx.engine
        .run_execution(&m.id, &a, Duration::from_secs(5))
        .unwrap();
    let dump = fs::read_to_string(PathBuf::from(&m.workspace_path).join("env_dump.txt")).unwrap();
    assert!(
        !dump.contains("CANARY_SECRET") && !dump.contains("s3cr3t-canary"),
        "{dump}"
    );
    assert!(dump.contains("ALLOWED_VAR=visible"));
    let keys: Vec<&str> = dump.lines().filter_map(|l| l.split('=').next()).collect();
    for k in &keys {
        assert!(
            [
                "PATH",
                "HOME",
                "LANG",
                "TAKO_MISSION_ID",
                "TAKO_WORKSPACE",
                "ALLOWED_VAR",
                "PWD",
                "SHLVL",
                "_",
                "OLDPWD"
            ]
            .contains(k),
            "unexpected var {k}"
        );
    }
}

// ---- AC7: passing and failing verification ---------------------------------------------
#[test]
fn ac07_verification_pass_and_fail() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check(), vec!["sh", "-c", "echo checked"]]);
    fx.run(&m.id, GOOD, 10);
    let ev = fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    assert_eq!(ev.result, "pass");
    assert!(ev.valid && ev.acceptable && !ev.verification_modified_subject);
    assert_eq!(ev.checks.len(), 2);
    assert_eq!(ev.checks[1].stdout_tail.trim(), "checked");
    assert_eq!(ev.tree_hash_before_verify, ev.subject_tree_hash);
    assert_eq!(ev.tree_hash_after_verify, ev.subject_tree_hash);

    let m2 = fx.approved(vec![vec!["sh", "-c", "echo nope >&2; exit 1"]]);
    fx.run(&m2.id, GOOD, 10);
    let ev2 = fx.engine.verify(&m2.id, Duration::from_secs(10)).unwrap();
    assert_eq!(ev2.result, "fail");
    assert!(ev2.valid && !ev2.acceptable);
    assert!(ev2.checks[0].stderr_tail.contains("nope"));
    // failed evidence cannot be accepted, but can be rejected, and is preserved
    assert!(matches!(
        fx.engine.accept(&m2.id, &HUMAN(), ""),
        Err(EngineError::EvidenceNotAcceptable(_))
    ));
    fx.engine.reject(&m2.id, &HUMAN(), "failing").unwrap();
    assert!(fx.engine.load_evidence(&m2.id, 1).is_ok());
}

#[test]
fn agent_claims_are_not_evidence() {
    let fx = Fx::new();
    let m = fx.approved(vec![vec!["sh", "-c", "test -f out.txt"]]);
    // agent prints success but produces nothing the check accepts
    let x = fx.run(&m.id, "echo ALL TESTS PASSED; printf x > other.txt", 10);
    assert!(fs::read_to_string(&x.stdout_path)
        .unwrap()
        .contains("ALL TESTS PASSED"));
    let ev = fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    assert_eq!(ev.result, "fail");
    assert!(!ev.acceptable);
}

// ---- AC8: verification mutation detected -----------------------------------------------
#[test]
fn ac08_verification_mutation_invalidates_evidence() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check(), vec!["sh", "-c", "echo x > mutated.txt"]]);
    fx.run(&m.id, GOOD, 10);
    let ev = fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    assert_eq!(ev.result, "pass", "all commands exit 0");
    assert!(ev.verification_modified_subject);
    assert!(!ev.valid && !ev.acceptable);
    assert_ne!(ev.tree_hash_before_verify, ev.tree_hash_after_verify);
    assert!(matches!(
        fx.engine.accept(&m.id, &HUMAN(), ""),
        Err(EngineError::EvidenceNotAcceptable(_))
    ));
    // sealed branch itself was not modified
    assert_eq!(
        git::tree_of(&fx.project, &format!("refs/heads/{}", m.branch)).unwrap(),
        ev.subject_tree_hash
    );
}

#[test]
fn verification_is_reproducible() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    fx.run(&m.id, GOOD, 10);
    let a = fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    let b = fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    assert_ne!(a.id, b.id);
    assert_eq!(
        (
            a.result,
            a.subject_tree_hash,
            a.candidate_commit,
            a.acceptable
        ),
        (
            b.result,
            b.subject_tree_hash,
            b.candidate_commit,
            b.acceptable
        )
    );
}

// ---- AC9: accept / reject --------------------------------------------------------------
#[test]
fn ac09_accept_fast_forward_and_repeat_refused() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    fx.run(&m.id, GOOD, 10);
    let ev = fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    let d = fx.engine.accept(&m.id, &HUMAN(), "ship it").unwrap();
    assert_eq!(d.result_tree_matches_verified, Some(true));
    assert_eq!(git::head(&fx.project).unwrap(), ev.candidate_commit);
    assert_eq!(
        fs::read_to_string(fx.project.join("out.txt")).unwrap(),
        "hello\n"
    );
    assert!(!git::is_dirty(&fx.project).unwrap());
    assert_eq!(fx.engine.load(&m.id).unwrap().state, MissionState::Accepted);
    // repeated / contradictory decisions
    assert!(matches!(
        fx.engine.accept(&m.id, &HUMAN(), ""),
        Err(EngineError::AlreadyDecided)
    ));
    assert!(matches!(
        fx.engine.reject(&m.id, &HUMAN(), ""),
        Err(EngineError::AlreadyDecided)
    ));
    assert!(git::git_ok(&fx.project, &["branch", "--list", "tako/*"])
        .unwrap()
        .is_empty());
}

#[test]
fn accept_clean_merge_when_head_moved() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    fx.run(&m.id, GOOD, 10);
    fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    sh(
        &fx.project,
        "printf 'other\\n' > other.txt && git add -A && git commit -qm other",
    );
    let d = fx.engine.accept(&m.id, &HUMAN(), "").unwrap();
    assert_eq!(
        d.result_tree_matches_verified,
        Some(false),
        "merged tree differs from verified tree and is reported"
    );
    assert!(fx.project.join("out.txt").exists() && fx.project.join("other.txt").exists());
}

#[test]
fn accept_conflict_leaves_project_unchanged() {
    let fx = Fx::new();
    let m = fx.approved(vec![vec!["sh", "-c", "grep -q mission README.md"]]);
    fx.run(&m.id, "printf 'mission\\n' > README.md", 10);
    fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    sh(
        &fx.project,
        "printf 'conflicting\\n' > README.md && git add -A && git commit -qm conflict",
    );
    let before = project_state(&fx.project);
    let e = fx.engine.accept(&m.id, &HUMAN(), "").unwrap_err();
    assert!(matches!(e, EngineError::Conflict(_)), "{e}");
    assert_eq!(before, project_state(&fx.project));
    assert!(!fx.project.join(".git/MERGE_HEAD").exists());
    assert_eq!(
        fx.engine.load(&m.id).unwrap().state,
        MissionState::EvidenceReady
    );
    // the human can still reject; project stays unchanged
    fx.engine.reject(&m.id, &HUMAN(), "conflict").unwrap();
    assert_eq!(
        fs::read_to_string(fx.project.join("README.md")).unwrap(),
        "conflicting\n"
    );
}

#[test]
fn accept_refused_on_dirty_project() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    fx.run(&m.id, GOOD, 10);
    fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    fs::write(fx.project.join("README.md"), "user edit\n").unwrap();
    assert!(matches!(
        fx.engine.accept(&m.id, &HUMAN(), ""),
        Err(EngineError::DirtyProject)
    ));
    assert_eq!(
        fs::read_to_string(fx.project.join("README.md")).unwrap(),
        "user edit\n"
    );
}

#[test]
fn accept_refused_if_candidate_changed_after_verification() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    fx.run(&m.id, GOOD, 10);
    fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    let ws = PathBuf::from(&m.workspace_path);
    sh(
        &ws,
        "echo tamper >> out.txt && git add -A && git commit -qm tamper",
    );
    assert!(matches!(
        fx.engine.accept(&m.id, &HUMAN(), ""),
        Err(EngineError::EvidenceNotAcceptable(_))
    ));
}

#[test]
fn reject_preserves_evidence_and_original_project() {
    let fx = Fx::new();
    let before = project_state(&fx.project);
    let m = fx.approved(vec![pass_check()]);
    fx.run(&m.id, GOOD, 10);
    let ev = fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    let d = fx.engine.reject(&m.id, &HUMAN(), "not needed").unwrap();
    assert_eq!(d.decision, "reject");
    assert_eq!(d.evidence_id.as_deref(), Some(ev.id.as_str()));
    assert_eq!(
        before,
        project_state(&fx.project),
        "HEAD, refs, files, status and worktrees restored"
    );
    assert!(!PathBuf::from(&m.workspace_path).exists());
    assert_eq!(fx.engine.load_evidence(&m.id, 1).unwrap().result, "pass");
    assert_eq!(fx.engine.load(&m.id).unwrap().state, MissionState::Rejected);
    assert!(matches!(
        fx.engine.reject(&m.id, &HUMAN(), ""),
        Err(EngineError::AlreadyDecided)
    ));
}

// ---- AC10: killing the engine mid-run --------------------------------------------------
/// Child half: only does work when launched by the crash tests with the env vars set.
#[test]
fn helper_engine_process() {
    let (Ok(data), Ok(id)) = (
        std::env::var("TAKO_TEST_DATA"),
        std::env::var("TAKO_TEST_MISSION"),
    ) else {
        return;
    };
    let e = Engine::with_env(data, BTreeMap::new()).unwrap();
    match std::env::var("TAKO_TEST_OP").as_deref() {
        Ok("verify") => {
            let _ = e.verify(&id, Duration::from_secs(600));
        }
        _ => {
            let _ = e.run_execution(
                &id,
                &agent("sleep 300 & echo $! > \"$HOME/bg.pid\"; wait"),
                Duration::from_secs(600),
            );
        }
    }
}

#[test]
fn ac10_killed_engine_leaves_recoverable_interrupted_state() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    let data = fx.root.join("data");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "helper_engine_process",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("TAKO_TEST_DATA", &data)
        .env("TAKO_TEST_MISSION", &m.id)
        .spawn()
        .unwrap();
    let bg = PathBuf::from(&m.workspace_path)
        .parent()
        .unwrap()
        .join("home/bg.pid");
    let t0 = std::time::Instant::now();
    while !bg.exists() && t0.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(bg.exists(), "agent did not start");
    std::thread::sleep(Duration::from_millis(200));
    let bgpid: u32 = fs::read_to_string(&bg).unwrap().trim().parse().unwrap();
    assert_eq!(
        fx.engine.load(&m.id).unwrap().state,
        MissionState::Executing
    );
    assert!(
        fx.engine.recover(&m.id).is_err(),
        "must not recover while engine is alive"
    );
    child.kill().unwrap(); // SIGKILL: no cleanup runs
    child.wait().unwrap();
    assert!(
        tako_engine::runner::pid_alive(bgpid),
        "orphaned agent still running before recovery"
    );
    assert_eq!(fx.engine.recover(&m.id).unwrap(), MissionState::Interrupted);
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        !tako_engine::runner::pid_alive(bgpid),
        "recovery must kill the orphaned process group"
    );
    let mm = fx.engine.load(&m.id).unwrap();
    assert_eq!(mm.state, MissionState::Interrupted);
    assert!(mm.active_run.is_none());
    fx.engine.reject(&m.id, &HUMAN(), "interrupted").unwrap();
    assert!(!PathBuf::from(&m.workspace_path).exists());
}

// ---- state is reconstructable from disk --------------------------------------------------
#[test]
fn ac_state_reconstructable_from_disk_by_new_engine() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    fx.run(&m.id, GOOD, 10);
    fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    let e2 = Engine::with_env(fx.root.join("data"), BTreeMap::new()).unwrap();
    let mm = e2.load(&m.id).unwrap();
    assert_eq!(mm.state, MissionState::EvidenceReady);
    assert!(e2.load_evidence(&m.id, 1).unwrap().acceptable);
    assert!(e2.events(&m.id).unwrap().len() >= 5);
    e2.accept(&m.id, &HUMAN(), "").unwrap();
}

#[test]
fn input_validation_and_data_dir_placement() {
    let fx = Fx::new();
    let p = fx.engine.open_project(&fx.project).unwrap();
    assert!(fx
        .engine
        .create_mission(&p, "", vec!["a".into()], vec![vec!["true".into()]])
        .is_err());
    assert!(fx
        .engine
        .create_mission(&p, "g", vec![], vec![vec!["true".into()]])
        .is_err());
    assert!(fx
        .engine
        .create_mission(&p, "g", vec!["a".into()], vec![])
        .is_err());
    // engine data inside the project is refused
    let inside = Engine::with_env(fx.project.join("data"), BTreeMap::new()).unwrap();
    assert!(inside
        .create_mission(&p, "g", vec!["a".into()], vec![vec!["true".into()]])
        .is_err());
    assert!(fx.engine.load("../etc").is_err());
    // non-git directory is rejected
    let plain = fx.root.join("plain");
    fs::create_dir_all(&plain).unwrap();
    assert!(fx.engine.open_project(&plain).is_err());
}

#[test]
fn approval_is_single_use() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    fx.run(&m.id, GOOD, 10);
    let e = fx
        .engine
        .run_execution(&m.id, &agent(GOOD), Duration::from_secs(5))
        .unwrap_err();
    assert!(matches!(e, EngineError::NotApproved(_)), "{e}");
}

// =====================================================================================
// TAKO-003H hardening tests
// =====================================================================================

fn mdir_of(m: &Mission) -> PathBuf {
    PathBuf::from(&m.workspace_path)
        .parent()
        .unwrap()
        .to_path_buf()
}
fn wait_until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t = std::time::Instant::now();
    while !f() {
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}
fn read_pid(p: &Path) -> Option<u32> {
    fs::read_to_string(p).ok()?.trim().parse().ok()
}
fn sentinel(fx: &Fx) -> PathBuf {
    fx.root.join("SENTINEL")
}
fn git_cfg(dir: &Path, args: &[&str]) {
    let mut a = vec!["config"];
    a.extend_from_slice(args);
    git::git_ok(dir, &a).unwrap();
}
fn verified(fx: &Fx, checks: Vec<Vec<&str>>) -> (Mission, Evidence) {
    let m = fx.approved(checks);
    fx.run(&m.id, GOOD, 10);
    let ev = fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    (m, ev)
}

// ---- WP1: cancellation and reaping -----------------------------------------------------
#[test]
fn cancel_kills_term_ignoring_group_and_reaps() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    let home = mdir_of(&m).join("home");
    let script = "trap '' TERM; (trap '' TERM; sleep 300) & echo $! > \"$HOME/bg.pid\"; echo $$ > \"$HOME/leader.pid\"; while :; do sleep 1; done";
    let t0 = std::time::Instant::now();
    let x = std::thread::scope(|s| {
        let h = s.spawn(|| {
            fx.engine
                .run_execution(&m.id, &agent(script), Duration::from_secs(120))
                .unwrap()
        });
        wait_until("agent pids", 20, || {
            read_pid(&home.join("bg.pid")).is_some() && read_pid(&home.join("leader.pid")).is_some()
        });
        assert_eq!(
            fx.engine.load(&m.id).unwrap().state,
            MissionState::Executing
        );
        fx.engine.cancel(&m.id).unwrap();
        h.join().unwrap()
    });
    assert!(t0.elapsed() < Duration::from_secs(30));
    assert!(matches!(x.status, ExecStatus::Cancelled), "{:?}", x.status);
    assert!(x.reaped);
    let note = x.note.clone().unwrap_or_default();
    assert!(
        note.starts_with("cancelled"),
        "the cancel reason must stay visible: {note}"
    );
    let (bg, leader) = (
        read_pid(&home.join("bg.pid")).unwrap(),
        read_pid(&home.join("leader.pid")).unwrap(),
    );
    assert!(
        !tako_engine::runner::pid_alive(bg),
        "grandchild survived cancellation"
    );
    assert!(
        !tako_engine::runner::pid_exists(leader),
        "leader was not reaped (zombie or alive)"
    );
    let mm = fx.engine.load(&m.id).unwrap();
    assert_eq!(mm.state, MissionState::Cancelled);
    assert!(mm.active_run.is_none());
    assert!(
        !mdir_of(&m).join("cancel").exists(),
        "cancel flag must be consumed"
    );
    // cancelled work is inspectable and rejectable; verification refuses it
    assert!(matches!(
        fx.engine.verify(&m.id, Duration::from_secs(5)),
        Err(EngineError::WrongState { .. })
    ));
    fx.engine.reject(&m.id, &HUMAN(), "cancelled").unwrap();
}

#[test]
fn cancel_when_nothing_is_running_is_refused() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    assert!(matches!(
        fx.engine.cancel(&m.id),
        Err(EngineError::WrongState { .. })
    ));
    fx.run(&m.id, GOOD, 10);
    assert!(matches!(
        fx.engine.cancel(&m.id),
        Err(EngineError::WrongState { .. })
    ));
    assert!(
        !mdir_of(&m).join("cancel").exists(),
        "no stale flag may be left behind"
    );
}

#[test]
fn cancel_during_verification_restores_state_and_leaves_no_evidence() {
    let fx = Fx::new();
    let m = fx.approved(vec![vec![
        "sh",
        "-c",
        "echo $$ > \"$HOME/check.pid\"; sleep 300",
    ]]);
    fx.run(&m.id, GOOD, 10);
    let dir = mdir_of(&m);
    let r = std::thread::scope(|s| {
        let h = s.spawn(|| fx.engine.verify(&m.id, Duration::from_secs(120)));
        wait_until("check start", 20, || {
            read_pid(&dir.join("verify-1-home/check.pid")).is_some()
        });
        assert_eq!(
            fx.engine.load(&m.id).unwrap().state,
            MissionState::Verifying
        );
        fx.engine.cancel(&m.id).unwrap();
        h.join().unwrap()
    });
    assert!(matches!(r, Err(EngineError::Cancelled)), "{r:?}");
    let pid = read_pid(&dir.join("verify-1-home/check.pid")).unwrap();
    assert!(
        !tako_engine::runner::pid_exists(pid),
        "verification check not killed and reaped"
    );
    let mm = fx.engine.load(&m.id).unwrap();
    assert_eq!(mm.state, MissionState::Sealed);
    assert_eq!(mm.evidence_count, 0);
    assert!(!dir.join("evidence-1.json").exists());
    assert!(
        !dir.join("verify-1").exists(),
        "verification checkout must be removed"
    );
    assert!(
        git::git_ok(&fx.project, &["worktree", "list", "--porcelain"])
            .unwrap()
            .matches("worktree ")
            .count()
            == 2
    );
}

#[test]
fn leftover_background_processes_are_killed_after_normal_exit() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    let x = fx.run(
        &m.id,
        "sleep 300 & echo $! > \"$HOME/bg.pid\"; printf 'hello\\n' > out.txt",
        10,
    );
    assert!(matches!(x.status, ExecStatus::Succeeded));
    assert!(x.leftover_processes_killed, "leftover must be reported");
    assert!(x.reaped);
    let bg = read_pid(&mdir_of(&m).join("home/bg.pid")).unwrap();
    wait_until("leftover dead", 5, || !tako_engine::runner::pid_alive(bg));
}

// ---- WP2: Git execution safety ---------------------------------------------------------
fn install_hooks(project: &Path, sentinel: &Path) {
    for h in [
        "pre-commit",
        "commit-msg",
        "post-commit",
        "prepare-commit-msg",
        "pre-merge-commit",
        "post-merge",
        "post-checkout",
        "reference-transaction",
        "pre-auto-gc",
    ] {
        let p = project.join(".git/hooks").join(h);
        fs::write(
            &p,
            format!("#!/bin/sh\ntouch '{}'\nexit 0\n", sentinel.display()),
        )
        .unwrap();
        fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }
}

#[test]
fn git_hooks_never_run_during_the_full_flow() {
    let fx = Fx::new();
    install_hooks(&fx.project, &sentinel(&fx));
    let m = fx.approved(vec![pass_check()]);
    fx.run(&m.id, GOOD, 10);
    fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    sh(&fx.project, "git -c core.hooksPath=/dev/null -c user.name=t -c user.email=t@t commit -q --allow-empty -m moved"); // force a real merge
    fx.engine.accept(&m.id, &HUMAN(), "").unwrap();
    assert!(
        !sentinel(&fx).exists(),
        "a repository hook was executed by the engine"
    );
    // control: the same hooks do fire for a plain git command, so the check above is meaningful
    sh(
        &fx.project,
        "git -c user.name=t -c user.email=t@t commit -q --allow-empty -m control",
    );
    assert!(
        sentinel(&fx).exists(),
        "control failed: hooks were not installed correctly"
    );
}

#[test]
fn repository_fsmonitor_is_neutralized() {
    let fx = Fx::new();
    let s = sentinel(&fx);
    let script = fx.root.join("fsm.sh");
    fs::write(
        &script,
        format!("#!/bin/sh\ntouch '{}'\nprintf '\\0'\n", s.display()),
    )
    .unwrap();
    fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    git_cfg(
        &fx.project,
        &["--local", "core.fsmonitor", &script.to_string_lossy()],
    );
    let m = fx.approved(vec![pass_check()]);
    fx.run(&m.id, GOOD, 10);
    fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    fx.engine.accept(&m.id, &HUMAN(), "").unwrap();
    assert!(!s.exists(), "fsmonitor program was executed by the engine");
}

#[test]
fn executable_local_config_is_refused_at_open() {
    for (key, val) in [
        ("filter.evil.clean", "touch SENT"),
        ("filter.evil.smudge", "touch SENT"),
        ("merge.evil.driver", "touch SENT %O"),
        ("diff.evil.textconv", "touch SENT"),
        ("diff.external", "touch SENT"),
        ("include.path", "./extra"),
        ("includeIf.gitdir:/x/.path", "./extra"),
        ("gpg.program", "touch"),
    ] {
        let fx = Fx::new();
        let before = raw_snapshot(&fx.project);
        git_cfg(&fx.project, &["--local", key, val]);
        let after_cfg = raw_snapshot(&fx.project);
        let e = fx.engine.open_project(&fx.project).unwrap_err();
        assert!(
            matches!(e, EngineError::UnsafeRepository(ref v) if !v.is_empty()),
            "{key}: {e}"
        );
        assert_eq!(
            after_cfg,
            raw_snapshot(&fx.project),
            "refusal must not modify the repo"
        );
        assert_ne!(before, after_cfg);
        git::git_ok(&fx.project, &["config", "--local", "--unset", key]).unwrap();
        assert!(
            fx.engine.open_project(&fx.project).is_ok(),
            "{key} removal should make the project openable again"
        );
    }
}

#[test]
fn worktree_scoped_config_is_audited_too() {
    let fx = Fx::new();
    git_cfg(&fx.project, &["extensions.worktreeConfig", "true"]);
    git_cfg(
        &fx.project,
        &["--worktree", "filter.evil.clean", "touch SENT"],
    );
    assert!(matches!(
        fx.engine.open_project(&fx.project),
        Err(EngineError::UnsafeRepository(_))
    ));
}

#[test]
fn config_turned_unsafe_by_agent_stops_sealing() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    let s = sentinel(&fx);
    let script = format!("printf 'hello\\n' > out.txt; printf '* filter=evil\\n' > .gitattributes; git config --local filter.evil.clean \"touch '{}'\"", s.display());
    let x = fx.run(&m.id, &script, 10);
    assert!(!s.exists(), "filter ran during seal");
    assert!(
        x.candidate_commit.is_none(),
        "nothing may be committed when config is unsafe"
    );
    assert!(
        x.note.as_deref().unwrap_or("").contains("unsafe"),
        "{:?}",
        x.note
    );
    let mm = fx.engine.load(&m.id).unwrap();
    assert_eq!(mm.state, MissionState::ExecutionFailed);
    // the engine refuses to touch the poisoned repo again, including for cleanup
    assert!(matches!(
        fx.engine.open_project(&fx.project),
        Err(EngineError::UnsafeRepository(_))
    ));
    fx.engine.reject(&m.id, &HUMAN(), "poisoned").unwrap();
    assert!(!s.exists(), "filter ran during cleanup");
}

#[test]
fn workspace_gitdir_redirect_is_detected() {
    let fx = Fx::new();
    let other = fx.root.join("other");
    fs::create_dir_all(&other).unwrap();
    sh(&other, "git init -q -b main && git config user.name t && git config user.email t@t && printf o > f && git add -A && git commit -qm init");
    let other_head = git::head(&other).unwrap();
    let m = fx.approved(vec![pass_check()]);
    let x = fx.run(
        &m.id,
        &format!(
            "printf 'gitdir: {}/.git\\n' > .git; printf 'hello\\n' > out.txt",
            other.display()
        ),
        10,
    );
    assert!(x.candidate_commit.is_none());
    assert!(x.note.unwrap().contains("no longer belongs"));
    assert_eq!(
        fx.engine.load(&m.id).unwrap().state,
        MissionState::ExecutionFailed
    );
    assert_eq!(git::head(&other).unwrap(), other_head);
    assert!(git::git_ok(&other, &["status", "--porcelain"])
        .unwrap()
        .is_empty());
    assert!(git::git_ok(&other, &["diff", "--cached", "--name-only"])
        .unwrap()
        .is_empty());
}

#[test]
fn config_turned_unsafe_by_verification_invalidates_evidence() {
    let fx = Fx::new();
    let s = sentinel(&fx);
    let evil = format!("git config --local filter.evil.clean \"touch '{}'\"; printf '* filter=evil\\n' > .gitattributes", s.display());
    let m = fx.approved(vec![pass_check(), vec!["sh", "-c", &evil]]);
    fx.run(&m.id, GOOD, 10);
    let ev = fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    assert!(!s.exists(), "filter ran during verification bookkeeping");
    assert_eq!(ev.result, "pass");
    assert!(
        ev.integrity_violations
            .contains(&"config_unsafe".to_string()),
        "{:?}",
        ev.integrity_violations
    );
    assert!(!ev.valid && !ev.acceptable);
    assert!(fx.engine.accept(&m.id, &HUMAN(), "").is_err());
    assert!(!s.exists());
}

// ---- WP3: verification integrity ---------------------------------------------------------
#[test]
fn verification_failure_and_mutation_are_distinct() {
    let fx = Fx::new();
    let (_, fail) = verified(&fx, vec![vec!["sh", "-c", "exit 1"]]);
    assert_eq!(fail.failed_checks, vec![0]);
    assert!(fail.integrity_violations.is_empty() && fail.valid && !fail.acceptable);

    let (_, mutation) = verified(
        &fx,
        vec![pass_check(), vec!["sh", "-c", "echo x > new.txt"]],
    );
    assert!(mutation.failed_checks.is_empty());
    assert_eq!(
        mutation.integrity_violations,
        vec!["subject_modified".to_string()]
    );
    assert_eq!(mutation.result, "pass");
    assert!(!mutation.valid && !mutation.acceptable);

    let (_, both) = verified(&fx, vec![vec!["sh", "-c", "echo x > new.txt; exit 2"]]);
    assert_eq!(both.failed_checks, vec![0]);
    assert_eq!(
        both.integrity_violations,
        vec!["subject_modified".to_string()]
    );
}

#[test]
fn modified_tracked_file_and_new_untracked_file_are_both_detected() {
    let fx = Fx::new();
    let (_, e1) = verified(
        &fx,
        vec![pass_check(), vec!["sh", "-c", "echo changed >> out.txt"]],
    );
    assert!(e1.verification_modified_subject, "tracked file edit missed");
    let (_, e2) = verified(
        &fx,
        vec![
            pass_check(),
            vec!["sh", "-c", "mkdir -p sub && echo z > sub/new.txt"],
        ],
    );
    assert!(e2.verification_modified_subject, "untracked file missed");
    let (_, e3) = verified(
        &fx,
        vec![pass_check(), vec!["sh", "-c", "chmod +x out.txt"]],
    );
    assert!(e3.verification_modified_subject, "mode change missed");
}

#[test]
fn ignored_build_artifacts_are_reported_but_do_not_invalidate() {
    let fx = Fx::new();
    let m = fx.approved(vec![
        pass_check(),
        vec!["sh", "-c", "mkdir -p build && echo o > build/x.o"],
    ]);
    fx.run(
        &m.id,
        "printf 'hello\\n' > out.txt; printf 'build/\\n' > .gitignore",
        10,
    );
    let ev = fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    assert!(ev.valid && ev.acceptable, "{:?}", ev.reasons);
    assert!(
        ev.ignored_artifacts.iter().any(|p| p.starts_with("build")),
        "{:?}",
        ev.ignored_artifacts
    );
    assert!(ev.limitations.iter().any(|l| l.contains("not sandboxed")));
}

#[test]
fn evidence_is_write_once_read_only_and_tamper_evident() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    fx.run(&m.id, GOOD, 10);
    let e1 = fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    let p1 = mdir_of(&m).join("evidence-1.json");
    let bytes1 = fs::read(&p1).unwrap();
    assert!(fs::metadata(&p1).unwrap().permissions().readonly());
    // re-verification creates a new file and never rewrites the first
    let e2 = fx.engine.verify(&m.id, Duration::from_secs(10)).unwrap();
    assert_ne!(e1.id, e2.id);
    assert_eq!(bytes1, fs::read(&p1).unwrap());
    // tampering with the accepted evidence file is detected at accept time
    let p2 = mdir_of(&m).join("evidence-2.json");
    fs::set_permissions(&p2, std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
    let t = fs::read_to_string(&p2)
        .unwrap()
        .replace("\"environment\": \"", "\"environment\": \"forged ");
    fs::write(&p2, t).unwrap();
    let e = fx.engine.accept(&m.id, &HUMAN(), "").unwrap_err();
    assert!(
        matches!(e, EngineError::EvidenceNotAcceptable(ref s) if s.contains("modified")),
        "{e}"
    );
    assert_eq!(
        git::head(&fx.project).unwrap(),
        m.base_commit,
        "project must be untouched"
    );
}

// ---- WP4: durable state, locking, recovery -------------------------------------------------
#[test]
fn mission_is_busy_while_executing() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    let dir = mdir_of(&m);
    std::thread::scope(|s| {
        let h = s.spawn(|| {
            fx.engine.run_execution(
                &m.id,
                &agent("echo $$ > \"$HOME/p\"; sleep 300"),
                Duration::from_secs(120),
            )
        });
        wait_until("agent start", 20, || dir.join("home/p").exists());
        let body = PlanBody {
            summary: "x".into(),
            steps: vec![],
            files_expected: vec![],
            requested_permissions: vec![],
            risks: vec![],
        };
        assert!(matches!(
            fx.engine.submit_plan(&m.id, body),
            Err(EngineError::Busy)
        ));
        assert!(matches!(
            fx.engine.approve_plan(&m.id, 1, "x", vec![], &HUMAN()),
            Err(EngineError::Busy)
        ));
        assert!(matches!(
            fx.engine
                .run_execution(&m.id, &agent(GOOD), Duration::from_secs(5)),
            Err(EngineError::Busy)
        ));
        assert!(matches!(
            fx.engine.verify(&m.id, Duration::from_secs(5)),
            Err(EngineError::Busy)
        ));
        assert!(matches!(
            fx.engine.accept(&m.id, &HUMAN(), ""),
            Err(EngineError::Busy)
        ));
        assert!(matches!(
            fx.engine.reject(&m.id, &HUMAN(), ""),
            Err(EngineError::Busy)
        ));
        assert!(matches!(fx.engine.recover(&m.id), Err(EngineError::Busy)));
        assert_eq!(
            fx.engine.load(&m.id).unwrap().state,
            MissionState::Executing,
            "refused calls must not change state"
        );
        fx.engine.cancel(&m.id).unwrap();
        assert!(matches!(
            h.join().unwrap().unwrap().status,
            ExecStatus::Cancelled
        ));
    });
}

#[test]
fn concurrent_decisions_produce_exactly_one_outcome() {
    for _ in 0..4 {
        let fx = Fx::new();
        let (m, _) = verified(&fx, vec![pass_check()]);
        let results: Vec<(&str, Result<Decision>)> = std::thread::scope(|s| {
            let hs = vec![
                s.spawn(|| ("accept", fx.engine.accept(&m.id, &HUMAN(), "a1"))),
                s.spawn(|| ("accept", fx.engine.accept(&m.id, &HUMAN(), "a2"))),
                s.spawn(|| ("reject", fx.engine.reject(&m.id, &HUMAN(), "r1"))),
            ];
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let oks: Vec<_> = results.iter().filter(|(_, r)| r.is_ok()).collect();
        assert_eq!(
            oks.len(),
            1,
            "exactly one decision may succeed: {results:?}"
        );
        for (_, r) in &results {
            if let Err(e) = r {
                assert!(
                    matches!(e, EngineError::Busy | EngineError::AlreadyDecided),
                    "unexpected {e}"
                );
            }
        }
        let mm = fx.engine.load(&m.id).unwrap();
        let d: Decision =
            serde_json::from_slice(&fs::read(mdir_of(&m).join("decision.json")).unwrap()).unwrap();
        match oks[0].0 {
            "accept" => {
                assert_eq!(mm.state, MissionState::Accepted);
                assert_eq!(d.decision, "accept");
                assert!(fx.project.join("out.txt").exists());
            }
            _ => {
                assert_eq!(mm.state, MissionState::Rejected);
                assert_eq!(d.decision, "reject");
                assert!(!fx.project.join("out.txt").exists());
            }
        }
    }
}

#[test]
fn lock_is_released_even_when_other_threads_fork() {
    struct Stop<'a>(&'a std::sync::atomic::AtomicBool);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst); // also on panic, so a failure cannot hang the scope
        }
    }
    let fx = Fx::new();
    let m = fx.mission(vec![pass_check()]);
    let stop = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|s| {
        s.spawn(|| {
            while !stop.load(Ordering::SeqCst) {
                let _ = Command::new("true").status();
            }
        });
        let _guard = Stop(&stop);
        for i in 0..60 {
            let body = PlanBody {
                summary: format!("p{i}"),
                steps: vec![],
                files_expected: vec![],
                requested_permissions: vec![],
                risks: vec![],
            };
            fx.engine
                .submit_plan(&m.id, body)
                .unwrap_or_else(|e| panic!("iteration {i}: {e}"));
        }
    });
}

#[test]
fn killed_verification_recovers_and_preserves_earlier_evidence() {
    let fx = Fx::new();
    let m = fx.approved(vec![
        pass_check(),
        vec![
            "sh",
            "-c",
            "if [ -f ../slow ]; then echo $$ > \"$HOME/slow.pid\"; sleep 300; fi",
        ],
    ]);
    fx.run(&m.id, GOOD, 10);
    let dir = mdir_of(&m);
    let e1 = fx.engine.verify(&m.id, Duration::from_secs(20)).unwrap();
    assert!(e1.acceptable);
    let bytes1 = fs::read(dir.join("evidence-1.json")).unwrap();
    fs::write(dir.join("slow"), "1").unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "helper_engine_process",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("TAKO_TEST_DATA", fx.root.join("data"))
        .env("TAKO_TEST_MISSION", &m.id)
        .env("TAKO_TEST_OP", "verify")
        .spawn()
        .unwrap();
    wait_until("slow check", 30, || {
        read_pid(&dir.join("verify-2-home/slow.pid")).is_some()
    });
    let pid = read_pid(&dir.join("verify-2-home/slow.pid")).unwrap();
    assert_eq!(
        fx.engine.load(&m.id).unwrap().state,
        MissionState::Verifying
    );
    assert!(
        matches!(fx.engine.recover(&m.id), Err(EngineError::Busy)),
        "must not recover under a live engine"
    );
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(
        tako_engine::runner::pid_alive(pid),
        "orphaned check should still run before recovery"
    );
    assert_eq!(
        fx.engine.recover(&m.id).unwrap(),
        MissionState::EvidenceReady
    );
    wait_until("orphan killed", 5, || !tako_engine::runner::pid_alive(pid));
    assert_eq!(
        fx.engine.recover(&m.id).unwrap(),
        MissionState::EvidenceReady,
        "recovery is idempotent"
    );
    assert_eq!(
        bytes1,
        fs::read(dir.join("evidence-1.json")).unwrap(),
        "earlier evidence must be untouched"
    );
    assert!(!dir.join("evidence-2.json").exists() && !dir.join("verify-2").exists());
    assert_eq!(fx.engine.load(&m.id).unwrap().evidence_count, 1);
    // the mission is usable again
    fs::remove_file(dir.join("slow")).unwrap();
    let e2 = fx.engine.verify(&m.id, Duration::from_secs(20)).unwrap();
    assert!(e2.acceptable);
    fx.engine.accept(&m.id, &HUMAN(), "").unwrap();
}

#[test]
fn residue_of_a_crashed_start_is_cleaned() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    let residue = |fx: &Fx, m: &Mission| {
        git::git_ok(
            &fx.project,
            &[
                "worktree",
                "add",
                "-b",
                &m.branch,
                &m.workspace_path,
                &m.base_commit,
            ],
        )
        .unwrap();
        assert!(Path::new(&m.workspace_path).exists());
    };
    residue(&fx, &m);
    assert_eq!(fx.engine.recover(&m.id).unwrap(), MissionState::Approved);
    assert!(!Path::new(&m.workspace_path).exists());
    assert!(git::git_ok(&fx.project, &["branch", "--list", "tako/*"])
        .unwrap()
        .is_empty());
    residue(&fx, &m);
    let x = fx.run(&m.id, GOOD, 10); // run_execution also clears residue itself
    assert!(matches!(x.status, ExecStatus::Succeeded));
}

#[test]
fn accept_completes_idempotently_after_a_crash_following_the_merge() {
    let fx = Fx::new();
    let (m, ev) = verified(&fx, vec![pass_check()]);
    // simulate: merge happened, engine died before recording the decision
    git::git_ok(&fx.project, &["merge", "--ff-only", &ev.candidate_commit]).unwrap();
    let head = git::head(&fx.project).unwrap();
    let d = fx.engine.accept(&m.id, &HUMAN(), "retry").unwrap();
    assert!(d.note.contains("interrupted accept"));
    assert_eq!(
        git::head(&fx.project).unwrap(),
        head,
        "must not merge twice"
    );
    assert_eq!(fx.engine.load(&m.id).unwrap().state, MissionState::Accepted);
    assert!(matches!(
        fx.engine.accept(&m.id, &HUMAN(), ""),
        Err(EngineError::AlreadyDecided)
    ));
}

#[test]
fn recover_is_a_noop_outside_interrupted_phases() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    fx.run(&m.id, GOOD, 10);
    assert_eq!(fx.engine.recover(&m.id).unwrap(), MissionState::Sealed);
    assert_eq!(fx.engine.load(&m.id).unwrap().state, MissionState::Sealed);
    assert!(
        PathBuf::from(&m.workspace_path).exists(),
        "recovery must not delete sealed work"
    );
}

// ---- WP5: untrusted execution stays disabled -----------------------------------------------
#[test]
fn provider_agents_are_refused_until_a_sandbox_exists() {
    let fx = Fx::new();
    let m = fx.approved(vec![pass_check()]);
    let mut a = agent(GOOD);
    a.kind = AgentKind::Provider;
    let e = fx
        .engine
        .run_execution(&m.id, &a, Duration::from_secs(5))
        .unwrap_err();
    assert!(matches!(e, EngineError::IsolationRequired(_)), "{e}");
    assert!(!Path::new(&m.workspace_path).exists(), "nothing may start");
    assert_eq!(fx.engine.load(&m.id).unwrap().state, MissionState::Approved);
    assert!(
        matches!(fx.run(&m.id, GOOD, 10).status, ExecStatus::Succeeded),
        "mission remains usable"
    );
}
