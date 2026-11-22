//! Process-isolated environment tests: no mutation of the test runner's credentials.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};
use std::{fs, path::PathBuf};
static N: AtomicUsize = AtomicUsize::new(0);
const KEY: &str = "native-fixture-credential-only";
struct Desktop {
    child: Child,
    stdin: ChildStdin,
    replies: mpsc::Receiver<Value>,
    root: PathBuf,
    reader: Option<std::thread::JoinHandle<()>>,
}
impl Desktop {
    fn new(key: Option<&str>) -> Self {
        let root = std::env::temp_dir().join(format!(
            "tako-proposals-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(root.join("project")).unwrap();
        let out = Command::new("sh").args(["-c","git init -qb main && git config user.name fixture && git config user.email fixture@example.test && echo base > README.md && git add README.md && git commit -qm base"])
            .current_dir(root.join("project")).output().unwrap();
        assert!(out.status.success());
        let mut command = Command::new(env!("CARGO_BIN_EXE_tako-bridge-stdio"));
        command
            .arg(root.join("data"))
            .env_remove("OPENAI_API_KEY")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(key) = key {
            command.env("OPENAI_API_KEY", key);
        }
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, replies) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if let Ok(line) = line {
                    if tx.send(serde_json::from_str(&line).unwrap()).is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            replies,
            root,
            reader: Some(reader),
        }
    }
    fn call(&mut self, cmd: &str, args: Value) -> Value {
        writeln!(self.stdin, "{}", json!({"id":1,"cmd":cmd,"args":args})).unwrap();
        self.stdin.flush().unwrap();
        let reply = self.replies.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!reply.to_string().contains(KEY));
        reply
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap();
        }
        fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn missing_credentials_and_unsupported_providers_fail_closed_without_t() {
    let mut desktop = Desktop::new(None);
    assert_eq!(
        desktop.call("integration_status", Value::Null)["ok"]["availability"],
        "disabled"
    );
    let request = json!({"provider":"open_ai","model":"fixture-model","prompt":"Suggest a refactor","timeout_s":1,"max_output_tokens":16});
    assert_eq!(
        desktop.call("proposal_start", request.clone())["err"]["code"],
        "missing_credentials"
    );
    let mut unsupported = request.clone();
    unsupported["provider"] = json!("anthropic");
    assert_eq!(
        desktop.call("proposal_start", unsupported)["err"]["code"],
        "provider_unavailable"
    );
    let mut injected = request;
    injected["api_key"] = json!(KEY);
    assert_eq!(
        desktop.call("proposal_start", injected)["err"]["code"],
        "invalid_input"
    );
    let path = desktop.root.join("project");
    assert_eq!(
        desktop.call("open_project", json!({"path":path}))["ok"]["source"],
        "local"
    );
    assert!(desktop.call("startup_report", Value::Null)["ok"]["errors"]
        .as_array()
        .unwrap()
        .is_empty());
}
#[test]
fn native_provider_credential_is_not_grantable_to_scripted_evidence() {
    let mut desktop = Desktop::new(Some(KEY));
    let path = desktop.root.join("project");
    let created=desktop.call("create_mission",json!({"project_path":path,"goal":"Check environment isolation","criteria":["no credential"],"checks":["true"]}));
    let id = created["ok"]["mission"]["id"].as_str().unwrap().to_owned();
    let plan=desktop.call("define_plan",json!({"mission_id":id,"script":"test -z \"${OPENAI_API_KEY+x}\" && printf 'credential absent\\n'","extra_permissions":["read_env:OPENAI_API_KEY"]}));
    let p = &plan["ok"]["plan"];
    let approved=desktop.call("approve_plan",json!({"mission_id":id,"version":p["version"],"plan_hash":p["hash"],"granted":["fs_write_workspace","exec_command:sh","read_env:OPENAI_API_KEY"]}));
    assert_eq!(approved["ok"]["mission"]["state"], "approved");
    let started = desktop.call("start_execution", json!({"mission_id":id,"timeout_s":2}));
    assert!(started.get("err").is_none(), "{started}");
    let deadline = Instant::now() + Duration::from_secs(5);
    let done = loop {
        let v = desktop.call("get_mission", json!({"mission_id":id}));
        if v["ok"]["job"]["running"] == false {
            break v;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(done["ok"]["execution"]["status"], "succeeded", "{done}");
    assert!(done["ok"]["execution"]["stdout_tail"]
        .as_str()
        .unwrap()
        .contains("credential absent"));
    for file in ["stdout.log", "stderr.log"] {
        let log = desktop
            .root
            .join("data/missions")
            .join(&id)
            .join("exec-1")
            .join(file);
        if let Ok(bytes) = fs::read(log) {
            assert!(!String::from_utf8_lossy(&bytes).contains(KEY));
        }
    }
}
