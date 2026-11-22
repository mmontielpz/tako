import { useEffect, useRef, useState } from "preact/hooks";
import { connectionView, type ConnectionBackend, type ConnectionAttempt, type ConnectionView } from "./providerConnection";
import "./providerConnection.css";
export interface ProviderConnectionStepProps { backend: ConnectionBackend; onBack(): void; onSkip(): void; onContinue(): void }
export function ProviderConnectionStep({ backend, onBack, onSkip, onContinue }: ProviderConnectionStepProps) {
  const [view, setView] = useState<ConnectionView | null>(null), [selected, setSelected] = useState(false), [pending, setPending] = useState(false), [error, setError] = useState("");
  const heading = useRef<HTMLHeadingElement>(null), alive = useRef(true), generation = useRef(0), owned = useRef<ConnectionAttempt | null>(null), checking = useRef(false), retained = useRef(false);
  function accept(v: ConnectionView) {
    const value = connectionView(v);
    if (alive.current) { setView(value); if (value.attempt_id) owned.current = { attempt_id: value.attempt_id, generation: value.generation }; }
  }
  async function load() { setError(""); try { accept(await backend.list()); } catch { if (alive.current) setError("Connection service is unavailable. You can skip for now."); } }
  useEffect(() => {
    alive.current = true; heading.current?.focus(); void load();
    const timer = setInterval(() => { if (owned.current && !checking.current) {
      const epoch = generation.current; checking.current = true;
      void backend.poll(owned.current).then(v => { if (alive.current && epoch === generation.current) accept(v); }).catch(() => {
        if (alive.current && epoch === generation.current) { setView(null); setError("Connection status is unavailable. Retry or skip setup."); }
      }).finally(() => { checking.current = false; });
    } }, 500);
    return () => { alive.current = false; generation.current++; clearInterval(timer); if (owned.current && !retained.current) void backend.cancel(owned.current).catch(() => {}); };
  }, [backend]);
  const available = !!view && ["AVAILABLE", "AUTH_REQUIRED", "FAILED"].includes(view.state);
  async function act(action: () => Promise<void>) {
    if (pending) return; setPending(true); setError("");
    try { await action(); } catch { if (alive.current) setError("Sign-in could not be completed. Retry or skip setup."); }
    finally { if (alive.current) setPending(false); }
  }
  async function cancel() {
    generation.current++;
    if (owned.current) { accept(await backend.cancel(owned.current)); owned.current = null; }
  }
  async function leave(next: () => void) { await cancel(); next(); }
  const active = view?.state === "CONNECTING";
  const unknownActive = !view && !!owned.current;
  return <main class="connection-step" aria-busy={pending}>
    <p class="connection-eyebrow">3 OF 4 · NATIVE CONNECTION PREVIEW</p>
    <h1 ref={heading} tabIndex={-1}>Connect AI</h1><p>Optional. You can connect later.</p>
    <label class="connection-provider"><input type="radio" name="provider" checked={selected} disabled={!available || pending || active} onChange={() => setSelected(true)} /><strong>OpenAI / Codex</strong><small>{!view ? "Checking availability…" : view.state === "NOT_INSTALLED" ? "Supported Codex installation not found." : view.state === "UNSUPPORTED" ? "Connection method unavailable in this build." : "Official device sign-in · no AI requests."}</small></label>
    <ul class="connection-unavailable" aria-label="Unavailable connections"><li aria-disabled="true">Claude Code <small>Unavailable</small></li><li aria-disabled="true">GitHub Copilot <small>Unavailable</small></li></ul>
    <div class="connection-status" role="status">
      {active && <><p>{view.reason === "STARTING_SIGN_IN" ? "Preparing sign-in…" : "Waiting for sign-in…"}</p>{view.user_code && <><p>Enter this code on the official page:</p><strong class="connection-code">{view.user_code}</strong></>}
        <button onClick={() => void act(cancel)}>Cancel</button></>}
      {view?.state === "CONNECTED" && <p>Connected. No AI task has started.</p>}
      {view?.state === "EXPIRED" && <p>Connection needs checking.</p>}
      {view?.state === "FAILED" && <p>Sign-in could not be verified. Retry or skip setup.</p>}
    </div>
    {unknownActive && <button onClick={() => void act(cancel)}>Cancel</button>}
    {error && <div role="alert">{error}</div>}
    {(error || view?.state === "FAILED" || view?.state === "UNSUPPORTED") && <details><summary>Technical details</summary><p>{view?.reason ?? "NATIVE_SERVICE_UNAVAILABLE"}</p><button disabled={pending} onClick={() => void act(load)}>Check availability</button></details>}
    <button class="connection-quiet" disabled={pending} onClick={() => void act(() => leave(onSkip))}>Skip for now</button>
    <div class="connection-actions"><button class="connection-quiet" disabled={pending} onClick={() => void act(() => leave(onBack))}>Back</button>
      {view?.state === "CONNECTED" ? <button disabled={pending} onClick={() => void act(async () => {
        if (!owned.current) return;
        const current = connectionView(await backend.poll(owned.current)); accept(current);
        if (current.state !== "CONNECTED") return;
        retained.current = true; generation.current++; onContinue();
      })}>Continue</button> : view?.state === "EXPIRED" ? <button disabled={pending} onClick={() => void act(async () => { if (owned.current) accept(await backend.check(owned.current)); })}>Check connection</button> : <button disabled={pending || !selected || !available || active} onClick={() => void act(async () => { const epoch = ++generation.current; const started = await backend.begin(view?.generation ?? 0);
        if (!alive.current || epoch !== generation.current) { if (started.attempt_id) await backend.cancel({ attempt_id: started.attempt_id, generation: started.generation }); return; }
        accept(started);
      })}>Connect</button>}
    </div>
  </main>;
}
