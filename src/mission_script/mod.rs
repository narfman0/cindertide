// TOML-driven mission scripting system.
//
// Scripts live in `assets/scripts/<name>.toml`. Each script defines a sequence
// of events that fire based on time elapsed or named beat conditions.

use bevy::prelude::*;
use serde::Deserialize;
use std::collections::{HashSet, VecDeque};

use crate::beats::{BeatId, FiredBeats};
use crate::map::{Faction, GridPos};
use crate::mission::Mission;
use crate::units::{UnitBundle, HomeBase};
use crate::factions::LoadedFactions;

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct MissionScript {
    pub events: Vec<ScriptEvent>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScriptEvent {
    pub id: String,
    pub trigger: Trigger,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    Time { seconds: f32 },
    Condition { condition: String, beat_id: Option<String> },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Dialogue { text: String },
    SpawnUnits {
        faction: String,
        unit_type: String,
        count: u32,
        x: i32,
        y: i32,
    },
    Objective { text: String },
    ChangeObjective { text: String },
    WinMission,
    LoseMission,
}

impl MissionScript {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }
}

// ── Runtime state ─────────────────────────────────────────────────────────────

#[derive(Resource, Default)]
pub struct ScriptState {
    pub script: Option<MissionScript>,
    /// Event ids that have already fired.
    pub fired: HashSet<String>,
    /// Pending dialogue messages — shown one at a time.
    pub dialogue_queue: VecDeque<String>,
    /// Override text shown in the objectives panel when `Some`.
    pub current_objective: Option<String>,
    /// Countdown timer for the currently-displayed dialogue line (seconds).
    pub dialogue_timer: f32,
}

impl ScriptState {
    /// Load (or reload) a script by name. Clears previous state.
    pub fn load_script(&mut self, name: &str) {
        let path = format!("assets/scripts/{name}.toml");
        match MissionScript::load(&path) {
            Ok(script) => {
                info!("[script] loaded {path} ({} events)", script.events.len());
                self.script = Some(script);
            }
            Err(e) => {
                info!("[script] no script at {path}: {e}");
                self.script = None;
            }
        }
        self.fired.clear();
        self.dialogue_queue.clear();
        self.current_objective = None;
        self.dialogue_timer = 0.0;
    }
}

// ── Helper: parse faction string ──────────────────────────────────────────────

fn parse_faction(s: &str) -> Faction {
    Faction::new(&s.to_lowercase())
}

fn parse_unit_type_id(s: &str) -> String {
    // Normalize common aliases to canonical IDs
    match s.to_lowercase().replace(' ', "_").as_str() {
        "rifleman" => "riflemen".to_string(),
        "heavyweapons" | "heavy_weapon" | "heavyweapon" => "heavy_weapons".to_string(),
        "lightvehicle" | "light_vehicle" => "light_vehicle".to_string(),
        "heavyarmor" | "heavy_armor" | "heavyarmour" | "heavy_armour" => "heavy_armor".to_string(),
        other => other.to_string(),
    }
}

fn parse_beat_id(s: &str) -> Option<BeatId> {
    match s {
        "HeroGoesDown" => Some(BeatId::HeroGoesDown),
        "LastStand" => Some(BeatId::LastStand),
        "AncientUnification" => Some(BeatId::AncientUnification),
        _ => None,
    }
}

// ── Systems ───────────────────────────────────────────────────────────────────

/// Tick all script events and fire them when their trigger condition is met.
pub fn script_tick_system(
    mut commands: Commands,
    mut script_state: ResMut<ScriptState>,
    time: Res<Time>,
    fired_beats: Res<FiredBeats>,
    mut mission_q: Query<&mut Mission>,
    loaded: Res<LoadedFactions>,
) {
    let dt = time.delta_secs();

    // Advance dialogue timer; pop front message when it expires.
    if !script_state.dialogue_queue.is_empty() {
        script_state.dialogue_timer -= dt;
        if script_state.dialogue_timer <= 0.0 {
            script_state.dialogue_queue.pop_front();
            script_state.dialogue_timer = if script_state.dialogue_queue.is_empty() {
                0.0
            } else {
                4.0
            };
        }
    }

    // Get the elapsed time from the first active mission.
    let elapsed = mission_q.iter().next().map(|m| m.elapsed).unwrap_or(0.0);

    // Collect events to fire (can't mutate while iterating the resource).
    let events_to_fire: Vec<usize> = {
        let Some(ref script) = script_state.script else {
            return;
        };
        script
            .events
            .iter()
            .enumerate()
            .filter_map(|(i, ev)| {
                if script_state.fired.contains(&ev.id) {
                    return None;
                }
                let triggered = match &ev.trigger {
                    Trigger::Time { seconds } => elapsed >= *seconds,
                    Trigger::Condition { condition, beat_id } => {
                        if condition == "beat" {
                            if let Some(bid_str) = beat_id {
                                if let Some(bid) = parse_beat_id(bid_str) {
                                    fired_beats.0.contains(&bid)
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                };
                if triggered { Some(i) } else { None }
            })
            .collect()
    };

    // Execute each triggered event.
    for idx in events_to_fire {
        let (event_id, actions) = {
            let script = script_state.script.as_ref().unwrap();
            let ev = &script.events[idx];
            (ev.id.clone(), ev.actions.clone())
        };

        for action in &actions {
            match action {
                Action::Dialogue { text } => {
                    let was_empty = script_state.dialogue_queue.is_empty();
                    script_state.dialogue_queue.push_back(text.clone());
                    if was_empty {
                        script_state.dialogue_timer = 4.0;
                    }
                }
                Action::SpawnUnits { faction, unit_type, count, x, y } => {
                    let f = parse_faction(faction);
                    let unit_id = parse_unit_type_id(unit_type);
                    for i in 0..*count as i32 {
                        let spawn_x = x + (i % 4);
                        let spawn_y = y + (i / 4);
                        let home = GridPos { x: spawn_x, y: spawn_y };
                        if let Some(def) = loaded.units.get(&unit_id) {
                            commands.spawn(UnitBundle::from_def(def, f.clone(), spawn_x, spawn_y))
                                .insert(HomeBase { pos: home });
                        } else {
                            commands.spawn(UnitBundle::default_riflemen(f.clone(), spawn_x, spawn_y))
                                .insert(HomeBase { pos: home });
                        }
                    }
                    info!("[script] spawned {} {} ({}) at ({}, {})", count, unit_id, faction, x, y);
                }
                Action::Objective { text } | Action::ChangeObjective { text } => {
                    script_state.current_objective = Some(text.clone());
                }
                Action::WinMission => {
                    for mut m in &mut mission_q {
                        if m.status == crate::mission::MissionStatus::Active {
                            m.status = crate::mission::MissionStatus::Won;
                        }
                    }
                }
                Action::LoseMission => {
                    for mut m in &mut mission_q {
                        if m.status == crate::mission::MissionStatus::Active {
                            m.status = crate::mission::MissionStatus::Lost;
                        }
                    }
                }
            }
        }

        script_state.fired.insert(event_id);
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct MissionScriptPlugin;

impl Plugin for MissionScriptPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptState>();
        app.add_systems(Update, script_tick_system);
    }
}
