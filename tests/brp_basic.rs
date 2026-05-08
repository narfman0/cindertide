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

/// Stateful tests must call this first. Acquires a process-wide mutex
/// (so parallel cargo-test threads don't race against the same server)
/// and resets the world via the dev/reset BRP method.
fn lock_world() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = post("dev/reset", serde_json::json!({}));
    g
}

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

/// Spawn a rifleman via BRP, then check unit/status returns health_current == 100.0.
#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_unit_spawn_and_status() {
    let _g = lock_world();
    // Step 1: spawn rifleman at (0, 0)
    let spawn_resp = post(
        "unit/spawn",
        serde_json::json!({ "unit_type": "rifleman", "x": 0, "y": 0 }),
    );
    let result = spawn_resp.get("result").expect("expected result from unit/spawn");
    let entity_id = result["entity_id"]
        .as_u64()
        .expect("entity_id should be a u64");

    // Step 2: query unit/status
    let status_resp = post(
        "unit/status",
        serde_json::json!({ "entity": entity_id }),
    );
    let status = status_resp.get("result").expect("expected result from unit/status");
    let health_current = status["health_current"]
        .as_f64()
        .expect("health_current should be a float");
    assert_eq!(health_current, 100.0, "rifleman health_current should be 100.0 at spawn");
}

fn spawn_rifleman(x: i32, y: i32) -> u64 {
    let resp = post(
        "unit/spawn",
        serde_json::json!({ "unit_type": "rifleman", "x": x, "y": y }),
    );
    resp["result"]["entity_id"]
        .as_u64()
        .expect("entity_id u64")
}

fn unit_status(entity: u64) -> serde_json::Value {
    let resp = post("unit/status", serde_json::json!({ "entity": entity }));
    resp["result"].clone()
}

fn combat_status(entity: u64) -> serde_json::Value {
    let resp = post("combat/status", serde_json::json!({ "entity": entity }));
    resp["result"].clone()
}

/// End-to-end: two riflemen, one orders an attack against the other; after a
/// short wait the target's health drops, suppression accumulates, and after
/// enough fire the target becomes pinned.
#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_combat_live_fire_drops_health_and_accumulates_suppression() {
    let _g = lock_world();
    let attacker = spawn_rifleman(0, 0);
    let target = spawn_rifleman(2, 0);

    let before = combat_status(target);
    assert_eq!(before["health_current"].as_f64().unwrap(), 100.0);
    assert_eq!(before["suppression_current"].as_f64().unwrap(), 0.0);

    let attack = post(
        "combat/attack",
        serde_json::json!({ "attacker_entity": attacker, "target_entity": target }),
    );
    assert!(
        attack.get("result").is_some(),
        "combat/attack should succeed: {attack}"
    );

    // Let combat run for a couple of seconds — attacker fires every 1s.
    std::thread::sleep(std::time::Duration::from_millis(2200));

    let after = combat_status(target);
    let health = after["health_current"].as_f64().unwrap();
    let supp = after["suppression_current"].as_f64().unwrap();
    assert!(health < 100.0, "expected health to drop, got {health}; full: {after}");
    assert!(supp > 0.0, "expected suppression to accumulate, got {supp}; full: {after}");
}

/// Verify spawn returns a usable entity id and unit/status reflects facing
/// (default North) and the morale system reports Steady at 100/100.
#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_unit_status_includes_facing_and_morale() {
    let _g = lock_world();
    let id = spawn_rifleman(5, 5);
    let s = unit_status(id);
    assert_eq!(s["facing"].as_str().unwrap(), "North");
    assert_eq!(s["morale_state"].as_str().unwrap(), "Steady");
    assert_eq!(s["pos_x"].as_i64().unwrap(), 5);
    assert_eq!(s["pos_y"].as_i64().unwrap(), 5);
}

fn spawn_faction(name: &str) -> u64 {
    let resp = post("faction/spawn", serde_json::json!({ "faction": name }));
    resp["result"]["entity_id"].as_u64().expect("entity_id u64")
}

