//! In-mission cutscene editor.
//!
//! Toggle with F9 while InMission. Opens a right-side egui panel.
//!
//! Phase 1 (replay controls): event list with fired/selected markers,
//! per-event action breakdown, Restart / Pause / Speed / Jump-to-event.
//!
//! Phase 2 (editing): inline-edit per-action fields (dialogue text, camera
//! framing dropdown, camera_shake sliders, spawn coords, etc.), add/remove
//! events and actions, Save back to disk via `toml::to_string_pretty`. The
//! `Save as edited copy` button writes to `<name>.edited.toml` so a
//! hand-authored script with comments isn't clobbered.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContextPass, EguiContexts, EguiPlugin};
use std::path::PathBuf;
use std::sync::Arc;

use crate::camera::{CameraFocusTarget, CameraShake, CameraTarget};
use crate::mission::{Mission, MissionStatus};
use crate::mission_script::{Action, MissionScript, ScriptEvent, ScriptState, Trigger};

/// Font files prefetched from the asset server. Loaded by `load_egui_fonts`
/// at startup and registered with bevy_egui for use throughout the editor UI.
pub const FONT_PATHS: &[&str] = &[
    "kenney_aio/Other/Fonts/Kenney Mini Square Mono.ttf",
    "kenney_aio/Other/Fonts/Kenney Future Narrow.ttf",
];

/// Camera framing preset names — keep in sync with `camera::framing_for`.
const FRAMING_PRESETS: &[&str] = &[
    "isometric",
    "low_angle_hero",
    "close_up",
    "over_shoulder",
    "off_kilter",
    "wide_establishing",
];

/// All action variant ids surfaced by the "+ Add action" menu.
const ACTION_TYPES: &[&str] = &[
    "dialogue",
    "spawn_units",
    "objective",
    "change_objective",
    "win_mission",
    "lose_mission",
    "camera_focus",
    "camera_release",
    "camera_shake",
];

/// Resource holding the on-disk root where prefetched assets live.
/// Populated at startup by the client's main fn; needed by `load_egui_fonts`
/// to locate the cached TTF files.
#[derive(Resource, Clone)]
pub struct AssetCacheRoot(pub PathBuf);

/// Editor UI state.
#[derive(Resource, Default)]
pub struct CutsceneEditorState {
    pub open: bool,
    pub selected_event: usize,
    pub status_msg: Option<(String, bool)>, // (text, is_error)
}

pub struct CutsceneEditorPlugin;

impl Plugin for CutsceneEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin {
            enable_multipass_for_primary_context: true,
        });
        app.init_resource::<CutsceneEditorState>();
        app.add_systems(EguiContextPass, load_egui_fonts);
        app.add_systems(Update, toggle_cutscene_editor);
        app.add_systems(EguiContextPass, cutscene_editor_panel.after(load_egui_fonts));
    }
}

/// Install Kenney fonts into egui on first frame.
fn load_egui_fonts(
    mut contexts: EguiContexts,
    cache: Option<Res<AssetCacheRoot>>,
    mut loaded: Local<bool>,
) {
    if *loaded {
        return;
    }
    let Some(cache) = cache else { return };
    let mono_path = cache.0.join(FONT_PATHS[0]);
    let display_path = cache.0.join(FONT_PATHS[1]);
    let Ok(mono_bytes) = std::fs::read(&mono_path) else {
        warn!(
            "Egui font missing: {} (falling back to default)",
            mono_path.display()
        );
        *loaded = true;
        return;
    };
    let display_bytes = std::fs::read(&display_path).ok();

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "kenney_mono".to_string(),
        Arc::new(egui::FontData::from_owned(mono_bytes)),
    );
    if let Some(b) = display_bytes {
        fonts.font_data.insert(
            "kenney_future".to_string(),
            Arc::new(egui::FontData::from_owned(b)),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "kenney_future".to_string());
    } else {
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "kenney_mono".to_string());
    }
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "kenney_mono".to_string());

    let ctx = contexts.ctx_mut();
    ctx.set_fonts(fonts);
    info!("[cutscene-editor] installed Kenney fonts into egui");
    *loaded = true;
}

