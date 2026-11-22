//! The engine: mission lifecycle with fail-closed authorization.
//!
//! Isolation level is `L0_worktree`: a Git worktree outside the project. It
//! reduces accidental damage to the project; it is NOT a security boundary.
//! See `docs/architecture/SECURITY_LIMITATIONS.md` and `ISOLATION_DECISION.md`.

use crate::error::{EngineError, Result};
use crate::git;
use crate::model::*;
use crate::runner::{self, Outcome};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const ISOLATION_LEVEL: &str = "L0_worktree";
static COUNTER: AtomicU32 = AtomicU32::new(0);

/// How much the engine may trust the program it is asked to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    /// A deterministic, human-authored script (fixtures, qualification). The caller vouches for it;
    /// the engine cannot verify that.
    Scripted,
    /// An AI provider or any other untrusted program. Refused until a sandbox level above L0
    /// is implemented and qualified (see TAKO-005).
    Provider,
}

/// Command the (fake or real) agent runs in the workspace.
#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub program: String,
    pub args: Vec<String>,
    /// Names of parent-environment variables to pass through (each needs a `read_env:` grant).
    pub env: Vec<String>,
    pub kind: AgentKind,
}

pub struct Engine {
    data_dir: PathBuf,
    parent_env: BTreeMap<String, String>,
}

/// Exclusive per-mission lock (advisory `flock`, released by the kernel if the holder dies).
///
/// Dropping explicitly unlocks instead of relying on `close`: in a multi-threaded host another
/// thread may `fork` while the lock fd is open, and the short-lived child's duplicate fd would
/// keep the lock held after our close. `LOCK_UN` releases the shared open file description.
struct MissionLock(fs::File);

impl Drop for MissionLock {
    fn drop(&mut self) {
        // SAFETY: flock on a valid, owned file descriptor.
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(v)?)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Create-new, read-only file; refuses to overwrite.
fn write_once<T: Serialize>(path: &Path, v: &T) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec_pretty(v)?;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o444))?;
    Ok(bytes)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let b =
        fs::read(path).map_err(|e| EngineError::NotFound(format!("{}: {e}", path.display())))?;
    Ok(serde_json::from_slice(&b)?)
}

fn tail(path: &Path, max: usize) -> String {
    let b = fs::read(path).unwrap_or_default();
    let s = if b.len() > max {
        &b[b.len() - max..]
    } else {
        &b[..]
    };
    String::from_utf8_lossy(s).into_owned()
}

pub fn validate_capability(c: &str) -> Result<()> {
    if c == "fs_write_workspace" {
        return Ok(());
    }
    if c == "network" || c.starts_with("network:") {
        return Err(EngineError::Unenforceable(format!(
            "'{c}': network access cannot be enforced at isolation level {ISOLATION_LEVEL}"
        )));
    }
    if let Some(p) = c.strip_prefix("exec_command:") {
        if p.is_empty() || p.chars().any(|ch| ch.is_whitespace() || ch == '\0') {
            return Err(EngineError::InvalidInput(format!("bad program in '{c}'")));
        }
        return Ok(());
    }
    if let Some(n) = c.strip_prefix("read_env:") {
        if n.is_empty() || !n.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            return Err(EngineError::InvalidInput(format!(
                "bad variable name in '{c}'"
            )));
        }
        return Ok(());
    }
    Err(EngineError::InvalidInput(format!(
        "unknown capability '{c}'"
    )))
}

struct Sealed {
    candidate: Option<String>,
    tree: Option<String>,
    files: Vec<String>,
    problem: Option<String>,
}

impl Engine {
    pub fn new(data_dir: impl AsRef<Path>) -> Result<Self> {
        Self::with_env(data_dir, std::env::vars().collect())
    }

    /// Construct with an explicit parent environment (used for `read_env:` lookups).
    pub fn with_env(
        data_dir: impl AsRef<Path>,
        parent_env: BTreeMap<String, String>,
    ) -> Result<Self> {
        fs::create_dir_all(data_dir.as_ref().join("missions"))?;
        Ok(Engine {
            data_dir: fs::canonicalize(data_dir.as_ref())?,
            parent_env,
        })
    }