fn resources_status(entity: u64) -> serde_json::Value {
    let resp = post("resources/status", serde_json::json!({ "entity": entity }));
    resp["result"].clone()
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_faction_spawn_has_starting_resources() {
    let _g = lock_world();
    let id = spawn_faction("combine");
    let s = resources_status(id);
    assert_eq!(s["fuel"].as_f64().unwrap(), 200.0);
    assert_eq!(s["scrap"].as_f64().unwrap(), 200.0);
    let m = s["manpower"].as_f64().unwrap();
    assert!(m >= 50.0 && m < 55.0, "manpower starts ~50, got {m}");
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_resources_spend_deducts_when_affordable() {
    let _g = lock_world();
    let id = spawn_faction("ironborn");
    let resp = post(
        "resources/spend",
        serde_json::json!({ "entity": id, "fuel": 50.0, "scrap": 30.0, "manpower": 10.0 }),
    );
    assert_eq!(resp["result"]["success"].as_bool().unwrap(), true);
    let s = resources_status(id);
    assert_eq!(s["fuel"].as_f64().unwrap(), 150.0);
    assert_eq!(s["scrap"].as_f64().unwrap(), 170.0);
    // manpower ticks up between spend and status query — bound the delta.
    let m = s["manpower"].as_f64().unwrap();
    assert!(m >= 40.0 && m < 45.0, "manpower ~40 after spend, got {m}");
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_resources_spend_rejects_when_unaffordable() {
    let _g = lock_world();
    let id = spawn_faction("covenant");
    let resp = post(
        "resources/spend",
        serde_json::json!({ "entity": id, "fuel": 99999.0 }),
    );
    assert_eq!(resp["result"]["success"].as_bool().unwrap(), false);
    let s = resources_status(id);
    assert_eq!(s["fuel"].as_f64().unwrap(), 200.0);
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_manpower_trickles_over_time() {
    let _g = lock_world();
    let id = spawn_faction("hollow");
    let before = resources_status(id);
    let m_before = before["manpower"].as_f64().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let after = resources_status(id);
    let m_after = after["manpower"].as_f64().unwrap();
    assert!(
        m_after > m_before,
        "manpower should trickle: before={m_before} after={m_after}"
    );
}

fn spawn_point(point_type: &str, x: i32, y: i32, radius: f64) -> u64 {
    let resp = post(
        "point/spawn",
        serde_json::json!({
            "point_type": point_type,
            "x": x,
            "y": y,
            "radius": radius,
        }),
    );
    resp["result"]["entity_id"].as_u64().expect("entity_id u64")
}

fn point_status(entity: u64) -> serde_json::Value {
    let resp = post("point/status", serde_json::json!({ "entity": entity }));
    resp["result"].clone()
}

fn spawn_rifleman_with_faction(x: i32, y: i32, faction: &str) -> u64 {
    let resp = post(
        "unit/spawn",
        serde_json::json!({
            "unit_type": "rifleman",
            "x": x,
            "y": y,
            "faction": faction,
        }),
    );
    resp["result"]["entity_id"].as_u64().expect("entity_id u64")
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_lone_unit_captures_neutral_point() {
    let _g = lock_world();
    let _faction = spawn_faction("combine");
    let pt = spawn_point("strategic", 100, 100, 2.0);
    let _u = spawn_rifleman_with_faction(100, 100, "combine");

    // capture_rate = 0.2/s; full capture at 5s. Wait 6s for safety.
    std::thread::sleep(std::time::Duration::from_millis(6000));

    let s = point_status(pt);
    assert_eq!(
        s["owner"].as_str().unwrap(),
        "Combine",
        "expected Combine to own point: {s}"
    );
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_contested_point_does_not_capture() {
    let _g = lock_world();
    let _f1 = spawn_faction("combine");
    let _f2 = spawn_faction("hollow");
    let pt = spawn_point("strategic", 200, 200, 2.0);
    let _u1 = spawn_rifleman_with_faction(200, 200, "combine");
    let _u2 = spawn_rifleman_with_faction(201, 200, "hollow");

    std::thread::sleep(std::time::Duration::from_millis(2000));

    let s = point_status(pt);
    assert!(
        s["owner"].is_null(),
        "expected contested point to remain neutral: {s}"
    );
}

fn place_building(faction: u64, building_type: &str, x: i32, y: i32) -> serde_json::Value {
    post(
        "building/place",
        serde_json::json!({
            "faction_entity": faction,
            "building_type": building_type,
            "x": x,
            "y": y,
        }),
    )
}

fn building_status(entity: u64) -> serde_json::Value {
    let resp = post("building/status", serde_json::json!({ "entity": entity }));
    resp["result"].clone()
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_building_place_succeeds_and_deducts_resources() {
    let _g = lock_world();
    let faction = spawn_faction("combine");
    // Refinery cost: 200 fuel, 50 scrap
    let resp = place_building(faction, "refinery", 10, 10);
    assert!(resp.get("result").is_some(), "place should succeed: {resp}");
    let bid = resp["result"]["entity_id"].as_u64().unwrap();

    let r = resources_status(faction);
    assert_eq!(r["fuel"].as_f64().unwrap(), 0.0);
    assert_eq!(r["scrap"].as_f64().unwrap(), 150.0);

    let s = building_status(bid);
    assert_eq!(s["building_type"].as_str().unwrap(), "Refinery");
    assert_eq!(s["under_construction"].as_bool().unwrap(), true);
    assert_eq!(s["built"].as_bool().unwrap(), false);
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_building_place_rejects_when_unaffordable() {
    let _g = lock_world();
    let faction = spawn_faction("hollow");
    // Drain resources first
    post("resources/spend", serde_json::json!({
        "entity": faction, "fuel": 200.0, "scrap": 200.0
    }));
    let resp = place_building(faction, "refinery", 20, 20);
    assert!(
        resp.get("error").is_some(),
        "expected error for unaffordable: {resp}"
    );
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_building_place_rejects_overlap() {
    let _g = lock_world();
    let faction = spawn_faction("ironborn");
    let _ = place_building(faction, "scrapyard", 30, 30);
    let resp = place_building(faction, "scrapyard", 30, 30);
    assert!(
        resp.get("error").is_some(),
        "expected error for occupied tile: {resp}"
    );
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_production_enqueue_rejects_non_producer() {
    let _g = lock_world();
    let faction = spawn_faction("combine");
    // tank_trap (5s build) doesn't produce units — should reject after build.
    let resp = place_building(faction, "tank_trap", 50, 50);
    let bid = resp["result"]["entity_id"].as_u64().expect("place tank_trap");
    std::thread::sleep(std::time::Duration::from_millis(6000));
    let r = post(
        "production/enqueue",
        serde_json::json!({
            "faction_entity": faction,
            "building_entity": bid,
            "unit_type": "rifleman",
        }),
    );
    assert!(r.get("error").is_some(), "expected error: {r}");
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_production_full_cycle_spawns_unit() {
    let _g = lock_world();
    let faction = spawn_faction("combine");
    // Top up resources for the test path: barracks build (~40s) is too slow.
    // We bypass by directly inserting Built via dev — but we don't have that
    // method. Instead, place barracks and wait for build, then enqueue.
    // 40s barracks + 8s rifleman = ~50s wait. This is the cost of a real e2e.
    let resp = place_building(faction, "barracks", 60, 60);
    assert!(resp.get("result").is_some(), "barracks place failed: {resp}");
    let bid = resp["result"]["entity_id"].as_u64().unwrap();

    // Wait for construction to finish (~40s).
    std::thread::sleep(std::time::Duration::from_millis(41_000));
    let s = building_status(bid);
    assert_eq!(s["built"].as_bool().unwrap(), true, "barracks not built: {s}");

    // Top up scrap (each rifleman costs 50 scrap, 5 manpower).
    // Default starting scrap was 200; barracks used 150. We have 50 scrap left.
    // That's exactly enough for 1 rifleman.
    let r = post(
        "production/enqueue",
        serde_json::json!({
            "faction_entity": faction,
            "building_entity": bid,
            "unit_type": "rifleman",
        }),
    );
    assert!(r.get("result").is_some(), "enqueue failed: {r}");

    let qs = post("production/queue_status", serde_json::json!({ "entity": bid }));
    let jobs = qs["result"]["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 1);

    // Wait for production (8s) plus a buffer.
    std::thread::sleep(std::time::Duration::from_millis(9000));

    let qs2 = post("production/queue_status", serde_json::json!({ "entity": bid }));
    let jobs2 = qs2["result"]["jobs"].as_array().unwrap();
    assert!(jobs2.is_empty(), "queue should be empty after production: {qs2}");
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_production_enqueue_rejects_unbuilt() {
    let _g = lock_world();
    let faction = spawn_faction("combine");
    let resp = place_building(faction, "barracks", 70, 70);
    let bid = resp["result"]["entity_id"].as_u64().unwrap();
    // Don't wait for construction — should reject.
    let r = post(
        "production/enqueue",
        serde_json::json!({
            "faction_entity": faction,
            "building_entity": bid,
            "unit_type": "rifleman",
        }),
    );
    assert!(r.get("error").is_some(), "expected unbuilt rejection: {r}");
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_building_finishes_construction_after_wait() {
    let _g = lock_world();
    let faction = spawn_faction("covenant");
    // TankTrap: 5s build, 50 scrap cost — cheapest + fastest.
    let resp = place_building(faction, "tank_trap", 40, 40);
    let bid = resp["result"]["entity_id"].as_u64().unwrap();

    let s0 = building_status(bid);
    assert_eq!(s0["under_construction"].as_bool().unwrap(), true);

    std::thread::sleep(std::time::Duration::from_millis(6000));

    let s1 = building_status(bid);
    assert_eq!(s1["built"].as_bool().unwrap(), true, "expected built: {s1}");
    assert_eq!(s1["under_construction"].as_bool().unwrap(), false);
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_held_fuel_depot_boosts_combine_fuel_trickle() {
    let _g = lock_world();
    let combine = spawn_faction("combine");
    let pt = spawn_point("fuel_depot", 300, 300, 2.0);
    let _u = spawn_rifleman_with_faction(300, 300, "combine");

    // Wait for capture (~5s) then a moment for trickle to apply.
    std::thread::sleep(std::time::Duration::from_millis(7000));

    let s = point_status(pt);
    assert_eq!(s["owner"].as_str().unwrap(), "Combine");

    let r = resources_status(combine);
    let fuel_trickle = r["fuel_trickle"].as_f64().unwrap();
    assert!(
        (fuel_trickle - 7.5).abs() < 0.01,
        "expected combine fuel trickle 7.5, got {fuel_trickle}: {r}"
    );
}
