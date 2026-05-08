use bevy::prelude::*;
use crate::mapgen::MissionType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayableFaction {
    Combine,
    Ironborn,
    Handler,
}

#[derive(Debug, Clone)]
pub struct MissionOutcome {
    pub mission_index: usize,
    pub won: bool,
    pub mission_type: MissionType,
}

#[derive(Resource, Debug, Clone)]
pub struct CampaignRun {
    pub faction: PlayableFaction,
    pub current_mission: usize,
    pub outcomes: Vec<MissionOutcome>,
    pub complete: bool,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct GlobalProgress {
    pub combine_beaten: bool,
    pub ironborn_beaten: bool,
    pub handler_unlocked: bool,
    pub handler_beaten: bool,
    pub first_beaten: Option<PlayableFaction>,
}

fn faction_sequence(faction: &PlayableFaction) -> [MissionType; 5] {
    match faction {
        PlayableFaction::Combine => [
            MissionType::Assault,
            MissionType::Control,
            MissionType::Defense,
            MissionType::Control,
            MissionType::Assault,
        ],
        PlayableFaction::Ironborn => [
            MissionType::Defense,
            MissionType::Assault,
            MissionType::Control,
            MissionType::Assault,
            MissionType::Defense,
        ],
        PlayableFaction::Handler => [
            MissionType::Extraction,
            MissionType::Survival,
            MissionType::Extraction,
            MissionType::Survival,
            MissionType::Assault,
        ],
    }
}

pub fn next_mission_type(run: &CampaignRun) -> Option<MissionType> {
    if run.current_mission >= 5 {
        return None;
    }
    let seq = faction_sequence(&run.faction);
    seq.into_iter().nth(run.current_mission)
}

pub fn apply_mission_outcome(
    run: &mut CampaignRun,
    progress: &mut GlobalProgress,
    won: bool,
    mission_type: MissionType,
) {
    run.outcomes.push(MissionOutcome {
        mission_index: run.current_mission,
        won,
        mission_type,
    });
    run.current_mission += 1;
    if run.current_mission >= 5 {
        run.complete = true;
        match run.faction {
            PlayableFaction::Combine => {
                if !progress.combine_beaten {
                    progress.combine_beaten = true;
                    if progress.first_beaten.is_none() {
                        progress.first_beaten = Some(PlayableFaction::Combine);
                    }
                }
            }
            PlayableFaction::Ironborn => {
                if !progress.ironborn_beaten {
                    progress.ironborn_beaten = true;
                    if progress.first_beaten.is_none() {
                        progress.first_beaten = Some(PlayableFaction::Ironborn);
                    }
                }
            }
            PlayableFaction::Handler => {
                progress.handler_beaten = true;
            }
        }
        if progress.combine_beaten || progress.ironborn_beaten {
            progress.handler_unlocked = true;
        }
    }
}

pub struct CampaignPlugin;

impl Plugin for CampaignPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GlobalProgress>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_run(faction: PlayableFaction) -> CampaignRun {
        CampaignRun { faction, current_mission: 0, outcomes: Vec::new(), complete: false }
    }

    #[test]
    fn combine_first_mission_is_assault() {
        let run = fresh_run(PlayableFaction::Combine);
        assert_eq!(next_mission_type(&run), Some(MissionType::Assault));
    }

    #[test]
    fn ironborn_first_mission_is_defense() {
        let run = fresh_run(PlayableFaction::Ironborn);
        assert_eq!(next_mission_type(&run), Some(MissionType::Defense));
    }

    #[test]
    fn handler_first_mission_is_extraction() {
        let run = fresh_run(PlayableFaction::Handler);
        assert_eq!(next_mission_type(&run), Some(MissionType::Extraction));
    }

    #[test]
    fn no_mission_after_five() {
        let mut run = fresh_run(PlayableFaction::Combine);
        run.current_mission = 5;
        assert_eq!(next_mission_type(&run), None);
    }

    #[test]
    fn complete_after_five_outcomes() {
        let mut run = fresh_run(PlayableFaction::Combine);
        let mut prog = GlobalProgress::default();
        for _ in 0..5 {
            apply_mission_outcome(&mut run, &mut prog, true, MissionType::Assault);
        }
        assert!(run.complete);
        assert_eq!(run.current_mission, 5);
        assert_eq!(run.outcomes.len(), 5);
    }

    #[test]
    fn combine_beaten_sets_handler_unlocked() {
        let mut run = fresh_run(PlayableFaction::Combine);
        let mut prog = GlobalProgress::default();
        for _ in 0..5 {
            apply_mission_outcome(&mut run, &mut prog, true, MissionType::Assault);
        }
        assert!(prog.combine_beaten);
        assert!(prog.handler_unlocked);
        assert_eq!(prog.first_beaten, Some(PlayableFaction::Combine));
    }

    #[test]
    fn ironborn_beaten_sets_handler_unlocked() {
        let mut run = fresh_run(PlayableFaction::Ironborn);
        let mut prog = GlobalProgress::default();
        for _ in 0..5 {
            apply_mission_outcome(&mut run, &mut prog, false, MissionType::Defense);
        }
        assert!(prog.ironborn_beaten);
        assert!(prog.handler_unlocked);
    }

    #[test]
    fn first_beaten_is_first_finished_faction() {
        let mut run_c = fresh_run(PlayableFaction::Combine);
        let mut prog = GlobalProgress::default();
        for _ in 0..5 {
            apply_mission_outcome(&mut run_c, &mut prog, true, MissionType::Assault);
        }
        let mut run_i = fresh_run(PlayableFaction::Ironborn);
        for _ in 0..5 {
            apply_mission_outcome(&mut run_i, &mut prog, true, MissionType::Defense);
        }
        assert_eq!(prog.first_beaten, Some(PlayableFaction::Combine));
    }

    #[test]
    fn outcome_indices_sequential() {
        let mut run = fresh_run(PlayableFaction::Ironborn);
        let mut prog = GlobalProgress::default();
        for i in 0..3 {
            apply_mission_outcome(&mut run, &mut prog, true, MissionType::Assault);
            assert_eq!(run.outcomes[i].mission_index, i);
        }
    }
}
