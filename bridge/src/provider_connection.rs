//! Opt-in, debug-only native authentication preview. No model or execution API.
//! Credentials belong to an isolated official Codex process, in memory only.
use crate::BridgeError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PIN: &str = "cdcb45aaa01d0f14e298985e09465aebccd47f64ecd298aea44814af7b62bbda";
const PAGE: &str = "https://auth.openai.com/codex/device";
const PROOF_TTL: Duration = Duration::from_secs(60);
const DEADLINE: Duration = Duration::from_secs(600);
const FRAME: u64 = 262_144;
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum State {
    Available,
    NotInstalled,
    AuthRequired,
    Connecting,
    Connected,
    Expired,
    Failed,
    Unsupported,
}
#[derive(Clone, Serialize)]
pub struct View {
    pub provider: &'static str,
    pub state: State,
    pub reason: &'static str,
    pub attempt_id: Option<String>,
    pub generation: u64,
    /// Transient device *user* code. Never a token, stored credential or log field.
    pub user_code: Option<String>,
    pub checked_at: Option<u64>,
    pub execution_authorized: bool,
}
impl View {
    fn new(state: State, reason: &'static str) -> Self {
        Self {
            provider: "codex",
            state,
            reason,
            attempt_id: None,
            generation: 0,
            user_code: None,
            checked_at: None,
            execution_authorized: false,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Begin {
    provider: String,
    intent: String,
    expected_generation: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    attempt_id: String,
    generation: u64,
}
struct Running {
    view: View,
    stop: Arc<std::sync::atomic::AtomicBool>,
    check: mpsc::SyncSender<()>,
    done: Arc<(Mutex<bool>, Condvar)>,
}
#[derive(Default)]
pub struct Connections {
    running: Arc<Mutex<Option<Running>>>,
}
fn error(code: &'static str) -> BridgeError {
    BridgeError::new(
        code,
        match code {
            "connection_disabled" => {
                "Provider connection requires the isolated debug preview build."
            }
            "connection_busy" => "A sign-in is already active. Cancel it before starting another.",
            "connection_stale" => "This sign-in attempt is no longer current.",
            "connection_unavailable" => "The qualified Codex 0.167.0 executable is unavailable.",
            "connection_cancel_pending" => {
                "Sign-in cleanup has not finished. Retry cancellation before leaving."
            }
            _ => "Provider sign-in could not be verified. Retry or skip setup.",
        },
    )
}
pub fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, BridgeError> {
    serde_json::from_value(value)
        .map_err(|_| BridgeError::invalid("Invalid provider connection fields."))
}
fn enabled() -> bool {
    cfg!(all(
        feature = "provider-auth-preview",
        debug_assertions,
        target_os = "linux"
    ))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn nonce() -> Result<String, BridgeError> {
    let mut bytes = [0; 24];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|_| error("connection_failed"))?;
    Ok(bytes.iter().map(|v| format!("{v:02x}")).collect())
}
fn valid_attempt(a: &Attempt, r: &Running) -> bool {
    a.attempt_id.len() == 48
        && r.view.attempt_id.as_ref() == Some(&a.attempt_id)
        && r.view.generation == a.generation
}
fn discover(home: &Path, stop: Option<&std::sync::atomic::AtomicBool>) -> Result<File, State> {
    if stop.is_some_and(|s| s.load(std::sync::atomic::Ordering::Acquire)) {
        return Err(State::Unsupported);
    }
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    let path = home.join(".local/bin/codex");
    let mut f = File::open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            State::NotInstalled
        } else {
            State::Unsupported
        }
    })?;
    let meta = f.metadata().map_err(|_| State::Unsupported)?;
    #[cfg(unix)]
    if meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o022 != 0
        || meta.mode() & 0o111 == 0
    {
        return Err(State::Unsupported);
    }
    if !meta.is_file() || meta.len() != 320_575_976 {
        return Err(State::Unsupported);
    }
    let mut hash = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        if stop.is_some_and(|s| s.load(std::sync::atomic::Ordering::Acquire)) {
            return Err(State::Unsupported);
        }
        let n = f.read(&mut buf).map_err(|_| State::Unsupported)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    if hash
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        != PIN
    {
        return Err(State::Unsupported);
    }
    Ok(f)
}
impl Connections {
    pub fn list(&self) -> View {
        if !enabled() {
            return View::new(State::Unsupported, "PREVIEW_REQUIRED");
        }
        if let Some(r) = self.running.lock().unwrap().as_mut() {
            expire_view(&mut r.view, now());
            return r.view.clone();
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        match home.and_then(|p| discover(&p, None).ok().map(|_| p)) {
            Some(_) => View::new(State::Available, "SIGN_IN_REQUIRED"),
            None => {
                let state = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|p| discover(&p, None).err().unwrap_or(State::Unsupported))
                    .unwrap_or(State::Unsupported);
                View::new(state, "EXECUTABLE_UNQUALIFIED")
            }
        }
    }
    pub fn begin(&self, args: Begin, root: &Path) -> Result<View, BridgeError> {
        if !enabled() {
            return Err(error("connection_disabled"));
        }
        if args.provider != "codex" || args.intent != "connect_provider" {
            return Err(BridgeError::invalid(
                "Select the supported provider and explicitly request Connect.",
            ));
        }
        let mut guard = self.running.lock().unwrap();
        if args.expected_generation != guard.as_ref().map_or(0, |r| r.view.generation) {
            return Err(error("connection_stale"));
        }
        if guard.as_ref().is_some_and(|r| !*r.done.0.lock().unwrap()) {
            return Err(error("connection_busy"));
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| error("connection_unavailable"))?;
        let id = nonce()?;
        let dir = private_attempt(root, &id)?;
        let generation = guard
            .as_ref()
            .map_or(Some(1), |r| r.view.generation.checked_add(1))
            .ok_or_else(|| error("connection_stale"))?;
        let mut view = View::new(State::Connecting, "STARTING_SIGN_IN");
        view.attempt_id = Some(id.clone());
        view.generation = generation;
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(1);
        let done = Arc::new((Mutex::new(false), Condvar::new()));
        *guard = Some(Running {
            view: view.clone(),
            stop: stop.clone(),
            check: tx,
            done: done.clone(),
        });
        drop(guard);
        let shared = self.running.clone();
        std::thread::spawn(move || {
            let result = match discover(&home, Some(&*stop)) {
                Ok(binary) => run(binary, &dir, &home, &shared, &id, generation, &stop, rx),
                Err(_) => Err("EXECUTABLE_UNQUALIFIED"),
            };
            if let Err(reason) = result {
                if !stop.load(std::sync::atomic::Ordering::Acquire) {
                    update(&shared, &id, generation, State::Failed, reason, None, None);
                }
            }
            // Mark this worker stopped only after its owned child has been reaped.
            stop.store(true, std::sync::atomic::Ordering::Release);
            *done.0.lock().unwrap() = true;
            done.1.notify_all();
        });
        Ok(view)
    }
    pub fn poll(&self, a: Attempt) -> Result<View, BridgeError> {
        if !enabled() {
            return Err(error("connection_disabled"));
        }
        let mut guard = self.running.lock().unwrap();
        let r = guard
            .as_mut()
            .filter(|r| valid_attempt(&a, r))
            .ok_or_else(|| error("connection_stale"))?;
        expire_view(&mut r.view, now());
        Ok(r.view.clone())
    }
    pub fn check(&self, a: Attempt) -> Result<View, BridgeError> {
        if !enabled() {
            return Err(error("connection_disabled"));
        }
        let guard = self.running.lock().unwrap();
        let r = guard
            .as_ref()
            .filter(|r| valid_attempt(&a, r))
            .ok_or_else(|| error("connection_stale"))?;
        if !matches!(r.view.state, State::Connected | State::Expired)
            || r.stop.load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(error("connection_stale"));
        }
        r.check
            .try_send(())
            .map_err(|_| error("connection_stale"))?;
        Ok(r.view.clone())
    }
    pub fn cancel(&self, a: Attempt) -> Result<View, BridgeError> {
        if !enabled() {
            return Err(error("connection_disabled"));
        }
        let done = {
            let mut guard = self.running.lock().unwrap();
            let r = guard
                .as_mut()
                .filter(|r| valid_attempt(&a, r))
                .ok_or_else(|| error("connection_stale"))?;
            r.stop.store(true, std::sync::atomic::Ordering::Release);
            r.done.clone()
        };
        let result = done
            .1
            .wait_timeout_while(done.0.lock().unwrap(), Duration::from_secs(3), |finished| {
                !*finished
            })
            .unwrap();
        if !*result.0 {
            return Err(error("connection_cancel_pending"));
        }
        drop(result);
        let mut guard = self.running.lock().unwrap();
        let r = guard
            .as_mut()
            .filter(|r| valid_attempt(&a, r))
            .ok_or_else(|| error("connection_stale"))?;
        r.view.state = State::AuthRequired;
        r.view.reason = "CANCELLED";
        r.view.user_code = None;
        r.view.checked_at = None;
        Ok(r.view.clone())
    }
}

