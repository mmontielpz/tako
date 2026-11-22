//! Thin wrapper over the system `git`. Hooks are disabled for every call the
//! engine makes, and ambient `GIT_*` location variables are removed.

use crate::error::{EngineError, Result};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct GitOut {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

const BASE_CONFIG: [&str; 10] = [
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "commit.gpgsign=false",
    "-c",
    "user.name=TAKO",
    "-c",
    "user.email=tako@localhost",
    "-c",
    "core.fsmonitor=false",
];

fn base(cwd: &Path) -> Command {
    let mut c = Command::new("git");
    c.args(BASE_CONFIG).current_dir(cwd).stdin(Stdio::null());
    for k in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
        "GIT_CEILING_DIRECTORIES",
    ] {
        c.env_remove(k);
    }
    c.env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C");
    c
}

fn collect(out: std::process::Output) -> GitOut {
    GitOut {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

pub fn git_env(cwd: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<GitOut> {
    let mut c = base(cwd);
    c.args(args);
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c
        .output()
        .map_err(|e| EngineError::Git(format!("cannot run git: {e}")))?;
    Ok(collect(out))
}

pub fn git(cwd: &Path, args: &[&str]) -> Result<GitOut> {
    git_env(cwd, args, &[])
}

/// Run git and return trimmed stdout, or an error carrying stderr.
pub fn git_ok(cwd: &Path, args: &[&str]) -> Result<String> {
    let o = git(cwd, args)?;
    if o.ok {
        Ok(o.stdout.trim().to_string())
    } else {
        Err(EngineError::Git(format!(
            "git {}: {}",
            args.join(" "),
            o.stderr.trim()
        )))
    }
}

pub fn toplevel(path: &Path) -> Result<String> {
    git_ok(path, &["rev-parse", "--show-toplevel"])
}

pub fn head(path: &Path) -> Result<String> {
    git_ok(path, &["rev-parse", "HEAD"])
}

/// Current branch name; errors on detached HEAD (unsupported in this slice).
pub fn current_branch(path: &Path) -> Result<String> {
    git_ok(path, &["symbolic-ref", "--short", "HEAD"])
        .map_err(|_| EngineError::InvalidInput("detached HEAD is not supported".into()))
}

pub fn porcelain(path: &Path) -> Result<String> {
    git_ok(path, &["status", "--porcelain", "--untracked-files=all"])
}

pub fn is_dirty(path: &Path) -> Result<bool> {
    Ok(!porcelain(path)?.is_empty())
}

pub fn tree_of(path: &Path, rev: &str) -> Result<String> {
    git_ok(path, &["rev-parse", &format!("{rev}^{{tree}}")])
}

/// Hash of the full working tree (tracked + untracked, non-ignored) computed
/// with a throwaway index outside the working tree.
pub fn working_tree_hash(dir: &Path, scratch_index: &Path) -> Result<String> {
    let idx = scratch_index.to_string_lossy().into_owned();
    let _ = std::fs::remove_file(scratch_index);
    let env = [("GIT_INDEX_FILE", idx.as_str())];
    let r = (|| {
        let a = git_env(dir, &["read-tree", "HEAD"], &env)?;
        if !a.ok {
            return Err(EngineError::Git(a.stderr));
        }
        let b = git_env(dir, &["add", "-A"], &env)?;
        if !b.ok {
            return Err(EngineError::Git(b.stderr));
        }
        let c = git_env(dir, &["write-tree"], &env)?;
        if !c.ok {
            return Err(EngineError::Git(c.stderr));
        }
        Ok(c.stdout.trim().to_string())
    })();
    let _ = std::fs::remove_file(scratch_index);
    r
}

/// Content identity (git blob id) of a byte string. Not tamper-proof against a
/// determined local attacker; it binds an approval to exact plan content.
pub fn hash_bytes(cwd: &Path, bytes: &[u8]) -> Result<String> {
    let mut c = base(cwd);
    c.args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| EngineError::Git(e.to_string()))?;
    child.stdin.take().unwrap().write_all(bytes)?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(EngineError::Git(
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn git_version() -> String {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

/// Is this repository-local config key one that can make Git run a program during the
/// local operations the engine performs (status, add, commit, merge, worktree, diff)?
/// Hooks and fsmonitor are *not* listed: they are neutralized on the command line.
fn risky_key(k: &str) -> bool {
    let ends = |x: &str| k.ends_with(x);
    (k.starts_with("filter.") && (ends(".clean") || ends(".smudge") || ends(".process")))
        || (k.starts_with("merge.") && ends(".driver"))
        || (k.starts_with("diff.") && (ends(".textconv") || ends(".command")))
        || k == "diff.external"
        || k.starts_with("include.")
        || k.starts_with("includeif.")
        || k == "core.worktree"
        || k == "gpg.program"
        || (k.starts_with("gpg.") && ends(".program"))
}

/// Audit repository-scoped (local and worktree) configuration. Returns the offending keys.
pub fn audit_config(dir: &Path) -> Result<Vec<String>> {
    let o = git(dir, &["config", "--list", "--show-scope"])?;
    if !o.ok {
        return Err(EngineError::Git(format!(
            "config audit failed: {}",
            o.stderr.trim()
        )));
    }
    let mut found = vec![];
    for line in o.stdout.lines() {
        let Some((scope, kv)) = line.split_once('\t') else {
            continue;
        };
        if scope != "local" && scope != "worktree" {
            continue;
        }
        let key = kv.split('=').next().unwrap_or("").to_lowercase();
        if risky_key(&key) {
            found.push(format!("{scope}:{key}"));
        }
    }
    Ok(found)
}

/// Canonical common git dir (the shared `.git` of a project or worktree).
pub fn common_dir(dir: &Path) -> Result<std::path::PathBuf> {
    let s = git_ok(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    Ok(std::fs::canonicalize(s)?)
}
