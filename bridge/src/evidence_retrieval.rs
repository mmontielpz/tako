//! Read-only retrieval. Exact bytes are parsed under v1; no production authenticator
//! is installed, so an available export remains explicitly unverified.
use crate::{client::TakoBackendClient, evidence_contract::*, BridgeError};
use serde::Serialize;

pub const BACKEND_ID: &str = "t-tako-adapter";
#[derive(Debug, Serialize)]
pub struct RetrievedEvidence {
    pub identity: Identity,
    pub assessment: Assessment,
    pub message: String,
}
fn component(s: &str) -> Result<String, BridgeError> {
    crate::client::encode_component(s).map_err(|e| BridgeError::new("invalid_input", e))
}

impl TakoBackendClient {
    pub fn get_mission_evidence(
        &self,
        project_id: &str,
        mission_id: &str,
        now_ms: u64,
    ) -> Result<RetrievedEvidence, BridgeError> {
        let expected = Identity {
            backend_id: BACKEND_ID.into(),
            project_id: project_id.into(),
            mission_id: mission_id.into(),
        };
        let path = format!(
            "/api/v1/missions/{}/evidence?project_id={}",
            component(mission_id)?,
            component(project_id)?
        );
        let (code, bytes) = self.raw_get_bytes(&path, true)
            .map_err(|_| BridgeError::new("backend_unavailable", "Authoritative evidence unavailable: adapter could not be reached or its response could not be read."))?;
        if ![200, 404, 503].contains(&code) {
            return Err(BridgeError::new(
                "evidence_unavailable",
                format!("Evidence request refused by adapter (HTTP {code})."),
            ));
        }
        // Assess the original bytes, not a serde_json::Value reserialization. This
        // preserves duplicate-key rejection and the future exact-byte trust boundary.
        let assessment = assess(&bytes, &expected, now_ms, &NoAuthority).map_err(|e| {
            BridgeError::new(
                if code == 404 {
                    "evidence_unavailable"
                } else {
                    "evidence_invalid"
                },
                format!("Authoritative evidence unavailable: {e}"),
            )
        })?;
        let response: Response = serde_json::from_slice(&bytes)
            .map_err(|_| BridgeError::new("evidence_invalid", "Invalid evidence contract."))?;
        let status_matches = matches!(
            (code, &response.availability),
            (200, Availability::Available | Availability::Incomplete)
                | (404, Availability::NotFound)
                | (503, Availability::Unavailable)
        );
        if !status_matches {
            return Err(BridgeError::new(
                "evidence_invalid",
                "HTTP status and evidence availability disagree.",
            ));
        }
        let message = match &assessment.outcome {
            Outcome::Unverified => "Authoritative evidence unavailable: the export has no independently established authenticity.".into(),
            Outcome::Incomplete => "Authoritative evidence incomplete: required records or independent authentication are unavailable.".into(),
            Outcome::NotFound => "The evidence source reports no matching mission.".into(),
            Outcome::Unavailable => "Authoritative backend evidence is unavailable.".into(),
            _ => return Err(BridgeError::new("evidence_unverified", "Production evidence authority is not installed.")),
        };
        Ok(RetrievedEvidence {
            identity: expected,
            assessment,
            message,
        })
    }
}
