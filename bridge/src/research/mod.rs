//! Independent local records only. Lazy access; no providers, tools or Research UI.
mod evidence;
mod json;
pub mod model;
pub mod planning;
mod state;
#[cfg(target_os = "linux")]
mod store;
use crate::BridgeError;
use serde_json::Value;
use std::path::Path;

pub(crate) fn error(reason: &str) -> BridgeError {
    let message = match reason {
        "invalid_input" => "Invalid local Research record fields or limits.",
        "not_found" => "The local Research record was not found.",
        "conflict" => "The Research project changed. Reload before saving.",
        "idempotency_conflict" => "This operation ID is already bound to different data.",
        "busy" => "Local Research records are busy. Retry explicitly.",
        "pending_operation" => "A local evidence operation is incomplete. Inspect its ID and retry the exact original request.",
        "missing_evidence" => "Recorded evidence content is missing. Metadata was retained; no content was reconstructed.",
        "integrity_mismatch" => "Local evidence content does not match its recorded digest or byte length.",
        "incomplete_creation" => "Project creation is incomplete. Retry its original operation.",
        "missing_project_data" => "Acknowledged Research project data is missing. Its identity and receipts were retained; no content was reconstructed.",
        "unsupported_schema" => "This Research record version is unsupported.",
        "unsafe_store" => "The local Research store cannot be safely accessed.",
        "storage_limit" => "The Research storage limit was reached. Existing records are retained.",
        "write_failed" => {
            "Research save was not acknowledged. Check the operation before retrying."
        }
        "unsupported_platform" => "Local Research storage is not qualified on this platform.",
        "review_authority_unavailable" => "No authenticated human-review authority is available at this native boundary. No approval or rejection was recorded.",
        _ => "Local Research records are unavailable. Existing data has been retained.",
    };
    BridgeError::new(&format!("research_{reason}"), message)
}
/// Raw stdio Research requests reject duplicate keys before conversion to Value.
/// Tauri's object arguments are separately validated by the same native commands.
pub fn validate_json(bytes: &[u8]) -> Result<Value, BridgeError> {
    json::decode(bytes)
}

pub fn dispatch(root: &Path, cmd: &str, args: Value) -> Result<Value, BridgeError> {
    #[cfg(target_os = "linux")]
    {
        store::dispatch(root, cmd, args)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (root, cmd, args);
        Err(error("unsupported_platform"))
    }
}
