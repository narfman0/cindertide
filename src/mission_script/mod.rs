// TOML-driven mission scripting system.
//
// Scripts live in `assets/scripts/<name>.toml`. Each script defines a sequence
// of events that fire based on time elapsed or named beat conditions.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};

use crate::beats::{BeatId, FiredBeats};
use crate::buildings::{BuildingPos, BuildingTypeId};
use crate::camera::{framing_for, CameraFocusTarget, CameraShake, CameraTarget, CinematicFraming, FramingPreset};
use crate::map::{Faction, GridPos};
use crate::mission::Mission;
use crate::units::{UnitBundle, UnitPos, UnitTypeId, HomeBase};
use crate::factions::LoadedFactions;

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MissionScript {
    pub events: Vec<ScriptEvent>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScriptEvent {
    pub id: String,
    pub trigger: Trigger,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    Time { seconds: f32 },
    Condition { condition: String, beat_id: Option<String> },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Dialogue { speaker: Option<String>, text: String },
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
    /// Direct camera at a focus target. The camera tweens smoothly; user WASD input
    /// will override the focus and return to free-look.
    ///
    /// `framing` resolution chain when omitted:
    ///   1. `CinematicFraming` component on the target entity (Unit / building)
    ///   2. The relevant unit/building def's `cinematic_framing` field
    ///   3. The relevant faction's `cinematic_framing` default
    ///   4. `"isometric"` fallback
    CameraFocus {
        target: CameraFocusTarget,
        #[serde(default)] framing: Option<String>,
    },
    /// Release scripted focus — camera returns to free-look (preserving its current pose).
    CameraRelease,
    /// Shake the camera with the given world-space amplitude over `duration` seconds.
    /// Intensities of ~0.05 read as small jolts; ~0.2 reads as an explosion;
    /// ~0.5 is heavy/uncanny (good for Hollow events).
    CameraShake { intensity: f32, duration: f32 },
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
    /// Filesystem path the current script was loaded from (e.g.
    /// `"assets/scripts/combine_m0.toml"`). Used by the cutscene editor to save
    /// edits back. `None` when no script is loaded or the script was loaded
    /// from an inline source (e.g., map TOML translated events).
    pub source_path: Option<String>,
    /// Event ids that have already fired.
    pub fired: HashSet<String>,
    /// Pending dialogue messages — shown one at a time.
    pub dialogue_queue: VecDeque<(String, String)>,
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
                self.source_path = Some(path);
            }
            Err(e) => {
                info!("[script] no script at {path}: {e}");
                self.script = None;
                self.source_path = None;
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
    mut camera_target: ResMut<CameraTarget>,
    mut camera_shake: ResMut<CameraShake>,
    units_q: Query<(Entity, &Faction, &UnitTypeId, &UnitPos)>,
    buildings_q: Query<(&Faction, &BuildingTypeId, &BuildingPos)>,
    cinematic_q: Query<&CinematicFraming>,
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
                Action::Dialogue { speaker, text } => {
                    let was_empty = script_state.dialogue_queue.is_empty();
                    script_state.dialogue_queue.push_back((
                        speaker.clone().unwrap_or_else(|| "COMMS".to_string()),
                        text.clone(),
                    ));
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
                Action::CameraFocus { target, framing } => {
                    if let Some(new_target) = resolve_camera_target(
                        target,
                        framing.as_deref(),
                        mission_q.iter().next(),
                        &loaded,
                        &units_q,
                        &buildings_q,
                        &cinematic_q,
                    ) {
                        *camera_target = new_target;
                    }
                }
                Action::CameraRelease => {
                    *camera_target = CameraTarget::Free;
                }
                Action::CameraShake { intensity, duration } => {
                    camera_shake.trigger(*intensity, *duration);
                }
            }
        }

        script_state.fired.insert(event_id);
    }
}

// ── Camera focus resolution ───────────────────────────────────────────────────

