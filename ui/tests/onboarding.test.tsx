import { useState } from "preact/hooks";
import { afterEach, describe, expect, it } from "vitest";
import { WelcomeWizard } from "../src/WelcomeWizard";
import { click, mount, must, q, tick, type, waitFor } from "./helpers/dom";

let ui: ReturnType<typeof mount> | null = null;
afterEach(() => { ui?.unmount(); ui = null; });

function Harness() {
  const [screen, setScreen] = useState<"welcome" | "choose">("welcome");
  const [wsPath, setWsPath] = useState("");
  const [topic, setTopic] = useState("State of the Art Survey");
  const [objective, setObjective] = useState("Identify foundational works and evidence");
  return (
    <WelcomeWizard
      screen={screen}
      pending={false}
      restoring={false}
      error={null}
      workspacePath={wsPath}
      topic={topic}
      objective={objective}
      onWorkspacePathChange={setWsPath}
      onTopicChange={setTopic}
      onObjectiveChange={setObjective}
      onContinue={() => setScreen("choose")}
      onSkip={() => setScreen("choose")}
      onBack={() => setScreen("welcome")}
      onCreateWorkspace={() => {}}
      onOpenWorkspace={() => {}}
      onEnterDirectly={() => {}}
      onRetryRead={() => {}}
    />
  );
}

describe("research-first WelcomeWizard accessibility and onboarding", () => {
  it("shows clean temporary text identity and no Engineering vs Research selection", async () => {
    ui = mount(<Harness />); await tick();
    expect(document.body.textContent).toContain("Agentic Research Development Environment");
    expect(document.body.textContent).not.toContain("TEMPORARY PLACEHOLDER");
    expect(document.body.textContent).not.toContain("Research unavailable");
    expect(document.querySelector("input")).toBeNull();
    expect(document.activeElement).toBe(q("welcome-heading"));
  });

  it("discloses offline guidance only on request and Escape returns focus to Setup options", async () => {
    ui = mount(<Harness />); await click("setup-options");
    expect(q("setup-options")!.getAttribute("aria-expanded")).toBe("true");
    expect(q("setup-guidance-copy")).toBeNull(); await click("setup-guidance");
    expect(q("setup-guidance-copy")!.textContent).toContain("isolated TAKO Python runtime");
    q("setup-guidance")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick(); expect(q("setup-options")!.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(q("setup-options"));
  });

  it("Step 2 lets user configure research workspace without Git or API keys", async () => {
    ui = mount(<Harness />); await click("welcome-skip");
    await waitFor("workspace step", () => !!q("onboarding-ws-path"));
    expect(must<HTMLButtonElement>("workspace-create-enter").disabled).toBe(true);
    await type("onboarding-ws-path", "/tmp/my_research_ws");
    expect(must<HTMLButtonElement>("workspace-create-enter").disabled).toBe(false);
    expect(must<HTMLButtonElement>("workspace-open-existing").disabled).toBe(false);
    await click("workspace-back");
    await waitFor("welcome focus", () => document.activeElement === q("welcome-heading"));
  });
});
