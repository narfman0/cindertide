// Buildings — placement, construction progress, costs per `mechanics.md`.

use bevy::prelude::*;
use crate::map::{GridPos, Faction};
use crate::resources::{ResourceCost, ResourcePool, spend, can_afford};
use std::collections::HashSet;

#[derive(Component, Debug, Clone, PartialEq, Eq, Hash)]
pub enum BuildingType {
    // Economy
    Refinery,
    Scrapyard,
    RecruitmentOffice,
    // Production
    Barracks,
    MotorPool,
    Foundry,
    Airfield,
    // Tech
    Workshop,
    CommandBunker,
    ResearchLab,
    // Support
    SupplyDepot,
    Watchtower,
    RepairBay,
    // Defense
    Pillbox,
    AAGun,
    TankTrap,
}

#[derive(Component, Debug, Clone)]
pub struct BuildingPos {
    pub pos: GridPos,
}

#[derive(Component, Debug, Clone)]
pub struct ConstructionProgress {
    pub elapsed: f32,
    pub total: f32,
}

/// Marker: building is being built. Removed (and Built inserted) when complete.
#[derive(Component, Debug)]
pub struct UnderConstruction;

/// Marker: building is fully constructed and operational.
#[derive(Component, Debug)]
pub struct Built;

// --- Cost / health / build-time tables ---

pub fn building_cost(bt: &BuildingType) -> ResourceCost {
    use BuildingType::*;
    let (f, s, m) = match bt {
        Refinery => (200.0, 50.0, 0.0),
        Scrapyard => (50.0, 200.0, 0.0),
        RecruitmentOffice => (50.0, 50.0, 0.0),
        Barracks => (100.0, 150.0, 0.0),
        MotorPool => (250.0, 200.0, 0.0),
        Foundry => (400.0, 350.0, 0.0),
        Airfield => (500.0, 400.0, 0.0),
        Workshop => (150.0, 100.0, 0.0),
        CommandBunker => (300.0, 200.0, 0.0),
        ResearchLab => (250.0, 200.0, 0.0),
        SupplyDepot => (100.0, 100.0, 0.0),
        Watchtower => (50.0, 75.0, 0.0),
        RepairBay => (100.0, 200.0, 0.0),
        Pillbox => (50.0, 100.0, 0.0),
        AAGun => (75.0, 150.0, 0.0),
        TankTrap => (0.0, 50.0, 0.0),
    };
    ResourceCost { fuel: f, scrap: s, manpower: m }
}

pub fn building_health(bt: &BuildingType) -> f32 {
    use BuildingType::*;
    match bt {
        TankTrap => 100.0,
        Pillbox | Watchtower | AAGun => 250.0,
        SupplyDepot | RecruitmentOffice | Scrapyard => 350.0,
        Refinery | Workshop | RepairBay => 500.0,
        Barracks | MotorPool | ResearchLab => 600.0,
        Foundry | Airfield => 800.0,
        CommandBunker => 1200.0,
    }
}

pub fn building_construction_seconds(bt: &BuildingType) -> f32 {
    use BuildingType::*;
    match bt {
        TankTrap => 5.0,
        Pillbox | Watchtower => 10.0,
        AAGun | RecruitmentOffice | SupplyDepot => 15.0,
        Refinery | Scrapyard | Workshop | RepairBay => 25.0,
        Barracks | MotorPool | ResearchLab => 40.0,
        Foundry | Airfield => 60.0,
        CommandBunker => 90.0,
    }
}

// --- Pure placement helpers ---

/// A position is placeable if it isn't already occupied by a building or
/// blocked by impassable terrain (e.g. void).
pub fn can_place(
    pos: &GridPos,
    occupied: &HashSet<GridPos>,
    blocked_terrain: &HashSet<GridPos>,
) -> bool {
    !occupied.contains(pos) && !blocked_terrain.contains(pos)
}

// --- Bundle ---

#[derive(Bundle)]
pub struct BuildingBundle {
    pub building_type: BuildingType,
    pub faction: Faction,
    pub pos: BuildingPos,
    pub health: crate::combat::Health,
    pub construction: ConstructionProgress,
    pub under_construction: UnderConstruction,
}

impl BuildingBundle {
    pub fn new(building_type: BuildingType, faction: Faction, x: i32, y: i32) -> Self {
        let total = building_construction_seconds(&building_type);
        let max_hp = building_health(&building_type);
        Self {
            building_type,
            faction,
            pos: BuildingPos { pos: GridPos { x, y } },
            health: crate::combat::Health { current: max_hp, max: max_hp },
            construction: ConstructionProgress { elapsed: 0.0, total },
            under_construction: UnderConstruction,
        }
    }
}