/// Pick the framing preset for a focus target. Resolution chain:
///   1. Explicit `script_framing` from the `CameraFocus` action
///   2. `CinematicFraming` component on the followed entity
///   3. The followed unit/building's `cinematic_framing` field in its TOML def
///   4. The faction's `cinematic_framing` default
///   5. `"isometric"` (the gameplay default)
fn pick_framing(
    script_framing: Option<&str>,
    entity: Option<Entity>,
    def_framing: Option<&str>,
    faction: Option<&Faction>,
    loaded: &LoadedFactions,
    cinematic_q: &Query<&CinematicFraming>,
) -> FramingPreset {
    if let Some(name) = script_framing {
        return framing_for(name);
    }
    if let Some(e) = entity {
        if let Ok(c) = cinematic_q.get(e) {
            return framing_for(&c.0);
        }
    }
    if let Some(name) = def_framing {
        if !name.is_empty() { return framing_for(name); }
    }
    if let Some(f) = faction {
        if let Some(fd) = loaded.factions.get(&f.0) {
            if !fd.cinematic_framing.is_empty() {
                return framing_for(&fd.cinematic_framing);
            }
        }
    }
    framing_for("isometric")
}

fn resolve_camera_target(
    focus: &CameraFocusTarget,
    script_framing: Option<&str>,
    mission: Option<&Mission>,
    loaded: &LoadedFactions,
    units_q: &Query<(Entity, &Faction, &UnitTypeId, &UnitPos)>,
    buildings_q: &Query<(&Faction, &BuildingTypeId, &BuildingPos)>,
    cinematic_q: &Query<&CinematicFraming>,
) -> Option<CameraTarget> {
    match focus {
        CameraFocusTarget::HomeBase => {
            let player_faction = mission.map(|m| m.player_faction.clone())?;
            // Average grid position of the player's command_bunker buildings.
            let mut sum = (0i64, 0i64);
            let mut count = 0i64;
            for (f, bt, pos) in buildings_q.iter() {
                if *f == player_faction && bt.id() == "command_bunker" {
                    sum.0 += pos.pos.x as i64;
                    sum.1 += pos.pos.y as i64;
                    count += 1;
                }
            }
            if count == 0 { return None; }
            let cx = (sum.0 as f32) / (count as f32);
            let cy = (sum.1 as f32) / (count as f32);
            // For HomeBase: per-building override from command_bunker def, then faction default.
            let def_fr = loaded.buildings.get("command_bunker")
                .map(|b| b.cinematic_framing.as_str());
            let framing = pick_framing(
                script_framing, None, def_fr, Some(&player_faction), loaded, cinematic_q,
            );
            Some(CameraTarget::LookAt { point: Vec3::new(cx, 0.0, cy), framing })
        }
        CameraFocusTarget::Position { x, y } => {
            let player_faction = mission.map(|m| m.player_faction.clone());
            let framing = pick_framing(
                script_framing, None, None, player_faction.as_ref(), loaded, cinematic_q,
            );
            Some(CameraTarget::LookAt {
                point: Vec3::new(*x as f32, 0.0, *y as f32),
                framing,
            })
        }
        CameraFocusTarget::Unit { faction, unit_type } => {
            let target_faction = parse_faction(faction);
            let target_id = parse_unit_type_id(unit_type);
            let (entity, _, _, _) = units_q
                .iter()
                .find(|(_, f, t, _)| **f == target_faction && t.id() == target_id)?;
            let def_fr = loaded
                .faction_unit(&target_faction.0, &target_id)
                .map(|u| u.cinematic_framing.as_str());
            let framing = pick_framing(
                script_framing, Some(entity), def_fr, Some(&target_faction), loaded, cinematic_q,
            );
            Some(CameraTarget::Follow { entity, framing })
        }
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct MissionScriptPlugin;

impl Plugin for MissionScriptPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptState>();
        app.init_resource::<CameraTarget>();
        app.init_resource::<CameraShake>();
        app.add_systems(Update, script_tick_system);
    }
}
