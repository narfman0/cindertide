use bevy::prelude::*;
use bevy::remote::{RemotePlugin, http::RemoteHttpPlugin, BrpResult, BrpError};
use serde_json::Value;

pub mod factions;
pub mod map;
pub mod units;
pub mod combat;
pub mod resources;
pub mod control;
pub mod buildings;
pub mod production;
pub mod heroes;
pub mod tech;
pub mod unit_ai;
pub mod repair;
pub mod mapgen;
pub mod mission;
pub mod campaign;
pub mod beats;
pub mod hollow;
pub mod editor;
pub mod save;
pub mod game;
#[cfg(feature = "render")]
pub mod render;
pub mod ai;
pub mod tui;
pub mod narrative;
pub mod mission_script;
pub mod camera;

use map::{MapPlugin, GridPos, Faction, ControlPoint, ControlPointType};
use units::{UnitPlugin, MoveTarget, MoveProgress, UnitPos, UnitKind, UnitBundle};
use combat::PlayerAttackOrder;
use combat::{CombatPlugin, AttackTarget, Health, Morale, Suppression, Facing, morale_state};
use resources::{ResourcesPlugin, FactionBundle, ResourcePool, ResourceTrickle, ResourceCost, spend, can_afford};
use control::ControlPlugin;
use buildings::{BuildingsPlugin, BuildingTypeId, BuildingBundle, BuildingPos, ConstructionProgress, building_cost, building_produces, try_pay, can_place, Built, UnderConstruction};
use production::{ProductionPlugin, ProductionQueue, try_enqueue, EnqueueError};
use factions::{LoadedFactions, FactionsPlugin};
use heroes::{HeroPlugin, HeroBundle, Hero, AbilityKind, SignatureAbility, Aura, HeroDowned, is_charge_full, within_aura};
use tech::{TechPlugin, Tech, Tier, Doctrine, ResearchTarget, ResearchInProgress, start_research};
use unit_ai::UnitAiPlugin;
use repair::RepairPlugin;
use ai::{AiPlugin, AiController};
use mission::{MissionPlugin, Mission, MissionStatus};
use campaign::{CampaignPlugin, CampaignRun, GlobalProgress, PlayableFaction, next_mission_type};
use beats::{BeatsPlugin, FiredBeats};
use hollow::{HollowPlugin, HollowSpawner, HollowMode};
use save::{SavePlugin, SaveSlots};
use game::{GamePlugin, GameState, ActiveRun, fire_exit};
use mission_script::MissionScriptPlugin;

pub fn run_server() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    #[cfg(feature = "render")]
    app.add_plugins(render::RenderPlugin);
    app
        .add_plugins(
            RemotePlugin::default()
                .with_method("unit/move", handle_unit_move)
                .with_method("unit/spawn", handle_unit_spawn)
                .with_method("unit/status", handle_unit_status)
                .with_method("combat/attack", handle_combat_attack)
                .with_method("combat/status", handle_combat_status)
                .with_method("faction/spawn", handle_faction_spawn)
                .with_method("resources/status", handle_resources_status)
                .with_method("resources/spend", handle_resources_spend)
                .with_method("point/spawn", handle_point_spawn)
                .with_method("point/status", handle_point_status)
                .with_method("building/place", handle_building_place)
                .with_method("building/status", handle_building_status)
                .with_method("dev/reset", handle_dev_reset)
                .with_method("dev/give_resources", handle_dev_give_resources)
                .with_method("ai/enable", handle_ai_enable)
                .with_method("map/generate", handle_map_generate)
                .with_method("mission/start", handle_mission_start)
                .with_method("mission/status", handle_mission_status)
                .with_method("campaign/init", handle_campaign_init)
                .with_method("campaign/state", handle_campaign_state)
                .with_method("campaign/advance", handle_campaign_advance)
                .with_method("campaign/options", handle_campaign_options)
                .with_method("beats/fired", handle_beats_fired)
                .with_method("hollow/spawn_point", handle_hollow_spawn_point)
                .with_method("editor/set_tile", handle_editor_set_tile)
                .with_method("editor/save_map", handle_editor_save_map)
                .with_method("editor/load_map", handle_editor_load_map)
                .with_method("save/write", handle_save_write)
                .with_method("save/read", handle_save_read)
                .with_method("game/state", handle_game_state)
                .with_method("game/new", handle_game_new)
                .with_method("game/load", handle_game_load)
                .with_method("game/save", handle_game_save)
                .with_method("game/exit", handle_game_exit)
                .with_method("mission/select", handle_mission_select)
                .with_method("mission/force_resolve", handle_mission_force_resolve)
                .with_method("production/enqueue", handle_production_enqueue)
                .with_method("production/queue_status", handle_production_queue_status)
                .with_method("hero/spawn", handle_hero_spawn)
                .with_method("hero/status", handle_hero_status)
                .with_method("hero/ability_use", handle_hero_ability_use)
                .with_method("tech/research", handle_tech_research)
                .with_method("tech/status", handle_tech_status)
                .with_method("world/list", handle_world_list)
                .with_method("game/pause", handle_game_pause)
                .with_method("game/pause_status", handle_game_pause_status)
                .with_method("save/list", handle_save_list)
                .with_method("game/abandon_mission", handle_game_abandon_mission)
                .with_method("narrative/mission", handle_narrative_mission)
                .with_method("narrative/debrief", handle_narrative_debrief)
                .with_method("narrative/finale", handle_narrative_finale)
                .with_method("unit/move_path", handle_unit_move_path)
                .with_method("unit/attack_order", handle_unit_attack_order)
                .with_method("building/construct", handle_building_construct)
        )
        .add_plugins(RemoteHttpPlugin::default().with_port(15703))
        .add_plugins(MapPlugin)
        .add_plugins(UnitPlugin)
        .add_plugins(CombatPlugin)
        .add_plugins(ResourcesPlugin)
        .add_plugins(ControlPlugin)
        .add_plugins(BuildingsPlugin)
        .add_plugins(ProductionPlugin)
        .add_plugins(HeroPlugin)
        .add_plugins(TechPlugin)
        .add_plugins(UnitAiPlugin)
        .add_plugins(RepairPlugin)
        .add_plugins(AiPlugin)
        .add_plugins(MissionPlugin)
        .add_plugins(CampaignPlugin)
        .add_plugins(BeatsPlugin)
        .add_plugins(HollowPlugin)
        .add_plugins(SavePlugin)
        .add_plugins(GamePlugin)
        .add_plugins(MissionScriptPlugin)
        .add_systems(Startup, on_startup);
    // Load faction/unit/building definitions from assets/factions/*.toml
    let loaded_factions = factions::LoadedFactions::load_from_dir("assets/factions");
    app.insert_resource(loaded_factions);

    // Load narrative data — path relative to working directory (project root when running via cargo)
    let narrative = narrative::NarrativeData::load("assets/narrative.toml")
        .unwrap_or_else(|e| {
            eprintln!("Warning: could not load assets/narrative.toml: {e}");
            narrative::NarrativeData {
                factions: Default::default(),
                finales: narrative::Finales {
                    combine_first: String::new(),
                    ironborn_first: String::new(),
                },
            }
        });
    app.insert_resource(narrative);
    app.run();
}

/// BRP handler for "unit/move": accepts { entity_id, target_x, target_y }
/// and inserts a MoveTarget component on the specified entity.
fn handle_unit_move(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;

    let entity_id = params["entity_id"]
        .as_u64()
        .ok_or_else(|| BrpError {
            code: -32602,
            message: "entity_id must be a u64".into(),
            data: None,
        })?;

    let target_x = params["target_x"]
        .as_i64()
        .ok_or_else(|| BrpError {
            code: -32602,
            message: "target_x must be an integer".into(),
            data: None,
        })? as i32;

    let target_y = params["target_y"]
        .as_i64()
        .ok_or_else(|| BrpError {
            code: -32602,
            message: "target_y must be an integer".into(),
            data: None,
        })? as i32;

    // Reconstruct the Entity from the raw bits
    let entity = Entity::from_bits(entity_id);

    world
        .get_entity_mut(entity)
        .map_err(|_| BrpError {
            code: -32602,
            message: format!("entity {entity_id} not found"),
            data: None,
        })?
        .insert(MoveTarget {
            target: GridPos { x: target_x, y: target_y },
        });

    Ok(Value::Bool(true))
}

/// BRP handler for "unit/move_path": { entity_id, x, y } — pathfinds and issues move order.
fn handle_unit_move_path(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError { code: -32602, message: "missing params".into(), data: None })?;
    let entity_id = params["entity_id"].as_u64().ok_or_else(|| BrpError { code: -32602, message: "entity_id required".into(), data: None })?;
    let tx = params["x"].as_i64().ok_or_else(|| BrpError { code: -32602, message: "x required".into(), data: None })? as i32;
    let ty = params["y"].as_i64().ok_or_else(|| BrpError { code: -32602, message: "y required".into(), data: None })? as i32;

    let entity = Entity::from_bits(entity_id);
    let start = {
        let er = world.get_entity(entity).map_err(|_| BrpError { code: -32602, message: format!("entity {entity_id} not found"), data: None })?;
        er.get::<UnitPos>().ok_or_else(|| BrpError { code: -32602, message: "entity has no UnitPos".into(), data: None })?.pos.clone()
    };
    let unit_kind = {
        let er = world.get_entity(entity).map_err(|_| BrpError { code: -32602, message: format!("entity {entity_id} not found"), data: None })?;
        er.get::<UnitKind>().cloned()
    };

    let goal = map::GridPos { x: tx, y: ty };
    let tile_map: std::collections::HashMap<(i32, i32), map::TerrainType> = {
        let mut q = world.query::<&map::Tile>();
        q.iter(world).map(|t| ((t.pos.x, t.pos.y), t.terrain_type.clone())).collect()
    };
    let max_x = tile_map.keys().map(|(x, _)| *x).max().unwrap_or(32);
    let max_y = tile_map.keys().map(|(_, y)| *y).max().unwrap_or(32);
    let pf_kind = match unit_kind {
        Some(UnitKind::Vehicle) => map::pathfinding::UnitKind::Vehicle,
        _ => map::pathfinding::UnitKind::Infantry,
    };
    let grid = map::pathfinding::PathfindingGrid { width: max_x + 1, height: max_y + 1, tiles: tile_map, unit_type: pf_kind, occupied: std::collections::HashSet::new(), destination: None };
    match grid.find_path(start, goal) {
        Some(path) => {
            let steps = path.len();
            world.get_entity_mut(entity).map_err(|_| BrpError { code: -32602, message: "entity not found".into(), data: None })?
                .insert(MoveTarget { target: map::GridPos { x: tx, y: ty } })
                .insert(MoveProgress { path, current_step: 0, elapsed: 0.0 });
            Ok(serde_json::json!({ "ok": true, "steps": steps }))
        }
        None => Ok(serde_json::json!({ "ok": false, "reason": "no path" })),
    }
}

/// BRP handler for "unit/attack_order": { entity_id, target_entity_id }
fn handle_unit_attack_order(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError { code: -32602, message: "missing params".into(), data: None })?;
    let entity_id = params["entity_id"].as_u64().ok_or_else(|| BrpError { code: -32602, message: "entity_id required".into(), data: None })?;
    let target_id = params["target_entity_id"].as_u64().ok_or_else(|| BrpError { code: -32602, message: "target_entity_id required".into(), data: None })?;

    let attacker = Entity::from_bits(entity_id);
    let target = Entity::from_bits(target_id);

    world.get_entity_mut(attacker).map_err(|_| BrpError { code: -32602, message: format!("entity {entity_id} not found"), data: None })?
        .remove::<MoveTarget>()
        .remove::<MoveProgress>()
        .insert(PlayerAttackOrder { target });

    Ok(serde_json::json!({ "ok": true }))
}

/// BRP handler for "building/construct": { x, y, building_type, faction } — player build order.
fn handle_building_construct(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError { code: -32602, message: "missing params".into(), data: None })?;
    let x = params["x"].as_i64().ok_or_else(|| BrpError { code: -32602, message: "x required".into(), data: None })? as i32;
    let y = params["y"].as_i64().ok_or_else(|| BrpError { code: -32602, message: "y required".into(), data: None })? as i32;
    let building_type = parse_building_type(params["building_type"].as_str().ok_or_else(|| BrpError { code: -32602, message: "building_type required".into(), data: None })?)?;
    let faction_str = params["faction"].as_str().ok_or_else(|| BrpError { code: -32602, message: "faction required".into(), data: None })?;
    let faction = parse_faction(faction_str)?;

    let target = map::GridPos { x, y };
    let mut occupied: std::collections::HashSet<map::GridPos> = std::collections::HashSet::new();
    {
        let mut q = world.query::<&BuildingPos>();
        for bp in q.iter(world) { occupied.insert(bp.pos.clone()); }
    }
    if !can_place(&target, &occupied, &std::collections::HashSet::new()) {
        return Ok(serde_json::json!({ "ok": false, "reason": "tile is occupied" }));
    }

    let loaded = world.resource::<LoadedFactions>().clone();
    let cost = building_cost(&building_type, &loaded);
    let faction_entity = {
        let mut q = world.query::<(Entity, &resources::FactionEntity)>();
        q.iter(world)
            .find(|(_, fe)| fe.faction == faction)
            .map(|(e, _)| e)
    };
    let faction_entity = faction_entity.ok_or_else(|| BrpError { code: -32000, message: "faction entity not found".into(), data: None })?;

    {
        let mut f_mut = world.get_entity_mut(faction_entity).map_err(|_| BrpError { code: -32000, message: "faction entity gone".into(), data: None })?;
        let mut pool = f_mut.get_mut::<resources::ResourcePool>().ok_or_else(|| BrpError { code: -32000, message: "faction has no ResourcePool".into(), data: None })?;
        if !can_afford(&pool, &cost) {
            return Ok(serde_json::json!({ "ok": false, "reason": "insufficient resources" }));
        }
        spend(&mut pool, &cost);
    }

    let bt_for_queue = building_type.clone();
    let entity = world.spawn(BuildingBundle::new(building_type, faction, x, y, &loaded)).id();
    if !building_produces(&bt_for_queue, &loaded).is_empty() {
        world.entity_mut(entity).insert(production::ProductionQueue::default());
    }

    Ok(serde_json::json!({ "ok": true }))
}

