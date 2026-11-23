import fs from "node:fs";
import path from "node:path";
import { once } from "node:events";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DesktopApp } from "../src/DesktopApp";
import { type Backend } from "../src/backend";
import type { CommandName } from "../src/types";
import { StdioBackend } from "./helpers/stdioBackend";
import { makeFixture, type Fixture } from "./helpers/fixtures";
import { click, mount, q, waitFor } from "./helpers/dom";

const renderControl = vi.hoisted(() => ({ fail: false }));
vi.mock("../src/SotaWorkspacePanel", async importOriginal => {
  const real = await importOriginal<typeof import("../src/SotaWorkspacePanel")>();
  return {
    ...real,
    SotaWorkspacePanel: (props: import("../src/SotaWorkspacePanel").SotaWorkspacePanelProps) => {
      if (renderControl.fail) throw new Error("Intentional Research render failure");
      return real.SotaWorkspacePanel(props);
    },
  };
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
const marker = () => path.join(fx.data, "onboarding-v1.json");

describe("research-first desktop entry with the real native onboarding bridge", () => {
  it("fresh user starts at Welcome, enters Research environment directly, and never sees legacy Engineering UI", async () => {
    start();
    await waitFor("Welcome", () => !!q("welcome-heading"));
    await click("welcome-continue");
    await waitFor("Workspace setup", () => !!q("workspace-enter-directly"));
    await click("workspace-enter-directly");
    await waitFor("Research mounted", () => !!q("sota-workspace-shell"));
    await waitFor("Onboarding persisted", () => fs.existsSync(marker()));

    expect(q("research-mate")).not.toBeNull();
    expect(q("sota-settings-disclosure")).not.toBeNull();
    expect(document.body.textContent).not.toContain("Open a Git project");
    expect(document.body.textContent).not.toContain("Enable optional T integration");
  });

  it("returning user restores directly into Research environment", async () => {
    start();
    await waitFor("Welcome", () => !!q("welcome-heading"));
    await click("welcome-skip");
    await waitFor("Direct enter", () => !!q("workspace-enter-directly"));
    await click("workspace-enter-directly");
    await waitFor("Onboarding persisted", () => fs.existsSync(marker()));

    ui?.unmount();
    ui = null;
    start(wrapper({}));
    await waitFor("Restored directly to research", () => !!q("sota-workspace-shell"));
    expect(q("welcome-heading")).toBeNull();
  });

  it("recovers to workspace setup if Research panel fails to render", async () => {
    renderControl.fail = true;
    start();
    await waitFor("Welcome", () => !!q("welcome-heading"));
    await click("welcome-skip");
    await waitFor("Direct enter", () => !!q("workspace-enter-directly"));
    await click("workspace-enter-directly");
    await waitFor("Fallback error", () => !!q("onboarding-status") && document.body.textContent!.includes("could not be displayed"));
  });
});
