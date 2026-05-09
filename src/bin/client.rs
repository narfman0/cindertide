use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::render::camera::ScalingMode;
use cindertide::map::{Faction, GridPos, Tile};
use cindertide::units::{UnitPos, UnitType};
use cindertide::buildings::{BuildingPos, BuildingType};
use cindertide::campaign::{CampaignRun, PlayableFaction, GlobalProgress, apply_mission_outcome, next_mission_type};
use cindertide::game::{ActiveRun, GameState};
use cindertide::mission::{Mission, MissionStatus};
use cindertide::mapgen::MissionType;
use cindertide::narrative::NarrativeData;
use cindertide::resources::FactionBundle;
use cindertide::combat::{PlayerAttackOrder, AttackMoveOrder, HoldPosition};
use cindertide::units::{MoveTarget, MoveProgress, UnitKind};
use cindertide::{
    map::MapPlugin,
    units::UnitPlugin,
    combat::CombatPlugin,
    resources::ResourcesPlugin,
    control::ControlPlugin,
    buildings::BuildingsPlugin,
    production::ProductionPlugin,
    heroes::HeroPlugin,
    tech::TechPlugin,
    unit_ai::UnitAiPlugin,
    repair::RepairPlugin,
    ai::AiPlugin,
    mission::MissionPlugin,
    campaign::CampaignPlugin,
    beats::BeatsPlugin,
    hollow::HollowPlugin,
    save::SavePlugin,
    game::GamePlugin,
};
use std::collections::HashMap;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Cindertide".into(),
                resolution: (1280.0, 720.0).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((MapPlugin, UnitPlugin, CombatPlugin, ResourcesPlugin, ControlPlugin))
        .add_plugins((BuildingsPlugin, ProductionPlugin, HeroPlugin, TechPlugin, UnitAiPlugin))
        .add_plugins((RepairPlugin, AiPlugin, MissionPlugin, CampaignPlugin, BeatsPlugin))
        .add_plugins((HollowPlugin, SavePlugin, GamePlugin))
        .init_resource::<VisualEntities>()
        .init_resource::<SelectedUnits>()
        .init_resource::<DragState>()
        .init_resource::<AttackMoveMode>()
        .init_resource::<Paused>()
        .init_resource::<ClientScreen>()
        .add_systems(Startup, setup_scene)
        .add_systems(Startup, setup_ui)
        .add_systems(Startup, load_narrative)
        .add_systems(Update, render_tiles)
        .add_systems(Update, spawn_unit_visuals)
        .add_systems(Update, sync_unit_positions)
        .add_systems(Update, spawn_building_visuals)
        .add_systems(Update, camera_pan_zoom)
        .add_systems(Update, edge_scroll)
        .add_systems(Update, handle_mouse_input)
        .add_systems(Update, sync_selection_rings)
        .add_systems(Update, update_drag_rect)
        .add_systems(Update, handle_keyboard_commands)
        .add_systems(Update, update_paused_overlay)
        .add_systems(Update, handle_ui_input)
        .add_systems(Update, update_screen_overlay)
        .add_systems(Update, poll_mission_end)
        .run();
}

// ── ClientScreen resource ────────────────────────────────────────────────────

#[derive(Resource, Debug, Clone, PartialEq)]
enum ClientScreen {
    Title,
    FactionPicker { selected: usize },
    Briefing { title: String, briefing: String },
    InMission,
    Debrief { title: String, text: String, won: bool },
    GameOver { won: bool, handler_unlocked: bool },
}

impl Default for ClientScreen {
    fn default() -> Self {
        ClientScreen::Title
    }
}

// ── Components ──────────────────────────────────────────────────────────────

#[derive(Component)]
struct IsometricCamera {
    pan_speed: f32,
    zoom_speed: f32,
}

#[derive(Component)]
struct RenderedTile;

#[derive(Component)]
struct SelectionRing {
    unit_entity: Entity,
}

#[derive(Component)]
struct DragRectUi;

#[derive(Component)]
struct PausedOverlay;

/// Marker for the full-screen campaign overlay panel root node.
#[derive(Component)]
struct ScreenOverlay;

/// Text node that shows the main title in the overlay.
#[derive(Component)]
struct OverlayTitleText;

/// Text node that shows the body/sub-text in the overlay.
#[derive(Component)]
struct OverlayBodyText;

/// Text node for the secondary hint line ("Press Enter…")
#[derive(Component)]
struct OverlayHintText;

// ── Resources ────────────────────────────────────────────────────────────────

#[derive(Resource, Default)]
struct VisualEntities {
    units: HashMap<Entity, Entity>,
    buildings: HashMap<Entity, Entity>,
}

#[derive(Resource, Default)]
struct SelectedUnits {
    entities: Vec<Entity>,
}

#[derive(Resource)]
struct PlayerFaction(Faction);

/// Tracks left-mouse drag state for box selection.
#[derive(Resource, Default)]
struct DragState {
    /// Screen position where drag started.
    start: Option<Vec2>,
    /// Current cursor position while dragging.
    current: Vec2,
}

/// When true, right-click issues an attack-move order instead of a regular move.
#[derive(Resource, Default)]
struct AttackMoveMode(bool);

/// When true, game logic is paused (uses Bevy's virtual time pause).
#[derive(Resource, Default)]
struct Paused(bool);

