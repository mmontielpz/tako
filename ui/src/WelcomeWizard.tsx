import { useLayoutEffect, useRef, useState } from "preact/hooks";
import "./onboarding.css";

export type OnboardingViewState =
  | "FIRST_RUN"
  | "RETURNING_USER"
  | "CREATE_RESEARCH"
  | "OPEN_RESEARCH";

export interface WelcomeWizardProps {
  view: OnboardingViewState;
  pending: boolean;
  error: string | null;
  notice?: string | null;
  canRetryRead?: boolean;
  lastWorkspacePath: string | null;
  lastTopic?: string | null;
  workspacePath: string;
  topic: string;
  objective: string;
  onWorkspacePathChange(v: string): void;
  onTopicChange(v: string): void;
  onObjectiveChange(v: string): void;
  onPickFolder?(): void;
  onGoCreate(): void;
  onGoOpen(): void;
  onGoWelcome(): void;
  onGoHome(): void;
  onResumeLast(): void;
  onCreateWorkspace(): void;
  onOpenWorkspace(): void;
  onEnterDirectly(): void;
  onRetryRead(): void;
}

export function OnboardingHeader() {
  return (
    <header class="onboarding-header">
      <div class="onboarding-brand-slot" aria-label="TAKO Research Environment">
        <span>TAKO</span>
        <small>Agentic Research Development Environment</small>
      </div>
      <span class="onboarding-mode">Research · Offline Demo</span>
    </header>
  );
}

