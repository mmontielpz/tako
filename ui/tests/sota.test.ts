import { describe, expect, it, vi } from "vitest";
import type { Backend } from "../src/backend";
import { pythonSotaBridge } from "../src/sotaBridge";

describe("pythonSotaBridge", () => {
  it("routes all seven typed SOTA IPC commands through Backend.call", async () => {
    const call = vi.fn(async (cmd: string, args: unknown) => ({
      contract_version: "tako-sota-ipc/v1",
      operation: cmd,
      workspace_path: "/tmp/ws",
      runtime: null,
      exit_code: 0,
      execution_status: "succeeded",
      scientific_status: null,
      data: args ?? {},
    }));
    const backend: Backend = { call: call as Backend["call"] };
    const sota = pythonSotaBridge(backend);

    await sota.resolveRuntime();
    await sota.wsInit({ workspace_path: "/tmp/ws", topic: "Edge AI", objective: "Survey" });
    await sota.wsStatus({ workspace_path: "/tmp/ws" });
    await sota.runDiscovery({ workspace_path: "/tmp/ws", fixture_path: "/tmp/fx.json", iterative: true });
    await sota.runAiNative({ workspace_path: "/tmp/ws", provider: "fixture", with_reviewer: true });
    await sota.readEvents({ workspace_path: "/tmp/ws" });
    await sota.exportArtifacts({ workspace_path: "/tmp/ws", out_dir: "export_out", format: "all" });

    expect(call.mock.calls.map((c) => c[0])).toEqual([
      "sota_resolve_runtime",
      "sota_ws_init",
      "sota_ws_status",
      "sota_run_discovery",
      "sota_run_ai_native",
      "sota_read_events",
      "sota_export",
    ]);
  });
});
