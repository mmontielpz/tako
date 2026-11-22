//! Read-only backend evidence v1. No network calls or mutation authority.
//! Wire assertions never authenticate themselves; NoAuthority always refuses trust.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const VERSION: &str = "tako.backend-evidence.v1";
pub const MAX_SNAPSHOT_AGE_MS: u64 = 300_000;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub backend_id: String,
    pub project_id: String,
    pub mission_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Incomplete,
    Unavailable,
    NotFound,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Unknown,
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Unverified,
    Pending,
    Failed,
    Passed,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Passed,
    Failed,
    Skipped,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Execution,
    Verification,
    Isolation,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationLevel {
    Unknown,
    L0Worktree,
    L1Bubblewrap,
}

// Explicit null is required for nullable wire fields; omission is not a default.
fn nullable<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d)
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    pub execution_id: String,
    pub attempt: u32,
    pub runner_id: String,
    pub status: ExecutionStatus,
    pub started_at_ms: u64,
    #[serde(deserialize_with = "nullable")]
    pub finished_at_ms: Option<u64>,
    #[serde(deserialize_with = "nullable")]
    pub exit_code: Option<i32>,
    pub subject_sha256: String,
    pub provenance_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub artifact_id: String,
    pub mission_id: String,
    pub execution_id: String,
    pub kind: ArtifactKind,
    pub sha256: String,
    pub issuer_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub check_id: String,
    pub status: CheckStatus,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Verification {
    pub status: VerificationStatus,
    #[serde(deserialize_with = "nullable")]
    pub evidence_id: Option<String>,
    pub expected_checks: Vec<String>,
    pub checks: Vec<Check>,
    #[serde(deserialize_with = "nullable")]
    pub subject_sha256: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Isolation {
    pub claimed_level: IsolationLevel,
    #[serde(deserialize_with = "nullable")]
    pub evidence_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub schema_version: String,
    pub identity: Identity,
    pub revision: String,
    pub observed_at_ms: u64,
    pub expires_at_ms: u64,
    pub availability: Availability,
    #[serde(deserialize_with = "nullable")]
    pub reason: Option<String>,
    pub lifecycle: Lifecycle,
    #[serde(deserialize_with = "nullable")]
    pub execution: Option<Execution>,
    pub artifacts: Vec<Artifact>,
    pub verification: Verification,
    pub isolation: Isolation,
    /// v1 has no remote acceptance protocol. Only "not_supported" is valid.
    pub acceptance: String,
}

/// Implementations must use trust roots configured outside the wire response.
/// Authenticate exact snapshot bytes, identity, revision and freshness; reject replay.
/// Artifact verification must retrieve immutable bytes, check SHA-256, authenticate
/// a role-authorized issuer, and compare the artifact's complete claim with `response`
/// (identity, revision, execution/attempt, subject, checks/results or isolation policy).
/// Implementations must also call validate_artifact_claim on the authenticated bytes.
/// Network reachability, a matching digest string, or a boolean in JSON is insufficient.
/// No production implementation or trust roots are installed by TAKO-007A.
pub trait Authority {
    fn authentic_snapshot(&self, bytes: &[u8], response: &Response) -> bool;
    fn authentic_artifact(&self, artifact: &Artifact, response: &Response) -> bool;
}
pub struct NoAuthority;
impl Authority for NoAuthority {
    fn authentic_snapshot(&self, _: &[u8], _: &Response) -> bool {
        false
    }
    fn authentic_artifact(&self, _: &Artifact, _: &Response) -> bool {
        false
    }
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Unavailable,
    NotFound,
    Incomplete,
    Unverified,
    Failed,
    Verified,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportedIsolation {
    Unverified,
    L0,
    IndependentlySupportedL1,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Assessment {
    pub outcome: Outcome,
    pub isolation: SupportedIsolation,
    pub remote_acceptance_enabled: bool,
    pub l1_badge_enabled: bool,
    pub provider_execution_enabled: bool,
    pub reason: &'static str,
}
fn assessment(outcome: Outcome, reason: &'static str) -> Assessment {
    Assessment {
        outcome,
        isolation: SupportedIsolation::Unverified,
        remote_acceptance_enabled: false,
        l1_badge_enabled: false,
        provider_execution_enabled: false,
        reason,
    }
}
fn nonempty(s: &str) -> bool {
    !s.trim().is_empty() && s.chars().count() <= 256
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn artifact<'a>(r: &'a Response, id: &str, kind: ArtifactKind) -> Option<&'a Artifact> {
    r.artifacts
        .iter()
        .find(|a| a.artifact_id == id && a.kind == kind)
}

/// Strict wire/semantic validation, then independently supplied authority checks.
/// Err means malformed/inconsistent contract, never a usable successful result.
/// Valid but unsupported/partial snapshots yield explicit non-success assessments.
pub fn assess(
    bytes: &[u8],
    expected: &Identity,
    now_ms: u64,
    authority: &impl Authority,
) -> Result<Assessment, String> {
    if bytes.len() > crate::client::MAX_RESPONSE_BYTES {
        return Err("response_too_large".into());
    }
    let r: Response =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid_contract: {e}"))?;
    if r.artifacts.len() > 1000
        || r.verification.expected_checks.len() > 1000
        || r.verification.checks.len() > 1000
    {
        return Err("too_many_records".into());
    }
    if r.schema_version != VERSION {
        return Err("unsupported_version".into());
    }
    if r.identity != *expected {
        return Err("identity_mismatch".into());
    }
    if ![
        &r.identity.backend_id,
        &r.identity.project_id,
        &r.identity.mission_id,
        &r.revision,
    ]
    .into_iter()
    .all(|s| nonempty(s))
        || r.acceptance != "not_supported"
    {
        return Err("invalid_identity_or_acceptance".into());
    }
    if r.observed_at_ms > now_ms
        || r.expires_at_ms < now_ms
        || r.expires_at_ms < r.observed_at_ms
        || now_ms - r.observed_at_ms > MAX_SNAPSHOT_AGE_MS
    {
        return Ok(assessment(Outcome::Unverified, "stale_or_future_snapshot"));
    }
    if r.availability != Availability::Available {
        if !r.reason.as_ref().map(|s| nonempty(s)).unwrap_or(false)
            || r.execution.is_some()
            || !r.artifacts.is_empty()
            || r.verification.status != VerificationStatus::Unverified
            || r.verification.evidence_id.is_some()
            || r.verification.subject_sha256.is_some()
            || !r.verification.expected_checks.is_empty()
            || !r.verification.checks.is_empty()
            || r.isolation.claimed_level != IsolationLevel::Unknown
            || r.isolation.evidence_id.is_some()
        {
            return Err("nonavailable_response_contains_claims_or_no_reason".into());
        }
        return Ok(assessment(
            match r.availability {
                Availability::NotFound => Outcome::NotFound,
                Availability::Unavailable => Outcome::Unavailable,
                _ => Outcome::Incomplete,
            },
            "backend_has_no_complete_evidence",
        ));
    }
    if r.reason.is_some() {
        return Err("available_response_has_failure_reason".into());
    }
    let Some(x) = &r.execution else {
        return Ok(assessment(Outcome::Incomplete, "missing_execution"));
    };
    if ![&x.execution_id, &x.runner_id, &x.provenance_id]
        .into_iter()
        .all(|s| nonempty(s))
        || x.attempt == 0
        || !digest(&x.subject_sha256)
        || x.started_at_ms > r.observed_at_ms
        || x.finished_at_ms
            .map(|t| t < x.started_at_ms || t > r.observed_at_ms)
            .unwrap_or(false)
    {
        return Err("invalid_execution_provenance".into());
    }
    let mut ids = HashSet::new();
    for a in &r.artifacts {
        if !nonempty(&a.artifact_id)
            || !nonempty(&a.issuer_id)
            || !digest(&a.sha256)
            || !ids.insert(&a.artifact_id)
            || a.mission_id != r.identity.mission_id
            || a.execution_id != x.execution_id
        {
            return Err("invalid_artifact_binding".into());
        }
    }
    let expected_checks: HashSet<_> = r.verification.expected_checks.iter().collect();
    let check_ids: HashSet<_> = r.verification.checks.iter().map(|c| &c.check_id).collect();
    if expected_checks.len() != r.verification.expected_checks.len()
        || check_ids.len() != r.verification.checks.len()
        || !expected_checks.iter().all(|s| nonempty(s))
        || !check_ids.iter().all(|s| nonempty(s))
        || !check_ids.is_subset(&expected_checks)
    {
        return Err("invalid_check_set".into());
    }
    let proven_result = matches!(
        r.verification.status,
        VerificationStatus::Passed | VerificationStatus::Failed
    );
    if proven_result
        && (x.status != ExecutionStatus::Succeeded
            || r.verification.subject_sha256.as_deref() != Some(&x.subject_sha256)
            || r.verification.evidence_id.is_none())
    {
        return Err("verification_binding_missing".into());
    }
    if !proven_result
        && (r.verification.evidence_id.is_some()
            || r.verification.subject_sha256.is_some()
            || !r.verification.checks.is_empty())
    {
        return Err("unverified_contains_results".into());
    }
    if matches!(x.status, ExecutionStatus::Succeeded)
        && (x.exit_code != Some(0) || x.finished_at_ms.is_none())
    {
        return Err("execution_success_inconsistent".into());
    }
    if x.status == ExecutionStatus::Running && (x.finished_at_ms.is_some() || x.exit_code.is_some())
    {
        return Err("running_execution_has_result".into());
    }
    if r.verification.status == VerificationStatus::Passed
        && (r.lifecycle != Lifecycle::Completed
            || x.status != ExecutionStatus::Succeeded
            || expected_checks.is_empty()
            || check_ids != expected_checks
            || r.verification
                .checks
                .iter()
                .any(|c| c.status != CheckStatus::Passed))
    {
        return Err("verification_pass_inconsistent".into());
    }
    if r.verification.status == VerificationStatus::Failed
        && !r
            .verification
            .checks
            .iter()
            .any(|c| c.status == CheckStatus::Failed)
    {
        return Err("verification_failure_without_failed_check".into());
    }
    if !authority.authentic_snapshot(bytes, &r) {
        return Ok(assessment(
            Outcome::Unverified,
            "snapshot_not_authenticated",
        ));
    }
    let Some(run) = artifact(&r, &x.provenance_id, ArtifactKind::Execution) else {
        return Ok(assessment(
            Outcome::Incomplete,
            "execution_provenance_missing",
        ));
    };
    if run.issuer_id != x.runner_id || !authority.authentic_artifact(run, &r) {
        return Ok(assessment(
            Outcome::Unverified,
            "execution_provenance_not_authenticated",
        ));
    }
    let mut result = assessment(Outcome::Unverified, "completion_is_not_verification");
    if proven_result {
        let Some(ev) = artifact(
            &r,
            r.verification.evidence_id.as_ref().unwrap(),
            ArtifactKind::Verification,
        ) else {
            return Ok(assessment(
                Outcome::Incomplete,
                "verification_artifact_missing",
            ));
        };
        if ev.issuer_id == x.runner_id
            || ev.issuer_id == r.identity.backend_id
            || !authority.authentic_artifact(ev, &r)
        {
            return Ok(assessment(
                Outcome::Unverified,
                "verification_not_independently_authenticated",
            ));
        }
        result = assessment(
            if r.verification.status == VerificationStatus::Passed {
                Outcome::Verified
            } else {
                Outcome::Failed
            },
            "authenticated_verification",
        );
    } else if r.lifecycle == Lifecycle::Failed
        && x.status == ExecutionStatus::Failed
        && x.finished_at_ms.is_some()
        && x.exit_code.map(|c| c != 0).unwrap_or(false)
    {
        result = assessment(Outcome::Failed, "authenticated_execution_failure");
    }
    match r.isolation.claimed_level {
        IsolationLevel::Unknown => {}
        IsolationLevel::L0Worktree => {
            result.isolation = SupportedIsolation::L0;
        }
        IsolationLevel::L1Bubblewrap => {
            if let Some(iso) = r
                .isolation
                .evidence_id
                .as_ref()
                .and_then(|id| artifact(&r, id, ArtifactKind::Isolation))
            {
                if iso.issuer_id != x.runner_id
                    && iso.issuer_id != r.identity.backend_id
                    && authority.authentic_artifact(iso, &r)
                {
                    result.isolation = SupportedIsolation::IndependentlySupportedL1;
                }
            }
        }
    }
    Ok(result)
}

/// Authenticated artifact bytes must also pass this claim-binding check.
/// This helper does not verify SHA-256, signatures or issuer authorization.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactDocument {
    pub schema_version: String,
    pub identity: Identity,
    pub revision: String,
    pub execution_id: String,
    pub attempt: u32,
    pub subject_sha256: String,
    pub issuer_id: String,
    pub claim: ArtifactClaim,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactClaim {
    Execution {
        record: Execution,
    },
    Verification {
        record: Verification,
    },
    Isolation {
        level: IsolationLevel,
        policy_sha256: String,
        runtime_id: String,
        filesystem_restricted: bool,
        network_namespace: bool,
        process_namespace: bool,
    },
}

pub fn validate_artifact_claim(
    bytes: &[u8],
    response: &Response,
    reference: &Artifact,
) -> Result<ArtifactDocument, String> {
    if bytes.len() > crate::client::MAX_RESPONSE_BYTES {
        return Err("artifact_too_large".into());
    }
    let doc: ArtifactDocument =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid_artifact: {e}"))?;
    let x = response
        .execution
        .as_ref()
        .ok_or("artifact_without_execution")?;
    if doc.schema_version != "tako.backend-artifact.v1"
        || doc.identity != response.identity
        || doc.revision != response.revision
        || doc.execution_id != x.execution_id
        || doc.attempt != x.attempt
        || doc.subject_sha256 != x.subject_sha256
        || doc.issuer_id != reference.issuer_id
        || reference.mission_id != response.identity.mission_id
        || reference.execution_id != x.execution_id
    {
        return Err("artifact_claim_binding_mismatch".into());
    }
    let matches = match &doc.claim {
        ArtifactClaim::Execution { record } => {
            reference.kind == ArtifactKind::Execution && record == x
        }
        ArtifactClaim::Verification { record } => {
            reference.kind == ArtifactKind::Verification && record == &response.verification
        }
        ArtifactClaim::Isolation {
            level,
            policy_sha256,
            runtime_id,
            filesystem_restricted,
            network_namespace,
            process_namespace,
        } => {
            reference.kind == ArtifactKind::Isolation
                && *level == IsolationLevel::L1Bubblewrap
                && response.isolation.claimed_level == *level
                && digest(policy_sha256)
                && nonempty(runtime_id)
                && *filesystem_restricted
                && *network_namespace
                && *process_namespace
        }
    };
    if !matches {
        return Err("artifact_claim_content_mismatch".into());
    }
    Ok(doc)
}
