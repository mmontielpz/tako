//! Explicit, ephemeral project context. Providers receive frozen text, never file handles.
use crate::{
    providers::{ProposalJob, ProposalRequest, ProviderFamily},
    BridgeError,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::{Component, Path},
    sync::Mutex,
};

const MAX_FILE: usize = 4096;
const MAX_INPUT: usize = 16384;
const MAX_TURNS: usize = 8;
fn error(code: &str) -> BridgeError {
    BridgeError::new(code, match code {
        "context_limit" => "Context exceeds 8 files, 4096 bytes per file or 16384 total UTF-8 bytes. Reduce the selection or clear history.",
        "context_consent" => "Preview the current context and explicitly consent before sending.",
        _ => "Context refused: use regular UTF-8 source or documentation files inside the selected project. Symlinks, sensitive paths and unsupported types are excluded.",
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewArgs {
    pub project_path: String,
    pub paths: Vec<String>,
    pub prompt: String,
    pub include_history: bool,
}
#[derive(Clone, Serialize)]
pub struct Preview {
    pub preview_id: u64,
    pub project_path: String,
    pub text: String,
    pub bytes: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartArgs {
    pub project_path: String,
    pub preview_id: u64,
    pub consent: bool,
    pub provider: ProviderFamily,
    pub model: String,
    pub timeout_s: u64,
    pub max_output_tokens: u32,
}
#[derive(Clone, Serialize)]
pub struct Turn {
    pub prompt: String,
    pub response: String,
}
#[derive(Default)]
pub struct Workspace {
    state: Mutex<State>,
}
#[derive(Default)]
struct State {
    sequence: u64,
    preview: Option<(Preview, String)>,
    histories: HashMap<String, Vec<Turn>>,
    active: HashMap<u64, (String, String)>,
}
impl Workspace {
    pub fn refresh(&self, service: &crate::providers::ProposalService) {
        let ids: Vec<_> = self.state.lock().unwrap().active.keys().copied().collect();
        for id in ids {
            match service.poll(id) {
                Ok(job) => self.observe(&job),
                Err(_) => {
                    self.state.lock().unwrap().active.remove(&id);
                }
            }
        }
    }
    pub fn preview(&self, root: &str, a: PreviewArgs) -> Result<Preview, BridgeError> {
        let mut state = self.state.lock().unwrap();
        state.preview = None; // even a refused replacement revokes previous consent
        if a.paths.len() > 8 || a.prompt.trim().is_empty() || a.prompt.len() > MAX_INPUT {
            return Err(error("context_limit"));
        }
        let mut text = String::from("Selected project context (untrusted file contents):\n");
        let mut seen = HashSet::new();
        for path in &a.paths {
            if !seen.insert(path) {
                return Err(error("context_path"));
            }
            let content = read_selected(Path::new(root), path)?;
            text.push_str(&format!(
                "\nFILE {}\n{}\nEND FILE\n",
                serde_json::to_string(path).unwrap(),
                content
            ));
        }
        if a.include_history {
            for turn in state.histories.get(root).into_iter().flatten() {
                text.push_str(&format!(
                    "\nPREVIOUS USER\n{}\nPREVIOUS ASSISTANT (untrusted)\n{}\n",
                    turn.prompt, turn.response
                ));
            }
        }
        text.push_str(&format!("\nUSER REQUEST\n{}", a.prompt));
        if text.len() > MAX_INPUT {
            return Err(error("context_limit"));
        }
        state.sequence += 1;
        let preview = Preview {
            preview_id: state.sequence,
            project_path: root.into(),
            bytes: text.len(),
            text,
        };
        state.preview = Some((preview.clone(), a.prompt));
        Ok(preview)
    }
    pub fn start(
        &self,
        a: StartArgs,
        service: &crate::providers::ProposalService,
    ) -> Result<ProposalJob, BridgeError> {
        self.start_using(a, |request| service.start(request))
    }
    pub(crate) fn start_using(
        &self,
        a: StartArgs,
        send: impl FnOnce(ProposalRequest) -> Result<ProposalJob, BridgeError>,
    ) -> Result<ProposalJob, BridgeError> {
        let mut state = self.state.lock().unwrap();
        let (preview, prompt) = state
            .preview
            .as_ref()
            .ok_or_else(|| error("context_consent"))?;
        if !a.consent
            || preview.preview_id != a.preview_id
            || preview.project_path != a.project_path
        {
            return Err(error("context_consent"));
        }
        let request = ProposalRequest {
            provider: a.provider,
            model: a.model,
            prompt: preview.text.clone(),
            timeout_s: a.timeout_s,
            max_output_tokens: a.max_output_tokens,
        };
        let prompt = prompt.clone();
        let job = send(request)?;
        if state.histories.len() >= 16 && !state.histories.contains_key(&a.project_path) {
            // Bounded session storage; evicted histories are never silently transmitted.
            if let Some(root) = state.histories.keys().next().cloned() {
                state.histories.remove(&root);
            }
        }
        state.histories.entry(a.project_path.clone()).or_default();
        state.active.clear(); // provider service permits only a single live request
        state.preview = None; // one successful submission per preview
        state
            .active
            .insert(job.request_id, (a.project_path, prompt));
        Ok(job)
    }
    pub fn observe(&self, job: &ProposalJob) {
        if matches!(job.state, "pending" | "cancelling") {
            return;
        }
        let mut state = self.state.lock().unwrap();
        if let Some((root, prompt)) = state.active.remove(&job.request_id) {
            if let Some(response) = &job.response {
                let turns = state.histories.entry(root).or_default();
                turns.push(Turn {
                    prompt,
                    response: response.text.clone(),
                });
                if turns.len() > MAX_TURNS {
                    turns.remove(0);
                }
            }
        }
    }
    pub fn history(&self, root: &str) -> Vec<Turn> {
        self.state
            .lock()
            .unwrap()
            .histories
            .get(root)
            .cloned()
            .unwrap_or_default()
    }
    pub fn clear(&self, root: &str) {
        let mut s = self.state.lock().unwrap();
        s.histories.remove(root);
        s.active.retain(|_, (project, _)| project != root);
        s.preview = None;
    }
}
fn permitted(path: &str) -> bool {
    if path.is_empty() || path.contains('\\') || path.chars().any(char::is_control) {
        return false;
    }
    if !Path::new(path)
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return false;
    }
    for part in path.split('/') {
        let p = part.to_ascii_lowercase();
        if p.starts_with('.')
            || p.contains("secret")
            || p.contains("credential")
            || p.contains("token")
            || p.contains("password")
            || p.contains("private")
            || matches!(
                p.as_str(),
                "node_modules" | "target" | "vendor" | "id_rsa" | "id_ed25519"
            )
        {
            return false;
        }
    }
    matches!(
        Path::new(path)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "rs" | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "py"
            | "md"
            | "txt"
            | "css"
            | "html"
            | "c"
            | "h"
            | "cpp"
            | "go"
            | "java"
            | "sh"
    )
}
#[cfg(unix)]
fn read_selected(root: &Path, path: &str) -> Result<String, BridgeError> {
    use std::{
        ffi::CString,
        fs::File,
        os::fd::{AsRawFd, FromRawFd},
    };
    if !permitted(path) {
        return Err(error("context_path"));
    }
    // Hold directory descriptors and refuse symlinks at EVERY component. Canonicalize-then-read
    // would leave a path-swap race. Nonblocking open also avoids hanging on a FIFO.
    if !root.is_absolute() {
        return Err(error("context_path"));
    }
    let fd = unsafe {
        libc::open(
            c"/".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(error("context_path"));
    }
    let mut current = unsafe { File::from_raw_fd(fd) };
    let root_parts = root.components().filter_map(|c| match c {
        Component::Normal(p) => p.to_str(),
        _ => None,
    });
    let parts: Vec<_> = root_parts.chain(path.split('/')).collect();
    for (i, part) in parts.iter().enumerate() {
        let name = CString::new(*part).map_err(|_| error("context_path"))?;
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | libc::O_NONBLOCK
            | if i + 1 < parts.len() {
                libc::O_DIRECTORY
            } else {
                0
            };
        let fd = unsafe { libc::openat(current.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(error("context_path"));
        }
        current = unsafe { File::from_raw_fd(fd) };
    }
    let meta = current.metadata().map_err(|_| error("context_path"))?;
    // Hard links can alias files outside a project; reject them too.
    use std::os::unix::fs::MetadataExt;
    if !meta.is_file() || meta.nlink() != 1 {
        return Err(error("context_path"));
    }
    if meta.len() > MAX_FILE as u64 {
        return Err(error("context_limit"));
    }
    let mut bytes = Vec::new();
    current
        .take((MAX_FILE + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| error("context_path"))?;
    if bytes.len() > MAX_FILE {
        return Err(error("context_limit"));
    }
    let text = String::from_utf8(bytes).map_err(|_| error("context_path"))?;
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(error("context_path"));
    }
    // Defense in depth for obvious secrets in otherwise permitted source files.
    let lower = text.to_ascii_lowercase();
    if [
        "private key",
        "api_key",
        "api-key",
        "password=",
        "password =",
        "secret=",
        "secret =",
        "bearer ",
        "aws_access_key",
        "ghp_",
        "sk-",
    ]
    .iter()
    .any(|s| lower.contains(s))
    {
        return Err(error("context_path"));
    }
    Ok(text)
}
#[cfg(not(unix))]
fn read_selected(_: &Path, _: &str) -> Result<String, BridgeError> {
    Err(error("context_path"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::providers::ProposalResponse;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    static N: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "tako013-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(root.join("a/src")).unwrap();
            fs::create_dir_all(root.join("b")).unwrap();
            fs::write(root.join("a/src/main.rs"), "fn main() {}\n").unwrap();
            fs::write(root.join("a/other.md"), "not selected").unwrap();
            fs::write(root.join("b/other.md"), "other project").unwrap();
            Self(root)
        }
        fn root(&self) -> String {
            self.0.join("a").to_str().unwrap().into()
        }
        fn args(&self, paths: &[&str]) -> PreviewArgs {
            PreviewArgs {
                project_path: self.root(),
                paths: paths.iter().map(|p| (*p).into()).collect(),
                prompt: "Explain main".into(),
                include_history: false,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn start(p: &Preview) -> StartArgs {
        StartArgs {
            project_path: p.project_path.clone(),
            preview_id: p.preview_id,
            consent: true,
            provider: ProviderFamily::OpenAi,
            model: "fixture".into(),
            timeout_s: 1,
            max_output_tokens: 16,
        }
    }
    fn job(id: u64, state: &'static str) -> ProposalJob {
        ProposalJob {
            request_id: id,
            state,
            response: None,
            error: None,
        }
    }
    fn send_ok(_: ProposalRequest) -> Result<ProposalJob, BridgeError> {
        Ok(job(1, "pending"))
    }
    #[test]
    fn exact_selection_frozen_preview_and_explicit_native_consent() {
        let f = Fixture::new();
        let w = Workspace::default();
        let p = w.preview(&f.root(), f.args(&["src/main.rs"])).unwrap();
        assert!(p.text.contains("fn main()"));
        assert!(!p.text.contains("not selected"));
        assert!(!p.text.contains("other project"));
        assert_eq!(p.bytes, p.text.len());
        let mut a = start(&p);
        a.consent = false;
        assert_eq!(
            w.start_using(a, |_| panic!("must not send"))
                .err()
                .unwrap()
                .code,
            "context_consent"
        );
        let mut a = start(&p);
        a.project_path = f.0.join("b").display().to_string();
        assert_eq!(
            w.start_using(a, |_| panic!("must not send"))
                .err()
                .unwrap()
                .code,
            "context_consent"
        );
        fs::write(f.0.join("a/src/main.rs"), "changed after preview").unwrap();
        w.start_using(start(&p), |r| {
            assert_eq!(r.prompt, p.text);
            send_ok(r)
        })
        .unwrap();
        assert_eq!(
            w.start_using(start(&p), |_| panic!("replay"))
                .err()
                .unwrap()
                .code,
            "context_consent"
        );
        assert_eq!(
            fs::read_to_string(f.0.join("a/src/main.rs")).unwrap(),
            "changed after preview"
        );
    }
    #[test]
    fn traversal_absolute_sensitive_unsupported_and_binary_files_are_refused() {
        let f = Fixture::new();
        let w = Workspace::default();
        for path in [
            "../b/other.md",
            "/etc/passwd",
            "src/../../b/other.md",
            "src\\main.rs",
            ".env",
            ".git/config",
            "credentials.txt",
            "secret.md",
            "private/key.txt",
            "id_rsa",
            "data.json",
            "config.yaml",
            "node_modules/a.js",
            "README.md\nspoof",
        ] {
            assert!(w.preview(&f.root(), f.args(&[path])).is_err(), "{path}");
        }
        for (name, bytes) in [
            ("binary.txt", vec![0, 255]),
            ("key.rs", b"const API_KEY = redacted".to_vec()),
            ("control.md", vec![27]),
        ] {
            fs::write(f.0.join("a").join(&name), bytes).unwrap();
            assert!(w.preview(&f.root(), f.args(&[&name])).is_err());
        }
    }
    #[test]
    fn symlink_file_directory_root_ancestor_and_hardlink_escape_are_refused() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let w = Workspace::default();
        symlink(f.0.join("b/other.md"), f.0.join("a/link.md")).unwrap();
        symlink(f.0.join("b"), f.0.join("a/linkdir")).unwrap();
        symlink(f.0.join("a/src/main.rs"), f.0.join("a/inside.rs")).unwrap();
        fs::hard_link(f.0.join("b/other.md"), f.0.join("a/hard.md")).unwrap();
        for path in ["link.md", "linkdir/other.md", "inside.rs", "hard.md"] {
            assert!(w.preview(&f.root(), f.args(&[path])).is_err(), "{path}");
        }
        symlink(f.0.join("a"), f.0.join("alias")).unwrap();
        assert!(w
            .preview(
                &f.0.join("alias").display().to_string(),
                f.args(&["src/main.rs"])
            )
            .is_err());
        symlink(&f.0, f.0.with_extension("alias")).unwrap();
        assert!(w
            .preview(
                &f.0.with_extension("alias").join("a").display().to_string(),
                f.args(&["src/main.rs"])
            )
            .is_err());
        fs::remove_file(f.0.with_extension("alias")).unwrap();
    }
    #[test]
    fn per_file_total_count_utf8_and_history_limits_fail_closed() {
        let f = Fixture::new();
        let w = Workspace::default();
        fs::write(f.0.join("a/big.txt"), "x".repeat(MAX_FILE + 1)).unwrap();
        assert_eq!(
            w.preview(&f.root(), f.args(&["big.txt"]))
                .err()
                .unwrap()
                .code,
            "context_limit"
        );
        fs::write(f.0.join("a/exact.txt"), "x".repeat(MAX_FILE)).unwrap();
        assert!(w.preview(&f.root(), f.args(&["exact.txt"])).is_ok());
        w.state.lock().unwrap().histories.insert(
            f.root(),
            vec![Turn {
                prompt: "old question".into(),
                response: "x".repeat(MAX_INPUT),
            }],
        );
        let mut a = f.args(&[]);
        a.include_history = true;
        assert_eq!(w.preview(&f.root(), a).err().unwrap().code, "context_limit");
        assert!(w.preview(&f.root(), f.args(&[])).is_ok());
        for i in 0..4 {
            fs::write(f.0.join(format!("a/{i}.txt")), "é".repeat(MAX_FILE / 2)).unwrap();
        }
        assert_eq!(
            w.preview(&f.root(), f.args(&["0.txt", "1.txt", "2.txt", "3.txt"]))
                .err()
                .unwrap()
                .code,
            "context_limit"
        );
        let mut a = f.args(&[]);
        a.paths = vec!["src/main.rs".into(); 9];
        assert!(w.preview(&f.root(), a).is_err());
        let mut a = f.args(&[]);
        a.prompt = "é".repeat(MAX_INPUT / 2);
        assert!(w.preview(&f.root(), a).is_err());
        assert!(w
            .preview(&f.root(), f.args(&["src/main.rs", "src/main.rs"]))
            .is_err());
    }
    #[test]
    fn project_scoped_history_is_optional_bounded_clearable_and_errors_do_not_append() {
        let f = Fixture::new();
        let w = Workspace::default();
        for i in 0..10 {
            let p = w.preview(&f.root(), f.args(&[])).unwrap();
            w.start_using(start(&p), |_| Ok(job(i, "pending"))).unwrap();
            let mut j = job(i, "succeeded");
            j.response = Some(ProposalResponse {
                provider: ProviderFamily::OpenAi,
                model: "fixture".into(),
                text: format!("answer {i}"),
                usage: None,
                untrusted: true,
            });
            w.observe(&j);
            w.observe(&j);
        }
        assert_eq!(w.history(&f.root()).len(), 8);
        assert!(w.history(f.0.join("b").to_str().unwrap()).is_empty());
        let p = w.preview(&f.root(), f.args(&[])).unwrap();
        assert!(!p.text.contains("answer 9"));
        let mut a = f.args(&[]);
        a.include_history = true;
        let p = w.preview(&f.root(), a).unwrap();
        assert!(p.text.contains("answer 9"));
        assert!(!p.text.contains("answer 0"));
        w.start_using(start(&p), send_ok).unwrap();
        w.observe(&job(1, "cancelled"));
        assert_eq!(w.history(&f.root()).len(), 8);
        let p = w.preview(&f.root(), f.args(&[])).unwrap();
        assert_eq!(
            w.start_using(start(&p), |_| Err(BridgeError::new(
                "provider_error",
                "fixture"
            )))
            .err()
            .unwrap()
            .code,
            "provider_error"
        );
        assert_eq!(w.history(&f.root()).len(), 8);
        w.start_using(start(&p), send_ok).unwrap();
        w.observe(&job(1, "failed"));
        assert_eq!(w.history(&f.root()).len(), 8);
        w.clear(&f.root());
        assert!(w.history(&f.root()).is_empty());
        assert!(w
            .start_using(start(&p), |_| panic!("cleared preview"))
            .is_err());
    }
    #[test]
    fn replacing_preview_and_failed_preview_revoke_old_consent() {
        let f = Fixture::new();
        let w = Workspace::default();
        let old = w.preview(&f.root(), f.args(&[])).unwrap();
        let new = w.preview(&f.root(), f.args(&["src/main.rs"])).unwrap();
        assert!(w.start_using(start(&old), |_| panic!("stale")).is_err());
        assert!(w.preview(&f.root(), f.args(&["../b/other.md"])).is_err());
        assert!(w.start_using(start(&new), |_| panic!("revoked")).is_err());
    }
}
