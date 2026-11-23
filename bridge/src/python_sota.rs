//! Typed, bounded offline IPC bridge to the authoritative TAKO Python scientific core.
//!
//! All SOTA scientific logic, workspace state, and evidence classification remain in Python.
//! This module enforces deterministic runtime resolution, workspace path confinement,
//! environment sanitization, bounded subprocess execution, and structured error propagation.

use crate::BridgeError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub const CONTRACT_VERSION: &str = "tako-sota-ipc/v1";
const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const MAX_TIMEOUT_MS: u64 = 300_000;
const VERSION_CHECK_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_EVENTS_READ: usize = 5_000;

pub fn handles(cmd: &str) -> bool {
    matches!(
        cmd,
        "sota_resolve_runtime"
            | "sota_ws_init"
            | "sota_ws_status"
            | "sota_run_discovery"
            | "sota_run_ai_native"
            | "sota_read_events"
            | "sota_export"
    )
}

pub fn dispatch(cmd: &str, args: Value) -> Result<Value, BridgeError> {
    match cmd {
        "sota_resolve_runtime" => resolve_runtime_cmd(parse(args)?),
        "sota_ws_init" => ws_init_cmd(parse(args)?),
        "sota_ws_status" => ws_status_cmd(parse(args)?),
        "sota_run_discovery" => run_discovery_cmd(parse(args)?),
        "sota_run_ai_native" => run_ai_native_cmd(parse(args)?),
        "sota_read_events" => read_events_cmd(parse(args)?),
        "sota_export" => export_cmd(parse(args)?),
        _ => Err(BridgeError::new(
            "unknown_command",
            format!("unknown python SOTA command: {cmd}"),
        )),
    }
}

fn parse<T: for<'a> Deserialize<'a>>(v: Value) -> Result<T, BridgeError> {
    let normalized = if v.is_null() { json!({}) } else { v };
    serde_json::from_value(normalized)
        .map_err(|e| BridgeError::invalid(format!("invalid SOTA IPC arguments: {e}")))
}

fn to_value<T: Serialize>(v: T) -> Result<Value, BridgeError> {
    serde_json::to_value(v).map_err(|e| BridgeError::new("internal", e.to_string()))
}

