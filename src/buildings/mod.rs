// Buildings — placement, construction progress, costs per `mechanics.md`.

use bevy::prelude::*;
use crate::map::{GridPos, Faction, NavMesh};
use crate::resources::{ResourceCost, ResourcePool, spend, can_afford};
use crate::factions::LoadedFactions;
use std::collections::HashSet;

#[derive(Component, Debug, Clone, PartialEq, Eq, Hash)]
pub struct BuildingTypeId(pub String);

impl BuildingTypeId {
    pub fn new(id: &str) -> Self { BuildingTypeId(id.to_string()) }
    pub fn id(&self) -> &str { &self.0 }
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

/// Buildings with this component contribute to their faction's fog of war.
#[derive(Component, Debug, Clone)]
pub struct VisionProvider {
    pub radius: f32,
}

// --- Cost / health / build-time functions (data-driven via LoadedFactions) ---

pub fn building_cost(bt: &BuildingTypeId, loaded: &LoadedFactions) -> ResourceCost {
    loaded.buildings.get(bt.id())
        .map(|def| ResourceCost { fuel: def.cost.fuel, scrap: def.cost.scrap, manpower: def.cost.manpower })
        .unwrap_or(ResourceCost { fuel: 0.0, scrap: 100.0, manpower: 0.0 })
}

pub fn building_health(bt: &BuildingTypeId, loaded: &LoadedFactions) -> f32 {
    loaded.buildings.get(bt.id()).map(|d| d.health).unwrap_or(500.0)
}

pub fn building_construction_seconds(bt: &BuildingTypeId, loaded: &LoadedFactions) -> f32 {
    loaded.buildings.get(bt.id()).map(|d| d.build_time_seconds).unwrap_or(30.0)
}

/// Which unit IDs does this building produce?
pub fn building_produces<'a>(bt: &BuildingTypeId, loaded: &'a LoadedFactions) -> Vec<&'a str> {
    loaded.buildings.get(bt.id())
        .map(|d| d.produces.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default()
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
    pub building_type: BuildingTypeId,
    pub faction: Faction,
    pub pos: BuildingPos,
    pub health: crate::combat::Health,
    pub construction: ConstructionProgress,
    pub under_construction: UnderConstruction,
}

impl BuildingBundle {
    pub fn new(building_type: BuildingTypeId, faction: Faction, x: i32, y: i32, loaded: &LoadedFactions) -> Self {
        let total = building_construction_seconds(&building_type, loaded);
        let max_hp = building_health(&building_type, loaded);
        Self {
            building_type,
            faction,
            pos: BuildingPos { pos: GridPos { x, y } },
            health: crate::combat::Health { current: max_hp, max: max_hp },
            construction: ConstructionProgress { elapsed: 0.0, total },
            under_construction: UnderConstruction,
        }
    }