/// BRP handler for "unit/spawn": { unit_type: "rifleman", x: i32, y: i32 }
/// Spawns the unit and returns { entity_id: u64 }.
fn handle_unit_spawn(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;

    let unit_type_str = params["unit_type"]
        .as_str()
        .ok_or_else(|| BrpError {
            code: -32602,
            message: "unit_type must be a string".into(),
            data: None,
        })?;

    let x = params["x"]
        .as_i64()
        .ok_or_else(|| BrpError {
            code: -32602,
            message: "x must be an integer".into(),
            data: None,
        })? as i32;

    let y = params["y"]
        .as_i64()
        .ok_or_else(|| BrpError {
            code: -32602,
            message: "y must be an integer".into(),
            data: None,
        })? as i32;

    let faction_str = params["faction"].as_str().unwrap_or("combine");
    let faction = Faction::new(faction_str);

    let unit_id = normalize_unit_type_id(unit_type_str);
    let loaded = world.resource::<LoadedFactions>().clone();
    let entity = if let Some(def) = loaded.units.get(&unit_id) {
        world.spawn(UnitBundle::from_def(def, faction, x, y)).id()
    } else {
        world.spawn(UnitBundle::default_riflemen(faction, x, y)).id()
    };
    world
        .entity_mut(entity)
        .insert(units::HomeBase { pos: GridPos { x, y } });
    let entity_id = entity.to_bits();

    Ok(serde_json::json!({ "entity_id": entity_id }))
}

/// BRP handler for "unit/status": { entity: u64 }
/// Returns full unit status including type, position, health, suppression, morale.
fn handle_unit_status(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;

    let entity_id = params["entity"]
        .as_u64()
        .ok_or_else(|| BrpError {
            code: -32602,
            message: "entity must be a u64".into(),
            data: None,
        })?;

    let entity = Entity::try_from_bits(entity_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;

    let entity_ref = world
        .get_entity(entity)
        .map_err(|_| BrpError {
            code: -32602,
            message: format!("entity {entity_id} not found"),
            data: None,
        })?;

    let unit_type = entity_ref
        .get::<units::UnitTypeId>()
        .map(|t| t.id().to_string())
        .unwrap_or_else(|| "unknown".into());

    let pos = entity_ref.get::<UnitPos>();
    let health = entity_ref.get::<Health>().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity has no Health component".into(),
        data: None,
    })?;

    let is_dead = entity_ref.get::<combat::Dead>().is_some();
    let facing = entity_ref.get::<Facing>().map(|f| format!("{:?}", f));
    let suppression = entity_ref.get::<Suppression>();
    let morale = entity_ref.get::<Morale>();
    let morale_st = morale.map(morale_state).map(|s| format!("{:?}", s));

    Ok(serde_json::json!({
        "unit_type": unit_type,
        "pos_x": pos.map(|p| p.pos.x),
        "pos_y": pos.map(|p| p.pos.y),
        "facing": facing,
        "health_current": health.current,
        "health_max": health.max,
        "is_dead": is_dead,
        "suppression_current": suppression.map(|s| s.current),
        "morale_current": morale.map(|m| m.current),
        "morale_state": morale_st,
    }))
}

/// BRP handler for "combat/attack": { attacker_entity, target_entity }
/// Inserts AttackTarget on the attacker entity.
fn handle_combat_attack(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;

    let attacker_id = params["attacker_entity"]
        .as_u64()
        .ok_or_else(|| BrpError {
            code: -32602,
            message: "attacker_entity must be a u64".into(),
            data: None,
        })?;

    let target_id = params["target_entity"]
        .as_u64()
        .ok_or_else(|| BrpError {
            code: -32602,
            message: "target_entity must be a u64".into(),
            data: None,
        })?;

    let attacker = Entity::try_from_bits(attacker_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("attacker entity {attacker_id} not found"),
        data: None,
    })?;
    let target = Entity::try_from_bits(target_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("target entity {target_id} not found"),
        data: None,
    })?;

    world
        .get_entity_mut(attacker)
        .map_err(|_| BrpError {
            code: -32602,
            message: format!("attacker entity {attacker_id} not found"),
            data: None,
        })?
        .insert(AttackTarget { entity: target });

    Ok(Value::Bool(true))
}

/// BRP handler for "combat/status": { entity }
/// Returns combat state — health plus optional suppression/pinned/cover fields.
fn handle_combat_status(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;

    let entity_id = params["entity"]
        .as_u64()
        .ok_or_else(|| BrpError {
            code: -32602,
            message: "entity must be a u64".into(),
            data: None,
        })?;

    let entity = Entity::try_from_bits(entity_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;

    let entity_ref = world
        .get_entity(entity)
        .map_err(|_| BrpError {
            code: -32602,
            message: format!("entity {entity_id} not found"),
            data: None,
        })?;

    let health = entity_ref
        .get::<Health>()
        .ok_or_else(|| BrpError {
            code: -32602,
            message: "entity has no Health component".into(),
            data: None,
        })?;

    let is_dead = entity_ref.get::<combat::Dead>().is_some();
    let is_pinned = entity_ref.get::<combat::Pinned>().is_some();
    let is_routing = entity_ref.get::<combat::Routing>().is_some();
    let supp = entity_ref.get::<combat::Suppression>();
    let in_cover = entity_ref.get::<combat::InCover>();
    let morale = entity_ref.get::<Morale>();
    let morale_st = morale.map(|m| morale_state(m));

    Ok(serde_json::json!({
        "health_current": health.current,
        "health_max": health.max,
        "is_dead": is_dead,
        "is_pinned": is_pinned,
        "is_routing": is_routing,
        "suppression_current": supp.map(|s| s.current),
        "suppression_max": supp.map(|s| s.max),
        "cover": in_cover.map(|c| format!("{:?}", c.density)),
        "morale_current": morale.map(|m| m.current),
        "morale_max": morale.map(|m| m.max),
        "morale_state": morale_st.map(|s| format!("{:?}", s)),
    }))
}

/// BRP handler for "faction/spawn": { faction: "combine"|"covenant"|"ironborn"|"hollow" }
/// Spawns a Faction entity with default ResourcePool/Caps/Trickle.
fn handle_faction_spawn(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;

    let name = params["faction"]
        .as_str()
        .ok_or_else(|| BrpError {
            code: -32602,
            message: "faction must be a string".into(),
            data: None,
        })?;

    let faction = Faction::new(name);
    let id = world.spawn(FactionBundle::new(faction)).id().to_bits();
    Ok(serde_json::json!({ "entity_id": id }))
}

/// BRP handler for "resources/status": { entity }
fn handle_resources_status(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;

    let entity_id = params["entity"]
        .as_u64()
        .ok_or_else(|| BrpError {
            code: -32602,
            message: "entity must be a u64".into(),
            data: None,
        })?;

    let entity = Entity::try_from_bits(entity_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;

    let entity_ref = world.get_entity(entity).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;

    let pool = entity_ref.get::<ResourcePool>().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity has no ResourcePool".into(),
        data: None,
    })?;
    let trickle = entity_ref.get::<ResourceTrickle>();

    let pop = entity_ref.get::<resources::PopCap>();

    Ok(serde_json::json!({
        "fuel": pool.fuel,
        "scrap": pool.scrap,
        "manpower": pool.manpower,
        "fuel_trickle": trickle.map(|t| t.fuel_per_second),
        "scrap_trickle": trickle.map(|t| t.scrap_per_second),
        "manpower_trickle": trickle.map(|t| t.manpower_per_second),
        "pop_current": pop.map(|p| p.current),
        "pop_max": pop.map(|p| p.max),
    }))
}

/// BRP handler for "resources/spend": { entity, fuel, scrap, manpower }
/// Returns { success: bool }. Used primarily for tests; production will
/// gate this behind building/research costs.
fn handle_resources_spend(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;

    let entity_id = params["entity"]
        .as_u64()
        .ok_or_else(|| BrpError {
            code: -32602,
            message: "entity must be a u64".into(),
            data: None,
        })?;

    let cost = ResourceCost {
        fuel: params["fuel"].as_f64().unwrap_or(0.0) as f32,
        scrap: params["scrap"].as_f64().unwrap_or(0.0) as f32,
        manpower: params["manpower"].as_f64().unwrap_or(0.0) as f32,
    };

    let entity = Entity::try_from_bits(entity_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;

    let mut entity_mut = world.get_entity_mut(entity).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;

    let mut pool = entity_mut.get_mut::<ResourcePool>().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity has no ResourcePool".into(),
        data: None,
    })?;

    let success = spend(&mut pool, &cost);
    Ok(serde_json::json!({ "success": success }))
}

fn parse_building_type(s: &str) -> Result<BuildingTypeId, BrpError> {
    // Accept any lowercase kebab-case building type string
    Ok(BuildingTypeId::new(s))
}

fn normalize_unit_type_id(s: &str) -> String {
    match s.to_lowercase().replace(' ', "_").as_str() {
        "rifleman" | "riflemen" => "riflemen".to_string(),
        "heavy_weapons" | "heavyweapons" | "heavy_weapon" => "heavy_weapons".to_string(),
        "light_vehicle" | "lightvehicle" => "light_vehicle".to_string(),
        "heavy_armor" | "heavyarmor" | "heavy_armour" | "heavyarmour" => "heavy_armor".to_string(),
        other => other.to_string(),
    }
}

/// BRP handler for "building/place": { faction_entity, building_type, x, y }
/// Validates affordability + non-overlap, deducts resources, spawns the
/// building under construction, returns { entity_id }.
fn handle_building_place(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;

    let faction_id = params["faction_entity"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "faction_entity must be a u64".into(),
        data: None,
    })?;
    let building_type = parse_building_type(params["building_type"].as_str().ok_or_else(
        || BrpError {
            code: -32602,
            message: "building_type must be a string".into(),
            data: None,
        },
    )?)?;
    let x = params["x"].as_i64().ok_or_else(|| BrpError {
        code: -32602,
        message: "x must be an integer".into(),
        data: None,
    })? as i32;
    let y = params["y"].as_i64().ok_or_else(|| BrpError {
        code: -32602,
        message: "y must be an integer".into(),
        data: None,
    })? as i32;

    let target = GridPos { x, y };

    // Collect occupied positions from existing buildings.
    let mut occupied: std::collections::HashSet<GridPos> = std::collections::HashSet::new();
    {
        let mut existing = world.query::<&BuildingPos>();
        for bp in existing.iter(world) {
            occupied.insert(bp.pos.clone());
        }
    }

    let blocked: std::collections::HashSet<GridPos> = std::collections::HashSet::new();
    if !can_place(&target, &occupied, &blocked) {
        return Err(BrpError {
            code: -32000,
            message: "tile is occupied".into(),
            data: None,
        });
    }

    let faction_entity = Entity::try_from_bits(faction_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("faction entity {faction_id} not found"),
        data: None,
    })?;

    let loaded = world.resource::<LoadedFactions>().clone();
    let cost = building_cost(&building_type, &loaded);

    // Look up the faction enum on the FactionEntity component.
    let faction = {
        let f_ref = world.get_entity(faction_entity).map_err(|_| BrpError {
            code: -32602,
            message: format!("faction entity {faction_id} not found"),
            data: None,
        })?;
        f_ref
            .get::<resources::FactionEntity>()
            .map(|fe| fe.faction.clone())
            .ok_or_else(|| BrpError {
                code: -32602,
                message: "entity is not a Faction".into(),
                data: None,
            })?
    };

    // Pay
    {
        let mut f_mut = world.get_entity_mut(faction_entity).map_err(|_| BrpError {
            code: -32602,
            message: format!("faction entity {faction_id} not found"),
            data: None,
        })?;
        let mut pool = f_mut.get_mut::<resources::ResourcePool>().ok_or_else(|| BrpError {
            code: -32602,
            message: "faction has no ResourcePool".into(),
            data: None,
        })?;
        try_pay(&mut pool, &cost).map_err(|_| BrpError {
            code: -32000,
            message: "insufficient resources".into(),
            data: None,
        })?;
    }

    // Look up TechBuff for this faction to apply build_time_mult
    let build_time_mult = {
        let mut q = world.query::<(&resources::FactionEntity, &tech::TechBuff)>();
        q.iter(world)
            .find(|(fe, _)| fe.faction == faction)
            .map(|(_, b)| b.build_time_mult)
            .unwrap_or(1.0)
    };

    let bt_for_query = building_type.clone();
    let mut bundle = BuildingBundle::new(building_type, faction, x, y, &loaded);
    bundle.construction.total *= build_time_mult;
    let entity = world.spawn(bundle).id();

    if !building_produces(&bt_for_query, &loaded).is_empty() {
        world.entity_mut(entity).insert(ProductionQueue::default());
    }

    Ok(serde_json::json!({ "entity_id": entity.to_bits() }))
}

/// BRP handler for "building/status": { entity }
fn handle_building_status(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let entity_id = params["entity"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity must be a u64".into(),
        data: None,
    })?;
    let entity = Entity::try_from_bits(entity_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;
    let er = world.get_entity(entity).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;

    let bt = er.get::<BuildingTypeId>().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity is not a building".into(),
        data: None,
    })?;
    let pos = er.get::<BuildingPos>();
    let h = er.get::<Health>();
    let cp = er.get::<ConstructionProgress>();
    let faction = er.get::<map::Faction>();
    let built = er.get::<Built>().is_some();
    let under = er.get::<UnderConstruction>().is_some();

    Ok(serde_json::json!({
        "building_type": bt.id(),
        "pos_x": pos.map(|p| p.pos.x),
        "pos_y": pos.map(|p| p.pos.y),
        "faction": faction.map(|f| f.id().to_string()),
        "health_current": h.map(|h| h.current),
        "health_max": h.map(|h| h.max),
        "construction_elapsed": cp.map(|c| c.elapsed),
        "construction_total": cp.map(|c| c.total),
        "built": built,
        "under_construction": under,
    }))
}

