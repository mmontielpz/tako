import type { BridgeErrorShape, CommandName, Commands } from "./types";

export class BridgeError extends Error {
  readonly code: string;
  readonly details: string[];
  constructor(e: BridgeErrorShape) {
    super(e.message);
    this.name = "BridgeError";
    this.code = e.code;
    this.details = e.details ?? [];
  }
}

/** Everything the UI knows about the engine goes through this one typed seam. */
export interface Backend {
  call<C extends CommandName>(cmd: C, args: Commands[C][0]): Promise<Commands[C][1]>;
  /** Native folder picker; null when cancelled or unavailable. */
  pickFolder?(): Promise<string | null>;
}

/** Production backend: the Tauri `bridge_call` command, which runs the Rust bridge off the UI thread. */
export function tauriBackend(): Backend {
  return {
    async call(cmd, args) {
      const { invoke } = await import("@tauri-apps/api/core");
      try {
        return (await invoke("bridge_call", { cmd, args: args ?? null })) as never;
      } catch (e) {
        const shape = e as Partial<BridgeErrorShape>;
        if (shape && typeof shape.code === "string") throw new BridgeError(shape as BridgeErrorShape);
        throw new BridgeError({ code: "internal", message: String(e), details: [] });
      }
    },
    async pickFolder() {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const r = await open({ directory: true, multiple: false, title: "Open a Git project" });
      return typeof r === "string" ? r : null;
    },
  };
}
