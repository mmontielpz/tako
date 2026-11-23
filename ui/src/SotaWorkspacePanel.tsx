import { useEffect, useRef, useState } from "preact/hooks";
import { BridgeError, type Backend } from "./backend";
import { researchCompanionFor } from "./companion";
import { Tako } from "./pixel";
import {
  pythonSotaBridge,
  type SotaIpcResponse,
  type SotaJobView,
  type SotaRuntimeInfo,
  type SotaWorkflowEvent,
} from "./sotaBridge";

export interface SotaWorkspacePanelProps {
  backend: Backend;
  pollMs?: number;
  initialWorkspacePath?: string;
  initialTopic?: string;
  initialObjective?: string;
}

function asRecord(v: unknown): Record<string, unknown> {
  return v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : {};
}

export function SotaWorkspacePanel({
  backend,
  pollMs = 400,
  initialWorkspacePath = "",
  initialTopic = "State of the Art Survey",
  initialObjective = "Identify foundational works and evidence",
}: SotaWorkspacePanelProps) {
  const sota = useRef(pythonSotaBridge(backend)).current;

  const [runtimeExe, setRuntimeExe] = useState("");
  const [runtimeInfo, setRuntimeInfo] = useState<SotaRuntimeInfo | null>(null);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);

  const [wsPath, setWsPath] = useState(initialWorkspacePath);
  const [topic, setTopic] = useState(initialTopic);
  const [objective, setObjective] = useState(initialObjective);
  const [scope, setScope] = useState("");
  const [wsStatus, setWsStatus] = useState<SotaIpcResponse | null>(null);

  const [fixturePath, setFixturePath] = useState("");
  const [iterative, setIterative] = useState(true);
  const [withReviewer, setWithReviewer] = useState(true);
  const [timeoutMs, setTimeoutMs] = useState("");

  const [job, setJob] = useState<SotaJobView | null>(null);
  const [events, setEvents] = useState<SotaWorkflowEvent[]>([]);
  const [exportFromDir, setExportFromDir] = useState("sota_ai_native_output");
  const [exportOutDir, setExportOutDir] = useState("export_out");
  const [exportFormat, setExportFormat] = useState<"all" | "markdown" | "csv" | "json" | "bibtex" | "ris">("all");
  const [exportResult, setExportResult] = useState<Record<string, unknown> | null>(null);

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const rtOpt = runtimeExe.trim() ? runtimeExe.trim() : undefined;
  const parsedTimeout = Number.parseInt(timeoutMs, 10);
  const timeoutOpt = Number.isFinite(parsedTimeout) && parsedTimeout >= 50 ? parsedTimeout : undefined;

  async function checkRuntime() {
    setBusy(true);
    setRuntimeError(null);
    try {
      const res = await sota.resolveRuntime(rtOpt ? { runtime_executable: rtOpt } : undefined);
      setRuntimeInfo(res.runtime ?? null);
    } catch (e) {
      setRuntimeInfo(null);
      setRuntimeError(e instanceof BridgeError ? `${e.code}: ${e.message}` : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function refreshWorkspace(pathOverride?: string) {
    const target = (pathOverride ?? wsPath).trim();
    if (!target) return;
    const res = await sota.wsStatus({ workspace_path: target, runtime_executable: rtOpt });
    setWsStatus(res);
    const data = asRecord(res.data);
    if (typeof data.topic === "string") setTopic(data.topic);
    if (typeof data.objective === "string") setObjective(data.objective);
    try {
      const evRes = await sota.readEvents({ workspace_path: target });
      setEvents(evRes.data.events ?? []);
    } catch {
      setEvents([]);
    }
  }

  useEffect(() => {
    void checkRuntime();
    if (initialWorkspacePath.trim()) {
      void refreshWorkspace(initialWorkspacePath).catch((e) => {
        setError(e instanceof BridgeError ? `${e.code}: ${e.message}` : String(e));
      });
    }
  }, []);

  async function initWorkspace() {
    setBusy(true);
    setError(null);
    try {
      const res = await sota.wsInit({
        workspace_path: wsPath.trim(),
        topic: topic.trim(),
        objective: objective.trim(),
        scope: scope.trim() || undefined,
        runtime_executable: rtOpt,
      });
      setWsStatus(res);
      setRuntimeInfo(res.runtime ?? runtimeInfo);
      setEvents([]);
    } catch (e) {
      setError(e instanceof BridgeError ? `${e.code}: ${e.message}` : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function openWorkspace() {
    setBusy(true);
    setError(null);
    try {
      await refreshWorkspace();
    } catch (e) {
      setError(e instanceof BridgeError ? `${e.code}: ${e.message}` : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function pickWorkspaceFolder() {
    if (!backend.pickFolder) return;
    const picked = await backend.pickFolder();
    if (picked) setWsPath(picked);
  }

  async function startWorkflow(kind: "discovery" | "ai_native") {
    setBusy(true);
    setError(null);
    try {
      const res = await sota.startJob({
        kind,
        workspace_path: wsPath.trim(),
        fixture_path: fixturePath.trim() || undefined,
        provider: kind === "ai_native" ? "fixture" : undefined,
        topic: topic.trim() || undefined,
        objective: objective.trim() || undefined,
        iterative: kind === "discovery" ? iterative : false,
        with_reviewer: kind === "ai_native" ? withReviewer : false,
        force: true,
        runtime_executable: rtOpt,
        timeout_ms: timeoutOpt,
      });
      setJob(res.data);
      setExportFromDir(kind === "discovery" ? "sota_output" : "sota_ai_native_output");
    } catch (e) {
      setError(e instanceof BridgeError ? `${e.code}: ${e.message}` : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function cancelActiveJob() {
    if (!job) return;
    setError(null);
    try {
      const res = await sota.cancelJob({ job_id: job.job_id });
      setJob(res.data);
      await refreshWorkspace(job.workspace_path);
    } catch (e) {
      setError(e instanceof BridgeError ? `${e.code}: ${e.message}` : String(e));
    }
  }

  useEffect(() => {
    if (!job || job.status !== "running") return;
    let active = true;
    const timer = setInterval(async () => {
      try {
        const st = await sota.jobStatus({ job_id: job.job_id });
        if (!active) return;
        setJob(st.data);
        if (st.data.status !== "running") {
          clearInterval(timer);
          await refreshWorkspace(st.data.workspace_path);
        }
      } catch {
        // Ignore transient poll error; next tick or manual refresh recovers.
      }
    }, pollMs);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [job?.job_id, job?.status, pollMs]);

  async function runExport() {
    setBusy(true);
    setError(null);
    try {
      const res = await sota.exportArtifacts({
        workspace_path: wsPath.trim(),
        from_dir: exportFromDir.trim() || undefined,
        out_dir: exportOutDir.trim(),
        format: exportFormat,
        force: true,
        runtime_executable: rtOpt,
      });
      setExportResult(asRecord(res.data));
      await refreshWorkspace();
    } catch (e) {
      setError(e instanceof BridgeError ? `${e.code}: ${e.message}` : String(e));
    } finally {
      setBusy(false);
    }
  }

  const wsData = asRecord(wsStatus?.data);
  const stages = asRecord(wsData.stages_effective);
  const discoveryStage = asRecord(stages.discovery);
  const discoverySummary = asRecord(discoveryStage.summary);
  const qualityStatus = asRecord(wsData.quality_status);
  const jobSci = asRecord(job?.scientific_status);
  const jobResult = asRecord(job?.result);

  const researchId = typeof wsData.research_id === "string" ? wsData.research_id : null;
  const acceptedCount =
    typeof jobSci.accepted === "number"
      ? jobSci.accepted
      : typeof discoverySummary.accepted_papers === "number"
        ? discoverySummary.accepted_papers
        : null;
  const candidateCount =
    typeof asRecord(jobSci.counters).candidates === "number"
      ? (asRecord(jobSci.counters).candidates as number)
      : typeof discoverySummary.candidates === "number"
        ? discoverySummary.candidates
        : null;
  const evidenceSufficient =
    typeof jobSci.evidence_sufficient === "boolean"
      ? jobSci.evidence_sufficient
      : wsStatus?.execution_status === "insufficient_evidence"
        ? false
        : null;

  const exportInventory = asRecord(exportResult?.inventory);
  const exportedArtifacts = Array.isArray(exportInventory.artifacts)
    ? (exportInventory.artifacts as Record<string, unknown>[])
    : [];

  const mate = researchCompanionFor({
    hasWorkspace: Boolean(researchId),
    workspaceId: researchId,
    topic,
    objective,
    jobStatus: job?.status ?? null,
    acceptedCount,
    candidateCount,
    evidenceSufficient,
    eventCount: events.length,
    exportedCount: exportedArtifacts.length,
    error: error ?? runtimeError ?? (job?.error ? `${job.error.code}: ${job.error.message}` : null),
  });

  return (
    <div class="shell" data-testid="sota-workspace-shell">
      <aside class="sidebar" aria-label="Research Workspace">
        <h1 class="brand" data-testid="research-brand">TAKO</h1>
        <p class="muted" style={{ fontSize: "12px", marginTop: "-6px", marginBottom: "12px" }}>
          ARDE · Research Environment
        </p>

        <div class="card" data-testid="sota-workspace-card">
          <strong style={{ fontSize: "12px" }}>Research Workspace</strong>
          <label class="field" style={{ marginTop: "6px" }}>
            <span>Workspace folder</span>
            <div class="row">
              <input
                data-testid="sota-ws-path"
                value={wsPath}
                placeholder="/path/to/research_ws"
                onInput={(e) => setWsPath((e.target as HTMLInputElement).value)}
              />
              {backend.pickFolder && (
                <button class="secondary" onClick={() => void pickWorkspaceFolder()}>
                  Browse
                </button>
              )}
            </div>
          </label>
          <div class="row">
            <button
              data-testid="sota-ws-init"
              disabled={busy || !wsPath.trim() || !topic.trim() || !objective.trim()}
              onClick={() => void initWorkspace()}
            >
              Create workspace
            </button>
            <button
              class="secondary"
              data-testid="sota-ws-open"
              disabled={busy || !wsPath.trim()}
              onClick={() => void openWorkspace()}
            >
              Open workspace
            </button>
          </div>
          {researchId && (
            <div class="muted" data-testid="sota-ws-identity" style={{ marginTop: "8px", fontSize: "12px" }}>
              <div>ID: <strong>{researchId}</strong></div>
              <div>TAKO: {String(wsData.tako_version ?? "unknown")}</div>
              <div>Quality: {String(qualityStatus.verdict ?? "UNKNOWN")}</div>
            </div>
          )}
        </div>

        <details class="card" data-testid="sota-settings-disclosure">
          <summary style={{ cursor: "pointer", fontSize: "12px", fontWeight: "600" }}>
            Settings &amp; Runtime
          </summary>
          <div style={{ marginTop: "8px" }}>
            <label class="field">
              <span>TAKO CLI executable (optional)</span>
              <input
                data-testid="sota-runtime-input"
                value={runtimeExe}
                placeholder="Defaults to TAKO_PYTHON_CLI_BIN"
                onInput={(e) => setRuntimeExe((e.target as HTMLInputElement).value)}
              />
            </label>
            <button
              class="secondary"
              data-testid="sota-check-runtime"
              disabled={busy}
              onClick={() => void checkRuntime()}
            >
              Check runtime
            </button>
            {runtimeInfo && (
              <p class="muted" data-testid="sota-runtime-status" role="status">
                Ready: {runtimeInfo.version} ({runtimeInfo.source}) — {runtimeInfo.executable}
              </p>
            )}
            {runtimeError && (
              <p class="error" data-testid="sota-runtime-error" role="alert">
                {runtimeError}
              </p>
            )}
            <label class="field" style={{ marginTop: "8px" }}>
              <span>Timeout override (ms, optional)</span>
              <input
                data-testid="sota-timeout-input"
                value={timeoutMs}
                placeholder="60000"
                onInput={(e) => setTimeoutMs((e.target as HTMLInputElement).value)}
              />
            </label>
            <label style={{ display: "block", marginTop: "6px", fontSize: "12px" }}>
              <input
                type="checkbox"
                checked={iterative}
                onChange={(e) => setIterative((e.target as HTMLInputElement).checked)}
              />{" "}
              Iterative discovery
            </label>
            <label style={{ display: "block", marginTop: "4px", fontSize: "12px" }}>
              <input
                type="checkbox"
                checked={withReviewer}
                onChange={(e) => setWithReviewer((e.target as HTMLInputElement).checked)}
              />{" "}
              Include AI-native Reviewer (fixture)
            </label>
          </div>
        </details>
      </aside>

      <main class="main">
        <div class="card row" role="status" data-testid="sota-offline-banner" style={{ justifyContent: "space-between", alignItems: "center" }}>
          <span>
            <strong>Offline Fixture Mode</strong> — Live network discovery, paid APIs, and authenticated AI CLIs are disabled.
          </span>
          <span class="chip" data-testid="sota-compact-runtime">
            {runtimeInfo ? `CLI ${runtimeInfo.version}` : runtimeError ? "Runtime missing" : "Checking runtime…"}
          </span>
        </div>

        {(error || runtimeError) && (
          <div class="card error" role="alert" data-testid="sota-error">
            {error ?? runtimeError}
          </div>
        )}

        <section class="card" data-testid="sota-config-card">
          <h2>State of the Art Workflow</h2>
          <label class="field">
            <span>Research topic</span>
            <input
              data-testid="sota-topic"
              value={topic}
              onInput={(e) => setTopic((e.target as HTMLInputElement).value)}
            />
          </label>
          <label class="field">
            <span>Research objective</span>
            <input
              data-testid="sota-objective"
              value={objective}
              onInput={(e) => setObjective((e.target as HTMLInputElement).value)}
            />
          </label>
          <label class="field">
            <span>Scope (optional)</span>
            <input
              data-testid="sota-scope"
              value={scope}
              onInput={(e) => setScope((e.target as HTMLInputElement).value)}
            />
          </label>
          <label class="field">
            <span>Discovery fixture JSON path (required for Discovery)</span>
            <input
              data-testid="sota-fixture-path"
              value={fixturePath}
              placeholder="/path/to/fixture.json"
              onInput={(e) => setFixturePath((e.target as HTMLInputElement).value)}
            />
          </label>
          <div class="row">
            <button
              data-testid="sota-run-discovery"
              disabled={busy || !wsPath.trim() || !fixturePath.trim() || job?.status === "running"}
              onClick={() => void startWorkflow("discovery")}
            >
              Run Offline Discovery
            </button>
            <button
              data-testid="sota-run-ai-native"
              disabled={busy || !wsPath.trim() || job?.status === "running"}
              onClick={() => void startWorkflow("ai_native")}
            >
              Run Offline AI-Native (Fixture)
            </button>
            <button
              class="secondary"
              data-testid="sota-cancel-job"
              disabled={!job || job.status !== "running"}
              onClick={() => void cancelActiveJob()}
            >
              Cancel Running Job
            </button>
          </div>
        </section>

        {job && (
          <section class="card" data-testid="sota-job-card" role="status">
            <h3>Execution Status: {job.status}</h3>
            <div class="muted" style={{ fontSize: "12px" }}>
              Job: {job.job_id} · Kind: {job.kind} · PID: {job.pid ?? "pending"} · Exit: {job.exit_code ?? "running"}
            </div>
            {job.error && (
              <p class="error" data-testid="sota-job-error">
                {job.error.code}: {job.error.message}
              </p>
            )}
            {typeof jobResult.termination === "string" && (
              <div data-testid="sota-termination">Termination: {jobResult.termination}</div>
            )}
          </section>
        )}

        <section class="card" data-testid="sota-results-card">
          <h3>Scientific Results &amp; Artifacts</h3>
          <div class="row" style={{ marginBottom: "8px" }}>
            <span class="chip" data-testid="sota-accepted-count">
              Accepted: {acceptedCount ?? "N/A"}
            </span>
            <span class="chip" data-testid="sota-candidate-count">
              Candidates: {candidateCount ?? "N/A"}
            </span>
            <span class="chip" data-testid="sota-sufficiency">
              Sufficiency:{" "}
              {evidenceSufficient === null
                ? "UNKNOWN"
                : evidenceSufficient
                  ? "SUFFICIENT"
                  : "INSUFFICIENT_EVIDENCE"}
            </span>
          </div>
          {evidenceSufficient === false && (
            <p class="error" data-testid="sota-insufficiency-notice" role="status">
              Workflow completed with insufficient scientific evidence (0 accepted publications or
              incomplete coverage). Do not treat this as a sufficient SOTA.
            </p>
          )}
          {Array.isArray(discoveryStage.outputs) && (
            <ul data-testid="sota-stage-outputs">
              {(discoveryStage.outputs as string[]).map((o) => (
                <li key={o}>{o}</li>
              ))}
            </ul>
          )}

          <h4>Export Artifacts</h4>
          <div class="row">
            <input
              data-testid="sota-export-from"
              value={exportFromDir}
              placeholder="Source subdir (e.g. sota_output)"
              onInput={(e) => setExportFromDir((e.target as HTMLInputElement).value)}
            />
            <input
              data-testid="sota-export-out"
              value={exportOutDir}
              placeholder="Export subdir (e.g. export_out)"
              onInput={(e) => setExportOutDir((e.target as HTMLInputElement).value)}
            />
            <select
              data-testid="sota-export-format"
              value={exportFormat}
              onChange={(e) =>
                setExportFormat((e.target as HTMLSelectElement).value as typeof exportFormat)
              }
            >
              <option value="all">all</option>
              <option value="markdown">markdown</option>
              <option value="csv">csv</option>
              <option value="json">json</option>
              <option value="bibtex">bibtex</option>
              <option value="ris">ris</option>
            </select>
            <button
              data-testid="sota-export-btn"
              disabled={busy || !wsPath.trim() || !exportOutDir.trim()}
              onClick={() => void runExport()}
            >
              Export
            </button>
          </div>
          {exportedArtifacts.length > 0 && (
            <ul data-testid="sota-exported-list">
              {exportedArtifacts.map((a, idx) => (
                <li key={idx}>
                  {String(a.filename)} ({String(a.bytes)} bytes)
                </li>
              ))}
            </ul>
          )}
        </section>

        <section class="card" data-testid="sota-events-card">
          <h3>Workflow Events ({events.length})</h3>
          {events.length === 0 ? (
            <p class="muted">No workflow events recorded yet.</p>
          ) : (
            <ul data-testid="sota-events-list">
              {events.map((ev, idx) => (
                <li key={idx}>
                  [{ev.elapsed_seconds}s] <strong>{ev.event_type}</strong> ({ev.stage}: {ev.status})
                  {ev.message ? ` — ${ev.message}` : ""}
                </li>
              ))}
            </ul>
          )}
        </section>
      </main>

      <aside class="companion" aria-label="Research Mate" data-testid="research-mate">
        <div class="tako-row">
          <Tako pose={mate.pose} />
          <div>
            <strong>Research Mate</strong>
            <div class="muted" style={{ fontSize: "11px" }} data-testid="research-mate-pose">
              {mate.pose}
            </div>
          </div>
        </div>
        <h3 data-testid="research-mate-headline">{mate.headline}</h3>
        <p data-testid="research-mate-detail">{mate.detail}</p>
        <p class="muted" data-testid="research-mate-next">
          <strong>Next:</strong> {mate.next}
        </p>
      </aside>
    </div>
  );
}
