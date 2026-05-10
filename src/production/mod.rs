// Production — per-building unit queues with cost-up-front and progress.

use bevy::prelude::*;
use crate::map::{Faction, GridPos};
use crate::units::{UnitBundle, HomeBase};
use crate::buildings::{BuildingPos, BuildingTypeId, Built};
use crate::resources::{ResourcePool, ResourceCost, FactionEntity, can_afford, spend, refund};
use crate::factions::LoadedFactions;

pub const QUEUE_CAP: usize = 5;

#[derive(Component, Debug, Clone, Default)]
pub struct ProductionQueue {
    pub jobs: Vec<String>,
    pub progress: f32,
}

// --- Data-driven tables ---

pub fn unit_production_seconds(unit_id: &str, loaded: &LoadedFactions) -> f32 {
    loaded.units.get(unit_id).map(|d| d.build_time_seconds).unwrap_or(10.0)
}

pub fn unit_production_cost(unit_id: &str, loaded: &LoadedFactions) -> ResourceCost {
    loaded.units.get(unit_id)
        .map(|d| ResourceCost {
            fuel: d.production_cost.fuel,
            scrap: d.production_cost.scrap,
            manpower: d.production_cost.manpower,
        })
        .unwrap_or(ResourceCost { fuel: 50.0, scrap: 50.0, manpower: 5.0 })
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
    bt: &BuildingTypeId,
    unit_id: String,
    is_built: bool,
    loaded: &LoadedFactions,
) -> Result<(), EnqueueError> {
    if !is_built {
        return Err(EnqueueError::NotBuilt);
    }
    let produces = crate::buildings::building_produces(bt, loaded);
    if !produces.contains(&unit_id.as_str()) {
        return Err(EnqueueError::BuildingCannotProduce);
    }
    if queue.jobs.len() >= QUEUE_CAP {
        return Err(EnqueueError::QueueFull);
    }
    let cost = unit_production_cost(&unit_id, loaded);
    if !can_afford(pool, &cost) {
        return Err(EnqueueError::Unaffordable);
    }
    spend(pool, &cost);
    queue.jobs.push(unit_id);
    Ok(())
}

/// Advance one step of progress. Returns Some(unit_id) if a unit completed
/// this step (the caller should spawn it).
pub fn step_progress(queue: &mut ProductionQueue, dt: f32, loaded: &LoadedFactions) -> Option<String> {
    let head = queue.jobs.first()?.clone();
    let total = unit_production_seconds(&head, loaded);
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
    loaded: Res<LoadedFactions>,
    mut buildings: Query<(&BuildingPos, &Faction, &mut ProductionQueue), With<Built>>,
    tech_buffs: Query<(&FactionEntity, &crate::tech::TechBuff)>,
) {
    let dt = time.delta_secs();
    for (pos, faction, mut queue) in &mut buildings {
        if let Some(unit_id) = step_progress(&mut queue, dt, &loaded) {
            let (x, y, f) = (pos.pos.x, pos.pos.y, faction.clone());
            if let Some(def) = loaded.units.get(&unit_id) {
                let buff = tech_buffs.iter()
                    .find(|(fe, _)| fe.faction == f)
                    .map(|(_, b)| b.clone())
                    .unwrap_or_default();
                let mut bundle = UnitBundle::from_def(def, f, x, y);
                bundle.health.current *= buff.hp_mult;
                bundle.health.max *= buff.hp_mult;
                bundle.attack_damage.base *= buff.damage_mult;
                let id = commands.spawn(bundle).id();
                commands.entity(id).insert(HomeBase { pos: pos.pos.clone() });
            }
        }
    }
}

pub struct ProductionPlugin;

impl Plugin for ProductionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, production_system);
    }
}

// suppress unused-import warnings
const _: Option<FactionEntity> = None;
const _: Option<GridPos> = None;

#[cfg(test)]
mod tests {
    use super::*;

    fn make_loaded() -> LoadedFactions {
        let mut loaded = LoadedFactions::default();
        loaded.units.insert("riflemen".to_string(), crate::factions::UnitDef {
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
            production_cost: crate::factions::ResourceCostDef { fuel: 0.0, scrap: 50.0, manpower: 5.0 },
            produced_at: vec!["barracks".to_string()],
            build_time_seconds: 8.0,
            ability_q: String::new(),
            model_file: String::new(),
            description: String::new(),
        });
        loaded.buildings.insert("barracks".to_string(), crate::factions::BuildingDef {
            id: "barracks".to_string(),
            display_name: "Barracks".to_string(),
            health: 600.0,
            cost: crate::factions::ResourceCostDef { fuel: 100.0, scrap: 150.0, manpower: 0.0 },
            build_time_seconds: 40.0,
            produces: vec!["riflemen".to_string()],
            model_file: String::new(),
            description: String::new(),
            trickle_fuel: 0.0, trickle_scrap: 0.0, trickle_manpower: 0.0, vision_radius: 0.0,
        });
        loaded.buildings.insert("refinery".to_string(), crate::factions::BuildingDef {
            id: "refinery".to_string(),
            display_name: "Refinery".to_string(),
            health: 500.0,
            cost: crate::factions::ResourceCostDef { fuel: 200.0, scrap: 50.0, manpower: 0.0 },
            build_time_seconds: 25.0,
            produces: vec![],
            model_file: String::new(),
            description: String::new(),
            trickle_fuel: 0.0, trickle_scrap: 0.0, trickle_manpower: 0.0, vision_radius: 0.0,
        });
        loaded
    }