// ── Helper functions ──────────────────────────────────────────────────────────

fn grid_to_world(x: i32, y: i32) -> Vec3 {
    Vec3::new(x as f32, 0.0, y as f32)
}

fn screen_to_grid(
    cursor_pos: Vec2,
    camera: &Camera,
    camera_transform: &GlobalTransform,
) -> Option<(i32, i32)> {
    let ray = camera.viewport_to_world(camera_transform, cursor_pos).ok()?;
    if ray.direction.y.abs() < 0.0001 {
        return None;
    }
    let t = -ray.origin.y / ray.direction.y;
    if t <= 0.0 {
        return None;
    }
    let hit = ray.origin + ray.direction * t;
    Some((hit.x.round() as i32, hit.z.round() as i32))
}

/// Returns the (min, size) in screen-space for the drag rect, or None if not dragging.
fn drag_rect(start: Vec2, current: Vec2) -> (Vec2, Vec2) {
    let min = Vec2::new(start.x.min(current.x), start.y.min(current.y));
    let max = Vec2::new(start.x.max(current.x), start.y.max(current.y));
    (min, max - min)
}

fn faction_name(f: PlayableFaction) -> &'static str {
    match f {
        PlayableFaction::Combine => "Combine",
        PlayableFaction::Ironborn => "Ironborn",
        PlayableFaction::Handler => "The Architect",
    }
}

fn faction_narrative_key(f: PlayableFaction) -> &'static str {
    match f {
        PlayableFaction::Combine => "combine",
        PlayableFaction::Ironborn => "ironborn",
        PlayableFaction::Handler => "architect",
    }
}

fn map_faction(f: PlayableFaction) -> Faction {
    match f {
        PlayableFaction::Combine => Faction::Combine,
        PlayableFaction::Ironborn => Faction::Ironborn,
        PlayableFaction::Handler => Faction::Combine, // Handler uses Combine visuals
    }
}

// ── Startup systems ───────────────────────────────────────────────────────────

fn load_narrative(mut commands: Commands) {
    let narrative = cindertide::narrative::NarrativeData::load("assets/narrative.toml")
        .unwrap_or_else(|e| {
            eprintln!("Warning: could not load assets/narrative.toml: {e}");
            cindertide::narrative::NarrativeData {
                factions: Default::default(),
                finales: cindertide::narrative::Finales {
                    combine_first: String::new(),
                    ironborn_first: String::new(),
                },
            }
        });
    commands.insert_resource(narrative);
}

fn setup_scene(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        Projection::Orthographic(OrthographicProjection {
            scale: 14.0,
            scaling_mode: ScalingMode::FixedVertical { viewport_height: 1.0 },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(32.0, 30.0, 32.0).looking_at(Vec3::new(20.0, 0.0, 12.0), Vec3::Y),
        IsometricCamera { pan_speed: 20.0, zoom_speed: 2.0 },
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 15000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.4, 0.0)),
    ));
}

fn setup_ui(mut commands: Commands) {
    // Root UI node
    commands.spawn(Node {
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        position_type: PositionType::Absolute,
        ..default()
    }).with_children(|parent| {
        // Drag selection rectangle (hidden by default via zero size)
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(0.0),
                height: Val::Px(0.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor(Color::srgba(1.0, 1.0, 0.3, 0.8)),
            BackgroundColor(Color::srgba(0.9, 0.9, 0.2, 0.08)),
            DragRectUi,
        ));

        // PAUSED overlay (top-center)
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(40.0),
                top: Val::Px(12.0),
                ..default()
            },
            Visibility::Hidden,
            PausedOverlay,
        )).with_children(|p| {
            p.spawn((
                Text::new("PAUSED"),
                TextColor(Color::srgb(1.0, 0.9, 0.1)),
                TextFont {
                    font_size: 36.0,
                    ..default()
                },
            ));
        });

        // Full-screen campaign overlay
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
            Visibility::Visible,
            ScreenOverlay,
        )).with_children(|p| {
            // Title text
            p.spawn((
                Text::new("CINDERTIDE"),
                TextColor(Color::srgb(1.0, 0.85, 0.2)),
                TextFont { font_size: 72.0, ..default() },
                OverlayTitleText,
            ));

            // Body text
            p.spawn((
                Node {
                    margin: UiRect::top(Val::Px(24.0)),
                    max_width: Val::Px(800.0),
                    ..default()
                },
                Text::new("a dieselpunk RTS"),
                TextColor(Color::srgb(0.85, 0.85, 0.85)),
                TextFont { font_size: 26.0, ..default() },
                OverlayBodyText,
            ));

            // Hint text
            p.spawn((
                Node {
                    margin: UiRect::top(Val::Px(48.0)),
                    ..default()
                },
                Text::new("Press Enter to begin"),
                TextColor(Color::srgb(0.65, 0.65, 0.65)),
                TextFont { font_size: 20.0, ..default() },
                OverlayHintText,
            ));
        });
    });
}

// ── Screen overlay update ─────────────────────────────────────────────────────

