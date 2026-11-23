//! Negative boundary and offline integration tests for `python_sota` (M029F.1).

use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use tako_bridge::Bridge;

struct TempDir(PathBuf);
impl TempDir {
    fn new(prefix: &str) -> Self {
        let mut p = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("tako_py_sota_{prefix}_{pid}_{nanos}"));
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_exec(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, body).unwrap();
    let mut perms = fs::metadata(&p).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&p, perms).unwrap();
    p
}

fn create_valid_workspace(root: &Path) -> PathBuf {
    let ws = root.join("ws");
    fs::create_dir_all(ws.join("runs")).unwrap();
    fs::write(
        ws.join("tako_workspace.json"),
        json!({
            "schema_version": "tako-workspace/v1",
            "research_id": "res-test-001",
            "topic": "Edge AI",
            "objective": "Survey latency",
            "tako_version": "0.1.0a3",
            "quality_status": {"verdict": "UNKNOWN", "report_qualification": "UNQUALIFIED"},
            "stages": {}
        })
        .to_string(),
    )
    .unwrap();
    ws
}

#[test]
fn negative_missing_and_incompatible_runtime() {
    let tmp = TempDir::new("runtime");
    let bridge = Bridge::open(tmp.path().join("data").tap_create()).unwrap();

    // Missing executable
    let err = bridge
        .dispatch(
            "sota_resolve_runtime",
            json!({"runtime_executable": "/nonexistent/path/to/tako"}),
        )
        .unwrap_err();
    assert_eq!(err.code, "missing_runtime");

    // Relative executable rejected
    let err = bridge
        .dispatch(
            "sota_resolve_runtime",
            json!({"runtime_executable": "tako"}),
        )
        .unwrap_err();
    assert_eq!(err.code, "missing_runtime");

    // Incompatible version (0.1.0a2)
    let old_cli = write_exec(
        tmp.path(),
        "tako_old",
        "#!/usr/bin/env bash\necho 'tako 0.1.0a2 (experimental researcher preview)'\n",
    );
    let err = bridge
        .dispatch(
            "sota_resolve_runtime",
            json!({"runtime_executable": old_cli.to_string_lossy()}),
        )
        .unwrap_err();
    assert_eq!(err.code, "incompatible_runtime");
}

#[test]
fn negative_invalid_args_and_workspace_security() {
    let tmp = TempDir::new("sec");
    let bridge = Bridge::open(tmp.path().join("data").tap_create()).unwrap();
    let ws = create_valid_workspace(tmp.path());
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.json"), "{}").unwrap();
    symlink(&outside, ws.join("runs_link")).unwrap();

    // Unknown field rejected
    let err = bridge
        .dispatch(
            "sota_ws_status",
            json!({"workspace_path": ws.to_string_lossy(), "extra_bad": 1}),
        )
        .unwrap_err();
    assert_eq!(err.code, "invalid_input");

    // Nonexistent workspace
    let err = bridge
        .dispatch(
            "sota_ws_status",
            json!({"workspace_path": tmp.path().join("missing_ws").to_string_lossy()}),
        )
        .unwrap_err();
    assert_eq!(err.code, "not_found");

    // Path traversal in workspace_path
    let err = bridge
        .dispatch(
            "sota_ws_status",
            json!({"workspace_path": format!("{}/../ws", ws.display())}),
        )
        .unwrap_err();
    assert_eq!(err.code, "path_traversal");

    // Path traversal in export out_dir
    let err = bridge
        .dispatch(
            "sota_export",
            json!({
                "workspace_path": ws.to_string_lossy(),
                "out_dir": "../outside_export"
            }),
        )
        .unwrap_err();
    assert_eq!(err.code, "path_traversal");

    // Symlink escape in workspace_path
    let ws_sym = tmp.path().join("ws_sym");
    symlink(&ws, &ws_sym).unwrap();
    let err = bridge
        .dispatch(
            "sota_ws_status",
            json!({"workspace_path": ws_sym.to_string_lossy()}),
        )
        .unwrap_err();
    assert_eq!(err.code, "symlink_escape");

    // Symlink escape in subpath
    let err = bridge
        .dispatch(
            "sota_export",
            json!({
                "workspace_path": ws.to_string_lossy(),
                "out_dir": "runs_link/sub"
            }),
        )
        .unwrap_err();
    assert_eq!(err.code, "symlink_escape");
}

#[test]
fn negative_unauthorized_provider_and_network_discovery() {
    let tmp = TempDir::new("auth");
    let bridge = Bridge::open(tmp.path().join("data").tap_create()).unwrap();
    let ws = create_valid_workspace(tmp.path());

    // Disallowed network discovery (missing fixture_path)
    let err = bridge
        .dispatch(
            "sota_run_discovery",
            json!({"workspace_path": ws.to_string_lossy()}),
        )
        .unwrap_err();
    assert_eq!(err.code, "unauthorized");

    // Unauthorized live AI provider
    for prov in ["claude-cli", "codex-cli", "agy-cli", "copilot-cli", "openai-responses"] {
        let err = bridge
            .dispatch(
                "sota_run_ai_native",
                json!({"workspace_path": ws.to_string_lossy(), "provider": prov}),
            )
            .unwrap_err();
        assert_eq!(err.code, "unauthorized", "provider {prov} must be rejected");
    }

    // Unauthorized flags even with fixture provider
    for flag in ["allow_cli_session", "allow_paid_api", "allow_unisolated_cli", "verify_bib"] {
        let err = bridge
            .dispatch(
                "sota_run_ai_native",
                json!({
                    "workspace_path": ws.to_string_lossy(),
                    "provider": "fixture",
                    flag: true
                }),
            )
            .unwrap_err();
        assert_eq!(err.code, "unauthorized", "flag {flag} must be rejected");
    }
}

