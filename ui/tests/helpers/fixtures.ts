import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

export interface Fixture {
  root: string;
  project: string;
  data: string;
  cleanup(): void;
}

function git(cwd: string, ...args: string[]) {
  return execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
}

/** A disposable Git project plus an engine data directory, both under the temp dir. */
export function makeFixture(): Fixture {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tako-ui-test-"));
  const project = path.join(root, "project");
  fs.mkdirSync(project);
  git(project, "init", "-q", "-b", "main");
  git(project, "config", "user.name", "t");
  git(project, "config", "user.email", "t@t");
  fs.writeFileSync(path.join(project, "README.md"), "base\n");
  git(project, "add", "-A");
  git(project, "commit", "-qm", "init");
  return {
    root,
    project,
    data: path.join(root, "data"),
    cleanup() {
      if (root.includes("tako-ui-test-") && root.startsWith(os.tmpdir())) fs.rmSync(root, { recursive: true, force: true });
    },
  };
}

export const gitIn = git;
