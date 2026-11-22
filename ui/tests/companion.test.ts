import { describe, expect, it } from "vitest";
import { companionFor, primaryAction } from "../src/companion";
import type { Evidence, MissionState, MissionView } from "../src/types";

// Pure-logic tests. The views are crafted here; the real engine's views are exercised in app.e2e.test.tsx.
const ALL_STATES: MissionState[] = [
  "created", "plan_ready", "approved", "executing", "cancelled", "sealed", "execution_failed",
  "timed_out", "interrupted", "verifying", "evidence_ready", "accepted", "rejected",
];

function evidence(over: Partial<Evidence> = {}): Evidence {
  return {
    id: "m1-e1", candidate_commit: "c", subject_tree_hash: "t", tree_hash_before_verify: "t", tree_hash_after_verify: "t",
    verification_modified_subject: false, integrity_violations: [], failed_checks: [], ignored_artifacts: [],
    checks: [{ command: ["sh", "-c", "true"], exit_code: 0, timed_out: false, duration_ms: 1, stdout_tail: "", stderr_tail: "" }],
    files_changed: ["a"], result: "pass", valid: true, acceptable: true, reasons: [], limitations: [], ...over,
  };
}

function view(state: MissionState, over: Partial<MissionView> = {}): MissionView {
  return {
    mission: {
      id: "m1", project_path: "/p", target_branch: "main", base_commit: "b", goal: "g", acceptance_criteria: ["c"],
      verification_commands: [], state, state_reason: null, plan_count: 1,
      approval: { plan_version: 1, plan_hash: "h", granted: ["fs_write_workspace"], approved_by: "me", approved_at: 0 },
      evidence_count: 0, workspace_path: "/w", branch: "tako/m1",
    },
    plan: { mission_id: "m1", version: 1, hash: "h", body: { summary: "s", steps: ["script:true"], files_expected: [], requested_permissions: ["fs_write_workspace"], risks: [] } },
    execution: { id: "x", status: "succeeded", exit_code: 0, signal: null, duration_ms: 1, files_changed: ["a"], project_touched: false, reaped: true, isolation_level: "L0_worktree", note: null, stdout_tail: "", stderr_tail: "", candidate_commit: "c" },
    evidence: null, decision: null, diff: null,
    job: { running: false, kind: null, error: null },
    next_actions: [], isolation: { level: "L0_worktree", notice: "n", is_security_sandbox: false, provider_execution: "disabled" },
    ...over,
  };
}

const SUCCESS_WORDS = /succe|passed|\bverified\b|all good|\bdone\b/i;

describe("Tako never claims success without linked, valid, passing evidence", () => {
  const evidences: (Evidence | null)[] = [null, evidence({ result: "fail", failed_checks: [0], acceptable: false }),
    evidence({ valid: false, acceptable: false, integrity_violations: ["subject_modified"] })];

  for (const state of ALL_STATES) {
    for (const [i, ev] of evidences.entries()) {
      it(`${state} / evidence #${i}`, () => {
        const say = companionFor({ hasProject: true, view: view(state, { evidence: ev }), error: null, recovered: 0 });
        expect(`${say.headline} ${say.detail}`).not.toMatch(SUCCESS_WORDS);
        expect(say.evidenceId === null || ev !== null || state === "accepted").toBe(true);
      });
    }
  }

  it("says verification passed only when the evidence is valid, passing and acceptable", () => {
    const say = companionFor({ hasProject: true, view: view("evidence_ready", { evidence: evidence() }), error: null, recovered: 0 });
    expect(say.headline).toBe("Verification passed.");
    expect(say.evidenceId).toBe("m1-e1");
    expect(say.pose).toBe("waiting");
  });

  it("does not say passed when result is pass but evidence is invalid", () => {
    const say = companionFor({ hasProject: true, view: view("evidence_ready", { evidence: evidence({ valid: false, acceptable: false, integrity_violations: ["subject_modified"] }) }), error: null, recovered: 0 });
    expect(say.pose).toBe("alert");
    expect(say.headline).toMatch(/invalid/i);
    expect(say.detail).toContain("subject_modified");
  });

  it("evidence_ready with no evidence is an alert, not a success", () => {
    const say = companionFor({ hasProject: true, view: view("evidence_ready"), error: null, recovered: 0 });
    expect(say.pose).toBe("alert");
  });

  it("does not treat the agent's exit code as success", () => {
    const say = companionFor({ hasProject: true, view: view("sealed"), error: null, recovered: 0 });
    expect(say.detail).toMatch(/not verification/);
  });

  it("acceptance without matching evidence is explicitly unverified", () => {
    const say = companionFor({ hasProject: true, view: view("accepted"), error: null, recovered: 0 });
    expect(say.headline).toBe("Acceptance is unverified.");
    expect(say.pose).toBe("alert");
    expect(say.evidenceId).toBeNull();
  });

  it("an accepted mission cites the recorded evidence", () => {
    const say = companionFor({ hasProject: true, view: view("accepted", { evidence: evidence(), decision: { decision: "accept", evidence_id: "m1-e1", decided_by: "me", note: "", resulting_head: "h", result_tree_matches_verified: true } }), error: null, recovered: 0 });
    expect(say.detail).toContain("m1-e1");
  });
});

