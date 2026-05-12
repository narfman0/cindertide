//! In-mission cutscene editor (Phase 1: read-only inspector + replay controls).
//!
//! Toggle with F9 while InMission. Opens a right-side egui panel showing:
//!   - Current mission elapsed time
//!   - Playback controls (restart, pause, speed)
//!   - Scrollable event list with fired/selected markers
//!   - Per-event action breakdown for the selected event
//!   - "Jump to event" button (resets fired set & elapsed; events replay from selection)
//!
//! Time-scale uses Bevy's `Time<Virtual>::set_relative_speed` so ALL gameplay
//! reacts (delta-aware systems automatically speed up/slow down). Pause toggles
//! `Time<Virtual>::pause()` for the same reason.
//!
//! Phase 2 (planned): inline-edit dialogue + camera_focus + camera_shake actions,
//! save back to disk via toml::to_string_pretty.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContextPass, EguiContexts, EguiPlugin};

use crate::camera::{CameraShake, CameraTarget};
use crate::mission::{Mission, MissionStatus};
use crate::mission_script::{Action, ScriptState, Trigger};

/// Resource holding the editor's transient UI state.
#[derive(Resource, Default)]
pub struct CutsceneEditorState {
    pub open: bool,
    pub selected_event: usize,
}

pub struct CutsceneEditorPlugin;

impl Plugin for CutsceneEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin {
            enable_multipass_for_primary_context: true,
        });
        app.init_resource::<CutsceneEditorState>();
        app.add_systems(Update, toggle_cutscene_editor);
        app.add_systems(EguiContextPass, cutscene_editor_panel);
    }
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
    // Only meaningful when there's an active mission to replay events against.
    let has_active = missions.iter().any(|m| m.status == MissionStatus::Active);
    if has_active {
        editor.open = !editor.open;
    }
}

/// Render the editor panel. No-op when `editor.open` is false.
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
        .min_width(360.0)
        .default_width(420.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.heading("Cutscene Editor");
            ui.separator();

            // ── Playback controls ────────────────────────────────────────────
            ui.horizontal(|ui| {
                if ui.button("⏮ Restart").on_hover_text("Reset elapsed=0, clear fired set, return camera to Free").clicked() {
                    restart_script(
                        &mut script_state,
                        mission_q.iter_mut().next(),
                        &mut camera_target,
                        &mut camera_shake,
                    );
                }
                let is_paused = virtual_time.is_paused();
                let pause_label = if is_paused { "▶" } else { "⏸" };
                if ui.button(pause_label).on_hover_text("Pause / resume game time").clicked() {
                    if is_paused {
                        virtual_time.unpause();
                    } else {
                        virtual_time.pause();
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Speed:");
                let current = virtual_time.relative_speed();
                for &s in &[0.25_f32, 0.5, 1.0, 2.0, 4.0] {
                    let label = format!("{}x", s);
                    if ui.selectable_label((current - s).abs() < 0.01, label).clicked() {
                        virtual_time.set_relative_speed(s);
                    }
                }
            });
            ui.label(format!("t = {:.1}s", elapsed));
            ui.separator();

            // ── Event list ───────────────────────────────────────────────────
            let Some(script) = script_state.script.clone() else {
                ui.colored_label(egui::Color32::YELLOW, "No script loaded for this mission.");
                return;
            };

            ui.label(format!("Events ({}):", script.events.len()));
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for (i, ev) in script.events.iter().enumerate() {
                        let fired = script_state.fired.contains(&ev.id);
                        let prefix = if fired { "✓" } else { "·" };
                        let trigger_str = format_trigger(&ev.trigger);
                        let label = format!("{}  {:<22} {}", prefix, truncate(&ev.id, 22), trigger_str);
                        let selected = editor.selected_event == i;
                        if ui.selectable_label(selected, egui::RichText::new(label).monospace()).clicked() {
                            editor.selected_event = i;
                        }
                    }
                });

            ui.separator();

            // ── Event detail ─────────────────────────────────────────────────
            let Some(ev) = script.events.get(editor.selected_event) else {
                ui.label("(select an event)");
                return;
            };

            ui.heading(&ev.id);
            ui.label(format!("Trigger: {}", format_trigger(&ev.trigger)));

            // Jump button — only valid for time-triggered events.
            if let Trigger::Time { seconds } = &ev.trigger {
                if ui
                    .button(format!("⏭ Jump to '{}' (t={:.1}s)", ev.id, seconds))
                    .on_hover_text("Reset elapsed, mark earlier events as fired, clear later events")
                    .clicked()
                {
                    let target_id = ev.id.clone();
                    let target_time = *seconds;
                    let earlier_ids: Vec<String> = script.events.iter()
                        .filter(|e| match &e.trigger {
                            Trigger::Time { seconds: s } => *s < target_time,
                            _ => false,
                        })
                        .map(|e| e.id.clone())
                        .collect();
                    jump_to_event(
                        &mut script_state,
                        mission_q.iter_mut().next(),
                        &mut camera_target,
                        &mut camera_shake,
                        target_time,
                        target_id,
                        earlier_ids,
                    );
                }
            } else {
                ui.colored_label(
                    egui::Color32::from_rgb(180, 180, 180),
                    "(beat-triggered — fires on game event, not time)",
                );
            }

            ui.separator();
            ui.label(format!("Actions ({}):", ev.actions.len()));
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .id_source("action_scroll")
                .show(ui, |ui| {
                    for (j, a) in ev.actions.iter().enumerate() {
                        ui.label(egui::RichText::new(format!("[{}] {}", j + 1, summarize_action(a))).monospace());
                    }
                });

            ui.separator();
            ui.small("F9 to close · Phase 2 will add inline editing + save-to-disk");
        });
}

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