fn update_screen_overlay(
    screen: Res<ClientScreen>,
    progress: Res<GlobalProgress>,
    active: Res<ActiveRun>,
    mut overlay_vis: Query<&mut Visibility, With<ScreenOverlay>>,
    mut title_text: Query<&mut Text, (With<OverlayTitleText>, Without<OverlayBodyText>, Without<OverlayHintText>)>,
    mut body_text: Query<&mut Text, (With<OverlayBodyText>, Without<OverlayTitleText>, Without<OverlayHintText>)>,
    mut hint_text: Query<&mut Text, (With<OverlayHintText>, Without<OverlayTitleText>, Without<OverlayBodyText>)>,
    narrative: Option<Res<NarrativeData>>,
) {
    if !screen.is_changed() && !active.is_changed() && !progress.is_changed() {
        return;
    }

    let Ok(mut vis) = overlay_vis.single_mut() else { return };
    let Ok(mut title) = title_text.single_mut() else { return };
    let Ok(mut body) = body_text.single_mut() else { return };
    let Ok(mut hint) = hint_text.single_mut() else { return };

    match screen.as_ref() {
        ClientScreen::InMission => {
            *vis = Visibility::Hidden;
        }
        ClientScreen::Title => {
            *vis = Visibility::Visible;
            **title = "CINDERTIDE".to_string();
            **body = "a dieselpunk RTS".to_string();
            **hint = "Press Enter to begin".to_string();
        }
        ClientScreen::FactionPicker { selected } => {
            *vis = Visibility::Visible;
            **title = "Choose Your Faction".to_string();

            let factions: &[PlayableFaction] = if progress.handler_unlocked {
                &[PlayableFaction::Combine, PlayableFaction::Ironborn, PlayableFaction::Handler]
            } else {
                &[PlayableFaction::Combine, PlayableFaction::Ironborn]
            };

            let mut lines = String::new();
            for (i, &f) in factions.iter().enumerate() {
                let marker = if i == *selected { "> " } else { "  " };
                lines.push_str(&format!("{}{}\n", marker, faction_name(f)));
            }
            **body = lines.trim_end().to_string();
            **hint = "W/S or Arrow keys to select, Enter to confirm".to_string();
        }
        ClientScreen::Briefing { title: mission_title, briefing } => {
            *vis = Visibility::Visible;
            **title = mission_title.clone();
            **body = briefing.clone();
            **hint = "Press Enter to deploy".to_string();
        }
        ClientScreen::Debrief { title: mission_title, text, won } => {
            *vis = Visibility::Visible;
            let outcome = if *won { "VICTORY" } else { "DEFEAT" };
            **title = format!("{} — {}", outcome, mission_title);
            **body = text.clone();
            **hint = "Press Enter to continue".to_string();
        }
        ClientScreen::GameOver { won, handler_unlocked } => {
            *vis = Visibility::Visible;
            let outcome = if *won { "CAMPAIGN COMPLETE" } else { "CAMPAIGN ENDED" };
            **title = outcome.to_string();

            let finale_text = if let Some(ref nd) = narrative {
                let combine_first = matches!(
                    active.run.as_ref().map(|r| r.faction),
                    Some(PlayableFaction::Combine)
                );
                nd.finale(combine_first).to_string()
            } else {
                String::new()
            };

            let mut body_str = finale_text;
            if *handler_unlocked {
                body_str.push_str("\n\nTHE ARCHITECT IS UNLOCKED");
            }
            **body = body_str;
            **hint = "Press Enter to return to faction select".to_string();
        }
    }
}

// ── UI input handler ──────────────────────────────────────────────────────────