fn parse_faction(s: &str) -> Result<Faction, BrpError> {
    Ok(Faction::new(&s.to_lowercase()))
}

fn parse_playable_faction(s: &str) -> Result<PlayableFaction, BrpError> {
    match s {
        "combine" => Ok(PlayableFaction::Combine),
        "ironborn" => Ok(PlayableFaction::Ironborn),
        "handler" => Ok(PlayableFaction::Handler),
        other => Err(BrpError {
            code: -32602,
            message: format!("unknown playable faction: {other} (valid: combine, ironborn, handler)"),
            data: None,
        }),
    }
}

fn playable_to_map_faction(f: &PlayableFaction) -> Faction {
    match f {
        PlayableFaction::Combine => Faction::combine(),
        PlayableFaction::Ironborn => Faction::ironborn(),
        PlayableFaction::Handler => Faction::combine(),
    }
}

fn parse_point_type(s: &str) -> Result<ControlPointType, BrpError> {
    match s {
        "strategic" => Ok(ControlPointType::Strategic),
        "fuel_depot" => Ok(ControlPointType::FuelDepot),
        "scrap_field" => Ok(ControlPointType::ScrapField),
        "high_ground" => Ok(ControlPointType::HighGround),
        "ancient_ruins" => Ok(ControlPointType::AncientRuins),
        other => Err(BrpError {
            code: -32602,
            message: format!("unknown point type: {other}"),
            data: None,
        }),
    }
}

/// BRP handler for "point/spawn": { point_type, x, y, radius?: f32 }
fn handle_point_spawn(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;

    let point_type = parse_point_type(params["point_type"].as_str().ok_or_else(|| BrpError {
        code: -32602,
        message: "point_type must be a string".into(),
        data: None,
    })?)?;

    let x = params["x"].as_i64().ok_or_else(|| BrpError {
        code: -32602,
        message: "x must be an integer".into(),
        data: None,
    })? as i32;
    let y = params["y"].as_i64().ok_or_else(|| BrpError {
        code: -32602,
        message: "y must be an integer".into(),
        data: None,
    })? as i32;
    let radius = params["radius"].as_f64().unwrap_or(2.0) as f32;

    let id = world
        .spawn(ControlPoint {
            point_type,
            pos: GridPos { x, y },
            capture_radius: radius,
            owner: None,
            contesting: None,
            capture_progress: 0.0,
        })
        .id()
        .to_bits();
    Ok(serde_json::json!({ "entity_id": id }))
}

/// BRP handler for "point/status": { entity }
fn handle_point_status(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;

    let entity_id = params["entity"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity must be a u64".into(),
        data: None,
    })?;

    let entity = Entity::try_from_bits(entity_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;

    let entity_ref = world.get_entity(entity).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;

    let cp = entity_ref.get::<ControlPoint>().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity is not a ControlPoint".into(),
        data: None,
    })?;

    Ok(serde_json::json!({
        "point_type": format!("{:?}", cp.point_type),
        "pos_x": cp.pos.x,
        "pos_y": cp.pos.y,
        "capture_radius": cp.capture_radius,
        "owner": cp.owner.as_ref().map(|f| f.id().to_string()),
        "contesting": cp.contesting.as_ref().map(|f| f.id().to_string()),
        "capture_progress": cp.capture_progress,
    }))
}


/// BRP handler for "production/enqueue": { faction_entity, building_entity, unit_type }
fn handle_production_enqueue(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;

    let faction_id = params["faction_entity"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "faction_entity required".into(),
        data: None,
    })?;
    let building_id = params["building_entity"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "building_entity required".into(),
        data: None,
    })?;
    let unit_type_raw = params["unit_type"].as_str().ok_or_else(|| BrpError {
        code: -32602,
        message: "unit_type required".into(),
        data: None,
    })?;
    let unit_type = normalize_unit_type_id(unit_type_raw);

    let faction_entity = Entity::try_from_bits(faction_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("faction entity {faction_id} not found"),
        data: None,
    })?;
    let building_entity = Entity::try_from_bits(building_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("building entity {building_id} not found"),
        data: None,
    })?;

    // Snapshot building state.
    let (bt, is_built) = {
        let r = world.get_entity(building_entity).map_err(|_| BrpError {
            code: -32602,
            message: format!("building entity {building_id} not found"),
            data: None,
        })?;
        let bt = r.get::<BuildingTypeId>().cloned().ok_or_else(|| BrpError {
            code: -32602,
            message: "entity is not a building".into(),
            data: None,
        })?;
        let is_built = r.get::<Built>().is_some();
        (bt, is_built)
    };

    // Pay from faction pool, then enqueue on building.
    let loaded = world.resource::<LoadedFactions>().clone();
    let cost = production::unit_production_cost(&unit_type, &loaded);
    {
        let mut fm = world.get_entity_mut(faction_entity).map_err(|_| BrpError {
            code: -32602,
            message: format!("faction entity {faction_id} not found"),
            data: None,
        })?;
        let mut pool = fm.get_mut::<resources::ResourcePool>().ok_or_else(|| BrpError {
            code: -32602,
            message: "faction has no ResourcePool".into(),
            data: None,
        })?;
        // Reuse the pure path with a temp queue check first.
        if !is_built {
            return Err(BrpError {
                code: -32000,
                message: "building not finished".into(),
                data: None,
            });
        }
        if !building_produces(&bt, &loaded).iter().any(|s| *s == unit_type.as_str()) {
            return Err(BrpError {
                code: -32000,
                message: format!("{} cannot produce {}", bt.id(), unit_type),
                data: None,
            });
        }
        if !resources::can_afford(&pool, &cost) {
            return Err(BrpError {
                code: -32000,
                message: "insufficient resources".into(),
                data: None,
            });
        }
        // Pop cap check: deny if current + already-queued >= max.
        // Note: 'current' lags by one frame relative to spawned units, so
        // we conservatively also count this faction's queued jobs across
        // its buildings.
        let pop = fm.get::<resources::PopCap>().cloned();
        drop(fm);
        if let Some(pc) = pop {
            // Tally queued jobs across all this faction's buildings.
            let mut queued_total: u32 = 0;
            let mut bq = world.query::<(&map::Faction, &ProductionQueue)>();
            for (bf, q) in bq.iter(world) {
                // We don't know the requesting faction's enum without re-fetching;
                // if this entity matches the request via faction_entity comparison
                // by enum, count it.
                let target_faction_enum = {
                    let r = world.get_entity(faction_entity).map_err(|_| BrpError {
                        code: -32602, message: "faction lost".into(), data: None,
                    })?;
                    r.get::<resources::FactionEntity>().map(|fe| fe.faction.clone()).unwrap_or(bf.clone())
                };
                if bf == &target_faction_enum {
                    queued_total += q.jobs.len() as u32;
                }
            }
            if pc.current + queued_total >= pc.max {
                return Err(BrpError {
                    code: -32000,
                    message: format!(
                        "pop cap reached: {}/{}",
                        pc.current + queued_total,
                        pc.max
                    ),
                    data: None,
                });
            }
        }
        // Re-acquire faction mut for spend.
        let mut fm = world.get_entity_mut(faction_entity).map_err(|_| BrpError {
            code: -32602, message: "faction lost".into(), data: None,
        })?;
        let mut pool = fm.get_mut::<resources::ResourcePool>().ok_or_else(|| BrpError {
            code: -32000, message: "no pool".into(), data: None,
        })?;
        resources::spend(&mut pool, &cost);
    }

    // Append to queue.
    let mut bm = world.get_entity_mut(building_entity).map_err(|_| BrpError {
        code: -32602,
        message: format!("building entity {building_id} not found"),
        data: None,
    })?;
    let mut queue = bm.get_mut::<ProductionQueue>().ok_or_else(|| BrpError {
        code: -32000,
        message: "building has no ProductionQueue".into(),
        data: None,
    })?;
    if queue.jobs.len() >= production::QUEUE_CAP {
        // Refund.
        drop(queue);
        let mut fm = world.entity_mut(faction_entity);
        if let Some(mut pool) = fm.get_mut::<resources::ResourcePool>() {
            resources::refund(&mut pool, &cost);
        }
        return Err(BrpError {
            code: -32000,
            message: "queue is full".into(),
            data: None,
        });
    }
    queue.jobs.push(unit_type.clone());

    // Suppress unused-import warning when EnqueueError isn't matched here.
    let _: Option<EnqueueError> = None;
    let _ = try_enqueue;

    Ok(serde_json::json!({ "queued": queue.jobs.len() }))
}

/// BRP handler for "production/queue_status": { entity }
fn handle_production_queue_status(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let entity_id = params["entity"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity required".into(),
        data: None,
    })?;
    let entity = Entity::try_from_bits(entity_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;
    let r = world.get_entity(entity).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;
    let q = r.get::<ProductionQueue>().ok_or_else(|| BrpError {
        code: -32000,
        message: "entity has no ProductionQueue".into(),
        data: None,
    })?;

    let jobs: Vec<String> = q.jobs.iter().cloned().collect();
    let progress = q.progress;
    let head_unit = q.jobs.first().cloned();
    drop(r);

    let loaded = world.resource::<LoadedFactions>().clone();
    let head_total = head_unit.as_deref().map(|u| production::unit_production_seconds(u, &loaded));
    Ok(serde_json::json!({
        "jobs": jobs,
        "progress": progress,
        "head_total": head_total,
    }))
}

fn parse_research_target(target: &str) -> Result<ResearchTarget, BrpError> {
    match target {
        "tier_2" => Ok(ResearchTarget::Tier(Tier::Two)),
        "tier_3" => Ok(ResearchTarget::Tier(Tier::Three)),
        "doctrine_assault" => Ok(ResearchTarget::Doctrine(Doctrine::Assault)),
        "doctrine_fortification" => Ok(ResearchTarget::Doctrine(Doctrine::Fortification)),
        "doctrine_salvage" => Ok(ResearchTarget::Doctrine(Doctrine::Salvage)),
        other => Err(BrpError {
            code: -32602,
            message: format!("unknown tech target: {other}"),
            data: None,
        }),
    }
}

/// BRP handler for "tech/research": { faction_entity, target }
fn handle_tech_research(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let faction_id = params["faction_entity"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "faction_entity required".into(),
        data: None,
    })?;
    let target = parse_research_target(params["target"].as_str().ok_or_else(|| BrpError {
        code: -32602,
        message: "target required".into(),
        data: None,
    })?)?;

    let faction_entity = Entity::try_from_bits(faction_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("faction entity {faction_id} not found"),
        data: None,
    })?;

    // Snapshot tech and in_progress flag.
    let (tech_snapshot, in_progress) = {
        let r = world.get_entity(faction_entity).map_err(|_| BrpError {
            code: -32602,
            message: format!("faction entity {faction_id} not found"),
            data: None,
        })?;
        let t = r.get::<Tech>().cloned().ok_or_else(|| BrpError {
            code: -32602,
            message: "entity has no Tech".into(),
            data: None,
        })?;
        (t, r.get::<ResearchInProgress>().is_some())
    };

    let rip = {
        let mut em = world.get_entity_mut(faction_entity).map_err(|_| BrpError {
            code: -32602,
            message: format!("faction entity {faction_id} not found"),
            data: None,
        })?;
        let mut pool = em.get_mut::<resources::ResourcePool>().ok_or_else(|| BrpError {
            code: -32602,
            message: "entity has no ResourcePool".into(),
            data: None,
        })?;
        start_research(&mut pool, &tech_snapshot, in_progress, target).map_err(|e| BrpError {
            code: -32000,
            message: format!("{:?}", e),
            data: None,
        })?
    };
    world.entity_mut(faction_entity).insert(rip);
    Ok(serde_json::json!({ "started": true }))
}

/// BRP handler for "tech/status": { faction_entity }
fn handle_tech_status(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let faction_id = params["faction_entity"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "faction_entity required".into(),
        data: None,
    })?;
    let entity = Entity::try_from_bits(faction_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("faction entity {faction_id} not found"),
        data: None,
    })?;
    let r = world.get_entity(entity).map_err(|_| BrpError {
        code: -32602,
        message: format!("faction entity {faction_id} not found"),
        data: None,
    })?;
    let tech = r.get::<Tech>().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity has no Tech".into(),
        data: None,
    })?;
    let rip = r.get::<ResearchInProgress>();

    Ok(serde_json::json!({
        "tier": format!("{:?}", tech.tier),
        "doctrine": tech.doctrine.map(|d| format!("{:?}", d)),
        "researching": rip.map(|r| format!("{:?}", r.target)),
        "research_elapsed": rip.map(|r| r.elapsed),
        "research_total": rip.map(|r| r.total),
    }))
}

fn parse_ability_kind(s: &str) -> Result<AbilityKind, BrpError> {
    match s {
        "rally" => Ok(AbilityKind::Rally),
        "area_damage" => Ok(AbilityKind::AreaDamage),
        other => Err(BrpError {
            code: -32602,
            message: format!("unknown ability_kind: {other}"),
            data: None,
        }),
    }
}

/// BRP handler for "hero/spawn": { name, faction, x, y, ability_kind? }
fn handle_hero_spawn(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let name = params["name"].as_str().unwrap_or("Hero").to_string();
    let faction = parse_faction(params["faction"].as_str().unwrap_or("combine"))?;
    let x = params["x"].as_i64().unwrap_or(0) as i32;
    let y = params["y"].as_i64().unwrap_or(0) as i32;
    let kind = parse_ability_kind(params["ability_kind"].as_str().unwrap_or("rally"))?;
    let entity = world.spawn(HeroBundle::new(name, x, y, faction, kind)).id();
    world
        .entity_mut(entity)
        .insert(units::HomeBase { pos: GridPos { x, y } });
    Ok(serde_json::json!({ "entity_id": entity.to_bits() }))
}

