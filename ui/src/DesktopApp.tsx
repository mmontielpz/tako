import { Component, type ComponentChildren } from "preact";
import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import { App } from "./App";
import type { Backend } from "./backend";
import { completeOnboarding, getOnboardingState, openEngineeringWorkspace } from "./onboarding";
import type { StartupReport } from "./types";
import { OnboardingHeader, WelcomeWizard } from "./WelcomeWizard";

class EngineeringBoundary extends Component<{ children: ComponentChildren; onFailure(): void }, { failed: boolean }> {
  override state = { failed: false };
  static override getDerivedStateFromError() { return { failed: true }; }
  override componentDidCatch() { this.props.onFailure(); }
  override render() { return this.state.failed ? null : this.props.children; }
}

/** This handshake runs only in the committed Engineering subtree, never on a route flag. */
function EngineeringMount({ backend, onReady }: { backend: Backend; onReady(): void }) {
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!root.current?.querySelector(".shell")) return;
    const heading = root.current.querySelector<HTMLElement>("h1");
    heading?.setAttribute("tabindex", "-1");
    heading?.focus();
    onReady();
  }, [onReady]);
  return <div ref={root} data-testid="engineering-mounted"><App backend={backend} /></div>;
}

export function DesktopApp({ backend, readTimeoutMs = 5000 }: { backend: Backend; readTimeoutMs?: number }) {
  const [route, setRoute] = useState<"loading" | "welcome" | "choose" | "engineering">("loading");
  const [selected, setSelected] = useState(false);
  const [pending, setPending] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [retryRead, setRetryRead] = useState(false);
  const [saveError, setSaveError] = useState(false);
  const [saving, setSaving] = useState(false);
  const [startup, setStartup] = useState<StartupReport | null>(null);
  const [startupFailed, setStartupFailed] = useState(false);
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
    if (!alive.current || !ready.current || currentRoute.current !== "engineering" || !needsCompletion.current || savingRef.current) return;
    const token = generation.current;
    savingRef.current = true; setSaving(true);
    try {
      const result = await completeOnboarding(backend);
      if (result.status !== "completed" || result.last_workspace !== "engineering") throw new Error("Invalid acknowledgement");
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

  const engineeringReady = useCallback(() => {
    if (!alive.current || currentRoute.current !== "engineering") return;
    ready.current = true;
    if (attempted.current !== generation.current) {
      attempted.current = generation.current;
      void savePreference();
    }
  }, [savePreference]);

  const enterEngineering = useCallback(async () => {
    if (!alive.current || opening.current) return;
    const token = ++generation.current;
    opening.current = true; ready.current = false;
    setPending(true); setError(null); setRetryRead(false);
    try {
      const result = await openEngineeringWorkspace(backend);
      if (result.workspace !== "engineering" || result.opened !== true) throw new Error("Workspace unavailable");
      if (!alive.current || token !== generation.current) return;
      navigate("engineering");
    } catch {
      if (!alive.current || token !== generation.current) return;
      setSelected(true);
      setError("Engineering could not open. Try again or choose another workspace.");
      navigate("choose");
    } finally {
      if (alive.current && token === generation.current) {
        opening.current = false; setPending(false);
      }
    }
  }, [backend, navigate]);

  const readPreference = useCallback(async () => {
    if (readTimer.current !== null) clearTimeout(readTimer.current);
    const token = ++generation.current;
    opening.current = false; ready.current = false; needsCompletion.current = true;
    setSelected(false); setPending(false); setError(null); setRetryRead(false); setRestoring(false);
    navigate("loading");
    function unavailable(message: string) {
      if (!alive.current || token !== generation.current) return;
      ++generation.current;
      setError(message); setRetryRead(true); setRestoring(true); navigate("choose");
    }
    const timer = setTimeout(() => unavailable("Reading your local start preference is taking longer than expected. Choose a workspace or retry."), readTimeoutMs);
    readTimer.current = timer;
    try {
      const state = await getOnboardingState(backend);
      if (!alive.current || token !== generation.current) return;
      clearTimeout(timer);
      if (state.status === "first_run") navigate("welcome");
      else if (state.status === "completed" && state.last_workspace === "engineering") {
        needsCompletion.current = false; setRestoring(true);
        await enterEngineering();
      } else unavailable("Your previous workspace could not be restored. The local start preference is unavailable; choose a workspace to continue.");
    } catch {
      unavailable("Your previous workspace could not be restored. TAKO could not read your local start preference; choose a workspace to continue.");
    } finally { clearTimeout(timer); if (readTimer.current === timer) readTimer.current = null; }
  }, [backend, enterEngineering, navigate, readTimeoutMs]);

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
    setPending(false); setSelected(true); setRetryRead(false);
    setError("Engineering could not open. Try again or choose another workspace.");
    navigate("choose");
  }, [navigate]);

  const notice = startupFailed ? "Startup recovery information is unavailable. Opening a workspace does not restart work." : startup && (startup.recovered.length || startup.busy.length || startup.errors.length)
    ? `Startup recovery: ${startup.recovered.length} recovered, ${startup.busy.length} busy, ${startup.errors.length} error(s). Open Engineering to inspect local work; nothing is restarted automatically.` : null;

  if (route === "engineering") return <div class="desktop-entry">
    <div class="onboarding-preference" aria-live="polite">{saveError && <>
      <p role="status" data-testid="onboarding-save-error">TAKO could not remember your start preference. Your workspace remains available; existing metadata was preserved.</p>
      <button data-testid="retry-onboarding-save" disabled={saving} onClick={() => void savePreference()}>{saving ? "Saving start preference…" : "Retry saving preference"}</button>
    </>}</div>
    <EngineeringBoundary key={generation.current} onFailure={renderFailed}>
      <EngineeringMount backend={backend} onReady={engineeringReady} />
    </EngineeringBoundary>
  </div>;

  if (route === "loading") return <div class="onboarding" data-testid="onboarding-loading"><OnboardingHeader />
    {notice && <p class="onboarding-notice" role="status">{notice}</p>}
    <main class="onboarding-loading"><p role="status">{pending ? "Opening Engineering…" : "Opening TAKO…"}</p>
      {pending && <button class="onboarding-secondary" data-testid="cancel-workspace-restore" onClick={() => {
        ++generation.current; opening.current = false; setPending(false); setSelected(false); setRestoring(true); navigate("choose");
      }}>Choose a workspace</button>}
    </main>
  </div>;

  return <WelcomeWizard screen={route} selected={selected} pending={pending} restoring={restoring} error={error} notice={notice} canRetryRead={retryRead}
    onContinue={() => { ++generation.current; setError(null); setRetryRead(false); setRestoring(false); navigate("choose"); }}
    onBack={() => { ++generation.current; opening.current = false; setPending(false); setError(null); setRetryRead(false); navigate("welcome"); }}
    onSelect={() => setSelected(true)} onOpen={() => { if (selected) void enterEngineering(); }} onRetryRead={() => void readPreference()} />;
}
