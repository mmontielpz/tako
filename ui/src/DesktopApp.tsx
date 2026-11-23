import { Component, type ComponentChildren } from "preact";
import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import { BridgeError, type Backend } from "./backend";
import { completeOnboarding, getOnboardingState, openEngineeringWorkspace } from "./onboarding";
import { pythonSotaBridge } from "./sotaBridge";
import { SotaWorkspacePanel } from "./SotaWorkspacePanel";
import type { StartupReport } from "./types";
import { OnboardingHeader, WelcomeWizard } from "./WelcomeWizard";

class ResearchBoundary extends Component<{ children: ComponentChildren; onFailure(): void }, { failed: boolean }> {
  override state = { failed: false };
  static override getDerivedStateFromError() { return { failed: true }; }
  override componentDidCatch() { this.props.onFailure(); }
  override render() { return this.state.failed ? null : this.props.children; }
}

function ResearchMount({
  backend,
  workspacePath,
  topic,
  objective,
  onReady,
}: {
  backend: Backend;
  workspacePath: string;
  topic: string;
  objective: string;
  onReady(): void;
}) {
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!root.current?.querySelector(".shell")) return;
    const heading = root.current.querySelector<HTMLElement>("h1");
    heading?.setAttribute("tabindex", "-1");
    heading?.focus();
    onReady();
  }, [onReady]);
  return (
    <div ref={root} data-testid="research-mounted">
      <SotaWorkspacePanel
        backend={backend}
        initialWorkspacePath={workspacePath}
        initialTopic={topic}
        initialObjective={objective}
      />
    </div>
  );
}