/// BRP handler for "hero/status": { entity }
fn handle_hero_status(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let entity_id = params["entity"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity required".into(),
        data: None,
    })?;
    let entity = Entity::try_from_bits(entity_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;
    let r = world.get_entity(entity).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;
    let hero = r.get::<Hero>().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity is not a hero".into(),
        data: None,
    })?;
    let h = r.get::<Health>();
    let s = r.get::<SignatureAbility>();
    let a = r.get::<Aura>();
    let downed = r.get::<HeroDowned>().is_some();
    let pos = r.get::<units::UnitPos>();

    Ok(serde_json::json!({
        "name": hero.name,
        "downed": downed,
        "pos_x": pos.map(|p| p.pos.x),
        "pos_y": pos.map(|p| p.pos.y),
        "health_current": h.map(|h| h.current),
        "health_max": h.map(|h| h.max),
        "charge": s.map(|s| s.charge),
        "max_charge": s.map(|s| s.max_charge),
        "charge_full": s.map(is_charge_full),
        "ability_kind": s.map(|s| format!("{:?}", s.kind)),
        "aura_radius": a.map(|a| a.radius),
        "aura_suppression_resist": a.map(|a| a.suppression_resist),
    }))
}

/// BRP handler for "hero/ability_use": { entity }
/// If charge is full, applies the ability and resets charge.
fn handle_hero_ability_use(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let entity_id = params["entity"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity required".into(),
        data: None,
    })?;
    let entity = Entity::try_from_bits(entity_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;

    let (kind, hero_pos, hero_faction, aura_radius) = {
        let r = world.get_entity(entity).map_err(|_| BrpError {
            code: -32602,
            message: format!("entity {entity_id} not found"),
            data: None,
        })?;
        if r.get::<Hero>().is_none() {
            return Err(BrpError {
                code: -32000,
                message: "entity is not a hero".into(),
                data: None,
            });
        }
        if r.get::<HeroDowned>().is_some() {
            return Err(BrpError {
                code: -32000,
                message: "hero is downed".into(),
                data: None,
            });
        }
        let s = r.get::<SignatureAbility>().ok_or_else(|| BrpError {
            code: -32000,
            message: "hero has no SignatureAbility".into(),
            data: None,
        })?;
        if !is_charge_full(s) {
            return Err(BrpError {
                code: -32000,
                message: format!("ability not ready: {}/{}", s.charge, s.max_charge),
                data: None,
            });
        }
        let pos = r
            .get::<units::UnitPos>()
            .map(|p| p.pos.clone())
            .ok_or_else(|| BrpError {
                code: -32000,
                message: "hero has no UnitPos".into(),
                data: None,
            })?;
        let faction = r
            .get::<Faction>()
            .cloned()
            .ok_or_else(|| BrpError {
                code: -32000,
                message: "hero has no Faction".into(),
                data: None,
            })?;
        let aura = r.get::<Aura>().map(|a| a.radius).unwrap_or(5.0);
        (s.kind.clone(), pos, faction, aura)
    };

    match kind {
        AbilityKind::Rally => {
            // Clear suppression on allied units in aura radius.
            let mut targets: Vec<Entity> = Vec::new();
            {
                let mut q = world.query::<(Entity, &units::UnitPos, &Faction, &Suppression)>();
                for (e, p, f, _) in q.iter(world) {
                    if f == &hero_faction && within_aura(&hero_pos, &p.pos, aura_radius) {
                        targets.push(e);
                    }
                }
            }
            for t in &targets {
                if let Ok(mut em) = world.get_entity_mut(*t) {
                    if let Some(mut s) = em.get_mut::<Suppression>() {
                        s.current = 0.0;
                    }
                }
            }
        }
        AbilityKind::AreaDamage => {
            let mut targets: Vec<Entity> = Vec::new();
            {
                let mut q = world.query::<(Entity, &units::UnitPos, &Faction, &Health)>();
                for (e, p, f, _) in q.iter(world) {
                    if f != &hero_faction && within_aura(&hero_pos, &p.pos, aura_radius) {
                        targets.push(e);
                    }
                }
            }
            for t in &targets {
                if let Ok(mut em) = world.get_entity_mut(*t) {
                    if let Some(mut h) = em.get_mut::<Health>() {
                        h.current = (h.current - 100.0).max(0.0);
                    }
                }
            }
        }
    }

    // Reset charge
    if let Ok(mut em) = world.get_entity_mut(entity) {
        if let Some(mut s) = em.get_mut::<SignatureAbility>() {
            s.charge = 0.0;
        }
    }

    Ok(serde_json::json!({ "fired": true }))
}

/// Despawn all gameplay entities and clear gameplay resources. Shared by
/// dev/reset and game/new.
pub fn wipe_world_entities(world: &mut World) {
    let mut to_despawn: Vec<Entity> = Vec::new();
    {
        let mut q = world.query_filtered::<Entity, Or<(
            With<resources::FactionEntity>,
            With<units::UnitTypeId>,
            With<map::ControlPoint>,
            With<BuildingTypeId>,
            With<Hero>,
            With<map::Tile>,
            With<Mission>,
            With<HollowSpawner>,
        )>>();
        for e in q.iter(world) {
            to_despawn.push(e);
        }
    }
    for e in to_despawn {
        world.despawn(e);
    }
    if let Some(mut fired) = world.get_resource_mut::<FiredBeats>() {
        fired.0.clear();
    }
}

fn game_state_json(world: &mut World) -> Value {
    let state_str = match world.resource::<GameState>() {
        GameState::Title => "Title".to_string(),
        GameState::Campaign => "Campaign".to_string(),
        GameState::InMission => "InMission".to_string(),
        GameState::GameOver { won, handler_unlocked } => format!(
            "GameOver({},handler_unlocked={})",
            if *won { "won" } else { "lost" },
            handler_unlocked,
        ),
    };
    let active = world.resource::<ActiveRun>().clone();
    let player_str = active.run.as_ref().map(|r| format!("{:?}", r.faction));
    let current_mission_index = active.run.as_ref().map(|r| r.current_mission);
    let current_mission_entity = active.current_mission_entity.map(|e| e.to_bits());
    serde_json::json!({
        "state": state_str,
        "missions_won": active.missions_won,
        "missions_lost": active.missions_lost,
        "current_mission_index": current_mission_index,
        "current_mission_entity": current_mission_entity,
        "player": player_str,
    })
}

/// BRP handler for "game/state": current state machine + run progress.
fn handle_game_state(In(_params): In<Option<Value>>, world: &mut World) -> BrpResult {
    Ok(game_state_json(world))
}

/// BRP handler for "game/new": { player }
/// Wipes any current world, initializes a fresh CampaignRun for the chosen faction,
/// sets GameState=Campaign, returns the new state snapshot.
fn handle_game_new(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let player_str = params
        .as_ref()
        .and_then(|p| p["player"].as_str())
        .unwrap_or("combine")
        .to_string();
    let faction = parse_playable_faction(&player_str)?;
    let map_faction = playable_to_map_faction(&faction);

    wipe_world_entities(world);
    *world.resource_mut::<ActiveRun>() = ActiveRun {
        run: Some(CampaignRun {
            faction,
            current_mission: 0,
            outcomes: Vec::new(),
            complete: false,
            campaign_id: String::new(),
            mission_maps: Vec::new(),
        }),
        current_mission_entity: None,
        missions_won: 0,
        missions_lost: 0,
    };
    *world.resource_mut::<GameState>() = GameState::Campaign;

    world.spawn(FactionBundle::new(map_faction));

    Ok(game_state_json(world))
}

/// BRP handler for "game/save": { slot } — persists GameState + ActiveRun + GlobalProgress.
fn handle_game_save(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602, message: "missing params".into(), data: None,
    })?;
    let slot = params["slot"].as_str().unwrap_or("default").to_string();

    let mut snapshot = snapshot_world(world);
    let state_label = match world.resource::<GameState>() {
        GameState::Title => "Title".to_string(),
        GameState::Campaign => "Campaign".to_string(),
        GameState::InMission => "InMission".to_string(),
        GameState::GameOver { won, handler_unlocked } => format!(
            "GameOver({},{})",
            if *won { "won" } else { "lost" },
            handler_unlocked,
        ),
    };
    let active = world.resource::<ActiveRun>().clone();
    let progress = world.resource::<GlobalProgress>().clone();
    snapshot["game_state"] = serde_json::json!(state_label);
    snapshot["active_run"] = serde_json::json!({
        "missions_won": active.missions_won,
        "missions_lost": active.missions_lost,
        "run": active.run.as_ref().map(|r| serde_json::json!({
            "faction": format!("{:?}", r.faction),
            "current_mission": r.current_mission,
            "complete": r.complete,
            "outcomes": r.outcomes.iter().map(|o| serde_json::json!({
                "mission_index": o.mission_index,
                "won": o.won,
                "mission_type": format!("{:?}", o.mission_type),
            })).collect::<Vec<_>>(),
        })),
    });
    snapshot["global_progress"] = serde_json::json!({
        "combine_beaten": progress.combine_beaten,
        "ironborn_beaten": progress.ironborn_beaten,
        "handler_unlocked": progress.handler_unlocked,
        "handler_beaten": progress.handler_beaten,
        "first_beaten": progress.first_beaten.as_ref().map(|f| format!("{:?}", f)),
    });

    let mut slots = world.get_resource_or_insert_with(SaveSlots::default);
    slots.0.insert(slot.clone(), snapshot);
    Ok(serde_json::json!({ "saved": slot }))
}

/// BRP handler for "game/load": { slot }
fn handle_game_load(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602, message: "missing params".into(), data: None,
    })?;
    let slot = params["slot"].as_str().unwrap_or("default").to_string();

    let snapshot = {
        let slots = world.get_resource::<SaveSlots>().ok_or_else(|| BrpError {
            code: -32000, message: "no save slots".into(), data: None,
        })?;
        slots.0.get(&slot).cloned().ok_or_else(|| BrpError {
            code: -32000, message: format!("no slot named {slot}"), data: None,
        })?
    };

    let _restored = restore_world(world, &snapshot)?;

    if let Some(s) = snapshot["game_state"].as_str() {
        let new_state = if s.starts_with("GameOver") {
            let won = s.contains("won");
            let handler_unlocked = s.contains("true");
            GameState::GameOver { won, handler_unlocked }
        } else {
            match s {
                "Campaign" => GameState::Campaign,
                "InMission" => GameState::Campaign,
                _ => GameState::Title,
            }
        };
        *world.resource_mut::<GameState>() = new_state;
    }
    if !snapshot["active_run"].is_null() {
        let ar = &snapshot["active_run"];
        let run = if ar["run"].is_null() {
            None
        } else {
            let r = &ar["run"];
            let faction = parse_playable_faction(
                &r["faction"].as_str().unwrap_or("combine").to_lowercase()
            ).unwrap_or(PlayableFaction::Combine);
            let outcomes = r["outcomes"]
                .as_array()
                .map(|arr| arr.iter().map(|o| campaign::MissionOutcome {
                    mission_index: o["mission_index"].as_u64().unwrap_or(0) as usize,
                    won: o["won"].as_bool().unwrap_or(false),
                    mission_type: parse_mission_type(
                        &o["mission_type"].as_str().unwrap_or("assault").to_lowercase()
                    ).unwrap_or(crate::mapgen::MissionType::Assault),
                }).collect())
                .unwrap_or_default();
            Some(CampaignRun {
                faction,
                current_mission: r["current_mission"].as_u64().unwrap_or(0) as usize,
                complete: r["complete"].as_bool().unwrap_or(false),
                outcomes,
                campaign_id: String::new(),
                mission_maps: Vec::new(),
            })
        };
        *world.resource_mut::<ActiveRun>() = ActiveRun {
            run,
            current_mission_entity: None,
            missions_won: ar["missions_won"].as_u64().unwrap_or(0) as u32,
            missions_lost: ar["missions_lost"].as_u64().unwrap_or(0) as u32,
        };
    }
    if !snapshot["global_progress"].is_null() {
        let gp = &snapshot["global_progress"];
        let first_beaten = gp["first_beaten"].as_str()
            .and_then(|s| parse_playable_faction(&s.to_lowercase()).ok());
        *world.resource_mut::<GlobalProgress>() = GlobalProgress {
            combine_beaten: gp["combine_beaten"].as_bool().unwrap_or(false),
            ironborn_beaten: gp["ironborn_beaten"].as_bool().unwrap_or(false),
            handler_unlocked: gp["handler_unlocked"].as_bool().unwrap_or(false),
            handler_beaten: gp["handler_beaten"].as_bool().unwrap_or(false),
            first_beaten,
            campaigns_beaten: Default::default(),
            missions_reached: Default::default(),
        };
    }

    Ok(game_state_json(world))
}

/// BRP handler for "game/exit": fires AppExit to terminate the process.
fn handle_game_exit(In(_params): In<Option<Value>>, world: &mut World) -> BrpResult {
    fire_exit(world);
    Ok(serde_json::json!({ "exiting": true }))
}

/// BRP handler for "mission/force_resolve": { entity, won }
/// Sets a Mission's status directly. Mainly for tests, but also a useful
/// hook for narrative beats / scripted defeats.
fn handle_mission_force_resolve(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602, message: "missing params".into(), data: None,
    })?;
    let entity_id = params["entity"].as_u64().ok_or_else(|| BrpError {
        code: -32602, message: "entity required".into(), data: None,
    })?;
    let won = params["won"].as_bool().unwrap_or(true);
    let entity = Entity::try_from_bits(entity_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;
    let mut em = world.get_entity_mut(entity).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;
    let mut m = em.get_mut::<Mission>().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity is not a Mission".into(),
        data: None,
    })?;
    m.status = if won { MissionStatus::Won } else { MissionStatus::Lost };
    Ok(serde_json::json!({ "resolved": format!("{:?}", m.status) }))
}

