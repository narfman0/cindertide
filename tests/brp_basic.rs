/// BRP integration tests — require a running Cindertide server.
///
/// To run these tests, first start the server in one terminal:
///   cargo run
///
/// Then in another terminal:
///   cargo test -- --include-ignored brp_
///
/// All tests in this file are marked `#[ignore]` so they are skipped
/// in normal `cargo test` runs (which have no server).

const URL: &str = "http://127.0.0.1:15703";

fn post(method: &str, params: serde_json::Value) -> serde_json::Value {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "id": 1,
        "params": params,
    });
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(URL)
        .json(&body)
        .send()
        .expect("failed to reach BRP server — is Cindertide running?");
    assert!(resp.status().is_success(), "expected HTTP 200, got {}", resp.status());
    resp.json().expect("response was not valid JSON")
}

/// Verify the BRP server responds to `bevy/list` and returns valid JSON.
#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_list_returns_valid_json() {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "bevy/list",
        "id": 1
    });

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(URL)
        .json(&body)
        .send()
        .expect("failed to reach BRP server — is Cindertide running?");

    assert!(
        resp.status().is_success(),
        "expected HTTP 200, got {}",
        resp.status()
    );

    let json: serde_json::Value = resp.json().expect("response was not valid JSON");
    assert!(
        json.get("result").is_some() || json.get("error").is_some(),
        "JSON-RPC response missing both 'result' and 'error' fields: {json}"
    );
}

/// Verify the `combat/attack` method is registered. Calling it with an
/// invalid attacker entity should produce an "entity not found" error
/// (-32602), not a "method not found" error (-32601).
#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_combat_attack_method_registered() {
    let json = post(
        "combat/attack",
        serde_json::json!({ "attacker_entity": 0u64, "target_entity": 0u64 }),
    );
    let err = json.get("error").expect("expected an error for invalid entity");
    let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
    assert_eq!(code, -32602, "expected entity-not-found, got: {json}");
}

/// Verify the `combat/status` method is registered the same way.
#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_combat_status_method_registered() {
    let json = post(
        "combat/status",
        serde_json::json!({ "entity": 0u64 }),
    );
    let err = json.get("error").expect("expected an error for invalid entity");
    let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
    assert_eq!(code, -32602, "expected entity-not-found, got: {json}");
}