// ---- Request & Response DTOs ---------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveRuntimeArgs {
    #[serde(default)]
    pub runtime_executable: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WsInitArgs {
    pub workspace_path: String,
    pub topic: String,
    pub objective: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub runtime_executable: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WsStatusArgs {
    pub workspace_path: String,
    #[serde(default)]
    pub runtime_executable: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunDiscoveryArgs {
    pub workspace_path: String,
    #[serde(default)]
    pub fixture_path: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub iterative: bool,
    #[serde(default)]
    pub max_papers: Option<u32>,
    #[serde(default)]
    pub max_rounds: Option<u32>,
    #[serde(default)]
    pub max_requests: Option<u32>,
    #[serde(default)]
    pub out_dir: Option<String>,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub runtime_executable: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_output_bytes: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunAiNativeArgs {
    pub workspace_path: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub fixture_path: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub max_papers: Option<u32>,
    #[serde(default)]
    pub with_reviewer: bool,
    #[serde(default)]
    pub allow_cli_session: bool,
    #[serde(default)]
    pub allow_paid_api: bool,
    #[serde(default)]
    pub allow_unisolated_cli: bool,
    #[serde(default)]
    pub verify_bib: bool,
    #[serde(default)]
    pub out_dir: Option<String>,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub runtime_executable: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_output_bytes: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadEventsArgs {
    pub workspace_path: String,
    #[serde(default)]
    pub run_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportArgs {
    pub workspace_path: String,
    #[serde(default)]
    pub from_dir: Option<String>,
    pub out_dir: String,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub runtime_executable: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_output_bytes: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInfo {
    pub executable: String,
    pub version: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SotaIpcResponse {
    pub contract_version: &'static str,
    pub operation: String,
    pub workspace_path: Option<String>,
    pub runtime: Option<RuntimeInfo>,
    pub exit_code: Option<i32>,
    pub execution_status: String,
    pub scientific_status: Option<Value>,
    pub data: Value,
}

// ---- Command Handlers ----------------------------------------------------------------------

fn resolve_runtime_cmd(args: ResolveRuntimeArgs) -> Result<Value, BridgeError> {
    let timeout = clamp_timeout(args.timeout_ms, VERSION_CHECK_TIMEOUT_MS);
    let runtime = resolve_and_verify_runtime(args.runtime_executable.as_deref(), timeout)?;
    to_value(SotaIpcResponse {
        contract_version: CONTRACT_VERSION,
        operation: "sota_resolve_runtime".into(),
        workspace_path: None,
        runtime: Some(runtime.clone()),
        exit_code: Some(0),
        execution_status: "succeeded".into(),
        scientific_status: None,
        data: json!({
            "executable": runtime.executable,
            "version": runtime.version,
            "source": runtime.source,
            "offline_only": true,
        }),
    })
}

fn ws_init_cmd(args: WsInitArgs) -> Result<Value, BridgeError> {
    if args.topic.trim().is_empty() || args.objective.trim().is_empty() {
        return Err(BridgeError::invalid(
            "topic and objective must be non-empty strings.",
        ));
    }
    let ws = validate_workspace_root(&args.workspace_path, false)?;
    let timeout = clamp_timeout(args.timeout_ms, DEFAULT_TIMEOUT_MS);
    let runtime = resolve_and_verify_runtime(args.runtime_executable.as_deref(), timeout)?;

    let mut cli_args = vec![
        "init".to_string(),
        "--workspace".into(),
        ws.to_string_lossy().into_owned(),
        "--topic".into(),
        args.topic,
        "--objective".into(),
        args.objective,
    ];
    if let Some(scope) = args.scope.filter(|s| !s.trim().is_empty()) {
        cli_args.push("--scope".into());
        cli_args.push(scope);
    }
    if args.force {
        cli_args.push("--force".into());
    }

    let init_cwd = existing_ancestor(&ws)?;
    let out = run_tako_cli(
        &runtime.executable,
        &cli_args,
        &init_cwd,
        &init_cwd,
        timeout,
        DEFAULT_MAX_OUTPUT_BYTES,
    )?;

    // Read initialized status JSON so caller receives authoritative workspace manifest.
    let status_args = vec![
        "status".to_string(),
        "--workspace".into(),
        ws.to_string_lossy().into_owned(),
        "--json".into(),
    ];
    let st_out = run_tako_cli(
        &runtime.executable,
        &status_args,
        &ws,
        &ws,
        timeout,
        DEFAULT_MAX_OUTPUT_BYTES,
    )?;
    let manifest = parse_json_stdout(&st_out.stdout)?;
    let sci = extract_workspace_scientific_status(&manifest);

    to_value(SotaIpcResponse {
        contract_version: CONTRACT_VERSION,
        operation: "sota_ws_init".into(),
        workspace_path: Some(ws.to_string_lossy().into_owned()),
        runtime: Some(runtime),
        exit_code: Some(out.exit_code),
        execution_status: "succeeded".into(),
        scientific_status: Some(sci),
        data: manifest,
    })
}

fn ws_status_cmd(args: WsStatusArgs) -> Result<Value, BridgeError> {
    let ws = validate_workspace_root(&args.workspace_path, true)?;
    let timeout = clamp_timeout(args.timeout_ms, DEFAULT_TIMEOUT_MS);
    let runtime = resolve_and_verify_runtime(args.runtime_executable.as_deref(), timeout)?;

    let cli_args = vec![
        "status".to_string(),
        "--workspace".into(),
        ws.to_string_lossy().into_owned(),
        "--json".into(),
    ];
    let init_cwd = existing_ancestor(&ws)?;
    let out = run_tako_cli(
        &runtime.executable,
        &cli_args,
        &init_cwd,
        &init_cwd,
        timeout,
        DEFAULT_MAX_OUTPUT_BYTES,
    )?;
    let manifest = parse_json_stdout(&out.stdout)?;
    let sci = extract_workspace_scientific_status(&manifest);
    let exec_status = classify_workspace_status(&manifest);

    to_value(SotaIpcResponse {
        contract_version: CONTRACT_VERSION,
        operation: "sota_ws_status".into(),
        workspace_path: Some(ws.to_string_lossy().into_owned()),
        runtime: Some(runtime),
        exit_code: Some(out.exit_code),
        execution_status: exec_status,
        scientific_status: Some(sci),
        data: manifest,
    })
}

fn run_discovery_cmd(args: RunDiscoveryArgs) -> Result<Value, BridgeError> {
    let ws = validate_workspace_root(&args.workspace_path, true)?;

    // M029F.1 offline boundary: require explicit fixture_path (never live network discovery).
    let Some(raw_fixture) = args.fixture_path.as_deref().filter(|s| !s.trim().is_empty()) else {
        return Err(BridgeError::new(
            "unauthorized",
            "Live network discovery is disabled in M029F.1 offline mode; provide fixture_path.",
        ));
    };
    let fixture = validate_fixture_path(raw_fixture)?;

    let timeout = clamp_timeout(args.timeout_ms, DEFAULT_TIMEOUT_MS);
    let max_bytes = args.max_output_bytes.unwrap_or(DEFAULT_MAX_OUTPUT_BYTES);
    let runtime = resolve_and_verify_runtime(args.runtime_executable.as_deref(), timeout)?;

    let mut cli_args = vec![
        "sota".to_string(),
        "--workspace".into(),
        ws.to_string_lossy().into_owned(),
        "--fixture".into(),
        fixture.to_string_lossy().into_owned(),
        "--json".into(),
    ];
    if let Some(topic) = args.topic.filter(|s| !s.trim().is_empty()) {
        cli_args.push("--topic".into());
        cli_args.push(topic);
    }
    if let Some(obj) = args.objective.filter(|s| !s.trim().is_empty()) {
        cli_args.push("--objective".into());
        cli_args.push(obj);
    }
    if args.iterative {
        cli_args.push("--iterative".into());
    }
    if let Some(n) = args.max_papers {
        cli_args.push("--max-papers".into());
        cli_args.push(n.to_string());
    }
    if let Some(n) = args.max_rounds {
        cli_args.push("--max-rounds".into());
        cli_args.push(n.to_string());
    }
    if let Some(n) = args.max_requests {
        cli_args.push("--max-requests".into());
        cli_args.push(n.to_string());
    }
    if let Some(rel_out) = args.out_dir.as_deref().filter(|s| !s.trim().is_empty()) {
        let resolved_out = resolve_workspace_subpath(&ws, rel_out, false)?;
        cli_args.push("--out-dir".into());
        cli_args.push(resolved_out.to_string_lossy().into_owned());
    }
    if args.force {
        cli_args.push("--force".into());
    }

    let out = run_tako_cli(&runtime.executable, &cli_args, &ws, &ws, timeout, max_bytes)?;
    let payload = parse_json_stdout(&out.stdout)?;
    let (exec_status, sci) = classify_discovery_result(&payload);

    to_value(SotaIpcResponse {
        contract_version: CONTRACT_VERSION,
        operation: "sota_run_discovery".into(),
        workspace_path: Some(ws.to_string_lossy().into_owned()),
        runtime: Some(runtime),
        exit_code: Some(out.exit_code),
        execution_status: exec_status,
        scientific_status: Some(sci),
        data: payload,
    })
}

fn run_ai_native_cmd(args: RunAiNativeArgs) -> Result<Value, BridgeError> {
    let ws = validate_workspace_root(&args.workspace_path, true)?;

    // M029F.1 offline & authorization boundary: fixture provider only, no live/paid/unisolated flags.
    let provider = args.provider.as_deref().unwrap_or("fixture");
    if provider != "fixture" {
        return Err(BridgeError::new(
            "unauthorized",
            format!("Provider '{provider}' is disabled in M029F.1 offline mode; only 'fixture' is permitted."),
        ));
    }
    if args.allow_cli_session || args.allow_paid_api || args.allow_unisolated_cli || args.verify_bib {
        return Err(BridgeError::new(
            "unauthorized",
            "Live provider sessions, paid API calls, unisolated CLIs, and network bibliographic verification are disabled in M029F.1.",
        ));
    }

    let timeout = clamp_timeout(args.timeout_ms, DEFAULT_TIMEOUT_MS);
    let max_bytes = args.max_output_bytes.unwrap_or(DEFAULT_MAX_OUTPUT_BYTES);
    let runtime = resolve_and_verify_runtime(args.runtime_executable.as_deref(), timeout)?;

    let mut cli_args = vec![
        "sota-ai".to_string(),
        "--workspace".into(),
        ws.to_string_lossy().into_owned(),
        "--provider".into(),
        "fixture".into(),
        "--json".into(),
    ];
    if let Some(raw_fx) = args.fixture_path.as_deref().filter(|s| !s.trim().is_empty()) {
        let fx = validate_fixture_path(raw_fx)?;
        cli_args.push("--fixture".into());
        cli_args.push(fx.to_string_lossy().into_owned());
    }
    if let Some(topic) = args.topic.filter(|s| !s.trim().is_empty()) {
        cli_args.push("--topic".into());
        cli_args.push(topic);
    }
    if let Some(obj) = args.objective.filter(|s| !s.trim().is_empty()) {
        cli_args.push("--objective".into());
        cli_args.push(obj);
    }
    if let Some(n) = args.max_papers {
        cli_args.push("--max-papers".into());
        cli_args.push(n.to_string());
    }
    if args.with_reviewer {
        cli_args.push("--with-reviewer".into());
    }
    if let Some(rel_out) = args.out_dir.as_deref().filter(|s| !s.trim().is_empty()) {
        let resolved_out = resolve_workspace_subpath(&ws, rel_out, false)?;
        cli_args.push("--out-dir".into());
        cli_args.push(resolved_out.to_string_lossy().into_owned());
    }
    if args.force {
        cli_args.push("--force".into());
    }

    let out = run_tako_cli(&runtime.executable, &cli_args, &ws, &ws, timeout, max_bytes)?;
    let payload = parse_json_stdout(&out.stdout)?;
    let sci = json!({
        "provider": payload.get("provider").cloned().unwrap_or(Value::Null),
        "reviewer": payload.get("reviewer").cloned().unwrap_or(Value::Null),
        "status": payload.get("status").cloned().unwrap_or(Value::Null),
        "offline_fixture": true,
    });

    to_value(SotaIpcResponse {
        contract_version: CONTRACT_VERSION,
        operation: "sota_run_ai_native".into(),
        workspace_path: Some(ws.to_string_lossy().into_owned()),
        runtime: Some(runtime),
        exit_code: Some(out.exit_code),
        execution_status: "succeeded".into(),
        scientific_status: Some(sci),
        data: payload,
    })
}

fn read_events_cmd(args: ReadEventsArgs) -> Result<Value, BridgeError> {
    let ws = validate_workspace_root(&args.workspace_path, true)?;
    let runs_dir = resolve_workspace_subpath(&ws, "runs", false)?;

    let mut available_runs: Vec<String> = Vec::new();
    if runs_dir.is_dir() {
        for entry in fs::read_dir(&runs_dir).map_err(|e| BridgeError::new("internal", e.to_string()))? {
            let entry = entry.map_err(|e| BridgeError::new("internal", e.to_string()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(rid) = name.strip_suffix("_events.jsonl") {
                available_runs.push(rid.to_string());
            }
        }
        available_runs.sort();
    }

    let selected_run_id = match args.run_id.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(rid) => {
            if rid.contains('/') || rid.contains('\\') || rid.contains("..") {
                return Err(BridgeError::invalid("run_id must not contain path separators or '..'."));
            }
            Some(rid.to_string())
        }
        None => available_runs.last().cloned(),
    };

    let mut events = Vec::new();
    if let Some(rid) = &selected_run_id {
        let ev_rel = format!("runs/{rid}_events.jsonl");
        let ev_path = resolve_workspace_subpath(&ws, &ev_rel, true)?;
        let meta = fs::symlink_metadata(&ev_path).map_err(|e| BridgeError::new("not_found", e.to_string()))?;
        if meta.file_type().is_symlink() {
            return Err(BridgeError::new("symlink_escape", "Refusing to read symlinked events file."));
        }
        let text = fs::read_to_string(&ev_path).map_err(|e| BridgeError::new("internal", e.to_string()))?;
        for (idx, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            if events.len() >= MAX_EVENTS_READ {
                return Err(BridgeError::new(
                    "output_limit",
                    format!("Workflow event log exceeds {MAX_EVENTS_READ} events."),
                ));
            }
            let ev: Value = serde_json::from_str(line).map_err(|e| {
                BridgeError::new(
                    "invalid_response",
                    format!("Malformed JSONL event at line {}: {e}", idx + 1),
                )
            })?;
            events.push(ev);
        }
    }

    to_value(SotaIpcResponse {
        contract_version: CONTRACT_VERSION,
        operation: "sota_read_events".into(),
        workspace_path: Some(ws.to_string_lossy().into_owned()),
        runtime: None,
        exit_code: Some(0),
        execution_status: "succeeded".into(),
        scientific_status: None,
        data: json!({
            "run_id": selected_run_id,
            "available_runs": available_runs,
            "event_count": events.len(),
            "events": events,
        }),
    })
}

fn export_cmd(args: ExportArgs) -> Result<Value, BridgeError> {
    let ws = validate_workspace_root(&args.workspace_path, true)?;
    let out_dir = resolve_workspace_subpath(&ws, &args.out_dir, false)?;

    let format = args.format.as_deref().unwrap_or("all");
    if !matches!(format, "all" | "markdown" | "csv" | "json" | "bibtex" | "ris") {
        return Err(BridgeError::invalid(format!("Unsupported export format: {format}")));
    }

    let timeout = clamp_timeout(args.timeout_ms, DEFAULT_TIMEOUT_MS);
    let max_bytes = args.max_output_bytes.unwrap_or(DEFAULT_MAX_OUTPUT_BYTES);
    let runtime = resolve_and_verify_runtime(args.runtime_executable.as_deref(), timeout)?;

    let mut cli_args = vec![
        "export".to_string(),
        "--workspace".into(),
        ws.to_string_lossy().into_owned(),
        "--out-dir".into(),
        out_dir.to_string_lossy().into_owned(),
        "--format".into(),
        format.into(),
    ];
    if let Some(rel_from) = args.from_dir.as_deref().filter(|s| !s.trim().is_empty()) {
        let from_dir = resolve_workspace_subpath(&ws, rel_from, true)?;
        cli_args.push("--from-dir".into());
        cli_args.push(from_dir.to_string_lossy().into_owned());
    }
    if args.force {
        cli_args.push("--force".into());
    }

    let out = run_tako_cli(&runtime.executable, &cli_args, &ws, &ws, timeout, max_bytes)?;

    let inv_path = out_dir.join("artifact_inventory.json");
    let inventory: Value = if inv_path.is_file() {
        let txt = fs::read_to_string(&inv_path)
            .map_err(|e| BridgeError::new("invalid_response", e.to_string()))?;
        serde_json::from_str(&txt)
            .map_err(|e| BridgeError::new("invalid_response", format!("Malformed artifact_inventory.json: {e}")))?
    } else {
        json!({ "exported_from": Value::Null, "format": format, "artifacts": [] })
    };

    to_value(SotaIpcResponse {
        contract_version: CONTRACT_VERSION,
        operation: "sota_export".into(),
        workspace_path: Some(ws.to_string_lossy().into_owned()),
        runtime: Some(runtime),
        exit_code: Some(out.exit_code),
        execution_status: "succeeded".into(),
        scientific_status: None,
        data: json!({
            "out_dir": out_dir.to_string_lossy(),
            "format": format,
            "inventory": inventory,
            "stdout": out.stdout.trim(),
        }),
    })
}

// ---- Runtime Resolution --------------------------------------------------------------------

fn resolve_and_verify_runtime(
    explicit_arg: Option<&str>,
    timeout: Duration,
) -> Result<RuntimeInfo, BridgeError> {
    let (exe_str, source) = if let Some(raw) = explicit_arg.filter(|s| !s.trim().is_empty()) {
        (raw.to_string(), "explicit_arg".to_string())
    } else if let Ok(env_val) = std::env::var("TAKO_PYTHON_CLI_BIN") {
        if env_val.trim().is_empty() {
            return Err(BridgeError::new(
                "missing_runtime",
                "TAKO_PYTHON_CLI_BIN is set but empty.",
            ));
        }
        (env_val, "env:TAKO_PYTHON_CLI_BIN".to_string())
    } else {
        return Err(BridgeError::new(
            "missing_runtime",
            "No explicit TAKO Python CLI runtime configured. Set TAKO_PYTHON_CLI_BIN or pass runtime_executable.",
        ));
    };

    let exe_path = PathBuf::from(&exe_str);
    if !exe_path.is_absolute()
        || exe_path
            .components()
            .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(BridgeError::new(
            "missing_runtime",
            format!("Runtime executable must be an absolute path without '..': {exe_str}"),
        ));
    }
    if !exe_path.is_file() {
        return Err(BridgeError::new(
            "missing_runtime",
            format!("TAKO Python CLI executable not found: {exe_str}"),
        ));
    }

    let check_cwd = std::env::temp_dir();
    let out = run_tako_cli(
        &exe_str,
        &["--version".to_string()],
        &check_cwd,
        &check_cwd,
        timeout,
        8192,
    )
    .map_err(|e| {
        if e.code == "timeout" || e.code == "output_limit" {
            e
        } else {
            BridgeError::new(
                "incompatible_runtime",
                format!("Failed to query TAKO CLI version from {exe_str}: {}", e.message),
            )
        }
    })?;

    let version_line = out.stdout.trim();
    let version = parse_and_check_cli_version(version_line).ok_or_else(|| {
        BridgeError::new(
            "incompatible_runtime",
            format!("Incompatible TAKO CLI version '{version_line}' at {exe_str}; requires >= 0.1.0a3."),
        )
    })?;

    Ok(RuntimeInfo {
        executable: exe_str,
        version,
        source,
    })
}

fn parse_and_check_cli_version(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    let prog = parts.next()?;
    let ver = parts.next()?;
    if prog != "tako" {
        return None;
    }
    // Accept 0.1.0a3+ or 0.1.0rc* or 0.1.0 (stable); reject 0.1.0a1 / 0.1.0a2 / 0.0.*
    if ver == "0.1.0a3" || ver.starts_with("0.1.0a3+") || ver.starts_with("0.1.0a3.") {
        return Some(ver.to_string());
    }
    if let Some(rest) = ver.strip_prefix("0.1.0a") {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<u32>() {
            if n >= 3 {
                return Some(ver.to_string());
            }
        }
        return None;
    }
    if ver == "0.1.0" || ver.starts_with("0.1.0rc") || ver.starts_with("0.1.0b") {
        return Some(ver.to_string());
    }
    None
}

// ---- Path & Workspace Validation -----------------------------------------------------------

fn validate_workspace_root(raw: &str, must_exist: bool) -> Result<PathBuf, BridgeError> {
    if raw.trim().is_empty() {
        return Err(BridgeError::invalid("workspace_path must be non-empty."));
    }
    let p = PathBuf::from(raw);
    if !p.is_absolute() {
        return Err(BridgeError::invalid("workspace_path must be an absolute path."));
    }
    if p.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(BridgeError::new(
            "path_traversal",
            "workspace_path must not contain '..' components.",
        ));
    }

    // Reject if the workspace root itself or any existing component is a symlink.
    if let Ok(meta) = fs::symlink_metadata(&p) {
        if meta.file_type().is_symlink() {
            return Err(BridgeError::new(
                "symlink_escape",
                "workspace_path must not be a symlink.",
            ));
        }
    }

    if must_exist {
        if !p.is_dir() {
            return Err(BridgeError::new(
                "not_found",
                format!("Workspace directory does not exist: {}", p.display()),
            ));
        }
        let manifest = p.join("tako_workspace.json");
        let meta = fs::symlink_metadata(&manifest).map_err(|_| {
            BridgeError::new(
                "not_found",
                format!("Workspace manifest tako_workspace.json not found in {}", p.display()),
            )
        })?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(BridgeError::new(
                "symlink_escape",
                "tako_workspace.json must be a regular non-symlink file.",
            ));
        }
        return fs::canonicalize(&p).map_err(|e| BridgeError::new("internal", e.to_string()));
    }

    // For initialization: ensure deepest existing ancestor is a real directory.
    let cur = existing_ancestor(&p)?;
    if let Ok(meta) = fs::symlink_metadata(&cur) {
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(BridgeError::new(
                "symlink_escape",
                "Workspace ancestor must be a real directory, not a symlink.",
            ));
        }
    }
    Ok(p)
}

fn existing_ancestor(p: &Path) -> Result<PathBuf, BridgeError> {
    let mut cur = p;
    while !cur.exists() {
        cur = cur.parent().ok_or_else(|| {
            BridgeError::invalid("workspace_path has no existing ancestor directory.")
        })?;
    }
    Ok(cur.to_path_buf())
}

fn resolve_workspace_subpath(
    ws_root: &Path,
    rel: &str,
    must_exist: bool,
) -> Result<PathBuf, BridgeError> {
    if rel.trim().is_empty() {
        return Err(BridgeError::invalid("Subpath must be non-empty."));
    }
    let rel_p = Path::new(rel);
    if rel_p.is_absolute() {
        return Err(BridgeError::new(
            "path_traversal",
            "Subpath must be relative to the workspace root.",
        ));
    }
    let canonical_ws =
        fs::canonicalize(ws_root).map_err(|e| BridgeError::new("internal", e.to_string()))?;
    let mut cur = canonical_ws.clone();
    for comp in rel_p.components() {
        match comp {
            Component::Normal(seg) => {
                cur.push(seg);
                if let Ok(meta) = fs::symlink_metadata(&cur) {
                    if meta.file_type().is_symlink() {
                        return Err(BridgeError::new(
                            "symlink_escape",
                            format!("Refusing symlink inside workspace path: {}", cur.display()),
                        ));
                    }
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(BridgeError::new(
                    "path_traversal",
                    "Path traversal ('..') is not allowed in workspace subpaths.",
                ));
            }
        }
    }
    if must_exist {
        if !cur.exists() {
            return Err(BridgeError::new(
                "not_found",
                format!("Workspace path does not exist: {}", cur.display()),
            ));
        }
        let canon_target =
            fs::canonicalize(&cur).map_err(|e| BridgeError::new("internal", e.to_string()))?;
        if !canon_target.starts_with(&canonical_ws) {
            return Err(BridgeError::new(
                "symlink_escape",
                "Resolved path escapes the workspace root.",
            ));
        }
    }
    Ok(cur)
}

fn validate_fixture_path(raw: &str) -> Result<PathBuf, BridgeError> {
    let p = PathBuf::from(raw);
    if !p.is_absolute() || p.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(BridgeError::new(
            "path_traversal",
            "fixture_path must be an absolute path without '..'.",
        ));
    }
    let meta = fs::symlink_metadata(&p).map_err(|_| {
        BridgeError::new("not_found", format!("Fixture file not found: {}", p.display()))
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(BridgeError::new(
            "symlink_escape",
            "fixture_path must be a regular non-symlink file.",
        ));
    }
    Ok(p)
}

// ---- Bounded Subprocess Execution ----------------------------------------------------------

struct CliOutput {
    exit_code: i32,
    stdout: String,
}

fn clamp_timeout(req_ms: Option<u64>, default_ms: u64) -> Duration {
    let ms = req_ms.unwrap_or(default_ms).clamp(50, MAX_TIMEOUT_MS);
    Duration::from_millis(ms)
}

fn run_tako_cli(
    executable: &str,
    args: &[String],
    cwd: &Path,
    isolated_home: &Path,
    timeout: Duration,
    max_output_bytes: usize,
) -> Result<CliOutput, BridgeError> {
    let mut cmd = Command::new(executable);
    cmd.args(args)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("NO_COLOR", "1")
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("HOME", isolated_home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        BridgeError::new(
            "missing_runtime",
            format!("Failed to launch TAKO CLI '{executable}': {e}"),
        )
    })?;

    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| BridgeError::new("internal", "Failed to capture stdout pipe."))?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| BridgeError::new("internal", "Failed to capture stderr pipe."))?;

    let overflow = Arc::new(AtomicBool::new(false));
    let out_handle = spawn_bounded_reader(stdout_pipe, max_output_bytes, Arc::clone(&overflow));
    let err_handle = spawn_bounded_reader(stderr_pipe, max_output_bytes, Arc::clone(&overflow));

    let start = Instant::now();
    let status = loop {
        if overflow.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = out_handle.join();
            let _ = err_handle.join();
            return Err(BridgeError::new(
                "output_limit",
                format!("Subprocess output exceeded {max_output_bytes} bytes."),
            ));
        }
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = out_handle.join();
                    let _ = err_handle.join();
                    return Err(BridgeError::new(
                        "timeout",
                        format!("TAKO CLI timed out after {} ms.", timeout.as_millis()),
                    ));
                }
                thread::sleep(Duration::from_millis(15));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(BridgeError::new("internal", e.to_string()));
            }
        }
    };

    let stdout_bytes = out_handle.join().unwrap_or_default();
    let stderr_bytes = err_handle.join().unwrap_or_default();
    if overflow.load(Ordering::Acquire) {
        return Err(BridgeError::new(
            "output_limit",
            format!("Subprocess output exceeded {max_output_bytes} bytes."),
        ));
    }

    let stdout = String::from_utf8_lossy(&stdout_bytes).into_owned();
    let stderr = String::from_utf8_lossy(&stderr_bytes).into_owned();

    if !status.success() {
        let code_opt = status.code();
        let err_code = if code_opt == Some(130) || code_opt.is_none() {
            "interrupted"
        } else if stderr.contains("OFFLINE_MODE_BLOCKED") || stderr.contains("PROVIDER_BLOCKED") {
            "unauthorized"
        } else {
            "subprocess_failed"
        };
        let mut details = vec![format!(
            "exit_code={}",
            code_opt
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".into())
        )];
        let tail: String = stderr.trim().chars().take(400).collect();
        if !tail.is_empty() {
            details.push(format!("stderr={tail}"));
        }
        return Err(BridgeError {
            code: err_code.into(),
            message: format!("TAKO CLI failed ({err_code})."),
            details,
        });
    }

    Ok(CliOutput {
        exit_code: status.code().unwrap_or(0),
        stdout,
    })
}

