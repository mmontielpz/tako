import { useState } from "preact/hooks";
import { afterEach, describe, expect, it } from "vitest";
import { WelcomeWizard } from "../src/WelcomeWizard";
import { click, mount, must, q, tick, waitFor } from "./helpers/dom";

let ui: ReturnType<typeof mount> | null = null;
afterEach(() => { ui?.unmount(); ui = null; });
function Harness() {
  const [screen, setScreen] = useState<"welcome" | "choose">("welcome");
  const [selected, setSelected] = useState(false);
  return <WelcomeWizard screen={screen} selected={selected} pending={false} restoring={false} error={null}
    onContinue={() => setScreen("choose")} onBack={() => setScreen("welcome")} onSelect={() => setSelected(true)} onOpen={() => { throw new Error("Enter on a radio must not open"); }} onRetryRead={() => {}} />;
}

describe("approved Welcome/selector accessibility and local presentation", () => {
  it("has a labelled temporary brand slot and no provider/account inputs", async () => {
    ui = mount(<Harness />); await tick();
    expect(document.body.textContent).toContain("TEMPORARY PLACEHOLDER");
    expect(document.querySelector("input")).toBeNull();
    expect(document.activeElement).toBe(q("welcome-heading"));
  });
  it("discloses guidance only on request and Escape returns focus to Setup options", async () => {
    ui = mount(<Harness />); await click("setup-options");
    expect(q("setup-options")!.getAttribute("aria-expanded")).toBe("true");
    expect(q("setup-guidance-copy")).toBeNull(); await click("setup-guidance");
    expect(q("setup-guidance-copy")!.textContent).toContain("continue without configuring anything");
    q("setup-guidance")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick(); expect(q("setup-options")!.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(q("setup-options"));
  });
  it("keyboard Space selects Engineering; Enter on that radio does not open it", async () => {
    ui = mount(<Harness />); await click("welcome-continue");
    const radio = must<HTMLInputElement>("workspace-engineering"); radio.focus();
    radio.dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true })); await tick();
    expect(must<HTMLInputElement>("workspace-engineering").checked).toBe(true);
    expect(must<HTMLButtonElement>("workspace-open").disabled).toBe(false);
    radio.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })); await tick();
    expect(q("workspace-heading")).not.toBeNull();
  });
  it("disabled Research has a readable keyboard-reachable explanation, separate from selection", async () => {
    ui = mount(<Harness />); await click("welcome-skip");
    const note = must<HTMLElement>("research-unavailable"); note.focus();
    expect(document.activeElement).toBe(note); expect(note.getAttribute("aria-label")).toContain("Research unavailable");
    expect(must<HTMLInputElement>("workspace-research").disabled).toBe(true);
    expect(must<HTMLInputElement>("workspace-engineering").checked).toBe(false);
    await click("workspace-back"); await waitFor("welcome focus", () => document.activeElement === q("welcome-heading"));
  });
});
