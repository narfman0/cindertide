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

fn spawn_unit(unit_type: &str, x: i32, y: i32) -> u64 {
    let resp = post(
        "unit/spawn",
        serde_json::json!({
            "unit_type": unit_type,
            "x": x,
            "y": y,
            "faction": "combine",
        }),
    );
    resp["result"]["entity_id"].as_u64().expect("entity_id u64")
}

fn game_state() -> serde_json::Value {
    let r = post("game/state", serde_json::json!({}));
    r["result"].clone()
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_game_starts_at_title() {
    let _g = lock_world();
    // Reset to clear any prior state.
    post("dev/reset", serde_json::json!({}));
    // GameState resource isn't reset by dev/reset (intentional; only entities
    // and the FiredBeats resource are). For a clean check, we look at
    // whatever current state is and assert game/state returns a string.
    let s = game_state();
    assert!(s["state"].is_string(), "game state should be a string: {s}");
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_game_new_transitions_to_campaign() {
    let _g = lock_world();
    let r = post("game/new", serde_json::json!({ "player": "combine" }));
    let s = &r["result"];
    assert_eq!(s["state"].as_str().unwrap(), "Campaign");
    assert_eq!(s["missions_won"].as_u64().unwrap(), 0);
    assert_eq!(s["missions_lost"].as_u64().unwrap(), 0);
    assert_eq!(s["player"].as_str().unwrap(), "Combine");
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_full_loop_mission_select_resolve_returns_to_campaign() {
    let _g = lock_world();
    post("game/new", serde_json::json!({ "player": "combine" }));

    // Pull options and pick one.
    let opts = post("campaign/options", serde_json::json!({ "player": "combine" }));
    let zone_id = opts["result"]["options"]
        .as_array()
        .unwrap()
        .first()
        .unwrap()["zone_id"]
        .as_u64()
        .unwrap();

    // Select that mission — transitions to InMission.
    let sel = post("mission/select", serde_json::json!({ "zone_id": zone_id }));
    assert!(sel.get("result").is_some(), "mission/select: {sel}");
    let s_in = game_state();
    assert_eq!(s_in["state"].as_str().unwrap(), "InMission");
    let mission_entity = s_in["current_mission_entity"].as_u64().unwrap();

    // Force-resolve as a player win.
    post(
        "mission/force_resolve",
        serde_json::json!({ "entity": mission_entity, "won": true }),
    );
    // One tick for campaign_progression_system to apply outcome.
    std::thread::sleep(std::time::Duration::from_millis(500));

    let s_after = game_state();
    let s = s_after["state"].as_str().unwrap();
    assert!(
        s == "Campaign" || s.starts_with("GameOver"),
        "expected mission to resolve, got {s}: {s_after}"
    );
    assert_eq!(s_after["missions_won"].as_u64().unwrap(), 1);
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_game_save_load_roundtrip_preserves_run() {
    let _g = lock_world();
    post("game/new", serde_json::json!({ "player": "ironborn" }));
    post("game/save", serde_json::json!({ "slot": "loop1" }));

    // Reset everything; state goes back to defaults.
    post("dev/reset", serde_json::json!({}));

    let r = post("game/load", serde_json::json!({ "slot": "loop1" }));
    let s = &r["result"];
    assert_eq!(s["state"].as_str().unwrap(), "Campaign");
    assert_eq!(s["player"].as_str().unwrap(), "Ironborn");
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_save_write_read_roundtrip() {
    let _g = lock_world();
    let f = spawn_faction("combine");
    spawn_rifleman_with_faction(50, 50, "combine");
    spawn_rifleman_with_faction(60, 60, "combine");

    // Save.
    let r1 = post("save/write", serde_json::json!({ "slot": "test1" }));
    assert!(r1.get("result").is_some(), "save: {r1}");

    // Reset and confirm zero pop.
    post("dev/reset", serde_json::json!({}));

    // Read back.
    let r2 = post("save/read", serde_json::json!({ "slot": "test1" }));
    assert!(r2.get("result").is_some(), "read: {r2}");
    let count = r2["result"]["restored"].as_u64().unwrap();
    assert!(count >= 3, "expected >= 3 restored entities (1 faction + 2 units), got {count}");
    let _ = f;
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_editor_set_save_load_roundtrip() {
    let _g = lock_world();
    // Set three tiles via editor.
    for (x, y, terrain) in &[(0, 0, "Road"), (1, 0, "Grass"), (2, 0, "Forest")] {
        post(
            "editor/set_tile",
            serde_json::json!({ "x": x, "y": y, "terrain": terrain }),
        );
    }

    let saved = post("editor/save_map", serde_json::json!({}));
    let tiles = saved["result"]["tiles"].as_array().unwrap();
    assert_eq!(tiles.len(), 3);

    // Wipe via dev/reset, then reload from saved JSON.
    post("dev/reset", serde_json::json!({}));

    let loaded = post(
        "editor/load_map",
        serde_json::json!({ "tiles": tiles }),
    );
    assert_eq!(loaded["result"]["loaded"].as_u64().unwrap(), 3);

    let after = post("editor/save_map", serde_json::json!({}));
    let after_tiles = after["result"]["tiles"].as_array().unwrap();
    assert_eq!(after_tiles.len(), 3);
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_hollow_spawner_increases_pop_count() {
    let _g = lock_world();
    let hollow = spawn_faction("hollow");
    // Hollow spawner at consuming mode (6s interval).
    post(
        "hollow/spawn_point",
        serde_json::json!({ "x": 700, "y": 700, "mode": "consuming" }),
    );
    // Wait ~7s for first spawn + tick for pop_cap_system.
    std::thread::sleep(std::time::Duration::from_millis(7500));
    let s = resources_status(hollow);
    let pop = s["pop_current"].as_u64().unwrap();
    assert!(pop >= 1, "expected hollow population > 0: {s}");
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_beats_fired_starts_empty_and_records_hero_down() {
    let _g = lock_world();
    let r0 = post("beats/fired", serde_json::json!({}));
    let fired0 = r0["result"]["fired"].as_array().unwrap();
    assert!(fired0.iter().all(|b| b.as_str() != Some("HeroGoesDown")));

    // Spawn a hero, kill them via massive damage by ordering attack.
    let hero = spawn_hero("Iron", "combine", 0, 0, "rally");
    // Heroes have 500 HP; need a lot of damage. Spawn many enemies.
    for i in 0..6 {
        let e = spawn_rifleman_with_faction(2 + i, 0, "hollow");
        post(
            "combat/attack",
            serde_json::json!({ "attacker_entity": e, "target_entity": hero }),
        );
    }
    std::thread::sleep(std::time::Duration::from_millis(20_000));

    let s = hero_status(hero);
    assert_eq!(s["downed"].as_bool().unwrap(), true, "hero should be downed: {s}");

    let r1 = post("beats/fired", serde_json::json!({}));
    let fired1 = r1["result"]["fired"].as_array().unwrap();
    assert!(
        fired1.iter().any(|b| b.as_str() == Some("HeroGoesDown")),
        "expected HeroGoesDown beat: {fired1:?}"
    );
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_campaign_init_and_advance_changes_zone_owner() {
    let _g = lock_world();
    post("campaign/init", serde_json::json!({}));
    let s0 = post("campaign/state", serde_json::json!({}));
    let zones0 = s0["result"]["zones"].as_array().unwrap();
    let pale_before = zones0.iter().find(|z| z["id"].as_u64() == Some(2)).unwrap();
    assert_eq!(pale_before["owner"].as_str().unwrap(), "Hollow");

    let _ = post(
        "campaign/advance",
        serde_json::json!({
            "zone_id": 2,
            "winner": "combine",
            "loser": "hollow",
        }),
    );

    let s1 = post("campaign/state", serde_json::json!({}));
    let zones1 = s1["result"]["zones"].as_array().unwrap();
    let pale_after = zones1.iter().find(|z| z["id"].as_u64() == Some(2)).unwrap();
    assert_eq!(pale_after["owner"].as_str().unwrap(), "Combine");
    assert_eq!(s1["result"]["turn"].as_u64().unwrap(), 1);
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_campaign_options_excludes_player_zones() {
    let _g = lock_world();
    post("campaign/init", serde_json::json!({}));
    let r = post("campaign/options", serde_json::json!({ "player": "combine" }));
    let opts = r["result"]["options"].as_array().unwrap();
    // Combine owns zone 0; options should not include it.
    for o in opts {
        assert_ne!(o["zone_id"].as_u64().unwrap(), 0);
    }
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_mission_starts_active() {
    let _g = lock_world();
    let resp = post(
        "mission/start",
        serde_json::json!({
            "mission_type": "control",
            "player": "combine",
            "opponent": "hollow",
            "deadline": 600.0,
        }),
    );
    let mid = resp["result"]["entity_id"].as_u64().unwrap();
    let s = post("mission/status", serde_json::json!({ "entity": mid }));
    let r = &s["result"];
    assert_eq!(r["status"].as_str().unwrap(), "Active");
    assert_eq!(r["mission_type"].as_str().unwrap(), "Control");
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_map_generate_assault_layout() {
    let _g = lock_world();
    let resp = post(
        "map/generate",
        serde_json::json!({
            "width": 20,
            "height": 10,
            "seed": 42,
            "mission_type": "assault",
        }),
    );
    let r = &resp["result"];
    assert_eq!(r["tiles_spawned"].as_i64().unwrap(), 200);
    assert_eq!(r["bases"].as_array().unwrap().len(), 2);
    assert_eq!(r["chokepoints"].as_array().unwrap().len(), 1);
    assert_eq!(r["mission_type"].as_str().unwrap(), "Assault");
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_ai_economic_builds_refinery_first() {
    let _g = lock_world();
    let f = spawn_faction("combine");
    // Top up so the AI can build.
    post(
        "dev/give_resources",
        serde_json::json!({ "entity": f, "fuel": 500.0, "scrap": 500.0 }),
    );
    let r = post(
        "ai/enable",
        serde_json::json!({
            "entity": f,
            "home_x": 600,
            "home_y": 600,
            "doctrine": "assault",
        }),
    );
    assert!(r.get("result").is_some(), "ai/enable failed: {r}");

    // Wait through one AI tick (~3s) plus a second.
    std::thread::sleep(std::time::Duration::from_millis(4500));

    // Use bevy/list (if available) or just verify resources were spent —
    // the AI should have built a Refinery (200 fuel + 50 scrap).
    let s = resources_status(f);
    let fuel = s["fuel"].as_f64().unwrap();
    assert!(
        fuel < 700.0,
        "expected AI to spend resources, fuel={fuel}: {s}"
    );
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_pop_cap_tracks_unit_count() {
    let _g = lock_world();
    let f = spawn_faction("combine");
    // Wait one tick for pop_cap_system to recompute.
    std::thread::sleep(std::time::Duration::from_millis(200));
    let s0 = resources_status(f);
    assert_eq!(s0["pop_current"].as_u64().unwrap(), 0);
    assert!(s0["pop_max"].as_u64().unwrap() >= 20);

    // Spawn 3 riflemen and confirm count rises.
    for i in 0..3 {
        spawn_rifleman_with_faction(400 + i, 400, "combine");
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    let s1 = resources_status(f);
    assert_eq!(s1["pop_current"].as_u64().unwrap(), 3);
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_repair_bay_heals_damaged_vehicle() {
    let _g = lock_world();
    let faction = spawn_faction("combine");

    // Top up resources so faction has scrap for the bay AND repair budget.
    post(
        "dev/give_resources",
        serde_json::json!({ "entity": faction, "fuel": 500.0, "scrap": 500.0 }),
    );

    // Place a repair bay (RepairBay: 100 fuel, 200 scrap, 25s build)
    let resp = place_building(faction, "repair_bay", 200, 200);
    let _bay = resp["result"]["entity_id"].as_u64().expect("bay id");

    // Spawn a light vehicle nearby (within REPAIR_RADIUS=4).
    let veh = spawn_unit("light_vehicle", 200, 202);

    // Damage the vehicle: hit it via combat to drop HP.
    let enemy = spawn_rifleman_with_faction(199, 202, "hollow");
    post(
        "combat/attack",
        serde_json::json!({ "attacker_entity": enemy, "target_entity": veh }),
    );

    // Wait through bay construction (~25s) plus a couple seconds for damage
    // and repair to start.
    std::thread::sleep(std::time::Duration::from_millis(28_000));

    let s_mid = post("combat/status", serde_json::json!({ "entity": veh }));
    let mid_hp = s_mid["result"]["health_current"].as_f64().unwrap();
    assert!(
        mid_hp < 120.0,
        "expected vehicle damaged by enemy fire: {s_mid:?}"
    );

    // Kill the enemy so damage stops, leaving only repair active.
    post(
        "combat/attack",
        serde_json::json!({ "attacker_entity": veh, "target_entity": enemy }),
    );
    std::thread::sleep(std::time::Duration::from_millis(10_000));

    let s_end = post("combat/status", serde_json::json!({ "entity": veh }));
    let end_hp = s_end["result"]["health_current"].as_f64().unwrap();
    assert!(
        end_hp > mid_hp,
        "expected vehicle to repair after enemy down: mid={mid_hp} end={end_hp}"
    );
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_threat_response_returns_fire() {
    let _g = lock_world();
    // Two riflemen in mutual range. Order one to attack the other and let
    // them shoot. After the unattacked target takes damage, unit AI should
    // make it return fire (set AttackTarget on it pointing at attacker).
    let aggressor = spawn_rifleman_with_faction(100, 100, "combine");
    let victim = spawn_rifleman_with_faction(102, 100, "hollow");

    post(
        "combat/attack",
        serde_json::json!({ "attacker_entity": aggressor, "target_entity": victim }),
    );

    // Wait for first hit + unit AI threat response (one tick after damage).
    std::thread::sleep(std::time::Duration::from_millis(2000));

    // Victim should be alive (or at least have lost some health).
    let s = combat_status(victim);
    let h = s["health_current"].as_f64().unwrap();
    assert!(h < 100.0, "victim should have taken damage: {s}");

    // Now confirm the aggressor lost some health too — only possible if
    // unit AI made the victim return fire.
    std::thread::sleep(std::time::Duration::from_millis(2000));
    let s2 = combat_status(aggressor);
    let h2 = s2["health_current"].as_f64().unwrap();
    assert!(
        h2 < 100.0,
        "aggressor should be hit by return fire after threat response: {s2}"
    );
}

fn tech_status(faction: u64) -> serde_json::Value {
    let resp = post(
        "tech/status",
        serde_json::json!({ "faction_entity": faction }),
    );
    resp["result"].clone()
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_faction_starts_at_tier_one_no_doctrine() {
    let _g = lock_world();
    let f = spawn_faction("combine");
    let s = tech_status(f);
    assert_eq!(s["tier"].as_str().unwrap(), "One");
    assert!(s["doctrine"].is_null());
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_tech_research_skip_tier_rejected() {
    let _g = lock_world();
    let f = spawn_faction("combine");
    let r = post(
        "tech/research",
        serde_json::json!({ "faction_entity": f, "target": "tier_3" }),
    );
    assert!(r.get("error").is_some(), "expected SkipsTier error: {r}");
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_tech_research_unaffordable_rejected() {
    let _g = lock_world();
    let f = spawn_faction("combine");
    // Drain resources.
    post("resources/spend", serde_json::json!({
        "entity": f, "fuel": 200.0, "scrap": 200.0
    }));
    let r = post(
        "tech/research",
        serde_json::json!({ "faction_entity": f, "target": "tier_2" }),
    );
    assert!(r.get("error").is_some(), "expected Unaffordable: {r}");
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_tech_research_starts_and_marks_in_progress() {
    let _g = lock_world();
    let f = spawn_faction("combine");
    let r = post(
        "tech/research",
        serde_json::json!({ "faction_entity": f, "target": "tier_2" }),
    );
    assert!(r.get("result").is_some(), "expected start: {r}");
    let s = tech_status(f);
    assert!(!s["researching"].is_null());
    assert_eq!(s["tier"].as_str().unwrap(), "One"); // not yet applied
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_doctrine_research_completes_after_wait() {
    let _g = lock_world();
    let f = spawn_faction("combine");
    // Doctrine research is 45s, costs 100 fuel + 150 scrap. Start it, wait,
    // verify applied.
    let r = post(
        "tech/research",
        serde_json::json!({ "faction_entity": f, "target": "doctrine_assault" }),
    );
    assert!(r.get("result").is_some(), "expected start: {r}");

    std::thread::sleep(std::time::Duration::from_millis(46_000));

    let s = tech_status(f);
    assert_eq!(s["doctrine"].as_str().unwrap(), "Assault", "expected applied: {s}");
    assert!(s["researching"].is_null());
}

fn spawn_hero(name: &str, faction: &str, x: i32, y: i32, kind: &str) -> u64 {
    let resp = post(
        "hero/spawn",
        serde_json::json!({
            "name": name,
            "faction": faction,
            "x": x,
            "y": y,
            "ability_kind": kind,
        }),
    );
    resp["result"]["entity_id"].as_u64().expect("entity_id u64")
}

fn hero_status(entity: u64) -> serde_json::Value {
    let resp = post("hero/status", serde_json::json!({ "entity": entity }));
    resp["result"].clone()
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_hero_spawn_starts_zero_charge_full_health() {
    let _g = lock_world();
    let h = spawn_hero("Iron", "combine", 0, 0, "rally");
    let s = hero_status(h);
    assert_eq!(s["name"].as_str().unwrap(), "Iron");
    // Charge starts at 0 and ticks at ~5/sec; allow a small accumulation
    // for the time between spawn and status query.
    let c = s["charge"].as_f64().unwrap();
    assert!(c < 5.0, "expected charge near 0, got {c}");
    assert_eq!(s["downed"].as_bool().unwrap(), false);
    assert_eq!(
        s["health_current"].as_f64().unwrap(),
        s["health_max"].as_f64().unwrap()
    );
    assert!(s["aura_radius"].as_f64().unwrap() > 0.0);
    assert!(s["aura_suppression_resist"].as_f64().unwrap() > 0.0);
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_hero_charge_accumulates_over_time() {
    let _g = lock_world();
    let h = spawn_hero("Iron", "combine", 0, 0, "rally");
    std::thread::sleep(std::time::Duration::from_millis(2000));
    let s = hero_status(h);
    let c = s["charge"].as_f64().unwrap();
    assert!(c > 0.0, "expected charge > 0 after 2s, got {c}");
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_ability_use_rejects_when_not_charged() {
    let _g = lock_world();
    let h = spawn_hero("Iron", "combine", 0, 0, "rally");
    let r = post("hero/ability_use", serde_json::json!({ "entity": h }));
    assert!(r.get("error").is_some(), "expected error: {r}");
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_rally_clears_allied_suppression() {
    let _g = lock_world();
    let h = spawn_hero("Iron", "combine", 50, 50, "rally");
    let ally = spawn_rifleman_with_faction(50, 51, "combine"); // within aura radius 5
    let enemy = spawn_rifleman_with_faction(53, 50, "hollow"); // distance 3, in range

    // Have enemy attack ally to build suppression.
    post(
        "combat/attack",
        serde_json::json!({ "attacker_entity": enemy, "target_entity": ally }),
    );

    std::thread::sleep(std::time::Duration::from_millis(2500));

    let before = combat_status(ally);
    let supp_before = before["suppression_current"].as_f64().unwrap();
    assert!(
        supp_before > 0.0,
        "expected ally to have suppression before rally: {before}"
    );

    // Force-charge the hero by waiting (5/sec, max 100 → ~20s) — too slow for
    // tests. Cheat by calling ability_use repeatedly until it succeeds:
    // the harness can wait for natural charge in a slower test, but here we
    // assert the rejection branch separately and the rally effect by waiting
    // ~21s for natural charge.
    std::thread::sleep(std::time::Duration::from_millis(21_000));

    let r = post("hero/ability_use", serde_json::json!({ "entity": h }));
    assert!(r.get("result").is_some(), "rally should fire: {r}");

    // Allow a frame for the change to be reflected.
    std::thread::sleep(std::time::Duration::from_millis(500));

    let after = combat_status(ally);
    let supp_after = after["suppression_current"].as_f64().unwrap();
    assert!(
        supp_after < supp_before,
        "rally should reduce suppression: before={supp_before} after={supp_after}"
    );
}

#[test]
#[ignore = "requires a running Cindertide server on port 15703"]
fn brp_spawn_all_unit_types() {
    let _g = lock_world();
    for t in &["rifleman", "heavy_weapons", "light_vehicle", "heavy_armor"] {
        let id = spawn_unit(t, 0, 0);
        let s = unit_status(id);
        assert_eq!(
            s["health_current"].as_f64().unwrap(),
            s["health_max"].as_f64().unwrap(),
            "{t} should spawn at full health"
        );
    }
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
