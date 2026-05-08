// AI opponent — economic, production, tactical, hero, and doctrine systems.

use bevy::prelude::*;
use crate::map::{Faction, GridPos};
use crate::resources::{ResourcePool, FactionEntity, can_afford, spend, PopCap};
use crate::buildings::{BuildingPos, BuildingType, BuildingBundle, building_cost, can_place, Built};
use crate::production::{ProductionQueue, building_produces, unit_production_cost, QUEUE_CAP};
use crate::units::{UnitPos, UnitType};
use crate::combat::AttackTarget;
use crate::heroes::{Hero, SignatureAbility, HeroDowned, is_charge_full};
use crate::tech::{Tech, Doctrine, ResearchTarget, ResearchInProgress, start_research, Tier};

pub const AI_TICK_SECONDS: f32 = 3.0;
pub const AI_BUILD_OFFSET_RADIUS: i32 = 3;

/// Marker — this faction is AI-controlled.
#[derive(Component, Debug, Clone)]
pub struct AiController {
    pub home: GridPos,
    pub doctrine: Doctrine,
    pub last_tick: f32,
}

impl AiController {
    pub fn new(home_x: i32, home_y: i32, doctrine: Doctrine) -> Self {
        Self {
            home: GridPos { x: home_x, y: home_y },
            doctrine,
            last_tick: 0.0,
        }
    }
}