fn handle_ui_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut screen: ResMut<ClientScreen>,
    mut active: ResMut<ActiveRun>,
    mut game_state: ResMut<GameState>,
    progress: Res<GlobalProgress>,
    narrative: Option<Res<NarrativeData>>,
    mut commands: Commands,
) {
    // Only handle UI input when not in mission
    if *screen == ClientScreen::InMission {
        return;
    }

    let enter = keys.just_pressed(KeyCode::Enter);
    let up = keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW);
    let down = keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS);

    match screen.clone() {
        ClientScreen::Title => {
            if enter {
                *screen = ClientScreen::FactionPicker { selected: 0 };
            }
        }

        ClientScreen::FactionPicker { selected } => {
            let count = if progress.handler_unlocked { 3 } else { 2 };
            if up {
                let new = if selected == 0 { count - 1 } else { selected - 1 };
                *screen = ClientScreen::FactionPicker { selected: new };
            } else if down {
                *screen = ClientScreen::FactionPicker { selected: (selected + 1) % count };
            } else if enter {
                let factions: &[PlayableFaction] = if progress.handler_unlocked {
                    &[PlayableFaction::Combine, PlayableFaction::Ironborn, PlayableFaction::Handler]
                } else {
                    &[PlayableFaction::Combine, PlayableFaction::Ironborn]
                };
                let faction = factions[selected];

                // Create a new campaign run
                let run = CampaignRun {
                    faction,
                    current_mission: 0,
                    outcomes: Vec::new(),
                    complete: false,
                };
                *active = ActiveRun {
                    run: Some(run),
                    current_mission_entity: None,
                    missions_won: 0,
                    missions_lost: 0,
                };

                // Transition to briefing for mission 0
                let (title, briefing) = get_mission_narrative(&narrative, faction, 0);
                *screen = ClientScreen::Briefing { title, briefing };
            }
        }

        ClientScreen::Briefing { .. } => {
            if enter {
                commands.queue(|world: &mut World| {
                    cindertide::wipe_world_entities(world);

                    let player = world.resource::<ActiveRun>()
                        .run.as_ref()
                        .map(|r| map_faction(r.faction))
                        .unwrap_or(Faction::Combine);

                    let opponent = if player == Faction::Combine {
                        Faction::Ironborn
                    } else {
                        Faction::Combine
                    };

                    let mission_type = world.resource::<ActiveRun>()
                        .run.as_ref()
                        .and_then(|r| next_mission_type(r))
                        .unwrap_or(MissionType::Assault);

                    world.spawn(FactionBundle::new(player.clone()));

                    let mission_entity = world.spawn(Mission {
                        mission_type,
                        player_faction: player.clone(),
                        opponent_faction: opponent,
                        status: MissionStatus::Active,
                        elapsed: 0.0,
                        deadline: 300.0,
                    }).id();

                    cindertide::setup_demo_scenario(world, &player);

                    world.resource_mut::<ActiveRun>().current_mission_entity = Some(mission_entity);
                    *world.resource_mut::<GameState>() = GameState::InMission;
                    world.insert_resource(PlayerFaction(player));
                    *world.resource_mut::<ClientScreen>() = ClientScreen::InMission;
                });
            }
        }

        ClientScreen::Debrief { won, .. } => {
            if enter {
                // Advance campaign
                let done = if let Some(ref run) = active.run {
                    run.current_mission >= 5 || run.complete
                } else {
                    true
                };

                if done {
                    let handler_unlocked = progress.handler_unlocked;
                    *screen = ClientScreen::GameOver { won, handler_unlocked };
                } else {
                    // Get next briefing
                    if let Some(ref run) = active.run {
                        let faction = run.faction;
                        let mission_idx = run.current_mission;
                        let (title, briefing) = get_mission_narrative(&narrative, faction, mission_idx);
                        *screen = ClientScreen::Briefing { title, briefing };
                    } else {
                        *screen = ClientScreen::FactionPicker { selected: 0 };
                    }
                }
            }
        }

        ClientScreen::GameOver { .. } => {
            if enter {
                *screen = ClientScreen::FactionPicker { selected: 0 };
                // Reset the active run
                *active = ActiveRun::default();
                *game_state = GameState::Title;
            }
        }

        ClientScreen::InMission => {}
    }
}

/// Fetch narrative title + briefing for a faction/mission index.
fn get_mission_narrative(
    narrative: &Option<Res<NarrativeData>>,
    faction: PlayableFaction,
    index: usize,
) -> (String, String) {
    if let Some(ref nd) = narrative {
        let key = faction_narrative_key(faction);
        if let Some(mn) = nd.mission(key, index) {
            return (mn.title.clone(), mn.briefing.clone());
        }
    }
    (format!("Mission {}", index + 1), String::new())
}

// ── Poll mission end ──────────────────────────────────────────────────────────

fn poll_mission_end(
    mut screen: ResMut<ClientScreen>,
    mut active: ResMut<ActiveRun>,
    mut progress: ResMut<GlobalProgress>,
    missions: Query<&Mission>,
    narrative: Option<Res<NarrativeData>>,
) {
    if *screen != ClientScreen::InMission {
        return;
    }

    let Some(mission_entity) = active.current_mission_entity else {
        return;
    };

    let Ok(m) = missions.get(mission_entity) else {
        return;
    };

    if m.status == MissionStatus::Active {
        return;
    }

    let won = m.status == MissionStatus::Won;
    let mission_type = m.mission_type.clone();
    let player_faction = m.player_faction.clone();

    // Determine debrief narrative
    let faction_playable = active.run.as_ref().map(|r| r.faction).unwrap_or(PlayableFaction::Combine);
    let mission_idx = active.run.as_ref().map(|r| r.current_mission).unwrap_or(0);
    let key = faction_narrative_key(faction_playable);

    let (title, debrief_text) = if let Some(ref nd) = narrative {
        if let Some(mn) = nd.mission(key, mission_idx) {
            let text = if won { mn.win.clone() } else { mn.loss.clone() };
            (mn.title.clone(), text)
        } else {
            (format!("Mission {}", mission_idx + 1), String::new())
        }
    } else {
        (format!("Mission {}", mission_idx + 1), String::new())
    };

    // Apply outcome to campaign run
    if let Some(ref mut run) = active.run {
        apply_mission_outcome(run, &mut progress, won, mission_type);
    }
    active.current_mission_entity = None;

    *screen = ClientScreen::Debrief {
        title,
        text: debrief_text,
        won,
    };
}

// ── Render systems ────────────────────────────────────────────────────────────

fn render_tiles(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    tiles: Query<&Tile, Without<RenderedTile>>,
) {
    for tile in &tiles {
        let color = terrain_color(&tile.terrain_type);
        let pos = grid_to_world(tile.pos.x, tile.pos.y);
        commands.spawn((
            Mesh3d(meshes.add(Plane3d::default().mesh().size(0.95, 0.95))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 0.9,
                ..default()
            })),
            Transform::from_translation(pos),
            RenderedTile,
        ));
    }
}

fn spawn_unit_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut visual_entities: ResMut<VisualEntities>,
    units: Query<(Entity, &UnitPos, &Faction), Added<UnitType>>,
) {
    for (entity, pos, faction) in &units {
        let color = faction_color(faction);
        let world_pos = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.75;
        let visual = commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.6, 1.5, 0.6))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                ..default()
            })),
            Transform::from_translation(world_pos),
        )).id();
        visual_entities.units.insert(entity, visual);
    }
}