// --- Systems ---

pub fn construction_system(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut ConstructionProgress), With<UnderConstruction>>,
) {
    let dt = time.delta_secs();
    for (entity, mut progress) in &mut query {
        progress.elapsed += dt;
        if progress.elapsed >= progress.total {
            progress.elapsed = progress.total;
            commands.entity(entity).remove::<UnderConstruction>().insert(Built);
        }
    }
}

pub struct BuildingsPlugin;

impl Plugin for BuildingsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, construction_system);
    }
}

// --- Try-place helper for the BRP handler ---

#[derive(Debug, PartialEq)]
pub enum PlaceError {
    Occupied,
    Unaffordable,
}

/// Attempt to spend resources for a building. Returns Ok(()) if the spend
/// succeeded. Caller is responsible for the actual spawn — this keeps the
/// pure-function path testable without an ECS world.
pub fn try_pay(pool: &mut ResourcePool, cost: &ResourceCost) -> Result<(), PlaceError> {
    if !can_afford(pool, cost) {
        return Err(PlaceError::Unaffordable);
    }
    spend(pool, cost);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refinery_cost() {
        let c = building_cost(&BuildingType::Refinery);
        assert_eq!(c.fuel, 200.0);
        assert_eq!(c.scrap, 50.0);
    }

    #[test]
    fn command_bunker_is_most_expensive() {
        let cb = building_cost(&BuildingType::CommandBunker);
        let tank_trap = building_cost(&BuildingType::TankTrap);
        assert!(cb.fuel + cb.scrap > tank_trap.fuel + tank_trap.scrap);
    }

    #[test]
    fn command_bunker_has_highest_health() {
        let cb = building_health(&BuildingType::CommandBunker);
        let pillbox = building_health(&BuildingType::Pillbox);
        assert!(cb > pillbox);
    }

    #[test]
    fn tank_trap_builds_fast() {
        assert!(building_construction_seconds(&BuildingType::TankTrap)
            < building_construction_seconds(&BuildingType::Refinery));
    }

    #[test]
    fn can_place_on_empty_unblocked_tile() {
        let occupied: HashSet<GridPos> = HashSet::new();
        let blocked: HashSet<GridPos> = HashSet::new();
        assert!(can_place(&GridPos { x: 0, y: 0 }, &occupied, &blocked));
    }

    #[test]
    fn cannot_place_on_occupied_tile() {
        let mut occupied: HashSet<GridPos> = HashSet::new();
        occupied.insert(GridPos { x: 1, y: 1 });
        let blocked: HashSet<GridPos> = HashSet::new();
        assert!(!can_place(&GridPos { x: 1, y: 1 }, &occupied, &blocked));
    }

    #[test]
    fn cannot_place_on_blocked_terrain() {
        let occupied: HashSet<GridPos> = HashSet::new();
        let mut blocked: HashSet<GridPos> = HashSet::new();
        blocked.insert(GridPos { x: 2, y: 2 });
        assert!(!can_place(&GridPos { x: 2, y: 2 }, &occupied, &blocked));
    }

    #[test]
    fn try_pay_succeeds_when_affordable() {
        let mut pool = ResourcePool { fuel: 500.0, scrap: 500.0, manpower: 50.0 };
        let cost = building_cost(&BuildingType::Refinery);
        assert!(try_pay(&mut pool, &cost).is_ok());
        assert_eq!(pool.fuel, 300.0);
        assert_eq!(pool.scrap, 450.0);
    }

    #[test]
    fn try_pay_rejects_when_unaffordable() {
        let mut pool = ResourcePool { fuel: 10.0, scrap: 10.0, manpower: 10.0 };
        let cost = building_cost(&BuildingType::Refinery);
        assert_eq!(try_pay(&mut pool, &cost), Err(PlaceError::Unaffordable));
        assert_eq!(pool.fuel, 10.0);
    }

    #[test]
    fn bundle_starts_with_full_health_and_zero_progress() {
        let b = BuildingBundle::new(BuildingType::Refinery, Faction::Combine, 5, 5);
        assert_eq!(b.health.current, b.health.max);
        assert_eq!(b.construction.elapsed, 0.0);
        assert_eq!(b.construction.total, building_construction_seconds(&BuildingType::Refinery));
    }
}