/// F9 toggles the editor while a mission is active.
fn toggle_cutscene_editor(
    keys: Res<ButtonInput<KeyCode>>,
    mut editor: ResMut<CutsceneEditorState>,
    missions: Query<&Mission>,
) {
    if !keys.just_pressed(KeyCode::F9) {
        return;
    }
    let has_active = missions.iter().any(|m| m.status == MissionStatus::Active);
    if has_active {
        editor.open = !editor.open;
    }
}

#[allow(clippy::too_many_arguments)]
fn cutscene_editor_panel(
    mut contexts: EguiContexts,
    mut editor: ResMut<CutsceneEditorState>,
    mut script_state: ResMut<ScriptState>,
    mut mission_q: Query<&mut Mission>,
    mut camera_target: ResMut<CameraTarget>,
    mut camera_shake: ResMut<CameraShake>,
    mut virtual_time: ResMut<Time<Virtual>>,
) {
    if !editor.open {
        return;
    }
    let ctx = contexts.ctx_mut();
    let elapsed = mission_q.iter().next().map(|m| m.elapsed).unwrap_or(0.0);

    egui::SidePanel::right("cutscene_editor_panel")
        .min_width(440.0)
        .default_width(520.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.heading("Cutscene Editor");

            // ── File row ─────────────────────────────────────────────────
            if let Some(path) = &script_state.source_path {
                ui.label(egui::RichText::new(path.clone()).monospace().small());
            } else {
                ui.colored_label(egui::Color32::from_rgb(180, 180, 180), "(no source file)");
            }
            // We need a mutable handle to ScriptState for both load & save; defer
            // the actual button actions to after we've finished borrowing fields below.
            let mut save_request: Option<bool> = None; // Some(as_edited)
            let mut reload_request = false;
            ui.horizontal(|ui| {
                let can_save = script_state.source_path.is_some() && script_state.script.is_some();
                if ui.add_enabled(can_save, egui::Button::new("Save")).clicked() {
                    save_request = Some(false);
                }
                if ui.add_enabled(can_save, egui::Button::new("Save as edited")).clicked() {
                    save_request = Some(true);
                }
                if ui.add_enabled(script_state.source_path.is_some(), egui::Button::new("Reload")).clicked() {
                    reload_request = true;
                }
            });
            if let Some((msg, is_err)) = &editor.status_msg {
                let color = if *is_err { egui::Color32::from_rgb(220, 80, 80) } else { egui::Color32::from_rgb(120, 200, 120) };
                ui.colored_label(color, msg);
            }
            ui.separator();

            // ── Playback ─────────────────────────────────────────────────
            ui.horizontal(|ui| {
                if ui.button("[<<] Restart").on_hover_text("Reset elapsed=0, clear fired").clicked() {
                    restart_script(&mut script_state, mission_q.iter_mut().next(), &mut camera_target, &mut camera_shake);
                }
                let paused = virtual_time.is_paused();
                if ui.button(if paused { "[>] Play" } else { "[||] Pause" }).clicked() {
                    if paused { virtual_time.unpause() } else { virtual_time.pause() }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Speed:");
                let current = virtual_time.relative_speed();
                for &s in &[0.25_f32, 0.5, 1.0, 2.0, 4.0] {
                    if ui.selectable_label((current - s).abs() < 0.01, format!("{}x", s)).clicked() {
                        virtual_time.set_relative_speed(s);
                    }
                }
            });
            ui.label(format!("t = {:.1}s", elapsed));
            ui.separator();

            // ── Splittable borrow: fired (read) + script (write) ─────────
            let state = &mut *script_state;
            let fired_ref = &state.fired;
            let Some(script) = state.script.as_mut() else {
                ui.colored_label(egui::Color32::YELLOW, "No script loaded for this mission.");
                return;
            };

            // ── Event list ───────────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.label(format!("Events ({}):", script.events.len()));
                if ui.small_button("+ Event").clicked() {
                    let id = format!("event_{}", script.events.len() + 1);
                    script.events.push(default_event(id));
                    editor.selected_event = script.events.len() - 1;
                }
            });

            let mut event_delete: Option<usize> = None;
            let mut event_move: Option<(usize, isize)> = None; // (index, direction +/- 1)
            let event_count = script.events.len();
            egui::ScrollArea::vertical()
                .id_salt("event_scroll")
                .max_height(180.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (i, ev) in script.events.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let fired_marker = if fired_ref.contains(&ev.id) { "[x]" } else { "[ ]" };
                            let trigger_str = format_trigger(&ev.trigger);
                            let label = format!("{}  {:<22} {}", fired_marker, truncate(&ev.id, 22), trigger_str);
                            if ui.selectable_label(editor.selected_event == i, egui::RichText::new(&label).monospace()).clicked() {
                                editor.selected_event = i;
                            }
                            if ui.add_enabled(i > 0, egui::Button::new("^").small())
                                .on_hover_text("Move up").clicked()
                            {
                                event_move = Some((i, -1));
                            }
                            if ui.add_enabled(i + 1 < event_count, egui::Button::new("v").small())
                                .on_hover_text("Move down").clicked()
                            {
                                event_move = Some((i, 1));
                            }
                            if ui.small_button("DEL").on_hover_text("Delete event").clicked() {
                                event_delete = Some(i);
                            }
                        });
                    }
                });
            if let Some((i, dir)) = event_move {
                let j = (i as isize + dir) as usize;
                script.events.swap(i, j);
                if editor.selected_event == i {
                    editor.selected_event = j;
                } else if editor.selected_event == j {
                    editor.selected_event = i;
                }
            }
            if let Some(i) = event_delete {
                script.events.remove(i);
                if editor.selected_event >= script.events.len() && !script.events.is_empty() {
                    editor.selected_event = script.events.len() - 1;
                }
            }

            ui.separator();

            // ── Selected event detail ────────────────────────────────────
            let Some(ev) = script.events.get_mut(editor.selected_event) else {
                ui.label("(no event selected)");
                return;
            };

            ui.horizontal(|ui| {
                ui.label("ID:");
                ui.text_edit_singleline(&mut ev.id);
            });
            edit_trigger(ui, &mut ev.trigger);

            // Jump for time-triggered events.
            if let Trigger::Time { seconds } = &ev.trigger {
                let seconds = *seconds;
                let ev_id = ev.id.clone();
                if ui.button(format!("[>>|] Jump to '{}' (t={:.1}s)", ev_id, seconds)).clicked() {
                    let earlier_ids: Vec<String> = script.events.iter()
                        .filter_map(|e| match &e.trigger {
                            Trigger::Time { seconds: s } if *s < seconds => Some(e.id.clone()),
                            _ => None,
                        })
                        .collect();
                    let target_id = ev_id;
                    drop_borrows_and_jump(
                        state,
                        mission_q.iter_mut().next(),
                        &mut camera_target,
                        &mut camera_shake,
                        seconds,
                        target_id,
                        earlier_ids,
                    );
                    return;
                }
            }
            ui.separator();

            // ── Actions ──────────────────────────────────────────────────
            // Re-borrow after potential jump path returned above.
            let Some(ev) = script.events.get_mut(editor.selected_event) else { return };

            ui.horizontal(|ui| {
                ui.label(format!("Actions ({}):", ev.actions.len()));
                add_action_menu(ui, &mut ev.actions);
            });

            let mut action_delete: Option<usize> = None;
            let mut action_move: Option<(usize, isize)> = None;
            let action_count = ev.actions.len();
            egui::ScrollArea::vertical()
                .id_salt("action_scroll")
                .max_height(360.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (j, a) in ev.actions.iter_mut().enumerate() {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(format!("[{}]", j + 1)).strong());
                                ui.label(action_type_name(a));
                                if ui.add_enabled(j > 0, egui::Button::new("^").small())
                                    .on_hover_text("Move up").clicked()
                                {
                                    action_move = Some((j, -1));
                                }
                                if ui.add_enabled(j + 1 < action_count, egui::Button::new("v").small())
                                    .on_hover_text("Move down").clicked()
                                {
                                    action_move = Some((j, 1));
                                }
                                if ui.small_button("DEL").on_hover_text("Delete action").clicked() {
                                    action_delete = Some(j);
                                }
                            });
                            edit_action(ui, a);
                        });
                    }
                });
            if let Some((j, dir)) = action_move {
                let k = (j as isize + dir) as usize;
                ev.actions.swap(j, k);
            }
            if let Some(j) = action_delete {
                ev.actions.remove(j);
            }

            ui.separator();
            ui.small("F9 to close | edits are live | Save writes to disk");

            // ── Deferred save / reload ───────────────────────────────────
            if let Some(as_edited) = save_request {
                let res = save_to_disk(state, as_edited);
                editor.status_msg = Some(res);
            }
            if reload_request {
                if let Some(path) = state.source_path.clone() {
                    let name = path_to_script_name(&path).unwrap_or_default();
                    state.load_script(&name);
                    editor.status_msg = Some((format!("Reloaded {}", path), false));
                }
            }
        });
}

