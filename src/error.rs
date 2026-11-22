use std::fmt;

#[derive(Debug)]
pub enum EngineError {
    Io(String),
    Json(String),
    Git(String),
    InvalidInput(String),
    NotFound(String),
    /// A decision that requires a human was attempted by a non-human actor.
    NotHuman,
    /// Operation needs a different mission state.
    WrongState {
        expected: String,
        actual: String,
    },
    /// Execution attempted without a valid approval.
    NotApproved(String),
    /// The requested action is outside the granted capabilities.
    Unauthorized(String),
    /// A capability the engine cannot enforce was requested (fails closed).
    Unenforceable(String),
    PlanHashMismatch,
    DirtyProject,
    Conflict(String),
    AlreadyDecided,
    EvidenceNotAcceptable(String),
    /// Another operation holds the mission lock.
    Busy,
    /// The operation was cancelled by request.
    Cancelled,
    /// Repository configuration could execute programs the engine cannot constrain.
    UnsafeRepository(Vec<String>),
    /// The requested execution needs a sandbox level that is not implemented.
    IsolationRequired(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use EngineError::*;
        match self {
            Io(m) => write!(f, "io error: {m}"),
            Json(m) => write!(f, "json error: {m}"),
            Git(m) => write!(f, "git error: {m}"),
            InvalidInput(m) => write!(f, "invalid input: {m}"),
            NotFound(m) => write!(f, "not found: {m}"),
            NotHuman => write!(f, "this decision requires a human actor"),
            WrongState { expected, actual } => {
                write!(
                    f,
                    "wrong mission state: expected {expected}, found {actual}"
                )
            }
            NotApproved(m) => write!(f, "not approved: {m}"),
            Unauthorized(m) => write!(f, "unauthorized: {m}"),
            Unenforceable(m) => write!(f, "unenforceable capability refused: {m}"),
            PlanHashMismatch => write!(f, "plan content does not match the approved hash"),
            DirtyProject => write!(f, "project working tree has uncommitted changes"),
            Conflict(m) => write!(f, "merge conflict, project left unchanged: {m}"),
            AlreadyDecided => write!(f, "mission already has a final decision"),
            EvidenceNotAcceptable(m) => write!(f, "evidence not acceptable: {m}"),
            Busy => write!(f, "mission is busy: another operation holds its lock"),
            Cancelled => write!(f, "operation cancelled"),
            UnsafeRepository(v) => write!(
                f,
                "unsafe repository configuration refused: {}",
                v.join(", ")
            ),
            IsolationRequired(m) => write!(f, "isolation required: {m}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<std::io::Error> for EngineError {
    fn from(e: std::io::Error) -> Self {
        EngineError::Io(e.to_string())
    }
}
impl From<serde_json::Error> for EngineError {
    fn from(e: serde_json::Error) -> Self {
        EngineError::Json(e.to_string())
    }
}
