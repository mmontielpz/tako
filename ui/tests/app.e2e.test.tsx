import fs from "node:fs";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { App } from "../src/App";
import { StdioBackend } from "./helpers/stdioBackend";
import { gitIn, makeFixture, type Fixture } from "./helpers/fixtures";
import { check, click, mount, must, q, text, tick, type, waitFor } from "./helpers/dom";

// These tests render the real UI against the REAL Rust engine (via the stdio harness) using
// disposable Git projects. Nothing in the UI path is mocked.

let fx: Fixture;
let be: StdioBackend;
let ui: ReturnType<typeof mount> | null = null;

beforeEach(() => {
  fx = makeFixture();
  be = new StdioBackend(fx.data);
});
afterEach(() => {
  ui?.unmount();
  ui = null;
  be.close();
  fx.cleanup();
});

function start(backend = be) {
  ui = mount(<App backend={backend} pollMs={80} />);
}

async function openProject() {
  await waitFor("empty state", () => !!q("empty-state"));
  await type("project-path", fx.project);
  await click("open-project");
  await waitFor("project card", () => text("project-name") === fx.project);
}

async function createMission(opts: { goal?: string; checks?: string } = {}) {
  await type("goal", opts.goal ?? "Add hello.txt");
  await type("checks", opts.checks ?? "grep -q hello hello.txt");
  await click("create-mission");
  await waitFor("mission card", () => !!q("mission-card"));
}

async function definePlan(script?: string) {
  if (script) await type("script", script);
  await click("define-plan");
  await waitFor("plan card", () => !!q("plan-card") && !!q("approve"));
}

const BOTH = ["perm-fs_write_workspace", "perm-exec_command:sh"];
async function approveAll() {
  for (const p of BOTH) await check(p);
  await tick();
  await click("approve");
  await waitFor("approval", () => !!q("approval-line"));
}

async function runToSealed() {
  await waitFor("start button", () => !!q("start-run"));
  await click("start-run");
  await waitFor("sealed", () => text("state-chip") === "sealed");
}

async function verify() {
  await click("verify");
  await waitFor("evidence", () => text("state-chip") === "evidence_ready" && !!q("evidence"));
}

