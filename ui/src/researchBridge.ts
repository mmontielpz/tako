/** Headless S1 bindings. Importing this module performs no request or UI activation. */
import type { Backend } from "./backend";
import type { EvidenceGet, EvidenceView, EvidenceEvents, EvidenceEvent, EvidencePage, EventCursor, EvidenceOperation, EvidenceOperationStatus } from "./researchEvidenceBridge";

export type ResearchType = "literature_review" | "experimental_research" | "computational_research" | "technical_feasibility";
export type ResearchRecordKind = "project" | "question" | "plan" | "plan_revision" | "activity";
export interface ResearchRef { kind: ResearchRecordKind; id: string; version: number }
export interface ResearchMeta {
  schema_version: 1; id: string; project_id: string; owner_id: string; version: number;
  created_at_ms: number | null; updated_at_ms: number | null;
  clock_status: "known" | "uncertain"; clock_reason: string | null;
}
export interface ResearchProjectMetadata {
  title: string; primary_type: ResearchType | null; type_unknown_reason: string | null;
  secondary_types: ResearchType[];
}
export interface ResearchProject {
  meta: ResearchMeta; metadata: ResearchProjectMetadata; state: "open";
  active_question: ResearchRef; active_plan: ResearchRef | null; iteration_id: string;
}
export interface ResearchQuestionBody {
  question: string; objectives: string[];
  hypotheses: { text: string; timing: "prespecified" | "post_hoc" | "unknown" }[];
  evaluation_questions: string[]; constraints: string[];
}
export interface ResearchQuestion {
  meta: ResearchMeta; body: ResearchQuestionBody; previous: ResearchRef | null; revision_reason: string;
}
export interface ResearchDraftPlan {
  objective: string; methodology: string;
  milestones: { id: string; label: string; method: string; completion_criterion: string; included: boolean }[];
  technical_requirements: string[]; evidence_criteria: string[]; risks: string[]; assumptions: string[];
  required_human_decisions: string[]; expected_deliverables: string[];
}
export interface ResearchPlan {
  meta: ResearchMeta; draft_revision: ResearchRef; approved_revision: null; closed: false;
}
export interface ResearchPlanRevision {
  meta: ResearchMeta; plan_id: string; revision_number: number; parent: ResearchRef | null;
  question: ResearchRef; iteration_id: string; body: ResearchDraftPlan;
  origin: "local_supplied_draft"; revision_reason: string; digest: null; identity_status: "not_computed";
}
export interface ResearchRecordActivity {
  meta: ResearchMeta; activity_kind: "record_only"; state: "completed";
  operation_id: string; iteration_id: string; targets: ResearchRef[];
}
export type ResearchRecord =
  | { kind: "project"; record: ResearchProject }
  | { kind: "question"; record: ResearchQuestion }
  | { kind: "plan"; record: ResearchPlan }
  | { kind: "plan_revision"; record: ResearchPlanRevision }
  | { kind: "activity"; record: ResearchRecordActivity };
export interface ResearchMutationResult {
  status: "complete"; operation_id: string; project_id: string; project_version: number;
  commit_sequence: number; records: ResearchRef[];
}
export interface ResearchProjectView { project: ResearchProject; recovery: { orphan_files: number } }
export interface ResearchProjectPage {
  owner_id: string | null;
  items: { project_id: string; status: "available" | "pending_creation" | "unavailable"; project: ResearchProject | null; reason: string | null }[];
  next_cursor: string | null;
}
export interface ResearchEvent {
  id: string; project_sequence: number; activity: ResearchRef; operation_id: string;
  actor_id: string; at_ms: number | null; kind: "local_metadata_saved"; targets: ResearchRef[];
}
export interface ResearchOperationStatus {
  status: "unknown" | "pending" | "complete"; project_id: string | null; result: ResearchMutationResult | null;
}
export interface ResearchScope { schema_version: 1; project_id: string }
export interface ResearchWrite extends ResearchScope { operation_id: string; expected_project_version: number }
export interface ResearchCreate { schema_version: 1; operation_id: string; metadata: ResearchProjectMetadata; goal: string }
export interface ResearchUpdate extends ResearchWrite { metadata: ResearchProjectMetadata }
export interface ResearchQuestionSave extends ResearchWrite { previous: ResearchRef; body: ResearchQuestionBody; revision_reason: string }
export interface ResearchPlanSave extends ResearchWrite {
  plan_id: string | null; parent: ResearchRef | null; question: ResearchRef;
  body: ResearchDraftPlan; revision_reason: string;
}
export interface ResearchCommands {
  research_project_create: [ResearchCreate, ResearchMutationResult];
  research_project_update: [ResearchUpdate, ResearchMutationResult];
  research_project_get: [ResearchScope, ResearchProjectView];
  research_project_list: [{ schema_version: 1; cursor: string | null; limit: number }, ResearchProjectPage];
  research_record_get: [(ResearchScope & { reference: ResearchRef }) | EvidenceGet, ResearchRecord | EvidenceView];
  research_question_save: [ResearchQuestionSave, ResearchMutationResult];
  research_plan_revision_save: [ResearchPlanSave, ResearchMutationResult];
  research_events_page: [(ResearchScope & { cursor: number; limit: number }) | EvidenceEvents, { items: ResearchEvent[]; next_cursor: number | null } | EvidencePage<EvidenceEvent, EventCursor>];
  research_operation_status: [{ schema_version: 1; project_id: string | null; operation_id: string } | EvidenceOperation, ResearchOperationStatus | EvidenceOperationStatus];
}

export function localResearchRecords(backend: Backend) {
  return {
    create: (a: ResearchCreate) => backend.call("research_project_create", a),
    update: (a: ResearchUpdate) => backend.call("research_project_update", a),
    get: (a: ResearchScope) => backend.call("research_project_get", a),
    list: (a: ResearchCommands["research_project_list"][0]) => backend.call("research_project_list", a),
    record: (a: ResearchScope & { reference: ResearchRef }) => backend.call("research_record_get", a) as Promise<ResearchRecord>,
    saveQuestion: (a: ResearchQuestionSave) => backend.call("research_question_save", a),
    saveDraft: (a: ResearchPlanSave) => backend.call("research_plan_revision_save", a),
    events: (a: ResearchScope & { cursor: number; limit: number }) => backend.call("research_events_page", a) as Promise<{ items: ResearchEvent[]; next_cursor: number | null }>,
    operation: (a: { schema_version: 1; project_id: string | null; operation_id: string }) => backend.call("research_operation_status", a) as Promise<ResearchOperationStatus>,
  };
}
