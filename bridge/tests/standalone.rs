//! All network traffic here is an explicit ephemeral loopback fixture, never live T.
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc,
};
use std::time::{Duration, Instant};
use tako_bridge::{client::TakoBackendClient, Bridge};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct LocalFixture {
    root: PathBuf,
    project: PathBuf,
}
impl LocalFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tako-standalone-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        let o=Command::new("sh").args(["-c","git init -qb main && git config user.name fixture && git config user.email fixture@example.test && echo base > README.md && git add README.md && git commit -qm base"]).current_dir(&project).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        Self { root, project }
    }
}
impl Drop for LocalFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
struct Gateway {
    client: TakoBackendClient,
    online: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    started: mpsc::Receiver<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Gateway {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let client =
            TakoBackendClient::unauthenticated("127.0.0.1", listener.local_addr().unwrap().port());
        let online = Arc::new(AtomicBool::new(true));
        let pause = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (sent, started) = mpsc::channel();
        let (on, p, r, s) = (
            online.clone(),
            pause.clone(),
            requests.clone(),
            stop.clone(),
        );
        let thread = std::thread::spawn(move || {
            while !s.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buf = [0; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = stream.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buf[..n]);
                }
                r.fetch_add(1, Ordering::SeqCst);
                let text = String::from_utf8_lossy(&request);
                let path = text.split_whitespace().nth(1).unwrap_or("");
                if path == "/health" {
                    let _ = sent.send(());
                    while p.load(Ordering::SeqCst) && !s.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
                let (code, body) = if !on.load(Ordering::SeqCst) {
                    (503, json!({"error":"fixture unavailable"}))
                } else {
                    (
                        200,
                        match path {
                            "/health" => {
                                json!({"status":"fixture healthy","operational_mode":"read_only","g12_state":"UNKNOWN"})
                            }
                            "/api/v1/projects" => {
                                json!({"projects":[],"total":0,"allowed_roots":[]})
                            }
                            "/api/v1/approvals" => json!({"approvals":[],"total":0}),
                            "/api/v1/workstations" => {
                                json!({"workstations":[],"total":0,"timestamp":"fixture"})
                            }
                            _ => json!({"error":"fixture route unavailable"}),
                        },
                    )
                };
                let body = body.to_string();
                let _=write!(stream,"HTTP/1.1 {code} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
            }
        });
        Self {
            client,
            online,
            pause,
            requests,
            stop,
            started,
            thread: Some(thread),
        }
    }
}
impl Drop for Gateway {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.pause.store(false, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}
fn call(b: &Bridge, cmd: &str, args: Value) -> Value {
    b.dispatch(cmd, args)
        .unwrap_or_else(|e| panic!("{cmd}: {} {}", e.code, e.message))
}

#[test]
fn default_core_is_standalone_and_disabled_backend_commands_do_not_fall_back() {
    let fx = LocalFixture::new();
    let b = Bridge::open(fx.root.join("data")).unwrap();
    let state = call(&b, "integration_status", Value::Null);
    assert_eq!(state["mode"], "standalone");
    assert_eq!(state["availability"], "disabled");
    assert_eq!(state["provider_execution"], "disabled");
    let status = call(&b, "backend_status", Value::Null);
    assert_eq!(status["status"], "DISABLED");
    assert_eq!(status["online"], false);
    assert_eq!(
        b.dispatch("backend_projects", Value::Null)
            .unwrap_err()
            .code,
        "integration_disabled"
    );
    assert_eq!(
        b.dispatch(
            "backend_mission_evidence",
            json!({"project_id":"remote","mission_id":"T-missing"})
        )
        .unwrap_err()
        .code,
        "integration_disabled"
    );
    assert_eq!(
        b.dispatch("get_mission", json!({"mission_id":"T-missing"}))
            .unwrap_err()
            .code,
        "not_found"
    );
    let project = call(&b, "open_project", json!({"path":fx.project}));
    assert_eq!(project["source"], "local");
    assert!(project["backend_project_id"].is_null());
}
#[test]
fn availability_transitions_never_change_the_local_engine_or_permissions() {
    let fx = LocalFixture::new();
    let gw = Gateway::new();
    let b = Bridge::open_with_client(fx.root.join("data"), gw.client.clone()).unwrap();
    assert_eq!(
        call(&b, "integration_status", Value::Null)["availability"],
        "not_checked"
    );
    assert_eq!(call(&b, "backend_status", Value::Null)["online"], true);
    assert_eq!(
        call(&b, "integration_status", Value::Null)["availability"],
        "available"
    );
    gw.online.store(false, Ordering::SeqCst);
    assert_eq!(call(&b, "backend_status", Value::Null)["online"], false);
    assert_eq!(
        call(&b, "integration_status", Value::Null)["availability"],
        "unavailable"
    );
    let before = gw.requests.load(Ordering::SeqCst);
    let p = call(&b, "open_project", json!({"path":fx.project}));
    assert_eq!(p["source"], "local");
    let m = call(
        &b,
        "create_mission",
        json!({"project_path":fx.project,"goal":"standalone","criteria":["c"],"checks":["true"]}),
    );
    let plan = call(
        &b,
        "define_plan",
        json!({"mission_id":m["mission"]["id"],"script":"echo local > out.txt","expected_files":["out.txt"]}),
    );
    assert!(plan["mission"]["approval"].is_null());
    assert_eq!(plan["isolation"]["is_security_sandbox"], false);
    assert_eq!(
        gw.requests.load(Ordering::SeqCst),
        before,
        "local commands must issue no adapter request"
    );
    gw.online.store(true, Ordering::SeqCst);
    assert_eq!(call(&b, "backend_status", Value::Null)["online"], true);
    call(&b, "configure_integration", json!({"enabled":false}));
    let before = gw.requests.load(Ordering::SeqCst);
    call(&b, "backend_status", Value::Null);
    assert_eq!(gw.requests.load(Ordering::SeqCst), before);
    assert_eq!(
        call(&b, "integration_status", Value::Null)["availability"],
        "disabled"
    );
}
#[test]
fn local_commands_and_disconnect_do_not_wait_for_an_optional_probe() {
    let fx = LocalFixture::new();
    let gw = Gateway::new();
    gw.pause.store(true, Ordering::SeqCst);
    let b = Arc::new(Bridge::open_with_client(fx.root.join("data"), gw.client.clone()).unwrap());
    let remote = b.clone();
    let job = std::thread::spawn(move || remote.dispatch("backend_status", Value::Null));
    gw.started.recv_timeout(Duration::from_secs(2)).unwrap();
    let local = b.clone();
    let path = fx.project.clone();
    let (sent, received) = mpsc::channel();
    let local_job = std::thread::spawn(move || {
        let result = local.dispatch("open_project", json!({"path":path}));
        let _ = sent.send(result);
    });
    let result = received.recv_timeout(Duration::from_secs(1));
    call(&b, "configure_integration", json!({"enabled":false}));
    gw.pause.store(false, Ordering::SeqCst);
    assert!(
        result.unwrap().is_ok(),
        "local work must complete while the probe is withheld"
    );
    local_job.join().unwrap();
    assert_eq!(job.join().unwrap().unwrap_err().code, "integration_changed");
    assert_eq!(
        call(&b, "integration_status", Value::Null)["availability"],
        "disabled"
    );
}
#[test]
fn standalone_startup_does_not_even_open_the_optional_credential_path() {
    let fx = LocalFixture::new();
    let home = fx.root.join("home");
    fs::create_dir_all(home.join(".t-control")).unwrap();
    let status = Command::new("mkfifo")
        .arg(home.join(".t-control/operator.token"))
        .status()
        .unwrap();
    assert!(status.success());
    let mut child = Command::new(env!("CARGO_BIN_EXE_tako-bridge-stdio"))
        .arg(fx.root.join("child-data"))
        .env("HOME", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"{\"id\":1,\"cmd\":\"startup_report\"}\n{\"id\":2,\"cmd\":\"integration_status\"}\n{\"id\":3,\"cmd\":\"backend_status\"}\n{\"id\":4,\"cmd\":\"configure_integration\",\"args\":{\"enabled\":true}}\n{\"id\":5,\"cmd\":\"configure_integration\",\"args\":{\"enabled\":false}}\n").unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(3) {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("standalone startup waited on optional credentials");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let lines: Vec<Value> = BufReader::new(output.stdout.as_slice())
        .lines()
        .map(|l| serde_json::from_str(&l.unwrap()).unwrap())
        .collect();
    assert_eq!(lines[1]["ok"]["mode"], "standalone");
    assert_eq!(lines[2]["ok"]["status"], "DISABLED");
    assert_eq!(lines[3]["ok"]["mode"], "optional_backend");
    assert_eq!(lines[3]["ok"]["availability"], "not_checked");
    assert_eq!(lines[4]["ok"]["mode"], "standalone");
}

#[test]
fn protected_paths_can_be_inspected_offline_but_cannot_bypass_mutation_guards() {
    let mut fx = LocalFixture::new();
    let parent = fx.root.join("projects");
    fs::create_dir_all(&parent).unwrap();
    let protected = parent.join("t-offline");
    fs::rename(&fx.project, &protected).unwrap();
    fx.project = protected;
    let b = Bridge::open(fx.root.join("data")).unwrap();
    assert_eq!(
        call(&b, "open_project", json!({"path":fx.project}))["source"],
        "local"
    );
    let alias = fx.root.join("alias");
    std::os::unix::fs::symlink(&fx.project, &alias).unwrap();
    assert_eq!(
        b.dispatch(
            "create_mission",
            json!({"project_path":alias,"goal":"g","criteria":[],"checks":[]})
        )
        .unwrap_err()
        .code,
        "authorization_required"
    );
}
