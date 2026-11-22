//! Ephemeral HTTP fixtures only; no production adapter/runtime connections.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;
use std::time::Duration;
use tako_bridge::{client::TakoBackendClient, evidence_contract::Outcome, Bridge};

const ROUTE: &str = "/api/v1/missions/fixture-mission/evidence?project_id=fixture-project";
fn example() -> Value {
    let mut v: Value = serde_json::from_str(include_str!(
        "../../docs/contracts/backend-evidence/v1/conformance-example.json"
    ))
    .unwrap();
    v["identity"]["backend_id"] = json!("t-tako-adapter");
    v
}
fn negative(availability: &str) -> Value {
    let mut v = example();
    v["availability"] = json!(availability);
    v["reason"] = json!("fixture missing source");
    v["execution"] = Value::Null;
    v["artifacts"] = json!([]);
    v["lifecycle"] = json!("unknown");
    v["verification"] = json!({"status":"unverified","evidence_id":null,"expected_checks":[],"checks":[],"subject_sha256":null});
    v["isolation"] = json!({"claimed_level":"unknown","evidence_id":null});
    v
}
fn fixture(code: u16, body: Vec<u8>, route: &str) -> (TakoBackendClient, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client =
        TakoBackendClient::unauthenticated("127.0.0.1", listener.local_addr().unwrap().port());
    let route = route.to_string();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut req = Vec::new();
        let mut part = [0; 1024];
        while !req.windows(4).any(|s| s == b"\r\n\r\n") {
            let n = stream.read(&mut part).unwrap();
            if n == 0 {
                break;
            }
            req.extend_from_slice(&part[..n]);
        }
        let request = String::from_utf8(req).unwrap();
        assert_eq!(request.split_whitespace().nth(1), Some(route.as_str()));
        assert!(
            !request.contains("Authorization:"),
            "fixture must not transmit host credentials"
        );
        write!(stream,"HTTP/1.1 {code} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).unwrap();
        stream.write_all(&body).unwrap();
    });
    (client, server)
}
fn retrieve(
    code: u16,
    body: Value,
) -> Result<tako_bridge::evidence_retrieval::RetrievedEvidence, tako_bridge::BridgeError> {
    let (client, server) = fixture(code, serde_json::to_vec(&body).unwrap(), ROUTE);
    let result = client.get_mission_evidence("fixture-project", "fixture-mission", 1500);
    server.join().unwrap();
    result
}
#[test]
fn retrieved_complete_export_is_not_independently_authenticated() {
    let view = retrieve(200, example()).unwrap();
    assert_eq!(view.assessment.outcome, Outcome::Unverified);
    assert!(!view.assessment.l1_badge_enabled);
    assert!(!view.assessment.remote_acceptance_enabled);
    assert!(!view.assessment.provider_execution_enabled);
    assert!(view
        .message
        .contains("no independently established authenticity"));
}
#[test]
fn not_found_incomplete_and_unavailable_are_explicit() {
    for (code, status, outcome) in [
        (404, "not_found", Outcome::NotFound),
        (503, "unavailable", Outcome::Unavailable),
        (200, "incomplete", Outcome::Incomplete),
    ] {
        assert_eq!(
            retrieve(code, negative(status)).unwrap().assessment.outcome,
            outcome
        );
    }
}
#[test]
fn mismatched_identity_corrupt_references_and_inconsistent_status_are_rejected() {
    for pointer in [
        "/identity/backend_id",
        "/identity/project_id",
        "/identity/mission_id",
        "/artifacts/1/execution_id",
        "/artifacts/1/sha256",
    ] {
        let mut v = example();
        *v.pointer_mut(pointer).unwrap() = json!("wrong");
        assert_eq!(
            retrieve(200, v).unwrap_err().code,
            "evidence_invalid",
            "{pointer}"
        );
    }
    assert_eq!(
        retrieve(200, negative("unavailable")).unwrap_err().code,
        "evidence_invalid"
    );
    assert_eq!(
        retrieve(503, example()).unwrap_err().code,
        "evidence_invalid"
    );
}
#[test]
fn unsupported_unauthorized_malformed_and_duplicate_key_responses_never_succeed() {
    assert_eq!(
        retrieve(404, json!({"error":"unknown route"}))
            .unwrap_err()
            .code,
        "evidence_unavailable"
    );
    for code in [401, 403, 500] {
        assert_eq!(
            retrieve(code, json!({"error":"fixture refusal"}))
                .unwrap_err()
                .code,
            "evidence_unavailable"
        );
    }
    for body in [
        b"not json".to_vec(),
        serde_json::to_string(&example())
            .unwrap()
            .replacen(
                "\"revision\":",
                "\"revision\":\"duplicate\",\"revision\":",
                1,
            )
            .into_bytes(),
    ] {
        let (client, server) = fixture(200, body, ROUTE);
        assert_eq!(
            client
                .get_mission_evidence("fixture-project", "fixture-mission", 1500)
                .unwrap_err()
                .code,
            "evidence_invalid"
        );
        server.join().unwrap();
    }
}
#[test]
fn request_components_are_encoded_and_invalid_inputs_do_not_connect() {
    let route = "/api/v1/missions/m%2F%3F%23%20%C3%A9/evidence?project_id=p%26admin%3Dtrue";
    let (client, server) = fixture(404, b"{}".to_vec(), route);
    assert_eq!(
        client
            .get_mission_evidence("p&admin=true", "m/?# é", 1500)
            .unwrap_err()
            .code,
        "evidence_unavailable"
    );
    server.join().unwrap();
    let client = TakoBackendClient::unauthenticated("127.0.0.1", 0);
    for bad in ["", "\r\nGET /other"] {
        assert_eq!(
            client
                .get_mission_evidence("fixture-project", bad, 1500)
                .unwrap_err()
                .code,
            "invalid_input"
        );
    }
    assert_eq!(
        client
            .get_mission_evidence("fixture-project", "fixture-mission", 1500)
            .unwrap_err()
            .code,
        "backend_unavailable"
    );
}
#[test]
fn desktop_dispatch_uses_the_retrieval_contract_without_a_local_mission_fallback() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let mut v = negative("unavailable");
    v["observed_at_ms"] = json!(now);
    v["expires_at_ms"] = json!(now + 60_000);
    let (client, server) = fixture(503, serde_json::to_vec(&v).unwrap(), ROUTE);
    let root = std::env::temp_dir().join(format!("tako-retrieval-dispatch-{}", std::process::id()));
    let bridge = Bridge::open_with_client(&root, client).unwrap();
    let view = bridge
        .dispatch(
            "backend_mission_evidence",
            json!({"project_id":"fixture-project","mission_id":"fixture-mission"}),
        )
        .unwrap();
    assert_eq!(view["assessment"]["outcome"], "unavailable");
    assert!(view.get("mission").is_none());
    assert!(bridge
        .dispatch("get_mission", json!({"mission_id":"fixture-mission"}))
        .is_err());
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn optional_discovery_uses_opaque_encoded_project_ids() {
    let project_id = "p/?# é";
    let (client, server) = fixture(
        200,
        serde_json::to_vec(&json!({"project_id":project_id,"goals":[],"total":0})).unwrap(),
        "/api/v1/projects/p%2F%3F%23%20%C3%A9/goals",
    );
    assert_eq!(
        client.get_project_goals(project_id).unwrap().project_id,
        project_id
    );
    server.join().unwrap();
}