fn summarize_action(a: &Action) -> String {
    match a {
        Action::Dialogue { speaker, text } => {
            let speaker = speaker.as_deref().unwrap_or("?");
            format!("dialogue [{}] {}", speaker, truncate(text, 56))
        }
        Action::SpawnUnits { faction, unit_type, count, x, y } => {
            format!("spawn {} × {} {} @ ({},{})", count, faction, unit_type, x, y)
        }
        Action::Objective { text } => format!("objective: {}", truncate(text, 56)),
        Action::ChangeObjective { text } => format!("change_objective: {}", truncate(text, 56)),
        Action::WinMission => "win_mission".to_string(),
        Action::LoseMission => "lose_mission".to_string(),
        Action::CameraFocus { target, framing } => {
            let t = format!("{:?}", target);
            let f = framing.as_deref().unwrap_or("(auto)");
            format!("camera_focus → {} framing={}", t, f)
        }
        Action::CameraRelease => "camera_release".to_string(),
        Action::CameraShake { intensity, duration } => {
            format!("camera_shake i={:.2} d={:.1}s", intensity, duration)
        }
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
    camera_shake.intensity = 0.0;
    camera_shake.remaining = 0.0;
    camera_shake.total = 0.0;
    info!("[cutscene-editor] restarted script");
}

#[allow(clippy::too_many_arguments)]
fn jump_to_event(
    script_state: &mut ScriptState,
    mission: Option<Mut<Mission>>,
    camera_target: &mut CameraTarget,
    camera_shake: &mut CameraShake,
    target_time: f32,
    target_id: String,
    earlier_ids: Vec<String>,
) {
    // Mark all earlier time-triggered events as fired so they don't replay,
    // then nudge elapsed just before the target so it fires next tick.
    script_state.fired.clear();
    for id in earlier_ids {
        script_state.fired.insert(id);
    }
    // Ensure the target ID is NOT in fired (in case it was earlier).
    script_state.fired.remove(&target_id);
    script_state.dialogue_queue.clear();
    script_state.dialogue_timer = 0.0;
    if let Some(mut m) = mission {
        // Subtract a small epsilon so the next tick crosses the trigger.
        m.elapsed = (target_time - 0.05).max(0.0);
    }
    *camera_target = CameraTarget::Free;
    camera_shake.intensity = 0.0;
    camera_shake.remaining = 0.0;
    camera_shake.total = 0.0;
    info!("[cutscene-editor] jumped to '{}' @ t={:.1}s", target_id, target_time);
}
