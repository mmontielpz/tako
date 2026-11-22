import fs from "node:fs";
import path from "node:path";
import { once } from "node:events";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { BridgeError } from "../src/backend";
import { completeOnboarding, getOnboardingState, openEngineeringWorkspace } from "../src/onboarding";
import { StdioBackend } from "./helpers/stdioBackend";
import { gitIn, makeFixture, type Fixture } from "./helpers/fixtures";

let fx: Fixture;
let be: StdioBackend;
const children = new Set<StdioBackend>();
function start(data: string): StdioBackend {
  const child = new StdioBackend(data);
  children.add(child);
  return child;
}
async function stop(child: StdioBackend) {
  if (child.child.exitCode === null && child.child.signalCode === null) {
    const closed = once(child.child, "close");
    child.close();
    await closed;
  }
  children.delete(child);
}
const completed = { status: "completed", last_workspace: "engineering" };
const marker = () => path.join(fx.data, "onboarding-v1.json");
beforeEach(() => { fx = makeFixture(); be = start(fx.data); });
afterEach(async () => { await Promise.all([...children].map(stop)); fx.cleanup(); });

describe("native onboarding contract through actual Rust processes", () => {
  it("requires native workspace entry and never writes on missing-state reads or open alone", async () => {
    expect(await getOnboardingState(be)).toEqual({ status: "first_run" });
    await expect(completeOnboarding(be)).rejects.toMatchObject({ code: "onboarding_workspace_not_open" });
    expect(await openEngineeringWorkspace(be)).toEqual({ workspace: "engineering", opened: true });
    expect(await getOnboardingState(be)).toEqual({ status: "first_run" });
    expect(fs.existsSync(marker())).toBe(false);
    expect(be.calls).toEqual(["onboarding_state", "onboarding_complete", "onboarding_open_workspace", "onboarding_state"]);
    expect(gitIn(fx.project, "status", "--porcelain")).toBe("");
  });

  it("persists exact metadata across a stopped and newly launched native bridge process", async () => {
    await openEngineeringWorkspace(be);
    expect(await completeOnboarding(be)).toEqual(completed);
    const bytes = fs.readFileSync(marker());
    expect(JSON.parse(bytes.toString())).toEqual({ schema_version: 1, intro_version: 1, completed: true, last_workspace: "engineering" });
    const oldPid = be.child.pid;
    await stop(be);
    be = start(fx.data);
    expect(be.child.pid).not.toBe(oldPid);
    expect(await getOnboardingState(be)).toEqual(completed);
    await expect(completeOnboarding(be)).rejects.toMatchObject({ code: "onboarding_workspace_not_open" });
    await openEngineeringWorkspace(be);
    expect(await completeOnboarding(be)).toEqual(completed);
    expect(fs.readFileSync(marker())).toEqual(bytes);
    expect(be.count("open_project")).toBe(0);
  });

  it("incomplete native entry restarts first-run and stays isolated from another data root", async () => {
    await openEngineeringWorkspace(be);
    await stop(be);
    be = start(fx.data);
    expect(await getOnboardingState(be)).toEqual({ status: "first_run" });
    await expect(completeOnboarding(be)).rejects.toMatchObject({ code: "onboarding_workspace_not_open" });
    await openEngineeringWorkspace(be); await completeOnboarding(be);
    const other = start(path.join(fx.root, "other-data"));
    expect(await getOnboardingState(other)).toEqual({ status: "first_run" });
  });

  it("rejects Research and arbitrary paths at both TypeScript and native command boundaries", async () => {
    // @ts-expect-error Research is deliberately absent from the routable workspace type.
    await expect(be.call("onboarding_open_workspace", { workspace: "research" })).rejects.toMatchObject({ code: "invalid_input" });
    // @ts-expect-error No arbitrary persistence paths are accepted by the bridge.
    await expect(be.call("onboarding_complete", { workspace: "engineering", path: "../private" })).rejects.toMatchObject({ code: "invalid_input" });
    // @ts-expect-error State has no configurable filename or arguments.
    await expect(be.call("onboarding_state", {})).rejects.toBeInstanceOf(BridgeError);
    expect(fs.existsSync(marker())).toBe(false);
  });

  it("refuses corrupt/unknown metadata while actual local Engineering remains usable", async () => {
    await getOnboardingState(be);
    const corrupt = Buffer.from("{PRIVATE_SENTINEL"); fs.writeFileSync(marker(), corrupt);
    expect(await getOnboardingState(be)).toEqual({ status: "unavailable", reason: "invalid_data" });
    await openEngineeringWorkspace(be);
    await expect(completeOnboarding(be)).rejects.toMatchObject({ code: "onboarding_state_unavailable", details: [] });
    expect(fs.readFileSync(marker())).toEqual(corrupt);
    const pv = await be.call("open_project", { path: fx.project });
    expect(pv.isolation.provider_execution).toBe("disabled");
    const mission = await be.call("create_mission", { project_path: fx.project, goal: "Review locally", criteria: ["Review only"], checks: ["true"] });
    expect(mission.mission.state).toBe("created"); expect(mission.mission.approval).toBe(null);
    expect(be.count("start_execution")).toBe(0); expect(be.count("proposal_start")).toBe(0);
    expect((await be.call("integration_status", undefined)).availability).toBe("disabled");
    fs.writeFileSync(marker(), JSON.stringify({ schema_version: 2, intro_version: 1, completed: true, last_workspace: "engineering" }));
    expect(await getOnboardingState(be)).toEqual({ status: "unavailable", reason: "unsupported_version" });
  });

  it("refuses symlink metadata with the target and project content untouched", async () => {
    await getOnboardingState(be);
    const target = path.join(fx.project, "README.md");
    const before = fs.readFileSync(target); fs.symlinkSync(target, marker());
    await openEngineeringWorkspace(be);
    expect(await getOnboardingState(be)).toEqual({ status: "unavailable", reason: "unsafe_file" });
    await expect(completeOnboarding(be)).rejects.toMatchObject({ code: "onboarding_state_unavailable" });
    expect(fs.readFileSync(target)).toEqual(before); expect(fs.lstatSync(marker()).isSymbolicLink()).toBe(true);
  });
});