fn sync_unit_positions(
    units: Query<(Entity, &UnitPos), With<UnitType>>,
    visual_entities: Res<VisualEntities>,
    mut transforms: Query<&mut Transform>,
) {
    for (entity, pos) in &units {
        if let Some(&visual) = visual_entities.units.get(&entity) {
            if let Ok(mut transform) = transforms.get_mut(visual) {
                let target = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.75;
                transform.translation = transform.translation.lerp(target, 0.15);
            }
        }
    }
}

fn spawn_building_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut visual_entities: ResMut<VisualEntities>,
    buildings: Query<(Entity, &BuildingPos, &Faction), Added<BuildingType>>,
) {
    for (entity, pos, faction) in &buildings {
        if visual_entities.buildings.contains_key(&entity) {
            continue;
        }
        let color = faction_color(faction).mix(&Color::WHITE, 0.25);
        let world_pos = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.5;
        let visual = commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.9, 1.0, 0.9))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                ..default()
            })),
            Transform::from_translation(world_pos),
        )).id();
        visual_entities.buildings.insert(entity, visual);
    }
}

// ── Input systems ──────────────────────────────────────────────────────────────

fn handle_mouse_input(
    mut commands: Commands,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<IsometricCamera>>,
    units: Query<(Entity, &UnitPos, &Faction), With<UnitType>>,
    mut selected: ResMut<SelectedUnits>,
    player_faction: Option<Res<PlayerFaction>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    selection_rings: Query<Entity, With<SelectionRing>>,
    tiles: Query<&Tile>,
    mut drag_state: ResMut<DragState>,
    attack_move_mode: Res<AttackMoveMode>,
    screen: Res<ClientScreen>,
) {
    // Only handle mouse input during mission
    if *screen != ClientScreen::InMission {
        return;
    }

    let Some(player_faction) = player_faction else { return };
    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_transform)) = cameras.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };

    let shift_held = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    // ── Left mouse button ──────────────────────────────────────────────────────
    if buttons.just_pressed(MouseButton::Left) {
        drag_state.start = Some(cursor_pos);
        drag_state.current = cursor_pos;
    }

    if buttons.pressed(MouseButton::Left) {
        drag_state.current = cursor_pos;
    }

    if buttons.just_released(MouseButton::Left) {
        let start = drag_state.start.take();
        let current = drag_state.current;

        if let Some(start) = start {
            let drag_dist = (current - start).length();

            if drag_dist > 4.0 {
                // ── Box drag selection ─────────────────────────────────────────
                let (rect_min, rect_size) = drag_rect(start, current);
                let rect_max = rect_min + rect_size;

                if !shift_held {
                    for ring in &selection_rings {
                        commands.entity(ring).despawn();
                    }
                    selected.entities.clear();
                }

                let mut newly_selected: Vec<(Entity, GridPos)> = Vec::new();

                for (entity, pos, faction) in &units {
                    if *faction != player_faction.0 {
                        continue;
                    }
                    // Project unit world pos to screen
                    let world_pos = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.75;
                    if let Ok(screen_pos) = camera.world_to_viewport(cam_transform, world_pos) {
                        if screen_pos.x >= rect_min.x && screen_pos.x <= rect_max.x
                            && screen_pos.y >= rect_min.y && screen_pos.y <= rect_max.y
                        {
                            if !selected.entities.contains(&entity) {
                                selected.entities.push(entity);
                                newly_selected.push((entity, pos.pos.clone()));
                            }
                        }
                    }
                }

                for (unit_entity, pos) in newly_selected {
                    let ring_pos = grid_to_world(pos.x, pos.y) - Vec3::Y * 0.35;
                    commands.spawn((
                        Mesh3d(meshes.add(Cylinder::new(0.4, 0.05))),
                        MeshMaterial3d(materials.add(StandardMaterial {
                            base_color: Color::srgb(1.0, 1.0, 0.0),
                            emissive: LinearRgba::new(1.0, 1.0, 0.0, 1.0),
                            ..default()
                        })),
                        Transform::from_translation(ring_pos),
                        SelectionRing { unit_entity },
                    ));
                }
            } else {
                // ── Single click selection ─────────────────────────────────────
                let Some((gx, gy)) = screen_to_grid(cursor_pos, camera, cam_transform) else { return };

                let clicked_unit = units.iter().find(|(_, pos, faction)| {
                    pos.pos.x == gx && pos.pos.y == gy && **faction == player_faction.0
                }).map(|(e, _, _)| e);

                if shift_held {
                    // Shift+click: toggle unit in/out of selection
                    if let Some(unit_entity) = clicked_unit {
                        if let Some(idx) = selected.entities.iter().position(|&e| e == unit_entity) {
                            selected.entities.remove(idx);
                            // Despawn ring for deselected unit
                            for ring in &selection_rings {
                                commands.entity(ring).despawn();
                            }
                            // Respawn rings for remaining selected units
                            for &sel_entity in &selected.entities {
                                if let Ok((_, pos, _)) = units.get(sel_entity) {
                                    let ring_pos = grid_to_world(pos.pos.x, pos.pos.y) - Vec3::Y * 0.35;
                                    commands.spawn((
                                        Mesh3d(meshes.add(Cylinder::new(0.4, 0.05))),
                                        MeshMaterial3d(materials.add(StandardMaterial {
                                            base_color: Color::srgb(1.0, 1.0, 0.0),
                                            emissive: LinearRgba::new(1.0, 1.0, 0.0, 1.0),
                                            ..default()
                                        })),
                                        Transform::from_translation(ring_pos),
                                        SelectionRing { unit_entity: sel_entity },
                                    ));
                                }
                            }
                        } else {
                            // Add to selection
                            selected.entities.push(unit_entity);
                            if let Ok((_, pos, _)) = units.get(unit_entity) {
                                let ring_pos = grid_to_world(pos.pos.x, pos.pos.y) - Vec3::Y * 0.35;
                                commands.spawn((
                                    Mesh3d(meshes.add(Cylinder::new(0.4, 0.05))),
                                    MeshMaterial3d(materials.add(StandardMaterial {
                                        base_color: Color::srgb(1.0, 1.0, 0.0),
                                        emissive: LinearRgba::new(1.0, 1.0, 0.0, 1.0),
                                        ..default()
                                    })),
                                    Transform::from_translation(ring_pos),
                                    SelectionRing { unit_entity },
                                ));
                            }
                        }
                    }
                } else {
                    // Normal click: clear selection, select clicked unit
                    for ring in &selection_rings {
                        commands.entity(ring).despawn();
                    }
                    selected.entities.clear();

                    if let Some(unit_entity) = clicked_unit {
                        selected.entities.push(unit_entity);
                        if let Ok((_, pos, _)) = units.get(unit_entity) {
                            let ring_pos = grid_to_world(pos.pos.x, pos.pos.y) - Vec3::Y * 0.35;
                            commands.spawn((
                                Mesh3d(meshes.add(Cylinder::new(0.4, 0.05))),
                                MeshMaterial3d(materials.add(StandardMaterial {
                                    base_color: Color::srgb(1.0, 1.0, 0.0),
                                    emissive: LinearRgba::new(1.0, 1.0, 0.0, 1.0),
                                    ..default()
                                })),
                                Transform::from_translation(ring_pos),
                                SelectionRing { unit_entity },
                            ));
                        }
                    }
                }
            }
        }
    }

    // ── Right mouse button ─────────────────────────────────────────────────────
    if buttons.just_pressed(MouseButton::Right) {
        if selected.entities.is_empty() {
            return;
        }
        let Some((gx, gy)) = screen_to_grid(cursor_pos, camera, cam_transform) else { return };

        let target_pos = GridPos { x: gx, y: gy };

        let enemy_at_target = units.iter().find(|(_, pos, faction)| {
            pos.pos.x == gx && pos.pos.y == gy && **faction != player_faction.0
        }).map(|(e, _, _)| e);

        let tile_exists = tiles.iter().any(|t| t.pos.x == gx && t.pos.y == gy);
        if !tile_exists && enemy_at_target.is_none() {
            return;
        }

        let selected_entities: Vec<Entity> = selected.entities.clone();

        if let Some(target_entity) = enemy_at_target {
            // Right-click on enemy: direct attack order
            for &unit_entity in &selected_entities {
                commands.entity(unit_entity)
                    .remove::<MoveTarget>()
                    .remove::<MoveProgress>()
                    .remove::<AttackMoveOrder>()
                    .remove::<HoldPosition>()
                    .insert(PlayerAttackOrder { target: target_entity });
            }
        } else if shift_held {
            // Shift+right-click: queue waypoint
            info!("queued waypoint at ({gx}, {gy}) — single-target MoveTarget used");
            let tile_map: HashMap<(i32, i32), cindertide::map::TerrainType> = tiles
                .iter()
                .map(|t| ((t.pos.x, t.pos.y), t.terrain_type.clone()))
                .collect();
            let max_x = tile_map.keys().map(|(x, _)| *x).max().unwrap_or(40);
            let max_y = tile_map.keys().map(|(_, y)| *y).max().unwrap_or(25);

            for &unit_entity in &selected_entities {
                let unit_kind = units.get(unit_entity).ok().and_then(|(_, _, _)| {
                    None::<cindertide::units::UnitKind>
                });
                let start = units.get(unit_entity).ok().map(|(_, pos, _)| pos.pos.clone());
                let Some(start) = start else { continue };

                let pf_kind = match unit_kind {
                    Some(UnitKind::Vehicle) => cindertide::map::pathfinding::UnitKind::Vehicle,
                    _ => cindertide::map::pathfinding::UnitKind::Infantry,
                };
                let grid = cindertide::map::pathfinding::PathfindingGrid {
                    width: max_x + 1,
                    height: max_y + 1,
                    tiles: tile_map.clone(),
                    unit_type: pf_kind,
                };
                if let Some(path) = grid.find_path(start, target_pos.clone()) {
                    commands.entity(unit_entity)
                        .remove::<HoldPosition>()
                        .insert(MoveTarget { target: target_pos.clone() })
                        .insert(MoveProgress { path, current_step: 0, elapsed: 0.0 });
                }
            }
        } else if attack_move_mode.0 {
            // Attack-move: units move to destination but engage enemies en route
            for &unit_entity in &selected_entities {
                commands.entity(unit_entity)
                    .remove::<MoveTarget>()
                    .remove::<MoveProgress>()
                    .remove::<PlayerAttackOrder>()
                    .remove::<HoldPosition>()
                    .insert(AttackMoveOrder { target: target_pos.clone() });
            }
        } else {
            // Normal right-click move
            let tile_map: HashMap<(i32, i32), cindertide::map::TerrainType> = tiles
                .iter()
                .map(|t| ((t.pos.x, t.pos.y), t.terrain_type.clone()))
                .collect();
            let max_x = tile_map.keys().map(|(x, _)| *x).max().unwrap_or(40);
            let max_y = tile_map.keys().map(|(_, y)| *y).max().unwrap_or(25);

            for &unit_entity in &selected_entities {
                let unit_kind = units.get(unit_entity).ok().and_then(|(_, _, _)| {
                    None::<cindertide::units::UnitKind>
                });
                let start = units.get(unit_entity).ok().map(|(_, pos, _)| pos.pos.clone());
                let Some(start) = start else { continue };

                let pf_kind = match unit_kind {
                    Some(UnitKind::Vehicle) => cindertide::map::pathfinding::UnitKind::Vehicle,
                    _ => cindertide::map::pathfinding::UnitKind::Infantry,
                };
                let grid = cindertide::map::pathfinding::PathfindingGrid {
                    width: max_x + 1,
                    height: max_y + 1,
                    tiles: tile_map.clone(),
                    unit_type: pf_kind,
                };
                if let Some(path) = grid.find_path(start, target_pos.clone()) {
                    commands.entity(unit_entity)
                        .remove::<HoldPosition>()
                        .insert(MoveTarget { target: target_pos.clone() })
                        .insert(MoveProgress { path, current_step: 0, elapsed: 0.0 });
                }
            }
        }
    }
}

