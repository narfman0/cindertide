// Lua AI scripting engine — each AI faction can override ai_tick() and on_wave()
// via assets/ai/<faction_name_lowercase>.lua, falling back to assets/ai/default.lua.

use mlua::prelude::*;
use bevy::prelude::*;
use std::collections::HashMap;

use crate::map::{Faction, GridPos, Tile};
use crate::units::{UnitPos, UnitType};
use crate::buildings::{BuildingPos, BuildingType, Built};
use crate::combat::{AttackTarget, Health, AttackMoveOrder, InCover};
use crate::resources::FactionEntity;
use crate::ai::{AiController, AiStates};

// ------------------------------------------------------------------
// Engine wrapper
// ------------------------------------------------------------------

pub struct LuaAiEngine {
    pub lua: Lua,
}

impl LuaAiEngine {
    /// Load a Lua script for the given faction.  Tries
    /// `assets/ai/<faction_lower>.lua`, then falls back to the embedded
    /// `default.lua` source so the binary always has a working script.
    pub fn load(faction: &Faction) -> Result<Self, LuaError> {
        let faction_name = faction_to_script_name(faction);
        let path = format!("assets/ai/{}.lua", faction_name);

        let script = std::fs::read_to_string(&path)
            .or_else(|_| std::fs::read_to_string("assets/ai/default.lua"))
            .unwrap_or_else(|_| DEFAULT_LUA_SCRIPT.to_string());

        let lua = Lua::new();
        lua.load(&script).exec()?;
        Ok(Self { lua })
    }
}

fn faction_to_script_name(faction: &Faction) -> &'static str {
    match faction {
        Faction::Ironborn => "ironborn",
        Faction::Combine  => "combine",
        Faction::Covenant => "covenant",
        Faction::Hollow   => "hollow",
    }
}

// Minimal embedded fallback so the binary compiles without file-system assets.
const DEFAULT_LUA_SCRIPT: &str = r#"
function ai_tick(state)
    local cmds = {}
    for _, unit in ipairs(state.my_units) do
        for _, e in ipairs(state.enemy_visible) do
            table.insert(cmds, { cmd = "attack", unit_id = unit.id, target_id = e.id })
            break
        end
    end
    return cmds
end
function on_wave(state)
    local cmds = {}
    local tx, ty = 64, 40
    if #state.enemy_visible > 0 then tx = state.enemy_visible[1].x; ty = state.enemy_visible[1].y end
    for _, unit in ipairs(state.my_units) do
        table.insert(cmds, { cmd = "attack_move", unit_id = unit.id, x = tx, y = ty })
    end
    return cmds
end
"#;

// ------------------------------------------------------------------
// LuaAiEngines NonSend resource
// ------------------------------------------------------------------

/// Stores one LuaAiEngine per AI-controlled faction.
/// NonSend because `Lua` is not `Send`.
#[derive(Default)]
pub struct LuaAiEngines(pub HashMap<Faction, LuaAiEngine>);

// ------------------------------------------------------------------
// Helper: build the Lua state table for one faction
// ------------------------------------------------------------------

fn faction_name_str(f: &Faction) -> &'static str {
    match f {
        Faction::Combine  => "Combine",
        Faction::Ironborn => "Ironborn",
        Faction::Covenant => "Covenant",
        Faction::Hollow   => "Hollow",
    }
}

fn unit_type_str(ut: &UnitType) -> &'static str {
    match ut {
        UnitType::Riflemen    => "Riflemen",
        UnitType::HeavyWeapons => "HeavyWeapons",
        UnitType::LightVehicle => "LightVehicle",
        UnitType::HeavyArmor  => "HeavyArmor",
    }
}

// ------------------------------------------------------------------
// Lua AI system
// ------------------------------------------------------------------

