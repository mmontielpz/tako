import { useState } from "preact/hooks";
import { afterEach, describe, expect, it } from "vitest";
import { WelcomeWizard, type OnboardingViewState } from "../src/WelcomeWizard";
import { click, mount, must, q, tick, type, waitFor } from "./helpers/dom";

let ui: ReturnType<typeof mount> | null = null;
afterEach(() => {
  ui?.unmount();
  ui = null;
});

function Harness({ initial = "FIRST_RUN" as OnboardingViewState, lastPath = null as string | null }) {
  const [view, setView] = useState<OnboardingViewState>(initial);
  const [wsPath, setWsPath] = useState("");
  const [topic, setTopic] = useState("State of the Art Survey");
  const [objective, setObjective] = useState("Identify foundational works and evidence");
  return (
    <WelcomeWizard
      view={view}
      pending={false}
      error={null}
      lastWorkspacePath={lastPath}
      lastTopic="Edge AI"
      workspacePath={wsPath}
      topic={topic}
      objective={objective}
      onWorkspacePathChange={setWsPath}
      onTopicChange={setTopic}
      onObjectiveChange={setObjective}
      onGoCreate={() => setView("CREATE_RESEARCH")}
      onGoOpen={() => setView("OPEN_RESEARCH")}
      onGoWelcome={() => setView("FIRST_RUN")}
      onGoHome={() => setView(lastPath ? "RETURNING_USER" : "FIRST_RUN")}
      onResumeLast={() => {}}
      onCreateWorkspace={() => {}}
      onOpenWorkspace={() => {}}
      onEnterDirectly={() => {}}
      onRetryRead={() => {}}
    />
  );
}

describe("research-first WelcomeWizard and Research Home", () => {
  it("FIRST_RUN shows Welcome to TAKO without Engineering/Research selection", async () => {
    ui = mount(<Harness />);
    await tick();
    expect(document.body.textContent).toContain("Welcome to TAKO");
    expect(document.body.textContent).not.toContain("TEMPORARY PLACEHOLDER");
    expect(document.body.textContent).not.toContain("Research unavailable");
    expect(document.activeElement).toBe(q("welcome-heading"));
  });

  it("discloses offline runtime guidance on request and Escape returns focus", async () => {
    ui = mount(<Harness />);
    await click("setup-options");
    expect(q("setup-options")!.getAttribute("aria-expanded")).toBe("true");
    await click("setup-guidance");
    expect(q("setup-guidance-copy")!.textContent).toContain("isolated TAKO Python runtime");
    q("setup-guidance")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    expect(q("setup-options")!.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(q("setup-options"));
  });

  it("CREATE_RESEARCH validates workspace folder, topic, and objective without Git", async () => {
    ui = mount(<Harness />);
    await click("welcome-create");
    await waitFor("create view", () => !!q("onboarding-ws-path"));
    expect(must<HTMLButtonElement>("workspace-create-enter").disabled).toBe(true);
    await type("onboarding-ws-path", "/tmp/my_research_ws");
    expect(must<HTMLButtonElement>("workspace-create-enter").disabled).toBe(false);
    await click("workspace-back");
    await waitFor("back to welcome", () => document.activeElement === q("welcome-heading"));
  });

  it("RETURNING_USER shows Research Home with Resume, Create, Open, and View Welcome", async () => {
    ui = mount(<Harness initial="RETURNING_USER" lastPath="/tmp/saved_ws" />);
    await tick();
    expect(q("research-home-heading")!.textContent).toContain("Research Home");
    expect(q("home-resume-last")).not.toBeNull();
    expect(q("home-last-path")!.textContent).toContain("/tmp/saved_ws");
    expect(q("home-create-research")).not.toBeNull();
    expect(q("home-open-research")).not.toBeNull();
    expect(q("home-view-welcome")).not.toBeNull();
  });
});
