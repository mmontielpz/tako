/** Local-only onboarding state and research session helpers. No implicit project or provider calls. */
import type { Backend } from "./backend";
import type { OnboardingCompleted, OnboardingState, OnboardingWorkspaceOpened } from "./types";

const SESSION_KEY = "tako.research.session.v1";

export interface ResearchSessionPreference {
  completed: boolean;
  lastWorkspacePath: string | null;
  lastTopic: string | null;
  lastObjective: string | null;
}

const DEFAULT_SESSION: ResearchSessionPreference = {
  completed: false,
  lastWorkspacePath: null,
  lastTopic: null,
  lastObjective: null,
};

export function getOnboardingState(backend: Backend): Promise<OnboardingState> {
  return backend.call("onboarding_state", undefined);
}

/** Legacy Engineering workspace entry preserved for Rust contract tests; unused in Research onboarding. */
export function openEngineeringWorkspace(backend: Backend): Promise<OnboardingWorkspaceOpened> {
  return backend.call("onboarding_open_workspace", { workspace: "engineering" });
}

/** Legacy Engineering onboarding completion preserved for Rust contract tests. */
export function completeOnboarding(backend: Backend): Promise<OnboardingCompleted> {
  return backend.call("onboarding_complete", { workspace: "engineering" });
}

export function readResearchSession(): ResearchSessionPreference {
  try {
    const raw = window.localStorage.getItem(SESSION_KEY);
    if (!raw) return { ...DEFAULT_SESSION };
    const parsed = JSON.parse(raw) as Partial<ResearchSessionPreference>;
    return {
      completed: Boolean(parsed.completed),
      lastWorkspacePath: typeof parsed.lastWorkspacePath === "string" && parsed.lastWorkspacePath.trim()
        ? parsed.lastWorkspacePath
        : null,
      lastTopic: typeof parsed.lastTopic === "string" ? parsed.lastTopic : null,
      lastObjective: typeof parsed.lastObjective === "string" ? parsed.lastObjective : null,
    };
  } catch {
    return { ...DEFAULT_SESSION };
  }
}

export function saveResearchSession(next: Partial<ResearchSessionPreference>): ResearchSessionPreference {
  const merged: ResearchSessionPreference = {
    ...readResearchSession(),
    ...next,
  };
  try {
    window.localStorage.setItem(SESSION_KEY, JSON.stringify(merged));
  } catch {
    // Best-effort persistence when storage is unavailable.
  }
  return merged;
}
