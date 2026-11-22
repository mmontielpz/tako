import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { App } from "../src/App";
import type { Backend } from "../src/backend";
import type { BackendStatus, IntegrationView, ProjectView, BackendEvidenceView } from "../src/types";
import { StdioBackend } from "./helpers/stdioBackend";
import { makeFixture, type Fixture } from "./helpers/fixtures";
import { click, mount, must, q, text, tick, type, waitFor } from "./helpers/dom";

let fx: Fixture, be: StdioBackend;
let ui: ReturnType<typeof mount> | null = null;
beforeEach(() => { fx = makeFixture(); be = new StdioBackend(fx.data); });
afterEach(() => { ui?.unmount(); ui = null; be.close(); fx.cleanup(); });
const state = (enabled: boolean): IntegrationView => ({ mode: enabled ? "optional_backend" : "standalone",
  backend: enabled ? "t" : null, availability: enabled ? "not_checked" : "disabled", read_only: true, provider_execution: "disabled" });
const status = (online: boolean): BackendStatus => ({ online, base_url: "http://fixture.invalid",
  status: online ? "fixture reachable" : "OFFLINE", runtime_status: "UNKNOWN", g12_state: "UNKNOWN", operational_mode: "read_only",
  projects: online ? [{ project_id: "remote-project", name: "Optional project", path: fx.project, exists: true, is_git: true,
    branch: "fixture", head: "f".repeat(40), dirty: false, allowed_root: "/fixture", isolation_profile: "unverified" }] : [],
  limits: null, approvals: [], workstations: [] });
async function openLocal() {
  await type("project-path", fx.project); await click("open-project");
  await waitFor("local project", () => text("project-name") === fx.project, 3000);
}
async function createLocal() {
  await type("goal", "Local standalone mission"); await click("create-mission");
  await waitFor("local mission", () => text("state-chip") === "created", 3000);
}
function selectRemoteProject() {
  const select = must<HTMLSelectElement>("backend-projects-select"); select.value = "remote-project";
  select.dispatchEvent(new Event("change", { bubbles: true }));
}