/// Handle keyboard commands: A (attack-move toggle), S (stop), H (hold position),
/// Space (pause), Tab (cycle selected unit).
fn handle_keyboard_commands(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut selected: ResMut<SelectedUnits>,
    selection_rings: Query<Entity, With<SelectionRing>>,
    units: Query<(Entity, &UnitPos, &Faction), With<UnitType>>,
    player_faction: Option<Res<PlayerFaction>>,
    mut attack_move_mode: ResMut<AttackMoveMode>,
    mut paused: ResMut<Paused>,
    mut time: ResMut<Time<Virtual>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    screen: Res<ClientScreen>,
) {
    // Only handle gameplay keys during mission
    if *screen != ClientScreen::InMission {
        return;
    }

    // A key — toggle attack-move mode
    if keys.just_pressed(KeyCode::KeyA) {
        attack_move_mode.0 = !attack_move_mode.0;
        info!("Attack-move mode: {}", attack_move_mode.0);
    }

    // S key — stop all selected units
    if keys.just_pressed(KeyCode::KeyS) {
        for &unit_entity in &selected.entities {
            commands.entity(unit_entity)
                .remove::<MoveTarget>()
                .remove::<MoveProgress>()
                .remove::<PlayerAttackOrder>()
                .remove::<AttackMoveOrder>()
                .remove::<HoldPosition>();
        }
    }

    // H key — hold position (attack in range but do not move)
    if keys.just_pressed(KeyCode::KeyH) {
        for &unit_entity in &selected.entities {
            commands.entity(unit_entity)
                .remove::<MoveTarget>()
                .remove::<MoveProgress>()
                .remove::<PlayerAttackOrder>()
                .remove::<AttackMoveOrder>()
                .insert(HoldPosition);
        }
    }

    // Space — toggle pause via Bevy virtual time
    if keys.just_pressed(KeyCode::Space) {
        paused.0 = !paused.0;
        if paused.0 {
            time.pause();
        } else {
            time.unpause();
        }
    }

    // Tab — cycle to next player-faction unit
    if keys.just_pressed(KeyCode::Tab) {
        let Some(pf) = player_faction else { return };

        // Collect all player-faction unit entities in a stable order
        let mut player_units: Vec<Entity> = units.iter()
            .filter(|(_, _, f)| **f == pf.0)
            .map(|(e, _, _)| e)
            .collect();
        player_units.sort(); // stable order by Entity id

        if player_units.is_empty() {
            return;
        }

        // If exactly one unit is selected, deselect it and select the next one
        let next_entity = if selected.entities.len() == 1 {
            let current = selected.entities[0];
            let idx = player_units.iter().position(|&e| e == current).unwrap_or(0);
            let next_idx = (idx + 1) % player_units.len();
            player_units[next_idx]
        } else {
            // No single selection: just pick the first unit
            player_units[0]
        };

        // Clear old rings
        for ring in &selection_rings {
            commands.entity(ring).despawn();
        }
        selected.entities.clear();
        selected.entities.push(next_entity);

        // Spawn ring for newly selected unit
        if let Ok((_, pos, _)) = units.get(next_entity) {
            let ring_pos = grid_to_world(pos.pos.x, pos.pos.y) - Vec3::Y * 0.35;
            commands.spawn((
                Mesh3d(meshes.add(Cylinder::new(0.4, 0.05))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 1.0, 0.0),
                    emissive: LinearRgba::new(1.0, 1.0, 0.0, 1.0),
                    ..default()
                })),
                Transform::from_translation(ring_pos),
                SelectionRing { unit_entity: next_entity },
            ));
        }
    }
}