export function DesktopApp({ backend, readTimeoutMs = 5000 }: { backend: Backend; readTimeoutMs?: number }) {
  const [route, setRoute] = useState<"loading" | "welcome" | "choose" | "research">("loading");
  const [pending, setPending] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [retryRead, setRetryRead] = useState(false);
  const [saveError, setSaveError] = useState(false);
  const [saving, setSaving] = useState(false);
  const [startup, setStartup] = useState<StartupReport | null>(null);
  const [startupFailed, setStartupFailed] = useState(false);

  const [wsPath, setWsPath] = useState("");
  const [topic, setTopic] = useState("State of the Art Survey");
  const [objective, setObjective] = useState("Identify foundational works and evidence");

  const alive = useRef(true);
  const generation = useRef(0);
  const currentRoute = useRef(route);
  const opening = useRef(false);
  const needsCompletion = useRef(true);
  const savingRef = useRef(false);
  const ready = useRef(false);
  const attempted = useRef(-1);
  const readTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  currentRoute.current = route;

  const navigate = useCallback((next: typeof route) => {
    currentRoute.current = next;
    setRoute(next);
  }, []);

  const savePreference = useCallback(async () => {
    if (!alive.current || !ready.current || currentRoute.current !== "research" || !needsCompletion.current || savingRef.current) return;
    const token = generation.current;
    savingRef.current = true; setSaving(true);
    try {
      await openEngineeringWorkspace(backend);
      const result = await completeOnboarding(backend);
      if (result.status !== "completed") throw new Error("Invalid acknowledgement");
      if (!alive.current || token !== generation.current) return;
      needsCompletion.current = false;
      setSaveError(false);
    } catch {
      if (alive.current && token === generation.current) setSaveError(true);
    } finally {
      savingRef.current = false;
      if (alive.current && token === generation.current) setSaving(false);
    }
  }, [backend]);

  const researchReady = useCallback(() => {
    if (!alive.current || currentRoute.current !== "research") return;
    ready.current = true;
    if (attempted.current !== generation.current) {
      attempted.current = generation.current;
      void savePreference();
    }
  }, [savePreference]);

  const enterResearch = useCallback(async (mode: "direct" | "create" | "open") => {
    if (!alive.current || opening.current) return;
    const token = ++generation.current;
    opening.current = true; ready.current = false;
    setPending(true); setError(null); setRetryRead(false);
    try {
      const sota = pythonSotaBridge(backend);
      if (mode === "create") {
        await sota.wsInit({
          workspace_path: wsPath.trim(),
          topic: topic.trim(),
          objective: objective.trim(),
        });
      } else if (mode === "open") {
        await sota.wsStatus({ workspace_path: wsPath.trim() });
      }
      if (!alive.current || token !== generation.current) return;
      navigate("research");
    } catch (e) {
      if (!alive.current || token !== generation.current) return;
      setError(e instanceof BridgeError ? `${e.code}: ${e.message}` : "Research workspace could not be opened.");
      navigate("choose");
    } finally {
      if (alive.current && token === generation.current) {
        opening.current = false; setPending(false);
      }
    }
  }, [backend, navigate, objective, topic, wsPath]);

  const readPreference = useCallback(async () => {
    if (readTimer.current !== null) clearTimeout(readTimer.current);
    const token = ++generation.current;
    opening.current = false; ready.current = false; needsCompletion.current = true;
    setPending(false); setError(null); setRetryRead(false); setRestoring(false);
    navigate("loading");
    function unavailable(message: string) {
      if (!alive.current || token !== generation.current) return;
      ++generation.current;
      setError(message); setRetryRead(true); setRestoring(true); navigate("choose");
    }
    const timer = setTimeout(() => unavailable("Reading your local start preference is taking longer than expected. Set up a workspace or retry."), readTimeoutMs);
    readTimer.current = timer;
    try {
      const state = await getOnboardingState(backend);
      if (!alive.current || token !== generation.current) return;
      clearTimeout(timer);
      if (state.status === "first_run") navigate("welcome");
      else if (state.status === "completed") {
        needsCompletion.current = false; setRestoring(true);
        navigate("research");
      } else unavailable("Your previous session could not be restored automatically. Set up or open a research workspace to continue.");
    } catch {
      unavailable("TAKO could not read your local start preference. Set up or open a research workspace to continue.");
    } finally { clearTimeout(timer); if (readTimer.current === timer) readTimer.current = null; }
  }, [backend, navigate, readTimeoutMs]);

  useEffect(() => {
    alive.current = true;
    let active = true;
    void backend.call("startup_report", undefined).then(report => {
      if (active) setStartup(report);
    }, () => { if (active) setStartupFailed(true); });
    void readPreference();
    return () => { active = false; alive.current = false; ++generation.current; if (readTimer.current !== null) clearTimeout(readTimer.current); };
  }, [backend, readPreference]);

  const renderFailed = useCallback(() => {
    ++generation.current; ready.current = false; opening.current = false;
    setPending(false); setRetryRead(false);
    setError("The research workspace could not be displayed. Please try again.");
    navigate("choose");
  }, [navigate]);

  const notice = startupFailed ? "Startup recovery information is unavailable." : startup && (startup.recovered.length || startup.busy.length || startup.errors.length)
    ? `Startup recovery: ${startup.recovered.length} recovered, ${startup.busy.length} busy, ${startup.errors.length} error(s).` : null;

  if (route === "research") return <div class="desktop-entry">
    <div class="onboarding-preference" aria-live="polite">{saveError && <>
      <p role="status" data-testid="onboarding-save-error">TAKO could not remember your start preference. Your research workspace remains available.</p>
      <button data-testid="retry-onboarding-save" disabled={saving} onClick={() => void savePreference()}>{saving ? "Saving start preference…" : "Retry saving preference"}</button>
    </>}</div>
    <ResearchBoundary key={generation.current} onFailure={renderFailed}>
      <ResearchMount
        backend={backend}
        workspacePath={wsPath.trim()}
        topic={topic.trim()}
        objective={objective.trim()}
        onReady={researchReady}
      />
    </ResearchBoundary>
  </div>;

  if (route === "loading") return <div class="onboarding" data-testid="onboarding-loading"><OnboardingHeader />
    {notice && <p class="onboarding-notice" role="status">{notice}</p>}
    <main class="onboarding-loading"><p role="status">{pending ? "Opening research environment…" : "Opening TAKO…"}</p>
      {pending && <button class="onboarding-secondary" data-testid="cancel-workspace-restore" onClick={() => {
        ++generation.current; opening.current = false; setPending(false); setRestoring(true); navigate("choose");
      }}>Set up workspace</button>}
    </main>
  </div>;

  return <WelcomeWizard
    screen={route}
    pending={pending}
    restoring={restoring}
    error={error}
    notice={notice}
    canRetryRead={retryRead}
    workspacePath={wsPath}
    topic={topic}
    objective={objective}
    onWorkspacePathChange={setWsPath}
    onTopicChange={setTopic}
    onObjectiveChange={setObjective}
    onPickFolder={backend.pickFolder ? () => {
      void backend.pickFolder!().then(p => { if (p) setWsPath(p); });
    } : undefined}
    onContinue={() => { ++generation.current; setError(null); setRetryRead(false); setRestoring(false); navigate("choose"); }}
    onSkip={() => { ++generation.current; setError(null); setRetryRead(false); setRestoring(false); navigate("choose"); }}
    onBack={() => { ++generation.current; opening.current = false; setPending(false); setError(null); setRetryRead(false); navigate("welcome"); }}
    onCreateWorkspace={() => void enterResearch("create")}
    onOpenWorkspace={() => void enterResearch("open")}
    onEnterDirectly={() => void enterResearch("direct")}
    onRetryRead={() => void readPreference()}
  />;
}
