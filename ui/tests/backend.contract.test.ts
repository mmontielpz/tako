import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { BridgeError } from "../src/backend";
import { StdioBackend } from "./helpers/stdioBackend";
import { makeFixture, type Fixture } from "./helpers/fixtures";

let fx: Fixture;
let be: StdioBackend;
beforeEach(() => {
  fx = makeFixture();
  be = new StdioBackend(fx.data);
});
afterEach(() => {
  be.close();
  fx.cleanup();
});

const keys = (o: object) => Object.keys(o).sort();
function hasKeys(o: object, expected: string[]) {
  for (const k of expected) expect(o, `missing key ${k}`).toHaveProperty(k);
}

describe("typed bridge contract against the real engine", () => {
  it("startup_report and open_project return the shapes the UI types describe", async () => {
    const rep = await be.call("startup_report", undefined);
    expect(keys(rep)).toEqual(["busy", "errors", "recovered"]);
    const pv = await be.call("open_project", { path: fx.project });
    hasKeys(pv, ["project", "missions", "isolation"]);
    hasKeys(pv.project, ["path", "branch", "head", "dirty"]);
    expect(pv.isolation.level).toBe("L0_worktree");
    expect(pv.isolation.is_security_sandbox).toBe(false);
    expect(pv.isolation.provider_execution).toBe("disabled");
  });

  it("a full mission produces views with every field the UI reads", async () => {
    let v = await be.call("create_mission", { project_path: fx.project, goal: "g", criteria: ["c"], checks: ["grep -q hi a.txt"] });
    hasKeys(v, ["mission", "plan", "execution", "evidence", "decision", "diff", "job", "next_actions", "isolation"]);
    hasKeys(v.mission, ["id", "project_path", "goal", "acceptance_criteria", "verification_commands", "state", "state_reason", "plan_count", "approval", "evidence_count", "workspace_path", "branch"]);
    expect(v.next_actions).toEqual(["define_plan"]);

    v = await be.call("define_plan", { mission_id: v.mission.id, script: "echo hi > a.txt", expected_files: ["a.txt"] });
    hasKeys(v.plan!, ["version", "hash", "body"]);
    hasKeys(v.plan!.body, ["summary", "steps", "files_expected", "requested_permissions", "risks"]);
    expect(v.plan!.body.steps[0]).toBe("script:echo hi > a.txt");

    v = await be.call("approve_plan", { mission_id: v.mission.id, version: 1, plan_hash: v.plan!.hash, granted: ["fs_write_workspace", "exec_command:sh"] });
    hasKeys(v.mission.approval!, ["plan_version", "plan_hash", "granted", "approved_by", "approved_at"]);

    await be.call("start_execution", { mission_id: v.mission.id });
    for (let i = 0; i < 200 && (v.job.running || v.mission.state === "approved" || v.mission.state === "executing"); i++) {
      await new Promise((r) => setTimeout(r, 50));
      v = await be.call("get_mission", { mission_id: v.mission.id });
    }
    expect(v.mission.state).toBe("sealed");
    hasKeys(v.execution!, ["id", "status", "exit_code", "signal", "duration_ms", "files_changed", "project_touched", "reaped", "isolation_level", "note", "stdout_tail", "stderr_tail", "candidate_commit"]);
    expect(v.diff!.text).toContain("+hi");

    await be.call("start_verification", { mission_id: v.mission.id });
    for (let i = 0; i < 200 && v.mission.state !== "evidence_ready"; i++) {
      await new Promise((r) => setTimeout(r, 50));
      v = await be.call("get_mission", { mission_id: v.mission.id });
    }
    hasKeys(v.evidence!, ["id", "candidate_commit", "subject_tree_hash", "tree_hash_before_verify", "tree_hash_after_verify", "verification_modified_subject", "integrity_violations", "failed_checks", "ignored_artifacts", "checks", "files_changed", "result", "valid", "acceptable", "reasons", "limitations"]);
    hasKeys(v.evidence!.checks[0]!, ["command", "exit_code", "timed_out", "duration_ms", "stdout_tail", "stderr_tail"]);
    expect(v.evidence!.acceptable).toBe(true);

    v = await be.call("accept", { mission_id: v.mission.id, note: "n" });
    hasKeys(v.decision!, ["decision", "evidence_id", "decided_by", "note", "resulting_head", "result_tree_matches_verified"]);
    expect(v.mission.state).toBe("accepted");
  });

  it("engine refusals arrive as typed BridgeError with stable codes", async () => {
    for (const mission_id of ["nope", "PROG-unknown", "GOAL-unknown", "T-unknown"]) {
      await expect(be.call("get_mission", { mission_id })).rejects.toMatchObject({ name: "BridgeError", code: "not_found" });
    }
    await expect(be.call("open_project", { path: fx.root })).rejects.toBeInstanceOf(BridgeError);
    // @ts-expect-error deliberately invalid command, proving the harness rejects unknown commands
    await expect(be.call("format_disk", {})).rejects.toMatchObject({ code: "invalid_input" });
  });

  it("an unsafe repository is refused with the offending keys", async () => {
    const { gitIn } = await import("./helpers/fixtures");
    gitIn(fx.project, "config", "--local", "filter.evil.clean", "touch SENTINEL");
    const e = await be.call("open_project", { path: fx.project }).catch((x) => x as BridgeError);
    expect(e).toBeInstanceOf(BridgeError);
    expect((e as BridgeError).code).toBe("unsafe_repository");
    expect((e as BridgeError).details).toEqual(["local:filter.evil.clean"]);
  });
});