/// Helper to find an empty tile near home for a new building.
fn find_empty_near(home: &GridPos, occupied: &std::collections::HashSet<GridPos>) -> Option<GridPos> {
    for r in 1..=AI_BUILD_OFFSET_RADIUS {
        for dx in -r..=r {
            for dy in -r..=r {
                let p = GridPos { x: home.x + dx, y: home.y + dy };
                if !occupied.contains(&p) {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Pure decision: which economy building to build next given current state.
/// Priority: Refinery → Scrapyard → Barracks → SupplyDepot → MotorPool.
pub fn next_economy_target(
    have_refinery: bool,
    have_scrapyard: bool,
    have_barracks: bool,
    have_supply_depot: bool,
    have_motor_pool: bool,
) -> Option<BuildingType> {
    if !have_refinery { return Some(BuildingType::Refinery); }
    if !have_scrapyard { return Some(BuildingType::Scrapyard); }
    if !have_barracks { return Some(BuildingType::Barracks); }
    if !have_supply_depot { return Some(BuildingType::SupplyDepot); }
    if !have_motor_pool { return Some(BuildingType::MotorPool); }
    None
}

// --- Economic AI ---

pub fn economic_ai_system(
    mut commands: Commands,
    time: Res<Time>,
    mut ai: Query<(Entity, &mut AiController, &FactionEntity, &mut ResourcePool)>,
    buildings: Query<(&BuildingPos, &BuildingType, &Faction)>,
) {
    let now = time.elapsed_secs();
    for (faction_entity, mut a, fe, mut pool) in &mut ai {
        if now - a.last_tick < AI_TICK_SECONDS {
            continue;
        }

        // Inventory of buildings owned by this faction.
        let mut have = std::collections::HashMap::<BuildingType, u32>::new();
        let mut occupied = std::collections::HashSet::<GridPos>::new();
        for (bp, bt, f) in &buildings {
            occupied.insert(bp.pos.clone());
            if f == &fe.faction {
                *have.entry(bt.clone()).or_insert(0) += 1;
            }
        }

        let target = next_economy_target(
            have.contains_key(&BuildingType::Refinery),
            have.contains_key(&BuildingType::Scrapyard),
            have.contains_key(&BuildingType::Barracks),
            have.contains_key(&BuildingType::SupplyDepot),
            have.contains_key(&BuildingType::MotorPool),
        );

        if let Some(bt) = target {
            let cost = building_cost(&bt);
            if can_afford(&pool, &cost) {
                if let Some(pos) = find_empty_near(&a.home, &occupied) {
                    if can_place(&pos, &occupied, &Default::default()) {
                        spend(&mut pool, &cost);
                        let _ = faction_entity; // unused but kept for diagnostic clarity
                        let id = commands
                            .spawn(BuildingBundle::new(bt.clone(), fe.faction.clone(), pos.x, pos.y))
                            .id();
                        if !building_produces(&bt).is_empty() {
                            commands.entity(id).insert(ProductionQueue::default());
                        }
                    }
                }
            }
        }

        a.last_tick = now;
    }
}

// --- Production AI ---

pub fn production_ai_system(
    time: Res<Time>,
    ai: Query<(&FactionEntity, &AiController)>,
    mut buildings: Query<(&Faction, &BuildingType, &mut ProductionQueue), With<Built>>,
    mut pools: Query<(&FactionEntity, &mut ResourcePool, &PopCap)>,
) {
    let _ = time;
    use std::collections::HashSet;
    let ai_factions: HashSet<Faction> = ai.iter().map(|(fe, _)| fe.faction.clone()).collect();

    for (b_faction, bt, mut queue) in &mut buildings {
        if !ai_factions.contains(b_faction) {
            continue;
        }
        let producible = building_produces(bt);
        if producible.is_empty() {
            continue;
        }
        if queue.jobs.len() >= QUEUE_CAP {
            continue;
        }
        // Prefer Riflemen (cheapest) for early game.
        let chosen = producible.iter().find(|u| matches!(u, UnitType::Riflemen))
            .cloned()
            .unwrap_or_else(|| producible[0].clone());
        let cost = unit_production_cost(&chosen);

        for (fe, mut pool, popcap) in &mut pools {
            if &fe.faction != b_faction {
                continue;
            }
            if popcap.current + queue.jobs.len() as u32 >= popcap.max {
                break;
            }
            if can_afford(&pool, &cost) {
                spend(&mut pool, &cost);
                queue.jobs.push(chosen.clone());
            }
            break;
        }
    }
}

// --- Tactical AI ---

/// Once per AI tick, ensure every idle AI unit has an attack target.
pub fn tactical_ai_system(
    mut commands: Commands,
    ai: Query<&FactionEntity, With<AiController>>,
    units: Query<(Entity, &Faction, &UnitPos), (With<UnitType>, Without<AttackTarget>)>,
    enemies: Query<(Entity, &Faction, &UnitPos), With<UnitType>>,
) {
    use std::collections::HashSet;
    let ai_factions: HashSet<Faction> = ai.iter().map(|fe| fe.faction.clone()).collect();

    for (entity, faction, pos) in &units {
        if !ai_factions.contains(faction) {
            continue;
        }
        // Find nearest enemy unit.
        let mut best: Option<(Entity, i32)> = None;
        for (e_entity, e_faction, e_pos) in &enemies {
            if e_faction == faction {
                continue;
            }
            let dx = (pos.pos.x - e_pos.pos.x).abs();
            let dy = (pos.pos.y - e_pos.pos.y).abs();
            let d = dx.max(dy);
            match best {
                None => best = Some((e_entity, d)),
                Some((_, bd)) if d < bd => best = Some((e_entity, d)),
                _ => {}
            }
        }
        if let Some((target, _)) = best {
            commands.entity(entity).insert(AttackTarget { entity: target });
        }
    }
}

// --- Hero AI ---

/// Fires a charged hero ability whenever ready.
pub fn hero_ai_system(
    mut commands: Commands,
    ai: Query<&FactionEntity, With<AiController>>,
    mut heroes: Query<(Entity, &Faction, &mut SignatureAbility), (With<Hero>, Without<HeroDowned>)>,
) {
    use std::collections::HashSet;
    let ai_factions: HashSet<Faction> = ai.iter().map(|fe| fe.faction.clone()).collect();

    for (entity, faction, mut s) in &mut heroes {
        if !ai_factions.contains(faction) {
            continue;
        }
        if is_charge_full(&s) {
            // Fire — for now, just reset the meter (effect application is on
            // the manual BRP path; a follow-up step could call into the
            // shared ability dispatch.)
            s.charge = 0.0;
            let _ = commands.entity(entity);
        }
    }
}

// --- Doctrine consistency ---

/// At Tier::Two, if no doctrine selected and no research in progress, start
/// the AI's chosen doctrine research.
pub fn doctrine_consistency_system(
    mut commands: Commands,
    mut ai: Query<(Entity, &AiController, &Tech, &mut ResourcePool, Option<&ResearchInProgress>)>,
) {
    for (entity, a, tech, mut pool, in_progress) in &mut ai {
        if tech.doctrine.is_some() {
            continue;
        }
        if tech.tier != Tier::Two {
            continue;
        }
        if in_progress.is_some() {
            continue;
        }
        let target = ResearchTarget::Doctrine(a.doctrine);
        if let Ok(rip) = start_research(&mut pool, tech, false, target) {
            commands.entity(entity).insert(rip);
        }
    }
}

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                economic_ai_system,
                production_ai_system,
                tactical_ai_system,
                hero_ai_system,
                doctrine_consistency_system,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_economy_target_starts_with_refinery() {
        assert_eq!(
            next_economy_target(false, false, false, false, false),
            Some(BuildingType::Refinery)
        );
    }

    #[test]
    fn next_economy_target_progresses() {
        assert_eq!(
            next_economy_target(true, false, false, false, false),
            Some(BuildingType::Scrapyard)
        );
        assert_eq!(
            next_economy_target(true, true, false, false, false),
            Some(BuildingType::Barracks)
        );
        assert_eq!(
            next_economy_target(true, true, true, false, false),
            Some(BuildingType::SupplyDepot)
        );
        assert_eq!(
            next_economy_target(true, true, true, true, false),
            Some(BuildingType::MotorPool)
        );
    }

    #[test]
    fn next_economy_target_complete() {
        assert_eq!(next_economy_target(true, true, true, true, true), None);
    }
}
