//! Typed command layer between the desktop UI and the TAKO engine.
//!
//! Everything the UI shows is read from the engine's own state and evidence; nothing
//! here fabricates results. Long operations (execution, verification) run on worker
//! threads so a UI thread never blocks; callers poll `get_mission`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tako_engine::git;
use tako_engine::{
    Actor, AgentKind, AgentSpec, Decision, Engine, EngineError, Evidence, Execution, Mission,
    MissionState, Plan, PlanBody, Project,
};

#[cfg(all(feature = "native-qualification", not(debug_assertions)))]
compile_error!(
    "native-qualification is an offline debug-only fixture; it cannot be enabled in release builds"
);

pub mod client;
pub mod evidence_contract;
pub mod evidence_retrieval;
pub mod integration;
mod onboarding;
mod provider_connection;
pub mod providers;
#[cfg(all(feature = "provider-auth-preview", feature = "native-qualification"))]
compile_error!("Authentication preview cannot be combined with offline fixture mode");
#[cfg(all(feature = "provider-auth-preview", not(debug_assertions)))]
compile_error!("provider-auth-preview is debug-only and cannot be used for production cutover");
pub mod research;
pub mod workspace;
use client::TakoBackendClient;

pub const ISOLATION_LEVEL: &str = "L0_worktree";
pub const ISOLATION_NOTICE: &str = "Isolation level L0: the agent works in a Git worktree. A worktree is NOT a security sandbox. \
The agent runs as your user and can read and write anything you can. Only run scripts you wrote or trust.";
const MAX_DIFF_BYTES: usize = 200_000;
const MAX_LOG_BYTES: usize = 8_000;
const EARLY_RESULT_WAIT: Duration = Duration::from_millis(300);

// ---- errors ---------------------------------------------------------------------------

/// Stable, UI-facing error. `code` is machine-readable; `message` is for people.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeError {
    pub code: String,
    pub message: String,
    pub details: Vec<String>,
}

impl BridgeError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        BridgeError {
            code: code.into(),
            message: message.into(),
            details: vec![],
        }
    }
    fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_input", message)
    }
}

impl From<EngineError> for BridgeError {
    fn from(e: EngineError) -> Self {
        use EngineError::*;
        let message = e.to_string();
        let (code, details) = match &e {
            Busy => ("busy", vec![]),
            UnsafeRepository(v) => ("unsafe_repository", v.clone()),
            IsolationRequired(_) => ("isolation_required", vec![]),
            NotApproved(_) => ("not_approved", vec![]),
            Unauthorized(_) => ("unauthorized", vec![]),
            Unenforceable(_) => ("unenforceable", vec![]),
            WrongState { .. } => ("wrong_state", vec![]),
            AlreadyDecided => ("already_decided", vec![]),
            EvidenceNotAcceptable(_) => ("evidence_not_acceptable", vec![]),
            Conflict(_) => ("conflict", vec![]),
            DirtyProject => ("dirty_project", vec![]),
            Cancelled => ("cancelled", vec![]),
            InvalidInput(_) => ("invalid_input", vec![]),
            NotFound(_) => ("not_found", vec![]),
            PlanHashMismatch => ("plan_hash_mismatch", vec![]),
            NotHuman => ("not_human", vec![]),
            Io(_) | Json(_) | Git(_) => ("internal", vec![]),
        };
        BridgeError {
            code: code.into(),
            message,
            details,
        }
    }
}

