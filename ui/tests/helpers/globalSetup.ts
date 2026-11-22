import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** Build the real engine bridge harness once; UI tests drive it over stdio. */
export default function setup() {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
  execFileSync("cargo", ["build", "-p", "tako-bridge", "--bin", "tako-bridge-stdio", "--locked"], {
    cwd: root,
    stdio: "inherit",
  });
}
