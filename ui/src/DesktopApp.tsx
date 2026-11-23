import { Component, type ComponentChildren } from "preact";
import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import { BridgeError, type Backend } from "./backend";
import {
  getOnboardingState,
  readResearchSession,
  saveResearchSession,
} from "./onboarding";
import { pythonSotaBridge } from "./sotaBridge";
import { SotaWorkspacePanel } from "./SotaWorkspacePanel";
import type { StartupReport } from "./types";
import {
  OnboardingHeader,
  WelcomeWizard,
  type OnboardingViewState,
} from "./WelcomeWizard";

type AppRoute = "loading" | OnboardingViewState | "ACTIVE_RESEARCH";

class ResearchBoundary extends Component<
  { children: ComponentChildren; onFailure(): void },
  { failed: boolean }
> {
  override state = { failed: false };
  static override getDerivedStateFromError() {
    return { failed: true };
  }
  override componentDidCatch() {
    this.props.onFailure();
  }
  override render() {
    return this.state.failed ? null : this.props.children;
  }
}

function ResearchMount({
  backend,
  workspacePath,
  topic,
  objective,
  onReturnHome,
  onWorkspaceActive,
  onReady,
}: {
  backend: Backend;
  workspacePath: string;
  topic: string;
  objective: string;
  onReturnHome(): void;
  onWorkspaceActive(info: { path: string; topic?: string; objective?: string }): void;
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
        onReturnHome={onReturnHome}
        onWorkspaceActive={onWorkspaceActive}
      />
    </div>
  );
}

