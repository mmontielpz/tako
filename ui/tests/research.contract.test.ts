import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { once } from "node:events";
import { spawnSync } from "node:child_process";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { localResearchRecords, type ResearchCreate, type ResearchDraftPlan } from "../src/researchBridge";
import { StdioBackend } from "./helpers/stdioBackend";

let root: string;
const children = new Set<StdioBackend>();
function start(dir = root) { const b = new StdioBackend(dir); children.add(b); return b; }
async function stop(b: StdioBackend) {
  if (b.child.exitCode === null && b.child.signalCode === null) {
    const done = once(b.child, "close"); b.close(); await done;
  }
  children.delete(b);
}
const id = (n: number) => n.toString(16).padStart(32, "0");
const createArgs = (n = 1): ResearchCreate => ({ schema_version: 1, operation_id: id(n), goal: "Which evidence addresses this question?",
  metadata: { title: "Research", primary_type: null, type_unknown_reason: "not selected", secondary_types: [] } });
const body: ResearchDraftPlan = { objective: "Review supplied notes", methodology: "", milestones: [], technical_requirements: [], evidence_criteria: [], risks: [], assumptions: [], required_human_decisions: [], expected_deliverables: [] };
beforeEach(() => { root = fs.mkdtempSync(path.join(os.tmpdir(), "tako-research-contract-")); });
afterEach(async () => { await Promise.all([...children].map(stop)); fs.rmSync(root, { recursive: true, force: true }); });

