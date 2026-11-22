import fs from "node:fs";
import path from "node:path";
import { once } from "node:events";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DesktopApp } from "../src/DesktopApp";
import { BridgeError, type Backend } from "../src/backend";
import type { CommandName } from "../src/types";
import { StdioBackend } from "./helpers/stdioBackend";
import { makeFixture, type Fixture } from "./helpers/fixtures";
import { click, mount, must, q, text, tick, type, waitFor } from "./helpers/dom";

// Only the negative render-failure test intercepts rendering; normal tests use the
// existing App and actual native engine. This is never a production injection seam.
const renderControl = vi.hoisted(() => ({ fail: false }));
vi.mock("../src/App", async importOriginal => {
  const real = await importOriginal<typeof import("../src/App")>();
  return { ...real, App: (props: import("../src/App").AppProps) => {
    if (renderControl.fail) throw new Error("Intentional Engineering render failure");
    return real.App(props);
  } };
});

let fx: Fixture, be: StdioBackend;
let ui: ReturnType<typeof mount> | null = null;
beforeEach(() => { fx = makeFixture(); be = new StdioBackend(fx.data); renderControl.fail = false; });
async function stop() {
  if (be.child.exitCode === null && be.child.signalCode === null) {
    const closed = once(be.child, "close"); be.close(); await closed;
  }
}
afterEach(async () => { ui?.unmount(); ui = null; await stop(); fx.cleanup(); vi.restoreAllMocks(); });
function wrapper(overrides: Partial<Record<CommandName, (args: unknown) => Promise<unknown>>>): Backend {
  return { async call(cmd, args) { return overrides[cmd] ? await overrides[cmd]!(args) as never : be.call(cmd, args); } };
}
function start(backend: Backend = be, timeout = 5000) { ui = mount(<DesktopApp backend={backend} readTimeoutMs={timeout} />); }
async function welcome() { await waitFor("Welcome", () => !!q("welcome-heading")); }
async function choose() { await welcome(); await click("welcome-continue"); }
async function open() { await click("workspace-engineering"); await click("workspace-open"); }
const marker = () => path.join(fx.data, "onboarding-v1.json");
async function persisted() { await waitFor("native completion", () => fs.existsSync(marker()) && !!q("empty-state")); }