describe("desktop UI against the real engine", () => {
  it("completes the whole workflow and accepts", async () => {
    start();
    expect(text("companion-headline")).toMatch(/Hi, I'm Tako/);
    expect(q("companion")!.dataset.pose).toBe("idle");
    expect(text("isolation-badge")).toContain("not a security sandbox");

    await openProject();
    expect(q("companion")!.dataset.pose).toBe("waiting");
    await createMission();
    expect(text("state-chip")).toBe("created");

    await definePlan();
    expect(text("state-chip")).toBe("plan_ready");
    expect(text("plan-script")).toContain("hello from a scripted agent");
    // nothing is pre-selected, and the review card (not the revise form) is the active one
    for (const p of BOTH) expect(must<HTMLInputElement>(p).checked).toBe(false);
    expect(q("plan-card")!.getAttribute("data-active")).toBe("true");
    expect(q("plan-form")!.getAttribute("data-active")).toBe("false");
    expect(text("zero-grants-warning")).toMatch(/No permissions ticked/);
    expect(document.querySelector(".missions")!.textContent).toContain("plan_ready"); // sidebar tracks real state
    expect(text("approve")).toBe("Approve plan v1 with 0 permission(s)");
    expect(text("companion-detail")).toMatch(/None are granted until you tick/);
    expect(must<HTMLInputElement>("provider-radio").disabled).toBe(true); // AI provider stays disabled

    await approveAll();
    expect(text("state-chip")).toBe("approved");
    expect(q("zero-grants-warning")).toBeNull();
    expect(q("run-card")!.getAttribute("data-active")).toBe("true");
    expect(text("approval-line")).toContain("fs_write_workspace, exec_command:sh");

    await runToSealed();
    expect(text("exec-status")).toBe("succeeded");
    expect(text("agent-stdout")).toContain("done writing");
    expect(text("diff")).toContain("+hello from a scripted agent");
    expect(text("companion-headline")).toMatch(/changes are sealed/);
    expect(text("companion-detail")).toMatch(/not verification/);
    expect(q("evidence")).toBeNull();
    expect(fs.existsSync(path.join(fx.project, "hello.txt"))).toBe(false);

    await verify();
    expect(text("evidence-result")).toBe("PASS");
    expect(text("evidence-validity")).toBe("evidence intact");
    expect(text("check-exit-0")).toBe("0");
    expect(text("companion-headline")).toBe("Verification passed.");
    expect(text("companion-evidence")).toContain(text("evidence-id"));
    expect(text("decision-evidence-summary")).toMatch(/PASS, intact · 1\/1 checks passed · changes: hello.txt/);
    expect(q("accept")).not.toBeNull();

    await click("accept");
    await waitFor("accepted", () => text("state-chip") === "accepted");
    expect(text("decision")).toContain("Accepted");
    expect(text("decision")).toContain(text("evidence-id"));
    expect(q("accept")).toBeNull();
    expect(fs.readFileSync(path.join(fx.project, "hello.txt"), "utf8")).toContain("hello from a scripted agent");
    expect(gitIn(fx.project, "status", "--porcelain")).toBe("");

    // a second mission starts from an empty objective, not the previous one's text
    await click("new-mission");
    await waitFor("goal form", () => !!q("goal-form"));
    expect(must<HTMLTextAreaElement>("goal").value).toBe("");
  });

  it("reopening a project resumes the mission that is still in progress", async () => {
    start();
    await openProject();
    await createMission();
    await definePlan();
    await approveAll();
    ui!.unmount();
    ui = null;
    // a brand-new window (fresh UI state) over the same engine data
    start();
    await waitFor("empty", () => !!q("empty-state"));
    await type("project-path", fx.project);
    await click("open-project");
    await waitFor("resumed", () => text("state-chip") === "approved");
    expect(text("mission-goal")).toBe("Add hello.txt");
    expect(q("start-run")).not.toBeNull();
    expect(q("goal-form")).toBeNull();
  });

  it("reject needs a confirmation and leaves the project untouched", async () => {
    start();
    await openProject();
    const head = gitIn(fx.project, "rev-parse", "HEAD");
    await createMission();
    await definePlan();
    await approveAll();
    await runToSealed();
    await verify();
    await click("reject");
    expect(q("confirm-reject")).not.toBeNull();
    expect(text("state-chip")).toBe("evidence_ready"); // nothing happened yet
    await click("confirm-reject");
    await waitFor("rejected", () => text("state-chip") === "rejected");
    expect(text("decision")).toContain("Rejected");
    expect(text("evidence-result")).toBe("PASS"); // evidence survives rejection
    expect(gitIn(fx.project, "rev-parse", "HEAD")).toBe(head);
    expect(fs.existsSync(path.join(fx.project, "hello.txt"))).toBe(false);
    expect(text("companion-headline")).toBe("Rejected.");
  });

  it("failed verification blocks accept and Tako does not call it a success", async () => {
    start();
    await openProject();
    await createMission({ checks: "exit 1" });
    await definePlan();
    await approveAll();
    await runToSealed();
    await verify();
    expect(text("evidence-result")).toBe("FAIL");
    expect(text("failed-checks")).toContain("1");
    expect(q("state-chip")!.className).toContain("not-ok"); // not shown as a green success state
    expect(q("decision-card")!.getAttribute("data-active")).toBe("true"); // Reject is where the eye is
    expect(q("accept")).toBeNull();
    expect(text("accept-blocked")).toMatch(/blocked/);
    expect(text("companion-headline")).toBe("Verification failed.");
    expect(q("companion")!.dataset.pose).toBe("alert");
    expect(text("companion-detail") + text("companion-headline")).not.toMatch(/passed|success/i);
  });

  it("a verification that modifies the subject is shown as invalid evidence", async () => {
    start();
    await openProject();
    await createMission({ checks: "grep -q hello hello.txt\necho x > extra.txt" });
    await definePlan();
    await approveAll();
    await runToSealed();
    await verify();
    expect(text("evidence-result")).toBe("PASS"); // checks exited 0, but...
    expect(text("evidence-validity")).toBe("EVIDENCE INVALID");
    expect(text("integrity")).toContain("subject_modified");
    expect(q("accept")).toBeNull();
    expect(text("companion-headline")).toMatch(/invalid/i);
  });

  it("running without granting permissions is refused and nothing starts", async () => {
    start();
    await openProject();
    await createMission();
    await definePlan();
    await click("approve"); // zero grants
    await waitFor("approved", () => text("state-chip") === "approved");
    expect(text("approval-line")).toContain("granted: nothing");
    await click("start-run");
    await waitFor("error", () => !!q("error-banner") || !!q("job-error-banner"));
    expect((text("error-banner") + text("job-error-banner"))).toMatch(/unauthorized/);
    expect(text("state-chip")).toBe("approved");
    expect(fs.existsSync(path.join(fx.data, "missions"))).toBe(true);
  });

  it("cancel stops a long run and the partial work can be rejected", async () => {
    start();
    await openProject();
    await createMission();
    await definePlan("echo started > partial.txt\nsleep 300");
    await approveAll();
    await waitFor("start button", () => !!q("start-run"));
    await click("start-run");
    await waitFor("executing", () => text("state-chip") === "executing" && !!q("cancel"));
    expect(q("companion")!.dataset.pose).toBe("working");
    expect(q("start-run")).toBeNull(); // nothing else is offered while running
    expect(q("accept")).toBeNull();
    await click("cancel");
    await waitFor("cancelled", () => text("state-chip") === "cancelled");
    expect(text("exec-status")).toBe("cancelled");
    expect(text("state-reason")).toMatch(/^cancelled/); // not "agent made no changes"
    expect(text("diff")).toContain("+started"); // partial work stays inspectable
    expect(q("companion")!.dataset.pose).toBe("alert");
    await click("reject");
    await click("confirm-reject");
    await waitFor("rejected", () => text("state-chip") === "rejected");
  });

  it("prevents duplicate submissions from rapid clicks", async () => {
    start();
    await openProject();
    await createMission();
    await definePlan("sleep 2\necho a > a.txt");
    await approveAll();
    await waitFor("start button", () => !!q("start-run"));
    const btn = must<HTMLButtonElement>("start-run");
    btn.click();
    btn.click();
    btn.click();
    await waitFor("executing", () => text("state-chip") === "executing");
    expect(be.count("start_execution")).toBe(1);
    await click("cancel");
    await waitFor("cancelled", () => text("state-chip") === "cancelled");
  });

  it("surfaces an unsafe repository and offers no way forward", async () => {
    gitIn(fx.project, "config", "--local", "filter.evil.clean", "touch SENTINEL");
    start();
    await waitFor("empty", () => !!q("empty-state"));
    await type("project-path", fx.project);
    await click("open-project");
    await waitFor("error", () => !!q("error-banner"));
    expect(text("error-code")).toBe("unsafe_repository");
    expect(text("error-banner")).toContain("local:filter.evil.clean");
    expect(q("goal-form")).toBeNull();
    expect(text("companion-detail")).toMatch(/refuses to touch/);
    expect(q("companion")!.dataset.pose).toBe("alert");
    expect(fs.existsSync(path.join(fx.project, "SENTINEL"))).toBe(false);
  });

  it("shows AI agent execution as disabled and the engine refuses it", async () => {
    start();
    await openProject();
    await createMission();
    await waitFor("plan form", () => !!q("plan-form"));
    const radio = must<HTMLInputElement>("provider-radio");
    expect(radio.disabled).toBe(true);
    expect(text("status-bar")).toContain("AI agent execution: disabled");
    await definePlan();
    await approveAll();
    const id = JSON.parse(await (async () => JSON.stringify((await be.call("list_missions", { project_path: fx.project }))[0]!.id))());
    await expect(be.call("start_execution", { mission_id: id, kind: "provider" })).rejects.toMatchObject({ code: "isolation_required" });
    expect(text("state-chip")).toBe("approved");
    expect(fs.existsSync(path.join(fx.data, "missions", id, "workspace"))).toBe(false);
  });

  it("changing the plan voids approval and the UI shows it", async () => {
    start();
    await openProject();
    await createMission();
    await definePlan();
    await approveAll();
    expect(text("state-chip")).toBe("approved");
    await waitFor("plan form", () => !!q("plan-form"));
    await type("script", "echo changed > hello.txt");
    await click("define-plan");
    await waitFor("plan_ready", () => text("state-chip") === "plan_ready");
    expect(q("approval-line")).toBeNull();
    expect(q("start-run")).toBeNull();
    for (const p of BOTH) expect(must<HTMLInputElement>(p).checked).toBe(false); // grants reset for the new plan
  });

  it("recovers missions interrupted by a crash at startup and says so", async () => {
    // First app instance starts a long run, then the whole engine process is killed.
    const crashed = new StdioBackend(fx.data);
    const pv = await crashed.call("open_project", { path: fx.project });
    let v = await crashed.call("create_mission", { project_path: pv.project.path, goal: "g", criteria: ["c"], checks: ["true"] });
    v = await crashed.call("define_plan", { mission_id: v.mission.id, script: "echo a > a.txt; sleep 300" });
    v = await crashed.call("approve_plan", { mission_id: v.mission.id, version: 1, plan_hash: v.plan!.hash, granted: ["fs_write_workspace", "exec_command:sh"] });
    await crashed.call("start_execution", { mission_id: v.mission.id, timeout_s: 600 });
    await waitFor("run started", () => false || true);
    for (let i = 0; i < 100; i++) {
      v = await crashed.call("get_mission", { mission_id: v.mission.id });
      if (v.mission.state === "executing" && (v.mission as unknown as { active_run?: { pid: number | null } }).active_run?.pid) break;
      await tick(50);
    }
    expect(v.mission.state).toBe("executing");
    crashed.crash();
    await tick(200);

    // Second instance: the bridge recovers on open; the UI reports it.
    be.close();
    be = new StdioBackend(fx.data);
    start();
    await waitFor("recovered banner", () => !!q("recovered-banner"));
    expect(text("recovered-banner")).toContain("executing → interrupted");
    expect(text("companion-headline")).toMatch(/recovered 1/i);
    await type("project-path", fx.project);
    await click("open-project");
    await waitFor("missions list", () => document.querySelectorAll(".missions li").length === 1);
    expect(document.querySelector(".missions")!.textContent).toContain("interrupted");
    // opening the project resumes the unfinished mission by itself
    await waitFor("interrupted mission", () => text("state-chip") === "interrupted" && !!q("reject"));
    expect(q("start-run")).toBeNull();
    expect(q("accept")).toBeNull();
    expect(q("companion")!.dataset.pose).toBe("alert");
    await click("reject");
    await click("confirm-reject");
    await waitFor("rejected", () => text("state-chip") === "rejected");
  });

  it("shows busy when another instance holds the mission", async () => {
    // A second engine process holds a running mission; this UI instance must not interfere.
    const other = new StdioBackend(fx.data);
    const pv = await other.call("open_project", { path: fx.project });
    let v = await other.call("create_mission", { project_path: pv.project.path, goal: "busy one", criteria: ["c"], checks: ["true"] });
    v = await other.call("define_plan", { mission_id: v.mission.id, script: "sleep 300" });
    v = await other.call("approve_plan", { mission_id: v.mission.id, version: 1, plan_hash: v.plan!.hash, granted: ["fs_write_workspace", "exec_command:sh"] });
    await other.call("start_execution", { mission_id: v.mission.id, timeout_s: 600 });
    for (let i = 0; i < 100; i++) {
      v = await other.call("get_mission", { mission_id: v.mission.id });
      if (v.mission.state === "executing") break;
      await tick(50);
    }
    try {
      start();
      await openProject();
      await waitFor("mission listed", () => document.querySelectorAll(".missions li").length === 1);
      await waitFor("executing view", () => text("state-chip") === "executing" && !!q("recover"));
      expect(q("recover")).not.toBeNull(); // no worker attached to this window
      await click("recover");
      await waitFor("busy error", () => !!q("error-banner"));
      expect(text("error-code")).toBe("busy");
      expect(text("companion-headline")).toMatch(/busy/i);
      expect(text("state-chip")).toBe("executing"); // untouched
    } finally {
      await other.call("cancel", { mission_id: v.mission.id }).catch(() => undefined);
      await tick(1500);
      other.close();
    }
  });
});

describe("TAKO-006 evidence integrity", () => {
  it("labels backend completion unverified and clears the previous mission when backend identity is missing", async () => {
    const local = await be.call("create_mission", { project_path: fx.project, goal: "Local mission", criteria: ["c"], checks: ["true"] });
    // Discovery is a fixture; missing backend identity must fail before any remote or local lookup.
    const backend: import("../src/backend").Backend = {
      async call(cmd, args) {
        if (cmd === "open_project") {
          const pv = await be.call("open_project", args as { path: string });
          return { ...pv, missions: [...pv.missions, { id: "PROG-unavailable", goal: "Backend goal", state: null, reported_backend_state: "completed", created_at: 0 }] } as never;
        }
        return be.call(cmd, args);
      },
    };
    ui = mount(<App backend={backend} pollMs={80} />);
    await openProject();
    await waitFor("resumed local mission", () => text("mission-goal") === local.mission.goal);
    expect(text("mission-PROG-unavailable")).toContain("unverified");
    expect(text("mission-PROG-unavailable")).toContain("evidence unavailable");
    expect(text("mission-PROG-unavailable")).not.toContain("accepted");
    await click("mission-PROG-unavailable");
    await waitFor("lookup failure", () => text("error-code") === "evidence_unavailable");
    expect(q("mission-card")).toBeNull();
    expect(q("exec-status")).toBeNull();
    expect(q("decision")).toBeNull();
    expect(text("companion-headline")).not.toMatch(/Accepted|passed/);
  });

  it("shows local path errors without requiring a gateway and keeps local work accessible", async () => {
    start();
    await waitFor("startup", () => !!q("empty-state"));
    await type("project-path", "fixture-gateway-unavailable");
    await click("open-project");
    await waitFor("local path error", () => !!q("error-code"));
    expect(q("mission-card")).toBeNull();
    await openProject();
    expect(q("error-banner")).toBeNull();
    await createMission();
    expect(text("state-chip")).toBe("created");
  });
});

describe("TAKO-007B read-only backend evidence", () => {
  it("retrieves remote evidence by project and mission identity without a local fallback", async () => {
    const requests: unknown[] = [];
    const backend: import("../src/backend").Backend = {
      async call(cmd, args) {
        if (cmd === "open_project") {
          const pv = await be.call("open_project", args as { path: string });
          return { ...pv, missions: [{ id: "remote-mission", goal: "Remote goal", state: null,
            reported_backend_state: "completed", backend_project_id: "remote-project", created_at: 0 }] } as never;
        }
        if (cmd === "backend_mission_evidence") {
          requests.push(args);
          return {
            identity: { backend_id: "t-tako-adapter", project_id: "remote-project", mission_id: "remote-mission" },
            assessment: { outcome: "unverified", isolation: "unverified", remote_acceptance_enabled: false,
              l1_badge_enabled: false, provider_execution_enabled: false, reason: "snapshot_not_authenticated" },
            message: "Authoritative evidence unavailable: the export has no independently established authenticity.",
          } as never;
        }
        return be.call(cmd, args);
      },
    };
    ui = mount(<App backend={backend} pollMs={80} />);
    await openProject();
    const lookups = be.count("get_mission");
    await click("mission-remote-mission");
    await waitFor("unverified retrieval", () => text("backend-evidence-status") === "unverified");
    expect(requests).toEqual([{ project_id: "remote-project", mission_id: "remote-mission" }]);
    expect(be.count("get_mission")).toBe(lookups);
    expect(text("backend-evidence")).toContain("no independently established authenticity");
    expect(q("mission-card")).toBeNull();
    expect(q("accept")).toBeNull();
    expect(q("exec-status")).toBeNull();
    expect(q("decision")).toBeNull();
  });

  it("refuses disabled integration without displaying completion as verification", async () => {
    const backend: import("../src/backend").Backend = {
      async call(cmd, args) {
        if (cmd === "open_project") {
          const pv = await be.call("open_project", args as { path: string });
          return { ...pv, missions: [{ id: "remote-offline", goal: "Remote goal", state: null,
            backend_project_id: "remote-project", reported_backend_state: "completed", created_at: 0 }] } as never;
        }
        return be.call(cmd, args);
      },
    };
    ui = mount(<App backend={backend} pollMs={80} />);
    await openProject();
    await click("mission-remote-offline");
    await waitFor("adapter disabled", () => text("error-code") === "integration_disabled");
    expect(q("backend-evidence")).toBeNull();
    expect(q("mission-card")).toBeNull();
    expect(q("accept")).toBeNull();
    expect(text("companion-headline")).not.toMatch(/Accepted|passed/);
  });
});