/// BRP handler for "mission/select": advances to the next mission in linear sequence.
/// The zone_id parameter is accepted for API compatibility but the mission type is
/// determined by the faction sequence, not zone choice.
fn handle_mission_select(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let _params = params;

    if *world.resource::<GameState>() != GameState::Campaign {
        return Err(BrpError {
            code: -32000,
            message: "must be in Campaign state to select a mission".into(),
            data: None,
        });
    }

    let (mission_type, map_faction, current_mission_index) = {
        let active = world.resource::<ActiveRun>();
        let run = active.run.as_ref().ok_or_else(|| BrpError {
            code: -32000,
            message: "no active campaign run".into(),
            data: None,
        })?;
        let mt = next_mission_type(run).ok_or_else(|| BrpError {
            code: -32000,
            message: "campaign is already complete".into(),
            data: None,
        })?;
        let mf = playable_to_map_faction(&run.faction);
        let idx = run.current_mission;
        (mt, mf, idx)
    };

    let opponent = match map_faction.id() {
        "combine" => Faction::ironborn(),
        "ironborn" => Faction::combine(),
        _ => Faction::hollow(),
    };

    let mission_entity = world
        .spawn(Mission {
            mission_type: mission_type.clone(),
            player_faction: map_faction.clone(),
            opponent_faction: opponent.clone(),
            status: MissionStatus::Active,
            elapsed: 0.0,
            deadline: 300.0,
            hill_timer: 0.0,
            hill_threshold: 180.0,
            assassination_target: None,
            ffa_check_timer: 0.0,
        })
        .id();

    setup_demo_scenario(world, &map_faction, current_mission_index);

    // Bake NavMesh after tiles are spawned.
    bake_navmesh(world);

    world.resource_mut::<ActiveRun>().current_mission_entity = Some(mission_entity);
    *world.resource_mut::<GameState>() = GameState::InMission;

    Ok(serde_json::json!({
        "mission_entity": mission_entity.to_bits(),
        "mission_type": format!("{:?}", mission_type),
        "opponent": opponent.id(),
    }))
}

fn snapshot_world(world: &mut World) -> Value {
    let mut tiles: Vec<Value> = Vec::new();
    {
        let mut q = world.query::<&map::Tile>();
        for t in q.iter(world) {
            tiles.push(serde_json::json!({
                "x": t.pos.x, "y": t.pos.y,
                "terrain": editor::terrain_name(&t.terrain_type),
                "cover": editor::cover_name(&t.cover),
            }));
        }
    }
    let mut factions: Vec<Value> = Vec::new();
    {
        let mut q = world.query::<(&resources::FactionEntity, &resources::ResourcePool)>();
        for (fe, pool) in q.iter(world) {
            factions.push(serde_json::json!({
                "faction": fe.faction.id(),
                "fuel": pool.fuel,
                "scrap": pool.scrap,
                "manpower": pool.manpower,
            }));
        }
    }
    let mut units: Vec<Value> = Vec::new();
    {
        let mut q = world.query::<(&units::UnitTypeId, &units::UnitPos, &map::Faction, &Health)>();
        for (ut, pos, faction, h) in q.iter(world) {
            units.push(serde_json::json!({
                "unit_type": ut.id(),
                "x": pos.pos.x, "y": pos.pos.y,
                "faction": faction.id(),
                "health_current": h.current,
                "health_max": h.max,
            }));
        }
    }
    serde_json::json!({
        "tiles": tiles,
        "factions": factions,
        "units": units,
    })
}

fn restore_world(world: &mut World, snapshot: &Value) -> Result<u32, BrpError> {
    // Despawn current gameplay entities (use existing dev/reset logic).
    let mut to_despawn: Vec<Entity> = Vec::new();
    {
        let mut q = world.query_filtered::<Entity, Or<(
            With<resources::FactionEntity>,
            With<units::UnitTypeId>,
            With<map::ControlPoint>,
            With<BuildingTypeId>,
            With<Hero>,
            With<map::Tile>,
            With<Mission>,
            With<HollowSpawner>,
        )>>();
        for e in q.iter(world) {
            to_despawn.push(e);
        }
    }
    for e in to_despawn {
        world.despawn(e);
    }
    if let Some(mut fired) = world.get_resource_mut::<FiredBeats>() {
        fired.0.clear();
    }

    let mut count = 0u32;

    if let Some(tiles) = snapshot["tiles"].as_array() {
        for t in tiles {
            let x = t["x"].as_i64().unwrap_or(0) as i32;
            let y = t["y"].as_i64().unwrap_or(0) as i32;
            let terrain = editor::parse_terrain(t["terrain"].as_str().unwrap_or("Grass"))
                .unwrap_or(map::TerrainType::Grass);
            let cover = editor::parse_cover(t["cover"].as_str().unwrap_or("None"))
                .unwrap_or(map::CoverDensity::None);
            world.spawn(map::Tile { pos: GridPos { x, y }, terrain_type: terrain, cover });
            count += 1;
        }
    }

    if let Some(factions) = snapshot["factions"].as_array() {
        for f in factions {
            let faction_name = f["faction"].as_str().unwrap_or("Combine").to_lowercase();
            let faction = parse_faction(&faction_name)?;
            let id = world.spawn(FactionBundle::new(faction)).id();
            if let Some(mut em) = world.get_entity_mut(id).ok() {
                if let Some(mut pool) = em.get_mut::<resources::ResourcePool>() {
                    pool.fuel = f["fuel"].as_f64().unwrap_or(0.0) as f32;
                    pool.scrap = f["scrap"].as_f64().unwrap_or(0.0) as f32;
                    pool.manpower = f["manpower"].as_f64().unwrap_or(0.0) as f32;
                }
            }
            count += 1;
        }
    }

    if let Some(units_arr) = snapshot["units"].as_array() {
        for u in units_arr {
            let x = u["x"].as_i64().unwrap_or(0) as i32;
            let y = u["y"].as_i64().unwrap_or(0) as i32;
            let faction_name = u["faction"].as_str().unwrap_or("Combine").to_lowercase();
            let faction = parse_faction(&faction_name)?;
            // For now, use default_riflemen as a fallback when restoring.
            let entity = world.spawn(UnitBundle::default_riflemen(faction, x, y)).id();
            world.entity_mut(entity).insert(units::HomeBase { pos: GridPos { x, y } });
            // Apply HP if present.
            if let Some(hc) = u["health_current"].as_f64() {
                if let Some(mut em) = world.get_entity_mut(entity).ok() {
                    if let Some(mut h) = em.get_mut::<Health>() {
                        h.current = hc as f32;
                    }
                }
            }
            count += 1;
        }
    }

    Ok(count)
}

/// BRP handler for "save/write": { slot }
fn handle_save_write(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602, message: "missing params".into(), data: None,
    })?;
    let slot = params["slot"].as_str().unwrap_or("default").to_string();
    let snapshot = snapshot_world(world);
    let mut slots = world.get_resource_or_insert_with(SaveSlots::default);
    slots.0.insert(slot.clone(), snapshot);
    Ok(serde_json::json!({ "saved": slot }))
}

/// BRP handler for "save/read": { slot }
fn handle_save_read(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602, message: "missing params".into(), data: None,
    })?;
    let slot = params["slot"].as_str().unwrap_or("default").to_string();
    let snapshot = {
        let slots = world.get_resource::<SaveSlots>().ok_or_else(|| BrpError {
            code: -32000, message: "no save slots".into(), data: None,
        })?;
        slots.0.get(&slot).cloned().ok_or_else(|| BrpError {
            code: -32000, message: format!("no slot named {slot}"), data: None,
        })?
    };
    let count = restore_world(world, &snapshot)?;
    Ok(serde_json::json!({ "restored": count }))
}

/// BRP handler for "editor/set_tile": { x, y, terrain, cover? }
/// Replaces any existing Tile at (x,y) with the new one.
fn handle_editor_set_tile(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let x = params["x"].as_i64().ok_or_else(|| BrpError {
        code: -32602, message: "x required".into(), data: None,
    })? as i32;
    let y = params["y"].as_i64().ok_or_else(|| BrpError {
        code: -32602, message: "y required".into(), data: None,
    })? as i32;
    let terrain = editor::parse_terrain(params["terrain"].as_str().unwrap_or("Grass"))
        .ok_or_else(|| BrpError {
            code: -32602, message: "unknown terrain".into(), data: None,
        })?;
    let cover = editor::parse_cover(params["cover"].as_str().unwrap_or("None"))
        .ok_or_else(|| BrpError {
            code: -32602, message: "unknown cover".into(), data: None,
        })?;

    // Despawn any existing Tile at that position.
    let mut to_remove: Vec<Entity> = Vec::new();
    {
        let mut q = world.query::<(Entity, &map::Tile)>();
        for (e, t) in q.iter(world) {
            if t.pos.x == x && t.pos.y == y {
                to_remove.push(e);
            }
        }
    }
    for e in to_remove {
        world.despawn(e);
    }
    world.spawn(map::Tile {
        pos: GridPos { x, y },
        terrain_type: terrain,
        cover,
    });
    Ok(serde_json::json!({ "success": true }))
}

/// BRP handler for "editor/save_map": returns all tiles as JSON.
fn handle_editor_save_map(In(_params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let mut tiles: Vec<serde_json::Value> = Vec::new();
    let mut q = world.query::<&map::Tile>();
    for t in q.iter(world) {
        tiles.push(serde_json::json!({
            "x": t.pos.x,
            "y": t.pos.y,
            "terrain": editor::terrain_name(&t.terrain_type),
            "cover": editor::cover_name(&t.cover),
        }));
    }
    Ok(serde_json::json!({ "tiles": tiles }))
}

/// BRP handler for "editor/load_map": { tiles: [{x, y, terrain, cover}, ...] }
/// Despawns existing tiles and spawns new ones.
fn handle_editor_load_map(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602, message: "missing params".into(), data: None,
    })?;
    let tiles = params["tiles"].as_array().ok_or_else(|| BrpError {
        code: -32602, message: "tiles array required".into(), data: None,
    })?;

    // Despawn existing tiles.
    let mut to_remove: Vec<Entity> = Vec::new();
    {
        let mut q = world.query_filtered::<Entity, With<map::Tile>>();
        for e in q.iter(world) {
            to_remove.push(e);
        }
    }
    for e in to_remove {
        world.despawn(e);
    }

    let mut spawned = 0;
    for t in tiles {
        let x = t["x"].as_i64().unwrap_or(0) as i32;
        let y = t["y"].as_i64().unwrap_or(0) as i32;
        let terrain = editor::parse_terrain(t["terrain"].as_str().unwrap_or("Grass"))
            .unwrap_or(map::TerrainType::Grass);
        let cover = editor::parse_cover(t["cover"].as_str().unwrap_or("None"))
            .unwrap_or(map::CoverDensity::None);
        world.spawn(map::Tile {
            pos: GridPos { x, y },
            terrain_type: terrain,
            cover,
        });
        spawned += 1;
    }
    Ok(serde_json::json!({ "loaded": spawned }))
}

fn parse_hollow_mode(s: &str) -> Result<HollowMode, BrpError> {
    match s {
        "consuming" => Ok(HollowMode::Consuming),
        "subsuming" => Ok(HollowMode::Subsuming),
        "indifferent" => Ok(HollowMode::Indifferent),
        other => Err(BrpError {
            code: -32602,
            message: format!("unknown hollow mode: {other}"),
            data: None,
        }),
    }
}

/// BRP handler for "hollow/spawn_point": { x, y, mode }
fn handle_hollow_spawn_point(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let x = params["x"].as_i64().unwrap_or(0) as i32;
    let y = params["y"].as_i64().unwrap_or(0) as i32;
    let mode = parse_hollow_mode(params["mode"].as_str().unwrap_or("consuming"))?;
    let id = world.spawn(HollowSpawner::new(mode, x, y)).id().to_bits();
    Ok(serde_json::json!({ "entity_id": id }))
}

