// Missions — win/lose state machine on top of generated maps.

use bevy::prelude::*;
use crate::map::{Faction, ControlPoint};
use crate::mapgen::MissionType;
use crate::buildings::{BuildingType, BuildingPos, Built};
use crate::combat::{Health, Dead};
use crate::units::{UnitType, UnitPos};
use crate::resources::FactionEntity;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub enum MissionStatus {
    Active,
    Won,
    Lost,
}

/// Singleton-ish component holding the active mission's state. Spawn one
/// when a match starts.
#[derive(Component, Debug, Clone)]
pub struct Mission {
    pub mission_type: MissionType,
    pub player_faction: Faction,
    pub opponent_faction: Faction,
    pub status: MissionStatus,
    pub elapsed: f32,
    pub deadline: f32,
    /// KotH: cumulative seconds the player has held the hill
    pub hill_timer: f32,
    /// KotH: seconds needed to win (default 180.0)
    pub hill_threshold: f32,
    /// Assassination: which entity is the enemy commander to kill
    pub assassination_target: Option<Entity>,
    /// FFA: throttle timer (checks every 2 seconds)
    pub ffa_check_timer: f32,
}

impl Mission {
    pub fn new(mission_type: MissionType, player_faction: Faction, opponent_faction: Faction) -> Self {
        Self {
            mission_type,
            player_faction,
            opponent_faction,
            status: MissionStatus::Active,
            elapsed: 0.0,
            deadline: 300.0,
            hill_timer: 0.0,
            hill_threshold: 180.0,
            assassination_target: None,
            ffa_check_timer: 0.0,
        }
    }
}

/// Tag component marking the designated commander unit of a faction.
#[derive(Component, Debug, Clone)]
pub struct Commander {
    pub faction: Faction,
}

// --- Pure rules ---

/// Decide win/lose based on snapshot of world state.
/// Returns the new status given current state.
pub fn evaluate_status(
    mt: &MissionType,
    status: &MissionStatus,
    elapsed: f32,
    deadline: f32,
    player_units_alive: u32,
    opponent_units_alive: u32,
    player_command_alive: bool,
    opponent_command_alive: bool,
    player_points_held: u32,
    opponent_points_held: u32,
) -> MissionStatus {
    if *status != MissionStatus::Active {
        return status.clone();
    }
    match mt {
        MissionType::Assault => {
            if !opponent_command_alive { MissionStatus::Won }
            else if !player_command_alive { MissionStatus::Lost }
            else { MissionStatus::Active }
        }
        MissionType::Control => {
            if elapsed >= deadline {
                if player_points_held > opponent_points_held { MissionStatus::Won }
                else if opponent_points_held > player_points_held { MissionStatus::Lost }
                else { MissionStatus::Lost } // tie defaults to player loss (defender)
            } else {
                MissionStatus::Active
            }
        }
        MissionType::Defense => {
            if !player_command_alive { MissionStatus::Lost }
            else if elapsed >= deadline { MissionStatus::Won }
            else { MissionStatus::Active }
        }
        MissionType::Extraction => {
            if player_units_alive == 0 { MissionStatus::Lost }
            else if elapsed >= deadline { MissionStatus::Won }
            else { MissionStatus::Active }
        }
        MissionType::Survival => {
            if player_units_alive == 0 { MissionStatus::Lost }
            else if elapsed >= deadline { MissionStatus::Won }
            else if opponent_units_alive == 0 { MissionStatus::Active } // waves continue
            else { MissionStatus::Active }
        }
        // These mission types have dedicated systems (ffa/koth/assassination_win_system)
        // and do not use the evaluate_status path.
        MissionType::Ffa | MissionType::KingOfTheHill | MissionType::Assassination => {
            MissionStatus::Active
        }
    }
}

// --- System ---