#[test]
fn negative_subprocess_failures_timeout_output_limit_and_insufficiency() {
    let tmp = TempDir::new("proc");
    let bridge = Bridge::open(tmp.path().join("data").tap_create()).unwrap();
    let ws = create_valid_workspace(tmp.path());
    let marker = ws.join("preserve_me.txt");
    fs::write(&marker, "keep").unwrap();

    let fx = tmp.path().join("fx.json");
    fs::write(&fx, "{\"results\":[]}").unwrap();

    // 1. Subprocess failure (exit 2)
    let fail_cli = write_exec(
        tmp.path(),
        "tako_fail",
        "#!/usr/bin/env bash\nif [ \"$1\" = \"--version\" ]; then echo 'tako 0.1.0a3'; exit 0; fi\necho 'boom' >&2\nexit 2\n",
    );
    let err = bridge
        .dispatch(
            "sota_ws_status",
            json!({
                "workspace_path": ws.to_string_lossy(),
                "runtime_executable": fail_cli.to_string_lossy()
            }),
        )
        .unwrap_err();
    assert_eq!(err.code, "subprocess_failed");

    // 2. Interrupted execution (exit 130)
    let int_cli = write_exec(
        tmp.path(),
        "tako_int",
        "#!/usr/bin/env bash\nif [ \"$1\" = \"--version\" ]; then echo 'tako 0.1.0a3'; exit 0; fi\necho 'interrupted' >&2\nexit 130\n",
    );
    let err = bridge
        .dispatch(
            "sota_ws_status",
            json!({
                "workspace_path": ws.to_string_lossy(),
                "runtime_executable": int_cli.to_string_lossy()
            }),
        )
        .unwrap_err();
    assert_eq!(err.code, "interrupted");

    // 3. Malformed JSON stdout
    let bad_json_cli = write_exec(
        tmp.path(),
        "tako_bad_json",
        "#!/usr/bin/env bash\nif [ \"$1\" = \"--version\" ]; then echo 'tako 0.1.0a3'; exit 0; fi\necho '{not valid json'\n",
    );
    let err = bridge
        .dispatch(
            "sota_ws_status",
            json!({
                "workspace_path": ws.to_string_lossy(),
                "runtime_executable": bad_json_cli.to_string_lossy()
            }),
        )
        .unwrap_err();
    assert_eq!(err.code, "invalid_response");

    // 4. Timeout
    let slow_cli = write_exec(
        tmp.path(),
        "tako_slow",
        "#!/usr/bin/env bash\nif [ \"$1\" = \"--version\" ]; then echo 'tako 0.1.0a3'; exit 0; fi\nsleep 5\n",
    );
    let err = bridge
        .dispatch(
            "sota_ws_status",
            json!({
                "workspace_path": ws.to_string_lossy(),
                "runtime_executable": slow_cli.to_string_lossy(),
                "timeout_ms": 120
            }),
        )
        .unwrap_err();
    assert_eq!(err.code, "timeout");

    // 5. Output limit
    let flood_cli = write_exec(
        tmp.path(),
        "tako_flood",
        "#!/usr/bin/env bash\nif [ \"$1\" = \"--version\" ]; then echo 'tako 0.1.0a3'; exit 0; fi\nhead -c 65536 /dev/zero | tr '\\0' 'a'\n",
    );
    let err = bridge
        .dispatch(
            "sota_run_discovery",
            json!({
                "workspace_path": ws.to_string_lossy(),
                "fixture_path": fx.to_string_lossy(),
                "runtime_executable": flood_cli.to_string_lossy(),
                "max_output_bytes": 1024
            }),
        )
        .unwrap_err();
    assert_eq!(err.code, "output_limit");

    // 6. Scientific insufficiency propagation
    let insuf_cli = write_exec(
        tmp.path(),
        "tako_insuf",
        "#!/usr/bin/env bash\nif [ \"$1\" = \"--version\" ]; then echo 'tako 0.1.0a3'; exit 0; fi\necho '{\"status\":\"SOTA_ITERATIVE_COMPLETE\",\"termination\":\"INSUFFICIENT_COVERAGE\",\"accepted\":0,\"counters\":{\"accepted_papers\":0}}'\n",
    );
    let ok = bridge
        .dispatch(
            "sota_run_discovery",
            json!({
                "workspace_path": ws.to_string_lossy(),
                "fixture_path": fx.to_string_lossy(),
                "iterative": true,
                "runtime_executable": insuf_cli.to_string_lossy()
            }),
        )
        .unwrap();
    assert_eq!(ok["execution_status"], "insufficient_evidence");
    assert_eq!(ok["scientific_status"]["evidence_sufficient"], Value::Bool(false));

    // Existing workspace file must be untouched across all failures.
    assert_eq!(fs::read_to_string(&marker).unwrap(), "keep");
}

trait TapCreate {
    fn tap_create(self) -> Self;
}
impl TapCreate for PathBuf {
    fn tap_create(self) -> Self {
        fs::create_dir_all(&self).unwrap();
        self
    }
}
