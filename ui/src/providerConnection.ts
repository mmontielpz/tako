/** Auth-only native preview contract. No provider token or model request seam. */
export type ConnectionState = "AVAILABLE" | "NOT_INSTALLED" | "AUTH_REQUIRED" | "CONNECTING" | "CONNECTED" | "EXPIRED" | "FAILED" | "UNSUPPORTED";
export interface ConnectionView {
  provider: "codex"; state: ConnectionState; reason: string; attempt_id: string | null;
  generation: number; user_code: string | null; checked_at: number | null; execution_authorized: false;
}
export interface ConnectionAttempt { attempt_id: string; generation: number }
export interface ConnectionBackend {
  list(): Promise<ConnectionView>;
  begin(expectedGeneration: number): Promise<ConnectionView>;
  poll(attempt: ConnectionAttempt): Promise<ConnectionView>;
  cancel(attempt: ConnectionAttempt): Promise<ConnectionView>;
  check(attempt: ConnectionAttempt): Promise<ConnectionView>;
}
const states = ["AVAILABLE", "NOT_INSTALLED", "AUTH_REQUIRED", "CONNECTING", "CONNECTED", "EXPIRED", "FAILED", "UNSUPPORTED"];
export function connectionView(value: unknown): ConnectionView {
  const v = value as ConnectionView;
  if (!v || v.provider !== "codex" || !states.includes(v.state) || v.execution_authorized !== false ||
      !Number.isSafeInteger(v.generation) || v.generation < 0 || typeof v.reason !== "string" ||
      !(v.attempt_id === null || typeof v.attempt_id === "string" && /^[a-f0-9]{48}$/.test(v.attempt_id)) ||
      !(v.user_code === null || typeof v.user_code === "string" && /^[A-Z0-9-]{4,16}$/.test(v.user_code)) ||
      !(v.checked_at === null || Number.isSafeInteger(v.checked_at) && v.checked_at > 0) ||
      v.state === "CONNECTED" && (!v.attempt_id || !v.checked_at || v.user_code !== null) ||
      v.user_code !== null && v.state !== "CONNECTING" ||
      Object.keys(v).sort().join() !== ["provider", "state", "reason", "attempt_id", "generation", "user_code", "checked_at", "execution_authorized"].sort().join()) {
    throw new Error("Connection status could not be verified.");
  }
  return v;
}
export function nativeConnectionBackend(timeoutMs = 8000): ConnectionBackend {
  async function call(cmd: string, args: unknown): Promise<ConnectionView> {
    const { invoke } = await import("@tauri-apps/api/core");
    let timer: ReturnType<typeof setTimeout> | undefined;
    const timeoutPromise = new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error("Connection timeout")), timeoutMs);
    });
    try {
      const res = await Promise.race([
        invoke("bridge_call", { cmd, args }),
        timeoutPromise
      ]);
      return connectionView(res);
    } catch {
      throw new Error("Connection service is unavailable. Retry or skip setup.");
    } finally {
      if (timer) clearTimeout(timer);
    }
  }
  return {
    list: () => call("provider_connection_list", null),
    begin: expectedGeneration => call("provider_connection_begin", { provider: "codex", intent: "connect_provider", expected_generation: expectedGeneration }),
    poll: a => call("provider_connection_poll", a),
    cancel: a => call("provider_connection_cancel", a),
    check: a => call("provider_connection_check", a),
  };
}