/// Runs every frame.  For each AI faction, checks whether `reaction_time`
/// seconds have elapsed and calls `ai_tick` via the faction's Lua engine.
/// Returned commands are applied as ECS mutations.
#[allow(clippy::too_many_arguments)]
pub fn lua_ai_system(
    world: &mut World,
) {
    // Collect AI faction data without holding borrow across mutations
    let ai_faction_data: Vec<(Faction, f32, GridPos)> = {
        let mut q = world.query::<(&FactionEntity, &AiController)>();
        q.iter(world)
            .map(|(fe, ctrl)| (fe.faction.clone(), ctrl.last_tick, ctrl.home.clone()))
            .collect()
    };

    let now = world.resource::<Time>().elapsed_secs();

    for (faction, _last_tick, home) in &ai_faction_data {
        // Get reaction_time from AiStates config
        let reaction_time = {
            let states = world.resource::<AiStates>();
            states.get(faction)
                .map(|s| s.config.params.reaction_time)
                .unwrap_or(1.5)
        };

        // Check if enough time has passed
        let should_tick = {
            let mut q = world.query::<(&FactionEntity, &AiController)>();
            q.iter(world)
                .filter(|(fe, _)| &fe.faction == faction)
                .any(|(_, ctrl)| now - ctrl.last_tick >= reaction_time)
        };

        if !should_tick {
            continue;
        }

        // Build state table data
        let my_units: Vec<(Entity, i32, i32, f32, f32, bool, &'static str)> = {
            let mut q = world.query::<(Entity, &Faction, &UnitPos, &Health, Option<&InCover>, &UnitType)>();
            q.iter(world)
                .filter(|(_, f, _, _, _, _)| *f == faction)
                .map(|(e, _, pos, hp, cover, ut)| {
                    (e, pos.pos.x, pos.pos.y, hp.current, hp.max, cover.is_some(), unit_type_str(ut))
                })
                .collect()
        };

        let my_buildings: Vec<(Entity, i32, i32, f32, &'static str)> = {
            let mut q = world.query_filtered::<(Entity, &Faction, &BuildingPos, &Health, &BuildingType), With<Built>>();
            q.iter(world)
                .filter(|(_, f, _, _, _)| *f == faction)
                .map(|(e, _, pos, hp, bt)| {
                    let type_str: &'static str = match bt {
                        BuildingType::CommandBunker => "CommandBunker",
                        BuildingType::Barracks => "Barracks",
                        BuildingType::Refinery => "Refinery",
                        _ => "Building",
                    };
                    (e, pos.pos.x, pos.pos.y, hp.current, type_str)
                })
                .collect()
        };

        // Enemy visible: simple approximation — all non-faction units
        let enemy_visible: Vec<(Entity, i32, i32, f32, &'static str, &'static str)> = {
            let mut q = world.query::<(Entity, &Faction, &UnitPos, &Health, &UnitType)>();
            q.iter(world)
                .filter(|(_, f, _, _, _)| *f != faction)
                .map(|(e, f, pos, hp, ut)| {
                    (e, pos.pos.x, pos.pos.y, hp.current, unit_type_str(ut), faction_name_str(f))
                })
                .collect()
        };

        // Cover positions near friendly units (Forest/Rubble tiles within scout_radius)
        let scout_radius = {
            let states = world.resource::<AiStates>();
            states.get(faction).map(|s| s.config.params.scout_radius).unwrap_or(10)
        };

        let cover_positions: Vec<(i32, i32)> = {
            let mut q = world.query::<&Tile>();
            q.iter(world)
                .filter(|t| {
                    matches!(t.terrain_type, crate::map::TerrainType::Forest | crate::map::TerrainType::Rubble)
                        && (t.pos.x - home.x).abs().max((t.pos.y - home.y).abs()) <= scout_radius
                })
                .map(|t| (t.pos.x, t.pos.y))
                .collect()
        };

        let (aggression, focus_fire, resource_multiplier) = {
            let states = world.resource::<AiStates>();
            if let Some(s) = states.get(faction) {
                (s.config.params.aggression, s.config.params.focus_fire, s.config.cheats.resource_multiplier)
            } else {
                (0.6, true, 1.0)
            }
        };
        let _ = (aggression, resource_multiplier);

        // Call Lua ai_tick
        let commands_data: Vec<LuaCommand> = {
            let engines = world.non_send_resource::<LuaAiEngines>();
            if let Some(engine) = engines.0.get(faction) {
                match call_ai_tick(engine, faction, &my_units, &my_buildings, &enemy_visible, &cover_positions, focus_fire) {
                    Ok(cmds) => cmds,
                    Err(e) => {
                        warn!("Lua AI error for {:?}: {}", faction, e);
                        vec![]
                    }
                }
            } else {
                vec![]
            }
        };

        // Apply commands
        apply_lua_commands(world, faction, &commands_data, &my_units, &enemy_visible);

        // Update last_tick
        {
            let mut q = world.query::<(&FactionEntity, &mut AiController)>();
            for (fe, mut ctrl) in q.iter_mut(world) {
                if &fe.faction == faction {
                    ctrl.last_tick = now;
                }
            }
        }
    }
}

// ------------------------------------------------------------------
// Parsed command from Lua
// ------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum LuaCommand {
    Move { unit_id: i64, x: i32, y: i32 },
    Attack { unit_id: i64, target_id: i64 },
    AttackMove { unit_id: i64, x: i32, y: i32 },
}

