// Repair — RepairBay buildings passively heal nearby allied vehicles
// at the cost of the faction's scrap.

use bevy::prelude::*;
use crate::map::{Faction, GridPos};
use crate::buildings::{BuildingPos, BuildingTypeId, Built};
use crate::combat::Health;
use crate::units::{UnitPos, UnitKind};
use crate::resources::{ResourcePool, FactionEntity};

pub const REPAIR_RADIUS: f32 = 4.0;
pub const REPAIR_HP_PER_SECOND: f32 = 8.0;
pub const REPAIR_SCRAP_COST_PER_HP: f32 = 0.2;

// --- Pure helpers ---

pub fn within_radius(a: &GridPos, b: &GridPos, r: f32) -> bool {
    let dx = (a.x - b.x).abs() as f32;
    let dy = (a.y - b.y).abs() as f32;
    dx.max(dy) <= r
}

/// Returns the actual HP that can be healed: limited by HP needed and by
/// the scrap budget at REPAIR_SCRAP_COST_PER_HP.
pub fn compute_heal(desired_hp: f32, current: f32, max: f32, faction_scrap: f32) -> f32 {
    let needed = (max - current).max(0.0);
    let by_hp = desired_hp.min(needed);
    let by_scrap = (faction_scrap / REPAIR_SCRAP_COST_PER_HP).max(0.0);
    by_hp.min(by_scrap)
}

// --- System ---

pub fn repair_passive_system(
    time: Res<Time>,
    bays: Query<(&BuildingPos, &Faction, &BuildingTypeId), With<Built>>,
    mut units: Query<(&UnitPos, &Faction, &UnitKind, &mut Health)>,
    mut factions: Query<(&FactionEntity, &mut ResourcePool)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    let bay_list: Vec<(GridPos, Faction)> = bays
        .iter()
        .filter(|(_, _, bt)| bt.id() == "repair_bay")
        .map(|(p, f, _)| (p.pos.clone(), f.clone()))
        .collect();

    if bay_list.is_empty() {
        return;
    }

    for (upos, ufaction, ukind, mut health) in &mut units {
        if !matches!(ukind, UnitKind::Vehicle) {
            continue;
        }
        if health.current >= health.max {
            continue;
        }
        let covered = bay_list
            .iter()
            .any(|(bp, bf)| bf == ufaction && within_radius(bp, &upos.pos, REPAIR_RADIUS));
        if !covered {
            continue;
        }

        for (fe, mut pool) in &mut factions {
            if &fe.faction != ufaction {
                continue;
            }
            let amount = compute_heal(
                REPAIR_HP_PER_SECOND * dt,
                health.current,
                health.max,
                pool.scrap,
            );
            if amount > 0.0 {
                pool.scrap = (pool.scrap - amount * REPAIR_SCRAP_COST_PER_HP).max(0.0);
                health.current = (health.current + amount).min(health.max);
            }
            break;
        }
    }
}

pub struct RepairPlugin;

impl Plugin for RepairPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, repair_passive_system);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn within_radius_chebyshev() {
        let a = GridPos { x: 0, y: 0 };
        assert!(within_radius(&a, &GridPos { x: 4, y: 4 }, 4.0));
        assert!(!within_radius(&a, &GridPos { x: 5, y: 0 }, 4.0));
    }

    #[test]
    fn compute_heal_capped_by_max() {
        assert_eq!(compute_heal(50.0, 95.0, 100.0, 1000.0), 5.0);
    }

    #[test]
    fn compute_heal_capped_by_scrap() {
        assert_eq!(compute_heal(100.0, 0.0, 1000.0, 100.0), 100.0);
        // 1 scrap / 0.2 = 5 HP cap.
        assert_eq!(compute_heal(100.0, 0.0, 1000.0, 1.0), 5.0);
    }

    #[test]
    fn compute_heal_zero_when_full_hp() {
        assert_eq!(compute_heal(50.0, 100.0, 100.0, 1000.0), 0.0);
    }

    #[test]
    fn compute_heal_zero_when_no_scrap() {
        assert_eq!(compute_heal(50.0, 50.0, 100.0, 0.0), 0.0);
    }
}
