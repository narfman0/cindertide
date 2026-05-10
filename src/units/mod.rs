use bevy::prelude::*;
use crate::map::{GridPos, Faction};
use crate::combat::{
    Health, AttackRange, AttackDamage, AttackSpeed, AttackCooldown,
    Suppression, Morale, Facing,
};
use crate::heroes::SuppressionResist;

pub mod movement;

#[derive(Component, Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnitTypeId(pub String);

impl UnitTypeId {
    pub fn new(id: &str) -> Self { UnitTypeId(id.to_string()) }
    pub fn id(&self) -> &str { &self.0 }
}

/// Unit kind used for unit bundles. Mirrors pathfinding::UnitKind but is the
/// canonical ECS component version.
#[derive(Component, Debug, Clone, PartialEq)]
pub enum UnitKind {
    Infantry,
    Vehicle,
}

// Current grid position of a unit
#[derive(Component, Debug, Clone)]
pub struct UnitPos {
    pub pos: GridPos,
}

/// Where a unit was spawned. Routing units retreat toward this position.
#[derive(Component, Debug, Clone)]
pub struct HomeBase {
    pub pos: GridPos,
}

// Destination when moving
#[derive(Component, Debug, Clone)]
pub struct MoveTarget {
    pub target: GridPos,
}

// Movement speed in tiles per second
#[derive(Component, Debug, Clone)]
pub struct MovementSpeed {
    pub tiles_per_second: f32,
}

// Time accumulator for movement
#[derive(Component, Debug, Clone)]
pub struct MoveProgress {
    pub elapsed: f32,
    pub path: Vec<GridPos>,
    pub current_step: usize,
}

impl MoveProgress {
    pub fn new(path: Vec<GridPos>) -> Self {
        Self {
            elapsed: 0.0,
            path,
            current_step: 0,
        }
    }
}

/// Generic unit bundle — all components for any unit type.
#[derive(Bundle)]
pub struct UnitBundle {
    pub unit_type: UnitTypeId,
    pub unit_kind: UnitKind,
    pub faction: Faction,
    pub pos: UnitPos,
    pub facing: Facing,
    pub health: Health,
    pub attack_range: AttackRange,
    pub attack_damage: AttackDamage,
    pub attack_speed: AttackSpeed,
    pub attack_cooldown: AttackCooldown,
    pub movement_speed: MovementSpeed,
    pub move_progress: MoveProgress,
    pub suppression: Suppression,
    pub morale: Morale,
    pub suppression_resist: SuppressionResist,
}

impl UnitBundle {
    pub fn from_def(def: &crate::factions::UnitDef, faction: Faction, x: i32, y: i32) -> Self {
        let unit_kind = if def.unit_kind == "vehicle" { UnitKind::Vehicle } else { UnitKind::Infantry };
        Self {
            unit_type: UnitTypeId::new(&def.id),
            unit_kind,
            faction,
            pos: UnitPos { pos: GridPos { x, y } },
            facing: Facing::North,
            health: Health { current: def.health, max: def.health },
            attack_range: AttackRange { tiles: def.attack_range },
            attack_damage: AttackDamage { base: def.attack_damage, suppression_value: def.suppression_value },
            attack_speed: AttackSpeed { attacks_per_second: def.attack_speed },
            attack_cooldown: AttackCooldown { remaining: 0.0 },
            movement_speed: MovementSpeed { tiles_per_second: def.move_speed },
            move_progress: MoveProgress::new(vec![]),
            suppression: Suppression { current: 0.0, max: 100.0 },
            morale: Morale { current: 100.0, max: 100.0 },
            suppression_resist: SuppressionResist { fraction: def.suppression_resistance.clamp(0.0, 1.0) },
        }
    }

    /// Convenience constructor for a unit with a given ID, using default stats.
    /// Used by the editor when LoadedFactions is not accessible via Commands.
    pub fn default_riflemen_id(unit_id: &str, faction: Faction, x: i32, y: i32) -> Self {
        let mut b = Self::default_riflemen(faction, x, y);
        b.unit_type = UnitTypeId::new(unit_id);
        b
    }

    /// Convenience constructor for a default riflemen unit when no faction data is available.
    pub fn default_riflemen(faction: Faction, x: i32, y: i32) -> Self {
        Self {
            unit_type: UnitTypeId::new("riflemen"),
            unit_kind: UnitKind::Infantry,
            faction,
            pos: UnitPos { pos: GridPos { x, y } },
            facing: Facing::North,
            health: Health { current: 80.0, max: 80.0 },
            attack_range: AttackRange { tiles: 4.0 },
            attack_damage: AttackDamage { base: 12.0, suppression_value: 15.0 },
            attack_speed: AttackSpeed { attacks_per_second: 1.2 },
            attack_cooldown: AttackCooldown { remaining: 0.0 },
            movement_speed: MovementSpeed { tiles_per_second: 2.5 },
            move_progress: MoveProgress::new(vec![]),
            suppression: Suppression { current: 0.0, max: 100.0 },
            morale: Morale { current: 100.0, max: 100.0 },
            suppression_resist: SuppressionResist { fraction: 0.0 },
        }
    }
}

