// Hand-written mirror of the Rust bridge DTOs (bridge/src/lib.rs). The contract tests in
// tests/backend.contract.test.ts run these shapes against the real engine.
import type { ResearchCommands } from "./researchBridge";
import type { ResearchEvidenceCommands } from "./researchEvidenceBridge";
import type { ResearchPlanningCommands } from "./researchPlanningBridge";
import type { SotaCommands } from "./sotaBridge";

export type MissionState =
  | "created" | "plan_ready" | "approved" | "executing" | "cancelled" | "sealed"
  | "execution_failed" | "timed_out" | "interrupted" | "verifying" | "evidence_ready"
  | "accepted" | "rejected";

export type ActionId =
  | "define_plan" | "approve_plan" | "start_execution" | "start_verification"
  | "cancel" | "recover" | "accept" | "reject";

export interface BridgeErrorShape { code: string; message: string; details: string[] }

export interface Project { path: string; branch: string; head: string; dirty: boolean }
export interface IsolationView {
  level: string; notice: string; is_security_sandbox: boolean; provider_execution: "disabled";
}
export interface MissionSummary {
  id: string; goal: string; state: MissionState | null; reported_backend_state?: string | null; backend_project_id?: string | null; created_at: number;
  aief_spec_sha256?: string | null; aief_lifecycle_state?: string | null;
}
export interface ProjectView { project: Project; missions: MissionSummary[]; isolation: IsolationView; source: "local" | "t_backend"; backend_project_id: string | null }

export interface Approval {
  plan_version: number; plan_hash: string; granted: string[]; approved_by: string; approved_at: number;
}
export interface Mission {
  id: string; project_path: string; target_branch: string; base_commit: string; goal: string;
  acceptance_criteria: string[]; verification_commands: string[][]; state: MissionState;
  state_reason: string | null; plan_count: number; approval: Approval | null;
  evidence_count: number; workspace_path: string; branch: string;
  aief_spec_sha256?: string | null; aief_lifecycle_state?: string | null;
}
export interface PlanBody {
  summary: string; steps: string[]; files_expected: string[]; requested_permissions: string[]; risks: string[];
}
export interface Plan { mission_id: string; version: number; hash: string; body: PlanBody }
export interface ExecutionView {
  id: string; status: "succeeded" | "failed" | "timeout" | "cancelled" | "spawn_failed";
  exit_code: number | null; signal: number | null; duration_ms: number; files_changed: string[];
  project_touched: boolean; reaped: boolean; isolation_level: string; note: string | null;
  stdout_tail: string; stderr_tail: string; candidate_commit: string | null;
}
export interface CheckResult {
  command: string[]; exit_code: number | null; timed_out: boolean; duration_ms: number;
  stdout_tail: string; stderr_tail: string;
}
export interface Evidence {
  id: string; candidate_commit: string; subject_tree_hash: string;
  tree_hash_before_verify: string; tree_hash_after_verify: string;
  verification_modified_subject: boolean; integrity_violations: string[]; failed_checks: number[];
  ignored_artifacts: string[]; checks: CheckResult[]; files_changed: string[];
  result: "pass" | "fail"; valid: boolean; acceptable: boolean; reasons: string[]; limitations: string[];
}
export interface Decision {
  decision: "accept" | "reject"; evidence_id: string | null; decided_by: string; note: string;
  resulting_head: string | null; result_tree_matches_verified: boolean | null;
}
export interface MissionView {
  mission: Mission; plan: Plan | null; execution: ExecutionView | null; evidence: Evidence | null;
  decision: Decision | null; diff: { text: string; truncated: boolean } | null;
  job: { running: boolean; kind: string | null; error: BridgeErrorShape | null };
  next_actions: ActionId[]; isolation: IsolationView;
}
export interface StartupReport {
  recovered: { mission_id: string; from: MissionState; to: MissionState }[];
  busy: string[]; errors: BridgeErrorShape[];
}

export interface IntegrationView {
  mode: "standalone" | "optional_backend";
  backend: "t" | null;
  availability: "disabled" | "not_checked" | "available" | "unavailable";
  read_only: true;
  provider_execution: "disabled";
}

export interface BackendStatus {
  online: boolean;
  base_url: string;
  status: string;
  runtime_status: string;
  g12_state: string;
  operational_mode: string;
  projects?: RegisteredProject[];
  limits?: GovernanceLimits | null;
  approvals?: BackendApproval[];
  workstations?: Workstation[];
  details?: unknown;
}

export interface Workstation {
  workstation_id: string;
  display_name: string;
  host: string;
  user: string;
  os: string;
  connectivity_status: "connected" | "disconnected" | "unknown";
  last_seen: string | null;
  transport: string;
  capabilities: string[];
  is_local: boolean;
}

export interface WorkstationsResponse {
  workstations: Workstation[];
  total: number;
  timestamp: string;
}

export interface RegisteredProject {
  project_id: string;
  name: string;
  path: string;
  exists: boolean;
  is_git: boolean;
  branch: string;
  head: string;
  dirty: boolean;
  allowed_root: string;
  isolation_profile: string;
}

