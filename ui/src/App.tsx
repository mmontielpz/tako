import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { ComponentChildren } from "preact";
import { BridgeError, type Backend } from "./backend";
import { companionFor } from "./companion";
import { Tako } from "./pixel";
import { Proposals } from "./Proposals";
import { SotaWorkspacePanel } from "./SotaWorkspacePanel";
import type { ActionId, MissionView, ProjectView, StartupReport, BackendStatus, RegisteredProject, BackendGovernanceResponse, BackendEvidenceView, IntegrationView } from "./types";

const STAGES = ["Goal", "Plan", "Run", "Verify", "Decide"] as const;

function stageOf(v: MissionView | null): number {
  if (!v) return 0;
  switch (v.mission.state) {
    case "created": case "plan_ready": case "approved": return 1;
    case "executing": case "sealed": case "execution_failed": case "timed_out": case "cancelled": case "interrupted": return 2;
    case "verifying": return 3;
    case "evidence_ready": return 4;
    case "accepted": case "rejected": return 5;
  }
}

function lines(s: string): string[] {
  return s.split("\n").map((l) => l.trim()).filter(Boolean);
}

const DEFAULT_SCRIPT = "printf 'hello from a scripted agent\\n' > hello.txt\necho done writing";
const DEFAULT_CHECK = "grep -q hello hello.txt";

export interface AppProps {
  backend: Backend;
  /** Polling interval while a mission is running. */
  pollMs?: number;
}

