import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { once } from "node:events";
import { createHash } from "node:crypto";
import { beforeEach, afterEach, expect, it } from "vitest";
import { StdioBackend } from "./helpers/stdioBackend";
import { localResearchRecords } from "../src/researchBridge";
import { localResearchPlanning, type PlanInput, type PlanTarget, type PlanWrite } from "../src/researchPlanningBridge";

let root: string;
const children = new Set<StdioBackend>();
const oid = (n: number) => n.toString(16).padStart(32, "0");
function start() { const b = new StdioBackend(root); children.add(b); return b; }
async function stop(b: StdioBackend) { if (b.child.exitCode === null && b.child.signalCode === null) { const done = once(b.child, "close"); b.close(); await done; } children.delete(b); }
beforeEach(() => { root = fs.mkdtempSync(path.join(os.tmpdir(), "tako-s3-contract-")); });
afterEach(async () => { await Promise.all([...children].map(stop)); fs.rmSync(root, { recursive: true, force: true }); });
async function fixture() {
  const b = start(); const legacy = localResearchRecords(b); const api = localResearchPlanning(b); expect(b.calls).toEqual([]);
  const project = await legacy.create({ schema_version: 1, operation_id: oid(1), metadata: { title: "Typed S3 fixture", primary_type: null, type_unknown_reason: "fixture", secondary_types: [] }, goal: "Local fixture, no research performed" });
  const input = JSON.parse(fs.readFileSync(path.resolve("../bridge/tests/fixtures/research-plan-v3.json"), "utf8")) as PlanInput;
  input.question = (await legacy.get({ schema_version: 1, project_id: project.project_id })).project.active_question as PlanInput["question"];
  const write: PlanWrite = { schema_version: 3, project_id: project.project_id, operation_id: oid(2), expected_project_version: 1, plan_id: null, parent_revision_id: null, input, revision_reason: "Typed fixture" };
  return { b, legacy, api, input, write };
}
function canonical(v: unknown): string {
  if (typeof v === "string") return JSON.stringify(v).replace(/[\u007f-\uffff]/g, c => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  if (Array.isArray(v)) return "[" + v.map(canonical).join(",") + "]";
  if (v !== null && typeof v === "object") return "{" + Object.entries(v).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0).map(([k, x]) => canonical(k) + ":" + canonical(x)).join(",") + "}";
  return JSON.stringify(v);
}
it("persists exact semantic identity and immutable revisions across real native process restart", async () => {
  const { b, api, write } = await fixture(); write.input.body.objective += " μ 🧪\r\n";
  const saved = await api.createDraft(write); const query = { schema_version: 3 as const, project_id: saved.project_id, plan_id: saved.plan_id, revision_id: null };
  const original = await api.read(query);
  const digest = createHash("sha256").update("TAKO-RESEARCH-PLAN-v1\0").update(canonical(original.revision.payload)).digest("hex");
  expect(saved.revision_digest).toEqual({ algorithm: "sha256", value: digest }); expect(original.review_state).toBe("DRAFT");
  expect(original.revision.payload.input.origin.kind).toBe("fixture"); const oldPid = b.child.pid; await stop(b);
  const next = start(); expect(next.child.pid).not.toBe(oldPid); const resumed = localResearchPlanning(next);
  expect(await resumed.read(query)).toEqual(original); expect(await resumed.createDraft(write)).toEqual(saved);
  const revised = await resumed.revise({ ...write, operation_id: oid(3), expected_project_version: 2, plan_id: saved.plan_id, parent_revision_id: saved.revision_id, input: { ...write.input, body: { ...write.input.body, objective: "Revised local scope" } } });
  expect((await resumed.read(query)).revision.revision_number).toBe(2); expect(revised.review_state).toBe("DRAFT");
  expect((await resumed.read({ ...query, revision_id: saved.revision_id })).revision).toEqual(original.revision);
  expect((await resumed.history({ schema_version: 3, project_id: saved.project_id, plan_id: saved.plan_id, cursor: 0, limit: 256 })).items.map(e => e.kind)).toEqual(["CREATED", "REVISED"]);
  expect(next.calls).not.toContain("proposal_start"); expect(next.calls).not.toContain("start_execution");
});
it("requires review submission and refuses unauthenticated approval/rejection without side effects", async () => {
  const { b, api, write } = await fixture(); const saved = await api.createDraft(write);
  const target: PlanTarget = { schema_version: 3, project_id: saved.project_id, plan_id: saved.plan_id, revision_id: saved.revision_id, expected_revision_digest: saved.revision_digest, expected_project_version: 2, operation_id: oid(3) };
  const submitted = await api.submit(target); expect(submitted.review_state).toBe("REVIEW_REQUIRED");
  const intent = { ...target, operation_id: oid(4), expected_project_version: 3, human_review_intent: true as const, review_rationale: "A local flag cannot authenticate a reviewer" };
  await expect(api.approve(intent)).rejects.toMatchObject({ code: "research_review_authority_unavailable" });
  await expect(api.reject(intent)).rejects.toMatchObject({ code: "research_review_authority_unavailable" });
  const history = await api.history({ schema_version: 3, project_id: saved.project_id, plan_id: saved.plan_id, cursor: 0, limit: 256 });
  expect(history.items.map(e => e.kind)).toEqual(["CREATED", "SUBMITTED_REVIEW"]); expect(history.decisions).toEqual([]);
  expect(b.calls).not.toContain("start_execution"); expect(b.calls).not.toContain("proposal_start");
});
it("validates supplied proposals honestly and maintains two-process CAS", async () => {
  const { b, api, write, input } = await fixture();
  const request = { schema_version: 3 as const, project_id: write.project_id, plan_id: null, revision_id: null };
  await expect(api.validate({ ...request, proposal_json: '{"approved":true}' })).rejects.toMatchObject({ code: "research_invalid_input" });
  await expect(api.validate({ ...request, proposal_json: '{"question":null,"question":null}' })).rejects.toMatchObject({ code: "research_invalid_input" });
  const supplied = { ...input, origin: { kind: "external_ai_proposal", description: "Externally reported, no provider executed", provider: "reported-provider", model: "reported-model" } };
  const report = await api.validate({ ...request, proposal_json: JSON.stringify(supplied) }); expect(report.structural_only).toBe(true); expect(report.reviewable).toBe(true); expect(report.warnings.join(" ")).toContain("externally reported");
  const peer = start(); const alternatives = [write, { ...write, operation_id: oid(3) }];
  const outcomes = await Promise.allSettled([api.createDraft(alternatives[0]!), localResearchPlanning(peer).createDraft(alternatives[1]!)]);
  for (let i = 0; i < outcomes.length; i++) if (outcomes[i]?.status === "rejected" && (outcomes[i] as PromiseRejectedResult).reason.code === "research_busy") outcomes[i] = (await Promise.allSettled([localResearchPlanning(i === 0 ? b : peer).createDraft(alternatives[i]!)]))[0]!;
  expect(outcomes.filter(x => x.status === "fulfilled")).toHaveLength(1); expect(outcomes.filter(x => x.status === "rejected").map(x => (x as PromiseRejectedResult).reason.code)).toEqual(["research_conflict"]);
});
