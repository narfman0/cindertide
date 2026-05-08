use bevy::prelude::*;
use bevy::remote::{RemotePlugin, http::RemoteHttpPlugin, BrpResult, BrpError};
use serde_json::Value;

mod map;
mod units;
mod combat;
mod resources;
mod control;
mod buildings;
mod production;
mod heroes;
mod tech;
mod unit_ai;
mod repair;
mod mapgen;
mod mission;
mod campaign;
mod beats;
mod hollow;
mod editor;
mod save;
#[cfg(feature = "render")]
mod render;
mod ai;

use map::{MapPlugin, GridPos, Faction, ControlPoint, ControlPointType};
use units::{UnitPlugin, MoveTarget, UnitPos, RiflemanBundle};
use combat::{CombatPlugin, AttackTarget, Health, Morale, Suppression, Facing, morale_state};
use resources::{ResourcesPlugin, FactionBundle, ResourcePool, ResourceTrickle, ResourceCost, spend};
use control::ControlPlugin;
use buildings::{BuildingsPlugin, BuildingType, BuildingBundle, BuildingPos, ConstructionProgress, building_cost, try_pay, can_place, Built, UnderConstruction};
use production::{ProductionPlugin, ProductionQueue, building_produces, try_enqueue, EnqueueError};
use heroes::{HeroPlugin, HeroBundle, Hero, AbilityKind, SignatureAbility, Aura, HeroDowned, is_charge_full, within_aura};
use tech::{TechPlugin, Tech, Tier, Doctrine, ResearchTarget, ResearchInProgress, start_research};
use unit_ai::UnitAiPlugin;
use repair::RepairPlugin;
use ai::{AiPlugin, AiController};
use mission::{MissionPlugin, Mission, MissionStatus};
use campaign::{CampaignState, default_state, apply_mission_outcome, spread_corruption, generate_mission_options};
use beats::{BeatsPlugin, FiredBeats};
use hollow::{HollowPlugin, HollowSpawner, HollowMode};
use save::{SavePlugin, SaveSlots};

