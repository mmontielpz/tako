/** Local S3 contracts only. Importing this module cannot dispatch or activate UI. */
import type { Backend } from "./backend";
import type { ResearchType } from "./researchBridge";
export interface PlanRef { kind: "question" | "evidence" | "source" | "artifact"; id: string; version: number }
export interface PlanDigest { algorithm: "sha256"; value: string }
export interface PlanStep {
  id: string; label: string; method: string;
  kind: "literature_review" | "experiment_design" | "computational_analysis" | "feasibility_analysis" | "writing";
  input_refs: PlanRef[]; dependencies: string[]; completion_criterion: string;
  included: boolean; unsupported_requirements: string[];
}
export interface ScientificPlanBody {
  objective: string; research_type: ResearchType; secondary_types: ResearchType[];
  intent: "exploratory" | "confirmatory";
  hypotheses: { text: string; prediction: string; timing: "prespecified" | "post_hoc" | "unknown" }[];
  evaluation_questions: string[]; methodology: string;
  method_requirements: { kind: "selection" | "appraisal" | "synthesis" | "design" | "measurement" | "controls" | "sampling" | "analysis" | "inputs" | "baseline" | "reproducibility" | "constraints" | "alternatives" | "evaluation"; description: string }[];
  assumptions: string[]; limitations: string[]; data_requirements: string[]; resource_requirements: string[];
  steps: PlanStep[];
  evidence_requirements: { id: string; description: string; input_refs: PlanRef[]; criterion: string }[];
  validation_criteria: string[]; constraints: string[]; risks: string[]; uncertainties: string[];
  unknowns: { field: "methodology" | "data_requirements" | "resource_requirements" | "evidence_requirements" | "validation_criteria" | "expected_deliverables"; reason: string }[];
  expected_deliverables: string[];
}
export interface PlanOrigin { kind: "human_authored" | "fixture" | "template" | "external_ai_proposal"; description: string; provider: string | null; model: string | null }
export interface PlanInput { question: PlanRef; selected_evidence: PlanRef[]; body: ScientificPlanBody; origin: PlanOrigin }
export interface PlanMaterial {
  reference: PlanRef; record_digest: PlanDigest;
  integrity: "matches" | "mismatch" | "unavailable" | "not_checked";
  availability: "available" | "unavailable" | null; content_state: "metadata_only" | "supplied_note";
  independent_verification: "not_established"; scientific_assessment: "not_assessed";
}
export interface PlanIdentity {
  schema_version: 3; record_id: string; project_id: string; plan_id: string;
  revision_id: string; revision_number: number; created_at: number | null; payload_digest: PlanDigest;
}
export interface ScientificPlanRevision extends PlanIdentity {
  clock_reason: string | null; parent_revision_id: string | null;
  payload: { input: PlanInput; question_digest: PlanDigest; materials: PlanMaterial[] };
}
export interface PlanReviewDecision extends PlanIdentity { revision_digest: PlanDigest; actor_id: string; decision: "APPROVED" | "REJECTED"; rationale: string }
export interface PlanLifecycleEvent extends PlanIdentity { clock_reason: string | null; sequence: number; operation_id: string; kind: "CREATED" | "REVISED" | "SUBMITTED_REVIEW"; revision_digest: PlanDigest }
export type PlanReviewState = "DRAFT" | "REVIEW_REQUIRED" | "APPROVED" | "REJECTED";
export interface PlanValidation { structural_only: true; reviewable: boolean; errors: string[]; warnings: string[] }
export interface PlanView {
  revision: ScientificPlanRevision; review_state: PlanReviewState; latest_revision_id: string;
  current_materials: PlanMaterial[]; project_version: number; legacy_project_version: number; s3_sequence: number;
  validation: PlanValidation; native_review_authority: "unavailable"; pending_operations: string[];
}
export interface PlanWrite { schema_version: 3; project_id: string; operation_id: string; expected_project_version: number; plan_id: string | null; parent_revision_id: string | null; input: PlanInput; revision_reason: string }
export interface PlanTarget { schema_version: 3; project_id: string; plan_id: string; revision_id: string; expected_revision_digest: PlanDigest; operation_id: string; expected_project_version: number }
export interface PlanDecisionIntent extends PlanTarget { human_review_intent: true; review_rationale: string }
export interface PlanMutation {
  status: "complete"; schema_version: 3; project_id: string; operation_id: string; plan_id: string; revision_id: string;
  revision_digest: PlanDigest; project_version: number; legacy_project_version: number; s3_sequence: number;
  review_state: "DRAFT" | "REVIEW_REQUIRED"; event_id: string; native_review_authority: "unavailable";
}
export interface PlanPageRequest { schema_version: 3; project_id: string; plan_id: string; cursor: number; limit: number }
export interface PlanPage<T> { items: T[]; next_cursor: number | null; native_review_authority: "unavailable" }
export interface ResearchPlanningCommands {
  create_plan_draft: [PlanWrite, PlanMutation];
  create_plan_revision: [PlanWrite, PlanMutation];
  read_plan: [{ schema_version: 3; project_id: string; plan_id: string; revision_id: string | null }, PlanView];
  list_plan_revisions: [PlanPageRequest, PlanPage<PlanView>];
  validate_plan_revision: [{ schema_version: 3; project_id: string; plan_id: string | null; revision_id: string | null; proposal_json: string | null }, PlanValidation];
  submit_plan_for_review: [PlanTarget, PlanMutation];
  approve_plan_revision: [PlanDecisionIntent, never];
  reject_plan_revision: [PlanDecisionIntent, never];
  read_plan_review_history: [PlanPageRequest, PlanPage<PlanLifecycleEvent> & { decisions: PlanReviewDecision[] }];
}
export function localResearchPlanning(backend: Backend) {
  return {
    createDraft: (a: PlanWrite) => backend.call("create_plan_draft", a),
    revise: (a: PlanWrite) => backend.call("create_plan_revision", a),
    read: (a: ResearchPlanningCommands["read_plan"][0]) => backend.call("read_plan", a),
    revisions: (a: PlanPageRequest) => backend.call("list_plan_revisions", a),
    validate: (a: ResearchPlanningCommands["validate_plan_revision"][0]) => backend.call("validate_plan_revision", a),
    submit: (a: PlanTarget) => backend.call("submit_plan_for_review", a),
    approve: (a: PlanDecisionIntent) => backend.call("approve_plan_revision", a),
    reject: (a: PlanDecisionIntent) => backend.call("reject_plan_revision", a),
    history: (a: PlanPageRequest) => backend.call("read_plan_review_history", a),
  };
}
