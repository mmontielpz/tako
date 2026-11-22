/** Explicit, local-only onboarding bridge functions. No UI, implicit reads or writes. */
import type { Backend } from "./backend";
import type { OnboardingCompleted, OnboardingState, OnboardingWorkspaceOpened } from "./types";

export function getOnboardingState(backend: Backend): Promise<OnboardingState> {
  return backend.call("onboarding_state", undefined);
}

/** Enter the real native Engineering workspace; does not open a Git project or render UI. */
export function openEngineeringWorkspace(backend: Backend): Promise<OnboardingWorkspaceOpened> {
  return backend.call("onboarding_open_workspace", { workspace: "engineering" });
}

/** Future UI calls this only after its Engineering mount succeeds. Native ordering is also checked. */
export function completeOnboarding(backend: Backend): Promise<OnboardingCompleted> {
  return backend.call("onboarding_complete", { workspace: "engineering" });
}
