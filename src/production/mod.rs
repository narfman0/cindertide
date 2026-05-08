// Production — per-building unit queues with cost-up-front and progress.

use bevy::prelude::*;
use crate::map::{Faction, GridPos};
use crate::units::{UnitType, RiflemanBundle, HeavyWeaponsBundle, LightVehicleBundle, HeavyArmorBundle, HomeBase};
use crate::buildings::{BuildingPos, BuildingType, Built};
use crate::resources::{ResourcePool, ResourceCost, FactionEntity, can_afford, spend, refund};

pub const QUEUE_CAP: usize = 5;

#[derive(Component, Debug, Clone, Default)]
pub struct ProductionQueue {
    pub jobs: Vec<UnitType>,
    pub progress: f32,
}

// --- Pure tables ---

pub fn unit_production_seconds(ut: &UnitType) -> f32 {
    match ut {
        UnitType::Riflemen => 8.0,
        UnitType::HeavyWeapons => 14.0,
        UnitType::LightVehicle => 18.0,
        UnitType::HeavyArmor => 35.0,
    }
}

pub fn unit_production_cost(ut: &UnitType) -> ResourceCost {
    match ut {
        UnitType::Riflemen => ResourceCost { fuel: 0.0, scrap: 50.0, manpower: 5.0 },
        UnitType::HeavyWeapons => ResourceCost { fuel: 10.0, scrap: 80.0, manpower: 5.0 },
        UnitType::LightVehicle => ResourceCost { fuel: 80.0, scrap: 60.0, manpower: 3.0 },
        UnitType::HeavyArmor => ResourceCost { fuel: 250.0, scrap: 200.0, manpower: 5.0 },
    }
}

/// Which unit categories does this building produce?
pub fn building_produces(bt: &BuildingType) -> &'static [UnitType] {
    match bt {
        BuildingType::Barracks => &[UnitType::Riflemen, UnitType::HeavyWeapons],
        BuildingType::MotorPool => &[UnitType::LightVehicle, UnitType::HeavyArmor],
        _ => &[],
    }
}

#[derive(Debug, PartialEq)]
pub enum EnqueueError {
    QueueFull,
    Unaffordable,
    BuildingCannotProduce,
    NotBuilt,
}

/// Pay-and-enqueue helper. Pure where it can be — takes pool + queue and
/// validates everything, mutates only on success.
pub fn try_enqueue(
    pool: &mut ResourcePool,
    queue: &mut ProductionQueue,
    bt: &BuildingType,
    ut: UnitType,
    is_built: bool,
) -> Result<(), EnqueueError> {
    if !is_built {
        return Err(EnqueueError::NotBuilt);
    }
    if !building_produces(bt).contains(&ut) {
        return Err(EnqueueError::BuildingCannotProduce);
    }
    if queue.jobs.len() >= QUEUE_CAP {
        return Err(EnqueueError::QueueFull);
    }
    let cost = unit_production_cost(&ut);
    if !can_afford(pool, &cost) {
        return Err(EnqueueError::Unaffordable);
    }
    spend(pool, &cost);
    queue.jobs.push(ut);
    Ok(())
}

/// Advance one step of progress. Returns Some(unit) if a unit completed
/// this step (the caller should spawn it).
pub fn step_progress(queue: &mut ProductionQueue, dt: f32) -> Option<UnitType> {
    let head = queue.jobs.first()?.clone();
    let total = unit_production_seconds(&head);
    queue.progress += dt;
    if queue.progress >= total {
        queue.progress = 0.0;
        queue.jobs.remove(0);
        return Some(head);
    }
    None
}

// --- System ---

pub fn production_system(
    mut commands: Commands,
    time: Res<Time>,
    mut buildings: Query<(&BuildingPos, &Faction, &mut ProductionQueue), With<Built>>,
) {
    let dt = time.delta_secs();
    for (pos, faction, mut queue) in &mut buildings {
        if let Some(unit_type) = step_progress(&mut queue, dt) {
            let (x, y, f) = (pos.pos.x, pos.pos.y, faction.clone());
            let id = match unit_type {
                UnitType::Riflemen => commands.spawn(RiflemanBundle::with_faction(x, y, f)).id(),
                UnitType::HeavyWeapons => commands.spawn(HeavyWeaponsBundle::with_faction(x, y, f)).id(),
                UnitType::LightVehicle => commands.spawn(LightVehicleBundle::with_faction(x, y, f)).id(),
                UnitType::HeavyArmor => commands.spawn(HeavyArmorBundle::with_faction(x, y, f)).id(),
            };
            commands.entity(id).insert(HomeBase { pos: pos.pos.clone() });
        }
    }
}