describe("Tako reflects real state", () => {
  it("poses", () => {
    const pose = (v: MissionView | null, hasProject = true) => companionFor({ hasProject, view: v, error: null, recovered: 0 }).pose;
    expect(pose(null, false)).toBe("idle");
    expect(pose(null)).toBe("waiting");
    expect(pose(view("plan_ready"))).toBe("waiting");
    expect(pose(view("executing", { job: { running: true, kind: "execution", error: null } }))).toBe("working");
    expect(pose(view("verifying", { job: { running: true, kind: "verification", error: null } }))).toBe("working");
    expect(pose(view("execution_failed"))).toBe("alert");
    expect(pose(view("timed_out"))).toBe("alert");
    expect(pose(view("cancelled"))).toBe("alert");
    expect(pose(view("interrupted"))).toBe("alert");
    expect(pose(view("rejected"))).toBe("idle");
  });

  it("surfaces errors with plain-language guidance", () => {
    const say = (code: string) => companionFor({ hasProject: true, view: null, error: { code, message: "raw", details: [] }, recovered: 0 });
    expect(say("busy").pose).toBe("alert");
    expect(say("busy").detail).toMatch(/already running/);
    expect(say("unsafe_repository").detail).toMatch(/refuses to touch/);
    expect(say("isolation_required").detail).toMatch(/disabled/);
    expect(say("something_new").detail).toBe("raw");
    // an error never leaves the user with a contradictory "nothing to do"
    const withView = companionFor({ hasProject: true, view: view("rejected"), error: { code: "unsafe_repository", message: "m", details: [] }, recovered: 0 });
    expect(withView.next).toMatch(/different project/);
  });

  it("explains the approval it needs and what is requested", () => {
    const say = companionFor({ hasProject: true, view: view("plan_ready", { next_actions: ["approve_plan"] }), error: null, recovered: 0 });
    expect(say.detail).toContain("fs_write_workspace");
    expect(say.detail).toMatch(/None are granted until you tick/);
    expect(say.next).toMatch(/approve/i);
  });

  it("names the next valid action from the engine's list only", () => {
    expect(primaryAction(view("sealed", { next_actions: ["start_verification", "reject"] }))).toMatch(/verification/);
    expect(primaryAction(view("accepted", { next_actions: [] }))).toBe("Nothing to do here.");
  });

  it("announces recovery after a crash", () => {
    const say = companionFor({ hasProject: false, view: null, error: null, recovered: 2 });
    expect(say.pose).toBe("alert");
    expect(say.headline).toMatch(/recovered 2/i);
  });
});
