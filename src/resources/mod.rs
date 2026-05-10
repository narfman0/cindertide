// Resources module — Fuel, Scrap, Manpower per `mechanics.md`.

use bevy::prelude::*;
use crate::map::Faction;

#[derive(Component, Debug, Clone)]
pub struct ResourcePool {
    pub fuel: f32,
    pub scrap: f32,
    pub manpower: f32,
}

#[derive(Component, Debug, Clone)]
pub struct ResourceCaps {
    pub fuel: f32,
    pub scrap: f32,
    pub manpower: f32,
}

#[derive(Component, Debug, Clone)]
pub struct ResourceTrickle {
    pub fuel_per_second: f32,
    pub scrap_per_second: f32,
    pub manpower_per_second: f32,
}

/// Per-faction population cap. Heroes are not counted toward `current`.
/// `max` is recomputed from a base + each Built Supply Depot.
#[derive(Component, Debug, Clone)]
pub struct PopCap {
    pub current: u32,
    pub max: u32,
}

pub const BASE_POP_CAP: u32 = 20;
pub const POP_PER_SUPPLY_DEPOT: u32 = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceCost {
    pub fuel: f32,
    pub scrap: f32,
    pub manpower: f32,
}

impl ResourceCost {
    pub fn zero() -> Self {
        Self { fuel: 0.0, scrap: 0.0, manpower: 0.0 }
    }
}

/// Marker on a Faction entity. Pairs with ResourcePool/Caps/Trickle.
#[derive(Component, Debug, Clone)]
pub struct FactionEntity {
    pub faction: Faction,
}

#[derive(Bundle)]
pub struct FactionBundle {
    pub faction: FactionEntity,
    pub pool: ResourcePool,
    pub caps: ResourceCaps,
    pub trickle: ResourceTrickle,
    pub tech: crate::tech::Tech,
    pub pop_cap: PopCap,
    pub tech_buff: crate::tech::TechBuff,
}

impl FactionBundle {
    pub fn new(faction: Faction) -> Self {
        let (fuel, scrap, manpower) = match faction.id() {
            "combine"  => (800.0, 400.0, 100.0),
            "ironborn" => (400.0, 900.0, 120.0),
            "covenant" => (600.0, 600.0, 80.0),
            "hollow"   => (800.0, 400.0, 100.0),
            _          => (500.0, 500.0, 100.0),
        };
        Self {
            faction: FactionEntity { faction },
            pool: ResourcePool { fuel, scrap, manpower },
            caps: ResourceCaps { fuel: 2000.0, scrap: 2000.0, manpower: 200.0 },
            trickle: ResourceTrickle { fuel_per_second: 0.0, scrap_per_second: 0.0, manpower_per_second: 1.0 },
            tech: crate::tech::Tech::default(),
            pop_cap: PopCap { current: 0, max: BASE_POP_CAP },
            tech_buff: crate::tech::TechBuff::default(),
        }
    }
}

// --- Pure functions ---

pub fn can_afford(pool: &ResourcePool, cost: &ResourceCost) -> bool {
    pool.fuel >= cost.fuel
        && pool.scrap >= cost.scrap
        && pool.manpower >= cost.manpower
}

/// Returns true if the cost was affordable and was deducted.
pub fn spend(pool: &mut ResourcePool, cost: &ResourceCost) -> bool {
    if !can_afford(pool, cost) {
        return false;
    }
    pool.fuel -= cost.fuel;
    pool.scrap -= cost.scrap;
    pool.manpower -= cost.manpower;
    true
}

pub fn refund(pool: &mut ResourcePool, cost: &ResourceCost) {
    pool.fuel += cost.fuel;
    pool.scrap += cost.scrap;
    pool.manpower += cost.manpower;
}

pub fn apply_trickle(pool: &mut ResourcePool, trickle: &ResourceTrickle, caps: &ResourceCaps, dt: f32) {
    pool.fuel = (pool.fuel + trickle.fuel_per_second * dt).min(caps.fuel);
    pool.scrap = (pool.scrap + trickle.scrap_per_second * dt).min(caps.scrap);
    pool.manpower = (pool.manpower + trickle.manpower_per_second * dt).min(caps.manpower);
}

// --- Systems ---

pub fn resource_trickle_system(
    time: Res<Time>,
    mut query: Query<(&mut ResourcePool, &ResourceTrickle, &ResourceCaps)>,
) {
    let dt = time.delta_secs();
    for (mut pool, trickle, caps) in &mut query {
        apply_trickle(&mut pool, trickle, caps, dt);
    }
}

/// Recomputes per-faction PopCap each frame from current units (non-hero)
/// and Built Supply Depots.
pub fn pop_cap_system(
    mut factions: Query<(&FactionEntity, &mut PopCap)>,
    units: Query<&Faction, (With<crate::units::UnitTypeId>, Without<crate::heroes::Hero>)>,
    depots: Query<(&Faction, &crate::buildings::BuildingTypeId), With<crate::buildings::Built>>,
) {
    use std::collections::HashMap;

    let mut counts: HashMap<Faction, u32> = HashMap::new();
    for f in &units {
        *counts.entry(f.clone()).or_insert(0) += 1;
    }

    let mut depot_counts: HashMap<Faction, u32> = HashMap::new();
    for (f, bt) in &depots {
        if bt.id() == "supply_depot" {
            *depot_counts.entry(f.clone()).or_insert(0) += 1;
        }
    }

    for (fe, mut cap) in &mut factions {
        cap.current = counts.get(&fe.faction).copied().unwrap_or(0);
        let depots = depot_counts.get(&fe.faction).copied().unwrap_or(0);
        cap.max = BASE_POP_CAP + depots * POP_PER_SUPPLY_DEPOT;
    }
}