/// Per-type stats for UI display and system use.
pub struct UnitStats {
    pub attack_range: f32,
    pub move_speed: f32,
    pub health: f32,
    pub attack_damage: f32,
    pub attack_rate: f32,
    pub vision_range: f32,
    pub description: String,
}

pub fn unit_stats_from_def(def: &crate::factions::UnitDef) -> UnitStats {
    UnitStats {
        attack_range: def.attack_range,
        move_speed: def.move_speed,
        health: def.health,
        attack_damage: def.attack_damage,
        attack_rate: def.attack_speed,
        vision_range: def.vision_range,
        description: def.description.clone(),
    }
}

pub struct UnitPlugin;

impl Plugin for UnitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, movement::move_units_system);
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    fn make_riflemen_def() -> crate::factions::UnitDef {
        crate::factions::UnitDef {
            id: "riflemen".to_string(),
            display_name: "Riflemen".to_string(),
            unit_kind: "infantry".to_string(),
            attack_range: 4.0,
            move_speed: 2.5,
            health: 80.0,
            attack_damage: 12.0,
            suppression_value: 15.0,
            suppression_resistance: 0.0,
            attack_speed: 1.2,
            vision_range: 6.0,
            production_cost: Default::default(),
            produced_at: vec![],
            build_time_seconds: 8.0,
            ability_q: String::new(),
            model_file: String::new(),
            description: "Standard all-rounder infantry".to_string(),
        }
    }

    fn make_vehicle_def() -> crate::factions::UnitDef {
        crate::factions::UnitDef {
            id: "light_vehicle".to_string(),
            display_name: "Light Vehicle".to_string(),
            unit_kind: "vehicle".to_string(),
            attack_range: 5.0,
            move_speed: 4.0,
            health: 100.0,
            attack_damage: 20.0,
            suppression_value: 10.0,
            suppression_resistance: 0.0,
            attack_speed: 0.8,
            vision_range: 10.0,
            production_cost: Default::default(),
            produced_at: vec![],
            build_time_seconds: 18.0,
            ability_q: String::new(),
            model_file: String::new(),
            description: "Fast scout vehicle".to_string(),
        }
    }

    fn make_heavy_def() -> crate::factions::UnitDef {
        crate::factions::UnitDef {
            id: "heavy_armor".to_string(),
            display_name: "Heavy Armor".to_string(),
            unit_kind: "vehicle".to_string(),
            attack_range: 4.0,
            move_speed: 1.8,
            health: 250.0,
            attack_damage: 50.0,
            suppression_value: 25.0,
            suppression_resistance: 0.0,
            attack_speed: 0.4,
            vision_range: 4.0,
            production_cost: Default::default(),
            produced_at: vec![],
            build_time_seconds: 35.0,
            ability_q: String::new(),
            model_file: String::new(),
            description: "Heavy front-line tank".to_string(),
        }
    }

    #[test]
    fn test_unit_bundle_from_def_infantry() {
        let def = make_riflemen_def();
        let b = UnitBundle::from_def(&def, Faction::combine(), 5, 7);
        assert_eq!(b.unit_type, UnitTypeId::new("riflemen"));
        assert_eq!(b.unit_kind, UnitKind::Infantry);
        assert_eq!(b.pos.pos.x, 5);
        assert_eq!(b.pos.pos.y, 7);
        assert_eq!(b.health.max, 80.0);
        assert_eq!(b.health.current, 80.0);
        assert_eq!(b.attack_range.tiles, 4.0);
        assert_eq!(b.attack_damage.base, 12.0);
        assert_eq!(b.attack_damage.suppression_value, 15.0);
        assert_eq!(b.movement_speed.tiles_per_second, 2.5);
        assert_eq!(b.attack_cooldown.remaining, 0.0);
        assert_eq!(b.suppression.current, 0.0);
        assert_eq!(b.morale.current, 100.0);
    }

    #[test]
    fn test_unit_bundle_from_def_vehicle() {
        let def = make_vehicle_def();
        let b = UnitBundle::from_def(&def, Faction::ironborn(), 0, 0);
        assert_eq!(b.unit_kind, UnitKind::Vehicle);
        assert!(b.movement_speed.tiles_per_second > 2.0);
    }

    #[test]
    fn test_unit_bundle_heavy_armor() {
        let def = make_heavy_def();
        let b = UnitBundle::from_def(&def, Faction::combine(), 0, 0);
        assert_eq!(b.unit_kind, UnitKind::Vehicle);
        assert!(b.health.max >= 2.0 * 80.0); // heavy armor is significantly tougher than infantry
        assert!(b.attack_damage.base >= 50.0);
    }

    #[test]
    fn test_unit_bundle_default_riflemen() {
        let b = UnitBundle::default_riflemen(Faction::combine(), 3, 4);
        assert_eq!(b.unit_type.id(), "riflemen");
        assert_eq!(b.pos.pos.x, 3);
        assert_eq!(b.pos.pos.y, 4);
        assert_eq!(b.health.current, b.health.max);
    }
}
