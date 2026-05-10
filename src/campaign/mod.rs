use bevy::prelude::*;
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};
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
    /// The id of the campaign definition this run belongs to (e.g. "combine").
    /// Empty string means legacy/unknown.
    #[allow(dead_code)]
    pub campaign_id: String,
    /// Ordered list of map paths for this campaign's missions (relative to assets/).
    pub mission_maps: Vec<String>,
}

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalProgress {
    pub combine_beaten: bool,
    pub ironborn_beaten: bool,
    pub handler_unlocked: bool,
    pub handler_beaten: bool,
    #[serde(skip)]
    pub first_beaten: Option<PlayableFaction>,
    #[serde(default)]
    pub campaigns_beaten: HashSet<String>,
    /// Highest mission index unlocked per campaign (= number of missions beaten,
    /// i.e. the frontier mission index the player can start from).
    #[serde(default)]
    pub missions_reached: HashMap<String, usize>,
}

// ── Data-driven campaign definitions ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignMissionDef {
    pub map: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignDef {
    pub id: String,
    pub name: String,
    pub faction: String,
    pub description: String,
    #[serde(default)]
    pub unlock_requires: String,
    pub missions: Vec<CampaignMissionDef>,
}

impl CampaignDef {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }

    /// Scan `assets/campaigns/*.toml`, load each file, sort by id.
    pub fn load_all() -> Vec<CampaignDef> {
        let dir = match std::fs::read_dir("assets/campaigns") {
            Ok(d) => d,
            Err(_) => return Vec::new(),
        };
        let mut campaigns: Vec<CampaignDef> = dir
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name();
                let s = name.to_string_lossy();
                s.ends_with(".toml")
            })
            .filter_map(|e| {
                let path = e.path();
                let path_str = path.to_string_lossy().to_string();
                match CampaignDef::load(&path_str) {
                    Ok(c) => Some(c),
                    Err(err) => {
                        eprintln!("CampaignDef::load_all: failed to load {path_str}: {err}");
                        None
                    }
                }
            })
            .collect();
        campaigns.sort_by(|a, b| a.id.cmp(&b.id));
        campaigns
    }

    /// Returns true if this campaign is unlocked given the current progress.
    /// `unlock_requires` is a comma-separated list of campaign ids that must be beaten.
    pub fn is_unlocked(&self, progress: &GlobalProgress) -> bool {
        if self.unlock_requires.is_empty() {
            return true;
        }
        for required in self.unlock_requires.split(',') {
            let req = required.trim();
            if !req.is_empty() && !progress.campaigns_beaten.contains(req) {
                return false;
            }
        }
        true
    }

    /// Return the `PlayableFaction` for this campaign's faction string.
    pub fn playable_faction(&self) -> PlayableFaction {
        match self.faction.as_str() {
            "Ironborn" => PlayableFaction::Ironborn,
            "Handler" | "Architect" => PlayableFaction::Handler,
            _ => PlayableFaction::Combine,
        }
    }
}

/// Path to the player's progress file.
fn progress_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home).join(".cindertide").join("progress.toml")
}

/// Save `GlobalProgress` to `~/.cindertide/progress.toml`.
pub fn save_progress(progress: &GlobalProgress) {
    let path = progress_path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("save_progress: failed to create dir: {e}");
            return;
        }
    }
    match toml::to_string(progress) {
        Ok(content) => {
            if let Err(e) = std::fs::write(&path, content) {
                eprintln!("save_progress: failed to write {}: {e}", path.display());
            }
        }
        Err(e) => eprintln!("save_progress: serialization failed: {e}"),
    }
}

/// Load `GlobalProgress` from `~/.cindertide/progress.toml`, returning default on any error.
pub fn load_progress() -> GlobalProgress {
    let path = progress_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => match toml::from_str::<GlobalProgress>(&content) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("load_progress: parse error: {e}");
                GlobalProgress::default()
            }
        },
        Err(_) => GlobalProgress::default(),
    }
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
    let total = if run.mission_maps.is_empty() { 5 } else { run.mission_maps.len() };
    if run.current_mission >= total {
        return None;
    }
    let seq = faction_sequence(&run.faction);
    // Wrap around the hardcoded sequence if the campaign has more than 5 missions.
    seq.into_iter().nth(run.current_mission % 5)
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
    if !won {
        return;
    }
    run.current_mission += 1;
    // Update missions_reached to track the frontier mission index.
    if !run.campaign_id.is_empty() {
        let reached = progress.missions_reached.entry(run.campaign_id.clone()).or_insert(0);
        *reached = (*reached).max(run.current_mission);
    }
    let total_missions = if run.mission_maps.is_empty() { 5 } else { run.mission_maps.len() };
    if run.current_mission >= total_missions {
        run.complete = true;
        // Record campaign beaten by id (data-driven).
        if !run.campaign_id.is_empty() {
            progress.campaigns_beaten.insert(run.campaign_id.clone());
        }
        // Also update legacy boolean flags for backwards compat.
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
        CampaignRun {
            faction,
            current_mission: 0,
            outcomes: Vec::new(),
            complete: false,
            campaign_id: String::new(),
            mission_maps: Vec::new(),
        }
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
            apply_mission_outcome(&mut run, &mut prog, true, MissionType::Defense);
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
