import { afterEach, beforeEach, describe, expect, it } from "vitest";
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

describe.skipIf(process.env.TAKO_LIVE_BACKEND_TESTS !== "1")("TAKO desktop backend integration (T-STABILIZATION-001)", () => {
  it("backend_status returns connection and runtime status from the live T adapter", async () => {
    const status = await be.call("backend_status", undefined);
    expect(status.online).toBe(true);
    expect(status.operational_mode).toBe("READ_ONLY_INTEGRATION_READY");
    expect(status.g12_state).toBe("DEFERRED");
    expect(status.base_url).toContain("8085");
  });

  it("backend_projects returns real registered projects from the T backend", async () => {
    const res = await be.call("backend_projects", undefined);
    expect(res.total).toBeGreaterThan(0);
    expect(res.projects.some((p) => p.project_id === "era1c-confkit")).toBe(true);
    expect(res.allowed_roots.length).toBeGreaterThan(0);
  });

  it("backend_governance returns spending limits, currency, and pending approvals", async () => {
    const gov = await be.call("backend_governance", undefined);
    expect(gov.limits.spending_limit_usd).toBe(0.0);
    expect(gov.limits.spending_currency).toBe("USD");
    expect(gov.limits.max_concurrent_workers).toBe(2);
    expect(gov.approvals.length).toBeGreaterThan(0);
    expect(gov.approvals.some((a) => a.approval_id.includes("campaign"))).toBe(true);
  });

  it("open_project with a backend project ID populates real backend missions", async () => {
    const pv = await be.call("open_backend_project", { project_id: "era1c-confkit" });
    expect(pv.project.path).toContain("era1c-confkit");
    expect(pv.missions.length).toBeGreaterThan(0);
    expect(pv.missions.some((m) => m.goal.includes("ConfigManager"))).toBe(true);
  });

  it("mutations against T ecosystem projects are refused and fail closed", async () => {
    await expect(
      be.call("create_mission", {
        project_path: "/workspace/projects/t-master-sandboxes/era1c-confkit",
        goal: "Unauthorized mutation attempt",
        criteria: ["c"],
        checks: ["true"],
      })
    ).rejects.toThrow(/Mutations and autonomous executions/);
  });

  it("backend_workstations returns configured workstations from JACK (T-JACK-001)", async () => {
    const res = await be.call("backend_workstations", undefined);
    expect(res.total).toBeGreaterThanOrEqual(2);
    expect(res.workstations.some((w) => w.workstation_id === "workstation-mac-itj1060")).toBe(true);
    expect(res.workstations.some((w) => w.workstation_id === "workstation-linux-cachyos")).toBe(true);
  });

  it("backend_workstation returns single workstation details (T-JACK-001)", async () => {
    const mac = await be.call("backend_workstation", { workstation_id: "workstation-mac-itj1060" });
    expect(mac.display_name).toContain("MacBook Pro");
    expect(mac.os).toBe("macOS");
    expect(mac.transport).toBe("tailscale-ssh");
    expect(mac.is_local).toBe(false);
  });

  it("backend_status includes workstations in consolidated view (T-JACK-001)", async () => {
    const status = await be.call("backend_status", undefined);
    expect(status.workstations).toBeDefined();
    expect(status.workstations!.length).toBeGreaterThanOrEqual(2);
    const mac = status.workstations!.find((w) => w.workstation_id === "workstation-mac-itj1060");
    expect(mac).toBeDefined();
    expect(mac?.connectivity_status).toBe("connected");
  });
});