// Optional-service timing is controlled at the Backend seam. All local calls still
// use the real Rust engine; Rust standalone.rs separately qualifies actual TCP concurrency.
describe("TAKO-011 standalone core and optional integration", () => {
  it("starts without an integration probe or configuration and plans locally", async () => {
    ui = mount(<App backend={be} />);
    await waitFor("standalone configuration", () => be.count("integration_status") === 1, 3000);
    expect(text("core-mode")).toContain("standalone");
    expect(text("backend-status")).toContain("disabled (optional)");
    expect(be.count("backend_status")).toBe(0);
    expect(be.count("configure_integration")).toBe(0);
    await openLocal(); await createLocal(); await click("define-plan");
    await waitFor("local plan", () => !!q("plan-card"), 3000);
    expect(must<HTMLInputElement>("perm-fs_write_workspace").checked).toBe(false);
    expect(must<HTMLInputElement>("perm-exec_command:sh").checked).toBe(false);
    expect(must<HTMLInputElement>("provider-radio").disabled).toBe(true);
    expect(be.count("backend_status")).toBe(0);
  });

  it("a pending optional probe does not lock local project selection or planning", async () => {
    let finish!: (s: BackendStatus) => void;
    const backend: Backend = { async call(cmd, args) {
      if (cmd === "configure_integration") return state((args as { enabled: boolean }).enabled) as never;
      if (cmd === "backend_status") return await new Promise<BackendStatus>((resolve) => { finish = resolve; }) as never;
      return be.call(cmd, args);
    } };
    ui = mount(<App backend={backend} />); await click("enable-integration");
    await waitFor("probe pending", () => !!q("integration-progress") && !!finish, 3000);
    expect(must<HTMLButtonElement>("open-project").disabled).toBe(true); // only because path is empty
    await openLocal(); await createLocal(); await click("define-plan");
    await waitFor("plan during optional outage", () => !!q("plan-card"), 3000);
    expect(q("integration-progress")).not.toBeNull();
    finish(status(false));
    await waitFor("unavailable status", () => text("backend-status").includes("unavailable"), 3000);
    expect(text("state-chip")).toBe("plan_ready");
  });

  it("availability changes clear stale backend lists and preserve local mission state", async () => {
    let online = true;
    const backend: Backend = { async call(cmd, args) {
      if (cmd === "configure_integration") return state((args as { enabled: boolean }).enabled) as never;
      if (cmd === "backend_status") return status(online) as never;
      return be.call(cmd, args);
    } };
    ui = mount(<App backend={backend} />); await openLocal(); await createLocal();
    await click("enable-integration");
    await waitFor("available", () => text("backend-status").includes("available"), 3000);
    expect(q("backend-projects-select")).not.toBeNull();
    expect(q("t-readonly-pill")).toBeNull(); // Registry data cannot reclassify a local project.
    online = false; await click("refresh-integration");
    await waitFor("unavailable", () => text("backend-status").includes("unavailable"), 3000);
    expect(q("backend-projects-select")).toBeNull(); expect(text("state-chip")).toBe("created");
    online = true; await click("refresh-integration");
    await waitFor("available again", () => !!q("backend-projects-select"), 3000);
    await click("disable-integration");
    await waitFor("disabled", () => text("backend-status").includes("disabled"), 3000);
    expect(q("backend-projects-select")).toBeNull(); expect(text("state-chip")).toBe("created");
  });

  it("a response arriving after disable cannot restore backend state", async () => {
    let finish!: (s: BackendStatus) => void;
    const backend: Backend = { async call(cmd, args) {
      if (cmd === "configure_integration") return state((args as { enabled: boolean }).enabled) as never;
      if (cmd === "backend_status") return await new Promise<BackendStatus>((resolve) => { finish = resolve; }) as never;
      return be.call(cmd, args);
    } };
    ui = mount(<App backend={backend} />); await openLocal(); await click("enable-integration");
    await waitFor("pending optional response", () => !!finish && !!q("disable-integration"), 3000);
    await click("disable-integration");
    await waitFor("standalone again", () => text("backend-status").includes("disabled"), 3000);
    finish(status(true)); await tick(40);
    expect(q("backend-projects-select")).toBeNull(); expect(text("backend-status")).toContain("disabled");
    expect(text("project-name")).toBe(fx.project);
  });

  it("pending backend project selection cannot block or replace a newer local project", async () => {
    let finish!: (p: ProjectView) => void;
    const local = await be.call("open_project", { path: fx.project });
    const backend: Backend = { async call(cmd, args) {
      if (cmd === "configure_integration") return state((args as { enabled: boolean }).enabled) as never;
      if (cmd === "backend_status") return status(true) as never;
      if (cmd === "open_backend_project") return await new Promise<ProjectView>((resolve) => { finish = resolve; }) as never;
      return be.call(cmd, args);
    } };
    ui = mount(<App backend={backend} />); await click("enable-integration");
    await waitFor("backend registry", () => !!q("backend-projects-select"), 3000);
    selectRemoteProject(); await waitFor("remote selection pending", () => !!finish, 3000);
    await openLocal();
    finish({ ...local, source: "t_backend", backend_project_id: "remote-project", project: { ...local.project, path: "/remote/fixture" } });
    await tick(40);
    expect(text("project-name")).toBe(fx.project); expect(q("t-readonly-pill")).toBeNull();
    await createLocal();
  });

  it("pending backend evidence cannot block local navigation or appear in a newer project", async () => {
    const local = await be.call("open_project", { path: fx.project });
    let finish!: (v: BackendEvidenceView) => void;
    const backend: Backend = { async call(cmd, args) {
      if (cmd === "configure_integration") return state((args as { enabled: boolean }).enabled) as never;
      if (cmd === "backend_status") return status(true) as never;
      if (cmd === "open_backend_project") return { ...local, source: "t_backend", backend_project_id: "remote-project",
        missions: [{ id: "remote-mission", goal: "Remote goal", state: null, backend_project_id: "remote-project", created_at: 0 }] } as never;
      if (cmd === "backend_mission_evidence") return await new Promise<BackendEvidenceView>((resolve) => { finish = resolve; }) as never;
      return be.call(cmd, args);
    } };
    ui = mount(<App backend={backend} />); await click("enable-integration");
    await waitFor("registry", () => !!q("backend-projects-select"), 3000); selectRemoteProject();
    await waitFor("remote view", () => !!q("t-readonly-pill"), 3000); await click("mission-remote-mission");
    await waitFor("remote evidence pending", () => !!finish, 3000);
    await openLocal();
    finish({ identity: { backend_id: "fixture", project_id: "remote-project", mission_id: "remote-mission" },
      assessment: { outcome: "unverified", isolation: "unverified", remote_acceptance_enabled: false,
        l1_badge_enabled: false, provider_execution_enabled: false, reason: "snapshot_not_authenticated" }, message: "Unsigned fixture" }); await tick(40);
    expect(q("backend-evidence")).toBeNull(); expect(q("t-readonly-pill")).toBeNull(); await createLocal();
  });
});