pub fn mission_check_system(
    time: Res<Time>,
    mut missions: Query<&mut Mission>,
    units: Query<&Faction, (With<UnitType>, Without<Dead>)>,
    bunkers: Query<(&Faction, &BuildingType, &Health)>,
    points: Query<&ControlPoint>,
    factions: Query<&FactionEntity>,
) {
    let dt = time.delta_secs();
    for mut m in &mut missions {
        if m.status != MissionStatus::Active {
            continue;
        }
        m.elapsed += dt;

        let mut player_units = 0u32;
        let mut opponent_units = 0u32;
        for f in &units {
            if f == &m.player_faction { player_units += 1; }
            else if f == &m.opponent_faction { opponent_units += 1; }
        }

        let mut player_cmd = true;
        let mut opponent_cmd = true;
        // If a faction has zero CommandBunkers (or just zero BUILT bunkers
        // alive), treat as command lost. We track any-alive bunker.
        let mut player_has_cmd = false;
        let mut opponent_has_cmd = false;
        for (f, bt, h) in &bunkers {
            if matches!(bt, BuildingType::CommandBunker) && h.current > 0.0 {
                if f == &m.player_faction { player_has_cmd = true; }
                if f == &m.opponent_faction { opponent_has_cmd = true; }
            }
        }
        // Only meaningful if ANY bunker exists for that faction to begin
        // with; otherwise treat as alive (mission start grace).
        let any_player_bunker = bunkers
            .iter()
            .any(|(f, bt, _)| f == &m.player_faction && matches!(bt, BuildingType::CommandBunker));
        let any_opp_bunker = bunkers
            .iter()
            .any(|(f, bt, _)| f == &m.opponent_faction && matches!(bt, BuildingType::CommandBunker));
        if any_player_bunker { player_cmd = player_has_cmd; }
        if any_opp_bunker { opponent_cmd = opponent_has_cmd; }

        let mut player_points = 0u32;
        let mut opponent_points = 0u32;
        for cp in &points {
            if let Some(o) = &cp.owner {
                if o == &m.player_faction { player_points += 1; }
                else if o == &m.opponent_faction { opponent_points += 1; }
            }
        }

        m.status = evaluate_status(
            &m.mission_type,
            &m.status,
            m.elapsed,
            m.deadline,
            player_units,
            opponent_units,
            player_cmd,
            opponent_cmd,
            player_points,
            opponent_points,
        );

        // Suppress unused-warning on factions param
        let _ = factions.is_empty();
    }
}

// ── FFA win condition ─────────────────────────────────────────────────────────

/// FFA: last faction with at least one living unit OR built building wins.
/// Throttled to check every 2 seconds.
pub fn ffa_win_system(
    time: Res<Time>,
    mut missions: Query<&mut Mission>,
    units: Query<(&Faction, Entity), (With<UnitType>, Without<Dead>)>,
    buildings: Query<(&Faction, Entity), (With<BuildingPos>, With<Built>, Without<Dead>)>,
) {
    let dt = time.delta_secs();
    for mut m in &mut missions {
        if m.status != MissionStatus::Active {
            continue;
        }
        if m.mission_type != MissionType::Ffa {
            continue;
        }

        m.ffa_check_timer += dt;
        if m.ffa_check_timer < 2.0 {
            continue;
        }
        m.ffa_check_timer = 0.0;

        // Collect factions still alive (have a unit or built building).
        let mut alive_factions: HashSet<Faction> = HashSet::new();
        for (f, _) in &units {
            alive_factions.insert(f.clone());
        }
        for (f, _) in &buildings {
            alive_factions.insert(f.clone());
        }

        if alive_factions.len() <= 1 {
            // Determine winner
            if alive_factions.contains(&m.player_faction) {
                m.status = MissionStatus::Won;
            } else {
                m.status = MissionStatus::Lost;
            }
        }
    }
}

// ── KotH win condition ────────────────────────────────────────────────────────