// ── Per-variant editors ──────────────────────────────────────────────────────

fn edit_trigger(ui: &mut egui::Ui, t: &mut Trigger) {
    ui.horizontal(|ui| {
        ui.label("Trigger:");
        let mut current = match t {
            Trigger::Time { .. } => "time",
            Trigger::Condition { .. } => "beat",
        };
        let prev = current;
        egui::ComboBox::from_id_salt("trigger_kind")
            .selected_text(current)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut current, "time", "time");
                ui.selectable_value(&mut current, "beat", "beat");
            });
        if current != prev {
            *t = match current {
                "time" => Trigger::Time { seconds: 0.0 },
                _ => Trigger::Condition { condition: "beat".to_string(), beat_id: Some("LastStand".to_string()) },
            };
        }
    });
    match t {
        Trigger::Time { seconds } => {
            ui.horizontal(|ui| {
                ui.label("seconds:");
                ui.add(egui::DragValue::new(seconds).speed(1.0).range(0.0..=3600.0));
            });
        }
        Trigger::Condition { condition: _, beat_id } => {
            ui.horizontal(|ui| {
                ui.label("beat:");
                let mut beat = beat_id.clone().unwrap_or_default();
                if ui.text_edit_singleline(&mut beat).changed() {
                    *beat_id = if beat.is_empty() { None } else { Some(beat) };
                }
            });
        }
    }
}

