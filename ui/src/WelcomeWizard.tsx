import { useLayoutEffect, useRef, useState } from "preact/hooks";
import type { KeyboardEventHandler } from "preact";
import "./onboarding.css";

export interface WelcomeWizardProps {
  screen: "welcome" | "choose";
  selected: boolean;
  pending: boolean;
  restoring: boolean;
  error: string | null;
  notice?: string | null;
  canRetryRead?: boolean;
  onContinue(): void;
  onBack(): void;
  onSelect(): void;
  onOpen(): void;
  onRetryRead(): void;
}

export function OnboardingHeader() {
  return <header class="onboarding-header">
    <div class="onboarding-brand-slot" aria-label="Temporary placeholder for the approved TAKO mark and wordmark">
      <span>Approved mark + wordmark</span>
      <small>TEMPORARY PLACEHOLDER · export needed</small>
    </div>
    <span class="onboarding-mode">Simple mode</span>
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
  const radioKeys: KeyboardEventHandler<HTMLInputElement> = event => {
    if (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End", " "].includes(event.key)) {
      event.preventDefault();
      if (!props.pending) props.onSelect();
    } else if (event.key === "Enter") event.preventDefault();
  };

  return <div class="onboarding" data-testid="onboarding">
    <OnboardingHeader />
    {props.notice && <p class="onboarding-notice" role="status" data-testid="onboarding-recovery">{props.notice}</p>}
    {props.screen === "welcome" ? <>
      <main class="onboarding-main onboarding-welcome">
        <p class="onboarding-step">STEP 1 OF 2</p>
        <h1 ref={heading} tabIndex={-1} data-testid="welcome-heading">Welcome to TAKO</h1>
        <p class="onboarding-intro">A local workspace for research and engineering.<br />You can start without connecting any services.</p>
        {!help ? <>
          <div class="onboarding-welcome-actions">
            <button data-testid="welcome-continue" onClick={props.onContinue}>Continue</button>
            <button class="onboarding-secondary" data-testid="welcome-skip" aria-describedby="welcome-skip-help" onClick={props.onContinue}>Skip introduction</button>
          </div>
          <button ref={setup} class="onboarding-text-action onboarding-setup" data-testid="setup-options" aria-expanded={false} aria-controls="onboarding-setup-panel" onClick={() => setHelp(true)}>Setup options (optional)</button>
          <p id="welcome-skip-help" class="onboarding-help">Choose a workspace next. Set up connections later.</p>
        </> : <section id="onboarding-setup-panel" class="onboarding-setup-panel" onKeyDown={event => {
          if (event.key === "Escape") { event.preventDefault(); closeHelp(); }
        }}>
          <button ref={setup} class="onboarding-text-action onboarding-setup-title" data-testid="setup-options" aria-expanded={true} aria-controls="onboarding-setup-panel" onClick={closeHelp}>Setup options (optional)</button>
          <p>Nothing needs connecting to start.<br />AI services and optional integrations can be configured later.</p>
          <button class="onboarding-text-action" data-testid="setup-guidance" aria-expanded={guidance} aria-controls="onboarding-guidance" onClick={() => setGuidance(value => !value)}>Show AI setup guidance</button>
          {guidance && <div id="onboarding-guidance" data-testid="setup-guidance-copy">
            <p>AI proposals are optional. Provider credentials are configured outside TAKO in the native environment; restarting TAKO may be required. You can continue without configuring anything.</p>
          </div>}
          <div class="onboarding-help-actions">
            <button data-testid="continue-without-setup" onClick={props.onContinue}>Continue without setup</button>
            <button class="onboarding-secondary" data-testid="back-to-welcome" onClick={closeHelp}>Back to welcome</button>
          </div>
        </section>}
      </main>
      <footer class="onboarding-footer">Start locally. Connect services only when you choose.</footer>
    </> : <main class="onboarding-main onboarding-selector" aria-busy={props.pending}>
      <p class="onboarding-step">{props.restoring ? "WORKSPACE" : "STEP 2 OF 2"}</p>
      <h1 ref={heading} tabIndex={-1} data-testid="workspace-heading">{props.restoring ? "Choose a workspace" : "What would you like to work on?"}</h1>
      <p class="onboarding-selector-intro">Choose a starting point. No setup is required.</p>
      <fieldset class="onboarding-choices" aria-describedby="research-unavailable">
        <legend class="onboarding-sr-only">Workspace</legend>
        <div class="onboarding-choice onboarding-unavailable">
          <label class="onboarding-choice-heading"><input type="radio" name="onboarding-workspace" value="research" disabled data-testid="workspace-research" /><strong>Research</strong><span class="onboarding-choice-state">Unavailable</span></label>
          <p class="onboarding-choice-copy">Start with a research question,<br class="onboarding-desktop-break" /> plan and research records.</p>
          <p id="research-unavailable" tabIndex={0} role="note" aria-label="Research unavailable. Research Lab is not in this build." class="onboarding-choice-help" data-testid="research-unavailable">Research Lab is not in this build.</p>
        </div>
        <label class={`onboarding-choice onboarding-engineering ${props.selected ? "onboarding-selected" : ""}`}>
          <span class="onboarding-choice-heading"><input type="radio" name="onboarding-workspace" value="engineering" checked={props.selected} disabled={props.pending} onChange={props.onSelect} onKeyDown={radioKeys} data-testid="workspace-engineering" /><strong>Engineering</strong>{props.selected && <span class="onboarding-choice-state">Selected</span>}</span>
          <span class="onboarding-choice-copy">Work with an existing local Git<br class="onboarding-desktop-break" /> project and review a plan before<br class="onboarding-desktop-break" /> running code.</span>
          <span class="onboarding-choice-help">Choose your project after opening.</span>
        </label>
      </fieldset>
      <div class={`onboarding-status ${props.error ? "onboarding-error" : ""}`} ref={error} tabIndex={props.error ? -1 : undefined} role={props.error ? "alert" : "status"} data-testid="onboarding-status">
        {props.error ? <><p>{props.error}</p>{props.canRetryRead && <button class="onboarding-text-action" data-testid="retry-onboarding-read" onClick={props.onRetryRead}>Retry reading preference</button>}</> : <p>{props.pending ? "Opening Engineering… No project or provider request starts." : "No connection or AI provider is required to choose a workspace."}</p>}
      </div>
      <div class="onboarding-selector-actions">
        <button class="onboarding-secondary" data-testid="workspace-back" onClick={props.onBack}>{props.restoring ? "Back to welcome" : "Back"}</button>
        <button data-testid="workspace-open" disabled={!props.selected || props.pending} onClick={props.onOpen}>{props.pending ? "Opening Engineering…" : props.selected ? (props.error && !props.canRetryRead ? "Retry opening Engineering" : "Open Engineering") : "Choose a workspace"}</button>
      </div>
      <p class="onboarding-selector-caption">{props.selected ? "Simple mode is the starting view. No connections are required." : "Select Engineering to continue in this build."}</p>
    </main>}
  </div>;
}
