//! Contract conformance fixtures are NOT real backend evidence.
use serde_json::{json, Value};
use tako_bridge::evidence_contract::*;

const EXAMPLE: &[u8] =
    include_bytes!("../../docs/contracts/backend-evidence/v1/conformance-example.json");
const SCHEMA: &str = include_str!("../../docs/contracts/backend-evidence/v1/response.schema.json");
fn example() -> Value {
    serde_json::from_slice(EXAMPLE).unwrap()
}
fn identity() -> Identity {
    serde_json::from_value(example()["identity"].clone()).unwrap()
}

// Only an explicitly approved exact fixture snapshot and artifact set are trusted.
// This models an external authenticator; it performs no real cryptography.
struct FixtureAuthority {
    bytes: Vec<u8>,
    artifacts: Vec<String>,
}
impl FixtureAuthority {
    fn for_value(v: &Value) -> Self {
        Self {
            bytes: serde_json::to_vec(v).unwrap(),
            artifacts: vec![
                "fixture-run".into(),
                "fixture-checks".into(),
                "fixture-isolation".into(),
            ],
        }
    }
}
impl Authority for FixtureAuthority {
    fn authentic_snapshot(&self, bytes: &[u8], _: &Response) -> bool {
        bytes == self.bytes
    }
    fn authentic_artifact(&self, a: &Artifact, _: &Response) -> bool {
        let expected: Response = serde_json::from_slice(&self.bytes).unwrap();
        self.artifacts.contains(&a.artifact_id)
            && expected
                .artifacts
                .iter()
                .any(|e| serde_json::to_value(e).unwrap() == serde_json::to_value(a).unwrap())
    }
}
fn evaluate(v: &Value) -> Result<Assessment, String> {
    assess(
        &serde_json::to_vec(v).unwrap(),
        &identity(),
        1500,
        &FixtureAuthority::for_value(v),
    )
}
fn disabled(a: &Assessment) {
    assert!(!a.remote_acceptance_enabled);
    assert!(!a.l1_badge_enabled);
    assert!(!a.provider_execution_enabled);
}
#[test]
fn exact_authoritative_claims_are_distinct_from_unsigned_json() {
    let v = example();
    let supported = evaluate(&v).unwrap();
    assert_eq!(supported.outcome, Outcome::Verified);
    assert_eq!(
        supported.isolation,
        SupportedIsolation::IndependentlySupportedL1
    );
    disabled(&supported);
    let unsigned = assess(
        &serde_json::to_vec(&v).unwrap(),
        &identity(),
        1500,
        &NoAuthority,
    )
    .unwrap();
    assert_eq!(unsigned.outcome, Outcome::Unverified);
    assert_eq!(unsigned.isolation, SupportedIsolation::Unverified);
    disabled(&unsigned);
}
#[test]
fn completion_and_legacy_labels_are_never_proof() {
    let mut v = example();
    v["verification"] = json!({"status":"unverified","evidence_id":null,"expected_checks":[],"checks":[],"subject_sha256":null});
    assert_eq!(evaluate(&v).unwrap().outcome, Outcome::Unverified);
    assert!(assess(
        br#"{"id":"fixture-mission","state":"completed","outcome_verification":"VERIFIED_PASS"}"#,
        &identity(),
        1500,
        &NoAuthority
    )
    .is_err());
}
#[test]
fn identity_is_bound_to_backend_project_and_mission_request() {
    for key in ["backend_id", "project_id", "mission_id"] {
        let mut v = example();
        v["identity"][key] = json!("another");
        assert_eq!(evaluate(&v).unwrap_err(), "identity_mismatch");
    }
}
#[test]
fn references_subjects_and_attempts_fail_closed() {
    for (pointer, value) in [
        ("/artifacts/1/mission_id", json!("other")),
        ("/artifacts/1/execution_id", json!("old-attempt")),
        ("/verification/subject_sha256", json!("e".repeat(64))),
        ("/execution/attempt", json!(0)),
        ("/execution/subject_sha256", json!("bad-digest")),
        ("/artifacts/1/sha256", json!("bad-digest")),
        ("/execution/exit_code", json!(1)),
        ("/execution/finished_at_ms", Value::Null),
        ("/verification/evidence_id", Value::Null),
    ] {
        let mut v = example();
        *v.pointer_mut(pointer).unwrap() = value;
        assert!(evaluate(&v).is_err(), "{pointer}");
    }
    for index in [0, 1] {
        let mut v = example();
        v["artifacts"].as_array_mut().unwrap().remove(index);
        assert_eq!(evaluate(&v).unwrap().outcome, Outcome::Incomplete);
    }
}
#[test]
fn false_or_partial_check_sets_never_pass() {
    for checks in [
        json!([]),
        json!([{"check_id":"unit","status":"skipped"}]),
        json!([{"check_id":"unit","status":"failed"}]),
        json!([{"check_id":"other","status":"passed"}]),
        json!([{"check_id":"unit","status":"passed"},{"check_id":"unit","status":"passed"}]),
    ] {
        let mut v = example();
        v["verification"]["checks"] = checks;
        assert!(evaluate(&v).is_err());
    }
    for checks in [json!([]), json!(["unit", "unit"])] {
        let mut v = example();
        v["verification"]["expected_checks"] = checks;
        assert!(evaluate(&v).is_err());
    }
}
#[test]
fn authoritative_failure_is_separate_from_unavailable_and_incomplete() {
    let mut v = example();
    v["verification"]["status"] = json!("failed");
    v["verification"]["checks"][0]["status"] = json!("failed");
    assert_eq!(evaluate(&v).unwrap().outcome, Outcome::Failed);
    let mut v = example();
    v["execution"] = Value::Null;
    assert_eq!(evaluate(&v).unwrap().outcome, Outcome::Incomplete);
    for (wire, outcome) in [
        ("unavailable", Outcome::Unavailable),
        ("not_found", Outcome::NotFound),
        ("incomplete", Outcome::Incomplete),
    ] {
        let mut v = example();
        v["availability"] = json!(wire);
        v["reason"] = json!("fixture source unavailable");
        v["execution"] = Value::Null;
        v["artifacts"] = json!([]);
        v["verification"] = json!({"status":"unverified","evidence_id":null,"expected_checks":[],"checks":[],"subject_sha256":null});
        v["isolation"] = json!({"claimed_level":"unknown","evidence_id":null});
        let a = evaluate(&v).unwrap();
        assert_eq!(a.outcome, outcome);
        disabled(&a);
        v["reason"] = Value::Null;
        assert!(evaluate(&v).is_err());
    }
}
#[test]
fn unsigned_artifacts_and_self_attestation_never_prove_verification_or_l1() {
    for id in ["fixture-run", "fixture-checks", "fixture-isolation"] {
        let v = example();
        let mut trust = FixtureAuthority::for_value(&v);
        trust.artifacts.retain(|a| a != id);
        let a = assess(&trust.bytes, &identity(), 1500, &trust).unwrap();
        if id != "fixture-isolation" {
            assert_ne!(a.outcome, Outcome::Verified);
        }
        assert_eq!(a.isolation, SupportedIsolation::Unverified);
        disabled(&a);
    }
    for issuer in ["fixture-runner", "fixture-backend"] {
        for index in [1, 2] {
            let mut v = example();
            v["artifacts"][index]["issuer_id"] = json!(issuer);
            let a = evaluate(&v).unwrap();
            if index == 1 {
                assert_eq!(a.outcome, Outcome::Unverified);
            }
            assert_eq!(a.isolation, SupportedIsolation::Unverified);
            disabled(&a);
        }
    }
}
#[test]
fn stale_future_and_replayed_snapshots_never_pass() {
    for now in [999, 2001, 400_000] {
        let v = example();
        let trust = FixtureAuthority::for_value(&v);
        assert_eq!(
            assess(&trust.bytes, &identity(), now, &trust)
                .unwrap()
                .outcome,
            Outcome::Unverified
        );
    }
    let original = example();
    let trust = FixtureAuthority::for_value(&original);
    for pointer in [
        "/revision",
        "/execution/execution_id",
        "/artifacts/1/sha256",
    ] {
        let mut v = original.clone();
        *v.pointer_mut(pointer).unwrap() = if pointer.ends_with("sha256") {
            json!("e".repeat(64))
        } else {
            json!("replayed")
        };
        assert!(
            assess(&serde_json::to_vec(&v).unwrap(), &identity(), 1500, &trust)
                .map(|a| a.outcome != Outcome::Verified)
                .unwrap_or(true)
        );
    }
}
#[test]
fn strict_version_enums_extra_fields_and_required_nulls() {
    for (pointer, bad) in [
        ("/schema_version", json!("v2")),
        ("/lifecycle", json!("accepted")),
        ("/verification/status", json!("verified")),
        ("/isolation/claimed_level", json!("L9")),
        ("/acceptance", json!("accepted")),
    ] {
        let mut v = example();
        *v.pointer_mut(pointer).unwrap() = bad;
        assert!(evaluate(&v).is_err());
    }
    let mut v = example();
    v["verified"] = json!(true);
    assert!(evaluate(&v).is_err());
    // Every property in every schema object is required, including explicitly nullable fields.
    let schema: Value = serde_json::from_str(SCHEMA).unwrap();
    for (definition, pointer) in [
        ("", ""),
        ("identity", "/identity"),
        ("execution", "/execution"),
        ("verification", "/verification"),
        ("isolation", "/isolation"),
        ("artifact", "/artifacts/0"),
        ("check", "/verification/checks/0"),
    ] {
        let shape = if definition.is_empty() {
            &schema
        } else {
            &schema["$defs"][definition]
        };
        assert_eq!(shape["additionalProperties"], false);
        assert_eq!(
            shape["properties"].as_object().unwrap().len(),
            shape["required"].as_array().unwrap().len()
        );
        for key in shape["required"].as_array().unwrap() {
            let mut v = example();
            v.pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key.as_str().unwrap());
            assert!(evaluate(&v).is_err(), "missing {pointer}/{key}");
        }
    }
    assert_eq!(schema["properties"]["schema_version"]["const"], VERSION);
}

