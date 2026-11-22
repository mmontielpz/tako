//! Persisted data types. Every file written by the engine carries `schema_version`.

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionState {
    Created,
    PlanReady,
    Approved,
    Executing,
    Cancelled,
    Sealed,
    ExecutionFailed,
    TimedOut,
    Interrupted,
    Verifying,
    EvidenceReady,
    Accepted,
    Rejected,
}

impl MissionState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, MissionState::Accepted | MissionState::Rejected)
    }
}

/// Who is acting. Approvals and decisions require `Human`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actor {
    Human(String),
    Agent(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub path: String,
    pub branch: String,
    pub head: String,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approval {
    pub plan_version: u32,
    pub plan_hash: String,
    /// Capabilities granted with the approval (subset of the plan's requests).
    pub granted: Vec<String>,
    pub approved_by: String,
    pub approved_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveRun {
    pub engine_pid: u32,
    pub engine_start_ticks: Option<u64>,
    pub pid: Option<u32>,
    pub start_ticks: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mission {
    pub schema_version: u32,
    pub id: String,
    pub project_path: String,
    pub target_branch: String,
    pub base_commit: String,
    pub goal: String,
    pub acceptance_criteria: Vec<String>,
    /// Each check is an argv vector (no shell interpretation by the engine).
    pub verification_commands: Vec<Vec<String>>,
    pub state: MissionState,
    pub state_reason: Option<String>,
    pub plan_count: u32,
    pub approval: Option<Approval>,
    pub active_run: Option<ActiveRun>,
    pub execution_count: u32,
    pub evidence_count: u32,
    /// Content id of the latest evidence file; checked at accept time.
    #[serde(default)]
    pub evidence_digest: Option<String>,
    pub branch: String,
    pub workspace_path: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanBody {
    pub summary: String,
    pub steps: Vec<String>,
    pub files_expected: Vec<String>,
    /// Capability strings: `fs_write_workspace`, `exec_command:<program>`, `read_env:<NAME>`.
    pub requested_permissions: Vec<String>,
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub schema_version: u32,
    pub mission_id: String,
    pub version: u32,
    pub hash: String,
    pub body: PlanBody,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecStatus {
    Succeeded,
    Failed,
    Timeout,
    Cancelled,
    SpawnFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub schema_version: u32,
    pub id: String,
    pub mission_id: String,
    pub plan_version: u32,
    pub program: String,
    pub args: Vec<String>,
    pub env_names: Vec<String>,
    pub timeout_ms: u64,
    pub status: ExecStatus,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub duration_ms: u128,
    pub stdout_path: String,
    pub stderr_path: String,
    pub base_commit: String,
    pub candidate_commit: Option<String>,
    pub candidate_tree: Option<String>,
    pub files_changed: Vec<String>,
    /// Best-effort: the project's HEAD or working tree changed during execution.
    pub project_touched: bool,
    pub leftover_processes_killed: bool,
    /// The agent's process-group leader was reaped.
    #[serde(default)]
    pub reaped: bool,
    pub isolation_level: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub command: Vec<String>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u128,
    pub stdout_path: String,
    pub stderr_path: String,
    pub stdout_tail: String,
    pub stderr_tail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub schema_version: u32,
    pub id: String,
    pub mission_id: String,
    pub execution_id: String,
    pub candidate_commit: String,
    /// Tree hash of the sealed candidate.
    pub subject_tree_hash: String,
    pub tree_hash_before_verify: String,
    pub tree_hash_after_verify: String,
    pub verification_modified_subject: bool,
    /// Machine-readable integrity failures: `subject_modified`, `candidate_changed`,
    /// `checkout_mismatch`, `config_unsafe`. Empty means the evidence is intact.
    #[serde(default)]
    pub integrity_violations: Vec<String>,
    /// Indices of checks that failed (distinct from integrity violations).
    #[serde(default)]
    pub failed_checks: Vec<usize>,
    /// Ignored files that appeared in the verification checkout (informational, max 50).
    #[serde(default)]
    pub ignored_artifacts: Vec<String>,
    pub checks: Vec<CheckResult>,
    pub files_changed: Vec<String>,
    /// Derived only from `checks`: "pass" iff every check exited 0.
    pub result: String,
    /// Evidence integrity: the subject was not modified by verification.
    pub valid: bool,
    /// `valid && result == "pass"`; required to accept.
    pub acceptable: bool,
    pub reasons: Vec<String>,
    pub limitations: Vec<String>,
    pub environment: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub schema_version: u32,
    pub mission_id: String,
    pub decision: String,
    pub evidence_id: Option<String>,
    pub decided_by: String,
    pub decided_at: i64,
    pub note: String,
    /// For accept: project HEAD after the merge and whether its tree equals the verified tree.
    pub resulting_head: Option<String>,
    pub result_tree_matches_verified: Option<bool>,
}
