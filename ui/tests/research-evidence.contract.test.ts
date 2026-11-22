import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { once } from "node:events";
import { createHash } from "node:crypto";
import { afterEach, beforeEach, expect, it } from "vitest";
import { localResearchRecords } from "../src/researchBridge";
import { localResearchEvidence, type EvidencePacket, type EvidenceRef } from "../src/researchEvidenceBridge";
import { StdioBackend } from "./helpers/stdioBackend";

let root: string;
const children = new Set<StdioBackend>();
function start() { const b = new StdioBackend(root); children.add(b); return b; }
async function stop(b: StdioBackend) { if (b.child.exitCode === null && b.child.signalCode === null) { const done = once(b.child, "close"); b.close(); await done; } children.delete(b); }
const id = (n: number) => n.toString(16).padStart(32, "0");
const ref = (records: EvidenceRef[], kind: EvidenceRef["kind"]) => { const r = records.find(r => r.kind === kind); if (!r) throw new Error("missing native ref"); return r; };
beforeEach(() => { root = fs.mkdtempSync(path.join(os.tmpdir(), "tako-s2-contract-")); });
afterEach(async () => { await Promise.all([...children].map(stop)); fs.rmSync(root, { recursive: true, force: true }); });
async function prepared() {
  const b = start(); const api = localResearchRecords(b); const s2 = localResearchEvidence(b);
  expect(b.calls).toEqual([]);
  const p = await api.create({ schema_version: 1, operation_id: id(1), goal: "Local fixture; no research performed", metadata: { title: "Typed fixture", primary_type: null, type_unknown_reason: "fixture", secondary_types: [] } });
  const scope = { schema_version: 2 as const, project_id: p.project_id };
  await s2.prepare({ ...scope, operation_id: id(2), expected_project_version: 1 });
  const packet: EvidencePacket = { ...scope, operation_id: id(3), expected_project_version: 2, previous: null, revision_reason: "local test", claim: "Supplied note", stance: "unknown", limitations: ["Unverified fixture"], origin: "local_input", text: "A supplied note: μ\r\n", artifact: null, source: { title: "Reported source", authors: [], date: null, url: "https://invalid.example/never-fetch", doi: null, attribution: "User supplied", previous: null }, source_ref: null, question: null, plan_revision: null, association_reason: "project note" };
  return { b, api, s2, scope, packet };
}
it("persists typed evidence/provenance and exact digest across actual native process restart", async () => {
  const { b, s2, scope, packet } = await prepared(); const saved = await s2.record(packet);
  const evidenceRef = ref(saved.records, "evidence"); const artifactRef = ref(saved.records, "artifact");
  const artifact = await s2.get({ ...scope, reference: artifactRef });
  if (artifact.kind !== "artifact") throw new Error("wrong native kind");
  expect(artifact.record.digest).toEqual({ algorithm: "sha256", value: createHash("sha256").update(packet.text!).digest("hex") });
  expect(artifact.record.byte_length).toBe(Buffer.byteLength(packet.text!));
  const source = await s2.get({ ...scope, reference: ref(saved.records, "source") });
  expect(source.kind === "source" && source.record.content_state).toBe("metadata_only");
  expect(await s2.text({ ...scope, reference: artifactRef })).toMatchObject({ reference: artifactRef, text: packet.text, integrity: "matches" });
  const evidence = await s2.get({ ...scope, reference: evidenceRef }); const oldPid = b.child.pid;
  await stop(b); const next = start(); expect(next.child.pid).not.toBe(oldPid);
  expect(await localResearchEvidence(next).get({ ...scope, reference: evidenceRef })).toEqual(evidence);
  expect(await localResearchEvidence(next).record(packet)).toEqual(saved);
  expect(evidence.inspection).toMatchObject({ integrity: "matches", scientific_assessment: "not_assessed", independent_verification: "not_established" });
  expect(next.calls).not.toContain("proposal_start"); expect(next.calls).not.toContain("start_execution");
});
it("keeps two-process CAS, explicit typed revisions and deterministic activity cursors", async () => {
  const { b, s2, scope, packet, api } = await prepared(); const peer = start();
  const writes = await Promise.allSettled([s2.record(packet), localResearchEvidence(peer).record({ ...packet, operation_id: id(4) })]);
  // Busy is an explicit local lock outcome. Retry only that exact fixture request.
  for (let i = 0; i < writes.length; i++) {
    if (writes[i]?.status === "rejected" && (writes[i] as PromiseRejectedResult).reason.code === "research_busy") {
      writes[i] = (await Promise.allSettled([localResearchEvidence(i === 0 ? b : peer).record({ ...packet, operation_id: id(i === 0 ? 3 : 4) })]))[0]!;
    }
  }
  expect(writes.filter(r => r.status === "fulfilled")).toHaveLength(1);
  expect(writes.filter(r => r.status === "rejected").map(r => (r as PromiseRejectedResult).reason.code)).toEqual(["research_conflict"]);
  const p = await api.get({ schema_version: 1, project_id: scope.project_id });
  await api.update({ schema_version: 1, project_id: scope.project_id, operation_id: id(5), expected_project_version: 3, metadata: { ...p.project.metadata, title: "After evidence" } });
  const first = await s2.events({ ...scope, cursor: null, limit: 2 }); expect(first.items.map(e => e.project_sequence)).toEqual([1, 2]);
  const second = await s2.events({ ...scope, cursor: first.next_cursor, limit: 2 }); expect(second.items.map(e => e.project_sequence)).toEqual([3, 4]);
  expect(second.next_cursor).toBeNull(); expect((await s2.activities({ ...scope, cursor: null, limit: 256 })).items).toHaveLength(4);
});
it("reports changed and missing content without repairing acknowledged evidence", async () => {
  const { s2, scope, packet } = await prepared(); const saved = await s2.record(packet); const artifactRef = ref(saved.records, "artifact");
  const owner = JSON.parse(fs.readFileSync(path.join(root, "research/v1/owner-v1.json"), "utf8")).owner_id as string;
  const blob = path.join(root, "research/v1", owner, scope.project_id, "blobs", artifactRef.id);
  fs.writeFileSync(blob, "tampered");
  await expect(s2.text({ ...scope, reference: artifactRef })).rejects.toMatchObject({ code: "research_integrity_mismatch" });
  expect((await s2.get({ ...scope, reference: ref(saved.records, "evidence") })).inspection.integrity).toBe("mismatch");
  fs.renameSync(blob, path.join(root, "quarantined-blob"));
  await expect(s2.record(packet)).rejects.toMatchObject({ code: "research_missing_evidence" });
  expect(fs.existsSync(blob)).toBe(false);
  expect(await s2.operation({ ...scope, operation_id: packet.operation_id })).toMatchObject({ status: "complete", material_status: "unavailable" });
});
