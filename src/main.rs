use bevy::prelude::*;
use bevy::remote::{RemotePlugin, http::RemoteHttpPlugin, BrpResult, BrpError};
use serde_json::Value;

mod map;
mod units;
mod combat;
mod resources;
mod ai;

use map::{MapPlugin, GridPos, Faction};
use units::{UnitPlugin, MoveTarget, UnitPos, RiflemanBundle};
use combat::{CombatPlugin, AttackTarget, Health, Morale, Suppression, Facing, morale_state};
use resources::{ResourcesPlugin, FactionBundle, ResourcePool, ResourceTrickle, ResourceCost, spend};

fn main() {
    App::new()
        .add_plugins(MinimalPlugins)
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
        )
        .add_plugins(RemoteHttpPlugin::default().with_port(15703))
        .add_plugins(MapPlugin)
        .add_plugins(UnitPlugin)
        .add_plugins(CombatPlugin)
        .add_plugins(ResourcesPlugin)
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

    let unit_type = params["unit_type"]
        .as_str()
        .ok_or_else(|| BrpError {
            code: -32602,
            message: "unit_type must be a string".into(),
            data: None,
        })?;

    if unit_type != "rifleman" {
        return Err(BrpError {
            code: -32602,
            message: format!("unknown unit_type: {unit_type}"),
            data: None,
        });
    }

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

    let entity = world.spawn(RiflemanBundle::new(x, y)).id();
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

    Ok(serde_json::json!({
        "fuel": pool.fuel,
        "scrap": pool.scrap,
        "manpower": pool.manpower,
        "fuel_trickle": trickle.map(|t| t.fuel_per_second),
        "scrap_trickle": trickle.map(|t| t.scrap_per_second),
        "manpower_trickle": trickle.map(|t| t.manpower_per_second),
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

fn on_startup() {
    info!("Cindertide initialized");
}