fn edit_action(ui: &mut egui::Ui, a: &mut Action) {
    match a {
        Action::Dialogue { speaker, text } => {
            ui.horizontal(|ui| {
                ui.label("speaker:");
                let mut s = speaker.clone().unwrap_or_default();
                if ui.text_edit_singleline(&mut s).changed() {
                    *speaker = if s.is_empty() { None } else { Some(s) };
                }
            });
            ui.label("text:");
            ui.add(egui::TextEdit::multiline(text).desired_rows(2).desired_width(f32::INFINITY));
        }
        Action::SpawnUnits { faction, unit_type, count, x, y } => {
            ui.horizontal(|ui| {
                ui.label("faction:");
                ui.text_edit_singleline(faction);
                ui.label("type:");
                ui.text_edit_singleline(unit_type);
            });
            ui.horizontal(|ui| {
                ui.label("count:");
                ui.add(egui::DragValue::new(count).speed(1.0).range(1..=99));
                ui.label("x:");
                ui.add(egui::DragValue::new(x).speed(1.0));
                ui.label("y:");
                ui.add(egui::DragValue::new(y).speed(1.0));
            });
        }
        Action::Objective { text } | Action::ChangeObjective { text } => {
            ui.add(egui::TextEdit::multiline(text).desired_rows(2).desired_width(f32::INFINITY));
        }
        Action::WinMission | Action::LoseMission | Action::CameraRelease => {
            ui.label(egui::RichText::new("(no fields)").italics().small());
        }
        Action::CameraFocus { target, framing } => {
            edit_focus_target(ui, target);
            ui.horizontal(|ui| {
                ui.label("framing:");
                let mut current = framing.clone().unwrap_or_else(|| "(auto)".to_string());
                let prev = current.clone();
                egui::ComboBox::from_id_salt(format!("framing_{:p}", framing as *const _))
                    .selected_text(&current)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut current, "(auto)".to_string(), "(auto — use unit/faction default)");
                        for preset in FRAMING_PRESETS {
                            ui.selectable_value(&mut current, preset.to_string(), *preset);
                        }
                    });
                if current != prev {
                    *framing = if current == "(auto)" { None } else { Some(current) };
                }
            });
        }
        Action::CameraShake { intensity, duration } => {
            ui.add(egui::Slider::new(intensity, 0.0..=1.0).text("intensity"));
            ui.add(egui::Slider::new(duration, 0.0..=5.0).text("duration (s)"));
        }
    }
}

