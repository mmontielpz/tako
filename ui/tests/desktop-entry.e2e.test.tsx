import { once } from "node:events";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DesktopApp } from "../src/DesktopApp";
import { type Backend } from "../src/backend";
import { completeOnboarding, openEngineeringWorkspace } from "../src/onboarding";
import type { CommandName } from "../src/types";
import { StdioBackend } from "./helpers/stdioBackend";
import { makeFixture, type Fixture } from "./helpers/fixtures";
import { click, mount, q, waitFor } from "./helpers/dom";

const renderControl = vi.hoisted(() => ({ fail: false }));
vi.mock("../src/SotaWorkspacePanel", async (importOriginal) => {
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
beforeEach(() => {
  window.localStorage.clear();
  fx = makeFixture();
  be = new StdioBackend(fx.data);
  renderControl.fail = false;
});
async function stop() {
  if (be.child.exitCode === null && be.child.signalCode === null) {
    const closed = once(be.child, "close");
    be.close();
    await closed;
  }
}
afterEach(async () => {
  ui?.unmount();
  ui = null;
  await stop();
  fx.cleanup();
  vi.restoreAllMocks();
});
function wrapper(overrides: Partial<Record<CommandName, (args: unknown) => Promise<unknown>>>): Backend {
  return {
    async call(cmd, args) {
      return overrides[cmd] ? ((await overrides[cmd]!(args)) as never) : be.call(cmd, args);
    },
  };
}
function start(backend: Backend = be, timeout = 5000) {
  ui = mount(<DesktopApp backend={backend} readTimeoutMs={timeout} />);
}

describe("research-first desktop onboarding and returning-user state machine", () => {
  it("FIRST_RUN -> ACTIVE_RESEARCH -> Return Home (RETURNING_USER) without calling openEngineeringWorkspace", async () => {
    start();
    await waitFor("Welcome", () => !!q("welcome-heading"));
    await click("welcome-skip");
    await waitFor("Research mounted", () => !!q("sota-workspace-shell"));

    expect(be.calls).not.toContain("onboarding_open_workspace");
    expect(q("sota-return-home")).not.toBeNull();
    expect(q("research-mate")).not.toBeNull();
    expect(document.body.textContent).not.toContain("Open a Git project");

    await click("sota-return-home");
    await waitFor("Research Home", () => !!q("research-home-heading"));

    // Returning user relaunch lands on Research Home
    ui?.unmount();
    ui = null;
    start(wrapper({}));
    await waitFor("Returning Research Home", () => !!q("research-home-heading"));
    expect(q("welcome-heading")).toBeNull();
  });

  it("preserves backward compatibility when onboarding-v1.json is already completed", async () => {
    await openEngineeringWorkspace(be);
    await completeOnboarding(be);
    start();
    await waitFor("Research Home from legacy completion", () => !!q("research-home-heading"));
  });

  it("recovers to FIRST_RUN or RETURNING_USER if Research panel fails to render", async () => {
    renderControl.fail = true;
    start();
    await waitFor("Welcome", () => !!q("welcome-heading"));
    await click("welcome-skip");
    await waitFor(
      "Fallback error",
      () => !!q("onboarding-status") && document.body.textContent!.includes("could not be displayed"),
    );
  });
});
