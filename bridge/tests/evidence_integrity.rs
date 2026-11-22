//! All gateway responses below are synthetic TEST FIXTURES, never production evidence.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tako_bridge::{client::TakoBackendClient, Bridge};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: std::path::PathBuf,
    bridge: Bridge,
    stop: Arc<AtomicBool>,
    server: Option<std::thread::JoinHandle<()>>,
}
impl Fixture {
    fn new(routes: Vec<(&'static str, u16, Value)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let server = std::thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("fixture accept: {e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buf = [0; 1024];
                while !request.windows(4).any(|s| s == b"\r\n\r\n") {
                    let n = stream.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buf[..n]);
                }
                let request = String::from_utf8_lossy(&request);
                let path = request.split_whitespace().nth(1).unwrap_or("");
                let (code, body) = routes
                    .iter()
                    .find(|r| r.0 == path)
                    .map(|r| (r.1, r.2.clone()))
                    .unwrap_or((404, json!({"error": "fixture route missing"})));
                let body = body.to_string();
                let _ = write!(stream, "HTTP/1.1 {code} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        let root = std::env::temp_dir().join(format!(
            "tako-integrity-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let bridge =
            Bridge::open_with_client(&root, TakoBackendClient::unauthenticated("127.0.0.1", port))
                .unwrap();
        Self {
            root,
            bridge,
            stop,
            server: Some(server),
        }
    }
    fn call(&self, cmd: &str, args: Value) -> Result<Value, tako_bridge::BridgeError> {
        self.bridge.dispatch(cmd, args)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.server.take().unwrap().join().unwrap();
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
fn projects() -> Value {
    json!({"projects": [{"project_id": "fixture", "name": "Fixture", "path": "/unavailable/fixture", "exists": false,
        "is_git": true, "branch": "fixture-branch", "head": "fixture-head", "dirty": true,
        "allowed_root": "/unavailable", "isolation_profile": "L1_bubblewrap"}], "total": 1, "allowed_roots": []})
}
#[test]
fn backend_completion_is_unverified_and_never_fabricates_a_mission() {
    let fx = Fixture::new(vec![
        ("/api/v1/projects", 200, projects()),
        (
            "/api/v1/projects/fixture/goals",
            200,
            json!({"project_id": "fixture", "total": 1,
            "goals": [{"id": "PROG-fixture", "project_id": "fixture", "goal": "Fixture goal", "state": "completed",
                "progress_percent": 100.0, "outcome_verification": "EVIDENCE_CONSISTENT", "requires_human_attention": false}]}),
        ),
    ]);
    let view = fx
        .call("open_backend_project", json!({"project_id": "fixture"}))
        .unwrap();
    assert!(view["missions"][0]["state"].is_null());
    assert_eq!(view["missions"][0]["reported_backend_state"], "completed");
    assert_eq!(view["project"]["head"], "fixture-head");
    assert_eq!(view["project"]["dirty"], true);
    assert_eq!(view["isolation"]["is_security_sandbox"], false);
    assert_eq!(view["isolation"]["provider_execution"], "disabled");
    for id in [
        "PROG-fixture",
        "PROG-unknown",
        "GOAL-unknown",
        "T-unknown",
        "unknown",
    ] {
        assert_eq!(
            fx.call("get_mission", json!({"mission_id": id}))
                .unwrap_err()
                .code,
            "not_found"
        );
    }
}
#[test]
fn gateway_http_errors_and_missing_goal_data_are_explicit() {
    for code in [401, 404, 503] {
        let fx = Fixture::new(vec![(
            "/api/v1/projects",
            code,
            json!({"error": "fixture failure"}),
        )]);
        assert_eq!(
            fx.call("open_backend_project", json!({"project_id": "fixture"}))
                .unwrap_err()
                .code,
            "backend_unavailable"
        );
        assert!(!fx.call("backend_status", Value::Null).unwrap()["online"]
            .as_bool()
            .unwrap());
        assert_eq!(
            fx.call("backend_governance", Value::Null).unwrap_err().code,
            "backend_unavailable"
        );
    }
    for body in [json!({"goals": []}), json!({"error": "missing fields"})] {
        let fx = Fixture::new(vec![
            ("/api/v1/projects", 200, projects()),
            ("/api/v1/projects/fixture/goals", 200, body),
        ]);
        assert_eq!(
            fx.call("open_backend_project", json!({"project_id": "fixture"}))
                .unwrap_err()
                .code,
            "backend_unavailable"
        );
    }
}
#[test]
fn unreachable_gateway_never_implies_success() {
    let mut fx = Fixture::new(vec![]);
    fx.bridge =
        Bridge::open_with_client(&fx.root, TakoBackendClient::unauthenticated("127.0.0.1", 0))
            .unwrap();
    let status = fx.call("backend_status", Value::Null).unwrap();
    assert_eq!(status["online"], false);
    assert_eq!(status["runtime_status"], "UNREACHABLE");
    assert_eq!(
        fx.call("backend_governance", Value::Null).unwrap_err().code,
        "backend_unavailable"
    );
    assert_eq!(
        fx.call("open_backend_project", json!({"project_id": "fixture"}))
            .unwrap_err()
            .code,
        "backend_unavailable"
    );
    for id in ["PROG-unknown", "GOAL-unknown", "T-unknown"] {
        assert_eq!(
            fx.call("get_mission", json!({"mission_id": id}))
                .unwrap_err()
                .code,
            "not_found"
        );
    }
}
#[test]
fn incomplete_health_does_not_invent_healthy_idle_or_governance_states() {
    let fx = Fixture::new(vec![("/health", 200, json!({}))]);
    let status = fx.call("backend_status", Value::Null).unwrap();
    assert_eq!(status["status"], "UNKNOWN");
    assert_eq!(status["runtime_status"], "UNKNOWN");
    assert_eq!(status["g12_state"], "UNKNOWN");
    assert!(status["limits"].is_null());
}