// ---- views (what the UI receives) --------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct JobView {
    pub running: bool,
    pub kind: Option<String>,
    /// Error from the last background operation, if it failed after the call returned.
    pub error: Option<BridgeError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionView {
    #[serde(flatten)]
    pub execution: Execution,
    pub stdout_tail: String,
    pub stderr_tail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffView {
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct IsolationView {
    pub level: &'static str,
    pub notice: &'static str,
    pub is_security_sandbox: bool,
    /// Always "disabled": AI providers are refused until a sandbox above L0 exists.
    pub provider_execution: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissionView {
    pub mission: Mission,
    pub plan: Option<Plan>,
    pub execution: Option<ExecutionView>,
    pub evidence: Option<Evidence>,
    pub decision: Option<Decision>,
    pub diff: Option<DiffView>,
    pub job: JobView,
    /// Action ids that are valid right now. The UI must not offer anything else.
    pub next_actions: Vec<&'static str>,
    pub isolation: IsolationView,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissionSummary {
    pub id: String,
    pub goal: String,
    pub state: Option<MissionState>,
    /// Backend status is informational; no local execution/evidence is available.
    pub reported_backend_state: Option<String>,
    pub backend_project_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectView {
    pub project: Project,
    pub missions: Vec<MissionSummary>,
    pub isolation: IsolationView,
    pub source: &'static str,
    pub backend_project_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Recovered {
    pub mission_id: String,
    pub from: MissionState,
    pub to: MissionState,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StartupReport {
    pub recovered: Vec<Recovered>,
    /// Missions another engine instance is still working on.
    pub busy: Vec<String>,
    pub errors: Vec<BridgeError>,
}

fn isolation() -> IsolationView {
    IsolationView {
        level: ISOLATION_LEVEL,
        notice: ISOLATION_NOTICE,
        is_security_sandbox: false,
        provider_execution: "disabled",
    }
}

// ---- bridge -----------------------------------------------------------------------------

struct Job {
    running: bool,
    kind: String,
    error: Option<BridgeError>,
}

pub struct Bridge {
    engine: Arc<Engine>,
    data_dir: PathBuf,
    jobs: Arc<Mutex<HashMap<String, Job>>>,
    startup: StartupReport,
    user: String,
    integration: integration::OptionalTIntegration,
    proposals: providers::ProposalService,
    workspace: workspace::Workspace,
    onboarding_engineering_open: AtomicBool,
    connections: provider_connection::Connections,
}

#[derive(Deserialize)]
struct PathArg {
    path: String,
}
#[derive(Deserialize)]
struct ProjectArg {
    project_path: String,
}
#[derive(Deserialize)]
struct IdArg {
    mission_id: String,
}
#[derive(Deserialize)]
struct CreateArgs {
    project_path: String,
    goal: String,
    criteria: Vec<String>,
    /// Verification commands as shell command lines; each runs as `sh -c <line>`.
    checks: Vec<String>,
}
#[derive(Deserialize)]
struct PlanArgs {
    mission_id: String,
    script: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    expected_files: Vec<String>,
    /// Extra capabilities beyond the two the scripted agent always needs (e.g. `read_env:NAME`).
    #[serde(default)]
    extra_permissions: Vec<String>,
}
#[derive(Deserialize)]
struct ApproveArgs {
    mission_id: String,
    version: u32,
    plan_hash: String,
    granted: Vec<String>,
}
#[derive(Deserialize)]
struct StartExecArgs {
    mission_id: String,
    #[serde(default = "scripted")]
    kind: String,
    #[serde(default = "default_timeout")]
    timeout_s: u64,
}
#[derive(Deserialize)]
struct StartVerifyArgs {
    mission_id: String,
    #[serde(default = "default_timeout")]
    timeout_s: u64,
}
#[derive(Deserialize)]
struct DecideArgs {
    mission_id: String,
    #[serde(default)]
    note: String,
}
fn scripted() -> String {
    "scripted".into()
}
fn default_timeout() -> u64 {
    120
}

fn parse<T: for<'a> Deserialize<'a>>(v: Value) -> Result<T, BridgeError> {
    serde_json::from_value(v).map_err(|e| BridgeError::invalid(format!("bad arguments: {e}")))
}
fn to_value<T: Serialize>(v: T) -> Result<Value, BridgeError> {
    serde_json::to_value(v).map_err(|e| BridgeError::new("internal", e.to_string()))
}

impl Bridge {
    /// Open the engine data directory and recover missions interrupted by a crash.
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Bridge, BridgeError> {
        Self::open_core(data_dir, None)
    }

    /// Explicitly compiled offline fixture for native GUI qualification. Never uses credentials/network.
    #[cfg(all(feature = "native-qualification", debug_assertions))]
    pub fn open_for_native_qualification(
        data_dir: impl AsRef<Path>,
    ) -> Result<Bridge, BridgeError> {
        let mut bridge = Self::open(data_dir)?;
        bridge.proposals = providers::ProposalService::offline_fixture();
        Ok(bridge)
    }

    /// Supply an explicit gateway client (also permits isolated deterministic fixtures).
    pub fn open_with_client(
        data_dir: impl AsRef<Path>,
        client: TakoBackendClient,
    ) -> Result<Bridge, BridgeError> {
        Self::open_core(data_dir, Some(client))
    }

    fn open_core(
        data_dir: impl AsRef<Path>,
        client: Option<TakoBackendClient>,
    ) -> Result<Bridge, BridgeError> {
        // Provider credentials belong only to the native proposal transport, never
        // the scripted runner's grantable parent environment or evidence logs.
        let engine = Arc::new(Engine::with_env(
            data_dir.as_ref(),
            std::env::vars()
                .filter(|(name, _)| name != "OPENAI_API_KEY")
                .collect(),
        )?);
        let mut report = StartupReport::default();
        for m in engine.list_missions()? {
            if matches!(m.state, MissionState::Executing | MissionState::Verifying) {
                match engine.recover(&m.id) {
                    Ok(to) => report.recovered.push(Recovered {
                        mission_id: m.id.clone(),
                        from: m.state.clone(),
                        to,
                    }),
                    Err(EngineError::Busy) => report.busy.push(m.id.clone()),
                    Err(e) => report.errors.push(e.into()),
                }
            }
        }
        let user = std::env::var("USER").unwrap_or_else(|_| "local-user".into());
        Ok(Bridge {
            engine,
            data_dir: std::fs::canonicalize(data_dir.as_ref())
                .map_err(|e| BridgeError::new("internal", e.to_string()))?,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            startup: report,
            user,
            integration: integration::OptionalTIntegration::new(client),
            proposals: providers::ProposalService::default(),
            workspace: workspace::Workspace::default(),
            onboarding_engineering_open: AtomicBool::new(false),
            connections: provider_connection::Connections::default(),
        })
    }

    pub fn startup_report(&self) -> &StartupReport {
        &self.startup
    }

    /// Single entry point used by the Tauri command and the test harness.
    pub fn dispatch(&self, cmd: &str, args: Value) -> Result<Value, BridgeError> {
        if cmd.starts_with("research_") || research::planning::handles(cmd) {
            return research::dispatch(&self.data_dir, cmd, args);
        }
        if matches!(
            cmd,
            "context_preview"
                | "workspace_start"
                | "conversation_history"
                | "conversation_clear"
                | "proposal_start"
        ) {
            self.workspace.refresh(&self.proposals);
        }
        match cmd {
            "provider_connection_list" => {
                if !args.is_null() {
                    return Err(BridgeError::invalid(
                        "Connection discovery takes no arguments.",
                    ));
                }
                to_value(self.connections.list())
            }
            "provider_connection_begin" => to_value(
                self.connections
                    .begin(provider_connection::parse(args)?, &self.data_dir)?,
            ),
            "provider_connection_poll" => {
                to_value(self.connections.poll(provider_connection::parse(args)?)?)
            }
            "provider_connection_cancel" => {
                to_value(self.connections.cancel(provider_connection::parse(args)?)?)
            }
            "provider_connection_check" => {
                to_value(self.connections.check(provider_connection::parse(args)?)?)
            }
            "onboarding_state" => {
                if !args.is_null() {
                    return Err(BridgeError::invalid("Onboarding state takes no arguments."));
                }
                to_value(onboarding::state(&self.data_dir)?)
            }
            "onboarding_open_workspace" | "onboarding_complete" => {
                let a: onboarding::WorkspaceArgs = serde_json::from_value(args)
                    .map_err(|_| BridgeError::invalid("Invalid onboarding workspace fields."))?;
                if cmd == "onboarding_open_workspace" {
                    // Explicit native Engineering entry, without selecting/creating a Git
                    // project or invoking any integration/provider. GUI mount follows later.
                    self.engine.list_missions().map_err(|_| {
                        BridgeError::new(
                            "onboarding_workspace_open_failed",
                            "Could not open the local Engineering workspace.",
                        )
                    })?;
                    self.onboarding_engineering_open
                        .store(true, Ordering::Release);
                    to_value(json!({ "workspace": a.workspace, "opened": true }))
                } else {
                    if !self.onboarding_engineering_open.load(Ordering::Acquire) {
                        return Err(BridgeError::new(
                            "onboarding_workspace_not_open",
                            "Open the Engineering workspace before saving onboarding completion.",
                        ));
                    }
                    to_value(onboarding::complete(&self.data_dir)?)
                }
            }
            "context_preview" => {
                let a: workspace::PreviewArgs = parse(args)?;
                let project = self.engine.open_project(&a.project_path)?;
                to_value(self.workspace.preview(&project.path, a)?)
            }
            "workspace_start" => {
                let a: workspace::StartArgs = parse(args)?;
                to_value(self.workspace.start(a, &self.proposals)?)
            }
            "conversation_history" | "conversation_clear" => {
                let a: ProjectArg = parse(args)?;
                let project = self.engine.open_project(&a.project_path)?;
                if cmd == "conversation_clear" {
                    self.workspace.clear(&project.path);
                }
                to_value(self.workspace.history(&project.path))
            }
            "proposal_start" => {
                let request = serde_json::from_value(args)
                    .map_err(|_| BridgeError::invalid("Invalid proposal request fields."))?;
                to_value(self.proposals.start(request)?)
            }
            "proposal_poll" | "proposal_cancel" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct RequestId {
                    request_id: u64,
                }
                let a: RequestId = parse(args)?;
                let job = if cmd == "proposal_poll" {
                    self.proposals.poll(a.request_id)?
                } else {
                    self.proposals.cancel(a.request_id)?
                };
                self.workspace.observe(&job);
                to_value(job)
            }
            "startup_report" => to_value(&self.startup),
            "integration_status" => to_value(self.integration.view()),
            "configure_integration" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct IntegrationArgs {
                    enabled: bool,
                }
                let a: IntegrationArgs = parse(args)?;
                to_value(self.integration.configure(a.enabled))
            }
            "backend_status" => to_value(self.integration.status()?),
            "open_backend_project" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct BackendProjectArgs {
                    project_id: String,
                }
                let a: BackendProjectArgs = parse(args)?;
                to_value(self.integration.open_project(&a.project_id)?)
            }
            "backend_mission_evidence" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct EvidenceArgs {
                    project_id: String,
                    mission_id: String,
                }
                let a: EvidenceArgs = parse(args)?;
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| {
                        BridgeError::new("evidence_unavailable", "System clock is unavailable.")
                    })?
                    .as_millis() as u64;
                to_value(
                    self.integration
                        .read(|c| c.get_mission_evidence(&a.project_id, &a.mission_id, now_ms))?,
                )
            }
            "backend_projects" => to_value(self.integration.read(|c| {
                c.list_projects()
                    .map_err(|e| BridgeError::new("backend_error", e))
            })?),
            "backend_workstations" => to_value(self.integration.read(|c| {
                c.list_workstations()
                    .map_err(|e| BridgeError::new("backend_error", e))
            })?),
            "backend_workstation" => {
                #[derive(Deserialize)]
                struct WArgs {
                    workstation_id: String,
                }
                let a: WArgs = parse(args)?;
                to_value(self.integration.read(|c| {
                    c.get_workstation(&a.workstation_id)
                        .map_err(|e| BridgeError::new("backend_error", e))
                })?)
            }
            "backend_governance" => {
                let (limits, approvals) = self.integration.read(|c| {
                    let limits = c
                        .get_governance_limits()
                        .map_err(|e| BridgeError::new("backend_unavailable", e))?;
                    let approvals = c
                        .get_approvals()
                        .map_err(|e| BridgeError::new("backend_unavailable", e))?
                        .approvals;
                    Ok((limits, approvals))
                })?;
                to_value(json!({ "limits": limits, "approvals": approvals }))
            }
            "open_project" => {
                let a: PathArg = parse(args)?;
                to_value(self.open_project(&a.path)?)
            }
            "list_missions" => {
                let a: ProjectArg = parse(args)?;
                to_value(self.summaries(&a.project_path)?)
            }
            "create_mission" => to_value(self.create_mission(parse(args)?)?),
            "define_plan" => to_value(self.define_plan(parse(args)?)?),
            "approve_plan" => to_value(self.approve_plan(parse(args)?)?),
            "start_execution" => to_value(self.start_execution(parse(args)?)?),
            "start_verification" => to_value(self.start_verification(parse(args)?)?),
            "cancel" => {
                let a: IdArg = parse(args)?;
                self.engine.cancel(&a.mission_id)?;
                to_value(self.view(&a.mission_id)?)
            }
            "accept" => {
                let a: DecideArgs = parse(args)?;
                self.engine
                    .accept(&a.mission_id, &Actor::Human(self.user.clone()), &a.note)?;
                to_value(self.view(&a.mission_id)?)
            }
            "reject" => {
                let a: DecideArgs = parse(args)?;
                self.engine
                    .reject(&a.mission_id, &Actor::Human(self.user.clone()), &a.note)?;
                to_value(self.view(&a.mission_id)?)
            }
            "recover" => {
                let a: IdArg = parse(args)?;
                self.engine.recover(&a.mission_id)?;
                to_value(self.view(&a.mission_id)?)
            }
            "get_mission" => {
                let a: IdArg = parse(args)?;
                to_value(self.view(&a.mission_id)?)
            }
            other => Err(BridgeError::invalid(format!("unknown command '{other}'"))),
        }
    }

    fn summaries(&self, project_path: &str) -> Result<Vec<MissionSummary>, BridgeError> {
        Ok(self
            .engine
            .list_missions()?
            .into_iter()
            .filter(|m| m.project_path == project_path)
            .map(|m| {
                Ok(MissionSummary {
                    state: Some(self.view(&m.id)?.mission.state),
                    id: m.id,
                    goal: m.goal,
                    reported_backend_state: None,
                    backend_project_id: None,
                    created_at: m.created_at,
                })
            })
            .collect::<Result<Vec<_>, BridgeError>>()?)
    }

    fn open_project(&self, path: &str) -> Result<ProjectView, BridgeError> {
        let project = self.engine.open_project(path)?;
        let missions = self.summaries(&project.path)?;
        Ok(ProjectView {
            project,
            missions,
            isolation: isolation(),
            source: "local",
            backend_project_id: None,
        })
    }

    fn create_mission(&self, a: CreateArgs) -> Result<MissionView, BridgeError> {
        if self.integration.is_protected_path(&a.project_path) {
            return Err(BridgeError::new(
                "authorization_required",
                "Mutations and autonomous executions on protected backend projects are blocked.",
            ));
        }
        let project = self.engine.open_project(&a.project_path)?;
        if self.integration.is_protected_path(&project.path) {
            return Err(BridgeError::new(
                "authorization_required",
                "Mutations and autonomous executions on protected backend projects are blocked.",
            ));
        }
        let checks: Vec<Vec<String>> = a
            .checks
            .iter()
            .filter(|c| !c.trim().is_empty())
            .map(|c| vec!["sh".to_string(), "-c".to_string(), c.clone()])
            .collect();
        let criteria: Vec<String> = a
            .criteria
            .into_iter()
            .filter(|c| !c.trim().is_empty())
            .collect();
        let m = self
            .engine
            .create_mission(&project, &a.goal, criteria, checks)?;
        self.view(&m.id)
    }

    fn define_plan(&self, a: PlanArgs) -> Result<MissionView, BridgeError> {
        if a.script.trim().is_empty() {
            return Err(BridgeError::invalid("the scripted agent needs a script"));
        }
        let mut perms = vec![
            "fs_write_workspace".to_string(),
            "exec_command:sh".to_string(),
        ];
        for p in a.extra_permissions {
            if !perms.contains(&p) {
                perms.push(p);
            }
        }
        let first = a
            .script
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string();
        let body = PlanBody {
            summary: if a.summary.trim().is_empty() {
                format!("Run scripted agent: {first}")
            } else {
                a.summary
            },
            // The script itself is part of the plan, so approval covers exactly what will run.
            steps: vec![format!("script:{}", a.script)],
            files_expected: a.expected_files,
            requested_permissions: perms,
            risks: vec![
                "A scripted agent runs as your user. L0 isolation is not a security sandbox."
                    .into(),
            ],
        };
        self.engine.submit_plan(&a.mission_id, body)?;
        self.view(&a.mission_id)
    }

    fn approve_plan(&self, a: ApproveArgs) -> Result<MissionView, BridgeError> {
        self.engine.approve_plan(
            &a.mission_id,
            a.version,
            &a.plan_hash,
            a.granted,
            &Actor::Human(self.user.clone()),
        )?;
        self.view(&a.mission_id)
    }

    /// Run `f` on a worker thread. Returns an early error synchronously (so the UI sees refusals
    /// immediately) and otherwise returns while the operation continues; poll `get_mission`.
    fn spawn_job(
        &self,
        mission_id: &str,
        kind: &str,
        f: impl FnOnce(&Engine) -> Result<(), EngineError> + Send + 'static,
    ) -> Result<(), BridgeError> {
        {
            let mut jobs = self.jobs.lock().unwrap();
            if jobs.get(mission_id).map(|j| j.running).unwrap_or(false) {
                return Err(EngineError::Busy.into());
            }
            jobs.insert(
                mission_id.to_string(),
                Job {
                    running: true,
                    kind: kind.into(),
                    error: None,
                },
            );
        }
        let (tx, rx) = mpsc::channel();
        let (engine, jobs, id) = (
            self.engine.clone(),
            self.jobs.clone(),
            mission_id.to_string(),
        );
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&engine)))
                .unwrap_or_else(|_| Err(EngineError::Io("worker thread panicked".into())));
            let err = r.as_ref().err().map(|e| BridgeError::from(clone_err(e)));
            if let Some(j) = jobs.lock().unwrap().get_mut(&id) {
                j.running = false;
                j.error = err;
            }
            let _ = tx.send(r);
        });
        match rx.recv_timeout(EARLY_RESULT_WAIT) {
            Ok(Err(e)) => {
                if let Some(j) = self.jobs.lock().unwrap().get_mut(mission_id) {
                    j.error = None; // reported synchronously instead
                }
                Err(e.into())
            }
            _ => Ok(()),
        }
    }

    fn start_execution(&self, a: StartExecArgs) -> Result<MissionView, BridgeError> {
        if integration::legacy_protected_id(&a.mission_id) {
            return Err(BridgeError::new(
                "authorization_required",
                "Mutations and autonomous executions on protected backend projects are blocked.",
            ));
        }
        let m = self.engine.load(&a.mission_id)?;
        let kind = match a.kind.as_str() {
            "scripted" => AgentKind::Scripted,
            "provider" => AgentKind::Provider,
            other => {
                return Err(BridgeError::invalid(format!(
                    "unknown agent kind '{other}'"
                )))
            }
        };
        let (program, args, env) = if kind == AgentKind::Provider {
            return Err(BridgeError::from(EngineError::IsolationRequired(
                "AI provider execution is disabled until qualified isolation and authorization exist.".into())));
        } else {
            let ap = m.approval.clone().ok_or_else(|| {
                BridgeError::from(EngineError::NotApproved("no approval recorded".into()))
            })?;
            let plan = self.engine.load_plan(&m.id, ap.plan_version)?;
            let script = plan
                .body
                .steps
                .first()
                .and_then(|s| s.strip_prefix("script:"))
                .ok_or_else(|| BridgeError::invalid("approved plan has no scripted-agent step"))?
                .to_string();
            let env = ap
                .granted
                .iter()
                .filter_map(|g| g.strip_prefix("read_env:").map(String::from))
                .collect();
            ("sh".to_string(), vec!["-c".to_string(), script], env)
        };
        let spec = AgentSpec {
            program,
            args,
            env,
            kind,
        };
        let timeout = Duration::from_secs(a.timeout_s.max(1));
        let id = m.id.clone();
        self.spawn_job(&m.id, "execution", move |e| {
            e.run_execution(&id, &spec, timeout).map(|_| ())
        })?;
        self.view(&a.mission_id)
    }

    fn start_verification(&self, a: StartVerifyArgs) -> Result<MissionView, BridgeError> {
        let timeout = Duration::from_secs(a.timeout_s.max(1));
        let id = a.mission_id.clone();
        self.spawn_job(&a.mission_id, "verification", move |e| {
            e.verify(&id, timeout).map(|_| ())
        })?;
        self.view(&a.mission_id)
    }

    // ---- view assembly --------------------------------------------------------------------

    pub fn view(&self, mission_id: &str) -> Result<MissionView, BridgeError> {
        let m = self.engine.load(mission_id)?;
        let plan = if m.plan_count > 0 {
            Some(self.engine.load_plan(&m.id, m.plan_count)?)
        } else {
            None
        };
        let execution = if m.execution_count > 0 {
            match self.engine.load_execution(&m.id, m.execution_count) {
                Ok(x) => {
                    let (so, se) = (tail_file(&x.stdout_path), tail_file(&x.stderr_path));
                    Some(ExecutionView {
                        execution: x,
                        stdout_tail: so,
                        stderr_tail: se,
                    })
                }
                // The engine reserves the counter before running and writes the result at exit.
                // Interrupted/cancelled runs may also have no completed execution record.
                Err(EngineError::NotFound(_))
                    if !matches!(
                        m.state,
                        MissionState::Sealed | MissionState::EvidenceReady | MissionState::Accepted
                    ) =>
                {
                    None
                }
                Err(e) => return Err(e.into()),
            }
        } else {
            None
        };
        let evidence = if m.evidence_count > 0 {
            let path = self
                .data_dir
                .join("missions")
                .join(&m.id)
                .join(format!("evidence-{}.json", m.evidence_count));
            let bytes = std::fs::read(&path)
                .map_err(|e| BridgeError::new("evidence_unavailable", e.to_string()))?;
            if m.evidence_digest.as_deref()
                != Some(git::hash_bytes(&self.data_dir, &bytes)?.as_str())
            {
                return Err(BridgeError::new(
                    "evidence_unverified",
                    "Evidence digest does not match the recorded verification.",
                ));
            }
            Some(
                serde_json::from_slice::<Evidence>(&bytes)
                    .map_err(|e| BridgeError::new("evidence_unverified", e.to_string()))?,
            )
        } else {
            None
        };
        let decision = self.engine.load_decision(&m.id)?;
        if matches!(
            m.state,
            MissionState::EvidenceReady | MissionState::Accepted
        ) && evidence.is_none()
        {
            return Err(BridgeError::new(
                "evidence_unavailable",
                "Mission evidence cannot be read; outcome is unverified.",
            ));
        }
        if matches!(
            m.state,
            MissionState::Sealed | MissionState::EvidenceReady | MissionState::Accepted
        ) && !execution
            .as_ref()
            .map(|x| matches!(x.execution.status, tako_engine::ExecStatus::Succeeded))
            .unwrap_or(false)
        {
            return Err(BridgeError::new(
                "evidence_unverified",
                "No successful execution record supports this mission state.",
            ));
        }
        if let Some(ev) = &evidence {
            if ev.mission_id != m.id
                || !execution
                    .as_ref()
                    .map(|x| ev.execution_id == x.execution.id)
                    .unwrap_or(false)
            {
                return Err(BridgeError::new(
                    "evidence_unverified",
                    "Evidence does not reference this mission execution.",
                ));
            }
        }
        if let Some(x) = &execution {
            if x.execution.isolation_level != ISOLATION_LEVEL {
                return Err(BridgeError::new(
                    "evidence_unverified",
                    "Unsupported isolation claim in local execution record.",
                ));
            }
        }
        if m.state == MissionState::Accepted {
            let supported = evidence
                .as_ref()
                .zip(decision.as_ref())
                .map(|(ev, d)| {
                    ev.valid
                        && ev.acceptable
                        && ev.result == "pass"
                        && d.mission_id == m.id
                        && d.decision == "accept"
                        && d.evidence_id.as_deref() == Some(ev.id.as_str())
                        && d.result_tree_matches_verified == Some(true)
                })
                .unwrap_or(false);
            if !supported {
                return Err(BridgeError::new(
                    "evidence_unverified",
                    "Acceptance has no matching valid evidence and decision.",
                ));
            }
        }
        let diff = execution
            .as_ref()
            .and_then(|x| x.execution.candidate_commit.as_ref())
            .and_then(|c| self.diff(&m, c));
        let job = {
            let jobs = self.jobs.lock().unwrap();
            match jobs.get(&m.id) {
                Some(j) => JobView {
                    running: j.running,
                    kind: Some(j.kind.clone()),
                    error: j.error.clone(),
                },
                None => JobView {
                    running: false,
                    kind: None,
                    error: None,
                },
            }
        };
        let next_actions = next_actions(&m, evidence.as_ref(), job.running);
        Ok(MissionView {
            mission: m,
            plan,
            execution,
            evidence,
            decision,
            diff,
            job,
            next_actions,
            isolation: isolation(),
        })
    }

    fn diff(&self, m: &Mission, candidate: &str) -> Option<DiffView> {
        let out = git::git(
            Path::new(&m.project_path),
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                &m.base_commit,
                candidate,
            ],
        )
        .ok()?;
        if !out.ok {
            return None;
        }
        let truncated = out.stdout.len() > MAX_DIFF_BYTES;
        let mut text = out.stdout;
        if truncated {
            let mut cut = MAX_DIFF_BYTES;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        Some(DiffView { text, truncated })
    }
}

