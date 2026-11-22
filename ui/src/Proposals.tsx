import { useEffect, useRef, useState } from "preact/hooks";
import { BridgeError, type Backend } from "./backend";
import type { ProposalJob, ProviderFamily, ContextPreview, ConversationTurn } from "./types";

/** Project context is opt-in, even after a project is opened. */
export function Proposals({ backend, pollMs = 200, projectPath }: { backend: Backend; pollMs?: number; projectPath?: string }) {
  const [enabled, setEnabled] = useState(false);
  useEffect(() => { if (!projectPath) setEnabled(false); }, [projectPath]);
  return <div>
    <label class="context-choice"><input data-testid="context-enabled" type="checkbox" checked={enabled} disabled={!projectPath}
      onChange={e => setEnabled(e.currentTarget.checked)} />Use selected local project for AI context</label>
    <ProposalWorkspace key={enabled && projectPath ? projectPath : "text-only"} backend={backend} pollMs={pollMs}
      projectPath={enabled ? projectPath : undefined} />
  </div>;
}
function ProposalWorkspace({ backend, pollMs, projectPath }: { backend: Backend; pollMs: number; projectPath?: string }) {
  const [paths, setPaths] = useState("");
  const [includeHistory, setIncludeHistory] = useState(false);
  const [preview, setPreview] = useState<ContextPreview | null>(null);
  const [consent, setConsent] = useState(false);
  const [history, setHistory] = useState<ConversationTurn[]>([]);
  const previewGeneration = useRef(0);
  function invalidate() { previewGeneration.current++; setPreview(null); setConsent(false); }
  async function prepare() {
    if (!projectPath) return;
    invalidate(); const generation = previewGeneration.current; setError("");
    try {
      const value = await backend.call("context_preview", { project_path: projectPath,
        paths: paths.split("\n").map(p => p.trim()).filter(Boolean), prompt, include_history: includeHistory });
      if (mounted.current && generation === previewGeneration.current) setPreview(value);
    } catch (e) { if (mounted.current && generation === previewGeneration.current) setError(e instanceof BridgeError ? `${e.code}: ${e.message}` : "Context preview unavailable."); }
  }
  const [provider, setProvider] = useState<ProviderFamily>("open_ai");
  const [model, setModel] = useState("");
  const [prompt, setPrompt] = useState("");
  const [timeout, setTimeoutSeconds] = useState(30);
  const [tokens, setTokens] = useState(1024);
  const [job, setJob] = useState<ProposalJob | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const lock = useRef(false);
  const mounted = useRef(true);
  const currentId = useRef<number | null>(null);
  const active = job?.state === "pending" || job?.state === "cancelling";
  useEffect(() => {
    if (!projectPath || active) return;
    let stopped = false;
    void backend.call("conversation_history", { project_path: projectPath }).then(h => { if (!stopped) setHistory(h); })
      .catch(() => { if (!stopped) setError("Conversation history unavailable."); });
    return () => { stopped = true; };
  }, [backend, projectPath, job?.state, job?.request_id]);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (currentId.current !== null) void backend.call("proposal_cancel", { request_id: currentId.current }).catch(() => {});
    };
  }, [backend]);
  // Sequential polling, no overlapping polls and no requests before explicit submit.
  useEffect(() => {
    if (!active || !job) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const id = job.request_id;
    async function poll() {
      try {
        const next = await backend.call("proposal_poll", { request_id: id });
        if (stopped) return;
        setError("");
        setJob(next);
        if (next.state === "pending" || next.state === "cancelling") timer = setTimeout(poll, pollMs);
        else {
          currentId.current = null;
          if (projectPath) invalidate();
        }
      } catch {
        if (stopped) return;
        setError("Proposal status is unavailable. Cancelling the native request; local work remains available.");
        void backend.call("proposal_cancel", { request_id: id }).catch(() => {});
        // Retain the active ID and cancel control; never label an unknown result successful.
        timer = setTimeout(poll, pollMs);
      }
    }
    timer = setTimeout(poll, pollMs);
    return () => { stopped = true; clearTimeout(timer); };
  }, [backend, job?.request_id, active, pollMs]);
  async function submit() {
    if (lock.current || active) return;
    lock.current = true; setPending(true); setError(""); setJob(null);
    try {
      if (projectPath && (!preview || !consent)) return;
      const next = projectPath && preview
        ? await backend.call("workspace_start", { project_path: projectPath, preview_id: preview.preview_id, consent,
          provider, model: model.trim(), timeout_s: timeout, max_output_tokens: tokens })
        : await backend.call("proposal_start", { provider, model: model.trim(), prompt, timeout_s: timeout, max_output_tokens: tokens });
      if (!mounted.current) {
        void backend.call("proposal_cancel", { request_id: next.request_id }).catch(() => {}); return;
      }
      currentId.current = next.request_id; setJob(next);
    } catch (e) {
      if (mounted.current) setError(e instanceof BridgeError ? `${e.code}: ${e.message}` : "Proposal request unavailable.");
    } finally { lock.current = false; if (mounted.current) setPending(false); }
  }
  async function cancel() {
    if (!job) return;
    const id = job.request_id;
    try {
      const next = await backend.call("proposal_cancel", { request_id: id });
      if (mounted.current && currentId.current === id) setJob(next);
    } catch { if (mounted.current && currentId.current === id) setError("Cancellation could not be confirmed. The native timeout still applies."); }
  }
  return <section class="card proposals" data-testid="proposal-workspace" aria-label="AI proposals">
    <h2>AI proposals</h2>
    <p>Untrusted suggestions only. Send transmits only the text below to the selected provider.
      Project files and prior turns are included only through the explicit preview below. No tools or execution. Do not enter credentials.</p>
    <label class="field">Provider<select data-testid="proposal-provider" value={provider} disabled={pending || active}
      onChange={(e) => setProvider(e.currentTarget.value as ProviderFamily)}>
      <option value="open_ai">OpenAI</option>
      <option value="anthropic" disabled>Anthropic — not implemented</option>
      <option value="gemini" disabled>Gemini — not implemented</option>
      <option value="local_model" disabled>Local model — not implemented</option>
    </select></label>
    <label class="field">Model ID<input data-testid="proposal-model" value={model} maxLength={128} disabled={pending || active}
      onInput={(e) => setModel(e.currentTarget.value)} placeholder="Enter a model available to your account" /></label>
    <label class="field">Timeout (seconds)<input data-testid="proposal-timeout" type="number" min={1} max={120} value={timeout} disabled={pending || active}
      onInput={(e) => setTimeoutSeconds(Number(e.currentTarget.value))} /></label>
    <label class="field">Maximum output tokens<input data-testid="proposal-tokens" type="number" min={16} max={4096} value={tokens} disabled={pending || active}
      onInput={(e) => setTokens(Number(e.currentTarget.value))} /></label>
    <label class="field">Engineering request<textarea data-testid="proposal-prompt" value={prompt} maxLength={16384} disabled={pending || active}
      onInput={(e) => { setPrompt(e.currentTarget.value); invalidate(); }} /></label>
    {projectPath && <div data-testid="project-context">
      <p>Project: <span class="mono">{projectPath}</span>. Files are read only when you request a preview.
        Select up to 8 UTF-8 source/documentation files, 4096 bytes each; total request limit 16384 bytes.
        Hidden, sensitive and unsupported paths are refused. Review source content for credentials before sending.</p>
      <label class="field">Selected relative file paths (one per line)<textarea data-testid="context-paths" value={paths} disabled={pending || active}
        onInput={e => { setPaths(e.currentTarget.value); invalidate(); }} /></label>
      <label class="context-choice"><input data-testid="context-history" type="checkbox" checked={includeHistory} disabled={pending || active}
        onChange={e => { setIncludeHistory(e.currentTarget.checked); invalidate(); }} />Include this project's previous questions and answers</label>
      <button data-testid="context-preview-button" disabled={pending || active || !prompt.trim()} onClick={prepare}>Preview exact provider input</button>
      {preview && <div><p>{preview.bytes} UTF-8 bytes. This frozen snapshot is sent even if files change afterward.</p>
        <pre data-testid="context-preview">{preview.text}</pre>
        <label class="context-choice"><input data-testid="context-consent" type="checkbox" checked={consent} disabled={pending || active}
          onChange={e => setConsent(e.currentTarget.checked)} />I reviewed this exact context and consent to sending it to the selected provider</label></div>}
      <div data-testid="conversation-history">{history.map(turn => <div><p>User: {turn.prompt}</p><pre>Untrusted assistant: {turn.response}</pre></div>)}</div>
      <button data-testid="conversation-clear" disabled={pending || active} onClick={async () => {
        invalidate();
        try { setHistory(await backend.call("conversation_clear", { project_path: projectPath })); }
        catch { setError("Conversation could not be cleared."); }
      }}>Clear project conversation</button>
      <p>History is kept for this application session (last 8 successful turns, up to 16 projects). It is excluded from requests unless selected.</p>
    </div>}
    <button data-testid="proposal-send" disabled={pending || active || (!!projectPath && (!preview || !consent)) || !model.trim() || !prompt.trim()
      || !Number.isInteger(timeout) || timeout < 1 || timeout > 120 || !Number.isInteger(tokens) || tokens < 16 || tokens > 4096}
      onClick={submit}>Send proposal request</button>
    {active && <button class="secondary" data-testid="proposal-cancel" onClick={cancel}>Cancel request</button>}
    <p class="muted">Credentials must be configured in the native application environment. Requests may incur provider charges.
      Cancel stops local transport; remote processing or charges may already have occurred.</p>
    {pending && <p data-testid="proposal-starting">Starting request…</p>}
    {job && <p data-testid="proposal-state">{job.state}</p>}
    {(error || job?.error) && <p class="bad" role="alert" data-testid="proposal-error">{error || `${job!.error!.code}: ${job!.error!.message}`}</p>}
    {job?.response && <div data-testid="proposal-result">
      <p>Untrusted model response · {job.response.provider} · {job.response.model}</p>
      <pre data-testid="proposal-text">{job.response.text}</pre>
      {job.response.usage && <p data-testid="proposal-usage">Tokens: {job.response.usage.input_tokens} input,
        {" "}{job.response.usage.output_tokens} output, {job.response.usage.total_tokens} total (provider reported).</p>}
    </div>}
  </section>;
}
