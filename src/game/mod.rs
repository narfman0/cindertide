// Game — top-level state machine (title / campaign / in-mission / over),
// the campaign run progression loop, and victory/defeat resolution.

use bevy::prelude::*;
use bevy::app::AppExit;
use crate::map::Faction;
use crate::campaign::CampaignState;
use crate::mission::{Mission, MissionStatus};

#[derive(Resource, Debug, Clone, PartialEq)]
pub enum GameState {
    Title,
    Campaign,
    InMission,
    GameOver { won: bool },
}

impl Default for GameState {
    fn default() -> Self { Self::Title }
}

#[derive(Resource, Debug, Default, Clone)]
pub struct CampaignRun {
    pub player: Option<Faction>,
    pub missions_won: u32,
    pub missions_lost: u32,
    /// (Mission entity, zone id the mission is contesting)
    pub current_mission: Option<(Entity, u32)>,
}

// --- Pure rules ---

/// Compute the campaign outcome from current zone ownership.
pub fn campaign_outcome(zones: &[crate::campaign::Zone], player: &Faction) -> Option<bool> {
    if zones.is_empty() {
        return None;
    }
    let owned = zones.iter().filter(|z| z.owner.as_ref() == Some(player)).count();
    if owned == zones.len() {
        Some(true) // victory
    } else if owned == 0 {
        Some(false) // defeat
    } else {
        None // continue
    }
}

// --- System ---

/// Watches the active mission for resolution; on Won/Lost applies the
/// campaign outcome (zone ownership shift) and returns to Campaign state,
/// or to GameOver when victory/defeat conditions are met.
pub fn campaign_progression_system(
    mut commands: Commands,
    mut state: ResMut<GameState>,
    mut run: ResMut<CampaignRun>,
    mut campaign: Option<ResMut<CampaignState>>,
    missions: Query<&Mission>,
) {
    if *state != GameState::InMission {
        return;
    }
    let Some((mission_entity, zone_id)) = run.current_mission else {
        return;
    };
    let Ok(m) = missions.get(mission_entity) else {
        // Mission entity is gone — clean up.
        run.current_mission = None;
        return;
    };
    if m.status == MissionStatus::Active {
        return;
    }

    let won = m.status == MissionStatus::Won;
    if won {
        run.missions_won += 1;
    } else {
        run.missions_lost += 1;
    }

    if let Some(ref mut cs) = campaign {
        let (winner, loser) = if won {
            (m.player_faction.clone(), m.opponent_faction.clone())
        } else {
            (m.opponent_faction.clone(), m.player_faction.clone())
        };
        crate::campaign::apply_mission_outcome(cs, zone_id, winner, loser);
        crate::campaign::spread_corruption(cs);
    }

    commands.entity(mission_entity).despawn();
    run.current_mission = None;

    // Victory / defeat check.
    if let (Some(cs), Some(player)) = (campaign.as_ref(), run.player.as_ref()) {
        if let Some(victory) = campaign_outcome(&cs.zones, player) {
            *state = GameState::GameOver { won: victory };
            return;
        }
    }

    *state = GameState::Campaign;
}

/// Sends an AppExit event to terminate the process. Used by game/exit.
pub fn fire_exit(world: &mut World) {
    world.send_event(AppExit::Success);
}

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameState>();
        app.init_resource::<CampaignRun>();
        app.add_systems(Update, campaign_progression_system);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::campaign::Zone;

    fn zone(id: u32, owner: Option<Faction>) -> Zone {
        Zone { id, name: format!("Z{id}"), owner, corruption: 0.0, adjacent: vec![] }
    }

    #[test]
    fn outcome_victory_when_all_zones_player_owned() {
        let zs = vec![
            zone(0, Some(Faction::Combine)),
            zone(1, Some(Faction::Combine)),
        ];
        assert_eq!(campaign_outcome(&zs, &Faction::Combine), Some(true));
    }

    #[test]
    fn outcome_defeat_when_zero_zones_player_owned() {
        let zs = vec![
            zone(0, Some(Faction::Hollow)),
            zone(1, None),
        ];
        assert_eq!(campaign_outcome(&zs, &Faction::Combine), Some(false));
    }

    #[test]
    fn outcome_continue_when_mixed() {
        let zs = vec![
            zone(0, Some(Faction::Combine)),
            zone(1, Some(Faction::Hollow)),
        ];
        assert_eq!(campaign_outcome(&zs, &Faction::Combine), None);
    }

    #[test]
    fn outcome_continue_with_neutral_zones() {
        let zs = vec![
            zone(0, Some(Faction::Combine)),
            zone(1, None),
        ];
        assert_eq!(campaign_outcome(&zs, &Faction::Combine), None);
    }

    #[test]
    fn default_state_is_title() {
        assert_eq!(GameState::default(), GameState::Title);
    }

    #[test]
    fn default_run_is_empty() {
        let r = CampaignRun::default();
        assert!(r.player.is_none());
        assert!(r.current_mission.is_none());
        assert_eq!(r.missions_won, 0);
        assert_eq!(r.missions_lost, 0);
    }
}