fn edit_focus_target(ui: &mut egui::Ui, t: &mut CameraFocusTarget) {
    let mut kind = match t {
        CameraFocusTarget::HomeBase => "home_base",
        CameraFocusTarget::Position { .. } => "position",
        CameraFocusTarget::Unit { .. } => "unit",
    };
    let prev = kind;
    ui.horizontal(|ui| {
        ui.label("target:");
        egui::ComboBox::from_id_salt(format!("focus_kind_{:p}", t as *const _))
            .selected_text(kind)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut kind, "home_base", "home_base");
                ui.selectable_value(&mut kind, "position", "position");
                ui.selectable_value(&mut kind, "unit", "unit");
            });
    });
    if kind != prev {
        *t = match kind {
            "home_base" => CameraFocusTarget::HomeBase,
            "position" => CameraFocusTarget::Position { x: 64, y: 40 },
            _ => CameraFocusTarget::Unit { faction: "combine".to_string(), unit_type: "riflemen".to_string() },
        };
    }
    match t {
        CameraFocusTarget::HomeBase => {}
        CameraFocusTarget::Position { x, y } => {
            ui.horizontal(|ui| {
                ui.label("x:");
                ui.add(egui::DragValue::new(x).speed(1.0));
                ui.label("y:");
                ui.add(egui::DragValue::new(y).speed(1.0));
            });
        }
        CameraFocusTarget::Unit { faction, unit_type } => {
            ui.horizontal(|ui| {
                ui.label("faction:");
                ui.text_edit_singleline(faction);
                ui.label("type:");
                ui.text_edit_singleline(unit_type);
            });
        }
    }
}

fn add_action_menu(ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    egui::ComboBox::from_id_salt("add_action")
        .selected_text("+ Add action")
        .show_ui(ui, |ui| {
            for kind in ACTION_TYPES {
                if ui.button(*kind).clicked() {
                    actions.push(default_action(kind));
                    ui.close_menu();
                }
            }
        });
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn format_trigger(t: &Trigger) -> String {
    match t {
        Trigger::Time { seconds } => format!("t={:.1}s", seconds),
        Trigger::Condition { condition, beat_id } => {
            if let Some(b) = beat_id {
                format!("{}:{}", condition, b)
            } else {
                condition.clone()
            }
        }
    }
}

fn action_type_name(a: &Action) -> &'static str {
    match a {
        Action::Dialogue { .. } => "dialogue",
        Action::SpawnUnits { .. } => "spawn_units",
        Action::Objective { .. } => "objective",
        Action::ChangeObjective { .. } => "change_objective",
        Action::WinMission => "win_mission",
        Action::LoseMission => "lose_mission",
        Action::CameraFocus { .. } => "camera_focus",
        Action::CameraRelease => "camera_release",
        Action::CameraShake { .. } => "camera_shake",
    }
}

