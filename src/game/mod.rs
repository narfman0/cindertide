use bevy::prelude::*;
use bevy::app::AppExit;
use crate::campaign::{CampaignRun, GlobalProgress, PlayableFaction, apply_mission_outcome};
use crate::mission::{Mission, MissionStatus};

#[derive(Resource, Debug, Clone, PartialEq)]
pub enum GameState {
    Title,
    Campaign,
    InMission,
    GameOver { won: bool, handler_unlocked: bool },
}

impl Default for GameState {
    fn default() -> Self { Self::Title }
}

#[derive(Resource, Debug, Default, Clone)]
pub struct ActiveRun {
    pub run: Option<CampaignRun>,
    pub current_mission_entity: Option<Entity>,
    pub missions_won: u32,
    pub missions_lost: u32,
}

pub fn campaign_progression_system(
    mut commands: Commands,
    mut state: ResMut<GameState>,
    mut active: ResMut<ActiveRun>,
    mut progress: ResMut<GlobalProgress>,
    missions: Query<&Mission>,
) {
    if *state != GameState::InMission {
        return;
    }
    let Some(mission_entity) = active.current_mission_entity else {
        return;
    };
    let Ok(m) = missions.get(mission_entity) else {
        active.current_mission_entity = None;
        return;
    };
    if m.status == MissionStatus::Active {
        return;
    }

    let won = m.status == MissionStatus::Won;
    if won { active.missions_won += 1; } else { active.missions_lost += 1; }

    let mission_type = m.mission_type.clone();
    commands.entity(mission_entity).despawn();
    active.current_mission_entity = None;

    if let Some(ref mut run) = active.run {
        apply_mission_outcome(run, &mut progress, won, mission_type);
        if run.complete {
            let handler_unlocked = progress.handler_unlocked;
            *state = GameState::GameOver { won: true, handler_unlocked };
            return;
        }
    }

    *state = GameState::Campaign;
}

pub fn fire_exit(world: &mut World) {
    world.send_event(AppExit::Success);
}

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameState>();
        app.init_resource::<ActiveRun>();
        app.add_systems(Update, campaign_progression_system);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::campaign::PlayableFaction;

    #[test]
    fn default_state_is_title() {
        assert_eq!(GameState::default(), GameState::Title);
    }

    #[test]
    fn active_run_default_is_empty() {
        let r = ActiveRun::default();
        assert!(r.run.is_none());
        assert!(r.current_mission_entity.is_none());
        assert_eq!(r.missions_won, 0);
        assert_eq!(r.missions_lost, 0);
    }

    #[test]
    fn game_over_carries_handler_flag() {
        let s = GameState::GameOver { won: true, handler_unlocked: true };
        if let GameState::GameOver { handler_unlocked, .. } = s {
            assert!(handler_unlocked);
        } else {
            panic!("expected GameOver");
        }
    }

    #[test]
    fn new_run_for_combine_starts_at_mission_zero() {
        let run = CampaignRun {
            faction: PlayableFaction::Combine,
            current_mission: 0,
            outcomes: Vec::new(),
            complete: false,
            campaign_id: String::new(),
            mission_maps: Vec::new(),
        };
        assert_eq!(run.current_mission, 0);
        assert!(!run.complete);
    }
}