/// BRP handler for "beats/fired": returns the set of authored beats that
/// have triggered so far.
fn handle_beats_fired(In(_params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let fired = world
        .get_resource::<FiredBeats>()
        .map(|f| {
            f.0.iter()
                .map(|b| format!("{:?}", b))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(serde_json::json!({ "fired": fired }))
}

/// BRP handler for "campaign/init": starts a new combine campaign (legacy compat).
fn handle_campaign_init(In(_params): In<Option<Value>>, world: &mut World) -> BrpResult {
    *world.resource_mut::<ActiveRun>() = ActiveRun {
        run: Some(CampaignRun {
            faction: PlayableFaction::Combine,
            current_mission: 0,
            outcomes: Vec::new(),
            complete: false,
            campaign_id: String::new(),
            mission_maps: Vec::new(),
        }),
        current_mission_entity: None,
        missions_won: 0,
        missions_lost: 0,
    };
    Ok(serde_json::json!({ "initialized": true }))
}

fn handle_campaign_state(In(_params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let active = world.resource::<ActiveRun>();
    let run = active.run.as_ref().ok_or_else(|| BrpError {
        code: -32000,
        message: "no active campaign run".into(),
        data: None,
    })?;
    Ok(serde_json::json!({
        "faction": format!("{:?}", run.faction),
        "current_mission": run.current_mission,
        "complete": run.complete,
        "missions_won": active.missions_won,
        "missions_lost": active.missions_lost,
        "outcomes": run.outcomes.iter().map(|o| serde_json::json!({
            "mission_index": o.mission_index,
            "won": o.won,
            "mission_type": format!("{:?}", o.mission_type),
        })).collect::<Vec<_>>(),
    }))
}

/// BRP handler for "campaign/advance": applies a mission outcome to the active run.
/// { won: bool } — advances the linear mission sequence.
fn handle_campaign_advance(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let won = params["won"].as_bool().unwrap_or(true);

    let mission_type = {
        let active = world.resource::<ActiveRun>();
        let run = active.run.as_ref().ok_or_else(|| BrpError {
            code: -32000, message: "no active campaign run".into(), data: None,
        })?;
        next_mission_type(run).ok_or_else(|| BrpError {
            code: -32000, message: "campaign already complete".into(), data: None,
        })?
    };

    // Extract a snapshot of progress, apply outcome to run, then merge back.
    let mut progress_snapshot = world.resource::<GlobalProgress>().clone();
    {
        let mut active = world.resource_mut::<ActiveRun>();
        let run = active.run.as_mut().unwrap();
        campaign::apply_mission_outcome(run, &mut progress_snapshot, won, mission_type);
    }
    *world.resource_mut::<GlobalProgress>() = progress_snapshot;

    let active = world.resource::<ActiveRun>();
    let run = active.run.as_ref().unwrap();
    Ok(serde_json::json!({
        "current_mission": run.current_mission,
        "complete": run.complete,
    }))
}

fn handle_campaign_options(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let _params = params;
    let active = world.resource::<ActiveRun>();
    let run = active.run.as_ref().ok_or_else(|| BrpError {
        code: -32000, message: "no active campaign run".into(), data: None,
    })?;
    let next = next_mission_type(run);
    let opts: Vec<Value> = match next {
        Some(mt) => vec![serde_json::json!({
            "mission_type": format!("{:?}", mt),
            "mission_index": run.current_mission,
        })],
        None => vec![],
    };
    Ok(serde_json::json!({ "options": opts }))
}

/// BRP handler for "mission/start": { mission_type, player, opponent, deadline? }
fn handle_mission_start(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let mission_type = parse_mission_type(params["mission_type"].as_str().unwrap_or("control"))?;
    let player = parse_faction(params["player"].as_str().unwrap_or("combine"))?;
    let opponent = parse_faction(params["opponent"].as_str().unwrap_or("hollow"))?;
    let deadline = params["deadline"].as_f64().unwrap_or(300.0) as f32;

    let id = world
        .spawn(Mission {
            mission_type,
            player_faction: player,
            opponent_faction: opponent,
            status: MissionStatus::Active,
            elapsed: 0.0,
            deadline,
            hill_timer: 0.0,
            hill_threshold: 180.0,
            assassination_target: None,
            ffa_check_timer: 0.0,
        })
        .id()
        .to_bits();
    Ok(serde_json::json!({ "entity_id": id }))
}

/// BRP handler for "mission/status": { entity }
fn handle_mission_status(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let entity_id = params["entity"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity required".into(),
        data: None,
    })?;
    let entity = Entity::try_from_bits(entity_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;
    let r = world.get_entity(entity).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;
    let m = r.get::<Mission>().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity is not a Mission".into(),
        data: None,
    })?;
    Ok(serde_json::json!({
        "mission_type": format!("{:?}", m.mission_type),
        "player": m.player_faction.id(),
        "opponent": m.opponent_faction.id(),
        "status": format!("{:?}", m.status),
        "elapsed": m.elapsed,
        "deadline": m.deadline,
    }))
}

fn parse_mission_type(s: &str) -> Result<mapgen::MissionType, BrpError> {
    match s {
        "assault" => Ok(mapgen::MissionType::Assault),
        "control" => Ok(mapgen::MissionType::Control),
        "defense" => Ok(mapgen::MissionType::Defense),
        "extraction" => Ok(mapgen::MissionType::Extraction),
        "survival" => Ok(mapgen::MissionType::Survival),
        "ffa" | "FFA" => Ok(mapgen::MissionType::Ffa),
        "king-of-the-hill" | "KingOfTheHill" | "koth" => Ok(mapgen::MissionType::KingOfTheHill),
        "assassination" | "Assassination" => Ok(mapgen::MissionType::Assassination),
        other => Err(BrpError {
            code: -32602,
            message: format!("unknown mission_type: {other}"),
            data: None,
        }),
    }
}

/// BRP handler for "map/generate": { width, height, seed, mission_type }
/// Spawns tile entities for the generated map. Returns summary stats.
fn handle_map_generate(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let width = params["width"].as_i64().unwrap_or(20) as i32;
    let height = params["height"].as_i64().unwrap_or(20) as i32;
    let seed = params["seed"].as_u64().unwrap_or(42);
    let mission = parse_mission_type(params["mission_type"].as_str().unwrap_or("control"))?;

    let m = mapgen::generate(width, height, seed, mission.clone());
    let tile_count = m.tiles.len();
    for t in &m.tiles {
        world.spawn(map::Tile {
            pos: t.pos.clone(),
            terrain_type: t.terrain.clone(),
            cover: t.cover.clone(),
        });
    }
    Ok(serde_json::json!({
        "tiles_spawned": tile_count,
        "bases": m.bases.iter().map(|p| serde_json::json!({"x": p.x, "y": p.y})).collect::<Vec<_>>(),
        "chokepoints": m.chokepoints.iter().map(|p| serde_json::json!({"x": p.x, "y": p.y})).collect::<Vec<_>>(),
        "mission_type": format!("{:?}", mission),
    }))
}

fn parse_doctrine(s: &str) -> Result<Doctrine, BrpError> {
    match s {
        "assault" => Ok(Doctrine::Assault),
        "fortification" => Ok(Doctrine::Fortification),
        "salvage" => Ok(Doctrine::Salvage),
        other => Err(BrpError {
            code: -32602,
            message: format!("unknown doctrine: {other}"),
            data: None,
        }),
    }
}

/// BRP handler for "ai/enable": { entity, home_x, home_y, doctrine }
/// Marks a faction as AI-controlled.
fn handle_ai_enable(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let entity_id = params["entity"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity required".into(),
        data: None,
    })?;
    let hx = params["home_x"].as_i64().unwrap_or(0) as i32;
    let hy = params["home_y"].as_i64().unwrap_or(0) as i32;
    let doctrine = parse_doctrine(params["doctrine"].as_str().unwrap_or("assault"))?;

    let entity = Entity::try_from_bits(entity_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;
    let mut em = world.get_entity_mut(entity).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;
    em.insert(AiController::new(hx, hy, doctrine));
    Ok(serde_json::json!({ "success": true }))
}

/// BRP handler for "dev/give_resources": { entity, fuel?, scrap?, manpower? }
/// Adds the given amounts to the faction's pool. For tests only.
fn handle_dev_give_resources(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let entity_id = params["entity"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity required".into(),
        data: None,
    })?;
    let fuel = params["fuel"].as_f64().unwrap_or(0.0) as f32;
    let scrap = params["scrap"].as_f64().unwrap_or(0.0) as f32;
    let manpower = params["manpower"].as_f64().unwrap_or(0.0) as f32;

    let entity = Entity::try_from_bits(entity_id).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;
    let mut em = world.get_entity_mut(entity).map_err(|_| BrpError {
        code: -32602,
        message: format!("entity {entity_id} not found"),
        data: None,
    })?;
    let mut pool = em.get_mut::<resources::ResourcePool>().ok_or_else(|| BrpError {
        code: -32602,
        message: "entity has no ResourcePool".into(),
        data: None,
    })?;
    pool.fuel += fuel;
    pool.scrap += scrap;
    pool.manpower += manpower;
    Ok(serde_json::json!({ "success": true }))
}

/// BRP handler for "dev/reset": despawns all gameplay entities (factions,
/// units, buildings, control points). For tests so each run starts clean.
fn handle_dev_reset(In(_params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let mut to_despawn: Vec<Entity> = Vec::new();
    {
        let mut q = world.query_filtered::<Entity, Or<(
            With<resources::FactionEntity>,
            With<units::UnitTypeId>,
            With<map::ControlPoint>,
            With<BuildingTypeId>,
            With<Hero>,
            With<map::Tile>,
            With<Mission>,
            With<HollowSpawner>,
        )>>();
        for e in q.iter(world) {
            to_despawn.push(e);
        }
    }
    let count = to_despawn.len();
    for e in to_despawn {
        world.despawn(e);
    }
    // Reset resources that accumulate across runs.
    if let Some(mut fired) = world.get_resource_mut::<FiredBeats>() {
        fired.0.clear();
    }
    Ok(serde_json::json!({ "despawned": count }))
}

fn on_startup() {
    info!("Cindertide initialized");
}

// =============================================================================
// Step 32: TUI-supporting BRP methods + faction-asymmetric scenario setup.
// =============================================================================

/// BRP handler for "world/list": { kind } where kind is one of
/// factions / units / buildings / points / missions / heroes / spawners / tiles.
/// Returns a flat array of summaries suitable for the TUI.
fn handle_world_list(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let kind = params["kind"].as_str().unwrap_or("").to_string();

    let items: Vec<Value> = match kind.as_str() {
        "factions" => {
            let mut out = Vec::new();
            let mut q = world.query::<(
                Entity,
                &resources::FactionEntity,
                &ResourcePool,
                &ResourceTrickle,
                &resources::PopCap,
            )>();
            for (e, fe, pool, trickle, pop) in q.iter(world) {
                out.push(serde_json::json!({
                    "entity_id": e.to_bits(),
                    "faction": fe.faction.id(),
                    "fuel": pool.fuel,
                    "scrap": pool.scrap,
                    "manpower": pool.manpower,
                    "fuel_trickle": trickle.fuel_per_second,
                    "scrap_trickle": trickle.scrap_per_second,
                    "manpower_trickle": trickle.manpower_per_second,
                    "pop_current": pop.current,
                    "pop_max": pop.max,
                }));
            }
            out
        }
        "units" => {
            let mut out = Vec::new();
            let mut q = world.query::<(
                Entity,
                &units::UnitTypeId,
                &UnitPos,
                &Faction,
                &Health,
            )>();
            for (e, ut, pos, faction, h) in q.iter(world) {
                out.push(serde_json::json!({
                    "entity_id": e.to_bits(),
                    "unit_type": ut.id(),
                    "faction": faction.id(),
                    "x": pos.pos.x,
                    "y": pos.pos.y,
                    "health": h.current,
                    "health_max": h.max,
                }));
            }
            out
        }
        "buildings" => {
            let mut out = Vec::new();
            let mut q = world.query::<(
                Entity,
                &BuildingTypeId,
                &BuildingPos,
                &Faction,
                &Health,
                Option<&Built>,
            )>();
            for (e, bt, pos, faction, h, built) in q.iter(world) {
                out.push(serde_json::json!({
                    "entity_id": e.to_bits(),
                    "building_type": bt.id(),
                    "faction": faction.id(),
                    "x": pos.pos.x,
                    "y": pos.pos.y,
                    "built": built.is_some(),
                    "health": h.current,
                    "health_max": h.max,
                }));
            }
            out
        }
        "points" => {
            let mut out = Vec::new();
            let mut q = world.query::<(Entity, &ControlPoint)>();
            for (e, cp) in q.iter(world) {
                out.push(serde_json::json!({
                    "entity_id": e.to_bits(),
                    "point_type": format!("{:?}", cp.point_type),
                    "x": cp.pos.x,
                    "y": cp.pos.y,
                    "owner": cp.owner.as_ref().map(|f| f.id().to_string()),
                    "capture_progress": cp.capture_progress,
                }));
            }
            out
        }
        "missions" => {
            let mut out = Vec::new();
            let mut q = world.query::<(Entity, &Mission)>();
            for (e, m) in q.iter(world) {
                out.push(serde_json::json!({
                    "entity_id": e.to_bits(),
                    "mission_type": format!("{:?}", m.mission_type),
                    "player": m.player_faction.id(),
                    "opponent": m.opponent_faction.id(),
                    "elapsed": m.elapsed,
                    "deadline": m.deadline,
                    "status": format!("{:?}", m.status),
                }));
            }
            out
        }
        "heroes" => {
            let mut out = Vec::new();
            let mut q = world.query::<(Entity, &Hero, &UnitPos, &Faction, &Health, &SignatureAbility)>();
            for (e, hero, pos, faction, h, s) in q.iter(world) {
                out.push(serde_json::json!({
                    "entity_id": e.to_bits(),
                    "name": hero.name,
                    "faction": faction.id(),
                    "x": pos.pos.x,
                    "y": pos.pos.y,
                    "health": h.current,
                    "health_max": h.max,
                    "charge": s.charge,
                    "max_charge": s.max_charge,
                }));
            }
            out
        }
        "spawners" => {
            let mut out = Vec::new();
            let mut q = world.query::<(Entity, &HollowSpawner)>();
            for (e, s) in q.iter(world) {
                out.push(serde_json::json!({
                    "entity_id": e.to_bits(),
                    "mode": format!("{:?}", s.mode),
                    "x": s.pos.x,
                    "y": s.pos.y,
                }));
            }
            out
        }
        "tiles" => {
            let mut out = Vec::new();
            let mut q = world.query::<&map::Tile>();
            for t in q.iter(world) {
                out.push(serde_json::json!({
                    "x": t.pos.x,
                    "y": t.pos.y,
                    "terrain": format!("{:?}", t.terrain_type),
                    "cover": format!("{:?}", t.cover),
                }));
            }
            out
        }
        other => {
            return Err(BrpError {
                code: -32602,
                message: format!("unknown kind: {other}"),
                data: None,
            });
        }
    };

    Ok(serde_json::json!({ "items": items }))
}

/// BRP handler for "game/pause": { paused }. Toggles `Time<Virtual>`.
fn handle_game_pause(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let paused = params
        .as_ref()
        .and_then(|p| p["paused"].as_bool())
        .unwrap_or(true);
    let mut time = world.resource_mut::<Time<Virtual>>();
    if paused {
        time.pause();
    } else {
        time.unpause();
    }
    Ok(serde_json::json!({ "paused": paused }))
}

/// BRP handler for "game/pause_status": returns whether virtual time is paused.
fn handle_game_pause_status(In(_params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let time = world.resource::<Time<Virtual>>();
    Ok(serde_json::json!({ "paused": time.is_paused() }))
}

/// BRP handler for "save/list": names of all save slots in memory.
fn handle_save_list(In(_params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let slots = world
        .get_resource::<SaveSlots>()
        .map(|s| s.0.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    Ok(serde_json::json!({ "slots": slots }))
}

/// BRP handler for "game/abandon_mission": force-resolves the active mission as a loss.
fn handle_game_abandon_mission(
    In(_params): In<Option<Value>>,
    world: &mut World,
) -> BrpResult {
    let mission_entity = world.resource::<ActiveRun>().current_mission_entity;
    let Some(entity) = mission_entity else {
        return Err(BrpError {
            code: -32000,
            message: "no active mission".into(),
            data: None,
        });
    };
    if let Ok(mut em) = world.get_entity_mut(entity) {
        if let Some(mut m) = em.get_mut::<Mission>() {
            m.status = MissionStatus::Lost;
        }
    }
    Ok(serde_json::json!({ "abandoned": true }))
}

// --- Faction-asymmetric scenario setup (Layer 1 per factions.md) ---

fn spawn_built_building(
    world: &mut World,
    bt: BuildingTypeId,
    faction: Faction,
    x: i32,
    y: i32,
    loaded: &LoadedFactions,
) {
    let id = world.spawn(BuildingBundle::new(bt.clone(), faction, x, y, loaded)).id();
    if let Ok(mut em) = world.get_entity_mut(id) {
        em.remove::<UnderConstruction>();
        em.insert(Built);
    }
    if !building_produces(&bt, loaded).is_empty() {
        if let Ok(mut em) = world.get_entity_mut(id) {
            em.insert(ProductionQueue::default());
        }
    }
}

fn spawn_unit_at(world: &mut World, faction: Faction, x: i32, y: i32) {
    spawn_unit_type_at(world, faction, "riflemen", x, y);
}

fn spawn_unit_type_at(world: &mut World, faction: Faction, unit_id: &str, x: i32, y: i32) {
    let loaded = world.resource::<LoadedFactions>().clone();
    let id = if let Some(def) = loaded.units.get(unit_id) {
        world.spawn(UnitBundle::from_def(def, faction, x, y)).id()
    } else {
        world.spawn(UnitBundle::default_riflemen(faction, x, y)).id()
    };
    if let Ok(mut em) = world.get_entity_mut(id) {
        em.insert(units::HomeBase { pos: GridPos { x, y } });
    }
}

/// Apply starting loadout for a given faction at a base origin tile.
///
/// `mission_index` controls player starting strength (0 = tutorial-rich, 4 = bare).
/// `is_player` = true uses the mission_index progression; false always uses full AI base.
/// `resource_bonus` is a flat starting resource bonus added to the faction's pool.
fn apply_faction_loadout(
    world: &mut World,
    faction: &Faction,
    base: &GridPos,
    mission_index: usize,
    is_player: bool,
    resource_bonus: f32,
) {
    let bx = base.x;
    let by = base.y;

    // Enemy AI always gets a full base regardless of mission index.
    // Player gets a progressively smaller starting loadout.
    let effective_index = if is_player { mission_index } else { 0 };

    let loaded = world.resource::<LoadedFactions>().clone();

    match faction.id() {
        "combine" => {
            // Index 0: full base (CommandBunker + Refinery + Barracks + Scrapyard + SupplyDepot, 4 Riflemen + 1 HeavyWeapons)
            // Index 1: light  (CommandBunker + Refinery, 2 Riflemen)
            // Index 2: minimal (CommandBunker only, 1 Rifleman)
            // Index 3-4: economic start (Refinery only, 1 Rifleman)
            match effective_index {
                0 => {
                    spawn_built_building(world, BuildingTypeId::new("command_bunker"), faction.clone(), bx, by, &loaded);
                    spawn_built_building(world, BuildingTypeId::new("refinery"), faction.clone(), bx + 2, by, &loaded);
                    spawn_built_building(world, BuildingTypeId::new("barracks"), faction.clone(), bx, by + 2, &loaded);
                    for i in 0..3 {
                        spawn_unit_type_at(world, faction.clone(), "riflemen", bx + i, by + 4);
                    }
                    spawn_unit_type_at(world, faction.clone(), "heavy_weapons", bx + 3, by + 4);
                }
                1 => {
                    spawn_built_building(world, BuildingTypeId::new("command_bunker"), faction.clone(), bx, by, &loaded);
                    spawn_built_building(world, BuildingTypeId::new("refinery"), faction.clone(), bx + 2, by, &loaded);
                    for i in 0..2 {
                        spawn_unit_type_at(world, faction.clone(), "riflemen", bx + i, by + 4);
                    }
                }
                2 => {
                    spawn_built_building(world, BuildingTypeId::new("command_bunker"), faction.clone(), bx, by, &loaded);
                    spawn_unit_type_at(world, faction.clone(), "riflemen", bx, by + 3);
                }
                _ => {
                    // Missions 3+: economic start — Refinery only, 1 Rifleman
                    spawn_built_building(world, BuildingTypeId::new("refinery"), faction.clone(), bx, by, &loaded);
                    spawn_unit_type_at(world, faction.clone(), "riflemen", bx, by + 3);
                }
            }
            // Full base for AI (effective_index == 0)
            if effective_index == 0 && !is_player {
                spawn_built_building(world, BuildingTypeId::new("scrapyard"), faction.clone(), bx + 2, by + 2, &loaded);
                spawn_built_building(world, BuildingTypeId::new("supply_depot"), faction.clone(), bx - 1, by + 1, &loaded);
                spawn_unit_type_at(world, faction.clone(), "riflemen", bx + 4, by + 4);
            }
        }
        "ironborn" => {
            match effective_index {
                0 => {
                    spawn_built_building(world, BuildingTypeId::new("foundry"), faction.clone(), bx, by, &loaded);
                    spawn_built_building(world, BuildingTypeId::new("scrapyard"), faction.clone(), bx + 2, by, &loaded);
                    spawn_built_building(world, BuildingTypeId::new("repair_bay"), faction.clone(), bx, by + 2, &loaded);
                    for i in 0..3 {
                        spawn_unit_type_at(world, faction.clone(), "riflemen", bx + i, by + 4);
                    }
                    spawn_unit_type_at(world, faction.clone(), "heavy_weapons", bx + 3, by + 4);
                }
                1 => {
                    spawn_built_building(world, BuildingTypeId::new("foundry"), faction.clone(), bx, by, &loaded);
                    spawn_built_building(world, BuildingTypeId::new("scrapyard"), faction.clone(), bx + 2, by, &loaded);
                    for i in 0..2 {
                        spawn_unit_type_at(world, faction.clone(), "riflemen", bx + i, by + 4);
                    }
                }
                2 => {
                    spawn_built_building(world, BuildingTypeId::new("foundry"), faction.clone(), bx, by, &loaded);
                    spawn_unit_type_at(world, faction.clone(), "riflemen", bx, by + 3);
                }
                _ => {
                    // Missions 3+: Foundry (primary economic building) + 1 Rifleman
                    spawn_built_building(world, BuildingTypeId::new("foundry"), faction.clone(), bx, by, &loaded);
                    spawn_unit_type_at(world, faction.clone(), "riflemen", bx, by + 3);
                }
            }
            if effective_index == 0 && !is_player {
                spawn_unit_type_at(world, faction.clone(), "light_vehicle", bx + 4, by + 4);
            }
        }
        "covenant" => {
            match effective_index {
                0 => {
                    spawn_built_building(world, BuildingTypeId::new("command_bunker"), faction.clone(), bx, by, &loaded);
                    spawn_built_building(world, BuildingTypeId::new("pillbox"), faction.clone(), bx + 3, by - 1, &loaded);
                    spawn_built_building(world, BuildingTypeId::new("workshop"), faction.clone(), bx, by + 2, &loaded);
                    for i in 0..3 {
                        spawn_unit_type_at(world, faction.clone(), "riflemen", bx + i, by + 4);
                    }
                    spawn_unit_type_at(world, faction.clone(), "heavy_weapons", bx + 3, by + 4);
                }
                1 => {
                    spawn_built_building(world, BuildingTypeId::new("command_bunker"), faction.clone(), bx, by, &loaded);
                    spawn_built_building(world, BuildingTypeId::new("workshop"), faction.clone(), bx, by + 2, &loaded);
                    for i in 0..2 {
                        spawn_unit_type_at(world, faction.clone(), "riflemen", bx + i, by + 4);
                    }
                }
                2 => {
                    spawn_built_building(world, BuildingTypeId::new("command_bunker"), faction.clone(), bx, by, &loaded);
                    spawn_unit_type_at(world, faction.clone(), "riflemen", bx, by + 3);
                }
                _ => {
                    // Missions 3+: Workshop (primary economic building) + 1 Rifleman
                    spawn_built_building(world, BuildingTypeId::new("workshop"), faction.clone(), bx, by, &loaded);
                    spawn_unit_type_at(world, faction.clone(), "riflemen", bx, by + 3);
                }
            }
            if effective_index == 0 && !is_player {
                spawn_built_building(world, BuildingTypeId::new("pillbox"), faction.clone(), bx + 3, by + 1, &loaded);
                spawn_built_building(world, BuildingTypeId::new("watchtower"), faction.clone(), bx + 3, by + 3, &loaded);
            }
        }
        "hollow" | _ => {
            world.spawn(HollowSpawner::new(hollow::HollowMode::Consuming, bx, by));
        }
    }

    // Apply starting resource bonus to this faction's pool if nonzero.
    if resource_bonus > 0.0 {
        let mut q = world.query::<(&resources::FactionEntity, &mut resources::ResourcePool)>();
        for (fe, mut pool) in q.iter_mut(world) {
            if &fe.faction == faction {
                pool.fuel += resource_bonus * 0.5;
                pool.scrap += resource_bonus * 0.5;
            }
        }
    }
}

/// Default opponent faction for a given player.
fn default_opponent(player: &Faction) -> Faction {
    match player.id() {
        "ironborn" => Faction::combine(),
        _ => Faction::hollow(),
    }
}

/// Generate a Control map and place loadouts for both factions.
/// Called from `mission/select` on the way into a mission.
/// `mission_index` controls the player starting loadout (0 = tutorial-rich, 4 = bare).
/// `spawn_zone_overrides`: if provided, use zone 0 as player base and zone 1 as AI base
///   instead of the procedurally-generated base positions.
pub fn setup_demo_scenario(world: &mut World, player: &Faction, mission_index: usize) {
    setup_demo_scenario_with_spawns(world, player, mission_index, None);
}

/// Like `setup_demo_scenario` but accepts optional spawn zone overrides.
/// When `spawn_overrides` is `Some((player_pos, opponent_pos))`, use those positions
/// instead of the procedurally-generated base positions.
pub fn setup_demo_scenario_with_spawns(
    world: &mut World,
    player: &Faction,
    mission_index: usize,
    spawn_overrides: Option<(GridPos, GridPos)>,
) {
    let opponent = default_opponent(player);
    let m = mapgen::generate_for_mission(17, mapgen::MissionType::Control);
    for t in &m.tiles {
        world.spawn(map::Tile {
            pos: t.pos.clone(),
            terrain_type: t.terrain.clone(),
            cover: t.cover.clone(),
        });
    }

    let half_x = m.width / 2;
    let mid_y = m.height / 2;
    let (p_base, o_base) = if let Some((pz, oz)) = spawn_overrides {
        (pz, oz)
    } else {
        let p = m.bases.first().cloned().unwrap_or(GridPos { x: 5, y: mid_y });
        let o = m.bases.get(1).cloned().unwrap_or(GridPos { x: m.width - 6, y: mid_y });
        (p, o)
    };

    apply_faction_loadout(world, player, &p_base, mission_index, true, 0.0);
    apply_faction_loadout(world, &opponent, &o_base, 0, false, 0.0);

    // Make sure the opponent faction entity exists (player faction was
    // spawned by game/new). Top them up so AI can build.
    let mut needed = true;
    {
        let mut q = world.query::<&resources::FactionEntity>();
        for fe in q.iter(world) {
            if &fe.faction == &opponent {
                needed = false;
                break;
            }
        }
    }
    if needed {
        world.spawn(FactionBundle::new(opponent.clone()));
    }

    // Enable AI on opponent (and player) so the demo plays itself.
    let mut player_faction_entity: Option<Entity> = None;
    let mut opponent_faction_entity: Option<Entity> = None;
    {
        let mut q = world.query::<(Entity, &resources::FactionEntity)>();
        for (e, fe) in q.iter(world) {
            if &fe.faction == player {
                player_faction_entity = Some(e);
            } else if &fe.faction == &opponent {
                opponent_faction_entity = Some(e);
            }
        }
    }
    if let Some(e) = player_faction_entity {
        if let Ok(mut em) = world.get_entity_mut(e) {
            em.insert(AiController::new(p_base.x, p_base.y, tech::Doctrine::Assault));
            if let Some(mut pool) = em.get_mut::<ResourcePool>() {
                pool.fuel += 600.0;
                pool.scrap += 600.0;
                pool.manpower += 30.0;
            }
        }
    }
    if let Some(e) = opponent_faction_entity {
        if let Ok(mut em) = world.get_entity_mut(e) {
            em.insert(AiController::new(o_base.x, o_base.y, tech::Doctrine::Assault));
            if let Some(mut pool) = em.get_mut::<ResourcePool>() {
                pool.fuel += 600.0;
                pool.scrap += 600.0;
                pool.manpower += 30.0;
            }
        }
    }

    // Spawn neutral strategic points evenly spaced along the midline.
    let step = half_x / 4;
    for i in 1..=3 {
        let sx = step * i;
        world.spawn(ControlPoint {
            point_type: ControlPointType::Strategic,
            pos: GridPos { x: sx, y: mid_y },
            capture_radius: 2.0,
            owner: None,
            contesting: None,
            capture_progress: 0.0,
        });
    }

    // Load mission script if one exists for this faction + mission index.
    let faction_name = player.id();
    let script_name = format!("{faction_name}_m{mission_index}");
    if let Some(mut script_state) = world.get_resource_mut::<mission_script::ScriptState>() {
        script_state.load_script(&script_name);
    }
}

// =============================================================================
// Campaign map loader.
// =============================================================================

#[derive(serde::Deserialize, Default)]
struct RawMapUnit {
    faction: String,
    unit_type: String,
    x: i32,
    y: i32,
}

#[derive(serde::Deserialize, Default)]
struct RawMapBuilding {
    faction: String,
    building_type: String,
    x: i32,
    y: i32,
}

#[derive(serde::Deserialize, Default)]
struct RawScriptAction {
    action_type: String,
    #[serde(default)] text: String,
    #[serde(default)] faction: String,
    #[serde(default)] unit_type: String,
    #[serde(default)] count: u32,
    #[serde(default)] x: i32,
    #[serde(default)] y: i32,
}

#[derive(serde::Deserialize)]
struct RawScriptEvent {
    id: String,
    trigger_type: String,
    #[serde(default)] trigger_seconds: f32,
    #[serde(default)] trigger_beat: String,
    #[serde(default)] actions: Vec<RawScriptAction>,
}

#[derive(serde::Deserialize, Default)]
struct RawMap {
    #[serde(default)]
    units: Vec<RawMapUnit>,
    #[serde(default)]
    buildings: Vec<RawMapBuilding>,
    #[serde(default)]
    player_faction: Option<String>,
    #[serde(default)]
    opponent_faction: Option<String>,
    #[serde(default)]
    mission_type: Option<String>,
    #[serde(default)]
    deadline_seconds: Option<f32>,
    #[serde(default)]
    script_events: Vec<RawScriptEvent>,
}

/// Convert CamelCase type names from map TOMLs to the lowercase snake_case ids
/// used internally (e.g. "HeavyWeapons" → "heavy_weapons").
fn camel_to_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_lowercase().next().unwrap());
    }
    out
}

/// Load a campaign mission map TOML from `map_path`, spawn all units/buildings,
/// spawn a `Mission` entity, and wire up AI controllers for both factions.
///
/// This is the headless equivalent of `setup_demo_scenario` but driven entirely
/// by the data in the TOML rather than procedural generation.
pub fn load_campaign_map(world: &mut World, map_path: &str) {
    let raw_toml = std::fs::read_to_string(map_path)
        .unwrap_or_else(|e| panic!("Failed to read map file {map_path}: {e}"));
    let raw: RawMap = toml::from_str(&raw_toml)
        .unwrap_or_else(|e| panic!("Failed to parse map TOML {map_path}: {e}"));

    // Determine mission type.
    let mission_type_str = raw.mission_type.as_deref().unwrap_or("Control");
    let mt = match mission_type_str {
        "Assault"      => mapgen::MissionType::Assault,
        "Defense"      => mapgen::MissionType::Defense,
        "Extraction"   => mapgen::MissionType::Extraction,
        "Survival"     => mapgen::MissionType::Survival,
        "Ffa"          => mapgen::MissionType::Ffa,
        "KingOfTheHill"=> mapgen::MissionType::KingOfTheHill,
        "Assassination"=> mapgen::MissionType::Assassination,
        _              => mapgen::MissionType::Control,
    };

    // Spawn flat grass tiles covering all unit/building positions plus margin.
    // Campaign maps define their own placements; procedurally-generated terrain
    // would introduce random obstacles that block paths between the two bases.
    let xs: Vec<i32> = raw.units.iter().map(|u| u.x)
        .chain(raw.buildings.iter().map(|b| b.x)).collect();
    let ys: Vec<i32> = raw.units.iter().map(|u| u.y)
        .chain(raw.buildings.iter().map(|b| b.y)).collect();
    let min_x = xs.iter().copied().min().unwrap_or(0) - 5;
    let max_x = xs.iter().copied().max().unwrap_or(128) + 5;
    let min_y = ys.iter().copied().min().unwrap_or(0) - 5;
    let max_y = ys.iter().copied().max().unwrap_or(80) + 5;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            world.spawn(map::Tile {
                pos: GridPos { x, y },
                terrain_type: map::TerrainType::Grass,
                cover: map::CoverDensity::None,
            });
        }
    }
    // Keep a generated map reference for control-point midline calculation.
    let generated = mapgen::generate_for_mission(17, mt.clone());

    let loaded = world.resource::<LoadedFactions>().clone();

    // Spawn units from the TOML.
    for u in &raw.units {
        let faction = Faction::new(&u.faction.to_lowercase());
        let unit_id = camel_to_snake(&u.unit_type);
        spawn_unit_type_at(world, faction, &unit_id, u.x, u.y);
    }

    // Spawn buildings from the TOML.
    for b in &raw.buildings {
        let faction = Faction::new(&b.faction.to_lowercase());
        let building_id = camel_to_snake(&b.building_type);
        spawn_built_building(world, BuildingTypeId::new(&building_id), faction, b.x, b.y, &loaded);
    }

    // Determine factions.
    let player_faction_str = raw.player_faction.as_deref().unwrap_or("combine");
    let opponent_faction_str = raw.opponent_faction.as_deref().unwrap_or("ironborn");
    let player_faction = Faction::new(&player_faction_str.to_lowercase());
    let opponent_faction = Faction::new(&opponent_faction_str.to_lowercase());

    // Derive home positions from CommandBunker buildings, falling back to unit
    // centroid, then map corners.
    let find_home = |faction_str: &str| -> GridPos {
        let id_lower = faction_str.to_lowercase();
        // Try command_bunker first.
        if let Some(b) = raw.buildings.iter().find(|b| {
            b.faction.to_lowercase() == id_lower && camel_to_snake(&b.building_type) == "command_bunker"
        }) {
            return GridPos { x: b.x, y: b.y };
        }
        // Fall back to centroid of all units for this faction.
        let faction_units: Vec<_> = raw.units.iter()
            .filter(|u| u.faction.to_lowercase() == id_lower)
            .collect();
        if !faction_units.is_empty() {
            let sum_x: i32 = faction_units.iter().map(|u| u.x).sum();
            let sum_y: i32 = faction_units.iter().map(|u| u.y).sum();
            let n = faction_units.len() as i32;
            return GridPos { x: sum_x / n, y: sum_y / n };
        }
        GridPos { x: 10, y: 10 }
    };
    let player_home   = find_home(player_faction_str);
    let opponent_home = find_home(opponent_faction_str);
    let deadline = raw.deadline_seconds.unwrap_or(300.0);

    // Spawn the Mission entity.
    world.spawn(Mission {
        mission_type: mt,
        player_faction: player_faction.clone(),
        opponent_faction: opponent_faction.clone(),
        status: mission::MissionStatus::Active,
        elapsed: 0.0,
        deadline,
        hill_timer: 0.0,
        hill_threshold: 180.0,
        assassination_target: None,
        ffa_check_timer: 0.0,
    });

    // Ensure FactionBundle entities exist for both factions.
    let mut has_player = false;
    let mut has_opponent = false;
    {
        let mut q = world.query::<&resources::FactionEntity>();
        for fe in q.iter(world) {
            if &fe.faction == &player_faction   { has_player = true; }
            if &fe.faction == &opponent_faction { has_opponent = true; }
        }
    }
    if !has_player   { world.spawn(FactionBundle::new(player_faction.clone())); }
    if !has_opponent { world.spawn(FactionBundle::new(opponent_faction.clone())); }

    // Spawn neutral control points along the diagonal path between the two bases.
    // Placing them at 1/4, 1/2, 3/4 of the line between player_home and opponent_home
    // ensures units naturally walk through them as they advance.
    for frac in [0.25f32, 0.50, 0.75] {
        let cp_x = (player_home.x as f32 + (opponent_home.x - player_home.x) as f32 * frac) as i32;
        let cp_y = (player_home.y as f32 + (opponent_home.y - player_home.y) as f32 * frac) as i32;
        world.spawn(map::ControlPoint {
            point_type: map::ControlPointType::Strategic,
            pos: GridPos { x: cp_x, y: cp_y },
            capture_radius: 3.0,
            owner: None,
            contesting: None,
            capture_progress: 0.0,
        });
    }

    // Wire up AI controllers for both factions.
    let mut player_entity: Option<Entity> = None;
    let mut opponent_entity: Option<Entity> = None;
    {
        let mut q = world.query::<(Entity, &resources::FactionEntity)>();
        for (e, fe) in q.iter(world) {
            if &fe.faction == &player_faction   { player_entity = Some(e); }
            if &fe.faction == &opponent_faction { opponent_entity = Some(e); }
        }
    }


    if let Some(e) = player_entity {
        if let Ok(mut em) = world.get_entity_mut(e) {
            em.insert(AiController::new(player_home.x, player_home.y, tech::Doctrine::Assault));
            if let Some(mut pool) = em.get_mut::<ResourcePool>() {
                pool.fuel     += 600.0;
                pool.scrap    += 600.0;
                pool.manpower += 30.0;
            }
        }
    }
    if let Some(e) = opponent_entity {
        if let Ok(mut em) = world.get_entity_mut(e) {
            em.insert(AiController::new(opponent_home.x, opponent_home.y, tech::Doctrine::Assault));
            if let Some(mut pool) = em.get_mut::<ResourcePool>() {
                pool.fuel     += 600.0;
                pool.scrap    += 600.0;
                pool.manpower += 30.0;
            }
        }
    }

    // Translate inline script_events from the map TOML into MissionScript format
    // and load them into ScriptState so MissionScriptPlugin can tick them.
    if !raw.script_events.is_empty() {
        let events: Vec<mission_script::ScriptEvent> = raw.script_events.iter().map(|e| {
            let trigger = if e.trigger_type == "time" {
                mission_script::Trigger::Time { seconds: e.trigger_seconds }
            } else {
                mission_script::Trigger::Condition {
                    condition: e.trigger_type.clone(),
                    beat_id: if e.trigger_beat.is_empty() { None } else { Some(e.trigger_beat.clone()) },
                }
            };
            let actions = e.actions.iter().filter_map(|a| {
                Some(match a.action_type.as_str() {
                    "dialogue" => mission_script::Action::Dialogue {
                        speaker: None,
                        text: a.text.clone(),
                    },
                    "spawn_units" => mission_script::Action::SpawnUnits {
                        faction: a.faction.clone(),
                        unit_type: a.unit_type.clone(),
                        count: a.count,
                        x: a.x,
                        y: a.y,
                    },
                    "objective" => mission_script::Action::Objective { text: a.text.clone() },
                    "change_objective" => mission_script::Action::ChangeObjective { text: a.text.clone() },
                    _ => return None,
                })
            }).collect();
            mission_script::ScriptEvent { id: e.id.clone(), trigger, actions }
        }).collect();

        if let Some(mut state) = world.get_resource_mut::<mission_script::ScriptState>() {
            state.script = Some(mission_script::MissionScript { events });
            state.fired.clear();
            state.dialogue_queue.clear();
            state.current_objective = None;
            state.dialogue_timer = 0.0;
        }
    }
}