impl Drop for Connections {
    fn drop(&mut self) {
        let done = self.running.lock().ok().and_then(|g| {
            g.as_ref().map(|r| {
                r.stop.store(true, std::sync::atomic::Ordering::Release);
                r.done.clone()
            })
        });
        if let Some(done) = done {
            let _result = done.1.wait_timeout_while(
                done.0.lock().unwrap(),
                Duration::from_secs(3),
                |finished| !*finished,
            );
        }
    }
}

fn update(
    inner: &Mutex<Option<Running>>,
    id: &str,
    generation: u64,
    state: State,
    reason: &'static str,
    code: Option<String>,
    checked: Option<u64>,
) {
    let mut guard = inner.lock().unwrap();
    if let Some(r) = guard.as_mut() {
        if r.view.attempt_id.as_deref() == Some(id)
            && r.view.generation == generation
            && !r.stop.load(std::sync::atomic::Ordering::Acquire)
        {
            r.view.state = state;
            r.view.reason = reason;
            r.view.user_code = code;
            r.view.checked_at = checked;
        }
    }
}

fn private_attempt(root: &Path, id: &str) -> Result<PathBuf, BridgeError> {
    #[cfg(unix)]
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    #[cfg(unix)]
    {
        // Other local users must not be able to replace an ancestor/config directory.
        for ancestor in root.ancestors() {
            let m = std::fs::symlink_metadata(ancestor).map_err(|_| error("connection_failed"))?;
            let trusted_owner = m.uid() == 0 || m.uid() == unsafe { libc::geteuid() };
            let protected_sticky = m.uid() == 0 && m.mode() & 0o1000 != 0;
            if !m.is_dir()
                || m.file_type().is_symlink()
                || !trusted_owner
                || m.mode() & 0o022 != 0 && !protected_sticky
            {
                return Err(error("connection_failed"));
            }
        }
    }
    if id.len() != 48
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(error("connection_failed"));
    }
    let parent = root.join("provider-auth-preview");
    let ensure = |p: &Path| -> Result<(), BridgeError> {
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        b.mode(0o700);
        match b.create(p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(error("connection_failed")),
        }
        let m = std::fs::symlink_metadata(p).map_err(|_| error("connection_failed"))?;
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err(error("connection_failed"));
        }
        #[cfg(unix)]
        if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
            return Err(error("connection_failed"));
        }
        Ok(())
    };
    ensure(&parent)?;
    let dir = parent.join(id);
    ensure(&dir)?;
    Ok(dir)
}
struct Rpc {
    child: Child,
    input: ChildStdin,
    rx: mpsc::Receiver<Result<Value, ()>>,
    reader: Option<std::thread::JoinHandle<()>>,
    next: u64,
    deferred: Vec<Value>,
}
impl Rpc {
    #[cfg(target_os = "linux")]
    fn start(binary: &File, dir: &Path) -> Result<Self, &'static str> {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        let parent_pid = unsafe { libc::getpid() };
        let mut command = Command::new(format!("/proc/self/fd/{}", binary.as_raw_fd()));
        command
            .args([
                "app-server",
                "--listen",
                "stdio://",
                "--strict-config",
                "-c",
                "cli_auth_credentials_store=\"ephemeral\"",
                "-c",
                "check_for_update_on_startup=false",
                "-c",
                "analytics.enabled=false",
            ])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", dir)
            .env("XDG_CONFIG_HOME", dir.join("config"))
            .env("XDG_CACHE_HOME", dir.join("cache"))
            .env("TMPDIR", dir)
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() < 0
                    || libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) < 0
                    || libc::getppid() != parent_pid
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().map_err(|_| "PROCESS_START_FAILED")?;
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::sync_channel(16);
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            for _ in 0..128 {
                let mut bytes = Vec::new();
                match (&mut reader).take(FRAME + 1).read_until(b'\n', &mut bytes) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if bytes.len() as u64 > FRAME => {
                        let _ = tx.send(Err(()));
                        break;
                    }
                    Ok(_) => {
                        if tx
                            .send(serde_json::from_slice(&bytes).map_err(|_| ()))
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });
        Ok(Self {
            child,
            input,
            rx,
            reader: Some(reader),
            next: 0,
            deferred: Vec::new(),
        })
    }
    #[cfg(not(target_os = "linux"))]
    fn start(_binary: &File, _dir: &Path) -> Result<Self, &'static str> {
        Err("UNSUPPORTED_PLATFORM")
    }
    fn message(
        &self,
        until: Instant,
        stop: &std::sync::atomic::AtomicBool,
    ) -> Result<Value, &'static str> {
        loop {
            if stop.load(std::sync::atomic::Ordering::Acquire) {
                return Err("CANCELLED");
            }
            if Instant::now() >= until {
                return Err("AUTH_TIMEOUT");
            }
            match self.rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Ok(v)) => return Ok(v),
                Ok(Err(_)) => return Err("INVALID_PROTOCOL"),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err("PROCESS_ENDED"),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
    fn call(
        &mut self,
        method: &str,
        params: Value,
        stop: &std::sync::atomic::AtomicBool,
    ) -> Result<Value, &'static str> {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Err("CANCELLED");
        }
        // This function is private; methods are literals at call sites, never IPC input.
        if ![
            "initialize",
            "account/read",
            "account/login/start",
            "account/rateLimits/read",
        ]
        .contains(&method)
        {
            return Err("METHOD_NOT_ALLOWED");
        }
        self.next += 1;
        let id = self.next;
        writeln!(
            self.input,
            "{}",
            json!({"id":id,"method":method,"params":params})
        )
        .map_err(|_| "PROCESS_WRITE_FAILED")?;
        self.input.flush().map_err(|_| "PROCESS_WRITE_FAILED")?;
        let until = Instant::now() + Duration::from_secs(20);
        loop {
            let v = self.message(until, stop)?;
            if v.get("id").and_then(Value::as_u64) == Some(id) && v.get("method").is_none() {
                if v.get("error").is_some() {
                    return Err("AUTH_CHECK_FAILED");
                }
                return v.get("result").cloned().ok_or("INVALID_PROTOCOL");
            }
            if v.get("id").is_some() || self.deferred.len() >= 16 {
                return Err("UNEXPECTED_PROTOCOL");
            }
            self.deferred.push(v);
        }
    }
}
impl Drop for Rpc {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Disconnect before joining so a saturated reader cannot block cleanup.
        let (_tx, replacement) = mpsc::channel();
        let old = std::mem::replace(&mut self.rx, replacement);
        drop(old);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
fn challenge(v: &Value) -> Result<(String, String), &'static str> {
    let id = v
        .get("loginId")
        .and_then(Value::as_str)
        .filter(|s| s.len() <= 128 && !s.is_empty())
        .ok_or("INVALID_CHALLENGE")?;
    let code = v
        .get("userCode")
        .and_then(Value::as_str)
        .filter(|s| {
            (4..=16).contains(&s.len())
                && s.bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
        })
        .ok_or("INVALID_CHALLENGE")?;
    if v.get("type").and_then(Value::as_str) != Some("chatgptDeviceCode")
        || v.get("verificationUrl").and_then(Value::as_str) != Some(PAGE)
    {
        return Err("UNAUTHORIZED_REDIRECT");
    }
    Ok((id.into(), code.into()))
}
fn completed(v: &Value, id: &str) -> Result<bool, &'static str> {
    if v.get("id").is_some() {
        return Err("UNEXPECTED_PROTOCOL");
    }
    if v.get("method").and_then(Value::as_str) != Some("account/login/completed") {
        return Ok(false);
    }
    let p = &v["params"];
    if p.get("loginId").and_then(Value::as_str) != Some(id) {
        return Err("STALE_LOGIN");
    }
    if p.get("success") == Some(&Value::Bool(true)) && p.get("error").is_none_or(Value::is_null) {
        Ok(true)
    } else {
        Err("AUTH_NOT_COMPLETED")
    }
}
fn open_official_page(
    home: &Path,
    stop: &std::sync::atomic::AtomicBool,
) -> Result<(), &'static str> {
    if stop.load(std::sync::atomic::Ordering::Acquire) {
        return Err("CANCELLED");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata("/usr/bin/xdg-open").map_err(|_| "BROWSER_UNAVAILABLE")?;
        if !m.is_file() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err("BROWSER_UNQUALIFIED");
        }
    }
    let mut cmd = Command::new("/usr/bin/xdg-open");
    cmd.arg(PAGE)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for name in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_RUNTIME_DIR",
        "XDG_CURRENT_DESKTOP",
    ] {
        if let Some(v) = std::env::var_os(name) {
            cmd.env(name, v);
        }
    }
    let mut child = cmd.spawn().map_err(|_| "BROWSER_UNAVAILABLE")?;
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(s)) => {
                return if s.success() {
                    Ok(())
                } else {
                    Err("BROWSER_UNAVAILABLE")
                }
            }
            Err(_) => return Err("BROWSER_UNAVAILABLE"),
            _ => {}
        }
        if stop.load(std::sync::atomic::Ordering::Acquire) || Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            return Err("BROWSER_TIMEOUT");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn expire_view(view: &mut View, current: u64) {
    if view.state == State::Connected
        && view
            .checked_at
            .is_none_or(|t| current < t || current.saturating_sub(t) >= PROOF_TTL.as_secs())
    {
        view.state = State::Expired;
        view.reason = "CHECK_REQUIRED";
        view.checked_at = None;
    }
}
fn verify_read(result: &Value) -> Result<(), &'static str> {
    if result.get("rateLimits").is_some_and(Value::is_object) {
        Ok(())
    } else {
        Err("INVALID_AUTH_CHECK")
    }
}
fn run(
    binary: File,
    dir: &Path,
    home: &Path,
    shared: &Mutex<Option<Running>>,
    id: &str,
    generation: u64,
    stop: &std::sync::atomic::AtomicBool,
    checks: mpsc::Receiver<()>,
) -> Result<(), &'static str> {
    if stop.load(std::sync::atomic::Ordering::Acquire) {
        return Ok(());
    }
    let mut rpc = Rpc::start(&binary, dir)?;
    rpc.call("initialize",json!({"clientInfo":{"name":"tako_auth_preview","version":"0.0.1"},"capabilities":{"experimentalApi":false,"explicitGatewayOauth":true}}),stop)?;
    writeln!(rpc.input, "{}", json!({"method":"initialized","params":{}}))
        .map_err(|_| "PROCESS_WRITE_FAILED")?;
    rpc.input.flush().map_err(|_| "PROCESS_WRITE_FAILED")?;
    let account = rpc.call("account/read", json!({"refreshToken":false}), stop)?;
    if account.get("account") != Some(&Value::Null)
        || account.get("requiresOpenaiAuth") != Some(&Value::Bool(true))
    {
        return Err("ISOLATION_NOT_CONFIRMED");
    }
    let response = rpc.call(
        "account/login/start",
        json!({"type":"chatgptDeviceCode"}),
        stop,
    )?;
    let (login_id, code) = challenge(&response)?;
    if stop.load(std::sync::atomic::Ordering::Acquire) {
        return Err("CANCELLED");
    }
    update(
        shared,
        id,
        generation,
        State::Connecting,
        "ENTER_DEVICE_CODE",
        Some(code),
        None,
    );
    open_official_page(home, stop)?;
    let until = Instant::now() + DEADLINE;
    loop {
        let v = if rpc.deferred.is_empty() {
            rpc.message(until, stop)?
        } else {
            rpc.deferred.remove(0)
        };
        if completed(&v, &login_id)? {
            break;
        }
    }
    // A cache hit is never enough. This method performs an authenticated, read-only network request.
    verify_read(&rpc.call("account/rateLimits/read", json!({}), stop)?)?;
    let mut verified = Instant::now();
    update(
        shared,
        id,
        generation,
        State::Connected,
        "AUTHENTICATED_METADATA_READ",
        None,
        Some(now()),
    );
    loop {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(());
        }
        match checks.recv_timeout(Duration::from_millis(50)) {
            Ok(()) => {
                update(
                    shared,
                    id,
                    generation,
                    State::Expired,
                    "CHECKING_CONNECTION",
                    None,
                    None,
                );
                verify_read(&rpc.call("account/rateLimits/read", json!({}), stop)?)?;
                verified = Instant::now();
                update(
                    shared,
                    id,
                    generation,
                    State::Connected,
                    "AUTHENTICATED_METADATA_READ",
                    None,
                    Some(now()),
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if verified.elapsed() > PROOF_TTL {
            update(
                shared,
                id,
                generation,
                State::Expired,
                "CHECK_REQUIRED",
                None,
                None,
            );
        }
        match rpc.rx.try_recv() {
            Ok(Ok(v)) => {
                if v.get("id").is_some() {
                    return Err("UNEXPECTED_PROTOCOL");
                }
                if v["method"] == "account/updated"
                    && v["params"]["authMode"] != Value::String("chatgpt".into())
                {
                    return Err("SESSION_CHANGED");
                }
            }
            Ok(Err(_)) => return Err("INVALID_PROTOCOL"),
            Err(mpsc::TryRecvError::Disconnected) => return Err("PROCESS_ENDED"),
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    fn challenge_value() -> Value {
        json!({"type":"chatgptDeviceCode","loginId":"fixture-login","verificationUrl":PAGE,"userCode":"ABCD-1234"})
    }
    fn attempt() -> Attempt {
        Attempt {
            attempt_id: "a".repeat(48),
            generation: 1,
        }
    }
    fn fixture(state: State) -> (Connections, mpsc::Receiver<()>, Arc<(Mutex<bool>, Condvar)>) {
        let (check_tx, check_rx) = mpsc::sync_channel(1);
        let done = Arc::new((Mutex::new(true), Condvar::new()));
        let mut view = View::new(state, "TEST_FIXTURE_ONLY");
        view.attempt_id = Some(attempt().attempt_id);
        view.generation = 1;
        (
            Connections {
                running: Arc::new(Mutex::new(Some(Running {
                    view,
                    stop: Arc::new(AtomicBool::new(false)),
                    check: check_tx,
                    done: done.clone(),
                }))),
            },
            check_rx,
            done,
        )
    }
    #[test]
    fn exact_official_device_page_only() {
        assert!(challenge(&challenge_value()).is_ok());
        for url in [
            "https://auth.openai.com.evil/codex/device",
            "https://user@auth.openai.com/codex/device",
            "http://auth.openai.com/codex/device",
            "https://auth.openai.com/codex/device?redirect=evil",
            "https://chatgpt.com/codex/device",
        ] {
            let mut v = challenge_value();
            v["verificationUrl"] = url.into();
            assert_eq!(challenge(&v).err(), Some("UNAUTHORIZED_REDIRECT"));
        }
    }
    #[test]
    fn malformed_challenges_do_not_expose_codes() {
        for v in [
            json!({}),
            json!({"type":"apiKey","apiKey":"SENTINEL"}),
            json!({"type":"chatgptAuthTokens","accessToken":"SENTINEL"}),
        ] {
            assert!(challenge(&v).is_err());
        }
        for code in [
            "",
            "<script>",
            "abcde",
            "ABCD\n1234",
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
        ] {
            let mut v = challenge_value();
            v["userCode"] = code.into();
            assert!(challenge(&v).is_err());
        }
    }
    #[test]
    fn completion_requires_exact_attempt_and_success() {
        let v = json!({"method":"account/login/completed","params":{"loginId":"fixture-login","success":true,"error":null}});
        assert_eq!(completed(&v, "fixture-login"), Ok(true));
        assert_eq!(completed(&v, "other"), Err("STALE_LOGIN"));
        for params in [
            json!({"loginId":"fixture-login","success":false,"error":"SENTINEL"}),
            json!({"loginId":"fixture-login","success":true,"error":"SENTINEL"}),
            json!({"loginId":"fixture-login"}),
        ] {
            assert_eq!(
                completed(
                    &json!({"method":"account/login/completed","params":params}),
                    "fixture-login"
                ),
                Err("AUTH_NOT_COMPLETED")
            );
        }
        assert_eq!(
            completed(&json!({"id":9,"method":"command/exec"}), "fixture-login"),
            Err("UNEXPECTED_PROTOCOL")
        );
        assert_eq!(
            completed(
                &json!({"method":"account/updated","params":{"authMode":"chatgpt"}}),
                "fixture-login"
            ),
            Ok(false)
        );
    }
    #[test]
    fn account_metadata_is_not_authenticated_read_proof() {
        for v in [
            json!({"account":{"type":"chatgpt"}}),
            json!({"rateLimits":null}),
            Value::Null,
            json!({"success":true}),
        ] {
            assert_eq!(verify_read(&v), Err("INVALID_AUTH_CHECK"));
        }
        // Structural fixture only, not evidence of an actual authenticated request.
        assert_eq!(verify_read(&json!({"rateLimits":{"primary":null}})), Ok(()));
    }
    #[test]
    fn untrusted_native_fields_rejected() {
        for v in [
            json!({"provider":"codex","intent":"connect_provider","access_token":"SENTINEL"}),
            json!({"provider":"codex","intent":"connect_provider","command":"exec"}),
            json!({"provider":"codex","intent":"connect_provider","url":"https://evil.invalid"}),
            json!({"provider":"codex","intent":"connect_provider","actor":"human"}),
        ] {
            assert!(serde_json::from_value::<Begin>(v).is_err());
        }
        assert!(serde_json::from_value::<Attempt>(
            json!({"attempt_id":"x","generation":1,"token":"SENTINEL"})
        )
        .is_err());
    }
    #[test]
    fn cancelled_or_replaced_generation_cannot_be_resurrected() {
        let (service, _checks, done) = fixture(State::Connecting);
        let id = attempt().attempt_id;
        {
            let mut g = service.running.lock().unwrap();
            g.as_mut().unwrap().stop.store(true, Ordering::Release);
        }
        update(
            &service.running,
            &id,
            1,
            State::Connected,
            "LATE",
            None,
            Some(now()),
        );
        assert!(service.running.lock().unwrap().as_ref().unwrap().view.state == State::Connecting);
        {
            let mut g = service.running.lock().unwrap();
            let r = g.as_mut().unwrap();
            r.stop.store(false, Ordering::Release);
            r.view.generation = 2;
        }
        update(
            &service.running,
            &id,
            1,
            State::Connected,
            "LATE",
            None,
            Some(now()),
        );
        assert!(service.running.lock().unwrap().as_ref().unwrap().view.state == State::Connecting);
        drop(done);
    }
    #[test]
    fn cancel_acknowledgement_clears_code_and_proof() {
        let (service, _checks, done) = fixture(State::Connected);
        *done.0.lock().unwrap() = true;
        done.1.notify_all();
        if enabled() {
            let v = service.cancel(attempt()).unwrap();
            assert!(v.state == State::AuthRequired);
            assert!(v.user_code.is_none() && v.checked_at.is_none());
            assert!(!v.execution_authorized);
        }
        drop(done);
    }
    #[test]
    fn process_drop_does_not_wait_on_its_own_state_lock() {
        let (service, _checks, done) = fixture(State::Connecting);
        drop(done);
        drop(service);
    }
    #[test]
    fn stale_and_cross_attempt_requests_refused() {
        let (service, _checks, done) = fixture(State::Connected);
        if enabled() {
            assert!(service
                .poll(Attempt {
                    attempt_id: "b".repeat(48),
                    generation: 1
                })
                .is_err());
            assert!(service
                .cancel(Attempt {
                    attempt_id: attempt().attempt_id,
                    generation: 2
                })
                .is_err());
        }
        drop(done);
    }
    #[test]
    fn credential_free_discovery_refuses_wrong_binary() {
        let root = std::env::temp_dir().join(format!("tako-auth-discovery-{}", nonce().unwrap()));
        std::fs::create_dir(&root).unwrap();
        assert!(matches!(discover(&root, None), Err(State::NotInstalled)));
        std::fs::create_dir_all(root.join(".local/bin")).unwrap();
        std::fs::write(root.join(".local/bin/codex"), b"fixture, never executed").unwrap();
        assert!(matches!(discover(&root, None), Err(State::Unsupported)));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn unsafe_profile_symlink_is_rejected() {
        let root = std::env::temp_dir().join(format!("tako-auth-profile-{}", nonce().unwrap()));
        std::fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(std::env::temp_dir(), root.join("provider-auth-preview"))
            .unwrap();
        assert!(private_attempt(&root, "fixture").is_err());
        std::fs::remove_file(root.join("provider-auth-preview")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn no_execution_or_credentials_in_discovery_view() {
        let view = View::new(State::Unsupported, "PREVIEW_REQUIRED");
        let value = serde_json::to_value(view).unwrap();
        assert_eq!(value["execution_authorized"], false);
        assert!(value.get("accessToken").is_none() && value.get("refreshToken").is_none());
    }
    #[test]
    fn ordinary_build_never_starts_authentication() {
        if !enabled() {
            let service = Connections::default();
            assert!(service.list().state == State::Unsupported);
            assert_eq!(
                service
                    .begin(
                        Begin {
                            provider: "codex".into(),
                            intent: "connect_provider".into(),
                            expected_generation: 0,
                        },
                        Path::new("/unused")
                    )
                    .err()
                    .unwrap()
                    .code,
                "connection_disabled"
            );
        }
    }
    #[test]
    #[ignore = "Explicit installed official-client protocol check; isolated HOME, never starts login"]
    fn isolated_official_protocol() {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
        let measured = Instant::now();
        let binary = discover(&home, None)
            .ok()
            .expect("Pinned installed official client required");
        println!(
            "qualified_binary_discovery_ms={}",
            measured.elapsed().as_millis()
        );
        let root = std::env::temp_dir().join(format!("tako-auth-official-{}", nonce().unwrap()));
        std::fs::create_dir(&root).unwrap();
        let stop = AtomicBool::new(false);
        let mut rpc = Rpc::start(&binary, &root).unwrap();
        let init=rpc.call("initialize",json!({"clientInfo":{"name":"tako_auth_negative_qualification","version":"0.0.1"},"capabilities":{"experimentalApi":false,"explicitGatewayOauth":true}}),&stop).unwrap();
        assert!(init.get("userAgent").is_some());
        writeln!(rpc.input, "{}", json!({"method":"initialized","params":{}})).unwrap();
        rpc.input.flush().unwrap();
        let status = rpc
            .call("account/read", json!({"refreshToken":false}), &stop)
            .unwrap();
        assert_eq!(status["account"], Value::Null);
        assert_eq!(status["requiresOpenaiAuth"], true);
        assert_eq!(
            rpc.call("turn/start", json!({}), &stop).err(),
            Some("METHOD_NOT_ALLOWED")
        );
        drop(rpc);
        assert!(!root.join(".codex/auth.json").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cancellation_waits_without_holding_the_state_lock() {
        if !enabled() {
            return;
        }
        let (service, _checks, done) = fixture(State::Connecting);
        *done.0.lock().unwrap() = false;
        let state = service.running.clone();
        let complete = done.clone();
        let worker = std::thread::spawn(move || {
            // Deterministic handoff: wait for cancellation, then acquire state as a real worker can.
            loop {
                let stopped = state
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .stop
                    .load(Ordering::Acquire);
                if stopped {
                    break;
                }
                std::thread::yield_now();
            }
            update(
                &state,
                &attempt().attempt_id,
                1,
                State::Connected,
                "LATE",
                None,
                Some(now()),
            );
            *complete.0.lock().unwrap() = true;
            complete.1.notify_all();
        });
        let result = service.cancel(attempt()).unwrap();
        assert!(result.state == State::AuthRequired);
        worker.join().unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn writable_profile_ancestor_and_path_injection_are_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("tako-auth-unsafe-{}", nonce().unwrap()));
        std::fs::create_dir(&root).unwrap();
        assert!(private_attempt(&root, "../escape").is_err());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(private_attempt(&root, &nonce().unwrap()).is_err());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn proof_freshness_expires_on_suspend_time_or_clock_rollback() {
        for current in [99, 160, 10000] {
            let mut v = View::new(State::Connected, "FIXTURE_ONLY");
            v.checked_at = Some(100);
            expire_view(&mut v, current);
            assert!(v.state == State::Expired);
            assert!(v.checked_at.is_none());
        }
        let mut v = View::new(State::Connected, "FIXTURE_ONLY");
        v.checked_at = Some(100);
        expire_view(&mut v, 159);
        assert!(v.state == State::Connected);
    }
    #[test]
    fn invalid_native_scalar_is_redacted_before_ipc_error() {
        let e = parse::<Attempt>(
            json!({"attempt_id":"fixture","generation":"SENTINEL_NOT_A_CREDENTIAL"}),
        )
        .err()
        .unwrap();
        assert_eq!(e.code, "invalid_input");
        assert_eq!(e.message, "Invalid provider connection fields.");
        assert!(!e.message.contains("SENTINEL"));
    }
    #[test]
    fn old_connect_intent_cannot_restart_a_completed_generation() {
        if !enabled() {
            return;
        }
        let (service, _check, _done) = fixture(State::AuthRequired);
        let result = service.begin(
            Begin {
                provider: "codex".into(),
                intent: "connect_provider".into(),
                expected_generation: 0,
            },
            Path::new("/unused"),
        );
        assert_eq!(result.err().unwrap().code, "connection_stale");
    }
    #[test]
    fn cancellation_prevents_executable_verification_and_process_start() {
        let stop = AtomicBool::new(true);
        assert!(matches!(
            discover(Path::new("/unused"), Some(&stop)),
            Err(State::Unsupported)
        ));
    }
}
