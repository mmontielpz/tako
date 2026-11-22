use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024; // 4 MB safety ceiling

/// Encode an opaque ID as one URL component; never interpolate it as an HTTP path.
pub(crate) fn encode_component(s: &str) -> Result<String, String> {
    if s.trim().is_empty() || s.chars().count() > 256 || s.chars().any(char::is_control) {
        return Err("Invalid backend identity.".into());
    }
    let mut encoded = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            encoded.push(b as char);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    Ok(encoded)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendHealth {
    pub status: String,
    pub service: String,
    pub api_version: String,
    pub operational_mode: String,
    pub g12_state: String,
    pub aws_mutations: String,
    pub runtime_status: Option<String>,
    pub active_missions_count: Option<u64>,
    pub queued_missions_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredProject {
    pub project_id: String,
    pub name: String,
    pub path: String,
    pub exists: bool,
    pub is_git: bool,
    pub branch: String,
    pub head: String,
    pub dirty: bool,
    pub allowed_root: String,
    pub isolation_profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectsResponse {
    pub projects: Vec<RegisteredProject>,
    pub total: usize,
    pub allowed_roots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendGoal {
    pub id: String,
    pub project_id: String,
    pub goal: String,
    pub state: String,
    pub progress_percent: f64,
    pub outcome_verification: Option<String>,
    pub requires_human_attention: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectGoalsResponse {
    pub project_id: String,
    pub goals: Vec<BackendGoal>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceLimits {
    pub spending_limit_usd: f64,
    pub spending_currency: String,
    pub budget_gate: String,
    pub max_runtime_hours: u32,
    pub max_concurrent_workers: u32,
    pub prohibited_operations: Vec<String>,
    pub g12_state: String,
    pub aws_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendApproval {
    pub approval_id: String,
    pub kind: String,
    pub project: String,
    pub operation: String,
    pub status: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalsResponse {
    pub approvals: Vec<BackendApproval>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workstation {
    pub workstation_id: String,
    pub display_name: String,
    pub host: String,
    pub user: String,
    pub os: String,
    pub connectivity_status: String,
    pub last_seen: Option<String>,
    pub transport: String,
    pub capabilities: Vec<String>,
    pub is_local: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkstationsResponse {
    pub workstations: Vec<Workstation>,
    pub total: usize,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkstationDetailResponse {
    pub workstation: Workstation,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendStatusView {
    pub online: bool,
    pub base_url: String,
    pub status: String,
    pub runtime_status: String,
    pub g12_state: String,
    pub operational_mode: String,
    pub projects: Vec<RegisteredProject>,
    pub limits: Option<GovernanceLimits>,
    pub approvals: Vec<BackendApproval>,
    #[serde(default)]
    pub workstations: Vec<Workstation>,
    pub details: Option<Value>,
}

/// Decodes chunked HTTP transfer encoding (RFC 9112).
pub fn decode_chunked_body(mut input: &[u8]) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    loop {
        let crlf = input
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("Malformed chunked stream: missing CRLF after chunk size")?;

        let size_str = std::str::from_utf8(&input[..crlf])
            .map_err(|e| format!("Invalid UTF-8 in chunk size: {e}"))?
            .trim();
        let size_hex = size_str.split(';').next().unwrap_or(size_str).trim();
        let chunk_size = usize::from_str_radix(size_hex, 16)
            .map_err(|e| format!("Invalid hex chunk size '{size_hex}': {e}"))?;

        input = &input[crlf + 2..];

        if chunk_size == 0 {
            break;
        }

        if input.len() < chunk_size {
            return Err(format!(
                "Premature EOF in chunked body: chunk size is {}, available bytes {}",
                chunk_size,
                input.len()
            ));
        }

        output.extend_from_slice(&input[..chunk_size]);
        input = &input[chunk_size..];

        if input.len() < 2 || &input[..2] != b"\r\n" {
            return Err("Malformed chunked stream: chunk not followed by CRLF".to_string());
        }
        input = &input[2..];
    }
    Ok(output)
}

/// Parses a complete raw HTTP/1.1 response buffer with strict RFC 9112 checks.
pub fn parse_http_response(buf: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let header_end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "Malformed HTTP response: no CRLFCRLF found".to_string())?;

    let headers_str = std::str::from_utf8(&buf[..header_end])
        .map_err(|e| format!("Malformed HTTP response: invalid UTF-8 in headers: {e}"))?;

    let mut lines = headers_str.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| "Empty HTTP response".to_string())?;

    let mut status_parts = status_line.split_whitespace();
    let version = status_parts.next().ok_or("Missing HTTP version")?;
    if !version.starts_with("HTTP/") {
        return Err(format!("Malformed HTTP status line: '{status_line}'"));
    }
    let status_code: u16 = status_parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("Malformed HTTP status code in '{status_line}'"))?;

    let mut content_length: Option<usize> = None;
    let mut is_chunked = false;

    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim().to_ascii_lowercase();
            if key == "content-length" {
                if let Ok(len) = val.parse::<usize>() {
                    content_length = Some(len);
                }
            } else if key == "transfer-encoding" && val.contains("chunked") {
                is_chunked = true;
            }
        }
    }

    let raw_body = &buf[header_end + 4..];

    if is_chunked {
        let decoded = decode_chunked_body(raw_body)?;
        Ok((status_code, decoded))
    } else if let Some(expected_len) = content_length {
        if raw_body.len() < expected_len {
            return Err(format!(
                "Premature EOF: received {} bytes, expected Content-Length {} bytes",
                raw_body.len(),
                expected_len
            ));
        }
        Ok((status_code, raw_body[..expected_len].to_vec()))
    } else {
        Ok((status_code, raw_body.to_vec()))
    }
}

#[derive(Debug, Clone)]
pub struct TakoBackendClient {
    base_host: String,
    base_port: u16,
    token: Option<String>,
}

impl Default for TakoBackendClient {
    fn default() -> Self {
        Self::new("127.0.0.1", 8085)
    }
}

impl TakoBackendClient {
    pub fn new(host: &str, port: u16) -> Self {
        let token = Self::read_operator_token();
        TakoBackendClient {
            base_host: host.to_string(),
            base_port: port,
            token,
        }
    }

    /// Isolated fixture clients must not read or transmit host operator credentials.
    pub fn unauthenticated(host: &str, port: u16) -> Self {
        Self {
            base_host: host.into(),
            base_port: port,
            token: None,
        }
    }

    fn read_operator_token() -> Option<String> {
        let home = std::env::var("HOME").ok()?;
        let path = PathBuf::from(home)
            .join(".t-control")
            .join("operator.token");
        let metadata = std::fs::symlink_metadata(&path).ok()?;
        if !metadata.is_file() || metadata.len() > 4096 {
            return None;
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .ok()?
            .take(4097)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() > 4096 {
            return None;
        }
        let token = String::from_utf8(bytes).ok()?.trim().to_string();
        if token.is_empty() || token.chars().any(char::is_whitespace) {
            None
        } else {
            Some(token)
        }
    }

    /// Exact HTTP body bytes for strict evidence parsing and future authentication.
    pub fn raw_get_bytes(&self, path: &str, auth: bool) -> Result<(u16, Vec<u8>), String> {
        let addr_str = format!("{}:{}", self.base_host, self.base_port);
        let addr: SocketAddr = addr_str
            .parse()
            .map_err(|e| format!("Invalid backend socket address: {e}"))?;

        // Gate G2 Security: Loopback-only enforcement
        if !addr.ip().is_loopback() {
            return Err(format!(
                "Security violation: Only loopback addresses (127.0.0.1, ::1) are permitted for T backend communication, got {}",
                addr.ip()
            ));
        }

        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(800))
            .map_err(|e| format!("Failed to connect to T backend at {addr_str}: {e}"))?;

        stream
            .set_read_timeout(Some(Duration::from_millis(2000)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_millis(2000)))
            .map_err(|e| e.to_string())?;

        let mut req = format!(
            "GET {path} HTTP/1.1\r\nHost: {addr_str}\r\nUser-Agent: TAKO-Desktop/0.0.1\r\nAccept: application/json\r\nConnection: close\r\n"
        );
        if auth {
            if let Some(ref tok) = self.token {
                req.push_str(&format!("Authorization: Bearer {tok}\r\n"));
            }
        }
        req.push_str("\r\n");

        stream
            .write_all(req.as_bytes())
            .map_err(|e| format!("Failed to write request: {e}"))?;

        // Gate G2 Security: Enforce maximum response size
        let mut buf = Vec::new();
        let mut take = (&mut stream).take((MAX_RESPONSE_BYTES + 1) as u64);
        take.read_to_end(&mut buf)
            .map_err(|e| format!("Failed to read response: {e}"))?;

        if buf.len() > MAX_RESPONSE_BYTES {
            return Err(format!(
                "Security violation: Response exceeded maximum permitted size of {} bytes",
                MAX_RESPONSE_BYTES
            ));
        }

        parse_http_response(&buf)
    }

    pub fn raw_get(&self, path: &str, auth: bool) -> Result<(u16, Value), String> {
        let (status_code, body_bytes) = self.raw_get_bytes(path, auth)?;

        let json_val: Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                if status_code >= 400 {
                    serde_json::json!({
                        "error": format!("HTTP_{status_code}"),
                        "raw_body": String::from_utf8_lossy(&body_bytes)
                    })
                } else {
                    return Err(format!("Invalid JSON response: {e}"));
                }
            }
        };

        Ok((status_code, json_val))
    }

    pub fn get_backend_status(&self) -> BackendStatusView {
        match self.raw_get("/health", false) {
            Ok((200, v)) => {
                let status = v
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("UNKNOWN")
                    .to_string();
                let operational_mode = v
                    .get("operational_mode")
                    .and_then(|s| s.as_str())
                    .unwrap_or("UNKNOWN")
                    .to_string();
                let g12_state = v
                    .get("g12_state")
                    .and_then(|s| s.as_str())
                    .unwrap_or("UNKNOWN")
                    .to_string();
                let runtime_status = v
                    .pointer("/subsystems/t_runtime/details/worker_status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("UNKNOWN")
                    .to_string();

                let projects = self.list_projects().map(|r| r.projects).unwrap_or_default();
                let limits = self.get_governance_limits().ok();
                let approvals = self
                    .get_approvals()
                    .map(|r| r.approvals)
                    .unwrap_or_default();
                let workstations = self
                    .list_workstations()
                    .map(|r| r.workstations)
                    .unwrap_or_default();

                BackendStatusView {
                    online: true,
                    base_url: format!("http://{}:{}", self.base_host, self.base_port),
                    status,
                    runtime_status,
                    g12_state,
                    operational_mode,
                    projects,
                    limits,
                    approvals,
                    workstations,
                    details: Some(v),
                }
            }
            Ok((code, v)) => BackendStatusView {
                online: false,
                base_url: format!("http://{}:{}", self.base_host, self.base_port),
                status: format!("HTTP_{code}"),
                runtime_status: "UNREACHABLE".into(),
                g12_state: "UNKNOWN".into(),
                operational_mode: "OFFLINE".into(),
                projects: vec![],
                limits: None,
                approvals: vec![],
                workstations: vec![],
                details: Some(v),
            },
            Err(e) => BackendStatusView {
                online: false,
                base_url: format!("http://{}:{}", self.base_host, self.base_port),
                status: "OFFLINE".into(),
                runtime_status: "UNREACHABLE".into(),
                g12_state: "UNKNOWN".into(),
                operational_mode: "OFFLINE".into(),
                projects: vec![],
                limits: None,
                approvals: vec![],
                workstations: vec![],
                details: Some(serde_json::json!({ "error": e })),
            },
        }
    }

    pub fn list_workstations(&self) -> Result<WorkstationsResponse, String> {
        let (code, v) = self.raw_get("/api/v1/workstations", true)?;
        if code != 200 {
            return Err(format!("Backend returned HTTP {code}: {v}"));
        }
        serde_json::from_value(v).map_err(|e| format!("Failed to parse workstations: {e}"))
    }

    pub fn get_workstation(&self, workstation_id: &str) -> Result<Workstation, String> {
        let path = format!("/api/v1/workstations/{}", encode_component(workstation_id)?);
        let (code, v) = self.raw_get(&path, true)?;
        if code != 200 {
            return Err(format!("Backend returned HTTP {code}: {v}"));
        }
        let resp: WorkstationDetailResponse =
            serde_json::from_value(v).map_err(|e| format!("Failed to parse workstation: {e}"))?;
        Ok(resp.workstation)
    }

    pub fn list_projects(&self) -> Result<ProjectsResponse, String> {
        let (code, v) = self.raw_get("/api/v1/projects", true)?;
        if code != 200 {
            return Err(format!("Backend returned HTTP {code}: {v}"));
        }
        serde_json::from_value(v).map_err(|e| format!("Failed to parse projects: {e}"))
    }

    pub fn get_project_goals(&self, project_id: &str) -> Result<ProjectGoalsResponse, String> {
        let path = format!("/api/v1/projects/{}/goals", encode_component(project_id)?);
        let (code, v) = self.raw_get(&path, true)?;
        if code != 200 {
            return Err(format!("Backend returned HTTP {code}: {v}"));
        }
        serde_json::from_value(v).map_err(|e| format!("Failed to parse goals: {e}"))
    }

    pub fn get_governance_limits(&self) -> Result<GovernanceLimits, String> {
        let (code, v) = self.raw_get("/api/v1/governance/limits", true)?;
        if code != 200 {
            return Err(format!("Backend returned HTTP {code}: {v}"));
        }
        serde_json::from_value(v).map_err(|e| format!("Failed to parse governance limits: {e}"))
    }

    pub fn get_approvals(&self) -> Result<ApprovalsResponse, String> {
        let (code, v) = self.raw_get("/api/v1/approvals", true)?;
        if code != 200 {
            return Err(format!("Backend returned HTTP {code}: {v}"));
        }
        serde_json::from_value(v).map_err(|e| format!("Failed to parse approvals: {e}"))
    }
}