    fn make_pool() -> ResourcePool {
        ResourcePool { fuel: 200.0, scrap: 200.0, manpower: 50.0 }
    }

    #[test]
    fn rifleman_cost_is_scrap_and_manpower() {
        let loaded = make_loaded();
        let c = unit_production_cost("riflemen", &loaded);
        assert_eq!(c.fuel, 0.0);
        assert_eq!(c.scrap, 50.0);
        assert_eq!(c.manpower, 5.0);
    }

    #[test]
    fn barracks_produces_riflemen() {
        let loaded = make_loaded();
        let produces = crate::buildings::building_produces(&BuildingTypeId::new("barracks"), &loaded);
        assert!(produces.contains(&"riflemen"));
    }

    #[test]
    fn refinery_produces_nothing() {
        let loaded = make_loaded();
        let produces = crate::buildings::building_produces(&BuildingTypeId::new("refinery"), &loaded);
        assert!(produces.is_empty());
    }

    #[test]
    fn enqueue_rejects_unbuilt() {
        let loaded = make_loaded();
        let mut p = make_pool();
        let mut q = ProductionQueue::default();
        let r = try_enqueue(&mut p, &mut q, &BuildingTypeId::new("barracks"), "riflemen".to_string(), false, &loaded);
        assert_eq!(r, Err(EnqueueError::NotBuilt));
    }

    #[test]
    fn enqueue_rejects_non_producer() {
        let loaded = make_loaded();
        let mut p = make_pool();
        let mut q = ProductionQueue::default();
        let r = try_enqueue(&mut p, &mut q, &BuildingTypeId::new("refinery"), "riflemen".to_string(), true, &loaded);
        assert_eq!(r, Err(EnqueueError::BuildingCannotProduce));
    }

    #[test]
    fn enqueue_rejects_full_queue() {
        let loaded = make_loaded();
        let mut p = ResourcePool { fuel: 1000.0, scrap: 1000.0, manpower: 1000.0 };
        let mut q = ProductionQueue::default();
        for _ in 0..QUEUE_CAP {
            assert!(try_enqueue(&mut p, &mut q, &BuildingTypeId::new("barracks"), "riflemen".to_string(), true, &loaded).is_ok());
        }
        let r = try_enqueue(&mut p, &mut q, &BuildingTypeId::new("barracks"), "riflemen".to_string(), true, &loaded);
        assert_eq!(r, Err(EnqueueError::QueueFull));
    }

    #[test]
    fn enqueue_rejects_unaffordable() {
        let loaded = make_loaded();
        let mut p = ResourcePool { fuel: 0.0, scrap: 0.0, manpower: 0.0 };
        let mut q = ProductionQueue::default();
        let r = try_enqueue(&mut p, &mut q, &BuildingTypeId::new("barracks"), "riflemen".to_string(), true, &loaded);
        assert_eq!(r, Err(EnqueueError::Unaffordable));
    }

    #[test]
    fn enqueue_succeeds_and_deducts() {
        let loaded = make_loaded();
        let mut p = make_pool();
        let mut q = ProductionQueue::default();
        try_enqueue(&mut p, &mut q, &BuildingTypeId::new("barracks"), "riflemen".to_string(), true, &loaded).unwrap();
        assert_eq!(p.scrap, 150.0);
        assert_eq!(p.manpower, 45.0);
        assert_eq!(q.jobs.len(), 1);
    }

    #[test]
    fn step_progress_completes_at_threshold() {
        let loaded = make_loaded();
        let mut q = ProductionQueue { jobs: vec!["riflemen".to_string()], progress: 0.0 };
        let total = unit_production_seconds("riflemen", &loaded);
        // partial steps
        assert!(step_progress(&mut q, total / 2.0, &loaded).is_none());
        assert_eq!(q.progress, total / 2.0);
        // complete
        let done = step_progress(&mut q, total / 2.0 + 0.01, &loaded);
        assert_eq!(done, Some("riflemen".to_string()));
        assert!(q.jobs.is_empty());
        assert_eq!(q.progress, 0.0);
    }

    #[test]
    fn step_progress_returns_none_on_empty_queue() {
        let loaded = make_loaded();
        let mut q = ProductionQueue::default();
        assert!(step_progress(&mut q, 1.0, &loaded).is_none());
    }

    #[test]
    fn step_progress_consumes_one_per_completion() {
        let loaded = make_loaded();
        let mut q = ProductionQueue {
            jobs: vec!["riflemen".to_string(), "riflemen".to_string()],
            progress: 0.0,
        };
        let total = unit_production_seconds("riflemen", &loaded);
        let done = step_progress(&mut q, total + 0.01, &loaded);
        assert_eq!(done, Some("riflemen".to_string()));
        assert_eq!(q.jobs.len(), 1);
    }
}