export interface BackendProjectsResponse {
  projects: RegisteredProject[];
  total: number;
  allowed_roots: string[];
}

export interface GovernanceLimits {
  spending_limit_usd: number;
  spending_currency: string;
  budget_gate: string;
  max_runtime_hours: number;
  max_concurrent_workers: number;
  prohibited_operations: string[];
  g12_state: string;
  aws_state: string;
}

export interface BackendApproval {
  approval_id: string;
  kind: string;
  project: string;
  operation: string;
  status: string;
  reason: string;
}

export interface BackendGovernanceResponse {
  limits: GovernanceLimits;
  approvals: BackendApproval[];
}

export interface BackendEvidenceView {
  identity: { backend_id: string; project_id: string; mission_id: string };
  assessment: {
    outcome: "unavailable" | "not_found" | "incomplete" | "unverified";
    isolation: "unverified";
    remote_acceptance_enabled: false;
    l1_badge_enabled: false;
    provider_execution_enabled: false;
    reason: string;
  };
  message: string;
}

/** Local navigation metadata; never a scientific approval or provider-consent record. */
export type OnboardingWorkspace = "engineering";
export interface OnboardingCompleted { status: "completed"; last_workspace: OnboardingWorkspace }
export type OnboardingState =
  | { status: "first_run" }
  | OnboardingCompleted
  | { status: "unavailable"; reason: "invalid_data" | "unsupported_version" | "unsafe_file" };
/** Native workspace entry only; this does not assert that a GUI screen rendered. */
export interface OnboardingWorkspaceOpened { workspace: OnboardingWorkspace; opened: true }

/** Command name -> [arguments, result]. */
export interface Commands extends ResearchCommands, ResearchEvidenceCommands, ResearchPlanningCommands, SotaCommands {
  onboarding_state: [undefined, OnboardingState];
  onboarding_open_workspace: [{ workspace: OnboardingWorkspace }, OnboardingWorkspaceOpened];
  onboarding_complete: [{ workspace: OnboardingWorkspace }, OnboardingCompleted];
  context_preview: [{ project_path: string; paths: string[]; prompt: string; include_history: boolean }, ContextPreview];
  workspace_start: [{ project_path: string; preview_id: number; consent: boolean; provider: ProviderFamily; model: string; timeout_s: number; max_output_tokens: number }, ProposalJob];
  conversation_history: [{ project_path: string }, ConversationTurn[]];
  conversation_clear: [{ project_path: string }, ConversationTurn[]];
  proposal_start: [ProposalRequest, ProposalJob];
  proposal_poll: [{ request_id: number }, ProposalJob];
  proposal_cancel: [{ request_id: number }, ProposalJob];
  startup_report: [undefined, StartupReport];
  integration_status: [undefined, IntegrationView];
  configure_integration: [{ enabled: boolean }, IntegrationView];
  open_backend_project: [{ project_id: string }, ProjectView];
  backend_status: [undefined, BackendStatus];
  backend_mission_evidence: [{ project_id: string; mission_id: string }, BackendEvidenceView];
  backend_projects: [undefined, BackendProjectsResponse];
  backend_governance: [undefined, BackendGovernanceResponse];
  backend_workstations: [undefined, WorkstationsResponse];
  backend_workstation: [{ workstation_id: string }, Workstation];
  open_project: [{ path: string }, ProjectView];
  list_missions: [{ project_path: string }, MissionSummary[]];
  create_mission: [{ project_path: string; goal: string; criteria: string[]; checks: string[] }, MissionView];
  define_plan: [
    { mission_id: string; script: string; summary?: string; expected_files?: string[]; extra_permissions?: string[] },
    MissionView,
  ];
  approve_plan: [{ mission_id: string; version: number; plan_hash: string; granted: string[] }, MissionView];
  start_execution: [{ mission_id: string; kind?: "scripted" | "provider"; timeout_s?: number }, MissionView];
  start_verification: [{ mission_id: string; timeout_s?: number }, MissionView];
  cancel: [{ mission_id: string }, MissionView];
  accept: [{ mission_id: string; note?: string }, MissionView];
  reject: [{ mission_id: string; note?: string }, MissionView];
  recover: [{ mission_id: string }, MissionView];
  get_mission: [{ mission_id: string }, MissionView];
}
export type CommandName = keyof Commands;

export type ProviderFamily = "open_ai" | "anthropic" | "gemini" | "local_model";
export interface ProposalRequest {
  provider: ProviderFamily; model: string; prompt: string; timeout_s: number; max_output_tokens: number;
}
export interface ProposalJob {
  request_id: number;
  state: "pending" | "cancelling" | "succeeded" | "failed" | "cancelled";
  response: { provider: ProviderFamily; model: string; text: string; untrusted: true;
    usage: { input_tokens: number; output_tokens: number; total_tokens: number } | null } | null;
  error: BridgeErrorShape | null;
}

export interface ContextPreview { preview_id: number; project_path: string; text: string; bytes: number }
export interface ConversationTurn { prompt: string; response: string }