    fn mdir(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(EngineError::InvalidInput(format!("bad mission id '{id}'")));
        }
        Ok(self.data_dir.join("missions").join(id))
    }

    pub fn load(&self, id: &str) -> Result<Mission> {
        read_json(&self.mdir(id)?.join("mission.json"))
    }

    fn save(&self, m: &Mission) -> Result<()> {
        write_json(&self.mdir(&m.id)?.join("mission.json"), m)
    }

    /// Take the mission lock or fail with `Busy`. Never blocks.
    fn lock(&self, id: &str) -> Result<MissionLock> {
        let dir = self.mdir(id)?;
        if !dir.is_dir() {
            return Err(EngineError::NotFound(format!("mission {id}")));
        }
        let f = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("lock"))?;
        // SAFETY: flock on a valid, owned file descriptor.
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(EngineError::Busy);
        }
        Ok(MissionLock(f))
    }

    /// Refuse to run Git against a repository whose scoped config could execute programs.
    fn guard(&self, dir: &Path) -> Result<()> {
        let f = git::audit_config(dir)?;
        if f.is_empty() {
            Ok(())
        } else {
            Err(EngineError::UnsafeRepository(f))
        }
    }

    fn event(&self, id: &str, kind: &str, detail: &str) {
        if let Ok(d) = self.mdir(id) {
            if let Ok(mut f) = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(d.join("events.jsonl"))
            {
                let line = serde_json::json!({"at": now(), "kind": kind, "detail": detail});
                let _ = writeln!(f, "{line}");
            }
        }
    }

    pub fn events(&self, id: &str) -> Result<Vec<serde_json::Value>> {
        let s = fs::read_to_string(self.mdir(id)?.join("events.jsonl")).unwrap_or_default();
        s.lines()
            .map(|l| serde_json::from_str(l).map_err(Into::into))
            .collect()
    }

    fn set_state(&self, m: &mut Mission, st: MissionState, reason: Option<String>) -> Result<()> {
        self.event(
            &m.id,
            "state",
            &format!(
                "{:?} -> {:?} {}",
                m.state,
                st,
                reason.clone().unwrap_or_default()
            ),
        );
        m.state = st;
        m.state_reason = reason;
        self.save(m)
    }

    fn require_state(m: &Mission, allowed: &[MissionState]) -> Result<()> {
        if allowed.contains(&m.state) {
            Ok(())
        } else {
            Err(EngineError::WrongState {
                expected: format!("{allowed:?}"),
                actual: format!("{:?}", m.state),
            })
        }
    }

    // ---- 1. project --------------------------------------------------------------------

    /// Read-only inspection of a Git project. Refuses projects whose repository-scoped
    /// configuration could make Git execute programs (see `git::audit_config`).
    pub fn open_project(&self, path: impl AsRef<Path>) -> Result<Project> {
        let top = git::toplevel(path.as_ref())?;
        let p = Path::new(&top);
        self.guard(p)?;
        Ok(Project {
            path: top.clone(),
            branch: git::current_branch(p)?,
            head: git::head(p)?,
            dirty: git::is_dirty(p)?,
        })
    }

    pub fn create_mission(
        &self,
        project: &Project,
        goal: &str,
        acceptance_criteria: Vec<String>,
        verification_commands: Vec<Vec<String>>,
    ) -> Result<Mission> {
        if goal.trim().is_empty() {
            return Err(EngineError::InvalidInput("goal is required".into()));
        }
        if acceptance_criteria.is_empty() || acceptance_criteria.iter().any(|c| c.trim().is_empty())
        {
            return Err(EngineError::InvalidInput(
                "acceptance criteria are required".into(),
            ));
        }
        if verification_commands.is_empty()
            || verification_commands
                .iter()
                .any(|c| c.is_empty() || c[0].is_empty())
        {
            return Err(EngineError::InvalidInput(
                "at least one verification command is required".into(),
            ));
        }
        let fresh = self.open_project(&project.path)?;
        if self.data_dir.starts_with(&fresh.path) {
            return Err(EngineError::InvalidInput(
                "engine data directory must be outside the project".into(),
            ));
        }
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let id = format!("m{:x}-{}-{}", now(), std::process::id(), n);
        let dir = self.mdir(&id)?;
        fs::create_dir_all(&dir)?;
        let m = Mission {
            schema_version: SCHEMA_VERSION,
            id: id.clone(),
            project_path: fresh.path,
            target_branch: fresh.branch,
            base_commit: fresh.head,
            goal: goal.into(),
            acceptance_criteria,
            verification_commands,
            state: MissionState::Created,
            state_reason: None,
            plan_count: 0,
            approval: None,
            active_run: None,
            execution_count: 0,
            evidence_count: 0,
            evidence_digest: None,
            branch: format!("tako/{id}"),
            workspace_path: dir.join("workspace").to_string_lossy().into_owned(),
            created_at: now(),
        };
        self.save(&m)?;
        self.event(&id, "created", &m.goal);
        Ok(m)
    }

    // ---- 2. plan & approval -------------------------------------------------------------

    fn plan_hash(&self, body: &PlanBody) -> Result<String> {
        git::hash_bytes(&self.data_dir, &serde_json::to_vec(body)?)
    }

    pub fn submit_plan(&self, mission_id: &str, body: PlanBody) -> Result<Plan> {
        let _lock = self.lock(mission_id)?;
        let mut m = self.load(mission_id)?;
        Self::require_state(
            &m,
            &[
                MissionState::Created,
                MissionState::PlanReady,
                MissionState::Approved,
            ],
        )?;
        if body.summary.trim().is_empty() {
            return Err(EngineError::InvalidInput("plan summary is required".into()));
        }
        for c in &body.requested_permissions {
            validate_capability(c)?;
        }
        let version = m.plan_count + 1;
        let plan = Plan {
            schema_version: SCHEMA_VERSION,
            mission_id: m.id.clone(),
            version,
            hash: self.plan_hash(&body)?,
            body,
            created_at: now(),
        };
        write_json(
            &self.mdir(&m.id)?.join(format!("plan-v{version}.json")),
            &plan,
        )?;
        m.plan_count = version;
        m.approval = None; // any new version voids approval
        self.set_state(&mut m, MissionState::PlanReady, None)?;
        Ok(plan)
    }

    /// All missions known to this engine, newest first. Unreadable entries are skipped.
    pub fn list_missions(&self) -> Result<Vec<Mission>> {
        let mut v: Vec<Mission> = vec![];
        for e in fs::read_dir(self.data_dir.join("missions"))?.flatten() {
            if let Ok(m) = read_json::<Mission>(&e.path().join("mission.json")) {
                v.push(m);
            }
        }
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        Ok(v)
    }

    /// The final decision, if one has been recorded.
    pub fn load_decision(&self, mission_id: &str) -> Result<Option<Decision>> {
        let p = self.mdir(mission_id)?.join("decision.json");
        if p.exists() {
            Ok(Some(read_json(&p)?))
        } else {
            Ok(None)
        }
    }

    pub fn load_plan(&self, mission_id: &str, version: u32) -> Result<Plan> {
        read_json(&self.mdir(mission_id)?.join(format!("plan-v{version}.json")))
    }

    /// Human approval of one exact plan version and a subset of its requested permissions.
    pub fn approve_plan(
        &self,
        mission_id: &str,
        version: u32,
        plan_hash: &str,
        granted: Vec<String>,
        actor: &Actor,
    ) -> Result<Approval> {
        let who = Self::human(actor)?;
        let _lock = self.lock(mission_id)?;
        let mut m = self.load(mission_id)?;
        Self::require_state(&m, &[MissionState::PlanReady])?;
        if version != m.plan_count {
            return Err(EngineError::InvalidInput(format!(
                "only the latest plan (v{}) can be approved",
                m.plan_count
            )));
        }
        let plan = self.load_plan(mission_id, version)?;
        if plan.hash != plan_hash || self.plan_hash(&plan.body)? != plan_hash {
            return Err(EngineError::PlanHashMismatch);
        }
        for g in &granted {
            validate_capability(g)?;
            if !plan.body.requested_permissions.contains(g) {
                return Err(EngineError::Unauthorized(format!(
                    "'{g}' was not requested by the plan"
                )));
            }
        }
        let a = Approval {
            plan_version: version,
            plan_hash: plan.hash,
            granted,
            approved_by: who,
            approved_at: now(),
        };
        m.approval = Some(a.clone());
        self.set_state(&mut m, MissionState::Approved, None)?;
        Ok(a)
    }

    // ---- 3. execution -------------------------------------------------------------------

    fn base_env(&self, m: &Mission, home: &Path) -> BTreeMap<String, String> {
        let mut e = BTreeMap::new();
        e.insert("PATH".into(), "/usr/bin:/bin".into());
        e.insert("HOME".into(), home.to_string_lossy().into_owned());
        e.insert("LANG".into(), "C".into());
        e.insert("TAKO_MISSION_ID".into(), m.id.clone());
        e
    }

    fn cancel_flag(&self, id: &str) -> Result<PathBuf> {
        Ok(self.mdir(id)?.join("cancel"))
    }

    /// Ask a running execution or verification to stop. Safe to call from any thread or
    /// process: it only writes a flag file the supervisor polls (it does not take the lock).
    pub fn cancel(&self, mission_id: &str) -> Result<()> {
        let m = self.load(mission_id)?;
        Self::require_state(&m, &[MissionState::Executing, MissionState::Verifying])?;
        fs::write(self.cancel_flag(mission_id)?, now().to_string())?;
        self.event(mission_id, "cancel_requested", "");
        Ok(())
    }

    /// Remove an abandoned workspace and mission branch (engine-owned paths only).
    fn discard_workspace(&self, m: &Mission) {
        let project = Path::new(&m.project_path);
        if Path::new(&m.workspace_path).starts_with(&self.data_dir) {
            let _ = git::git(
                project,
                &["worktree", "remove", "--force", &m.workspace_path],
            );
        }
        let _ = git::git(project, &["worktree", "prune"]);
        if m.branch.starts_with("tako/") {
            let _ = git::git(project, &["branch", "-D", &m.branch]);
        }
    }

    pub fn run_execution(
        &self,
        mission_id: &str,
        agent: &AgentSpec,
        timeout: Duration,
    ) -> Result<Execution> {
        let _lock = self.lock(mission_id)?;
        let mut m = self.load(mission_id)?;
        if m.state != MissionState::Approved {
            return Err(EngineError::NotApproved(format!(
                "mission is in state {:?}",
                m.state
            )));
        }
        if agent.kind == AgentKind::Provider {
            let msg = format!(
                "untrusted agents need a sandbox above {ISOLATION_LEVEL}, which is not implemented"
            );
            self.event(&m.id, "refused", &msg);
            return Err(EngineError::IsolationRequired(msg));
        }
        let approval = m
            .approval
            .clone()
            .ok_or_else(|| EngineError::NotApproved("no approval recorded".into()))?;
        let plan = self.load_plan(&m.id, approval.plan_version)?;
        if approval.plan_version != m.plan_count
            || plan.hash != approval.plan_hash
            || self.plan_hash(&plan.body)? != approval.plan_hash
        {
            self.event(&m.id, "refused", "plan changed after approval");
            return Err(EngineError::PlanHashMismatch);
        }
        let need = |cap: String| -> Result<()> {
            if approval.granted.contains(&cap) {
                Ok(())
            } else {
                Err(EngineError::Unauthorized(format!(
                    "capability '{cap}' was not granted"
                )))
            }
        };
        let checked = need("fs_write_workspace".into())
            .and_then(|_| need(format!("exec_command:{}", agent.program)))
            .and_then(|_| {
                agent
                    .env
                    .iter()
                    .try_for_each(|n| need(format!("read_env:{n}")))
            });
        if let Err(e) = checked {
            self.event(&m.id, "refused", &e.to_string());
            return Err(e);
        }

        let dir = self.mdir(&m.id)?;
        let ws = PathBuf::from(&m.workspace_path);
        let project = PathBuf::from(&m.project_path);
        if let Err(e) = self.guard(&project) {
            self.event(&m.id, "refused", &e.to_string());
            return Err(e);
        }
        // State is Approved, so any workspace or branch is residue of a crashed attempt.
        self.discard_workspace(&m);
        git::git_ok(
            &project,
            &[
                "worktree",
                "add",
                "-b",
                &m.branch,
                &m.workspace_path,
                &m.base_commit,
            ],
        )?;
        let home = dir.join("home");
        fs::create_dir_all(&home)?;
        let mut env = self.base_env(&m, &home);
        env.insert("TAKO_WORKSPACE".into(), m.workspace_path.clone());
        for n in &agent.env {
            if let Some(v) = self.parent_env.get(n) {
                env.insert(n.clone(), v.clone());
            }
        }

        m.execution_count += 1;
        let xid = format!("{}-x{}", m.id, m.execution_count);
        let xdir = dir.join(format!("exec-{}", m.execution_count));
        fs::create_dir_all(&xdir)?;
        let (so, se) = (xdir.join("stdout.txt"), xdir.join("stderr.txt"));
        let pre = (git::head(&project)?, git::porcelain(&project)?);
        let flag = self.cancel_flag(&m.id)?;
        let _ = fs::remove_file(&flag); // stale request from a previous run
        m.active_run = Some(ActiveRun {
            engine_pid: std::process::id(),
            engine_start_ticks: runner::proc_start_ticks(std::process::id()),
            pid: None,
            start_ticks: None,
        });
        self.set_state(&mut m, MissionState::Executing, None)?;

        let res = {
            let mm = &mut m;
            let mut on_spawn = |pid: u32| {
                if let Some(r) = mm.active_run.as_mut() {
                    r.pid = Some(pid);
                    r.start_ticks = runner::proc_start_ticks(pid);
                }
                let _ = self.save(mm);
            };
            runner::run_supervised(
                &agent.program,
                &agent.args,
                &ws,
                &env,
                timeout,
                &|| flag.exists(),
                &so,
                &se,
                &mut on_spawn,
            )
        };
        m.active_run = None;
        let _ = fs::remove_file(&flag);

        let post = (git::head(&project)?, git::porcelain(&project)?);
        let project_touched = pre != post;
        let sealed = self.seal(&m, &ws);
        let (status, exit_code, signal, mut note) = match &res.outcome {
            Outcome::Exited(0) => (ExecStatus::Succeeded, Some(0), None, None),
            Outcome::Exited(c) => (ExecStatus::Failed, Some(*c), None, None),
            Outcome::Signaled(s) => (ExecStatus::Failed, None, Some(*s), None),
            Outcome::TimedOut => (
                ExecStatus::Timeout,
                None,
                None,
                Some("timed out; process group killed".to_string()),
            ),
            Outcome::Cancelled => (
                ExecStatus::Cancelled,
                None,
                None,
                Some("cancelled; process group killed".to_string()),
            ),
            Outcome::SpawnFailed(e) => (
                ExecStatus::SpawnFailed,
                None,
                None,
                Some(format!("spawn failed: {e}")),
            ),
        };
        if !res.reaped {
            note = Some(format!(
                "{}agent process leader could not be reaped",
                note.map(|n| n + "; ").unwrap_or_default()
            ));
        }
        if let Some(p) = &sealed.problem {
            // Keep the primary reason (cancelled/timed out) visible; add the seal observation to it.
            note = Some(match (&status, note) {
                (ExecStatus::Timeout | ExecStatus::Cancelled, Some(n)) => format!("{n}; {p}"),
                _ => p.clone(),
            });
        }
        if project_touched {
            note = Some(format!(
                "{}project working tree or HEAD changed during execution (best-effort check)",
                note.map(|n| n + "; ").unwrap_or_default()
            ));
        }
        let exec = Execution {
            schema_version: SCHEMA_VERSION,
            id: xid,
            mission_id: m.id.clone(),
            plan_version: approval.plan_version,
            program: agent.program.clone(),
            args: agent.args.clone(),
            env_names: agent.env.clone(),
            timeout_ms: timeout.as_millis() as u64,
            status: status.clone(),
            exit_code,
            signal,
            duration_ms: res.duration_ms,
            stdout_path: so.to_string_lossy().into_owned(),
            stderr_path: se.to_string_lossy().into_owned(),
            base_commit: m.base_commit.clone(),
            candidate_commit: sealed.candidate.clone(),
            candidate_tree: sealed.tree.clone(),
            files_changed: sealed.files.clone(),
            project_touched,
            leftover_processes_killed: res.leftover_killed,
            reaped: res.reaped,
            isolation_level: ISOLATION_LEVEL.into(),
            note: note.clone(),
        };
        write_json(&xdir.join("execution.json"), &exec)?;
        let (st, reason) = match (&status, &sealed.problem) {
            (ExecStatus::Timeout, _) => (MissionState::TimedOut, note),
            (ExecStatus::Cancelled, _) => (MissionState::Cancelled, note),
            (_, Some(p)) => (MissionState::ExecutionFailed, Some(p.clone())),
            (ExecStatus::Succeeded, _) => (MissionState::Sealed, None),
            (ExecStatus::Failed, _) => (
                MissionState::ExecutionFailed,
                Some("agent exited unsuccessfully".into()),
            ),
            (ExecStatus::SpawnFailed, _) => (MissionState::ExecutionFailed, note),
        };
        self.set_state(&mut m, st, reason)?;
        Ok(exec)
    }

    pub fn load_execution(&self, mission_id: &str, n: u32) -> Result<Execution> {
        read_json(
            &self
                .mdir(mission_id)?
                .join(format!("exec-{n}"))
                .join("execution.json"),
        )
    }

    /// Commit whatever the agent left in the workspace onto the mission branch.
    /// The agent is no longer running here, but it may have tampered with Git state, so
    /// the workspace is re-validated before any Git command that could run configured helpers.
    fn seal(&self, m: &Mission, ws: &Path) -> Sealed {
        let fail = |p: &str| Sealed {
            candidate: None,
            tree: None,
            files: vec![],
            problem: Some(p.into()),
        };
        let project = Path::new(&m.project_path);
        match (git::common_dir(ws), git::common_dir(project)) {
            (Ok(a), Ok(b)) if a == b => {}
            _ => return fail("workspace git directory no longer belongs to the project"),
        }
        if let Err(e) = self.guard(ws) {
            return fail(&format!(
                "repository configuration became unsafe during execution: {e}"
            ));
        }
        match git::current_branch(ws) {
            Ok(b) if b == m.branch => {}
            _ => return fail("workspace is no longer on the mission branch"),
        }
        let r = (|| -> Result<Sealed> {
            git::git_ok(ws, &["add", "-A"])?;
            if !git::porcelain(ws)?.is_empty() {
                git::git_ok(
                    ws,
                    &[
                        "commit",
                        "--no-verify",
                        "-m",
                        &format!("TAKO seal: mission {}", m.id),
                    ],
                )?;
            }
            let head = git::head(ws)?;
            if !git::git(ws, &["merge-base", "--is-ancestor", &m.base_commit, &head])?.ok {
                return Ok(fail(
                    "workspace history no longer descends from the base commit",
                ));
            }
            let files = git::git_ok(
                ws,
                &[
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--name-only",
                    &m.base_commit,
                    &head,
                ],
            )?
            .lines()
            .map(String::from)
            .collect::<Vec<_>>();
            let tree = git::tree_of(ws, "HEAD")?;
            let problem = if head == m.base_commit {
                Some("agent made no changes".to_string())
            } else {
                None
            };
            Ok(Sealed {
                candidate: Some(head),
                tree: Some(tree),
                files,
                problem,
            })
        })();
        r.unwrap_or_else(|e| fail(&format!("seal failed: {e}")))
    }

    // ---- 4. verification ----------------------------------------------------------------

    /// Run the mission's checks in a separate clean checkout of the sealed commit.
    /// Checks are NOT sandboxed: they run as the current user (see SECURITY_LIMITATIONS.md).
    pub fn verify(&self, mission_id: &str, per_check_timeout: Duration) -> Result<Evidence> {
        let _lock = self.lock(mission_id)?;
        let mut m = self.load(mission_id)?;
        Self::require_state(&m, &[MissionState::Sealed, MissionState::EvidenceReady])?;
        let prior = m.state.clone();
        let project = PathBuf::from(&m.project_path);
        self.guard(&project)?;
        let flag = self.cancel_flag(&m.id)?;
        let _ = fs::remove_file(&flag);
        m.active_run = Some(ActiveRun {
            engine_pid: std::process::id(),
            engine_start_ticks: runner::proc_start_ticks(std::process::id()),
            pid: None,
            start_ticks: None,
        });
        self.set_state(&mut m, MissionState::Verifying, None)?;
        let n = m.evidence_count + 1;
        let r = self.verify_inner(&mut m, n, per_check_timeout, &flag);
        let _ = fs::remove_file(&flag);
        m.active_run = None;
        match r {
            Ok(ev) => {
                self.set_state(&mut m, MissionState::EvidenceReady, None)?;
                Ok(ev)
            }
            Err(e) => {
                // Restore the pre-verification state; earlier evidence is untouched.
                self.discard_verify_checkout(&m, n);
                self.set_state(&mut m, prior, Some(format!("verification aborted: {e}")))?;
                Err(e)
            }
        }
    }

    fn discard_verify_checkout(&self, m: &Mission, n: u32) {
        if let Ok(dir) = self.mdir(&m.id) {
            let v = dir.join(format!("verify-{n}"));
            if v.starts_with(&self.data_dir) && v.exists() {
                let project = Path::new(&m.project_path);
                if self.guard(project).is_ok() {
                    let _ = git::git(
                        project,
                        &["worktree", "remove", "--force", &v.to_string_lossy()],
                    );
                    let _ = git::git(project, &["worktree", "prune"]);
                }
            }
        }
    }

    fn verify_inner(
        &self,
        m: &mut Mission,
        n: u32,
        per_check_timeout: Duration,
        flag: &Path,
    ) -> Result<Evidence> {
        let x = self.load_execution(&m.id, m.execution_count)?;
        let (cand, subject_tree) = match (&x.candidate_commit, &x.candidate_tree) {
            (Some(c), Some(t)) => (c.clone(), t.clone()),
            _ => return Err(EngineError::InvalidInput("no sealed candidate".into())),
        };
        let project = PathBuf::from(&m.project_path);
        let ws = PathBuf::from(&m.workspace_path);
        let dir = self.mdir(&m.id)?;
        let mut violations: Vec<String> = vec![];
        let mut reasons: Vec<String> = vec![];

        // The candidate must be exactly what was sealed.
        let tip = git::git_ok(&ws, &["rev-parse", &format!("refs/heads/{}", m.branch)])?;
        if tip != cand || git::tree_of(&ws, &cand)? != subject_tree {
            return Err(EngineError::EvidenceNotAcceptable(
                "mission branch no longer matches the sealed candidate".into(),
            ));
        }
        if !git::git(
            &project,
            &["merge-base", "--is-ancestor", &m.base_commit, &cand],
        )?
        .ok
        {
            return Err(EngineError::EvidenceNotAcceptable(
                "candidate does not descend from the base commit".into(),
            ));
        }
        let vpath = dir.join(format!("verify-{n}"));
        let logs = dir.join(format!("verify-{n}-logs"));
        let home = dir.join(format!("verify-{n}-home"));
        fs::create_dir_all(&logs)?;
        fs::create_dir_all(&home)?;
        git::git_ok(
            &project,
            &[
                "worktree",
                "add",
                "--detach",
                &vpath.to_string_lossy(),
                &cand,
            ],
        )?;
        let idx = dir.join(format!("scratch-index-{n}"));
        let before = git::working_tree_hash(&vpath, &idx)?;
        if before != subject_tree {
            violations.push("checkout_mismatch".into());
            reasons.push("clean checkout does not match the sealed tree".into());
        }
        let env = self.base_env(m, &home);
        let mut checks = vec![];
        let cmds = m.verification_commands.clone();
        for (i, cmd) in cmds.iter().enumerate() {
            let (so, se) = (
                logs.join(format!("check-{i}.stdout")),
                logs.join(format!("check-{i}.stderr")),
            );
            let mm = &mut *m;
            let mut on_spawn = |pid: u32| {
                if let Some(r) = mm.active_run.as_mut() {
                    r.pid = Some(pid);
                    r.start_ticks = runner::proc_start_ticks(pid);
                }
                let _ = self.save(mm);
            };
            let r = runner::run_supervised(
                &cmd[0],
                &cmd[1..],
                &vpath,
                &env,
                per_check_timeout,
                &|| flag.exists(),
                &so,
                &se,
                &mut on_spawn,
            );
            if r.outcome == Outcome::Cancelled {
                return Err(EngineError::Cancelled);
            }
            let (code, timed_out) = match r.outcome {
                Outcome::Exited(c) => (Some(c), false),
                Outcome::TimedOut => (None, true),
                _ => (None, false),
            };
            checks.push(CheckResult {
                command: cmd.clone(),
                exit_code: code,
                timed_out,
                duration_ms: r.duration_ms,
                stdout_path: so.to_string_lossy().into_owned(),
                stderr_path: se.to_string_lossy().into_owned(),
                stdout_tail: tail(&so, 2000),
                stderr_tail: tail(&se, 2000),
            });
        }

        // Checks run unsandboxed and may have touched Git configuration. If so, run no further Git.
        let mut after = String::from("unavailable");
        let mut modified = false;
        let mut ignored: Vec<String> = vec![];
        if let Err(e) = self.guard(&project) {
            violations.push("config_unsafe".into());
            reasons.push(format!(
                "repository configuration became unsafe during verification: {e}"
            ));
        } else {
            after = git::working_tree_hash(&vpath, &idx)?;
            modified = after != before;
            if modified {
                violations.push("subject_modified".into());
                reasons.push("verification modified the checked-out subject".into());
            }
            let ws_tree_now = git::working_tree_hash(&ws, &idx)?;
            let tip_now = git::git_ok(&ws, &["rev-parse", &format!("refs/heads/{}", m.branch)])?;
            if ws_tree_now != subject_tree || tip_now != cand {
                violations.push("candidate_changed".into());
                reasons.push("sealed workspace or branch changed during verification".into());
            }
            if let Ok(st) = git::git_ok(
                &vpath,
                &[
                    "status",
                    "--porcelain",
                    "--ignored",
                    "--untracked-files=all",
                ],
            ) {
                ignored = st
                    .lines()
                    .filter_map(|l| l.strip_prefix("!! "))
                    .take(50)
                    .map(String::from)
                    .collect();
            }
            let _ = git::git(
                &project,
                &["worktree", "remove", "--force", &vpath.to_string_lossy()],
            );
        }
        let failed_checks: Vec<usize> = checks
            .iter()
            .enumerate()
            .filter(|(_, c)| c.exit_code != Some(0))
            .map(|(i, _)| i)
            .collect();
        let pass = !checks.is_empty() && failed_checks.is_empty();
        let valid = violations.is_empty();
        if !pass {
            reasons.push("one or more checks failed".into());
        }
        let ev = Evidence {
            schema_version: SCHEMA_VERSION,
            id: format!("{}-e{}", m.id, n),
            mission_id: m.id.clone(),
            execution_id: x.id.clone(),
            candidate_commit: cand,
            subject_tree_hash: subject_tree,
            tree_hash_before_verify: before,
            tree_hash_after_verify: after,
            verification_modified_subject: modified,
            integrity_violations: violations,
            failed_checks,
            ignored_artifacts: ignored,
            checks,
            files_changed: x.files_changed.clone(),
            result: if pass { "pass" } else { "fail" }.into(),
            valid,
            acceptable: valid && pass,
            reasons,
            limitations: vec![
                format!("isolation level {ISOLATION_LEVEL} is not a security boundary"),
                "verification checks are not sandboxed; they run as the current user".into(),
                "ignored files (e.g. build output) are not part of the tree-hash comparison".into(),
                "evidence is tamper-evident, not tamper-proof, against code running as the same user".into(),
            ],
            environment: format!("{}; {}", std::env::consts::OS, git::git_version()),
            created_at: now(),
        };
        // Write-once and read-only; its content id is recorded in the mission and re-checked on accept.
        let bytes = write_once(&dir.join(format!("evidence-{n}.json")), &ev)?;
        m.evidence_digest = Some(git::hash_bytes(&self.data_dir, &bytes)?);
        m.evidence_count = n;
        Ok(ev)
    }

    pub fn load_evidence(&self, mission_id: &str, n: u32) -> Result<Evidence> {
        read_json(&self.mdir(mission_id)?.join(format!("evidence-{n}.json")))
    }

    // ---- 5. decisions -------------------------------------------------------------------

    fn human(actor: &Actor) -> Result<String> {
        match actor {
            Actor::Human(n) => Ok(n.clone()),
            Actor::Agent(_) => Err(EngineError::NotHuman),
        }
    }

    /// Merge the verified candidate into the project. Never forces; aborts and
    /// leaves the project unchanged on conflict. Completes idempotently if a previous
    /// accept merged but crashed before recording the decision.
    pub fn accept(&self, mission_id: &str, actor: &Actor, note: &str) -> Result<Decision> {
        let who = Self::human(actor)?;
        let _lock = self.lock(mission_id)?;
        let mut m = self.load(mission_id)?;
        if m.state.is_terminal() {
            return Err(EngineError::AlreadyDecided);
        }
        Self::require_state(&m, &[MissionState::EvidenceReady])?;
        if m.evidence_count == 0 {
            return Err(EngineError::EvidenceNotAcceptable("no evidence".into()));
        }
        let evpath = self
            .mdir(&m.id)?
            .join(format!("evidence-{}.json", m.evidence_count));
        let bytes =
            fs::read(&evpath).map_err(|e| EngineError::NotFound(format!("evidence: {e}")))?;
        if m.evidence_digest.as_deref() != Some(git::hash_bytes(&self.data_dir, &bytes)?.as_str()) {
            return Err(EngineError::EvidenceNotAcceptable(
                "evidence file was modified after verification".into(),
            ));
        }
        let ev: Evidence = serde_json::from_slice(&bytes)?;
        if !ev.acceptable {
            return Err(EngineError::EvidenceNotAcceptable(ev.reasons.join("; ")));
        }
        let project = PathBuf::from(&m.project_path);
        let ws = PathBuf::from(&m.workspace_path);
        self.guard(&project)?;
        let tip = git::git_ok(&ws, &["rev-parse", &format!("refs/heads/{}", m.branch)])?;
        if tip != ev.candidate_commit || git::tree_of(&ws, &tip)? != ev.subject_tree_hash {
            return Err(EngineError::EvidenceNotAcceptable(
                "candidate no longer matches the verified evidence".into(),
            ));
        }
        if git::current_branch(&project)? != m.target_branch {
            return Err(EngineError::InvalidInput(format!(
                "project is not on target branch '{}'",
                m.target_branch
            )));
        }
        let target_ref = format!("refs/heads/{}", m.target_branch);
        let already_merged = git::git(
            &project,
            &[
                "merge-base",
                "--is-ancestor",
                &ev.candidate_commit,
                &target_ref,
            ],
        )?
        .ok;
        let mut final_note = note.to_string();
        if already_merged {
            final_note
                .push_str(" [completed after an interrupted accept: candidate was already merged]");
        } else {
            if git::is_dirty(&project)? {
                return Err(EngineError::DirtyProject);
            }
            let pre = git::head(&project)?;
            let merged = if pre == m.base_commit {
                git::git(&project, &["merge", "--ff-only", &ev.candidate_commit])?
            } else {
                git::git(
                    &project,
                    &[
                        "merge",
                        "--no-ff",
                        "--no-edit",
                        "-m",
                        &format!("TAKO: accept mission {}", m.id),
                        &ev.candidate_commit,
                    ],
                )?
            };
            if !merged.ok {
                let detail = format!("{} {}", merged.stdout.trim(), merged.stderr.trim());
                if git::git(&project, &["rev-parse", "-q", "--verify", "MERGE_HEAD"])?.ok {
                    git::git_ok(&project, &["merge", "--abort"])?;
                }
                if git::head(&project)? != pre || git::is_dirty(&project)? {
                    return Err(EngineError::Git(format!(
                        "could not restore project after failed merge: {detail}"
                    )));
                }
                self.event(&m.id, "accept_refused", &detail);
                return Err(EngineError::Conflict(detail));
            }
        }
        let head_now = git::head(&project)?;
        let d = Decision {
            schema_version: SCHEMA_VERSION,
            mission_id: m.id.clone(),
            decision: "accept".into(),
            evidence_id: Some(ev.id.clone()),
            decided_by: who,
            decided_at: now(),
            note: final_note,
            result_tree_matches_verified: Some(
                git::tree_of(&project, "HEAD")? == ev.subject_tree_hash,
            ),
            resulting_head: Some(head_now),
        };
        write_json(&self.mdir(&m.id)?.join("decision.json"), &d)?;
        self.set_state(&mut m, MissionState::Accepted, None)?;
        self.discard_workspace(&m);
        Ok(d)
    }

    /// Discard the isolated workspace and branch; evidence and decision are kept.
    pub fn reject(&self, mission_id: &str, actor: &Actor, note: &str) -> Result<Decision> {
        let who = Self::human(actor)?;
        let _lock = self.lock(mission_id)?;
        let mut m = self.load(mission_id)?;
        if m.state.is_terminal() {
            return Err(EngineError::AlreadyDecided);
        }
        Self::require_state(
            &m,
            &[
                MissionState::Sealed,
                MissionState::ExecutionFailed,
                MissionState::TimedOut,
                MissionState::Cancelled,
                MissionState::Interrupted,
                MissionState::EvidenceReady,
            ],
        )?;
        let ev = if m.evidence_count > 0 {
            Some(self.load_evidence(&m.id, m.evidence_count)?.id)
        } else {
            None
        };
        let d = Decision {
            schema_version: SCHEMA_VERSION,
            mission_id: m.id.clone(),
            decision: "reject".into(),
            evidence_id: ev,
            decided_by: who,
            decided_at: now(),
            note: note.into(),
            resulting_head: None,
            result_tree_matches_verified: None,
        };
        write_json(&self.mdir(&m.id)?.join("decision.json"), &d)?;
        self.discard_workspace(&m);
        self.set_state(&mut m, MissionState::Rejected, None)?;
        Ok(d)
    }

    // ---- 6. recovery --------------------------------------------------------------------

    /// Deterministic recovery after an engine crash. Taking the mission lock proves no live
    /// engine is working on the mission (the kernel releases it when its holder dies).
    ///
    /// - `executing`  -> kill the recorded process group (only if its start time matches) -> `interrupted`
    /// - `verifying`  -> kill, drop the verification checkout -> back to `evidence_ready`/`sealed`
    ///   (earlier evidence is never touched)
    /// - `approved`   -> remove residue of a run that crashed before starting
    /// - any other state is returned unchanged.
    pub fn recover(&self, mission_id: &str) -> Result<MissionState> {
        let _lock = self.lock(mission_id)?;
        let mut m = self.load(mission_id)?;
        let kill_recorded = |m: &Mission| {
            if let Some(r) = &m.active_run {
                if let (Some(pid), Some(t)) = (r.pid, r.start_ticks) {
                    if runner::proc_start_ticks(pid) == Some(t) {
                        runner::killpg(pid as i32, libc::SIGKILL);
                    }
                }
            }
        };
        match m.state {
            MissionState::Executing => {
                kill_recorded(&m);
                m.active_run = None;
                self.set_state(
                    &mut m,
                    MissionState::Interrupted,
                    Some("engine stopped while executing".into()),
                )?;
            }
            MissionState::Verifying => {
                kill_recorded(&m);
                m.active_run = None;
                self.discard_verify_checkout(&m, m.evidence_count + 1);
                let back = if m.evidence_count > 0 {
                    MissionState::EvidenceReady
                } else {
                    MissionState::Sealed
                };
                self.set_state(
                    &mut m,
                    back,
                    Some("engine stopped while verifying; no evidence was recorded".into()),
                )?;
            }
            MissionState::Approved => {
                self.discard_workspace(&m);
            }
            _ => {}
        }
        Ok(m.state)
    }
}
