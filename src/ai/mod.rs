// AI opponent — economic, production, tactical, hero, and doctrine systems.

use bevy::prelude::*;
use crate::map::{Faction, GridPos};
use crate::resources::{ResourcePool, FactionEntity, can_afford, spend, PopCap};
use crate::buildings::{BuildingPos, BuildingType, BuildingBundle, building_cost, can_place, Built};
use crate::production::{ProductionQueue, building_produces, unit_production_cost, QUEUE_CAP};
use crate::units::{UnitPos, UnitType, MoveTarget, MoveProgress};
use crate::combat::{AttackTarget, Health};
use crate::heroes::{Hero, SignatureAbility, HeroDowned, is_charge_full};
use crate::tech::{Tech, Doctrine, ResearchTarget, ResearchInProgress, start_research, Tier};

pub const AI_TICK_SECONDS: f32 = 3.0;
pub const AI_BUILD_OFFSET_RADIUS: i32 = 3;
pub const ATTACK_WAVE_INTERVAL: f32 = 90.0;
pub const RETREAT_HEALTH_FRACTION: f32 = 0.25;
pub const DEFENSIVE_RADIUS_TILES: i32 = 10;
pub const DEFEND_HOME_RADIUS: i32 = 5;

/// The phase of AI development.
#[derive(Debug, Clone, PartialEq)]
pub enum AiPhase {
    /// 0–60s: build economy + military.
    EarlyGame,
    /// 60–180s: train infantry waves.
    MidGame,
    /// 180s+: train heavy units, build defenses.
    LateGame,
}

/// Per-AI-faction strategic state.
#[derive(Resource, Debug, Clone)]
pub struct AiState {
    pub phase: AiPhase,
    pub wave_timer: f32,
    /// The faction this state belongs to.
    pub faction: Faction,
    /// Whether we've sent the opening scout.
    pub scout_sent: bool,
    /// Elapsed game time in seconds (for phase transitions).
    pub elapsed: f32,
}

impl AiState {
    pub fn new(faction: Faction) -> Self {
        Self {
            phase: AiPhase::EarlyGame,
            wave_timer: 0.0,
            faction,
            scout_sent: false,
            elapsed: 0.0,
        }
    }
}

/// Marker — this unit is retreating to its home base.
#[derive(Component, Debug, Clone)]
pub struct Retreating;

/// Marker — this unit is part of an active attack wave.
#[derive(Component, Debug, Clone)]
pub struct AttackWave;

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

/// Phase-aware build order. Mid/late game may add defenses.
pub fn next_build_target(
    phase: &AiPhase,
    have: &std::collections::HashMap<BuildingType, u32>,
) -> Option<BuildingType> {
    let has = |bt: &BuildingType| have.contains_key(bt);

    match phase {
        AiPhase::EarlyGame => {
            // Economy first, then barracks.
            if !has(&BuildingType::Refinery) { return Some(BuildingType::Refinery); }
            if !has(&BuildingType::Scrapyard) { return Some(BuildingType::Scrapyard); }
            if !has(&BuildingType::Barracks)  { return Some(BuildingType::Barracks); }
            None
        }
        AiPhase::MidGame => {
            // Complete economy, add supply.
            if !has(&BuildingType::Refinery)    { return Some(BuildingType::Refinery); }
            if !has(&BuildingType::Scrapyard)   { return Some(BuildingType::Scrapyard); }
            if !has(&BuildingType::Barracks)    { return Some(BuildingType::Barracks); }
            if !has(&BuildingType::SupplyDepot) { return Some(BuildingType::SupplyDepot); }
            None
        }
        AiPhase::LateGame => {
            // Full build order + MotorPool + defensive Pillbox.
            if !has(&BuildingType::Refinery)    { return Some(BuildingType::Refinery); }
            if !has(&BuildingType::Scrapyard)   { return Some(BuildingType::Scrapyard); }
            if !has(&BuildingType::Barracks)    { return Some(BuildingType::Barracks); }
            if !has(&BuildingType::SupplyDepot) { return Some(BuildingType::SupplyDepot); }
            if !has(&BuildingType::MotorPool)   { return Some(BuildingType::MotorPool); }
            if !has(&BuildingType::Pillbox)     { return Some(BuildingType::Pillbox); }
            None
        }
    }
}