#[test]
fn bounds_duplicate_keys_and_corrupt_provenance_are_rejected() {
    let mut v = example();
    v["artifacts"][1]["artifact_id"] = v["artifacts"][0]["artifact_id"].clone();
    assert!(evaluate(&v).is_err());
    let mut v = example();
    v["artifacts"][0]["kind"] = json!("verification");
    assert_eq!(evaluate(&v).unwrap().outcome, Outcome::Incomplete);
    let mut v = example();
    v["execution"]["runner_id"] = json!("unrelated-runner");
    assert_eq!(evaluate(&v).unwrap().outcome, Outcome::Unverified);
    let mut v = example();
    v["execution"]["status"] = json!("running");
    v["execution"]["exit_code"] = Value::Null;
    v["execution"]["finished_at_ms"] = Value::Null;
    assert!(
        evaluate(&v).is_err(),
        "verification cannot precede a successful execution"
    );
    let mut v = example();
    v["verification"]["expected_checks"] = json!(vec!["unit"; 1001]);
    assert_eq!(evaluate(&v).unwrap_err(), "too_many_records");
    let text = String::from_utf8(EXAMPLE.to_vec()).unwrap().replacen(
        "\"revision\":",
        "\"revision\":\"duplicate\",\"revision\":",
        1,
    );
    assert!(assess(text.as_bytes(), &identity(), 1500, &NoAuthority).is_err());
    assert!(assess(
        &vec![b' '; 4 * 1024 * 1024 + 1],
        &identity(),
        1500,
        &NoAuthority
    )
    .is_err());
}

