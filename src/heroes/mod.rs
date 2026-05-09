// Heroes — persistent commanders with auras and signature abilities.

use bevy::prelude::*;
use crate::map::{Faction, GridPos};
use crate::combat::{Health, Suppression, AttackRange, AttackDamage, AttackSpeed, AttackCooldown, Morale, Facing};
use crate::units::{UnitPos, UnitKind, MovementSpeed, MoveProgress};

#[derive(Component, Debug, Clone)]
pub struct Hero {
    pub name: String,
}

/// Marker — hero is downed (HP 0) but recoverable. Not the same as Dead.
#[derive(Component, Debug)]
pub struct HeroDowned;

#[derive(Debug, Clone, PartialEq)]
pub enum AbilityKind {
    Rally,        // clears suppression on allies in aura radius
    AreaDamage,   // damages enemies in aura radius
}

#[derive(Component, Debug, Clone)]
pub struct SignatureAbility {
    pub charge: f32,
    pub max_charge: f32,
    pub charge_per_second: f32,
    pub kind: AbilityKind,
}

#[derive(Component, Debug, Clone)]
pub struct Aura {
    pub radius: f32,
    pub suppression_resist: f32, // 0..1
}

/// Set on units currently within a friendly hero's aura. Consumed by combat
/// to reduce suppression accumulation on hits.
#[derive(Component, Debug, Clone)]
pub struct SuppressionResist {
    pub fraction: f32,
}

// --- Pure functions ---

pub fn is_charge_full(s: &SignatureAbility) -> bool {
    s.charge >= s.max_charge
}

pub fn within_aura(hero_pos: &GridPos, unit_pos: &GridPos, radius: f32) -> bool {
    let dx = (hero_pos.x - unit_pos.x).abs() as f32;
    let dy = (hero_pos.y - unit_pos.y).abs() as f32;
    dx.max(dy) <= radius
}

// --- Bundle ---

#[derive(Bundle)]
pub struct HeroBundle {
    pub hero: Hero,
    pub faction: Faction,
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
    pub ability: SignatureAbility,
    pub aura: Aura,
}

impl HeroBundle {
    pub fn new(name: impl Into<String>, x: i32, y: i32, faction: Faction, kind: AbilityKind) -> Self {
        Self {
            hero: Hero { name: name.into() },
            faction,
            unit_kind: UnitKind::Infantry,
            pos: UnitPos { pos: GridPos { x, y } },
            facing: Facing::North,
            health: Health { current: 500.0, max: 500.0 }, // Heroes are tanky
            attack_range: AttackRange { tiles: 4.0 },
            attack_damage: AttackDamage { base: 35.0, suppression_value: 20.0 },
            attack_speed: AttackSpeed { attacks_per_second: 1.5 },
            attack_cooldown: AttackCooldown { remaining: 0.0 },
            movement_speed: MovementSpeed { tiles_per_second: 2.5 },
            move_progress: MoveProgress::new(vec![]),
            suppression: Suppression { current: 0.0, max: 200.0 }, // resistant
            morale: Morale { current: 100.0, max: 100.0 },
            ability: SignatureAbility {
                charge: 0.0,
                max_charge: 100.0,
                charge_per_second: 5.0,
                kind,
            },
            aura: Aura {
                radius: 5.0,
                suppression_resist: 0.5, // 50% reduction inside aura
            },
        }
    }
}

// --- Systems ---

pub fn charge_system(time: Res<Time>, mut q: Query<&mut SignatureAbility, Without<HeroDowned>>) {
    let dt = time.delta_secs();
    for mut s in &mut q {
        if s.charge < s.max_charge {
            s.charge = (s.charge + s.charge_per_second * dt).min(s.max_charge);
        }
    }
}

/// Apply / refresh / clear SuppressionResist on units inside friendly hero auras.
pub fn aura_system(
    mut commands: Commands,
    heroes: Query<(&UnitPos, &Faction, &Aura), (With<Hero>, Without<HeroDowned>)>,
    units: Query<(Entity, &UnitPos, &Faction), Without<Hero>>,
    existing: Query<&SuppressionResist>,
) {
    use std::collections::HashMap;
    let mut best: HashMap<Entity, f32> = HashMap::new();

    for (h_pos, h_faction, aura) in &heroes {
        for (u_entity, u_pos, u_faction) in &units {
            if u_faction != h_faction {
                continue;
            }
            if within_aura(&h_pos.pos, &u_pos.pos, aura.radius) {
                let cur = best.get(&u_entity).copied().unwrap_or(0.0);
                if aura.suppression_resist > cur {
                    best.insert(u_entity, aura.suppression_resist);
                }
            }
        }
    }

    // Update / insert
    for (entity, fraction) in &best {
        let needs = match existing.get(*entity) {
            Ok(e) => (e.fraction - fraction).abs() > 1e-3,
            Err(_) => true,
        };
        if needs {
            commands.entity(*entity).insert(SuppressionResist { fraction: *fraction });
        }
    }

    // Remove on units no longer in any aura
    for (u_entity, _, _) in &units {
        if !best.contains_key(&u_entity) && existing.get(u_entity).is_ok() {
            commands.entity(u_entity).remove::<SuppressionResist>();
        }
    }
}

/// Heroes don't die — when health hits 0 they are downed (recoverable).
pub fn hero_down_system(
    mut commands: Commands,
    q: Query<(Entity, &Health), (With<Hero>, Without<HeroDowned>)>,
) {
    for (entity, h) in &q {
        if h.current <= 0.0 {
            commands.entity(entity).insert(HeroDowned);
        }
    }
}

pub struct HeroPlugin;

impl Plugin for HeroPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (charge_system, aura_system, hero_down_system).chain());
    }
}

// suppress unused warning on imports re-used elsewhere
const _: Option<&str> = None;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charge_full_at_max() {
        let s = SignatureAbility {
            charge: 100.0, max_charge: 100.0, charge_per_second: 5.0, kind: AbilityKind::Rally,
        };
        assert!(is_charge_full(&s));
    }

    #[test]
    fn charge_not_full_below_max() {
        let s = SignatureAbility {
            charge: 99.0, max_charge: 100.0, charge_per_second: 5.0, kind: AbilityKind::Rally,
        };
        assert!(!is_charge_full(&s));
    }

    #[test]
    fn within_aura_chebyshev() {
        let h = GridPos { x: 0, y: 0 };
        assert!(within_aura(&h, &GridPos { x: 5, y: 5 }, 5.0));
        assert!(!within_aura(&h, &GridPos { x: 6, y: 0 }, 5.0));
    }

    #[test]
    fn hero_bundle_starts_with_full_health_and_no_charge() {
        let h = HeroBundle::new("Test", 0, 0, Faction::combine(), AbilityKind::Rally);
        assert_eq!(h.health.current, h.health.max);
        assert_eq!(h.ability.charge, 0.0);
        assert_eq!(h.hero.name, "Test");
    }

    #[test]
    fn hero_bundle_aura_has_suppression_resist() {
        let h = HeroBundle::new("R", 0, 0, Faction::combine(), AbilityKind::Rally);
        assert!(h.aura.suppression_resist > 0.0);
        assert!(h.aura.radius > 0.0);
    }
}
