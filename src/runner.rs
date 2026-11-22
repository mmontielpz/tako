//! Supervised subprocess execution: cleared environment, own process group,
//! wall-clock timeout, cooperative cancellation, group kill with escalation and
//! guaranteed reaping of the group leader. Output goes to files, never pipes.
//!
//! Ordering matters for safety: the leader's exit is *peeked* (`waitid` with
//! `WNOWAIT`) so the zombie leader keeps its pid/pgid reserved while leftovers in
//! the group are killed; only then is the leader reaped. This removes the window
//! in which a recycled pid could be signalled.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Exited(i32),
    Signaled(i32),
    TimedOut,
    Cancelled,
    SpawnFailed(String),
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub outcome: Outcome,
    pub duration_ms: u128,
    pub pid: Option<u32>,
    /// Non-leader processes were still alive in the group when the leader exited, and were killed.
    /// Detected via `/proc` (Linux); always false elsewhere.
    pub leftover_killed: bool,
    /// The group leader was reaped (no zombie left behind).
    pub reaped: bool,
}

pub fn killpg(pgid: i32, sig: i32) -> bool {
    if pgid <= 1 {
        return false;
    }
    // SAFETY: plain syscall with integer arguments.
    unsafe { libc::killpg(pgid, sig) == 0 }
}

/// Start time (clock ticks since boot) of a process, Linux only.
pub fn proc_start_ticks(pid: u32) -> Option<u64> {
    let s = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &s[s.rfind(')')? + 1..];
    rest.split_whitespace().nth(19)?.parse().ok()
}

/// True if the process exists and is not a zombie (Linux only; false if unknown).
pub fn pid_alive(pid: u32) -> bool {
    match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(s) => match s.rfind(')') {
            Some(i) => !matches!(
                s[i + 1..].split_whitespace().next(),
                Some("Z") | Some("X") | None
            ),
            None => false,
        },
        Err(_) => false,
    }
}

/// True if `/proc/<pid>` still exists at all (a zombie counts).
pub fn pid_exists(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// Live (non-zombie) members of process group `pgid`, excluding `except`. Linux only.
pub fn group_members(pgid: i32, except: u32) -> Vec<u32> {
    let mut v = vec![];
    let Ok(rd) = fs::read_dir("/proc") else {
        return v;
    };
    for e in rd.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        if pid == except {
            continue;
        }
        let Ok(s) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        let Some(i) = s.rfind(')') else { continue };
        let f: Vec<&str> = s[i + 1..].split_whitespace().collect();
        if f.len() > 2 && f[0] != "Z" && f[0] != "X" && f[2].parse::<i32>().ok() == Some(pgid) {
            v.push(pid);
        }
    }
    v
}

/// Has the child exited? Does not reap it.
fn peek_exited(pid: u32) -> bool {
    // SAFETY: siginfo_t is plain data; waitid with WNOWAIT only inspects state.
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        let r = libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        );
        if r != 0 {
            return true; // ECHILD etc.: nothing left to wait for
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let p = info.si_pid();
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        let p = info.si_pid;
        p != 0
    }
}

/// SIGTERM, short grace, SIGKILL to the whole group, then bounded reap of the leader.
fn terminate(child: &mut Child) -> bool {
    let pid = child.id();
    let pgid = pid as i32;
    killpg(pgid, libc::SIGTERM);
    let grace = Instant::now();
    while grace.elapsed() < Duration::from_millis(500) && !peek_exited(pid) {
        sleep(Duration::from_millis(20));
    }
    killpg(pgid, libc::SIGKILL); // also catches TERM-ignoring members and leftovers
    reap(child)
}

fn reap(child: &mut Child) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return true;
        }
        sleep(Duration::from_millis(10));
    }
    false
}

#[allow(clippy::too_many_arguments)]
pub fn run_supervised(
    program: &str,
    args: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
    timeout: Duration,
    cancel: &dyn Fn() -> bool,
    stdout_path: &Path,
    stderr_path: &Path,
    on_spawn: &mut dyn FnMut(u32),
) -> RunResult {
    let start = Instant::now();
    let failed = |e: String| RunResult {
        outcome: Outcome::SpawnFailed(e),
        duration_ms: 0,
        pid: None,
        leftover_killed: false,
        reaped: true,
    };
    let files = File::create(stdout_path).and_then(|o| File::create(stderr_path).map(|e| (o, e)));
    let (out, err) = match files {
        Ok(f) => f,
        Err(e) => return failed(e.to_string()),
    };
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .process_group(0);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return failed(e.to_string()),
    };
    let pid = child.id();
    let pgid = pid as i32;
    on_spawn(pid);

    let mut leftover = false;
    let (outcome, reaped) = loop {
        if peek_exited(pid) {
            // Leader is a zombie: its pid/pgid are reserved. Kill leftovers, then reap.
            leftover = !group_members(pgid, pid).is_empty();
            killpg(pgid, libc::SIGKILL);
            let reaped = reap(&mut child);
            let o = match child.try_wait() {
                Ok(Some(st)) => match (st.code(), st.signal()) {
                    (Some(c), _) => Outcome::Exited(c),
                    (None, Some(s)) => Outcome::Signaled(s),
                    _ => Outcome::Exited(-1),
                },
                _ => Outcome::Exited(-1),
            };
            break (o, reaped);
        }
        if cancel() {
            let r = terminate(&mut child);
            break (Outcome::Cancelled, r);
        }
        if start.elapsed() >= timeout {
            let r = terminate(&mut child);
            break (Outcome::TimedOut, r);
        }
        sleep(Duration::from_millis(10));
    };
    RunResult {
        outcome,
        duration_ms: start.elapsed().as_millis(),
        pid: Some(pid),
        leftover_killed: leftover,
        reaped,
    }
}