// --- Economic AI (phase-aware build orders) ---

pub fn economic_ai_system(
    mut commands: Commands,
    time: Res<Time>,
    mut ai: Query<(Entity, &mut AiController, &FactionEntity, &mut ResourcePool)>,
    buildings: Query<(&BuildingPos, &BuildingType, &Faction)>,
    ai_states: Res<AiStates>,
) {
    let now = time.elapsed_secs();
    for (_faction_entity, mut a, fe, mut pool) in &mut ai {
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

        // Determine current phase.
        let phase = ai_states.get(&fe.faction)
            .map(|s| s.phase.clone())
            .unwrap_or(AiPhase::EarlyGame);

        let target = next_build_target(&phase, &have);

        if let Some(bt) = target {
            let cost = building_cost(&bt);
            if can_afford(&pool, &cost) {
                if let Some(pos) = find_empty_near(&a.home, &occupied) {
                    if can_place(&pos, &occupied, &Default::default()) {
                        spend(&mut pool, &cost);
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

// --- Production AI (phase-aware unit training) ---

pub fn production_ai_system(
    time: Res<Time>,
    ai: Query<(&FactionEntity, &AiController)>,
    mut buildings: Query<(&Faction, &BuildingType, &mut ProductionQueue), With<Built>>,
    mut pools: Query<(&FactionEntity, &mut ResourcePool, &PopCap)>,
    ai_states: Res<AiStates>,
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

        // Phase-based unit selection.
        let phase = ai_states.get(b_faction)
            .map(|s| s.phase.clone())
            .unwrap_or(AiPhase::EarlyGame);

        let chosen = match phase {
            AiPhase::EarlyGame | AiPhase::MidGame => {
                // Prefer cheapest infantry.
                producible.iter().find(|u| matches!(u, UnitType::Riflemen))
                    .cloned()
                    .unwrap_or_else(|| producible[0].clone())
            }
            AiPhase::LateGame => {
                // Prefer heavier units: HeavyWeapons, then LightVehicle, then HeavyArmor, fallback Riflemen.
                producible.iter().find(|u| matches!(u, UnitType::HeavyWeapons))
                    .or_else(|| producible.iter().find(|u| matches!(u, UnitType::LightVehicle)))
                    .or_else(|| producible.iter().find(|u| matches!(u, UnitType::HeavyArmor)))
                    .or_else(|| producible.iter().find(|u| matches!(u, UnitType::Riflemen)))
                    .cloned()
                    .unwrap_or_else(|| producible[0].clone())
            }
        };

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

// --- Phase tracker ---

/// Tracks all AI faction states.
#[derive(Resource, Default)]
pub struct AiStates(pub std::collections::HashMap<Faction, AiState>);

impl AiStates {
    pub fn get(&self, faction: &Faction) -> Option<&AiState> {
        self.0.get(faction)
    }
    pub fn get_mut(&mut self, faction: &Faction) -> Option<&mut AiState> {
        self.0.get_mut(faction)
    }
}

/// Advance phase timers and transition AI phases.
pub fn phase_tracker_system(
    time: Res<Time>,
    ai: Query<&FactionEntity, With<AiController>>,
    mut ai_states: ResMut<AiStates>,
) {
    let dt = time.delta_secs();
    use std::collections::HashSet;
    let ai_factions: HashSet<Faction> = ai.iter().map(|fe| fe.faction.clone()).collect();

    // Ensure every AI faction has a state entry.
    for f in &ai_factions {
        ai_states.0.entry(f.clone()).or_insert_with(|| AiState::new(f.clone()));
    }

    for f in &ai_factions {
        if let Some(state) = ai_states.0.get_mut(f) {
            state.elapsed += dt;
            state.wave_timer += dt;
            state.phase = if state.elapsed < 60.0 {
                AiPhase::EarlyGame
            } else if state.elapsed < 180.0 {
                AiPhase::MidGame
            } else {
                AiPhase::LateGame
            };
        }
    }
}

// --- Scouting system ---

/// At game start, send one idle AI unit toward the map centre to scout.
pub fn scouting_system(
    mut commands: Commands,
    mut ai_states: ResMut<AiStates>,
    ai: Query<(&FactionEntity, &AiController)>,
    idle_units: Query<
        (Entity, &Faction, &UnitPos),
        (With<UnitType>, Without<AttackTarget>, Without<Retreating>, Without<AttackWave>),
    >,
) {
    // Compute map center as average of all idle unit positions.
    let positions: Vec<&UnitPos> = idle_units.iter().map(|(_, _, p)| p).collect();
    if positions.is_empty() {
        return;
    }
    let cx: i32 = positions.iter().map(|p| p.pos.x).sum::<i32>() / positions.len() as i32;
    let cy: i32 = positions.iter().map(|p| p.pos.y).sum::<i32>() / positions.len() as i32;
    let center = GridPos { x: cx, y: cy };

    for (fe, _ctrl) in &ai {
        let state = ai_states.0.entry(fe.faction.clone()).or_insert_with(|| AiState::new(fe.faction.clone()));
        if state.scout_sent {
            continue;
        }
        // Pick one idle unit and send it toward center via Commands.
        for (entity, faction, _pos) in &idle_units {
            if faction != &fe.faction {
                continue;
            }
            commands.entity(entity)
                .insert(MoveTarget { target: center.clone() })
                .insert(MoveProgress { path: vec![center.clone()], current_step: 0, elapsed: 0.0 });
            state.scout_sent = true;
            break;
        }
    }
}

// --- Attack wave system ---

/// Every ATTACK_WAVE_INTERVAL seconds, collect idle AI military units and
/// send them on an attack-move toward the nearest enemy building or unit.
pub fn attack_wave_system(
    mut commands: Commands,
    mut ai_states: ResMut<AiStates>,
    ai: Query<(&FactionEntity, &AiController)>,
    mut ai_units: Query<
        (Entity, &Faction, &UnitPos),
        (With<UnitType>, Without<AttackTarget>, Without<Retreating>, Without<crate::combat::Dead>),
    >,
    enemy_buildings: Query<(&Faction, &BuildingPos), With<Built>>,
    enemy_units: Query<(Entity, &Faction, &UnitPos), (With<UnitType>, Without<crate::combat::Dead>)>,
) {
    use std::collections::HashSet;
    let ai_factions: HashSet<Faction> = ai.iter().map(|(fe, _)| fe.faction.clone()).collect();

    for (fe, ctrl) in &ai {
        let state = match ai_states.0.get_mut(&fe.faction) {
            Some(s) => s,
            None => continue,
        };

        if state.wave_timer < ATTACK_WAVE_INTERVAL {
            continue;
        }
        state.wave_timer = 0.0;

        // Decide attack target: nearest enemy building, or nearest enemy unit.
        let target_pos: Option<GridPos> = {
            let mut best: Option<(GridPos, i32)> = None;
            for (b_faction, b_pos) in &enemy_buildings {
                if ai_factions.contains(b_faction) {
                    continue; // skip friendly buildings
                }
                let dx = (ctrl.home.x - b_pos.pos.x).abs();
                let dy = (ctrl.home.y - b_pos.pos.y).abs();
                let d = dx.max(dy);
                match &best {
                    None => best = Some((b_pos.pos.clone(), d)),
                    Some((_, bd)) if d < *bd => best = Some((b_pos.pos.clone(), d)),
                    _ => {}
                }
            }
            if best.is_none() {
                for (_, u_faction, u_pos) in &enemy_units {
                    if ai_factions.contains(u_faction) {
                        continue;
                    }
                    let dx = (ctrl.home.x - u_pos.pos.x).abs();
                    let dy = (ctrl.home.y - u_pos.pos.y).abs();
                    let d = dx.max(dy);
                    match &best {
                        None => best = Some((u_pos.pos.clone(), d)),
                        Some((_, bd)) if d < *bd => best = Some((u_pos.pos.clone(), d)),
                        _ => {}
                    }
                }
            }
            best.map(|(p, _)| p)
        };

        let Some(target) = target_pos else { continue };

        // Send all idle friendly units on an attack-move.
        for (entity, faction, _pos) in &mut ai_units {
            if faction != &fe.faction {
                continue;
            }
            commands.entity(entity)
                .insert(AttackWave)
                .insert(crate::combat::AttackMoveOrder { target: target.clone() });
        }
    }
}

// --- Retreat logic ---

/// Units whose health drops below RETREAT_HEALTH_FRACTION flee to nearest friendly building.
pub fn retreat_system(
    mut commands: Commands,
    ai: Query<&FactionEntity, With<AiController>>,
    injured: Query<
        (Entity, &Faction, &UnitPos, &Health),
        (With<UnitType>, Without<Retreating>, Without<crate::combat::Dead>),
    >,
    friendly_buildings: Query<(&Faction, &BuildingPos), With<Built>>,
) {
    use std::collections::HashSet;
    let ai_factions: HashSet<Faction> = ai.iter().map(|fe| fe.faction.clone()).collect();

    for (entity, faction, pos, health) in &injured {
        if !ai_factions.contains(faction) {
            continue;
        }
        let frac = health.current / health.max;
        if frac >= RETREAT_HEALTH_FRACTION {
            continue;
        }

        // Find the nearest friendly building.
        let mut best: Option<(GridPos, i32)> = None;
        for (b_faction, b_pos) in &friendly_buildings {
            if b_faction != faction {
                continue;
            }
            let dx = (pos.pos.x - b_pos.pos.x).abs();
            let dy = (pos.pos.y - b_pos.pos.y).abs();
            let d = dx.max(dy);
            match &best {
                None => best = Some((b_pos.pos.clone(), d)),
                Some((_, bd)) if d < *bd => best = Some((b_pos.pos.clone(), d)),
                _ => {}
            }
        }

        if let Some((retreat_pos, _)) = best {
            commands.entity(entity)
                .insert(Retreating)
                .remove::<AttackTarget>()
                .remove::<AttackWave>()
                .insert(MoveTarget { target: retreat_pos.clone() })
                .insert(MoveProgress { path: vec![retreat_pos], current_step: 0, elapsed: 0.0 });
        }
    }
}

/// Clear Retreating once a unit has recovered health above 50%.
pub fn retreat_recovery_system(
    mut commands: Commands,
    retreating: Query<(Entity, &Health), With<Retreating>>,
) {
    for (entity, health) in &retreating {
        if health.current / health.max >= 0.5 {
            commands.entity(entity).remove::<Retreating>();
        }
    }
}

// --- Defensive response system ---

/// If an enemy unit enters within DEFENSIVE_RADIUS_TILES of an AI building,
/// nearby idle AI units intercept.
pub fn defensive_response_system(
    mut commands: Commands,
    ai: Query<&FactionEntity, With<AiController>>,
    friendly_buildings: Query<(&Faction, &BuildingPos), With<Built>>,
    enemy_units: Query<(Entity, &Faction, &UnitPos), (With<UnitType>, Without<crate::combat::Dead>)>,
    mut ai_units: Query<
        (Entity, &Faction, &UnitPos),
        (With<UnitType>, Without<AttackTarget>, Without<Retreating>, Without<crate::combat::Dead>),
    >,
) {
    use std::collections::HashSet;
    let ai_factions: HashSet<Faction> = ai.iter().map(|fe| fe.faction.clone()).collect();

    // Build a set of threat positions (enemy units within range of any AI building).
    let mut threats: Vec<(Faction, Entity)> = Vec::new(); // (ai_faction, enemy entity)

    for (b_faction, b_pos) in &friendly_buildings {
        if !ai_factions.contains(b_faction) {
            continue;
        }
        for (e_entity, e_faction, e_pos) in &enemy_units {
            if ai_factions.contains(e_faction) {
                continue; // friendly
            }
            let dx = (b_pos.pos.x - e_pos.pos.x).abs();
            let dy = (b_pos.pos.y - e_pos.pos.y).abs();
            if dx.max(dy) <= DEFENSIVE_RADIUS_TILES {
                threats.push((b_faction.clone(), e_entity));
            }
        }
    }

    if threats.is_empty() {
        return;
    }

    // For each AI faction with a threat, send idle nearby units to intercept.
    for (ai_faction, enemy_entity) in &threats {
        for (entity, faction, _pos) in &mut ai_units {
            if faction != ai_faction {
                continue;
            }
            commands.entity(entity).insert(AttackTarget { entity: *enemy_entity });
        }
    }
}

// --- Tactical AI (baseline: idle units attack nearest enemy) ---

/// Once per AI tick, ensure every idle AI unit that isn't retreating or on a wave
/// has an attack target or is defending near home.
pub fn tactical_ai_system(
    mut commands: Commands,
    ai: Query<(&FactionEntity, &AiController)>,
    units: Query<
        (Entity, &Faction, &UnitPos),
        (With<UnitType>, Without<AttackTarget>, Without<Retreating>, Without<crate::combat::Dead>),
    >,
    enemies: Query<(Entity, &Faction, &UnitPos), (With<UnitType>, Without<crate::combat::Dead>)>,
) {
    use std::collections::HashSet;
    let ai_data: Vec<(Faction, GridPos)> = ai.iter()
        .map(|(fe, ctrl)| (fe.faction.clone(), ctrl.home.clone()))
        .collect();
    let ai_factions: HashSet<Faction> = ai_data.iter().map(|(f, _)| f.clone()).collect();

    for (entity, faction, pos) in &units {
        if !ai_factions.contains(faction) {
            continue;
        }

        // Find the home pos for this faction.
        let home = ai_data.iter()
            .find(|(f, _)| f == faction)
            .map(|(_, h)| h.clone())
            .unwrap_or(pos.pos.clone());

        // Check if this unit is near home (defense stance).
        let home_dx = (pos.pos.x - home.x).abs();
        let home_dy = (pos.pos.y - home.y).abs();
        let near_home = home_dx.max(home_dy) <= DEFEND_HOME_RADIUS;

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

        // If near home, only attack enemies that are also nearby.
        if let Some((target, dist)) = best {
            if near_home && dist > DEFENSIVE_RADIUS_TILES {
                // Don't chase; stay near base.
                continue;
            }
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
        app.init_resource::<AiStates>();
        app.add_systems(
            Update,
            (
                phase_tracker_system,
                economic_ai_system,
                production_ai_system,
                scouting_system,
                attack_wave_system,
                retreat_system,
                retreat_recovery_system,
                defensive_response_system,
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

    #[test]
    fn next_build_target_early_game_starts_refinery() {
        let have = std::collections::HashMap::new();
        assert_eq!(next_build_target(&AiPhase::EarlyGame, &have), Some(BuildingType::Refinery));
    }

    #[test]
    fn next_build_target_early_game_completes_at_barracks() {
        let mut have = std::collections::HashMap::new();
        have.insert(BuildingType::Refinery, 1);
        have.insert(BuildingType::Scrapyard, 1);
        have.insert(BuildingType::Barracks, 1);
        assert_eq!(next_build_target(&AiPhase::EarlyGame, &have), None);
    }

    #[test]
    fn next_build_target_late_game_adds_motor_pool_and_pillbox() {
        let mut have = std::collections::HashMap::new();
        have.insert(BuildingType::Refinery, 1);
        have.insert(BuildingType::Scrapyard, 1);
        have.insert(BuildingType::Barracks, 1);
        have.insert(BuildingType::SupplyDepot, 1);
        // MotorPool not yet built → should target it next.
        assert_eq!(next_build_target(&AiPhase::LateGame, &have), Some(BuildingType::MotorPool));
    }

    #[test]
    fn ai_state_phase_starts_early() {
        let s = AiState::new(Faction::Combine);
        assert_eq!(s.phase, AiPhase::EarlyGame);
        assert!(!s.scout_sent);
    }
}