#[test]
fn independent_isolation_support_is_not_inferred_from_verification() {
    let mut v = example();
    v["artifacts"].as_array_mut().unwrap().remove(2);
    let result = evaluate(&v).unwrap();
    assert_eq!(result.outcome, Outcome::Verified);
    assert_eq!(result.isolation, SupportedIsolation::Unverified);
    disabled(&result);
    let mut v = example();
    v["isolation"] = json!({"claimed_level":"l0_worktree","evidence_id":null});
    let result = evaluate(&v).unwrap();
    assert_eq!(result.isolation, SupportedIsolation::L0);
    disabled(&result);
}

#[test]
fn snapshot_maximum_age_and_attempt_changes_are_checked_independently() {
    let mut v = example();
    v["expires_at_ms"] = json!(900_000);
    let trust = FixtureAuthority::for_value(&v);
    assert_eq!(
        assess(&trust.bytes, &identity(), 301_001, &trust)
            .unwrap()
            .outcome,
        Outcome::Unverified
    );
    let original = example();
    let trust = FixtureAuthority::for_value(&original);
    let mut v = original.clone();
    v["execution"]["attempt"] = json!(2);
    assert_eq!(
        assess(&serde_json::to_vec(&v).unwrap(), &identity(), 1500, &trust)
            .unwrap()
            .outcome,
        Outcome::Unverified
    );
}

