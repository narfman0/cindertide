// Missions — win/lose state machine on top of generated maps.

use bevy::prelude::*;
use crate::map::{Faction, ControlPoint};
use crate::mapgen::MissionType;
use crate::buildings::{BuildingType, BuildingPos};
use crate::combat::{Health, Dead};
use crate::units::UnitType;
use crate::resources::FactionEntity;

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

pub struct MissionPlugin;

impl Plugin for MissionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, mission_check_system);
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
