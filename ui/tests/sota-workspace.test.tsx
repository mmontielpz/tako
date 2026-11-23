import { afterEach, describe, expect, it, vi } from "vitest";
import { BridgeError, type Backend } from "../src/backend";
import { SotaWorkspacePanel } from "../src/SotaWorkspacePanel";
import { click, mount, q, text, type, waitFor } from "./helpers/dom";

let ui: ReturnType<typeof mount> | null = null;
afterEach(() => {
  ui?.unmount();
  ui = null;
});

describe("SotaWorkspacePanel simplified research experience", () => {
  it("runs primary Offline Demo without JSON fixture path, shows insufficiency, exports, and collapses Research Mate", async () => {
    let jobCounter = 0;
    const call = vi.fn(async (cmd: string, args: Record<string, unknown> = {}) => {
      if (cmd === "sota_resolve_runtime") {
        if (args?.runtime_executable === "/bad/tako") {
          throw new BridgeError({ code: "missing_runtime", message: "missing", details: [] });
        }
        return {
          contract_version: "tako-sota-ipc/v1",
          operation: cmd,
          workspace_path: null,
          runtime: { executable: "/opt/tako/bin/tako", version: "0.1.0a3", source: "env:TAKO_PYTHON_CLI_BIN" },
          exit_code: 0,
          execution_status: "succeeded",
          scientific_status: null,
          data: {},
        };
      }
      if (cmd === "sota_ws_init" || cmd === "sota_ws_status") {
        return {
          contract_version: "tako-sota-ipc/v1",
          operation: cmd,
          workspace_path: String(args.workspace_path),
          runtime: { executable: "/opt/tako/bin/tako", version: "0.1.0a3", source: "env:TAKO_PYTHON_CLI_BIN" },
          exit_code: 0,
          execution_status: cmd === "sota_ws_status" ? "insufficient_evidence" : "succeeded",
          scientific_status: null,
          data: {
            research_id: "edge-ai-001",
            topic: "Edge AI Segmentation",
            objective: "Survey latency",
            tako_version: "0.1.0a3",
            quality_status: { verdict: "INSUFFICIENT_EVIDENCE" },
            stages_effective: { discovery: { outputs: ["sota_report.md"], summary: { accepted_papers: 0, candidates: 2 } } },
          },
        };
      }
      if (cmd === "sota_start_job") {
        jobCounter += 1;
        return {
          contract_version: "tako-sota-ipc/v1",
          operation: cmd,
          workspace_path: String(args.workspace_path),
          runtime: { executable: "/opt/tako/bin/tako", version: "0.1.0a3", source: "env:TAKO_PYTHON_CLI_BIN" },
          exit_code: null,
          execution_status: "running",
          scientific_status: null,
          data: {
            job_id: `sota-job-${jobCounter}`,
            kind: args.kind,
            workspace_path: args.workspace_path,
            status: "running",
            pid: 4242,
            runtime: { executable: "/opt/tako/bin/tako", version: "0.1.0a3", source: "env:TAKO_PYTHON_CLI_BIN" },
            exit_code: null,
            execution_status: "running",
            scientific_status: null,
            result: null,
            error: null,
          },
        };
      }
      if (cmd === "sota_job_status") {
        return {
          contract_version: "tako-sota-ipc/v1",
          operation: cmd,
          workspace_path: "/tmp/ws",
          runtime: { executable: "/opt/tako/bin/tako", version: "0.1.0a3", source: "env:TAKO_PYTHON_CLI_BIN" },
          exit_code: 0,
          execution_status: "insufficient_evidence",
          scientific_status: { accepted: 0, evidence_sufficient: false, counters: { candidates: 2 } },
          data: {
            job_id: args.job_id,
            kind: "ai_native",
            workspace_path: "/tmp/ws",
            status: "insufficient_evidence",
            pid: 4242,
            runtime: { executable: "/opt/tako/bin/tako", version: "0.1.0a3", source: "env:TAKO_PYTHON_CLI_BIN" },
            exit_code: 0,
            execution_status: "insufficient_evidence",
            scientific_status: { accepted: 0, evidence_sufficient: false, counters: { candidates: 2 } },
            result: { termination: "INSUFFICIENT_EVIDENCE" },
            error: null,
          },
        };
      }
      if (cmd === "sota_read_events") {
        return {
          contract_version: "tako-sota-ipc/v1",
          operation: cmd,
          workspace_path: "/tmp/ws",
          runtime: null,
          exit_code: 0,
          execution_status: "succeeded",
          scientific_status: null,
          data: {
            run_id: "sota-run-1",
            available_runs: ["sota-run-1"],
            event_count: 1,
            events: [{ schema_version: "tako-workflow-event/v1", event_type: "workflow_started", run_id: "sota-run-1", workspace_id: "edge-ai-001", workflow: "sota-ai", stage: "discovery", timestamp: "2026-11-23T00:00:00Z", elapsed_seconds: 0.1, status: "RUNNING", counters: {}, artifacts: [], error: null }],
          },
        };
      }
      if (cmd === "sota_export") {
        return {
          contract_version: "tako-sota-ipc/v1",
          operation: cmd,
          workspace_path: "/tmp/ws",
          runtime: null,
          exit_code: 0,
          execution_status: "succeeded",
          scientific_status: null,
          data: { out_dir: "/tmp/ws/export_out", format: "all", inventory: { artifacts: [{ filename: "sota_report.md", bytes: 120 }] } },
        };
      }
      throw new Error(`Unexpected command: ${cmd}`);
    });

    const backend: Backend = { call: call as Backend["call"] };
    ui = mount(<SotaWorkspacePanel backend={backend} pollMs={20} onReturnHome={() => {}} />);

    await waitFor("runtime ready", () => text("sota-compact-runtime").includes("0.1.0a3"));
    expect(q("sota-offline-banner")).toBeTruthy();
    expect(q("research-mate")).toBeTruthy();
    expect(q("sota-settings-disclosure")).toBeTruthy();
    expect(q("sota-events-disclosure")).toBeTruthy();
    expect(text("research-mate-headline")).toContain("Ready to start research");

    await type("sota-ws-path", "/tmp/ws");
    await click("sota-ws-init");
    await waitFor("ws identity", () => text("sota-ws-identity").includes("edge-ai-001"));

    // Primary Start State of the Art requires NO JSON fixture path
    await click("sota-primary-start");
    await waitFor("insufficiency notice", () => !!q("sota-insufficiency-notice"));
    expect(text("sota-accepted-count")).toContain("0");
    expect(text("sota-sufficiency")).toContain("INSUFFICIENT_EVIDENCE");
    expect(text("sota-events-list")).toContain("workflow_started");

    await click("sota-export-btn");
    await waitFor("exported list", () => text("sota-exported-list").includes("sota_report.md"));

    // Collapsible Research Mate
    await click("research-mate-toggle");
    expect(q("research-mate-headline")).toBeNull();
    await click("research-mate-toggle");
    expect(q("research-mate-headline")).not.toBeNull();
  });
});