fn default_action(kind: &str) -> Action {
    match kind {
        "dialogue" => Action::Dialogue { speaker: Some("Field Comms".to_string()), text: String::new() },
        "spawn_units" => Action::SpawnUnits {
            faction: "Combine".to_string(),
            unit_type: "Riflemen".to_string(),
            count: 1,
            x: 0,
            y: 0,
        },
        "objective" => Action::Objective { text: String::new() },
        "change_objective" => Action::ChangeObjective { text: String::new() },
        "win_mission" => Action::WinMission,
        "lose_mission" => Action::LoseMission,
        "camera_focus" => Action::CameraFocus { target: CameraFocusTarget::HomeBase, framing: None },
        "camera_release" => Action::CameraRelease,
        "camera_shake" => Action::CameraShake { intensity: 0.2, duration: 0.8 },
        _ => Action::Dialogue { speaker: None, text: String::new() },
    }
}

fn default_event(id: String) -> ScriptEvent {
    ScriptEvent {
        id,
        trigger: Trigger::Time { seconds: 0.0 },
        actions: Vec::new(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn path_to_script_name(path: &str) -> Option<String> {
    // "assets/scripts/combine_m0.toml" → "combine_m0"
    let stem = std::path::Path::new(path).file_stem()?.to_str()?;
    Some(stem.trim_end_matches(".edited").to_string())
}

/// Save the in-memory script to disk. `as_edited=true` redirects to
/// `<stem>.edited.toml` to preserve hand-authored sources with comments.
fn save_to_disk(state: &ScriptState, as_edited: bool) -> (String, bool) {
    let Some(path) = state.source_path.as_ref() else {
        return ("No source_path".to_string(), true);
    };
    let Some(script) = state.script.as_ref() else {
        return ("No script loaded".to_string(), true);
    };
    let target = if as_edited {
        path.strip_suffix(".toml").map(|p| format!("{}.edited.toml", p))
            .unwrap_or_else(|| format!("{}.edited.toml", path))
    } else {
        path.clone()
    };
    let text = match toml::to_string_pretty(&MissionScript { events: script.events.clone() }) {
        Ok(t) => t,
        Err(e) => return (format!("serialize: {}", e), true),
    };
    match std::fs::write(&target, text) {
        Ok(_) => (format!("Saved {}", target), false),
        Err(e) => (format!("write {}: {}", target, e), true),
    }
}

// ── Replay helpers ───────────────────────────────────────────────────────────

fn restart_script(
    script_state: &mut ScriptState,
    mission: Option<Mut<Mission>>,
    camera_target: &mut CameraTarget,
    camera_shake: &mut CameraShake,
) {
    script_state.fired.clear();
    script_state.dialogue_queue.clear();
    script_state.dialogue_timer = 0.0;
    script_state.current_objective = None;
    if let Some(mut m) = mission {
        m.elapsed = 0.0;
    }
    *camera_target = CameraTarget::Free;
    *camera_shake = CameraShake::default();
    info!("[cutscene-editor] restarted script");
}

fn drop_borrows_and_jump(
    script_state: &mut ScriptState,
    mission: Option<Mut<Mission>>,
    camera_target: &mut CameraTarget,
    camera_shake: &mut CameraShake,
    target_time: f32,
    target_id: String,
    earlier_ids: Vec<String>,
) {
    script_state.fired.clear();
    for id in earlier_ids {
        script_state.fired.insert(id);
    }
    script_state.fired.remove(&target_id);
    script_state.dialogue_queue.clear();
    script_state.dialogue_timer = 0.0;
    if let Some(mut m) = mission {
        m.elapsed = (target_time - 0.05).max(0.0);
    }
    *camera_target = CameraTarget::Free;
    *camera_shake = CameraShake::default();
    info!("[cutscene-editor] jumped to '{}' @ t={:.1}s", target_id, target_time);
}