/// Update the drag-selection rectangle UI overlay.
fn update_drag_rect(
    drag_state: Res<DragState>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut drag_rect_query: Query<&mut Node, With<DragRectUi>>,
) {
    let Ok(mut node) = drag_rect_query.single_mut() else { return };

    if buttons.pressed(MouseButton::Left) {
        if let Some(start) = drag_state.start {
            let dist = (drag_state.current - start).length();
            if dist > 4.0 {
                let (rect_min, rect_size) = drag_rect(start, drag_state.current);
                node.left = Val::Px(rect_min.x);
                node.top = Val::Px(rect_min.y);
                node.width = Val::Px(rect_size.x);
                node.height = Val::Px(rect_size.y);
                return;
            }
        }
    }
    // Hide rect when not dragging
    node.width = Val::Px(0.0);
    node.height = Val::Px(0.0);
}

/// Show/hide the PAUSED overlay based on the Paused resource.
fn update_paused_overlay(
    paused: Res<Paused>,
    mut overlay_query: Query<&mut Visibility, With<PausedOverlay>>,
) {
    if !paused.is_changed() {
        return;
    }
    for mut vis in &mut overlay_query {
        *vis = if paused.0 { Visibility::Visible } else { Visibility::Hidden };
    }
}

fn sync_selection_rings(
    units: Query<&UnitPos, With<UnitType>>,
    mut rings: Query<(&SelectionRing, &mut Transform)>,
) {
    for (ring, mut transform) in &mut rings {
        if let Ok(pos) = units.get(ring.unit_entity) {
            let target = grid_to_world(pos.pos.x, pos.pos.y) - Vec3::Y * 0.35;
            transform.translation = transform.translation.lerp(target, 0.15);
        }
    }
}

