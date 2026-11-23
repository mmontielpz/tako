/** Typed frontend bindings for the M029F.1 offline Python SOTA IPC bridge. */
import type { Backend } from "./backend";

export type SotaContractVersion = "tako-sota-ipc/v1";

export type SotaExecutionStatus =
  | "succeeded"
  | "insufficient_evidence"
  | "failed"
  | "interrupted"
  | "unauthorized"
  | "missing_runtime"
  | "incompatible_runtime"
  | "invalid_response";

export interface SotaRuntimeInfo {
  executable: string;
  version: string;
  source: string;
}

export interface SotaIpcResponse<T = Record<string, unknown>> {
  contract_version: SotaContractVersion;
  operation: string;
  workspace_path: string | null;
  runtime: SotaRuntimeInfo | null;
  exit_code: number | null;
  execution_status: SotaExecutionStatus;
  scientific_status: Record<string, unknown> | null;
  data: T;
}

export interface SotaResolveRuntimeArgs {
  runtime_executable?: string;
  timeout_ms?: number;
}

export interface SotaWsInitArgs {
  workspace_path: string;
  topic: string;
  objective: string;
  scope?: string;
  force?: boolean;
  runtime_executable?: string;
  timeout_ms?: number;
}

export interface SotaWsStatusArgs {
  workspace_path: string;
  runtime_executable?: string;
  timeout_ms?: number;
}

export interface SotaRunDiscoveryArgs {
  workspace_path: string;
  fixture_path: string;
  topic?: string;
  objective?: string;
  iterative?: boolean;
  max_papers?: number;
  max_rounds?: number;
  max_requests?: number;
  out_dir?: string;
  force?: boolean;
  runtime_executable?: string;
  timeout_ms?: number;
  max_output_bytes?: number;
}

export interface SotaRunAiNativeArgs {
  workspace_path: string;
  provider?: "fixture";
  fixture_path?: string;
  topic?: string;
  objective?: string;
  max_papers?: number;
  with_reviewer?: boolean;
  out_dir?: string;
  force?: boolean;
  runtime_executable?: string;
  timeout_ms?: number;
  max_output_bytes?: number;
}

export interface SotaReadEventsArgs {
  workspace_path: string;
  run_id?: string;
}

export interface SotaExportArgs {
  workspace_path: string;
  from_dir?: string;
  out_dir: string;
  format?: "all" | "markdown" | "csv" | "json" | "bibtex" | "ris";
  force?: boolean;
  runtime_executable?: string;
  timeout_ms?: number;
  max_output_bytes?: number;
}

export interface SotaWorkflowEvent {
  schema_version: "tako-workflow-event/v1";
  event_type: string;
  run_id: string;
  workspace_id: string;
  workflow: string;
  stage: string;
  timestamp: string;
  elapsed_seconds: number;
  status: string;
  counters: Record<string, unknown>;
  artifacts: string[];
  error: string | null;
  message?: string;
}

export interface SotaReadEventsData {
  run_id: string | null;
  available_runs: string[];
  event_count: number;
  events: SotaWorkflowEvent[];
}

export interface SotaCommands {
  sota_resolve_runtime: [SotaResolveRuntimeArgs | undefined, SotaIpcResponse<SotaRuntimeInfo & { offline_only: boolean }>];
  sota_ws_init: [SotaWsInitArgs, SotaIpcResponse];
  sota_ws_status: [SotaWsStatusArgs, SotaIpcResponse];
  sota_run_discovery: [SotaRunDiscoveryArgs, SotaIpcResponse];
  sota_run_ai_native: [SotaRunAiNativeArgs, SotaIpcResponse];
  sota_read_events: [SotaReadEventsArgs, SotaIpcResponse<SotaReadEventsData>];
  sota_export: [SotaExportArgs, SotaIpcResponse];
}

export function pythonSotaBridge(backend: Backend) {
  return {
    resolveRuntime: (args?: SotaResolveRuntimeArgs) => backend.call("sota_resolve_runtime", args),
    wsInit: (args: SotaWsInitArgs) => backend.call("sota_ws_init", args),
    wsStatus: (args: SotaWsStatusArgs) => backend.call("sota_ws_status", args),
    runDiscovery: (args: SotaRunDiscoveryArgs) => backend.call("sota_run_discovery", args),
    runAiNative: (args: SotaRunAiNativeArgs) => backend.call("sota_run_ai_native", args),
    readEvents: (args: SotaReadEventsArgs) => backend.call("sota_read_events", args),
    exportArtifacts: (args: SotaExportArgs) => backend.call("sota_export", args),
  };
}
