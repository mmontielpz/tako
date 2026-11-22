/** Headless local S2 bindings. No import-time calls, renderer access or UI activation. */
import type { Backend } from "./backend";
import type { ResearchMeta, ResearchMutationResult, ResearchRecord } from "./researchBridge";
export interface EvidenceRef { kind: "project" | "question" | "plan" | "plan_revision" | "activity" | "source" | "artifact" | "evidence"; id: string; version: number }
export interface EvidenceScope { schema_version: 2; project_id: string }
export interface EvidenceWrite extends EvidenceScope { operation_id: string; expected_project_version: number }
export interface EvidenceSourceInput { title: string; authors: string[]; date: string | null; url: string | null; doi: string | null; attribution: string; previous: EvidenceRef | null }
export interface EvidencePacket extends EvidenceWrite {
  previous: EvidenceRef | null; revision_reason: string; claim: string;
  stance: "supporting" | "challenging" | "contextual" | "insufficient" | "unknown";
  limitations: string[]; origin: "local_input" | "imported_reported" | "ai_generated_reported";
  text: string | null; artifact: EvidenceRef | null; source: EvidenceSourceInput | null; source_ref: EvidenceRef | null;
  question: EvidenceRef | null; plan_revision: EvidenceRef | null; association_reason: string | null;
}
export type EvidenceMeta = Omit<ResearchMeta, "schema_version"> & { schema_version: 2 };
export interface EvidenceRecord {
  meta: EvidenceMeta; activity: EvidenceRef; previous: EvidenceRef | null; revision_reason: string; claim: string;
  stance: EvidencePacket["stance"]; limitations: string[]; origin: EvidencePacket["origin"];
  source: EvidenceRef | null; artifact: EvidenceRef; question: EvidenceRef | null;
  plan_revision: EvidenceRef | null; association_reason: string | null;
  scientific_assessment: "not_assessed"; independent_verification: "not_established";
}
export interface EvidenceArtifact {
  meta: EvidenceMeta; kind: "note"; activity: EvidenceRef; origin: EvidencePacket["origin"];
  source: EvidenceRef | null; supersedes: EvidenceRef | null; blob_id: string; media_type: "text/plain";
  byte_length: number; digest: { algorithm: "sha256"; value: string }; recorded_availability: "available";
}
export interface EvidenceActivity { meta: EvidenceMeta; activity_kind: "record_only"; state: "completed"; operation_id: string; iteration_id: string; targets: EvidenceRef[]; outcome: string }
export interface EvidenceSource { meta: EvidenceMeta; body: EvidenceSourceInput; origin: "supplied_metadata"; activity: EvidenceRef; content_state: "metadata_only" }
export type EvidenceTagged =
  | { kind: "evidence"; record: EvidenceRecord }
  | { kind: "artifact"; record: EvidenceArtifact }
  | { kind: "source"; record: EvidenceSource }
  | { kind: "activity"; record: EvidenceActivity };
export interface EvidenceInspection { integrity: "not_checked" | "matches" | "mismatch" | "unavailable"; availability?: "available" | "unavailable"; review_state?: "recorded" | "needs_review"; reason?: string; scientific_assessment: "not_assessed"; independent_verification?: "not_established" }
export type EvidenceView = (EvidenceTagged | ResearchRecord) & { inspection: EvidenceInspection };
export interface EvidenceResult extends Omit<ResearchMutationResult, "records"> { records: EvidenceRef[] }
export interface EventCursor { project_sequence: number; event_index: number }
export interface EvidenceEvent extends EventCursor { id: string; version: 1; activity: EvidenceRef; operation_id: string; actor_id: string; at_ms: number | null; clock_reason: string | null; kind: string; targets: EvidenceRef[]; outcome: string }
export interface EvidencePage<T, C> { items: T[]; next_cursor: C | null }
export interface EvidenceGet extends EvidenceScope { reference: EvidenceRef }
export interface EvidenceEvents extends EvidenceScope { cursor: EventCursor | null; limit: number }
export interface EvidenceOperation extends EvidenceScope { operation_id: string }
export interface EvidenceOperationStatus { status: "unknown" | "pending" | "pending_preparation" | "conflict" | "complete"; result: EvidenceResult | ResearchMutationResult | null; material_status?: "available" | "unavailable"; pending_operations?: string[] }
export interface ResearchEvidenceCommands {
  research_prepare_evidence_store: [EvidenceWrite, EvidenceResult];
  research_evidence_record: [EvidencePacket, EvidenceResult];
  research_evidence_correct: [EvidencePacket, EvidenceResult];
  research_evidence_list: [EvidenceScope & { cursor: string | null; limit: number }, EvidencePage<Extract<EvidenceTagged, { kind: "evidence" }> & { inspection: EvidenceInspection }, string>];
  research_activity_list: [EvidenceEvents, EvidencePage<Extract<EvidenceTagged | ResearchRecord, { kind: "activity" }>, EventCursor>];
  research_artifact_text_get: [EvidenceGet, { reference: EvidenceRef; text: string; media_type: "text/plain"; integrity: "matches" }];
}
export function localResearchEvidence(backend: Backend) {
  return {
    prepare: (a: EvidenceWrite) => backend.call("research_prepare_evidence_store", a),
    record: (a: EvidencePacket) => backend.call("research_evidence_record", a),
    correct: (a: EvidencePacket) => backend.call("research_evidence_correct", a),
    get: (a: EvidenceGet) => backend.call("research_record_get", a) as Promise<EvidenceView>,
    list: (a: ResearchEvidenceCommands["research_evidence_list"][0]) => backend.call("research_evidence_list", a),
    activities: (a: EvidenceEvents) => backend.call("research_activity_list", a),
    events: (a: EvidenceEvents) => backend.call("research_events_page", a) as Promise<EvidencePage<EvidenceEvent, EventCursor>>,
    text: (a: EvidenceGet) => backend.call("research_artifact_text_get", a),
    operation: (a: EvidenceOperation) => backend.call("research_operation_status", a) as Promise<EvidenceOperationStatus>,
  };
}