export function WelcomeWizard(props: WelcomeWizardProps) {
  const [help, setHelp] = useState(false);
  const [guidance, setGuidance] = useState(false);
  const heading = useRef<HTMLHeadingElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const setupRef = useRef<HTMLButtonElement>(null);
  const helpInitialized = useRef(false);

  useLayoutEffect(() => {
    heading.current?.focus();
  }, [props.view]);
  useLayoutEffect(() => {
    if (props.error) errorRef.current?.focus();
  }, [props.error]);
  useLayoutEffect(() => {
    if (helpInitialized.current) setupRef.current?.focus();
    helpInitialized.current = true;
  }, [help]);

  function closeHelp() {
    setHelp(false);
    setGuidance(false);
  }

  const canCreate = Boolean(
    props.workspacePath.trim() && props.topic.trim() && props.objective.trim() && !props.pending,
  );
  const canOpen = Boolean(props.workspacePath.trim() && !props.pending);

  return (
    <div class="onboarding" data-testid="onboarding">
      <OnboardingHeader />
      {props.notice && (
        <p class="onboarding-notice" role="status" data-testid="onboarding-recovery">
          {props.notice}
        </p>
      )}

      {props.view === "FIRST_RUN" && (
        <>
          <main class="onboarding-main onboarding-welcome">
            <p class="onboarding-step">WELCOME</p>
            <h1 ref={heading} tabIndex={-1} data-testid="welcome-heading">
              Welcome to TAKO
            </h1>
            <p class="onboarding-intro">
              An Agentic Research Development Environment for State-of-the-Art discovery and synthesis.
              <br />
              Start locally without Git repositories, API keys, or paid services.
            </p>
            {!help ? (
              <>
                <div class="onboarding-welcome-actions">
                  <button data-testid="welcome-create" onClick={props.onGoCreate}>
                    Create Research Workspace
                  </button>
                  <button
                    class="onboarding-secondary"
                    data-testid="welcome-open"
                    onClick={props.onGoOpen}
                  >
                    Open Existing Workspace
                  </button>
                </div>
                <div class="onboarding-welcome-actions" style={{ marginTop: "8px" }}>
                  <button
                    class="onboarding-text-action"
                    data-testid="welcome-skip"
                    onClick={props.onEnterDirectly}
                  >
                    Skip to Research Environment
                  </button>
                  <button
                    ref={setupRef}
                    class="onboarding-text-action onboarding-setup"
                    data-testid="setup-options"
                    aria-expanded={false}
                    aria-controls="onboarding-setup-panel"
                    onClick={() => setHelp(true)}
                  >
                    Setup options (optional)
                  </button>
                </div>
              </>
            ) : (
              <section
                id="onboarding-setup-panel"
                class="onboarding-setup-panel"
                onKeyDown={(event) => {
                  if (event.key === "Escape") {
                    event.preventDefault();
                    closeHelp();
                  }
                }}
              >
                <button
                  ref={setupRef}
                  class="onboarding-text-action onboarding-setup-title"
                  data-testid="setup-options"
                  aria-expanded={true}
                  aria-controls="onboarding-setup-panel"
                  onClick={closeHelp}
                >
                  Setup options (optional)
                </button>
                <p>
                  Nothing needs connecting to start. TAKO runs offline fixture workflows by default.
                </p>
                <button
                  class="onboarding-text-action"
                  data-testid="setup-guidance"
                  aria-expanded={guidance}
                  aria-controls="onboarding-guidance"
                  onClick={() => setGuidance((v) => !v)}
                >
                  Show offline runtime guidance
                </button>
                {guidance && (
                  <div id="onboarding-guidance" data-testid="setup-guidance-copy">
                    <p>
                      Offline SOTA workflows use the configured isolated TAKO Python runtime (v0.1.0a3
                      or newer). No Git repository, API key, or network connection is required.
                    </p>
                  </div>
                )}
                <div class="onboarding-help-actions">
                  <button data-testid="continue-without-setup" onClick={props.onGoCreate}>
                    Create Research Workspace
                  </button>
                  <button class="onboarding-secondary" data-testid="back-to-welcome" onClick={closeHelp}>
                    Back
                  </button>
                </div>
              </section>
            )}
          </main>
          <footer class="onboarding-footer">
            Research first. Start locally without connecting external services.
          </footer>
        </>
      )}

      {props.view === "RETURNING_USER" && (
        <main class="onboarding-main onboarding-selector" aria-busy={props.pending}>
          <p class="onboarding-step">RESEARCH HOME</p>
          <h1 ref={heading} tabIndex={-1} data-testid="research-home-heading">
            Research Home
          </h1>
          <p class="onboarding-selector-intro">
            Resume your last research workspace, create a new one, or open an existing workspace.
          </p>

          {props.lastWorkspacePath && (
            <div class="onboarding-setup-panel" style={{ textAlign: "left", marginBottom: "16px" }}>
              <div style={{ fontSize: "12px", opacity: 0.8 }}>Last research workspace</div>
              <div style={{ fontWeight: "600", marginTop: "4px" }} data-testid="home-last-topic">
                {props.lastTopic || "Saved research workspace"}
              </div>
              <div style={{ fontSize: "12px", opacity: 0.8, marginTop: "2px" }} data-testid="home-last-path">
                {props.lastWorkspacePath}
              </div>
              <div style={{ marginTop: "10px" }}>
                <button
                  data-testid="home-resume-last"
                  disabled={props.pending}
                  onClick={props.onResumeLast}
                >
                  {props.pending ? "Opening…" : "Resume Research"}
                </button>
              </div>
            </div>
          )}

          {props.error && (
            <div
              class="onboarding-status onboarding-error"
              ref={errorRef}
              tabIndex={-1}
              role="alert"
              data-testid="onboarding-status"
            >
              <p>{props.error}</p>
              {props.canRetryRead && (
                <button class="onboarding-text-action" data-testid="retry-onboarding-read" onClick={props.onRetryRead}>
                  Retry
                </button>
              )}
            </div>
          )}

          <div class="onboarding-selector-actions">
            <button data-testid="home-create-research" disabled={props.pending} onClick={props.onGoCreate}>
              Create Research
            </button>
            <button
              class="onboarding-secondary"
              data-testid="home-open-research"
              disabled={props.pending}
              onClick={props.onGoOpen}
            >
              Open Existing
            </button>
          </div>
          <p class="onboarding-selector-caption">
            <button
              type="button"
              class="onboarding-text-action"
              data-testid="home-view-welcome"
              onClick={props.onGoWelcome}
            >
              View Welcome introduction
            </button>
          </p>
        </main>
      )}

      {(props.view === "CREATE_RESEARCH" || props.view === "OPEN_RESEARCH") && (
        <main class="onboarding-main onboarding-selector" aria-busy={props.pending}>
          <p class="onboarding-step">
            {props.view === "CREATE_RESEARCH" ? "CREATE RESEARCH" : "OPEN RESEARCH"}
          </p>
          <h1 ref={heading} tabIndex={-1} data-testid="workspace-heading">
            {props.view === "CREATE_RESEARCH"
              ? "Create a research workspace"
              : "Open an existing research workspace"}
          </h1>
          <p class="onboarding-selector-intro">
            {props.view === "CREATE_RESEARCH"
              ? "Choose a folder and define your research topic and objective. No Git repository is required."
              : "Select an existing TAKO research workspace directory."}
          </p>

          <div class="onboarding-setup-panel" style={{ textAlign: "left", marginBottom: "16px" }}>
            <label class="field" style={{ display: "block", marginBottom: "10px" }}>
              <span>Workspace folder</span>
              <div style={{ display: "flex", gap: "8px", marginTop: "4px" }}>
                <input
                  data-testid="onboarding-ws-path"
                  style={{ flex: 1 }}
                  value={props.workspacePath}
                  placeholder="/path/to/research_workspace"
                  onInput={(e) => props.onWorkspacePathChange((e.target as HTMLInputElement).value)}
                />
                {props.onPickFolder && (
                  <button
                    type="button"
                    class="onboarding-secondary"
                    data-testid="onboarding-pick-folder"
                    onClick={props.onPickFolder}
                  >
                    Browse
                  </button>
                )}
              </div>
            </label>

            {props.view === "CREATE_RESEARCH" && (
              <>
                <label class="field" style={{ display: "block", marginBottom: "10px" }}>
                  <span>Research topic or question</span>
                  <input
                    data-testid="onboarding-topic"
                    style={{ width: "100%", marginTop: "4px" }}
                    value={props.topic}
                    onInput={(e) => props.onTopicChange((e.target as HTMLInputElement).value)}
                  />
                </label>
                <label class="field" style={{ display: "block" }}>
                  <span>Research objective</span>
                  <input
                    data-testid="onboarding-objective"
                    style={{ width: "100%", marginTop: "4px" }}
                    value={props.objective}
                    onInput={(e) => props.onObjectiveChange((e.target as HTMLInputElement).value)}
                  />
                </label>
              </>
            )}
          </div>

          <div
            class={`onboarding-status ${props.error ? "onboarding-error" : ""}`}
            ref={errorRef}
            tabIndex={props.error ? -1 : undefined}
            role={props.error ? "alert" : "status"}
            data-testid="onboarding-status"
          >
            {props.error ? (
              <p>{props.error}</p>
            ) : (
              <p>
                {props.pending
                  ? "Preparing research workspace…"
                  : "Offline Demo Mode is active. No API keys or paid services are required."}
              </p>
            )}
          </div>

          <div class="onboarding-selector-actions">
            <button class="onboarding-secondary" data-testid="workspace-back" onClick={props.onGoHome}>
              Back
            </button>
            {props.view === "CREATE_RESEARCH" ? (
              <button
                data-testid="workspace-create-enter"
                disabled={!canCreate}
                onClick={props.onCreateWorkspace}
              >
                Create &amp; Enter Research
              </button>
            ) : (
              <button
                data-testid="workspace-open-existing"
                disabled={!canOpen}
                onClick={props.onOpenWorkspace}
              >
                Open &amp; Enter Research
              </button>
            )}
          </div>
        </main>
      )}
    </div>
  );
}