/// Recomputes `ResourceTrickle` each frame from the faction's currently built buildings.
/// Also applies the faction's `TechBuff.trickle_mult`.
pub fn building_trickle_system(
    loaded: Res<crate::factions::LoadedFactions>,
    buildings: Query<(&crate::buildings::BuildingTypeId, &Faction), With<crate::buildings::Built>>,
    mut factions: Query<(&FactionEntity, &mut ResourceTrickle, Option<&crate::tech::TechBuff>)>,
) {
    for (fe, mut trickle, tech_buff) in &mut factions {
        trickle.fuel_per_second = 0.0;
        trickle.scrap_per_second = 0.0;
        trickle.manpower_per_second = 1.0; // base manpower trickle always 1/s
        for (bt, bf) in &buildings {
            if bf != &fe.faction { continue; }
            if let Some(def) = loaded.faction_building(fe.faction.id(), bt.id()) {
                trickle.fuel_per_second += def.trickle_fuel;
                trickle.scrap_per_second += def.trickle_scrap;
                trickle.manpower_per_second += def.trickle_manpower;
            }
        }
        // Apply tech buff multiplier
        let mult = tech_buff.map(|b| b.trickle_mult).unwrap_or(1.0);
        trickle.fuel_per_second *= mult;
        trickle.scrap_per_second *= mult;
        trickle.manpower_per_second *= mult;
    }
}

pub struct ResourcesPlugin;

impl Plugin for ResourcesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (resource_trickle_system, building_trickle_system, pop_cap_system));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(f: f32, s: f32, m: f32) -> ResourcePool {
        ResourcePool { fuel: f, scrap: s, manpower: m }
    }

    fn cost(f: f32, s: f32, m: f32) -> ResourceCost {
        ResourceCost { fuel: f, scrap: s, manpower: m }
    }

    #[test]
    fn can_afford_when_pool_meets_or_exceeds() {
        assert!(can_afford(&pool(100.0, 100.0, 10.0), &cost(50.0, 50.0, 5.0)));
        assert!(can_afford(&pool(50.0, 50.0, 5.0), &cost(50.0, 50.0, 5.0)));
    }

    #[test]
    fn cannot_afford_when_any_resource_short() {
        assert!(!can_afford(&pool(10.0, 100.0, 10.0), &cost(50.0, 50.0, 5.0)));
        assert!(!can_afford(&pool(100.0, 10.0, 10.0), &cost(50.0, 50.0, 5.0)));
        assert!(!can_afford(&pool(100.0, 100.0, 1.0), &cost(50.0, 50.0, 5.0)));
    }

    #[test]
    fn spend_deducts_when_affordable() {
        let mut p = pool(100.0, 100.0, 10.0);
        assert!(spend(&mut p, &cost(30.0, 20.0, 5.0)));
        assert_eq!(p.fuel, 70.0);
        assert_eq!(p.scrap, 80.0);
        assert_eq!(p.manpower, 5.0);
    }

    #[test]
    fn spend_rejects_when_unaffordable() {
        let mut p = pool(100.0, 100.0, 10.0);
        assert!(!spend(&mut p, &cost(30.0, 20.0, 50.0)));
        assert_eq!(p.fuel, 100.0);
        assert_eq!(p.scrap, 100.0);
        assert_eq!(p.manpower, 10.0);
    }

    #[test]
    fn refund_adds_back() {
        let mut p = pool(50.0, 50.0, 5.0);
        refund(&mut p, &cost(30.0, 20.0, 5.0));
        assert_eq!(p.fuel, 80.0);
        assert_eq!(p.scrap, 70.0);
        assert_eq!(p.manpower, 10.0);
    }

    #[test]
    fn trickle_accumulates_over_time() {
        let mut p = pool(0.0, 0.0, 0.0);
        let t = ResourceTrickle { fuel_per_second: 10.0, scrap_per_second: 5.0, manpower_per_second: 1.0 };
        let c = ResourceCaps { fuel: 1000.0, scrap: 1000.0, manpower: 100.0 };
        apply_trickle(&mut p, &t, &c, 2.0);
        assert_eq!(p.fuel, 20.0);
        assert_eq!(p.scrap, 10.0);
        assert_eq!(p.manpower, 2.0);
    }

    #[test]
    fn trickle_clamps_to_caps() {
        let mut p = pool(995.0, 0.0, 99.0);
        let t = ResourceTrickle { fuel_per_second: 100.0, scrap_per_second: 0.0, manpower_per_second: 100.0 };
        let c = ResourceCaps { fuel: 1000.0, scrap: 1000.0, manpower: 100.0 };
        apply_trickle(&mut p, &t, &c, 1.0);
        assert_eq!(p.fuel, 1000.0);
        assert_eq!(p.manpower, 100.0);
    }

    #[test]
    fn cost_zero_is_zero() {
        let z = ResourceCost::zero();
        assert_eq!(z.fuel, 0.0);
        assert_eq!(z.scrap, 0.0);
        assert_eq!(z.manpower, 0.0);
    }

    #[test]
    fn faction_bundle_default_starting_pool() {
        let b = FactionBundle::new(Faction::combine());
        assert_eq!(b.pool.fuel, 800.0);
        assert_eq!(b.pool.scrap, 400.0);
        assert_eq!(b.pool.manpower, 100.0);
        assert_eq!(b.faction.faction, Faction::combine());
    }

    #[test]
    fn faction_bundle_pop_cap_starts_at_base() {
        let b = FactionBundle::new(Faction::combine());
        assert_eq!(b.pop_cap.current, 0);
        assert_eq!(b.pop_cap.max, BASE_POP_CAP);
    }
}