    /// Convenience constructor using default stats (30s build, 500hp).
    /// Use when LoadedFactions is not accessible (e.g. from Commands).
    pub fn new_default(building_type: BuildingTypeId, faction: Faction, x: i32, y: i32) -> Self {
        Self {
            building_type,
            faction,
            pos: BuildingPos { pos: GridPos { x, y } },
            health: crate::combat::Health { current: 500.0, max: 500.0 },
            construction: ConstructionProgress { elapsed: 0.0, total: 30.0 },
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

/// When a building finishes construction (Built marker added), mark its tile
/// as impassable in the NavMesh.
pub fn update_navmesh_on_building_placed(
    mut nav: Option<ResMut<NavMesh>>,
    query: Query<&BuildingPos, Added<Built>>,
) {
    let Some(ref mut nav) = nav else { return };
    for bp in &query {
        nav.set_impassable(bp.pos.x, bp.pos.y);
    }
}

/// When a building finishes construction, insert `VisionProvider` if the building
/// definition has a non-zero vision_radius.
pub fn apply_vision_provider_system(
    mut commands: Commands,
    loaded: Res<LoadedFactions>,
    query: Query<(Entity, &BuildingTypeId, &crate::map::Faction), (Added<Built>, Without<VisionProvider>)>,
) {
    for (entity, bt, faction) in &query {
        if let Some(def) = loaded.faction_building(faction.id(), bt.id()) {
            if def.vision_radius > 0.0 {
                commands.entity(entity).insert(VisionProvider { radius: def.vision_radius });
            }
        }
    }
}

pub struct BuildingsPlugin;

impl Plugin for BuildingsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, construction_system);
        app.add_systems(Update, update_navmesh_on_building_placed);
        app.add_systems(Update, apply_vision_provider_system);
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

    fn make_loaded() -> LoadedFactions {
        let mut loaded = LoadedFactions::default();
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
        loaded.buildings.insert("command_bunker".to_string(), crate::factions::BuildingDef {
            id: "command_bunker".to_string(),
            display_name: "Command Bunker".to_string(),
            health: 1200.0,
            cost: crate::factions::ResourceCostDef { fuel: 300.0, scrap: 200.0, manpower: 0.0 },
            build_time_seconds: 90.0,
            produces: vec![],
            model_file: String::new(),
            description: String::new(),
            trickle_fuel: 0.0, trickle_scrap: 0.0, trickle_manpower: 0.0, vision_radius: 0.0,
        });
        loaded.buildings.insert("pillbox".to_string(), crate::factions::BuildingDef {
            id: "pillbox".to_string(),
            display_name: "Pillbox".to_string(),
            health: 250.0,
            cost: crate::factions::ResourceCostDef { fuel: 50.0, scrap: 100.0, manpower: 0.0 },
            build_time_seconds: 10.0,
            produces: vec![],
            model_file: String::new(),
            description: String::new(),
            trickle_fuel: 0.0, trickle_scrap: 0.0, trickle_manpower: 0.0, vision_radius: 0.0,
        });
        loaded.buildings.insert("tank_trap".to_string(), crate::factions::BuildingDef {
            id: "tank_trap".to_string(),
            display_name: "Tank Trap".to_string(),
            health: 100.0,
            cost: crate::factions::ResourceCostDef { fuel: 0.0, scrap: 50.0, manpower: 0.0 },
            build_time_seconds: 5.0,
            produces: vec![],
            model_file: String::new(),
            description: String::new(),
            trickle_fuel: 0.0, trickle_scrap: 0.0, trickle_manpower: 0.0, vision_radius: 0.0,
        });
        loaded
    }

    #[test]
    fn refinery_cost() {
        let loaded = make_loaded();
        let c = building_cost(&BuildingTypeId::new("refinery"), &loaded);
        assert_eq!(c.fuel, 200.0);
        assert_eq!(c.scrap, 50.0);
    }

    #[test]
    fn command_bunker_is_most_expensive() {
        let loaded = make_loaded();
        let cb = building_cost(&BuildingTypeId::new("command_bunker"), &loaded);
        let tank_trap = building_cost(&BuildingTypeId::new("tank_trap"), &loaded);
        assert!(cb.fuel + cb.scrap > tank_trap.fuel + tank_trap.scrap);
    }

    #[test]
    fn command_bunker_has_highest_health() {
        let loaded = make_loaded();
        let cb = building_health(&BuildingTypeId::new("command_bunker"), &loaded);
        let pillbox = building_health(&BuildingTypeId::new("pillbox"), &loaded);
        assert!(cb > pillbox);
    }

    #[test]
    fn tank_trap_builds_fast() {
        let loaded = make_loaded();
        assert!(
            building_construction_seconds(&BuildingTypeId::new("tank_trap"), &loaded)
                < building_construction_seconds(&BuildingTypeId::new("refinery"), &loaded)
        );
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
        let loaded = make_loaded();
        let mut pool = ResourcePool { fuel: 500.0, scrap: 500.0, manpower: 50.0 };
        let cost = building_cost(&BuildingTypeId::new("refinery"), &loaded);
        assert!(try_pay(&mut pool, &cost).is_ok());
        assert_eq!(pool.fuel, 300.0);
        assert_eq!(pool.scrap, 450.0);
    }

    #[test]
    fn try_pay_rejects_when_unaffordable() {
        let loaded = make_loaded();
        let mut pool = ResourcePool { fuel: 10.0, scrap: 10.0, manpower: 10.0 };
        let cost = building_cost(&BuildingTypeId::new("refinery"), &loaded);
        assert_eq!(try_pay(&mut pool, &cost), Err(PlaceError::Unaffordable));
        assert_eq!(pool.fuel, 10.0);
    }

    #[test]
    fn bundle_starts_with_full_health_and_zero_progress() {
        let loaded = make_loaded();
        let b = BuildingBundle::new(BuildingTypeId::new("refinery"), Faction::combine(), 5, 5, &loaded);
        assert_eq!(b.health.current, b.health.max);
        assert_eq!(b.construction.elapsed, 0.0);
        assert_eq!(b.construction.total, building_construction_seconds(&BuildingTypeId::new("refinery"), &loaded));
    }
}
