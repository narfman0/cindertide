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

/// Verify the BRP server responds to `bevy/list` and returns valid JSON.
#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_list_returns_valid_json() {
    let url = "http://127.0.0.1:15703";
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "bevy/list",
        "id": 1
    });

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(url)
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