fn edge_scroll(
    mut cameras: Query<(&mut Transform, &IsometricCamera)>,
    windows: Query<&Window>,
    time: Res<Time>,
    screen: Res<ClientScreen>,
) {
    if *screen != ClientScreen::InMission {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Ok((mut transform, cam)) = cameras.single_mut() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let w = window.width();
    let h = window.height();
    let margin = 20.0;
    let dt = time.delta_secs();
    let mut pan = Vec3::ZERO;
    if cursor.x < margin { pan += Vec3::new(-1.0, 0.0, 1.0).normalize(); }
    if cursor.x > w - margin { pan += Vec3::new(1.0, 0.0, -1.0).normalize(); }
    if cursor.y < margin { pan += Vec3::new(-1.0, 0.0, -1.0).normalize(); }
    if cursor.y > h - margin { pan += Vec3::new(1.0, 0.0, 1.0).normalize(); }
    transform.translation += pan * cam.pan_speed * dt;
}

fn camera_pan_zoom(
    mut query: Query<(&mut Transform, &mut Projection, &IsometricCamera)>,
    keys: Res<ButtonInput<KeyCode>>,
    mut scroll: EventReader<MouseWheel>,
    time: Res<Time>,
    screen: Res<ClientScreen>,
) {
    if *screen != ClientScreen::InMission {
        // Still consume scroll events to avoid buildup
        for _ in scroll.read() {}
        return;
    }

    let Ok((mut transform, mut projection, cam)) = query.single_mut() else { return };

    let dt = time.delta_secs();
    let mut pan = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) { pan += Vec3::new(-1.0, 0.0, -1.0).normalize(); }
    if keys.pressed(KeyCode::KeyS) { pan += Vec3::new(1.0, 0.0, 1.0).normalize(); }
    if keys.pressed(KeyCode::KeyA) { pan += Vec3::new(-1.0, 0.0, 1.0).normalize(); }
    if keys.pressed(KeyCode::KeyD) { pan += Vec3::new(1.0, 0.0, -1.0).normalize(); }
    transform.translation += pan * cam.pan_speed * dt;

    if let Projection::Orthographic(ref mut ortho) = *projection {
        for ev in scroll.read() {
            ortho.scale = (ortho.scale - ev.y * cam.zoom_speed).clamp(2.0, 60.0);
        }
    }
}

// ── Color helpers ──────────────────────────────────────────────────────────────

fn terrain_color(terrain: &cindertide::map::TerrainType) -> Color {
    use cindertide::map::TerrainType;
    match terrain {
        TerrainType::Grass   => Color::srgb(0.30, 0.50, 0.20),
        TerrainType::Road    => Color::srgb(0.40, 0.40, 0.40),
        TerrainType::Forest  => Color::srgb(0.10, 0.35, 0.10),
        TerrainType::Rubble  => Color::srgb(0.50, 0.45, 0.40),
        TerrainType::Mud     => Color::srgb(0.45, 0.30, 0.15),
        TerrainType::Corrupted => Color::srgb(0.60, 0.10, 0.60),
        _                    => Color::srgb(0.50, 0.50, 0.50),
    }
}

fn faction_color(faction: &Faction) -> Color {
    match faction {
        Faction::Combine  => Color::srgb(0.90, 0.75, 0.10),
        Faction::Ironborn => Color::srgb(0.60, 0.60, 0.65),
        Faction::Covenant => Color::srgb(0.20, 0.40, 0.90),
        Faction::Hollow   => Color::srgb(0.70, 0.10, 0.70),
    }
}
