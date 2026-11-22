//! Proposal-only provider boundary. No engine, project, tools or evidence authority.
use crate::BridgeError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_RESPONSE: usize = 262_144;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFamily {
    OpenAi,
    Anthropic,
    Gemini,
    LocalModel,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalRequest {
    pub provider: ProviderFamily,
    pub model: String,
    pub prompt: String,
    pub timeout_s: u64,
    pub max_output_tokens: u32,
}
impl ProposalRequest {
    fn validate(&self) -> Result<(), BridgeError> {
        if self.model.is_empty()
            || self.model.len() > 128
            || !self
                .model
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-._:/".contains(&c))
            || self.prompt.trim().is_empty()
            || self.prompt.len() > 16_384
            || !(1..=120).contains(&self.timeout_s)
            || !(16..=4096).contains(&self.max_output_tokens)
        {
            return Err(err("invalid_input"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct ProposalResponse {
    pub provider: ProviderFamily,
    pub model: String,
    pub text: String,
    pub usage: Option<TokenUsage>,
    pub untrusted: bool,
}
#[derive(Clone, Serialize)]
pub struct ProposalJob {
    pub request_id: u64,
    pub state: &'static str,
    pub response: Option<ProposalResponse>,
    pub error: Option<BridgeError>,
}
struct Active {
    view: ProposalJob,
    cancel: Arc<AtomicBool>,
}
#[derive(Default)]
pub struct ProposalService {
    job: Arc<Mutex<Option<Active>>>,
    sequence: AtomicU64,
    #[cfg(all(feature = "native-qualification", debug_assertions))]
    fixture: bool,
}
/// Implementations are trusted native application code; output is untrusted text.
/// Cancellation is cooperative and must be checked throughout bounded transport I/O.
pub trait PlanningProvider: Send + Sync {
    fn identity(&self) -> ProviderFamily;
    fn propose(
        &self,
        request: &ProposalRequest,
        control: &RequestControl,
    ) -> Result<ProposalResponse, BridgeError>;
}
pub struct RequestControl {
    cancel: Arc<AtomicBool>,
    deadline: Instant,
}
impl RequestControl {
    pub fn check(&self) -> Result<(), BridgeError> {
        if self.cancel.load(Ordering::SeqCst) {
            Err(err("cancelled"))
        } else if Instant::now() >= self.deadline {
            Err(err("timeout"))
        } else {
            Ok(())
        }
    }
}
fn err(code: &str) -> BridgeError {
    let message = match code {
        "missing_credentials" => "Configure OPENAI_API_KEY in the native application's environment before submitting.",
        "authentication" => "The provider rejected authentication. Check native credentials and account access.",
        "network" => "The provider connection failed. No proposal is available.",
        "timeout" => "The proposal request timed out. The provider may still charge for work already received.",
        "cancelled" => "The local request was cancelled. Remote processing or billing may already have occurred.",
        "invalid_payload" => "The provider returned an invalid, incomplete or unsupported response.",
        "response_too_large" => "The provider response exceeded the local size limit.",
        "rate_limited" => "The provider rate or quota limit was reached.",
        "provider_error" => "The provider rejected or failed the request.",
        "provider_unavailable" => "This provider adapter is not implemented.",
        "transport_unavailable" => "The native HTTPS transport (/usr/bin/curl) is unavailable.",
        "busy" => "A proposal request is already active.",
        "not_found" => "The proposal request is no longer available.",
        "invalid_input" => "Use a model ID, 1–120 seconds, 16–4096 output tokens and a nonempty prompt of at most 16384 UTF-8 bytes.",
        _ => "The proposal request failed.",
    };
    BridgeError::new(code, message)
}
impl ProposalService {
    #[cfg(all(feature = "native-qualification", debug_assertions))]
    pub(crate) fn offline_fixture() -> Self {
        let mut service = Self::default();
        service.fixture = true;
        service
    }

    pub fn start(&self, request: ProposalRequest) -> Result<ProposalJob, BridgeError> {
        request.validate()?;
        #[cfg(all(feature = "native-qualification", debug_assertions))]
        if self.fixture {
            return self.start_with(request, Arc::new(OfflineNativeFixture));
        }
        if request.provider != ProviderFamily::OpenAi {
            return Err(err("provider_unavailable"));
        }
        // Credential access occurs only after an explicit submit, never at startup.
        let adapter = OpenAiAdapter::from_environment()?;
        self.start_with(request, Arc::new(adapter))
    }
    fn start_with(
        &self,
        request: ProposalRequest,
        adapter: Arc<dyn PlanningProvider>,
    ) -> Result<ProposalJob, BridgeError> {
        request.validate()?;
        if request.provider != adapter.identity() {
            return Err(err("provider_unavailable"));
        }
        let mut slot = self.job.lock().unwrap();
        if slot
            .as_ref()
            .is_some_and(|j| matches!(j.view.state, "pending" | "cancelling"))
        {
            return Err(err("busy"));
        }
        let request_id = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let cancel = Arc::new(AtomicBool::new(false));
        let view = ProposalJob {
            request_id,
            state: "pending",
            response: None,
            error: None,
        };
        *slot = Some(Active {
            view: view.clone(),
            cancel: cancel.clone(),
        });
        let jobs = self.job.clone();
        let control = RequestControl {
            cancel,
            deadline: Instant::now() + Duration::from_secs(request.timeout_s),
        };
        let spawned = std::thread::Builder::new()
            .name("tako-proposal".into())
            .spawn(move || {
                let outcome = adapter.propose(&request, &control);
                let mut slot = jobs.lock().unwrap();
                if let Some(job) = slot.as_mut().filter(|j| j.view.request_id == request_id) {
                    // Serialize cancellation vs completion, so a late response never wins.
                    let outcome = control.check().and(outcome);
                    match outcome {
                        Ok(response) => {
                            job.view.state = "succeeded";
                            job.view.response = Some(response);
                        }
                        Err(error) => {
                            job.view.state = if error.code == "cancelled" {
                                "cancelled"
                            } else {
                                "failed"
                            };
                            job.view.error = Some(error);
                        }
                    }
                }
            });
        if spawned.is_err() {
            *slot = None;
            return Err(err("provider_error"));
        }
        Ok(view)
    }
    pub fn poll(&self, id: u64) -> Result<ProposalJob, BridgeError> {
        self.job
            .lock()
            .unwrap()
            .as_ref()
            .filter(|j| j.view.request_id == id)
            .map(|j| j.view.clone())
            .ok_or_else(|| err("not_found"))
    }
    pub fn cancel(&self, id: u64) -> Result<ProposalJob, BridgeError> {
        let mut slot = self.job.lock().unwrap();
        let job = slot
            .as_mut()
            .filter(|j| j.view.request_id == id)
            .ok_or_else(|| err("not_found"))?;
        if matches!(job.view.state, "pending" | "cancelling") {
            job.cancel.store(true, Ordering::SeqCst);
            job.view.state = "cancelling";
        }
        Ok(job.view.clone())
    }
}
impl Drop for ProposalService {
    fn drop(&mut self) {
        if let Some(job) = self.job.lock().unwrap().as_ref() {
            job.cancel.store(true, Ordering::SeqCst);
        }
    }
}

// This adapter exists only in an explicitly selected debug qualification build.
// It has no key, endpoint, socket, tool, filesystem operation or fallback transport.
#[cfg(all(feature = "native-qualification", debug_assertions))]
struct OfflineNativeFixture;
#[cfg(all(feature = "native-qualification", debug_assertions))]
impl PlanningProvider for OfflineNativeFixture {
    fn identity(&self) -> ProviderFamily {
        ProviderFamily::OpenAi
    }
    fn propose(
        &self,
        r: &ProposalRequest,
        control: &RequestControl,
    ) -> Result<ProposalResponse, BridgeError> {
        control.check()?;
        match r.model.as_str() {
            "fixture-auth" => return Err(err("authentication")),
            "fixture-error" => return Err(err("provider_error")),
            "fixture-timeout" | "fixture-slow" => {
                let end = Instant::now()
                    + Duration::from_secs(if r.model == "fixture-timeout" { 121 } else { 3 });
                while Instant::now() < end {
                    control.check()?;
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            "fixture-success" | "fixture-long" => {}
            _ => return Err(err("provider_unavailable")),
        }
        Ok(ProposalResponse {
            provider: r.provider,
            model: r.model.clone(),
            text: format!(
                "OFFLINE FIXTURE — no model API. Exact received input:\n{}\n{}",
                r.prompt,
                if r.model == "fixture-long" {
                    "long_unbroken_source_identifier_".repeat(30)
                } else {
                    String::new()
                }
            ),
            usage: None,
            untrusted: true,
        })
    }
}

// The key deliberately implements neither Debug nor Serialize. No IPC endpoint override.
struct OpenAiAdapter {
    key: String,
    endpoint: String,
    fixture: bool,
}
impl OpenAiAdapter {
    fn from_environment() -> Result<Self, BridgeError> {
        let key = std::env::var("OPENAI_API_KEY").map_err(|_| err("missing_credentials"))?;
        if key.len() < 8
            || key.len() > 512
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err(err("missing_credentials"));
        }
        Ok(Self {
            key,
            endpoint: "https://api.openai.com/v1/responses".into(),
            fixture: false,
        })
    }
    fn request_body(request: &ProposalRequest) -> Value {
        json!({"model":request.model,"input":request.prompt,"max_output_tokens":request.max_output_tokens,
            "store":false,"stream":false,"tools":[],"tool_choice":"none"})
    }
}
// Curl config quoting is separate from shell quoting: no shell is ever invoked.
fn config_quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}
impl PlanningProvider for OpenAiAdapter {
    fn identity(&self) -> ProviderFamily {
        ProviderFamily::OpenAi
    }
    fn propose(
        &self,
        request: &ProposalRequest,
        control: &RequestControl,
    ) -> Result<ProposalResponse, BridgeError> {
        control.check()?;
        let body = Self::request_body(request).to_string();
        let config = format!(
            "header = {}\nheader = \"Content-Type: application/json\"\ndata-binary = {}\n",
            config_quote(&format!("Authorization: Bearer {}", self.key)),
            config_quote(&body)
        );
        let mut child = Command::new("/usr/bin/curl")
            .env_clear() // no inherited API keys, proxy settings, home config or TLS overrides
            .args([
                "--disable",
                "--silent",
                "--no-progress-meter",
                "--noproxy",
                "*",
                "--proto",
                if self.fixture { "=http" } else { "=https" },
                "--max-redirs",
                "0",
                "--connect-timeout",
                "10",
                "--max-time",
                &request.timeout_s.to_string(),
                "--max-filesize",
                &MAX_RESPONSE.to_string(),
                "--write-out",
                "\n%{http_code}",
                "--config",
                "-",
                "--url",
                &self.endpoint,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| err("transport_unavailable"))?;
        let mut stdin = child.stdin.take().unwrap();
        // Writer/reader are bounded and joined after the child is reaped, including cancellation.
        let writer = std::thread::spawn(move || stdin.write_all(config.as_bytes()));
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stdout
                .take((MAX_RESPONSE + 5) as u64)
                .read_to_end(&mut bytes)
                .map(|_| bytes);
            let _ = tx.send(result);
        });
        let mut received = None;
        let result = loop {
            if let Err(e) = control.check() {
                break Err(e);
            }
            if received.is_none() {
                match rx.try_recv() {
                    Ok(Ok(bytes)) if bytes.len() >= MAX_RESPONSE + 5 => {
                        break Err(err("response_too_large"))
                    }
                    Ok(Ok(bytes)) => received = Some(bytes),
                    Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => {
                        break Err(err("network"))
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    let bytes = match received.take() {
                        Some(b) => Ok(b),
                        None => rx.recv().map_err(|_| ()).and_then(|r| r.map_err(|_| ())),
                    };
                    break match bytes {
                        Ok(bytes) if status.success() => self.parse_http(&bytes),
                        _ if status.code() == Some(28) => Err(err("timeout")),
                        _ if status.code() == Some(63) => Err(err("response_too_large")),
                        _ => Err(err("network")),
                    };
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => break Err(err("network")),
            }
        };
        let _ = child.kill();
        let _ = child.wait();
        let _ = writer.join();
        let _ = reader.join();
        control.check()?;
        result
    }
}
impl OpenAiAdapter {
    fn parse_http(&self, bytes: &[u8]) -> Result<ProposalResponse, BridgeError> {
        if bytes.len() > MAX_RESPONSE + 4 {
            return Err(err("response_too_large"));
        }
        let split = bytes
            .iter()
            .rposition(|b| *b == b'\n')
            .ok_or_else(|| err("invalid_payload"))?;
        let code = std::str::from_utf8(&bytes[split + 1..]).map_err(|_| err("invalid_payload"))?;
        match code {
            "200" => {}
            "401" | "403" => return Err(err("authentication")),
            "429" => return Err(err("rate_limited")),
            _ => return Err(err("provider_error")),
        }
        let v: Value =
            serde_json::from_slice(&bytes[..split]).map_err(|_| err("invalid_payload"))?;
        if v["object"] != "response" || v["status"] != "completed" || !v["error"].is_null() {
            return Err(err("invalid_payload"));
        }
        let model = v["model"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .ok_or_else(|| err("invalid_payload"))?;
        let output = v["output"]
            .as_array()
            .ok_or_else(|| err("invalid_payload"))?;
        let mut texts = Vec::new();
        for item in output {
            if item["type"] == "reasoning" {
                continue;
            }
            if item["type"] != "message"
                || item["role"] != "assistant"
                || item["status"] != "completed"
            {
                return Err(err("invalid_payload"));
            }
            for content in item["content"]
                .as_array()
                .ok_or_else(|| err("invalid_payload"))?
            {
                if content["type"] != "output_text" {
                    return Err(err("invalid_payload"));
                }
                texts.push(
                    content["text"]
                        .as_str()
                        .ok_or_else(|| err("invalid_payload"))?,
                );
            }
        }
        let text = texts.join("\n");
        if text.trim().is_empty() {
            return Err(err("invalid_payload"));
        }
        let usage = if v["usage"].is_null() {
            None
        } else {
            let u = &v["usage"];
            let usage = TokenUsage {
                input_tokens: u["input_tokens"]
                    .as_u64()
                    .ok_or_else(|| err("invalid_payload"))?,
                output_tokens: u["output_tokens"]
                    .as_u64()
                    .ok_or_else(|| err("invalid_payload"))?,
                total_tokens: u["total_tokens"]
                    .as_u64()
                    .ok_or_else(|| err("invalid_payload"))?,
            };
            if usage.input_tokens.checked_add(usage.output_tokens) != Some(usage.total_tokens) {
                return Err(err("invalid_payload"));
            }
            Some(usage)
        };
        Ok(ProposalResponse {
            provider: self.identity(),
            model: model.replace(&self.key, "[REDACTED]"),
            text: text.replace(&self.key, "[REDACTED]"),
            usage,
            untrusted: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc::{Receiver, RecvTimeoutError};
    const KEY: &str = "fixture-credential-only";
    fn request() -> ProposalRequest {
        ProposalRequest {
            provider: ProviderFamily::OpenAi,
            model: "fixture-model".into(),
            prompt: "Explain a safe refactor".into(),
            timeout_s: 2,
            max_output_tokens: 128,
        }
    }
    fn response(text: &str) -> Value {
        json!({"object":"response","status":"completed","error":null,"model":"fixture-model",
          "output":[{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text}]}],
          "usage":{"input_tokens":3,"output_tokens":7,"total_tokens":10}})
    }
    fn control(seconds: u64) -> RequestControl {
        RequestControl {
            cancel: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + Duration::from_secs(seconds),
        }
    }
    fn read_request(stream: &mut TcpStream) -> Vec<u8> {
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 2048];
        loop {
            let n = stream.read(&mut buffer).unwrap();
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let length: usize = headers
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                if bytes.len() >= end + 4 + length {
                    break;
                }
            }
        }
        bytes
    }
    // Only test builds can construct a non-TLS loopback endpoint. IPC has no URL field.
    fn server(
        status: u16,
        body: String,
        delay: Duration,
    ) -> (
        OpenAiAdapter,
        Receiver<Vec<u8>>,
        std::thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut stream = loop {
                if let Ok((stream, _)) = listener.accept() {
                    break stream;
                }
                assert!(
                    Instant::now() < deadline,
                    "fixture never received a request"
                );
                std::thread::sleep(Duration::from_millis(5));
            };
            let _ = tx.send(read_request(&mut stream));
            std::thread::sleep(delay);
            if status == 0 {
                return;
            } // simulate an abruptly lost connection
            let header = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(body.as_bytes());
        });
        (
            OpenAiAdapter {
                key: KEY.into(),
                endpoint,
                fixture: true,
            },
            rx,
            handle,
        )
    }
    fn finish(service: &ProposalService, id: u64) -> ProposalJob {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            let job = service.poll(id).unwrap();
            if !matches!(job.state, "pending" | "cancelling") {
                return job;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn valid_http_uses_only_explicit_prompt_no_tools_and_redacts_credential() {
        let (adapter, rx, handle) = server(
            200,
            response(&format!("<script>not executed</script> {KEY}")).to_string(),
            Duration::ZERO,
        );
        let mut r = request();
        r.prompt = "Literal \\\"\n$(do-not-execute)\t unicode 🐙".into();
        let result = adapter.propose(&r, &control(3)).unwrap();
        assert!(result.untrusted);
        assert_eq!(result.usage.unwrap().total_tokens, 10);
        assert!(!result.text.contains(KEY));
        assert!(result.text.contains("[REDACTED]"));
        let raw = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let raw = String::from_utf8(raw).unwrap();
        assert!(raw.starts_with("POST /v1/responses HTTP/1.1"));
        assert!(raw.contains(&format!("Authorization: Bearer {KEY}")));
        let payload: Value = serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(payload["input"], r.prompt);
        assert_eq!(payload["model"], r.model);
        assert_eq!(payload["max_output_tokens"], 128);
        assert_eq!(payload["tools"], json!([]));
        assert_eq!(payload["tool_choice"], "none");
        assert_eq!(payload["store"], false);
        assert_eq!(payload["stream"], false);
        assert_eq!(payload.as_object().unwrap().len(), 7);
        assert!(!payload.to_string().contains(KEY));
        handle.join().unwrap();
    }
    #[test]
    fn http_errors_never_expose_raw_provider_body() {
        for (status, code) in [
            (401, "authentication"),
            (403, "authentication"),
            (429, "rate_limited"),
            (500, "provider_error"),
            (302, "provider_error"),
        ] {
            let (adapter, _, handle) = server(status, format!("leaked {KEY}"), Duration::ZERO);
            let e = adapter.propose(&request(), &control(3)).unwrap_err();
            assert_eq!(e.code, code);
            assert!(!serde_json::to_string(&e).unwrap().contains(KEY));
            handle.join().unwrap();
        }
    }
    #[test]
    fn network_failure_is_explicit() {
        let (adapter, _, handle) = server(0, String::new(), Duration::ZERO);
        assert_eq!(
            adapter.propose(&request(), &control(3)).unwrap_err().code,
            "network"
        );
        handle.join().unwrap();
    }
    #[test]
    fn timeout_reaps_transport_and_does_not_return_success() {
        let (adapter, _, handle) = server(
            200,
            response("too late").to_string(),
            Duration::from_millis(1300),
        );
        let mut r = request();
        r.timeout_s = 1;
        let service = ProposalService::default();
        let job = service.start_with(r, Arc::new(adapter)).unwrap();
        let job = finish(&service, job.request_id);
        assert_eq!(job.state, "failed");
        assert_eq!(job.error.unwrap().code, "timeout");
        assert!(job.response.is_none());
        handle.join().unwrap();
    }
    #[test]
    fn cancellation_is_bound_to_request_and_rejects_late_success() {
        let (adapter, rx, handle) = server(
            200,
            response("late").to_string(),
            Duration::from_millis(300),
        );
        let service = ProposalService::default();
        let job = service.start_with(request(), Arc::new(adapter)).unwrap();
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(
            service.cancel(job.request_id + 1).err().unwrap().code,
            "not_found"
        );
        assert_eq!(service.cancel(job.request_id).unwrap().state, "cancelling");
        let done = finish(&service, job.request_id);
        assert_eq!(done.state, "cancelled");
        assert!(done.response.is_none());
        assert_eq!(done.error.unwrap().code, "cancelled");
        handle.join().unwrap();
    }
    #[test]
    fn invalid_incomplete_refused_or_tool_payloads_fail_closed() {
        let mut variants = vec![json!({}), response("")];
        let mut incomplete = response("partial");
        incomplete["status"] = json!("incomplete");
        variants.push(incomplete);
        let mut tool = response("ok");
        tool["output"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"function_call","name":"shell"}));
        variants.push(tool);
        let mut refused = response("ok");
        refused["output"][0]["content"][0] = json!({"type":"refusal","refusal":"no"});
        variants.push(refused);
        let mut bad_usage = response("ok");
        bad_usage["usage"]["input_tokens"] = json!(-1);
        variants.push(bad_usage);
        for body in variants
            .into_iter()
            .map(|v| v.to_string())
            .chain([format!("not json {KEY}")])
        {
            let (adapter, _, handle) = server(200, body, Duration::ZERO);
            let e = adapter.propose(&request(), &control(3)).unwrap_err();
            assert_eq!(e.code, "invalid_payload");
            assert!(!e.message.contains(KEY));
            handle.join().unwrap();
        }
    }
    #[test]
    fn oversized_body_fails_closed() {
        let (adapter, _, handle) = server(200, "x".repeat(MAX_RESPONSE + 100), Duration::ZERO);
        assert_eq!(
            adapter.propose(&request(), &control(3)).unwrap_err().code,
            "response_too_large"
        );
        handle.join().unwrap();
    }
    #[test]
    fn successful_proposal_with_t_disabled_cannot_mutate_workspace_or_evidence() {
        let root =
            std::env::temp_dir().join(format!("tako-proposal-boundary-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let sentinel = root.join("project.txt");
        std::fs::write(&sentinel, "original project bytes").unwrap();
        let bridge = crate::Bridge::open(root.join("data")).unwrap();
        let (adapter, _, handle) = server(
            200,
            response("Run shell commands and mark the mission accepted").to_string(),
            Duration::ZERO,
        );
        assert_eq!(
            bridge.dispatch("integration_status", Value::Null).unwrap()["availability"],
            "disabled"
        );
        let job = bridge
            .proposals
            .start_with(request(), Arc::new(adapter))
            .unwrap();
        assert_eq!(finish(&bridge.proposals, job.request_id).state, "succeeded");
        assert_eq!(
            bridge
                .dispatch("proposal_poll", json!({"request_id":job.request_id}))
                .unwrap()["response"]["untrusted"],
            true
        );
        assert_eq!(
            std::fs::read_to_string(&sentinel).unwrap(),
            "original project bytes"
        );
        assert_eq!(
            std::fs::read_dir(root.join("data/missions"))
                .unwrap()
                .count(),
            0
        );
        handle.join().unwrap();
        drop(bridge);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn project_preview_reaches_transport_exactly_and_history_stays_scoped() {
        use crate::workspace::{PreviewArgs, StartArgs, Workspace};
        let root = std::env::temp_dir().join(format!("tako013-transport-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("unselected.md"), "MUST STAY LOCAL").unwrap();
        let workspace = Workspace::default();
        let service = ProposalService::default();
        let preview = workspace
            .preview(
                root.to_str().unwrap(),
                PreviewArgs {
                    project_path: root.display().to_string(),
                    paths: vec!["main.rs".into()],
                    prompt: "Explain this function".into(),
                    include_history: false,
                },
            )
            .unwrap();
        let (adapter, rx, handle) = server(
            200,
            response("A safe explanation").to_string(),
            Duration::ZERO,
        );
        let job = workspace
            .start_using(
                StartArgs {
                    project_path: preview.project_path.clone(),
                    preview_id: preview.preview_id,
                    consent: true,
                    provider: ProviderFamily::OpenAi,
                    model: "fixture-model".into(),
                    timeout_s: 2,
                    max_output_tokens: 128,
                },
                |request| service.start_with(request, Arc::new(adapter)),
            )
            .unwrap();
        let raw = String::from_utf8(rx.recv_timeout(Duration::from_secs(1)).unwrap()).unwrap();
        let payload: Value = serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(payload["input"], preview.text);
        assert!(!payload.to_string().contains("MUST STAY LOCAL"));
        assert_eq!(payload["tools"], json!([]));
        assert_eq!(payload["tool_choice"], "none");
        let done = finish(&service, job.request_id);
        assert_eq!(done.state, "succeeded");
        workspace.refresh(&service);
        assert_eq!(
            workspace.history(root.to_str().unwrap())[0].response,
            "A safe explanation"
        );
        assert!(workspace.history("/different/project").is_empty());
        assert_eq!(
            std::fs::read_to_string(root.join("main.rs")).unwrap(),
            "fn main() {}\n"
        );
        handle.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(feature = "native-qualification")]
    #[test]
    fn offline_native_fixture_preserves_errors_limits_cancellation_and_never_falls_back() {
        let service = ProposalService::offline_fixture();
        for (model, code) in [
            ("fixture-auth", "authentication"),
            ("fixture-error", "provider_error"),
            ("unknown-model", "provider_unavailable"),
        ] {
            let mut r = request();
            r.model = model.into();
            let job = service.start(r).unwrap();
            let done = finish(&service, job.request_id);
            assert_eq!(done.state, "failed");
            assert_eq!(done.error.unwrap().code, code);
        }
        let mut r = request();
        r.model = "fixture-timeout".into();
        r.timeout_s = 1;
        let job = service.start(r).unwrap();
        assert_eq!(
            finish(&service, job.request_id).error.unwrap().code,
            "timeout"
        );
        let mut r = request();
        r.model = "fixture-slow".into();
        let job = service.start(r).unwrap();
        service.cancel(job.request_id).unwrap();
        assert_eq!(finish(&service, job.request_id).state, "cancelled");
        let mut r = request();
        r.model = "fixture-success".into();
        let job = service.start(r.clone()).unwrap();
        let response = finish(&service, job.request_id).response.unwrap();
        assert!(response.text.contains("OFFLINE FIXTURE"));
        assert!(response.text.contains(&r.prompt));
        r.timeout_s = 0;
        assert_eq!(service.start(r).err().unwrap().code, "invalid_input");
    }
    #[test]
    fn single_flight_no_fallback_and_ephemeral_results() {
        let (adapter, rx, handle) = server(
            200,
            response("suggestion").to_string(),
            Duration::from_millis(150),
        );
        let service = ProposalService::default();
        let adapter: Arc<dyn PlanningProvider> = Arc::new(adapter);
        let job = service.start_with(request(), adapter.clone()).unwrap();
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(
            service
                .start_with(request(), adapter.clone())
                .err()
                .unwrap()
                .code,
            "busy"
        );
        assert_eq!(finish(&service, job.request_id).state, "succeeded");
        let mut unsupported = request();
        unsupported.provider = ProviderFamily::Anthropic;
        assert_eq!(
            service.start(unsupported).err().unwrap().code,
            "provider_unavailable"
        );
        assert!(serde_json::from_value::<ProposalRequest>(json!({"provider":"open_ai","model":"x","prompt":"x","timeout_s":1,"max_output_tokens":16,"endpoint":"http://evil"})).is_err());
        assert!(serde_json::from_value::<ProposalRequest>(json!({"provider":"open_ai","model":"x","prompt":"x","timeout_s":1,"max_output_tokens":16,"api_key":KEY})).is_err());
        for bad in ["", "model\nheader", "model\"", "../model?x"] {
            let mut r = request();
            r.model = bad.into();
            assert!(r.validate().is_err());
        }
        assert!(
            !serde_json::to_string(&service.poll(job.request_id).unwrap())
                .unwrap()
                .contains(KEY)
        );
        handle.join().unwrap();
        assert!(matches!(
            rx.recv_timeout(Duration::from_millis(30)),
            Err(RecvTimeoutError::Disconnected)
        ));
    }
}
