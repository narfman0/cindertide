use bevy::prelude::*;
use crate::map::GridPos;
use crate::combat::{
    Health, AttackRange, AttackDamage, AttackSpeed, AttackCooldown,
    Suppression, Morale, Facing, morale_state, MoraleState,
};

pub mod movement;

#[derive(Component, Debug, Clone, PartialEq)]
pub enum UnitType {
    Riflemen,
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

/// All components for a Rifleman unit in a single bundle.
#[derive(Bundle)]
pub struct RiflemanBundle {
    pub unit_type: UnitType,
    pub unit_kind: UnitKind,
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
}

impl RiflemanBundle {
    pub fn new(x: i32, y: i32) -> Self {
        Self {
            unit_type: UnitType::Riflemen,
            unit_kind: UnitKind::Infantry,
            pos: UnitPos { pos: GridPos { x, y } },
            facing: Facing::North,
            health: Health { current: 100.0, max: 100.0 },
            attack_range: AttackRange { tiles: 3.0 },
            attack_damage: AttackDamage { base: 20.0, suppression_value: 15.0 },
            attack_speed: AttackSpeed { attacks_per_second: 1.0 },
            attack_cooldown: AttackCooldown { remaining: 0.0 },
            movement_speed: MovementSpeed { tiles_per_second: 2.0 },
            move_progress: MoveProgress::new(vec![]),
            suppression: Suppression { current: 0.0, max: 100.0 },
            morale: Morale { current: 100.0, max: 100.0 },
        }
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

    #[test]
    fn test_rifleman_bundle_default_stats() {
        let r = RiflemanBundle::new(0, 0);
        assert_eq!(r.unit_type, UnitType::Riflemen);
        assert_eq!(r.unit_kind, UnitKind::Infantry);
        assert_eq!(r.health.max, 100.0);
        assert_eq!(r.attack_range.tiles, 3.0);
        assert_eq!(r.attack_damage.base, 20.0);
        assert_eq!(r.attack_damage.suppression_value, 15.0);
        assert_eq!(r.attack_speed.attacks_per_second, 1.0);
        assert_eq!(r.movement_speed.tiles_per_second, 2.0);
        assert_eq!(r.suppression.current, 0.0);
        assert_eq!(r.suppression.max, 100.0);
        assert_eq!(r.morale.current, 100.0);
        assert_eq!(r.morale.max, 100.0);
    }

    #[test]
    fn test_rifleman_bundle_position() {
        let r = RiflemanBundle::new(5, 7);
        assert_eq!(r.pos.pos.x, 5);
        assert_eq!(r.pos.pos.y, 7);
    }

    #[test]
    fn test_rifleman_bundle_ready_to_attack() {
        let r = RiflemanBundle::new(0, 0);
        assert_eq!(r.attack_cooldown.remaining, 0.0);
    }

    #[test]
    fn test_rifleman_full_health_at_spawn() {
        let r = RiflemanBundle::new(0, 0);
        assert_eq!(r.health.current, r.health.max);
    }

    #[test]
    fn test_rifleman_morale_steady_at_spawn() {
        let r = RiflemanBundle::new(0, 0);
        assert_eq!(morale_state(&r.morale), MoraleState::Steady);
    }
}