pub struct ProductionPlugin;

impl Plugin for ProductionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, production_system);
    }
}

// --- Refund helper for tests ---

pub fn _refund_unused(pool: &mut ResourcePool, cost: &ResourceCost) {
    refund(pool, cost);
}

// suppress unused-import warnings for FactionEntity / GridPos (re-exported potentially)
const _: Option<FactionEntity> = None;
const _: Option<GridPos> = None;

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pool() -> ResourcePool {
        ResourcePool { fuel: 200.0, scrap: 200.0, manpower: 50.0 }
    }

    #[test]
    fn rifleman_cost_is_scrap_and_manpower() {
        let c = unit_production_cost(&UnitType::Riflemen);
        assert_eq!(c.fuel, 0.0);
        assert_eq!(c.scrap, 50.0);
        assert_eq!(c.manpower, 5.0);
    }

    #[test]
    fn barracks_produces_riflemen() {
        assert!(building_produces(&BuildingType::Barracks).contains(&UnitType::Riflemen));
    }

    #[test]
    fn refinery_produces_nothing() {
        assert!(building_produces(&BuildingType::Refinery).is_empty());
    }

    #[test]
    fn enqueue_rejects_unbuilt() {
        let mut p = make_pool();
        let mut q = ProductionQueue::default();
        let r = try_enqueue(&mut p, &mut q, &BuildingType::Barracks, UnitType::Riflemen, false);
        assert_eq!(r, Err(EnqueueError::NotBuilt));
    }

    #[test]
    fn enqueue_rejects_non_producer() {
        let mut p = make_pool();
        let mut q = ProductionQueue::default();
        let r = try_enqueue(&mut p, &mut q, &BuildingType::Refinery, UnitType::Riflemen, true);
        assert_eq!(r, Err(EnqueueError::BuildingCannotProduce));
    }

    #[test]
    fn enqueue_rejects_full_queue() {
        let mut p = ResourcePool { fuel: 1000.0, scrap: 1000.0, manpower: 1000.0 };
        let mut q = ProductionQueue::default();
        for _ in 0..QUEUE_CAP {
            assert!(try_enqueue(&mut p, &mut q, &BuildingType::Barracks, UnitType::Riflemen, true).is_ok());
        }
        let r = try_enqueue(&mut p, &mut q, &BuildingType::Barracks, UnitType::Riflemen, true);
        assert_eq!(r, Err(EnqueueError::QueueFull));
    }

    #[test]
    fn enqueue_rejects_unaffordable() {
        let mut p = ResourcePool { fuel: 0.0, scrap: 0.0, manpower: 0.0 };
        let mut q = ProductionQueue::default();
        let r = try_enqueue(&mut p, &mut q, &BuildingType::Barracks, UnitType::Riflemen, true);
        assert_eq!(r, Err(EnqueueError::Unaffordable));
    }

    #[test]
    fn enqueue_succeeds_and_deducts() {
        let mut p = make_pool();
        let mut q = ProductionQueue::default();
        try_enqueue(&mut p, &mut q, &BuildingType::Barracks, UnitType::Riflemen, true).unwrap();
        assert_eq!(p.scrap, 150.0);
        assert_eq!(p.manpower, 45.0);
        assert_eq!(q.jobs.len(), 1);
    }

    #[test]
    fn step_progress_completes_at_threshold() {
        let mut q = ProductionQueue { jobs: vec![UnitType::Riflemen], progress: 0.0 };
        let total = unit_production_seconds(&UnitType::Riflemen);
        // partial steps
        assert!(step_progress(&mut q, total / 2.0).is_none());
        assert_eq!(q.progress, total / 2.0);
        // complete
        let done = step_progress(&mut q, total / 2.0 + 0.01);
        assert_eq!(done, Some(UnitType::Riflemen));
        assert!(q.jobs.is_empty());
        assert_eq!(q.progress, 0.0);
    }

    #[test]
    fn step_progress_returns_none_on_empty_queue() {
        let mut q = ProductionQueue::default();
        assert!(step_progress(&mut q, 1.0).is_none());
    }

    #[test]
    fn step_progress_consumes_one_per_completion() {
        let mut q = ProductionQueue {
            jobs: vec![UnitType::Riflemen, UnitType::Riflemen],
            progress: 0.0,
        };
        let total = unit_production_seconds(&UnitType::Riflemen);
        let done = step_progress(&mut q, total + 0.01);
        assert_eq!(done, Some(UnitType::Riflemen));
        assert_eq!(q.jobs.len(), 1);
    }
}