fn main() {
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
                .with_method("production/enqueue", handle_production_enqueue)
                .with_method("production/queue_status", handle_production_queue_status)
                .with_method("hero/spawn", handle_hero_spawn)
                .with_method("hero/status", handle_hero_status)
                .with_method("hero/ability_use", handle_hero_ability_use)
                .with_method("tech/research", handle_tech_research)
                .with_method("tech/status", handle_tech_status)
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
        .add_plugins(BeatsPlugin)
        .add_plugins(HollowPlugin)
        .add_plugins(SavePlugin)
        .add_systems(Startup, on_startup)
        .run();
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

    let unit_type = parse_unit_type(unit_type_str)?;

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

    let faction = match params["faction"].as_str() {
        Some("combine") | None => Faction::Combine,
        Some("covenant") => Faction::Covenant,
        Some("ironborn") => Faction::Ironborn,
        Some("hollow") => Faction::Hollow,
        Some(other) => {
            return Err(BrpError {
                code: -32602,
                message: format!("unknown faction: {other}"),
                data: None,
            });
        }
    };

    let entity = match unit_type {
        units::UnitType::Riflemen => world.spawn(RiflemanBundle::with_faction(x, y, faction)).id(),
        units::UnitType::HeavyWeapons => world.spawn(units::HeavyWeaponsBundle::with_faction(x, y, faction)).id(),
        units::UnitType::LightVehicle => world.spawn(units::LightVehicleBundle::with_faction(x, y, faction)).id(),
        units::UnitType::HeavyArmor => world.spawn(units::HeavyArmorBundle::with_faction(x, y, faction)).id(),
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
        .get::<units::UnitType>()
        .map(|t| format!("{:?}", t))
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

    let faction = match name {
        "combine" => Faction::Combine,
        "covenant" => Faction::Covenant,
        "ironborn" => Faction::Ironborn,
        "hollow" => Faction::Hollow,
        other => {
            return Err(BrpError {
                code: -32602,
                message: format!("unknown faction: {other}"),
                data: None,
            });
        }
    };

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

fn parse_building_type(s: &str) -> Result<BuildingType, BrpError> {
    use BuildingType::*;
    match s {
        "refinery" => Ok(Refinery),
        "scrapyard" => Ok(Scrapyard),
        "recruitment_office" => Ok(RecruitmentOffice),
        "barracks" => Ok(Barracks),
        "motor_pool" => Ok(MotorPool),
        "foundry" => Ok(Foundry),
        "airfield" => Ok(Airfield),
        "workshop" => Ok(Workshop),
        "command_bunker" => Ok(CommandBunker),
        "research_lab" => Ok(ResearchLab),
        "supply_depot" => Ok(SupplyDepot),
        "watchtower" => Ok(Watchtower),
        "repair_bay" => Ok(RepairBay),
        "pillbox" => Ok(Pillbox),
        "aa_gun" => Ok(AAGun),
        "tank_trap" => Ok(TankTrap),
        other => Err(BrpError {
            code: -32602,
            message: format!("unknown building_type: {other}"),
            data: None,
        }),
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

    let cost = building_cost(&building_type);

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

    let bt_for_query = building_type.clone();
    let entity = world
        .spawn(BuildingBundle::new(building_type, faction, x, y))
        .id();

    if !building_produces(&bt_for_query).is_empty() {
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

    let bt = er.get::<BuildingType>().ok_or_else(|| BrpError {
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
        "building_type": format!("{:?}", bt),
        "pos_x": pos.map(|p| p.pos.x),
        "pos_y": pos.map(|p| p.pos.y),
        "faction": faction.map(|f| format!("{:?}", f)),
        "health_current": h.map(|h| h.current),
        "health_max": h.map(|h| h.max),
        "construction_elapsed": cp.map(|c| c.elapsed),
        "construction_total": cp.map(|c| c.total),
        "built": built,
        "under_construction": under,
    }))
}

fn parse_faction(s: &str) -> Result<Faction, BrpError> {
    match s {
        "combine" => Ok(Faction::Combine),
        "covenant" => Ok(Faction::Covenant),
        "ironborn" => Ok(Faction::Ironborn),
        "hollow" => Ok(Faction::Hollow),
        other => Err(BrpError {
            code: -32602,
            message: format!("unknown faction: {other}"),
            data: None,
        }),
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
        "owner": cp.owner.as_ref().map(|f| format!("{:?}", f)),
        "contesting": cp.contesting.as_ref().map(|f| format!("{:?}", f)),
        "capture_progress": cp.capture_progress,
    }))
}

fn parse_unit_type(s: &str) -> Result<units::UnitType, BrpError> {
    match s {
        "rifleman" | "riflemen" => Ok(units::UnitType::Riflemen),
        "heavy_weapons" => Ok(units::UnitType::HeavyWeapons),
        "light_vehicle" => Ok(units::UnitType::LightVehicle),
        "heavy_armor" => Ok(units::UnitType::HeavyArmor),
        other => Err(BrpError {
            code: -32602,
            message: format!("unknown unit_type: {other}"),
            data: None,
        }),
    }
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
    let unit_type = parse_unit_type(params["unit_type"].as_str().ok_or_else(|| BrpError {
        code: -32602,
        message: "unit_type required".into(),
        data: None,
    })?)?;

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
        let bt = r.get::<BuildingType>().cloned().ok_or_else(|| BrpError {
            code: -32602,
            message: "entity is not a building".into(),
            data: None,
        })?;
        let is_built = r.get::<Built>().is_some();
        (bt, is_built)
    };

    // Pay from faction pool, then enqueue on building.
    let cost = production::unit_production_cost(&unit_type);
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
        if !building_produces(&bt).contains(&unit_type) {
            return Err(BrpError {
                code: -32000,
                message: format!("{:?} cannot produce {:?}", bt, unit_type),
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
    queue.jobs.push(unit_type);

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

    let jobs: Vec<String> = q.jobs.iter().map(|u| format!("{:?}", u)).collect();
    Ok(serde_json::json!({
        "jobs": jobs,
        "progress": q.progress,
        "head_total": q.jobs.first().map(|u| production::unit_production_seconds(u)),
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
                "faction": format!("{:?}", fe.faction),
                "fuel": pool.fuel,
                "scrap": pool.scrap,
                "manpower": pool.manpower,
            }));
        }
    }
    let mut units: Vec<Value> = Vec::new();
    {
        let mut q = world.query::<(&units::UnitType, &units::UnitPos, &map::Faction, &Health)>();
        for (ut, pos, faction, h) in q.iter(world) {
            units.push(serde_json::json!({
                "unit_type": format!("{:?}", ut),
                "x": pos.pos.x, "y": pos.pos.y,
                "faction": format!("{:?}", faction),
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
            With<units::UnitType>,
            With<map::ControlPoint>,
            With<BuildingType>,
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
            // For now, only Riflemen are restorable — extend per-type as needed.
            let entity = world.spawn(RiflemanBundle::with_faction(x, y, faction)).id();
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

/// BRP handler for "campaign/init": initializes the default 4-zone campaign state.
fn handle_campaign_init(In(_params): In<Option<Value>>, world: &mut World) -> BrpResult {
    world.insert_resource(default_state());
    Ok(serde_json::json!({ "initialized": true }))
}

fn campaign_state_json(state: &CampaignState) -> serde_json::Value {
    serde_json::json!({
        "act": format!("{:?}", state.act),
        "turn": state.turn,
        "zones": state.zones.iter().map(|z| serde_json::json!({
            "id": z.id,
            "name": z.name,
            "owner": z.owner.as_ref().map(|f| format!("{:?}", f)),
            "corruption": z.corruption,
        })).collect::<Vec<_>>(),
    })
}

fn handle_campaign_state(In(_params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let state = world.get_resource::<CampaignState>().ok_or_else(|| BrpError {
        code: -32000,
        message: "campaign not initialized".into(),
        data: None,
    })?;
    Ok(campaign_state_json(state))
}

/// BRP handler for "campaign/advance": { zone_id, winner, loser }
fn handle_campaign_advance(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let zone_id = params["zone_id"].as_u64().ok_or_else(|| BrpError {
        code: -32602,
        message: "zone_id required".into(),
        data: None,
    })? as u32;
    let winner = parse_faction(params["winner"].as_str().unwrap_or("combine"))?;
    let loser = parse_faction(params["loser"].as_str().unwrap_or("hollow"))?;

    let mut state = world.get_resource_mut::<CampaignState>().ok_or_else(|| BrpError {
        code: -32000,
        message: "campaign not initialized".into(),
        data: None,
    })?;
    apply_mission_outcome(&mut state, zone_id, winner, loser);
    spread_corruption(&mut state);
    Ok(campaign_state_json(&state))
}

fn handle_campaign_options(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params.ok_or_else(|| BrpError {
        code: -32602,
        message: "missing params".into(),
        data: None,
    })?;
    let player = parse_faction(params["player"].as_str().unwrap_or("combine"))?;
    let state = world.get_resource::<CampaignState>().ok_or_else(|| BrpError {
        code: -32000,
        message: "campaign not initialized".into(),
        data: None,
    })?;
    let opts = generate_mission_options(state, &player);
    Ok(serde_json::json!({
        "options": opts.iter().map(|o| serde_json::json!({
            "zone_id": o.zone_id,
            "mission_type": format!("{:?}", o.mission_type),
            "opponent": format!("{:?}", o.opponent),
        })).collect::<Vec<_>>(),
    }))
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
        "player": format!("{:?}", m.player_faction),
        "opponent": format!("{:?}", m.opponent_faction),
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
            With<units::UnitType>,
            With<map::ControlPoint>,
            With<BuildingType>,
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
    world.remove_resource::<CampaignState>();
    Ok(serde_json::json!({ "despawned": count }))
}

fn on_startup() {
    info!("Cindertide initialized");
}