export function DesktopApp({
  backend,
  readTimeoutMs = 5000,
}: {
  backend: Backend;
  readTimeoutMs?: number;
}) {
  const initialSession = readResearchSession();
  const [route, setRoute] = useState<AppRoute>("loading");
  const [hasCompletedBefore, setHasCompletedBefore] = useState(initialSession.completed);
  const [lastWorkspacePath, setLastWorkspacePath] = useState<string | null>(
    initialSession.lastWorkspacePath,
  );
  const [lastTopic, setLastTopic] = useState<string | null>(initialSession.lastTopic);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [retryRead, setRetryRead] = useState(false);
  const [startup, setStartup] = useState<StartupReport | null>(null);
  const [startupFailed, setStartupFailed] = useState(false);

  const [wsPath, setWsPath] = useState(initialSession.lastWorkspacePath ?? "");
  const [topic, setTopic] = useState(initialSession.lastTopic ?? "State of the Art Survey");
  const [objective, setObjective] = useState(
    initialSession.lastObjective ?? "Identify foundational works and evidence",
  );

  const alive = useRef(true);
  const generation = useRef(0);
  const opening = useRef(false);
  const readTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const navigate = useCallback((next: AppRoute) => {
    setRoute(next);
  }, []);

  const persistWorkspace = useCallback(
    (info: { path: string; topic?: string; objective?: string }) => {
      const saved = saveResearchSession({
        completed: true,
        lastWorkspacePath: info.path || null,
        lastTopic: info.topic ?? null,
        lastObjective: info.objective ?? null,
      });
      setHasCompletedBefore(saved.completed);
      setLastWorkspacePath(saved.lastWorkspacePath);
      setLastTopic(saved.lastTopic);
    },
    [],
  );

  const enterResearch = useCallback(
    async (mode: "direct" | "create" | "open" | "resume") => {
      if (!alive.current || opening.current) return;
      const token = ++generation.current;
      opening.current = true;
      setPending(true);
      setError(null);
      setRetryRead(false);
      const targetPath = (mode === "resume" ? lastWorkspacePath ?? "" : wsPath).trim();
      try {
        const sota = pythonSotaBridge(backend);
        if (mode === "create") {
          await sota.wsInit({
            workspace_path: targetPath,
            topic: topic.trim(),
            objective: objective.trim(),
          });
          persistWorkspace({
            path: targetPath,
            topic: topic.trim(),
            objective: objective.trim(),
          });
        } else if (mode === "open" || mode === "resume") {
          const st = await sota.wsStatus({ workspace_path: targetPath });
          const data = (st.data ?? {}) as Record<string, unknown>;
          const loadedTopic = typeof data.topic === "string" ? data.topic : topic;
          const loadedObj = typeof data.objective === "string" ? data.objective : objective;
          setWsPath(targetPath);
          setTopic(loadedTopic);
          setObjective(loadedObj);
          persistWorkspace({ path: targetPath, topic: loadedTopic, objective: loadedObj });
        } else {
          const saved = saveResearchSession({ completed: true });
          setHasCompletedBefore(saved.completed);
        }
        if (!alive.current || token !== generation.current) return;
        navigate("ACTIVE_RESEARCH");
      } catch (e) {
        if (!alive.current || token !== generation.current) return;
        setError(
          e instanceof BridgeError
            ? `${e.code}: ${e.message}`
            : "Research workspace could not be opened.",
        );
        navigate(mode === "resume" ? "RETURNING_USER" : mode === "create" ? "CREATE_RESEARCH" : "OPEN_RESEARCH");
      } finally {
        if (alive.current && token === generation.current) {
          opening.current = false;
          setPending(false);
        }
      }
    },
    [backend, lastWorkspacePath, navigate, objective, persistWorkspace, topic, wsPath],
  );

  const readPreference = useCallback(async () => {
    if (readTimer.current !== null) clearTimeout(readTimer.current);
    const token = ++generation.current;
    opening.current = false;
    setPending(false);
    setError(null);
    setRetryRead(false);
    navigate("loading");

    const session = readResearchSession();
    const timer = setTimeout(() => {
      if (!alive.current || token !== generation.current) return;
      ++generation.current;
      setRetryRead(true);
      navigate(session.completed ? "RETURNING_USER" : "FIRST_RUN");
    }, readTimeoutMs);
    readTimer.current = timer;

    try {
      const nativeState = await getOnboardingState(backend);
      if (!alive.current || token !== generation.current) return;
      clearTimeout(timer);
      // Preserve backward compatibility with already-completed onboarding-v1.json.
      const completed = session.completed || nativeState.status === "completed";
      if (completed && !session.completed) {
        saveResearchSession({ completed: true });
      }
      setHasCompletedBefore(completed);
      setLastWorkspacePath(session.lastWorkspacePath);
      setLastTopic(session.lastTopic);
      navigate(completed ? "RETURNING_USER" : "FIRST_RUN");
    } catch {
      if (!alive.current || token !== generation.current) return;
      clearTimeout(timer);
      navigate(session.completed ? "RETURNING_USER" : "FIRST_RUN");
    } finally {
      clearTimeout(timer);
      if (readTimer.current === timer) readTimer.current = null;
    }
  }, [backend, navigate, readTimeoutMs]);

  useEffect(() => {
    alive.current = true;
    let active = true;
    void backend.call("startup_report", undefined).then(
      (report) => {
        if (active) setStartup(report);
      },
      () => {
        if (active) setStartupFailed(true);
      },
    );
    void readPreference();
    return () => {
      active = false;
      alive.current = false;
      ++generation.current;
      if (readTimer.current !== null) clearTimeout(readTimer.current);
    };
  }, [backend, readPreference]);

  const renderFailed = useCallback(() => {
    ++generation.current;
    opening.current = false;
    setPending(false);
    setRetryRead(false);
    setError("The research workspace could not be displayed. Please try again.");
    navigate(hasCompletedBefore ? "RETURNING_USER" : "FIRST_RUN");
  }, [hasCompletedBefore, navigate]);

  const notice = startupFailed
    ? "Startup recovery information is unavailable."
    : startup && (startup.recovered.length || startup.busy.length || startup.errors.length)
      ? `Startup recovery: ${startup.recovered.length} recovered, ${startup.busy.length} busy, ${startup.errors.length} error(s).`
      : null;

  if (route === "ACTIVE_RESEARCH") {
    return (
      <div class="desktop-entry">
        <ResearchBoundary key={generation.current} onFailure={renderFailed}>
          <ResearchMount
            backend={backend}
            workspacePath={wsPath.trim()}
            topic={topic.trim()}
            objective={objective.trim()}
            onReturnHome={() => {
              setError(null);
              navigate("RETURNING_USER");
            }}
            onWorkspaceActive={persistWorkspace}
            onReady={() => {}}
          />
        </ResearchBoundary>
      </div>
    );
  }

  if (route === "loading") {
    return (
      <div class="onboarding" data-testid="onboarding-loading">
        <OnboardingHeader />
        {notice && (
          <p class="onboarding-notice" role="status">
            {notice}
          </p>
        )}
        <main class="onboarding-loading">
          <p role="status">
            {pending ? "Opening research workspace…" : "Opening TAKO…"}
          </p>
        </main>
      </div>
    );
  }

  return (
    <WelcomeWizard
      view={route}
      pending={pending}
      error={error}
      notice={notice}
      canRetryRead={retryRead}
      lastWorkspacePath={lastWorkspacePath}
      lastTopic={lastTopic}
      workspacePath={wsPath}
      topic={topic}
      objective={objective}
      onWorkspacePathChange={setWsPath}
      onTopicChange={setTopic}
      onObjectiveChange={setObjective}
      onPickFolder={
        backend.pickFolder
          ? () => {
              void backend.pickFolder!().then((p) => {
                if (p) setWsPath(p);
              });
            }
          : undefined
      }
      onGoCreate={() => {
        setError(null);
        navigate("CREATE_RESEARCH");
      }}
      onGoOpen={() => {
        setError(null);
        navigate("OPEN_RESEARCH");
      }}
      onGoWelcome={() => {
        setError(null);
        navigate("FIRST_RUN");
      }}
      onGoHome={() => {
        setError(null);
        navigate(hasCompletedBefore ? "RETURNING_USER" : "FIRST_RUN");
      }}
      onResumeLast={() => void enterResearch("resume")}
      onCreateWorkspace={() => void enterResearch("create")}
      onOpenWorkspace={() => void enterResearch("open")}
      onEnterDirectly={() => void enterResearch("direct")}
      onRetryRead={() => void readPreference()}
    />
  );
}