fn clone_err(e: &EngineError) -> EngineError {
    // EngineError is not Clone; rebuild the variants that matter for UI codes.
    use EngineError::*;
    match e {
        Busy => Busy,
        Cancelled => Cancelled,
        UnsafeRepository(v) => UnsafeRepository(v.clone()),
        IsolationRequired(m) => IsolationRequired(m.clone()),
        NotApproved(m) => NotApproved(m.clone()),
        Unauthorized(m) => Unauthorized(m.clone()),
        PlanHashMismatch => PlanHashMismatch,
        other => Io(other.to_string()),
    }
}

fn tail_file(path: &str) -> String {
    let b = std::fs::read(PathBuf::from(path)).unwrap_or_default();
    let s = if b.len() > MAX_LOG_BYTES {
        &b[b.len() - MAX_LOG_BYTES..]
    } else {
        &b[..]
    };
    String::from_utf8_lossy(s).into_owned()
}

/// The single source of truth for which actions are valid.
fn next_actions(m: &Mission, ev: Option<&Evidence>, job_running: bool) -> Vec<&'static str> {
    use MissionState::*;
    if job_running {
        return if matches!(m.state, Executing | Verifying) {
            vec!["cancel"]
        } else {
            vec![]
        };
    }
    match m.state {
        Created => vec!["define_plan"],
        PlanReady => vec!["approve_plan", "define_plan"],
        Approved => vec!["start_execution", "define_plan"],
        // No job in this process but the mission claims to be running: another instance, or a crash.
        Executing | Verifying => vec!["recover", "cancel"],
        Sealed => vec!["start_verification", "reject"],
        ExecutionFailed | TimedOut | Cancelled | Interrupted => vec!["reject"],
        EvidenceReady => {
            if ev.map(|e| e.acceptable).unwrap_or(false) {
                vec!["accept", "reject", "start_verification"]
            } else {
                vec!["reject", "start_verification"]
            }
        }
        Accepted | Rejected => vec![],
    }
}

pub fn json_ok(id: &Value, v: Value) -> Value {
    json!({"id": id, "ok": v})
}
pub fn json_err(id: &Value, e: &BridgeError) -> Value {
    json!({"id": id, "err": e})
}