describe("production desktop entry with the real native onboarding bridge", () => {
  it("starts Welcome locally; Continue leaves Research disabled and Engineering unselected", async () => {
    start(); await choose();
    expect(must<HTMLInputElement>("workspace-research").disabled).toBe(true);
    expect(must<HTMLInputElement>("workspace-engineering").checked).toBe(false);
    expect(must<HTMLButtonElement>("workspace-open").disabled).toBe(true);
    expect(document.activeElement).toBe(q("workspace-heading"));
    expect(new Set(be.calls)).toEqual(new Set(["startup_report", "onboarding_state"]));
    expect(fs.existsSync(marker())).toBe(false);
  });

  it("mounts actual Engineering before completion and never automatically opens a project or provider", async () => {
    let sawCommittedApp = false;
    start(wrapper({ onboarding_complete: async args => {
      sawCommittedApp = !!document.querySelector(".shell") && !!q("empty-state");
      return be.call("onboarding_complete", args as { workspace: "engineering" });
    } }));
    await choose(); await open(); await persisted();
    expect(sawCommittedApp).toBe(true);
    expect(JSON.parse(fs.readFileSync(marker(), "utf8"))).toEqual({ schema_version: 1, intro_version: 1, completed: true, last_workspace: "engineering" });
    expect(be.calls.indexOf("onboarding_open_workspace")).toBeLessThan(be.calls.indexOf("onboarding_complete"));
    for (const command of ["open_project", "create_mission", "proposal_start", "workspace_start", "configure_integration", "backend_status", "start_execution", "context_preview"]) expect(be.count(command)).toBe(0);
    expect(text("isolation-badge")).toContain("not a security sandbox");
  });

  it("Skip introduction still requires selection; Back preserves temporary choice without saving", async () => {
    start(); await welcome(); await click("welcome-skip");
    expect(must<HTMLButtonElement>("workspace-open").disabled).toBe(true);
    await click("workspace-engineering"); await click("workspace-back");
    expect(q("welcome-heading")).not.toBeNull(); await click("welcome-continue");
    expect(must<HTMLInputElement>("workspace-engineering").checked).toBe(true);
    expect(fs.existsSync(marker())).toBe(false); expect(be.count("onboarding_complete")).toBe(0);
  });

  it("an interrupted chooser restarts at Welcome with no remembered selection", async () => {
    start(); await choose(); await click("workspace-engineering");
    ui!.unmount(); ui = null; await stop(); be = new StdioBackend(fx.data);
    start(); await welcome(); await click("welcome-continue");
    expect(must<HTMLInputElement>("workspace-engineering").checked).toBe(false);
    expect(fs.existsSync(marker())).toBe(false);
  });

  it("returning users bypass the wizard after an actual bridge process restart without rewriting completion", async () => {
    start(); await choose(); await open(); await persisted();
    const before = fs.readFileSync(marker());
    ui!.unmount(); ui = null; const pid = be.child.pid; await stop(); be = new StdioBackend(fx.data);
    start(); await waitFor("returning Engineering", () => !!q("empty-state"));
    expect(be.child.pid).not.toBe(pid); expect(q("welcome-heading")).toBeNull();
    expect(be.count("onboarding_open_workspace")).toBe(1); expect(be.count("onboarding_complete")).toBe(0);
    expect(be.count("open_project")).toBe(0); expect(fs.readFileSync(marker())).toEqual(before);
  });

  it("Back invalidates a delayed native open; late success never mounts or completes", async () => {
    let finish: (() => Promise<void>) | undefined;
    start(wrapper({ onboarding_open_workspace: args => new Promise(resolve => {
      finish = async () => resolve(await be.call("onboarding_open_workspace", args as { workspace: "engineering" }));
    }) }));
    await choose(); await open(); await waitFor("pending open", () => !!finish);
    expect(must<HTMLButtonElement>("workspace-open").disabled).toBe(true);
    await click("workspace-back"); await finish!(); await tick(180);
    expect(q("welcome-heading")).not.toBeNull(); expect(q("empty-state")).toBeNull();
    expect(fs.existsSync(marker())).toBe(false); expect(be.count("onboarding_complete")).toBe(0);
  });

  it("open failure retains selection and needs deliberate retry", async () => {
    let attempts = 0;
    start(wrapper({ onboarding_open_workspace: async args => {
      if (++attempts === 1) throw new BridgeError({ code: "onboarding_workspace_open_failed", message: "fixture", details: [] });
      return be.call("onboarding_open_workspace", args as { workspace: "engineering" });
    } }));
    await choose(); await open(); await waitFor("open failure", () => text("onboarding-status").includes("could not open"));
    expect(document.activeElement).toBe(q("onboarding-status"));
    expect(must<HTMLInputElement>("workspace-engineering").checked).toBe(true);
    expect(be.count("onboarding_complete")).toBe(0);
    await click("workspace-open"); await persisted(); expect(attempts).toBe(2);
  });

  it("corrupt metadata stays intact while explicit Engineering entry and retry remain usable", async () => {
    await be.call("onboarding_state", undefined); const original = "{PRIVATE_FIXTURE"; fs.writeFileSync(marker(), original);
    start(); await waitFor("restore failure", () => !!q("workspace-heading"));
    expect(text("onboarding-status")).toContain("could not be restored");
    expect(must<HTMLInputElement>("workspace-engineering").checked).toBe(false);
    await open(); await waitFor("save warning", () => !!q("onboarding-save-error"));
    await type("project-path", "draft-path"); const input = q("project-path");
    await click("retry-onboarding-save"); await tick(120);
    expect(q("project-path")).toBe(input); expect(must<HTMLInputElement>("project-path").value).toBe("draft-path");
    expect(fs.readFileSync(marker(), "utf8")).toBe(original); expect(q("empty-state")).not.toBeNull();
  });

  it("a transient save failure never remounts Engineering and explicit retry writes metadata only", async () => {
    let writes = 0;
    start(wrapper({ onboarding_complete: async args => {
      if (++writes === 1) throw new BridgeError({ code: "onboarding_write_failed", message: "fixture", details: [] });
      return be.call("onboarding_complete", args as { workspace: "engineering" });
    } }));
    await choose(); await open(); await waitFor("remember warning", () => !!q("onboarding-save-error"));
    await type("project-path", fx.project); const input = q("project-path");
    await click("retry-onboarding-save"); await waitFor("save retry", () => !q("onboarding-save-error") && fs.existsSync(marker()));
    expect(q("project-path")).toBe(input); expect(must<HTMLInputElement>("project-path").value).toBe(fx.project);
    expect(writes).toBe(2); expect(be.count("open_project")).toBe(0);
  });

  it("a local read error offers an explicit retry, without a fallback workspace", async () => {
    let reads = 0;
    start(wrapper({ onboarding_state: async () => {
      if (++reads === 1) throw new BridgeError({ code: "onboarding_read_failed", message: "fixture", details: [] });
      return be.call("onboarding_state", undefined);
    } }));
    await waitFor("read failure", () => !!q("retry-onboarding-read"));
    expect(must<HTMLButtonElement>("workspace-open").disabled).toBe(true);
    await click("retry-onboarding-read"); await welcome(); expect(reads).toBe(2);
    expect(be.count("onboarding_open_workspace")).toBe(0);
  });

  it("read timeout and late completed metadata cannot override newer navigation", async () => {
    let finish: ((value: unknown) => void) | undefined;
    start(wrapper({ onboarding_state: () => new Promise(resolve => { finish = resolve; }) }), 40);
    await waitFor("read timeout", () => text("onboarding-status").includes("longer than expected"));
    await click("workspace-back"); finish!({ status: "completed", last_workspace: "engineering" }); await tick(150);
    expect(q("welcome-heading")).not.toBeNull(); expect(q("empty-state")).toBeNull();
    expect(be.count("onboarding_open_workspace")).toBe(0); expect(fs.existsSync(marker())).toBe(false);
  });

  it("a child render failure cannot be mistaken for mounted Engineering or saved completion", async () => {
    renderControl.fail = true; start(); await choose(); await open();
    await waitFor("render error", () => text("onboarding-status").includes("could not open"));
    await tick(180); expect(be.count("onboarding_complete")).toBe(0); expect(fs.existsSync(marker())).toBe(false);
    renderControl.fail = false; await click("workspace-open"); await persisted();
  });

  it("shows actual startup recovery notices without restarting work", async () => {
    start(wrapper({ startup_report: async () => ({ recovered: [{ mission_id: "fixture", from: "executing", to: "interrupted" }], busy: [], errors: [] }) }));
    await welcome(); expect(text("onboarding-recovery")).toContain("1 recovered");
    expect(be.count("recover")).toBe(0); expect(be.count("start_execution")).toBe(0);
  });

  it("lets a returning user leave a delayed restore without a late automatic navigation", async () => {
    await be.call("onboarding_open_workspace", { workspace: "engineering" });
    await be.call("onboarding_complete", { workspace: "engineering" });
    let finish: (() => Promise<void>) | undefined;
    start(wrapper({ onboarding_open_workspace: args => new Promise(resolve => {
      finish = async () => resolve(await be.call("onboarding_open_workspace", args as { workspace: "engineering" }));
    }) }));
    await waitFor("restore cancellation", () => !!q("cancel-workspace-restore"));
    await click("cancel-workspace-restore"); await finish!(); await tick(180);
    expect(q("workspace-heading")).not.toBeNull(); expect(q("empty-state")).toBeNull();
    expect(must<HTMLInputElement>("workspace-engineering").checked).toBe(false);
    expect(be.count("onboarding_complete")).toBe(1); // preparation only, no new write
  });
});