fn spawn_bounded_reader<R: Read + Send + 'static>(
    mut reader: R,
    limit: usize,
    overflow: Arc<AtomicBool>,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if buf.len().saturating_add(n) > limit {
                        overflow.store(true, Ordering::Release);
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                Err(_) => break,
            }
        }
        buf
    })
}

// ---- Output Parsing & Scientific Classification --------------------------------------------

fn parse_json_stdout(stdout: &str) -> Result<Value, BridgeError> {
    serde_json::from_str(stdout.trim()).map_err(|e| {
        BridgeError::new(
            "invalid_response",
            format!("TAKO CLI returned malformed JSON: {e}"),
        )
    })
}

fn extract_workspace_scientific_status(manifest: &Value) -> Value {
    json!({
        "research_id": manifest.get("research_id").cloned().unwrap_or(Value::Null),
        "tako_version": manifest.get("tako_version").cloned().unwrap_or(Value::Null),
        "quality_status": manifest.get("quality_status").cloned().unwrap_or(Value::Null),
        "stages_effective": manifest.get("stages_effective").cloned().unwrap_or(Value::Null),
    })
}

fn classify_workspace_status(manifest: &Value) -> String {
    let verdict = manifest
        .get("quality_status")
        .and_then(|q| q.get("verdict"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if matches!(
        verdict,
        "INSUFFICIENT_EVIDENCE" | "NOT_VERIFIED" | "FAIL" | "FAILED"
    ) {
        "insufficient_evidence".into()
    } else {
        "succeeded".into()
    }
}

fn classify_discovery_result(payload: &Value) -> (String, Value) {
    let termination = payload
        .get("termination")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let accepted = payload
        .get("accepted")
        .and_then(|v| v.as_u64())
        .or_else(|| payload.get("papers").and_then(|v| v.as_u64()))
        .unwrap_or(0);
    let insufficient = termination.contains("INSUFFICIENT") || accepted == 0;
    let exec_status = if insufficient {
        "insufficient_evidence".into()
    } else {
        "succeeded".into()
    };
    let sci = json!({
        "status": payload.get("status").cloned().unwrap_or(Value::Null),
        "termination": payload.get("termination").cloned().unwrap_or(Value::Null),
        "accepted": accepted,
        "counters": payload.get("counters").cloned().unwrap_or(Value::Null),
        "evidence_sufficient": !insufficient,
    });
    (exec_status, sci)
}