/// KotH: player accumulates hill_timer when they have more units in the center
/// zone (radius 5 tiles) than the enemy. First to hill_threshold seconds wins.
pub fn koth_win_system(
    time: Res<Time>,
    mut missions: Query<&mut Mission>,
    units: Query<(&Faction, &UnitPos), (With<UnitType>, Without<Dead>)>,
) {
    let dt = time.delta_secs();
    // Hill center for a 128×80 map
    const HILL_CX: i32 = 64;
    const HILL_CY: i32 = 40;
    const HILL_RADIUS: i32 = 5;

    for mut m in &mut missions {
        if m.status != MissionStatus::Active {
            continue;
        }
        if m.mission_type != MissionType::KingOfTheHill {
            continue;
        }

        let mut player_count = 0i32;
        let mut enemy_count = 0i32;

        for (f, pos) in &units {
            let dx = (pos.pos.x - HILL_CX).abs();
            let dy = (pos.pos.y - HILL_CY).abs();
            if dx <= HILL_RADIUS && dy <= HILL_RADIUS {
                if f == &m.player_faction {
                    player_count += 1;
                } else if f == &m.opponent_faction {
                    enemy_count += 1;
                }
            }
        }

        if player_count > enemy_count {
            m.hill_timer += dt;
        } else if enemy_count > player_count {
            m.hill_timer = (m.hill_timer - dt * 0.5).max(0.0);
        }
        m.hill_timer = m.hill_timer.clamp(0.0, m.hill_threshold);

        if m.hill_timer >= m.hill_threshold {
            m.status = MissionStatus::Won;
        }
    }
}

// ── Assassination win condition ───────────────────────────────────────────────

/// Assassination: kill the enemy commander to win; losing your own commander
/// is an instant loss.
pub fn assassination_win_system(
    mut missions: Query<&mut Mission>,
    commanders: Query<(Entity, &Commander, Option<&Dead>)>,
) {
    for mut m in &mut missions {
        if m.status != MissionStatus::Active {
            continue;
        }
        if m.mission_type != MissionType::Assassination {
            continue;
        }

        let mut player_commander_dead = false;
        let mut enemy_commander_dead = false;

        for (_, commander, dead) in &commanders {
            let is_dead = dead.is_some();
            if commander.faction == m.player_faction && is_dead {
                player_commander_dead = true;
            } else if commander.faction == m.opponent_faction && is_dead {
                enemy_commander_dead = true;
            }
        }

        if enemy_commander_dead {
            m.status = MissionStatus::Won;
        } else if player_commander_dead {
            m.status = MissionStatus::Lost;
        }
    }
}

pub struct MissionPlugin;

impl Plugin for MissionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, mission_check_system);
        app.add_systems(Update, (ffa_win_system, koth_win_system, assassination_win_system));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assault_won_when_opponent_command_dies() {
        let s = evaluate_status(
            &MissionType::Assault, &MissionStatus::Active,
            10.0, 600.0, 5, 5, true, false, 0, 0,
        );
        assert_eq!(s, MissionStatus::Won);
    }

    #[test]
    fn assault_lost_when_player_command_dies() {
        let s = evaluate_status(
            &MissionType::Assault, &MissionStatus::Active,
            10.0, 600.0, 5, 5, false, true, 0, 0,
        );
        assert_eq!(s, MissionStatus::Lost);
    }

    #[test]
    fn control_won_at_deadline_with_more_points() {
        let s = evaluate_status(
            &MissionType::Control, &MissionStatus::Active,
            301.0, 300.0, 5, 5, true, true, 3, 1,
        );
        assert_eq!(s, MissionStatus::Won);
    }

    #[test]
    fn defense_won_at_deadline_with_command_alive() {
        let s = evaluate_status(
            &MissionType::Defense, &MissionStatus::Active,
            301.0, 300.0, 1, 0, true, false, 0, 0,
        );
        assert_eq!(s, MissionStatus::Won);
    }

    #[test]
    fn extraction_lost_when_all_player_units_die() {
        let s = evaluate_status(
            &MissionType::Extraction, &MissionStatus::Active,
            10.0, 600.0, 0, 5, true, true, 0, 0,
        );
        assert_eq!(s, MissionStatus::Lost);
    }

    #[test]
    fn survival_won_at_deadline() {
        let s = evaluate_status(
            &MissionType::Survival, &MissionStatus::Active,
            301.0, 300.0, 5, 0, true, false, 0, 0,
        );
        assert_eq!(s, MissionStatus::Won);
    }

    #[test]
    fn won_status_is_terminal() {
        let s = evaluate_status(
            &MissionType::Assault, &MissionStatus::Won,
            0.0, 600.0, 0, 0, false, false, 0, 0,
        );
        assert_eq!(s, MissionStatus::Won);
    }
}
