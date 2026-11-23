import { useLayoutEffect, useRef, useState } from "preact/hooks";
import "./onboarding.css";

export interface WelcomeWizardProps {
  screen: "welcome" | "choose";
  pending: boolean;
  restoring: boolean;
  error: string | null;
  notice?: string | null;
  canRetryRead?: boolean;
  workspacePath: string;
  topic: string;
  objective: string;
  onWorkspacePathChange(v: string): void;
  onTopicChange(v: string): void;
  onObjectiveChange(v: string): void;
  onPickFolder?(): void;
  onContinue(): void;
  onSkip(): void;
  onBack(): void;
  onCreateWorkspace(): void;
  onOpenWorkspace(): void;
  onEnterDirectly(): void;
  onRetryRead(): void;
}

export function OnboardingHeader() {
  return <header class="onboarding-header">
    <div class="onboarding-brand-slot" aria-label="TAKO Research Environment">
      <span>TAKO</span>
      <small>Agentic Research Development Environment</small>
    </div>
    <span class="onboarding-mode">Research · Offline mode</span>
  </header>;
}

export function WelcomeWizard(props: WelcomeWizardProps) {
  const [help, setHelp] = useState(false);
  const [guidance, setGuidance] = useState(false);
  const heading = useRef<HTMLHeadingElement>(null);
  const error = useRef<HTMLDivElement>(null);
  const setup = useRef<HTMLButtonElement>(null);
  const helpInitialized = useRef(false);
  useLayoutEffect(() => { heading.current?.focus(); }, [props.screen]);
  useLayoutEffect(() => { if (props.error) error.current?.focus(); }, [props.error]);
  useLayoutEffect(() => {
    if (helpInitialized.current) setup.current?.focus();
    helpInitialized.current = true;
  }, [help]);
  function closeHelp() { setHelp(false); setGuidance(false); }

  const canCreate = Boolean(props.workspacePath.trim() && props.topic.trim() && props.objective.trim() && !props.pending);
  const canOpen = Boolean(props.workspacePath.trim() && !props.pending);

  return <div class="onboarding" data-testid="onboarding">
    <OnboardingHeader />
    {props.notice && <p class="onboarding-notice" role="status" data-testid="onboarding-recovery">{props.notice}</p>}
    {props.screen === "welcome" ? <>
      <main class="onboarding-main onboarding-welcome">
        <p class="onboarding-step">STEP 1 OF 2</p>
        <h1 ref={heading} tabIndex={-1} data-testid="welcome-heading">Welcome to TAKO</h1>
        <p class="onboarding-intro">An Agentic Research Development Environment for State-of-the-Art discovery and synthesis.<br />Start locally without Git repositories, API keys, or paid services.</p>
        {!help ? <>
          <div class="onboarding-welcome-actions">
            <button data-testid="welcome-continue" onClick={props.onContinue}>Continue</button>
            <button class="onboarding-secondary" data-testid="welcome-skip" aria-describedby="welcome-skip-help" onClick={props.onSkip}>Skip introduction</button>
          </div>
          <button ref={setup} class="onboarding-text-action onboarding-setup" data-testid="setup-options" aria-expanded={false} aria-controls="onboarding-setup-panel" onClick={() => setHelp(true)}>Setup options (optional)</button>
          <p id="welcome-skip-help" class="onboarding-help">Go straight to creating or opening a research workspace.</p>
        </> : <section id="onboarding-setup-panel" class="onboarding-setup-panel" onKeyDown={event => {
          if (event.key === "Escape") { event.preventDefault(); closeHelp(); }
        }}>
          <button ref={setup} class="onboarding-text-action onboarding-setup-title" data-testid="setup-options" aria-expanded={true} aria-controls="onboarding-setup-panel" onClick={closeHelp}>Setup options (optional)</button>
          <p>Nothing needs connecting to start.<br />TAKO runs offline fixture workflows by default. Runtime and execution settings can be adjusted later inside the workspace.</p>
          <button class="onboarding-text-action" data-testid="setup-guidance" aria-expanded={guidance} aria-controls="onboarding-guidance" onClick={() => setGuidance(value => !value)}>Show offline runtime guidance</button>
          {guidance && <div id="onboarding-guidance" data-testid="setup-guidance-copy">
            <p>Offline SOTA workflows use the configured isolated TAKO Python runtime (v0.1.0a3 or newer). No Git repository, API key, or network connection is required.</p>
          </div>}
          <div class="onboarding-help-actions">
            <button data-testid="continue-without-setup" onClick={props.onContinue}>Continue without setup</button>
            <button class="onboarding-secondary" data-testid="back-to-welcome" onClick={closeHelp}>Back to welcome</button>
          </div>
        </section>}
      </main>
      <footer class="onboarding-footer">Research first. Start locally without connecting external services.</footer>
    </> : <main class="onboarding-main onboarding-selector" aria-busy={props.pending}>
      <p class="onboarding-step">{props.restoring ? "RESEARCH WORKSPACE" : "STEP 2 OF 2"}</p>
      <h1 ref={heading} tabIndex={-1} data-testid="workspace-heading">{props.restoring ? "Open or create a research workspace" : "Set up your research workspace"}</h1>
      <p class="onboarding-selector-intro">Create a new portable research workspace or open an existing one. No Git repository or API key is required.</p>

      <div class="onboarding-setup-panel" style={{ textAlign: "left", marginBottom: "16px" }}>
        <label class="field" style={{ display: "block", marginBottom: "10px" }}>
          <span>Workspace folder</span>
          <div style={{ display: "flex", gap: "8px", marginTop: "4px" }}>
            <input
              data-testid="onboarding-ws-path"
              style={{ flex: 1 }}
              value={props.workspacePath}
              placeholder="/path/to/research_workspace"
              onInput={e => props.onWorkspacePathChange((e.target as HTMLInputElement).value)}
            />
            {props.onPickFolder && (
              <button type="button" class="onboarding-secondary" data-testid="onboarding-pick-folder" onClick={props.onPickFolder}>
                Browse
              </button>
            )}
          </div>
        </label>
        <label class="field" style={{ display: "block", marginBottom: "10px" }}>
          <span>Research topic</span>
          <input
            data-testid="onboarding-topic"
            style={{ width: "100%", marginTop: "4px" }}
            value={props.topic}
            onInput={e => props.onTopicChange((e.target as HTMLInputElement).value)}
          />
        </label>
        <label class="field" style={{ display: "block" }}>
          <span>Research objective</span>
          <input
            data-testid="onboarding-objective"
            style={{ width: "100%", marginTop: "4px" }}
            value={props.objective}
            onInput={e => props.onObjectiveChange((e.target as HTMLInputElement).value)}
          />
        </label>
      </div>

      <div class={`onboarding-status ${props.error ? "onboarding-error" : ""}`} ref={error} tabIndex={props.error ? -1 : undefined} role={props.error ? "alert" : "status"} data-testid="onboarding-status">
        {props.error ? <>{<p>{props.error}</p>}{props.canRetryRead && <button class="onboarding-text-action" data-testid="retry-onboarding-read" onClick={props.onRetryRead}>Retry reading preference</button>}</> : <p>{props.pending ? "Preparing research workspace…" : "Offline fixture mode is active. No network or AI provider credentials are required."}</p>}
      </div>

      <div class="onboarding-selector-actions">
        <button class="onboarding-secondary" data-testid="workspace-back" onClick={props.onBack}>Back</button>
        <button class="onboarding-secondary" data-testid="workspace-open-existing" disabled={!canOpen} onClick={props.onOpenWorkspace}>Open existing</button>
        <button data-testid="workspace-create-enter" disabled={!canCreate} onClick={props.onCreateWorkspace}>Create &amp; enter research</button>
      </div>
      <p class="onboarding-selector-caption">
        <button type="button" class="onboarding-text-action" data-testid="workspace-enter-directly" onClick={props.onEnterDirectly}>
          Enter research environment without selecting a workspace yet
        </button>
      </p>
    </main>}
  </div>;
}