// =============================================================================
// NavMesh baking helper.
// =============================================================================

/// Collect all Tile entities, build a NavMesh from them, and insert it as a
/// world resource. Call this after all tiles have been spawned.
pub fn bake_navmesh(world: &mut World) {
    let tile_map: std::collections::HashMap<(i32, i32), map::TerrainType> = {
        let mut q = world.query::<&map::Tile>();
        q.iter(world).map(|t| ((t.pos.x, t.pos.y), t.terrain_type.clone())).collect()
    };
    let max_x = tile_map.keys().map(|(x, _)| *x).max().unwrap_or(0);
    let max_y = tile_map.keys().map(|(_, y)| *y).max().unwrap_or(0);
    let nav = map::NavMesh::build(max_x + 1, max_y + 1, &tile_map);
    world.insert_resource(nav);
}

// =============================================================================
// Narrative BRP handlers.
// =============================================================================

/// BRP handler for "narrative/mission": { faction: String, index: usize }
/// Returns { title, briefing } for the requested mission.
fn handle_narrative_mission(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let faction = params["faction"].as_str().ok_or_else(|| BrpError {
        code: -32602,
        message: "faction required".into(),
        data: None,
    })?;
    let index = params["index"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "index required".into(),
        data: None,
    })? as usize;

    let narrative = world.get_resource::<narrative::NarrativeData>().ok_or_else(|| BrpError {
        code: -32000,
        message: "narrative data not loaded".into(),
        data: None,
    })?;
    let mission = narrative.mission(faction, index).ok_or_else(|| BrpError {
        code: -32000,
        message: format!("no mission {index} for faction {faction}"),
        data: None,
    })?;
    Ok(serde_json::json!({
        "title": mission.title,
        "briefing": mission.briefing,
    }))
}

