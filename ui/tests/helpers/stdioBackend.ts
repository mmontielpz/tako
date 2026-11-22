import { spawn, type ChildProcess } from "node:child_process";
import path from "node:path";
import readline from "node:readline";
import { fileURLToPath } from "node:url";
import { BridgeError, type Backend } from "../../src/backend";
import type { CommandName, Commands } from "../../src/types";

const BIN = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../target/debug/tako-bridge-stdio");

/** Test-only Backend: speaks to the REAL Rust bridge over stdio. There is no mock engine. */
export class StdioBackend implements Backend {
  readonly child: ChildProcess;
  readonly calls: string[] = [];
  private next = 1;
  private waiting = new Map<number, { ok: (v: unknown) => void; err: (e: unknown) => void }>();

  constructor(dataDir: string, gatewayPort: number | "t" | undefined = process.env.TAKO_LIVE_BACKEND_TESTS === "1" ? "t" : undefined) {
    this.child = spawn(BIN, [dataDir, ...(gatewayPort === undefined ? [] : [String(gatewayPort)])], {
      stdio: ["pipe", "pipe", "inherit"], env: { ...process.env, OPENAI_API_KEY: "" },
    }); // Offline qualification must never inherit a real provider credential.
    const rl = readline.createInterface({ input: this.child.stdout! });
    rl.on("line", (line) => {
      const msg = JSON.parse(line) as { id: number; ok?: unknown; err?: { code: string; message: string; details: string[] } };
      const w = this.waiting.get(msg.id);
      if (!w) return;
      this.waiting.delete(msg.id);
      if (msg.err) w.err(new BridgeError(msg.err));
      else w.ok(msg.ok);
    });
  }

  call<C extends CommandName>(cmd: C, args: Commands[C][0]): Promise<Commands[C][1]> {
    this.calls.push(cmd);
    const id = this.next++;
    return new Promise((ok, err) => {
      this.waiting.set(id, { ok: ok as (v: unknown) => void, err });
      this.child.stdin!.write(JSON.stringify({ id, cmd, args: args ?? null }) + "\n");
    });
  }

  /** SIGKILL: simulates a crash with no cleanup. */
  crash() {
    this.child.kill("SIGKILL");
  }
  close() {
    this.child.kill("SIGTERM");
  }
  count(cmd: string) {
    return this.calls.filter((c) => c === cmd).length;
  }
}