#[test]
fn execution_failure_requires_authenticated_failure_provenance() {
    let mut v = example();
    v["lifecycle"] = json!("failed");
    v["execution"]["status"] = json!("failed");
    v["execution"]["exit_code"] = json!(1);
    v["verification"] = json!({"status":"unverified","evidence_id":null,"expected_checks":[],"checks":[],"subject_sha256":null});
    assert_eq!(evaluate(&v).unwrap().outcome, Outcome::Failed);
    assert_eq!(
        assess(
            &serde_json::to_vec(&v).unwrap(),
            &identity(),
            1500,
            &NoAuthority
        )
        .unwrap()
        .outcome,
        Outcome::Unverified
    );
}

#[test]
fn authenticated_artifact_content_must_match_the_exact_snapshot_claim() {
    let response: Response = serde_json::from_slice(EXAMPLE).unwrap();
    for (index, kind) in [(0, "execution"), (1, "verification"), (2, "isolation")] {
        let claim = match kind {
            "execution" => json!({"kind":kind,"record":response.execution}),
            "verification" => json!({"kind":kind,"record":response.verification}),
            _ => json!({"kind":kind,"level":"l1_bubblewrap","policy_sha256":"e".repeat(64),
                "runtime_id":"fixture-runtime","filesystem_restricted":true,"network_namespace":true,"process_namespace":true}),
        };
        let doc = json!({"schema_version":"tako.backend-artifact.v1","identity":response.identity,
            "revision":response.revision,"execution_id":"fixture-execution-1","attempt":1,
            "subject_sha256":"a".repeat(64),"issuer_id":response.artifacts[index].issuer_id,"claim":claim});
        let validate = |v: &Value| {
            validate_artifact_claim(
                &serde_json::to_vec(v).unwrap(),
                &response,
                &response.artifacts[index],
            )
        };
        assert!(validate(&doc).is_ok());
        for (pointer, value) in [
            ("/identity/mission_id", json!("other")),
            ("/identity/project_id", json!("other")),
            ("/identity/backend_id", json!("other")),
            ("/revision", json!("old-revision")),
            ("/execution_id", json!("other-attempt")),
            ("/attempt", json!(2)),
            ("/subject_sha256", json!("f".repeat(64))),
            ("/issuer_id", json!("unauthorized")),
        ] {
            let mut changed = doc.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(validate(&changed).is_err(), "{kind}: {pointer}");
        }
        let mut changed = doc.clone();
        match kind {
            "execution" => changed["claim"]["record"]["exit_code"] = json!(9),
            "verification" => changed["claim"]["record"]["checks"][0]["status"] = json!("failed"),
            _ => changed["claim"]["network_namespace"] = json!(false),
        }
        assert!(validate(&changed).is_err());
    }
}