describe("S1 typed bridge against real native processes, no Research UI or providers", () => {
  it("is lazy and restores actual records and owner identity in a new native process", async () => {
    let b = start(); const api = localResearchRecords(b); expect(b.calls).toEqual([]);
    expect(await api.list({ schema_version: 1, cursor: null, limit: 256 })).toEqual({ owner_id: null, items: [], next_cursor: null });
    expect(fs.existsSync(path.join(root, "research"))).toBe(false);
    const created = await api.create(createArgs()); const scope = { schema_version: 1 as const, project_id: created.project_id };
    const before = await api.get(scope); const oldPid = b.child.pid;
    await stop(b); b = start(); const after = await localResearchRecords(b).get(scope);
    expect(b.child.pid).not.toBe(oldPid); expect(after).toEqual(before);
    expect(after.project.meta.owner_id).toMatch(/^[0-9a-f]{32}$/);
    expect(await localResearchRecords(b).create(createArgs())).toEqual(created);
    expect(b.count("proposal_start")).toBe(0); expect(b.count("start_execution")).toBe(0);
    expect(await b.call("integration_status", undefined)).toMatchObject({ availability: "disabled", provider_execution: "disabled" });
  });

  it("preserves typed question and draft revisions, metadata versions and event pages", async () => {
    const b = start(); const api = localResearchRecords(b); const made = await api.create(createArgs());
    const scope = { schema_version: 1 as const, project_id: made.project_id };
    let p = (await api.get(scope)).project; const originalQuestion = p.active_question;
    const updated = await api.update({ ...scope, operation_id: id(2), expected_project_version: 1, metadata: { ...p.metadata, title: "Refined" } });
    expect(updated.project_version).toBe(2);
    await api.saveQuestion({ ...scope, operation_id: id(3), expected_project_version: 2, previous: p.active_question,
      body: { question: "Refined question", objectives: [], hypotheses: [], evaluation_questions: [], constraints: [] }, revision_reason: "refine" });
    p = (await api.get(scope)).project;
    await api.saveDraft({ ...scope, operation_id: id(4), expected_project_version: 3, plan_id: null, parent: null, question: p.active_question, body, revision_reason: "initial" });
    p = (await api.get(scope)).project; const plan = await api.record({ ...scope, reference: p.active_plan! });
    expect(plan.kind).toBe("plan"); if (plan.kind !== "plan") throw new Error("wrong native kind");
    const revision = plan.record.draft_revision;
    await api.saveDraft({ ...scope, operation_id: id(5), expected_project_version: 4, plan_id: plan.record.meta.id, parent: revision,
      question: p.active_question, body: { ...body, objective: "Narrower scope" }, revision_reason: "narrow" });
    const old = await api.record({ ...scope, reference: revision });
    expect(old.kind).toBe("plan_revision"); if (old.kind !== "plan_revision") throw new Error("wrong native kind");
    expect(old.record.body.objective).toBe(body.objective); expect(old.record.digest).toBeNull(); expect(old.record.identity_status).toBe("not_computed");
    const q = await api.record({ ...scope, reference: originalQuestion });
    expect(q.kind === "question" && q.record.body.question).toBe(createArgs().goal);
    const events = await api.events({ ...scope, cursor: 0, limit: 2 });
    expect(events.items).toHaveLength(2); expect(events.next_cursor).toBe(2);
    expect(events.items[0]!.kind).toBe("local_metadata_saved");
  });

  it("deduplicates operation keys, reports conflict and isolates projects and profiles", async () => {
    const b = start(); const api = localResearchRecords(b); const a = await api.create(createArgs());
    expect(await api.create(createArgs())).toEqual(a);
    await expect(api.create({ ...createArgs(), goal: "changed" })).rejects.toMatchObject({ code: "research_idempotency_conflict", details: [] });
    const other = await api.create(createArgs(2));
    const scope = { schema_version: 1 as const, project_id: a.project_id };
    const q = (await api.get(scope)).project.active_question;
    await expect(api.record({ ...scope, project_id: other.project_id, reference: q })).rejects.toMatchObject({ code: "research_not_found" });
    expect((await api.operation({ schema_version: 1, project_id: null, operation_id: id(1) })).result).toEqual(a);
    const fresh = localResearchRecords(start(path.join(root, "other-profile")));
    expect((await fresh.list({ schema_version: 1, cursor: null, limit: 1 })).items).toEqual([]);
  });

  it("serializes concurrent native writers with explicit CAS and retryable Busy", async () => {
    const first = localResearchRecords(start()); const second = localResearchRecords(start());
    const a = await first.create(createArgs()); const p = (await first.get({ schema_version: 1, project_id: a.project_id })).project;
    const write = async (api: ReturnType<typeof localResearchRecords>, op: number) => {
      const args = { schema_version: 1 as const, project_id: a.project_id, operation_id: id(op), expected_project_version: 1, metadata: { ...p.metadata, title: `Writer ${op}` } };
      for (let i = 0; i < 100; i++) {
        try { return await api.update(args); } catch (e) {
          if ((e as { code: string }).code !== "research_busy") throw e;
          await new Promise(r => setTimeout(r, 5));
        }
      }
      throw new Error("busy did not resolve");
    };
    const outcomes = await Promise.allSettled([write(first, 2), write(second, 3)]);
    expect(outcomes.filter(o => o.status === "fulfilled")).toHaveLength(1);
    const rejected = outcomes.find(o => o.status === "rejected") as PromiseRejectedResult;
    expect(rejected.reason).toMatchObject({ code: "research_conflict" });
    expect((await first.get({ schema_version: 1, project_id: a.project_id })).project.meta.version).toBe(2);
  });

  it("rejects duplicate JSON keys, floats, unknown fields and traversal at the raw native boundary", () => {
    const bin = path.resolve("../target/debug/tako-bridge-stdio");
    const good = JSON.stringify({ id: 1, cmd: "research_project_create", args: createArgs() });
    const duplicate = good.replace('"schema_version":1', '"schema_version":1,"schema_version":1');
    const float = good.replace('"schema_version":1', '"schema_version":1.0');
    for (const request of [duplicate, float]) {
      const result = spawnSync(bin, [root], { input: request + "\n", encoding: "utf8", env: { ...process.env, OPENAI_API_KEY: "" } });
      expect(result.status).toBe(0); expect(JSON.parse(result.stdout).err).toMatchObject({ code: "research_invalid_input", details: [] });
    }
    expect(fs.existsSync(path.join(root, "research"))).toBe(false);
  });

  it("reports corruption after process restart and preserves corrupt bytes and healthy Engineering", async () => {
    let b = start(); const api = localResearchRecords(b); const made = await api.create(createArgs());
    const p = (await api.get({ schema_version: 1, project_id: made.project_id })).project;
    const file = path.join(root, "research/v1", p.meta.owner_id, made.project_id, "commits/00000000000000000001.json");
    const corrupt = "{PRIVATE_SENTINEL"; fs.writeFileSync(file, corrupt); await stop(b); b = start();
    await expect(localResearchRecords(b).get({ schema_version: 1, project_id: made.project_id })).rejects.toMatchObject({ code: "research_corrupt_store", details: [] });
    expect(fs.readFileSync(file, "utf8")).toBe(corrupt);
    expect(await b.call("onboarding_open_workspace", { workspace: "engineering" })).toEqual({ workspace: "engineering", opened: true });
    expect(b.count("proposal_start")).toBe(0);
  });

  it("keeps acknowledged missing projects discoverable after a real process restart without rebuilding data", async () => {
    let b = start(); let api = localResearchRecords(b); const made = await api.create(createArgs());
    const scope = { schema_version: 1 as const, project_id: made.project_id };
    const p = (await api.get(scope)).project;
    const dataPath = path.join(root, "research/v1", p.meta.owner_id, made.project_id);
    const saved = path.join(root, "quarantined-project");
    fs.renameSync(dataPath, saved); const initialBytes = fs.readFileSync(path.join(saved, "commits/00000000000000000001.json"));
    await stop(b); b = start(); api = localResearchRecords(b);
    for (let n = 0; n < 3; n++) {
      expect((await api.list({ schema_version: 1, cursor: null, limit: 1 })).items).toEqual([
        { project_id: made.project_id, project: null, status: "unavailable", reason: "research_missing_project_data" },
      ]);
      await expect(api.get(scope)).rejects.toMatchObject({ code: "research_missing_project_data", details: [] });
      await expect(api.operation({ schema_version: 1, project_id: null, operation_id: id(1) })).rejects.toMatchObject({ code: "research_missing_project_data" });
      await expect(api.create(createArgs())).rejects.toMatchObject({ code: "research_missing_project_data" });
      expect(fs.existsSync(dataPath)).toBe(false);
    }
    expect(fs.readFileSync(path.join(saved, "commits/00000000000000000001.json"))).toEqual(initialBytes);
    expect(b.count("proposal_start")).toBe(0); expect(b.count("start_execution")).toBe(0);
  });
});