/// BRP handler for "narrative/debrief": { faction: String, index: usize, won: bool }
/// Returns { text } with the win or loss debrief for the requested mission.
fn handle_narrative_debrief(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let faction = params["faction"].as_str().ok_or_else(|| BrpError {
        code: -32602,
        message: "faction required".into(),
        data: None,
    })?;
    let index = params["index"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "index required".into(),
        data: None,
    })? as usize;
    let won = params["won"].as_bool().unwrap_or(true);

    let narrative = world.get_resource::<narrative::NarrativeData>().ok_or_else(|| BrpError {
        code: -32000,
        message: "narrative data not loaded".into(),
        data: None,
    })?;
    let mission = narrative.mission(faction, index).ok_or_else(|| BrpError {
        code: -32000,
        message: format!("no mission {index} for faction {faction}"),
        data: None,
    })?;
    let text = if won { &mission.win } else { &mission.loss };
    Ok(serde_json::json!({ "text": text }))
}

fn handle_narrative_finale(In(_params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let (has_beaten, combine_beaten_first) = world
        .get_resource::<GlobalProgress>()
        .map(|g| {
            let beaten = g.first_beaten.is_some();
            let combine_first = g.first_beaten.as_ref().map(|f| matches!(f, PlayableFaction::Combine)).unwrap_or(false);
            (beaten, combine_first)
        })
        .unwrap_or((false, false));
    if !has_beaten {
        return Ok(serde_json::json!({ "text": "" }));
    }
    let narrative = world.get_resource::<narrative::NarrativeData>().ok_or_else(|| BrpError {
        code: -32000,
        message: "narrative data not loaded".into(),
        data: None,
    })?;
    let text = narrative.finale(combine_beaten_first);
    Ok(serde_json::json!({ "text": text }))
}