export function App({ backend, pollMs = 500 }: AppProps) {
  const [startup, setStartup] = useState<StartupReport | null>(null);
  const [integration, setIntegration] = useState<IntegrationView | null>(null);
  const [integrationPending, setIntegrationPending] = useState(false);
  const [integrationError, setIntegrationError] = useState<string | null>(null);
  const integrationGeneration = useRef(0);
  const selectionGeneration = useRef(0);
  const [backendStatus, setBackendStatus] = useState<BackendStatus | null>(null);
  const [backendProjects, setBackendProjects] = useState<RegisteredProject[]>([]);
  const [governance, setGovernance] = useState<BackendGovernanceResponse | null>(null);
  const [pathInput, setPathInput] = useState("");
  const [pv, setPv] = useState<ProjectView | null>(null);
  const projectRef = useRef<ProjectView | null>(null);
  projectRef.current = pv;
  const [view, setView] = useState<MissionView | null>(null);
  const [remoteEvidence, setRemoteEvidence] = useState<BackendEvidenceView | null>(null);
  const [error, setError] = useState<BridgeError | null>(null);
  const [pending, setPending] = useState(false);
  const [workspaceMode, setWorkspaceMode] = useState<"engineering" | "research">("engineering");
  const lock = useRef(false);

  // goal form
  const [goal, setGoal] = useState("");
  const [criteria, setCriteria] = useState("hello.txt exists and contains hello");
  const [checks, setChecks] = useState(DEFAULT_CHECK);
  // plan form
  const [script, setScript] = useState(DEFAULT_SCRIPT);
  const [expected, setExpected] = useState("hello.txt");
  const [extraPerms, setExtraPerms] = useState("");
  // approval form
  const [grants, setGrants] = useState<string[]>([]);
  const [note, setNote] = useState("");
  const [confirmReject, setConfirmReject] = useState(false);

  /** One call at a time from the UI: prevents accidental duplicate submissions. */
  async function run<T>(fn: () => Promise<T>): Promise<T | undefined> {
    if (lock.current) return undefined;
    lock.current = true;
    setPending(true);
    setError(null);
    try {
      return await fn();
    } catch (e) {
      setError(e instanceof BridgeError ? e : new BridgeError({ code: "internal", message: String(e), details: [] }));
      return undefined;
    } finally {
      lock.current = false;
      setPending(false);
    }
  }

  // Startup: the bridge has already run engine recovery; show what it found.
  useEffect(() => {
    backend.call("startup_report", undefined).then(setStartup, (e) => setError(e as BridgeError));
    // Configuration is local metadata. Startup never probes a private service.
    backend.call("integration_status", undefined).then((current) => {
      if (integrationGeneration.current === 0) setIntegration(current);
    }, () => {
      if (integrationGeneration.current === 0) setIntegrationError("Optional integration settings are unavailable. Local work remains available.");
    });
  }, [backend]);

  function clearBackendData() {
    setBackendStatus(null);
    setBackendProjects([]);
    setGovernance(null);
    setRemoteEvidence(null);
  }

  // Optional I/O uses its own state, never the lock/pending flag for local work.
  async function refreshIntegration() {
    const generation = ++integrationGeneration.current;
    setIntegrationPending(true);
    setIntegrationError(null);
    clearBackendData();
    try {
      const status = await backend.call("backend_status", undefined);
      if (generation !== integrationGeneration.current) return;
      setBackendStatus(status);
      setIntegration((current) => current && { ...current, availability: status.online ? "available" : "unavailable" });
      if (status.online) {
        setBackendProjects(status.projects ?? []);
        if (status.limits) setGovernance({ limits: status.limits, approvals: status.approvals ?? [] });
      }
    } catch {
      if (generation !== integrationGeneration.current) return;
      setIntegration((current) => current && { ...current, availability: "unavailable" });
      setIntegrationError("T integration is unavailable. Local work remains available.");
    } finally {
      if (generation === integrationGeneration.current) setIntegrationPending(false);
    }
  }
  async function configureIntegration(enabled: boolean) {
    const generation = ++integrationGeneration.current;
    setIntegrationPending(true);
    setIntegrationError(null);
    clearBackendData();
    try {
      const state = await backend.call("configure_integration", { enabled });
      if (generation !== integrationGeneration.current) return;
      setIntegration(state);
      if (!enabled && projectRef.current?.source === "t_backend") { projectRef.current = null; setPv(null); setView(null); }
      if (enabled) { await refreshIntegration(); }
    } catch {
      if (generation === integrationGeneration.current) setIntegrationError("Could not change the optional integration. Local work remains available.");
    } finally {
      // refreshIntegration owns its newer generation and pending state if enabled.
      if (generation === integrationGeneration.current) setIntegrationPending(false);
    }
  }

  // Poll the real mission state while anything is running.
  const state = view?.mission.state;
  const active = !!view && (view.job.running || state === "executing" || state === "verifying");
  const missionId = view?.mission.id;
  useEffect(() => {
    if (!active || !missionId) return;
    let stop = false;
    const t = setInterval(async () => {
      try {
        const v = await backend.call("get_mission", { mission_id: missionId });
        if (!stop) setView(v);
      } catch (e) {
        if (!stop) setError(e as BridgeError);
      }
    }, pollMs);
    return () => {
      stop = true;
      clearInterval(t);
    };
  }, [active, missionId, backend, pollMs]);

  const planHash = view?.plan?.hash;
  useLayoutEffect(() => setGrants([]), [planHash]); // reset before the new plan accepts input

  const can = (a: ActionId) => !!view && !pending && view.next_actions.includes(a);

  async function openProject(path: string, backendProjectId?: string) {
    const selection = ++selectionGeneration.current;
    const generation = integrationGeneration.current;
    setRemoteEvidence(null);
    let r: ProjectView | undefined;
    if (backendProjectId) {
      setIntegrationPending(true);
      setIntegrationError(null);
      try {
        r = await backend.call("open_backend_project", { project_id: backendProjectId });
        if (generation !== integrationGeneration.current || selection !== selectionGeneration.current) return;
      } catch {
        if (generation === integrationGeneration.current && selection === selectionGeneration.current) {
          setIntegrationError("Backend project unavailable. Open a local folder to continue.");
        }
        return;
      } finally {
        if (generation === integrationGeneration.current) setIntegrationPending(false);
      }
    } else {
      r = await run(() => backend.call("open_project", { path }));
      if (selection !== selectionGeneration.current) return;
    }
    if (r) {
      projectRef.current = r;
      setPv(r);
      setView(null);
      setConfirmReject(false);
      const open = r.missions.find((m) => m.state !== null && m.state !== "accepted" && m.state !== "rejected");
      if (open) {
        try {
          const resumed = await backend.call("get_mission", { mission_id: open.id });
          if (selection === selectionGeneration.current) setView(resumed);
        } catch (e) {
          if (selection === selectionGeneration.current) setError(e instanceof BridgeError ? e : new BridgeError({ code: "internal", message: String(e), details: [] }));
        }
      }
    }
  }
  async function browse() {
    const p = await backend.pickFolder?.();
    if (p) {
      setPathInput(p);
      await openProject(p);
    }
  }
  async function selectMission(id: string) {
    const selection = ++selectionGeneration.current;
    const generation = integrationGeneration.current;
    setView(null);
    setRemoteEvidence(null);
    const summary = pv?.missions.find((m) => m.id === id);
    if (summary?.state === null) {
      if (!summary.backend_project_id) {
        setError(new BridgeError({ code: "evidence_unavailable", message: "Backend project identity is unavailable; evidence cannot be requested.", details: [] }));
        return;
      }
      setIntegrationPending(true);
      setError(null);
      try {
        const result = await backend.call("backend_mission_evidence", {
          project_id: summary.backend_project_id, mission_id: id,
        });
        if (selection === selectionGeneration.current && generation === integrationGeneration.current) setRemoteEvidence(result);
      } catch (e) {
        if (selection === selectionGeneration.current && generation === integrationGeneration.current) {
          setError(e instanceof BridgeError ? e : new BridgeError({ code: "internal", message: String(e), details: [] }));
        }
      } finally {
        if (generation === integrationGeneration.current) setIntegrationPending(false);
      }
      return;
    }
    const v = await run(() => backend.call("get_mission", { mission_id: id }));
    if (v) setView(v);
  }
  async function act<C extends "approve_plan" | "start_execution" | "start_verification" | "cancel" | "accept" | "reject" | "recover" | "define_plan">(
    cmd: C,
    args: Parameters<Backend["call"]>[1],
  ) {
    const v = await run(() => (backend.call as (c: string, a: unknown) => Promise<MissionView>)(cmd, args));
    if (v) {
      setView(v);
      setPv((p) =>
        p && p.missions.some((x) => x.id === v.mission.id)
          ? { ...p, missions: p.missions.map((x) => (x.id === v.mission.id ? { ...x, state: v.mission.state } : x)) }
          : p,
      );
    }
    return v;
  }

  // Reflect the latest real state of the open mission in the sidebar list.
  useEffect(() => {
    if (!view) return;
    setPv((p) =>
      p && p.missions.some((x) => x.id === view.mission.id && x.state !== view.mission.state)
        ? { ...p, missions: p.missions.map((x) => (x.id === view.mission.id ? { ...x, state: view.mission.state } : x)) }
        : p,
    );
  }, [view?.mission.id, view?.mission.state]);

  const say = companionFor({ hasProject: !!pv, view, error, recovered: startup?.recovered.length ?? 0 });
  const stage = stageOf(view);
  const m = view?.mission;
  const ev = view?.evidence;

  const isBackendProject = pv?.source === "t_backend";

  if (workspaceMode === "research") {
    return (
      <SotaWorkspacePanel
        backend={backend}
        pollMs={pollMs}
        onBackToEngineering={() => setWorkspaceMode("engineering")}
      />
    );
  }

  return (
    <div class="shell">
      <aside class="sidebar" aria-label="Projects">
        <h1 class="brand">TAKO</h1>
        <div class="row" style={{ marginBottom: "10px" }}>
          <span class="chip">Engineering</span>
          <button
            class="secondary"
            data-testid="open-research-workspace"
            onClick={() => setWorkspaceMode("research")}
          >
            Research (SOTA)
          </button>
        </div>
        <div class="backend-status-bar" data-testid="backend-status-bar" style={{ marginBottom: "12px" }}>
          <span class="chip" data-testid="core-mode">Local standalone core</span>
          <div data-testid="backend-status">
            {integration?.mode === "optional_backend"
              ? `T integration: ${integration.availability}`
              : "T integration: disabled (optional)"}
          </div>
          {integration?.mode === "optional_backend" ? (
            <div class="row">
              <button class="secondary" data-testid="refresh-integration" disabled={integrationPending} onClick={refreshIntegration}>Refresh T status</button>
              <button class="secondary" data-testid="disable-integration" onClick={() => configureIntegration(false)}>Disable T integration</button>
            </div>
          ) : (
            <button class="secondary" data-testid="enable-integration" disabled={integrationPending} onClick={() => configureIntegration(true)}>Enable optional T integration</button>
          )}
          {integrationPending && <p class="muted" data-testid="integration-progress">Checking optional integration… Local work remains available.</p>}
          {integrationError && <p class="muted" data-testid="integration-error">{integrationError}</p>}
          {governance && <p class="muted">Backend reported limit: ${governance.limits.spending_limit_usd.toFixed(2)}</p>}
        </div>
        {backendProjects.length > 0 && (
          <div style={{ marginBottom: "12px" }}>
            <span class="muted" style={{ fontSize: "12px", display: "block", marginBottom: "4px" }}>
              Registered T Projects ({backendProjects.length}):
            </span>
            <select
              data-testid="backend-projects-select"
              disabled={pending || integrationPending}
              style={{ width: "100%", padding: "6px", fontSize: "12px", background: "var(--bg-card, #2d3748)", color: "inherit", border: "1px solid var(--border, #4a5568)", borderRadius: "4px" }}
              onChange={(e) => {
                const val = (e.target as HTMLSelectElement).value;
                if (val) openProject("", val);
              }}
              value=""
            >
              <option value="" disabled>-- Select a T Project --</option>
              {backendProjects.map((p) => (
                <option key={p.project_id} value={p.project_id}>
                  {p.name} ({p.branch})
                </option>
              ))}
            </select>
          </div>
        )}
        {backendStatus?.workstations && backendStatus.workstations.length > 0 && (
          <div class="card" data-testid="workstations-card" style={{ marginBottom: "12px", padding: "10px" }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "6px" }}>
              <span style={{ fontSize: "12px", fontWeight: "bold" }}>Workstations (JACK)</span>
              <span class="muted" style={{ fontSize: "10px" }}>Read-Only</span>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: "6px" }}>
              {backendStatus.workstations.map((w) => (
                <div
                  key={w.workstation_id}
                  data-testid={`workstation-item-${w.workstation_id}`}
                  style={{
                    padding: "6px 8px",
                    background: "var(--bg, #1a202c)",
                    border: "1px solid var(--border, #2d3748)",
                    borderRadius: "4px",
                    fontSize: "11px",
                  }}
                >
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                    <span style={{ fontWeight: "600" }}>{w.display_name}</span>
                    <span
                      class="chip"
                      style={{
                        fontSize: "10px",
                        padding: "1px 6px",
                        background: w.connectivity_status === "connected" ? "#22543d" : "#742a2a",
                        color: w.connectivity_status === "connected" ? "#9ae6b4" : "#feb2b2",
                      }}
                    >
                      {w.connectivity_status === "connected" ? "● Online" : "○ Offline"}
                    </span>
                  </div>
                  <div class="muted" style={{ display: "flex", justifyContent: "space-between", marginTop: "2px" }}>
                    <span>{w.os} · {w.transport}</span>
                    <span>{w.is_local ? "Local" : w.host}</span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}
        <label class="field">
          <span>Project folder</span>
          <input
            data-testid="project-path"
            value={pathInput}
            placeholder="/path/to/git/project"
            onInput={(e) => setPathInput((e.target as HTMLInputElement).value)}
          />
        </label>
        <div class="row">
          <button data-testid="open-project" disabled={pending || !pathInput.trim()} onClick={() => openProject(pathInput.trim())}>
            Open project
          </button>
          {backend.pickFolder && (
            <button class="secondary" data-testid="browse" disabled={pending} onClick={browse}>
              Browse…
            </button>
          )}
        </div>
        {pv && (
          <section class="card" data-testid="project-card">
            <h2>Project</h2>
            <p class="mono" data-testid="project-name">{pv.project.path}</p>
            <p>
              Branch <b>{pv.project.branch}</b> · <span class="mono">{pv.project.head.slice(0, 8)}</span>
              {pv.project.dirty && <span class="chip warn" title="Uncommitted changes"> dirty</span>}
            </p>
            {isBackendProject && (
              <p>
                <span class="chip" data-testid="t-readonly-pill" style={{ background: "#2a4365", color: "#90cdf4" }}>
                  🔒 T Read-Only Mode
                </span>
              </p>
            )}
            <h2>Missions</h2>
            {pv.missions.length === 0 && <p class="muted">No missions yet.</p>}
            <ul class="missions">
              {pv.missions.map((s) => (
                <li key={s.id}>
                  <button class="link" data-testid={`mission-${s.id}`} disabled={pending} onClick={() => selectMission(s.id)}>
                    {s.goal} <span class="chip">{s.state ?? "unverified"}</span>
                    {s.state === null && <span class="muted"> Backend reports: {s.reported_backend_state ?? "unknown"}; evidence unavailable.</span>}
                  </button>
                </li>
              ))}
            </ul>
            {view && !isBackendProject && (
              <button class="secondary" data-testid="new-mission" disabled={pending} onClick={() => { setView(null); setConfirmReject(false); }}>
                New mission
              </button>
            )}
          </section>
        )}
      </aside>

      <main class="workspace">
        <nav class="stages" aria-label="Mission progress">
          {STAGES.map((s, i) => (
            <span key={s} class={`stage ${i < stage ? "done" : i === stage ? "current" : ""}`} aria-current={i === stage ? "step" : undefined}>
              {s}
            </span>
          ))}
        </nav>

        {startup && startup.recovered.length > 0 && (
          <Banner kind="warn" testid="recovered-banner">
            Recovered {startup.recovered.length} mission(s) interrupted by a crash:{" "}
            {startup.recovered.map((r) => `${r.mission_id} (${r.from} → ${r.to})`).join(", ")}. Partial work is kept for inspection.
          </Banner>
        )}
        {startup && startup.busy.length > 0 && (
          <Banner kind="info" testid="busy-banner">
            Another TAKO instance is working on: {startup.busy.join(", ")}.
          </Banner>
        )}
        {error && (
          <Banner kind="error" testid="error-banner">
            <b data-testid="error-code">{error.code}</b>: {error.message}
            {error.details.length > 0 && (
              <ul>{error.details.map((d) => <li class="mono" key={d}>{d}</li>)}</ul>
            )}
          </Banner>
        )}
        {remoteEvidence && (
          <Banner kind="warn" testid="backend-evidence">
            <b data-testid="backend-evidence-status">{remoteEvidence.assessment.outcome}</b>: {remoteEvidence.message}
            <p class="mono">{remoteEvidence.identity.mission_id}</p>
          </Banner>
        )}
        {view?.job.error && (
          <Banner kind="error" testid="job-error-banner">
            The last background operation failed: <b>{view.job.error.code}</b>: {view.job.error.message}
          </Banner>
        )}

        {!pv && (
          <section class="card empty" data-testid="empty-state">
            <h2>Open a Git project</h2>
            <p>Missions run against a project's committed state in a separate worktree. Your working tree changes only when you accept.</p>
          </section>
        )}

        {pv && !view && (
          <section class="card" data-testid="goal-form">
            <h2>1 · Define the goal</h2>
            <label class="field">
              <span>Objective</span>
              <textarea data-testid="goal" rows={2} value={goal} onInput={(e) => setGoal((e.target as HTMLTextAreaElement).value)} />
            </label>
            <label class="field">
              <span>Acceptance criteria (one per line)</span>
              <textarea data-testid="criteria" rows={2} value={criteria} onInput={(e) => setCriteria((e.target as HTMLTextAreaElement).value)} />
            </label>
            <label class="field">
              <span>Verification commands (one per line, run as <code>sh -c</code> on a clean checkout)</span>
              <textarea data-testid="checks" rows={2} value={checks} onInput={(e) => setChecks((e.target as HTMLTextAreaElement).value)} />
            </label>
            <button
              data-testid="create-mission"
              disabled={pending || !goal.trim() || lines(criteria).length === 0 || lines(checks).length === 0}
              onClick={() => createMission()}
            >
              Create mission
            </button>
          </section>
        )}

        {m && view && (
          <section class="card mission-head" data-testid="mission-card">
            <div class="row between">
              <h2 data-testid="mission-goal">{m.goal}</h2>
              <span class={`chip state-${m.state}${m.state === "evidence_ready" && !(ev?.acceptable) ? " not-ok" : ""}`} data-testid="state-chip">{m.state}</span>
            </div>
            <ul class="criteria">{m.acceptance_criteria.map((c) => <li key={c}>{c}</li>)}</ul>
            {m.state_reason && <p class="muted" data-testid="state-reason">{m.state_reason}</p>}
          </section>
        )}

        {view && can("define_plan") && (
          <section class="card" data-testid="plan-form" data-active={!view.plan}>
            <details open={!view.plan} class="revise">
            <summary><h2 class="inline">2 · {view.plan ? "Revise the plan" : "Describe the scripted agent"}</h2></summary>
            <fieldset class="agents">
              <legend>Agent</legend>
              <label><input type="radio" name="agent" checked readOnly /> Scripted agent: runs the script below, as you</label>
              <label class="disabled" title="Disabled until a sandbox above L0 exists (TAKO-005)">
                <input type="radio" name="agent" disabled data-testid="provider-radio" /> AI agent execution: <b>disabled</b>, needs L1 isolation
              </label>
            </fieldset>
            <label class="field">
              <span>Script (shell, runs in the isolated worktree)</span>
              <textarea data-testid="script" rows={4} class="mono" value={script} onInput={(e) => setScript((e.target as HTMLTextAreaElement).value)} />
            </label>
            <label class="field">
              <span>Files you expect it to change (one per line)</span>
              <textarea data-testid="expected" rows={2} value={expected} onInput={(e) => setExpected((e.target as HTMLTextAreaElement).value)} />
            </label>
            <label class="field">
              <span>Extra permissions to request, e.g. <code>read_env:NAME</code> (one per line)</span>
              <textarea data-testid="extra-perms" rows={1} value={extraPerms} onInput={(e) => setExtraPerms((e.target as HTMLTextAreaElement).value)} />
            </label>
            <button
              data-testid="define-plan"
              disabled={pending || !script.trim()}
              onClick={() =>
                act("define_plan", {
                  mission_id: m!.id,
                  script,
                  expected_files: lines(expected),
                  extra_permissions: lines(extraPerms),
                })
              }
            >
              Define plan
            </button>
            </details>
          </section>
        )}

        {view?.plan && (
          <section class="card" data-testid="plan-card" data-active={can("approve_plan")}>
            <h2>Plan v{view.plan.version}</h2>
            <p data-testid="plan-summary">{view.plan.body.summary}</p>
            <h3>Script that will run (verbatim)</h3>
            <pre class="mono" data-testid="plan-script">{view.plan.body.steps.map((s) => s.replace(/^script:/, "")).join("\n")}</pre>
            {view.plan.body.files_expected.length > 0 && <p>Expected files: <span class="mono">{view.plan.body.files_expected.join(", ")}</span></p>}
            <ul class="risks">{view.plan.body.risks.map((r) => <li key={r}>⚠ {r}</li>)}</ul>
            <h3>Requested permissions</h3>
            <ul class="perms" data-testid="perms">
              {view.plan.body.requested_permissions.map((p) => {
                const granted = m?.approval?.granted.includes(p);
                const approvable = can("approve_plan");
                return (
                  <li key={p}>
                    <label>
                      <input
                        type="checkbox"
                        data-testid={`perm-${p}`}
                        checked={approvable ? grants.includes(p) : !!granted}
                        disabled={!approvable}
                        onChange={(e) => {
                          const on = (e.target as HTMLInputElement).checked;
                          setGrants((g) => (on ? [...g, p] : g.filter((x) => x !== p)));
                        }}
                      />{" "}
                      <span class="mono">{p}</span>
                    </label>
                  </li>
                );
              })}
            </ul>
            {can("approve_plan") && (
              <>
                <p class="muted">Nothing is granted until you tick it. The run is refused without <code>fs_write_workspace</code> and <code>exec_command:sh</code>.</p>
                {grants.length === 0 && (
                  <p class="warn-text" data-testid="zero-grants-warning">
                    No permissions ticked: approving will record the plan, but the run will be refused until you grant them.
                  </p>
                )}
                <button
                  data-testid="approve"
                  class={grants.length === 0 ? "secondary" : ""}
                  disabled={pending}
                  onClick={() =>
                    act("approve_plan", {
                      mission_id: m!.id,
                      version: view.plan!.version,
                      plan_hash: view.plan!.hash,
                      granted: view.plan!.body.requested_permissions.filter((p) => grants.includes(p)),
                    })
                  }
                >
                  Approve plan v{view.plan.version} with {grants.length} permission(s)
                </button>
              </>
            )}
            {m?.approval && (
              <p data-testid="approval-line" class="muted">Approved by {m.approval.approved_by}; granted: {m.approval.granted.join(", ") || "nothing"}.</p>
            )}
          </section>
        )}

        {view && (can("start_execution") || can("cancel") || view.execution) && (
          <section class="card" data-testid="run-card" data-active={can("start_execution") || can("cancel") || can("recover")}>
            <h2>3 · Run</h2>
            <div class="row">
              {can("start_execution") && (
                <button data-testid="start-run" disabled={pending} onClick={() => act("start_execution", { mission_id: m!.id, kind: "scripted" })}>
                  Start run
                </button>
              )}
              {can("cancel") && (
                <button class="danger" data-testid="cancel" disabled={pending} onClick={() => act("cancel", { mission_id: m!.id })}>
                  Cancel
                </button>
              )}
              {can("recover") && (
                <button class="secondary" data-testid="recover" disabled={pending} onClick={() => act("recover", { mission_id: m!.id })}>
                  Recover mission
                </button>
              )}
            </div>
            {view.job.running && <p data-testid="running" class="muted">Running ({view.job.kind})…</p>}
            {view.execution && (
              <div data-testid="execution">
                <p>
                  Agent <b data-testid="exec-status">{view.execution.status}</b>
                  {view.execution.exit_code !== null && <> · exit code {view.execution.exit_code}</>} · {view.execution.duration_ms} ms · isolation{" "}
                  <b>{view.execution.isolation_level}</b>
                  {view.execution.project_touched && <span class="chip warn"> project touched</span>}
                </p>
                {view.execution.note && <p class="muted">{view.execution.note}</p>}
                <h3>Agent output <span class="muted">(the agent's own words, not evidence)</span></h3>
                <pre class="mono log" data-testid="agent-stdout">{view.execution.stdout_tail || "(no output)"}</pre>
                {view.execution.stderr_tail && <pre class="mono log err">{view.execution.stderr_tail}</pre>}
                <p>Changed: <span class="mono">{view.execution.files_changed.join(", ") || "nothing"}</span></p>
              </div>
            )}
          </section>
        )}

        {view && (can("start_verification") || ev) && (
          <section class="card" data-testid="evidence-card" data-active={can("start_verification") && !ev}>
            <h2>4 · Verification evidence</h2>
            {can("start_verification") && (
              <button data-testid="verify" disabled={pending} onClick={() => act("start_verification", { mission_id: m!.id })}>
                {ev ? "Verify again" : "Run verification"}
              </button>
            )}
            {!ev && <p class="muted">No evidence yet. The agent's output above is not verification.</p>}
            {ev && (
              <div data-testid="evidence">
                <p>
                  <b data-testid="evidence-result" class={ev.result === "pass" ? "ok" : "bad"}>{ev.result.toUpperCase()}</b> ·{" "}
                  <span data-testid="evidence-validity">{ev.valid ? "evidence intact" : "EVIDENCE INVALID"}</span> ·{" "}
                  <span class="mono" data-testid="evidence-id">{ev.id}</span>
                </p>
                {ev.integrity_violations.length > 0 && (
                  <p class="bad" data-testid="integrity">Integrity violations: {ev.integrity_violations.join(", ")}</p>
                )}
                {ev.failed_checks.length > 0 && <p class="bad" data-testid="failed-checks">Failed checks: {ev.failed_checks.map((i) => i + 1).join(", ")}</p>}
                <table class="checks">
                  <thead><tr><th>#</th><th>Command</th><th>Exit</th><th>ms</th></tr></thead>
                  <tbody>
                    {ev.checks.map((c, i) => (
                      <tr key={i}>
                        <td>{i + 1}</td>
                        <td class="mono">{c.command.slice(2).join(" ") || c.command.join(" ")}</td>
                        <td data-testid={`check-exit-${i}`}>{c.timed_out ? "timeout" : c.exit_code}</td>
                        <td>{c.duration_ms}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {ev.checks.map((c, i) => (c.stdout_tail || c.stderr_tail) ? (
                  <details key={i}><summary>Check {i + 1} output</summary><pre class="mono log">{c.stdout_tail}{c.stderr_tail}</pre></details>
                ) : null)}
                <p class="muted mono">tree {ev.subject_tree_hash.slice(0, 10)} → before {ev.tree_hash_before_verify.slice(0, 10)} / after {ev.tree_hash_after_verify.slice(0, 10)}</p>
                <details>
                  <summary>Limitations</summary>
                  <ul>{ev.limitations.map((l) => <li key={l}>{l}</li>)}</ul>
                </details>
              </div>
            )}
          </section>
        )}

        {view?.diff && view.diff.text.trim() !== "" && (
          <section class="card" data-testid="diff-card">
            <h2>Changes <span class="muted">(git diff base → candidate)</span></h2>
            <pre class="diff mono" data-testid="diff">
              {view.diff.text.split("\n").map((l, i) => (
                <span key={i} class={l.startsWith("+") && !l.startsWith("+++") ? "add" : l.startsWith("-") && !l.startsWith("---") ? "del" : ""}>{l + "\n"}</span>
              ))}
            </pre>
            {view.diff.truncated && <p class="muted">Diff truncated.</p>}
          </section>
        )}

        {view && (can("accept") || can("reject") || view.decision) && (
          <section class="card" data-testid="decision-card" data-active={can("accept") || (can("reject") && m?.state !== "sealed")}>
            <h2>5 · Your decision</h2>
            {ev && (
              <p data-testid="decision-evidence-summary">
                Evidence <span class="mono">{ev.id}</span>:{" "}
                <b class={ev.result === "pass" && ev.valid ? "ok" : "bad"}>{ev.result.toUpperCase()}</b>
                {ev.valid ? ", intact" : ", INVALID (" + ev.integrity_violations.join(", ") + ")"} ·{" "}
                {ev.checks.length - ev.failed_checks.length}/{ev.checks.length} checks passed · changes:{" "}
                <span class="mono">{ev.files_changed.join(", ") || "none"}</span>. Details are below.
              </p>
            )}
            {(can("accept") || can("reject")) && (
              <label class="field"><span>Note (optional)</span><input data-testid="note" value={note} onInput={(e) => setNote((e.target as HTMLInputElement).value)} /></label>
            )}
            <div class="row">
              {can("accept") && (
                <button data-testid="accept" disabled={pending} onClick={() => act("accept", { mission_id: m!.id, note })}>
                  Accept change
                </button>
              )}
              {can("reject") && !confirmReject && (
                <button class="danger" data-testid="reject" disabled={pending} onClick={() => setConfirmReject(true)}>Reject…</button>
              )}
              {can("reject") && confirmReject && (
                <>
                  <button class="danger" data-testid="confirm-reject" disabled={pending} onClick={() => act("reject", { mission_id: m!.id, note }).then(() => setConfirmReject(false))}>
                    Confirm: discard this workspace
                  </button>
                  <button class="secondary" onClick={() => setConfirmReject(false)}>Keep it</button>
                </>
              )}
            </div>
            {!can("accept") && view.mission.state === "evidence_ready" && <p class="muted" data-testid="accept-blocked">Accepting is blocked: the evidence is not acceptable.</p>}
            {view.decision && (
              <p data-testid="decision">
                <b>{view.decision.decision === "accept" ? "Accepted" : "Rejected"}</b> by {view.decision.decided_by}
                {view.decision.evidence_id && <> on evidence <span class="mono">{view.decision.evidence_id}</span></>}.
                {view.decision.resulting_head && <> Project now at <span class="mono">{view.decision.resulting_head.slice(0, 8)}</span>.</>}
              </p>
            )}
          </section>
        )}
        <Proposals backend={backend} projectPath={pv?.source === "local" ? pv.project.path : undefined} />
      </main>

      <aside class="companion" aria-label="Tako companion">
        <Tako pose={say.pose} />
        <div class="bubble" data-testid="companion" data-pose={say.pose}>
          <p class="headline" data-testid="companion-headline">{say.headline}</p>
          <p data-testid="companion-detail">{say.detail}</p>
          <p class="next" data-testid="companion-next"><b>Next:</b> {say.next}</p>
          {say.evidenceId && <p class="mono" data-testid="companion-evidence">Evidence: {say.evidenceId}</p>}
        </div>
      </aside>

      <footer class="status" data-testid="status-bar">
        <span class="chip warn" data-testid="isolation-badge">Isolation L0 · not a security sandbox</span>
        <span>AI agent execution: <b>disabled</b> (needs L1); proposals only</span>
        <span class="muted">{view?.isolation.notice ?? pv?.isolation.notice ?? "Scripts run as your user. Only run what you trust."}</span>
      </footer>
    </div>
  );

  async function createMission() {
    if (!pv) return;
    const v = await run(() =>
      backend.call("create_mission", { project_path: pv.project.path, goal, criteria: lines(criteria), checks: lines(checks) }),
    );
    if (v) {
      setView(v);
      setGoal(""); // the next mission starts with an empty objective
      const r = await backend.call("open_project", { path: pv.project.path }).catch(() => null);
      if (r) setPv(r);
    }
  }
}

function Banner({ kind, testid, children }: { kind: "error" | "warn" | "info"; testid: string; children: ComponentChildren }) {
  return <div class={`banner ${kind}`} role={kind === "error" ? "alert" : "status"} data-testid={testid}>{children}</div>;
}
