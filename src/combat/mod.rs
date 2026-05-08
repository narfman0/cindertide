// Combat module — damage, targeting, and resolution systems.

use bevy::prelude::*;
use crate::map::{CoverDensity, GridPos, Tile, damage_reduction};
use crate::units::UnitPos;

#[derive(Component, Debug, Clone)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

#[derive(Component, Debug, Clone)]
pub struct AttackRange {
    pub tiles: f32,
}

#[derive(Component, Debug, Clone)]
pub struct AttackDamage {
    pub base: f32,
}

#[derive(Component, Debug, Clone)]
pub struct AttackSpeed {
    pub attacks_per_second: f32,
}

#[derive(Component, Debug, Clone)]
pub struct AttackCooldown {
    pub remaining: f32,
}

#[derive(Component, Debug, Clone)]
pub struct AttackTarget {
    pub entity: Entity,
}

#[derive(Component, Debug)]
pub struct Dead;

// --- Pure functions ---

pub fn chebyshev_distance(a: &GridPos, b: &GridPos) -> f32 {
    let dx = (a.x - b.x).abs();
    let dy = (a.y - b.y).abs();
    dx.max(dy) as f32
}

pub fn is_in_range(attacker_pos: &GridPos, target_pos: &GridPos, range: f32) -> bool {
    chebyshev_distance(attacker_pos, target_pos) <= range
}

/// Apply damage to health. Returns true if the unit died (current <= 0).
pub fn apply_damage(health: &mut Health, damage: f32) -> bool {
    health.current -= damage;
    if health.current < 0.0 {
        health.current = 0.0;
    }
    health.current <= 0.0
}

/// Compute final damage after cover reduction. mechanics.md: light=25%, heavy=50%.
pub fn damage_after_cover(base_damage: f32, cover: &CoverDensity) -> f32 {
    base_damage * (1.0 - damage_reduction(cover))
}

// --- Systems ---

pub fn cooldown_system(time: Res<Time>, mut query: Query<&mut AttackCooldown>) {
    for mut cooldown in &mut query {
        cooldown.remaining -= time.delta_secs();
        if cooldown.remaining < 0.0 {
            cooldown.remaining = 0.0;
        }
    }
}

pub fn attack_system(
    mut commands: Commands,
    mut attackers: Query<(Entity, &AttackTarget, &AttackDamage, &mut AttackCooldown, &UnitPos, &AttackSpeed, &AttackRange)>,
    mut targets: Query<(&mut Health, &UnitPos), Without<Dead>>,
    tiles: Query<&Tile>,
) {
    let mut to_remove_target: Vec<Entity> = Vec::new();

    for (attacker_entity, attack_target, damage, mut cooldown, attacker_pos, speed, range) in &mut attackers {
        if cooldown.remaining > 0.0 {
            continue;
        }

        let target_entity = attack_target.entity;

        if let Ok((mut health, target_pos)) = targets.get_mut(target_entity) {
            if is_in_range(&attacker_pos.pos, &target_pos.pos, range.tiles) {
                let cover = tiles
                    .iter()
                    .find(|t| t.pos == target_pos.pos)
                    .map(|t| t.cover.clone())
                    .unwrap_or(CoverDensity::None);
                let final_damage = damage_after_cover(damage.base, &cover);
                let died = apply_damage(&mut health, final_damage);
                cooldown.remaining = 1.0 / speed.attacks_per_second;
                if died {
                    to_remove_target.push(attacker_entity);
                }
            }
        } else {
            // Target missing (dead or despawned), remove target
            to_remove_target.push(attacker_entity);
        }
    }

    for entity in to_remove_target {
        if let Ok(mut e) = commands.get_entity(entity) {
            e.remove::<AttackTarget>();
        }
    }
}

pub fn death_system(
    mut commands: Commands,
    query: Query<(Entity, &Health), Without<Dead>>,
) {
    for (entity, health) in &query {
        if health.current <= 0.0 {
            commands.entity(entity).insert(Dead).remove::<AttackTarget>();
        }
    }
}

// --- Plugin ---

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (cooldown_system, attack_system, death_system).chain());
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    fn make_health(current: f32, max: f32) -> Health {
        Health { current, max }
    }

    fn make_pos(x: i32, y: i32) -> GridPos {
        GridPos { x, y }
    }

    #[test]
    fn test_apply_damage_reduces_health() {
        let mut h = make_health(100.0, 100.0);
        apply_damage(&mut h, 30.0);
        assert_eq!(h.current, 70.0);
    }

    #[test]
    fn test_apply_damage_returns_true_on_death() {
        let mut h = make_health(10.0, 100.0);
        let died = apply_damage(&mut h, 10.0);
        assert!(died);
    }

    #[test]
    fn test_apply_damage_does_not_go_below_zero() {
        let mut h = make_health(5.0, 100.0);
        apply_damage(&mut h, 50.0);
        assert_eq!(h.current, 0.0);
    }

    #[test]
    fn test_chebyshev_distance_adjacent() {
        let a = make_pos(0, 0);
        let b = make_pos(1, 0);
        assert_eq!(chebyshev_distance(&a, &b), 1.0);
    }

    #[test]
    fn test_chebyshev_distance_diagonal() {
        let a = make_pos(0, 0);
        let b = make_pos(1, 1);
        assert_eq!(chebyshev_distance(&a, &b), 1.0);
    }

    #[test]
    fn test_chebyshev_distance_two_away() {
        let a = make_pos(0, 0);
        let b = make_pos(2, 1);
        assert_eq!(chebyshev_distance(&a, &b), 2.0);
    }

    #[test]
    fn test_is_in_range_true() {
        let a = make_pos(0, 0);
        let b = make_pos(2, 2);
        assert!(is_in_range(&a, &b, 2.0));
    }

    #[test]
    fn test_is_in_range_false() {
        let a = make_pos(0, 0);
        let b = make_pos(3, 0);
        assert!(!is_in_range(&a, &b, 2.0));
    }

    #[test]
    fn test_health_not_dead_at_positive() {
        let mut h = make_health(1.0, 100.0);
        let died = apply_damage(&mut h, 0.5);
        assert!(!died);
        assert!(h.current > 0.0);
    }

    #[test]
    fn test_damage_after_no_cover_is_unchanged() {
        assert_eq!(damage_after_cover(40.0, &CoverDensity::None), 40.0);
    }

    #[test]
    fn test_damage_after_light_cover_reduced_25pct() {
        assert_eq!(damage_after_cover(40.0, &CoverDensity::Light), 30.0);
    }

    #[test]
    fn test_damage_after_heavy_cover_reduced_50pct() {
        assert_eq!(damage_after_cover(40.0, &CoverDensity::Heavy), 20.0);
    }
}