fn call_ai_tick(
    engine: &LuaAiEngine,
    faction: &Faction,
    my_units: &[(Entity, i32, i32, f32, f32, bool, &'static str)],
    my_buildings: &[(Entity, i32, i32, f32, &'static str)],
    enemy_visible: &[(Entity, i32, i32, f32, &'static str, &'static str)],
    cover_positions: &[(i32, i32)],
    focus_fire: bool,
) -> Result<Vec<LuaCommand>, LuaError> {
    let lua = &engine.lua;

    // Build state table
    let state = lua.create_table()?;
    state.set("faction", faction_name_str(faction))?;

    // my_units array
    let units_tbl = lua.create_table()?;
    for (i, (entity, x, y, hp, max_hp, in_cover, ut)) in my_units.iter().enumerate() {
        let u = lua.create_table()?;
        // Use entity index as a stable integer ID
        u.set("id", entity.index() as i64)?;
        u.set("type", *ut)?;
        u.set("x", *x)?;
        u.set("y", *y)?;
        u.set("health", *hp)?;
        u.set("max_health", *max_hp)?;
        u.set("in_cover", *in_cover)?;
        units_tbl.set(i + 1, u)?;
    }
    state.set("my_units", units_tbl)?;

    // my_buildings array
    let bldg_tbl = lua.create_table()?;
    for (i, (entity, x, y, hp, bt)) in my_buildings.iter().enumerate() {
        let b = lua.create_table()?;
        b.set("id", entity.index() as i64)?;
        b.set("type", *bt)?;
        b.set("x", *x)?;
        b.set("y", *y)?;
        b.set("health", *hp)?;
        bldg_tbl.set(i + 1, b)?;
    }
    state.set("my_buildings", bldg_tbl)?;

    // enemy_visible array
    let enemy_tbl = lua.create_table()?;
    for (i, (entity, x, y, hp, ut, f)) in enemy_visible.iter().enumerate() {
        let e = lua.create_table()?;
        e.set("id", entity.index() as i64)?;
        e.set("type", *ut)?;
        e.set("x", *x)?;
        e.set("y", *y)?;
        e.set("health", *hp)?;
        e.set("faction", *f)?;
        enemy_tbl.set(i + 1, e)?;
    }
    state.set("enemy_visible", enemy_tbl)?;

    // cover_positions array
    let cover_tbl = lua.create_table()?;
    for (i, (x, y)) in cover_positions.iter().enumerate() {
        let c = lua.create_table()?;
        c.set("x", *x)?;
        c.set("y", *y)?;
        cover_tbl.set(i + 1, c)?;
    }
    state.set("cover_positions", cover_tbl)?;

    // params sub-table
    let params = lua.create_table()?;
    params.set("focus_fire", focus_fire)?;
    state.set("params", params)?;

    // Call ai_tick(state)
    let func: LuaFunction = lua.globals().get("ai_tick")?;
    let result: LuaTable = func.call(state)?;

    // Parse returned commands
    let mut cmds = Vec::new();
    for pair in result.pairs::<LuaValue, LuaTable>() {
        let (_, tbl) = pair?;
        let cmd_str: String = tbl.get("cmd")?;
        match cmd_str.as_str() {
            "move" => {
                let unit_id: i64 = tbl.get("unit_id")?;
                let x: i32 = tbl.get("x")?;
                let y: i32 = tbl.get("y")?;
                cmds.push(LuaCommand::Move { unit_id, x, y });
            }
            "attack" => {
                let unit_id: i64 = tbl.get("unit_id")?;
                let target_id: i64 = tbl.get("target_id")?;
                cmds.push(LuaCommand::Attack { unit_id, target_id });
            }
            "attack_move" => {
                let unit_id: i64 = tbl.get("unit_id")?;
                let x: i32 = tbl.get("x")?;
                let y: i32 = tbl.get("y")?;
                cmds.push(LuaCommand::AttackMove { unit_id, x, y });
            }
            _ => {} // ignore unknown commands
        }
    }

    Ok(cmds)
}

fn apply_lua_commands(
    world: &mut World,
    faction: &Faction,
    commands: &[LuaCommand],
    my_units: &[(Entity, i32, i32, f32, f32, bool, &'static str)],
    enemy_visible: &[(Entity, i32, i32, f32, &'static str, &'static str)],
) {
    // Build lookup: entity index → Entity
    let unit_by_id: HashMap<u32, Entity> = my_units.iter()
        .map(|(e, _, _, _, _, _, _)| (e.index(), *e))
        .collect();
    let enemy_by_id: HashMap<u32, Entity> = enemy_visible.iter()
        .map(|(e, _, _, _, _, _)| (e.index(), *e))
        .collect();
    let _ = faction;

    for cmd in commands {
        match cmd {
            LuaCommand::Move { unit_id, x, y } => {
                if let Some(&entity) = unit_by_id.get(&(*unit_id as u32)) {
                    let target = GridPos { x: *x, y: *y };
                    world.entity_mut(entity)
                        .insert(crate::units::MoveTarget { target: target.clone() })
                        .insert(crate::units::MoveProgress { path: vec![target], current_step: 0, elapsed: 0.0 });
                }
            }
            LuaCommand::Attack { unit_id, target_id } => {
                if let Some(&entity) = unit_by_id.get(&(*unit_id as u32)) {
                    if let Some(&target_entity) = enemy_by_id.get(&(*target_id as u32)) {
                        world.entity_mut(entity).insert(AttackTarget { entity: target_entity });
                    }
                }
            }
            LuaCommand::AttackMove { unit_id, x, y } => {
                if let Some(&entity) = unit_by_id.get(&(*unit_id as u32)) {
                    world.entity_mut(entity).insert(AttackMoveOrder {
                        target: GridPos { x: *x, y: *y },
                    });
                }
            }
        }
    }
}
