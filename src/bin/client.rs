use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::render::camera::ScalingMode;
use cindertide::map::{Faction, GridPos, Tile};
use cindertide::units::{UnitPos, UnitType};
use cindertide::buildings::{BuildingPos, BuildingType};
use cindertide::campaign::{CampaignRun, PlayableFaction, GlobalProgress, apply_mission_outcome, next_mission_type, save_progress, load_progress};
use cindertide::game::{ActiveRun, GameState};
use cindertide::mission::{Mission, MissionStatus};
use cindertide::mapgen::MissionType;
use cindertide::narrative::NarrativeData;
use cindertide::resources::{FactionBundle, FactionEntity, ResourcePool};
use cindertide::tech::{Tech, ResearchInProgress, ResearchTarget, Tier, Doctrine, start_research};
use cindertide::combat::{PlayerAttackOrder, AttackMoveOrder, HoldPosition, AttackTarget, Health, Suppressed, AbilityCooldowns};
use cindertide::units::{MoveTarget, MoveProgress, UnitKind};
use cindertide::buildings::Built;
use cindertide::production::{ProductionQueue, unit_production_seconds};
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
use std::collections::{HashMap, HashSet};
use serde::{Serialize, Deserialize};

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
        .init_resource::<EditorState>()
        .init_resource::<TechPanelVisible>()
        .init_resource::<FogOfWar>()
        .insert_resource(MinimapTimer(0.0))
        .add_systems(Startup, setup_scene)
        .add_systems(Startup, setup_ui)
        .add_systems(Startup, load_narrative)
        .add_systems(Startup, startup_load_progress)
        .add_systems(Update, render_tiles)
        .add_systems(Update, sync_rendered_tile_colors)
        .add_systems(Update, spawn_unit_visuals)
        .add_systems(Update, sync_unit_positions)
        .add_systems(Update, spawn_building_visuals)
        .add_systems(Update, camera_pan_zoom)
        .add_systems(Update, edge_scroll)
        .add_systems(Update, handle_mouse_input)
        .add_systems(Update, handle_editor_mouse_input)
        .add_systems(Update, sync_selection_rings)
        .add_systems(Update, update_drag_rect)
        .add_systems(Update, handle_keyboard_commands)
        .add_systems(Update, handle_editor_keyboard)
        .add_systems(Update, update_paused_overlay)
        .add_systems(Update, handle_ui_input)
        .add_systems(Update, update_screen_overlay)
        .add_systems(Update, poll_mission_end)
        .add_systems(Update, update_editor_panel)
        .add_systems(Update, update_hud_visibility)
        .add_systems(Update, update_resource_bar)
        .add_systems(Update, update_unit_info_panel)
        .add_systems(Update, update_production_queue)
        .add_systems(Update, update_minimap)
        .add_systems(Update, handle_minimap_click)
        .add_systems(Update, update_mission_objectives)
        .add_systems(Update, handle_ability_input)
        .add_systems(Update, update_tech_panel)
        .add_systems(Update, update_fog_of_war)
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
    MapEditor,
}

impl Default for ClientScreen {
    fn default() -> Self {
        ClientScreen::Title
    }
}

// ── Map editor tool ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum EditorTool {
    PaintTerrain,
    PlaceUnit,
    PlaceBuilding,
    Erase,
}

impl Default for EditorTool {
    fn default() -> Self { EditorTool::PaintTerrain }
}

/// Terrain types cycled in paint mode (right-click).
const EDITOR_TERRAINS: &[cindertide::map::TerrainType] = &[
    cindertide::map::TerrainType::Grass,
    cindertide::map::TerrainType::Road,
    cindertide::map::TerrainType::Forest,
    cindertide::map::TerrainType::Rubble,
    cindertide::map::TerrainType::Mud,
    cindertide::map::TerrainType::Corrupted,
];

/// Factions cycled with F in editor.
const EDITOR_FACTIONS: &[Faction] = &[
    Faction::Combine,
    Faction::Ironborn,
    Faction::Covenant,
    Faction::Hollow,
];

/// Unit types cycled with T in editor.
const EDITOR_UNIT_TYPES: &[UnitType] = &[
    UnitType::Riflemen,
    UnitType::HeavyWeapons,
    UnitType::LightVehicle,
    UnitType::HeavyArmor,
];

/// Building types cycled with B in editor.
const EDITOR_BUILDING_TYPES: &[BuildingType] = &[
    BuildingType::Barracks,
    BuildingType::Refinery,
    BuildingType::CommandBunker,
    BuildingType::MotorPool,
    BuildingType::Pillbox,
    BuildingType::Watchtower,
];

#[derive(Resource)]
struct EditorState {
    tool: EditorTool,
    terrain_idx: usize,
    faction_idx: usize,
    unit_type_idx: usize,
    building_type_idx: usize,
    /// Timer for double-press Del confirm (seconds since first press).
    del_confirm_timer: Option<f32>,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            tool: EditorTool::PaintTerrain,
            terrain_idx: 0,
            faction_idx: 0,
            unit_type_idx: 0,
            building_type_idx: 0,
            del_confirm_timer: None,
        }
    }
}

// ── Map save/load data structures ────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SavedTile {
    x: i32,
    y: i32,
    terrain: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SavedUnit {
    x: i32,
    y: i32,
    faction: String,
    unit_type: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SavedBuilding {
    x: i32,
    y: i32,
    faction: String,
    building_type: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SavedMap {
    tiles: Vec<SavedTile>,
    units: Vec<SavedUnit>,
    buildings: Vec<SavedBuilding>,
}

// ── Components ──────────────────────────────────────────────────────────────

#[derive(Component)]
struct IsometricCamera {
    pan_speed: f32,
    zoom_speed: f32,
}

#[derive(Component)]
struct RenderedTile {
    pos: GridPos,
}

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

/// Root node of the editor side panel.
#[derive(Component)]
struct EditorPanel;

/// Text inside the editor panel.
#[derive(Component)]
struct EditorPanelText;

/// Root node of the in-mission HUD (parent of the three HUD panels).
#[derive(Component)]
struct HudRoot;

/// Text node for the top resource bar.
#[derive(Component)]
struct ResourceBarText;

/// Text node for the bottom unit info panel.
#[derive(Component)]
struct UnitInfoText;

/// Text node for the bottom-right production queue panel.
#[derive(Component)]
struct ProductionQueueText;

/// Root panel of the minimap (bottom-left, InMission only).
#[derive(Component)]
struct MinimapPanel;

/// Colored dot node inside the minimap.
#[derive(Component)]
struct MinimapDot;

/// Text node showing the current mission objective (top-right, InMission only).
#[derive(Component)]
struct ObjectivesText;

/// Root node of the tech tree overlay panel.
#[derive(Component)]
struct TechPanel;

/// Text inside the tech tree panel.
#[derive(Component)]
struct TechPanelText;

/// Resource tracking tech panel open/close state and selected index.
#[derive(Resource, Default)]
struct TechPanelVisible {
    visible: bool,
    selected_idx: usize,
}

// ── Resources ────────────────────────────────────────────────────────────────

/// Timer for throttling minimap rebuilds (rebuild at most every 0.5 s).
#[derive(Resource)]
struct MinimapTimer(f32);

/// Fog of war state: which tiles are currently visible and which have been explored.
#[derive(Resource, Default)]
struct FogOfWar {
    /// Tiles currently within vision range of at least one player unit/building.
    visible: HashSet<(i32, i32)>,
    /// Tiles ever seen by a player unit/building (superset of visible).
    explored: HashSet<(i32, i32)>,
    /// Countdown timer — fog updates every 0.25 s.
    timer: f32,
}

#[derive(Resource, Default)]
struct VisualEntities {
    units: HashMap<Entity, Entity>,
    buildings: HashMap<Entity, Entity>,
    /// Maps grid position to the StandardMaterial handle of that tile's mesh.
    tile_materials: HashMap<(i32, i32), Handle<StandardMaterial>>,
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

/// On startup, load GlobalProgress from disk and insert it as a resource.
fn startup_load_progress(mut commands: Commands) {
    let progress = load_progress();
    commands.insert_resource(progress);
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

        // ── In-mission HUD ──────────────────────────────────────────────────
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            Visibility::Hidden,
            HudRoot,
        )).with_children(|hud| {
            // Top resource bar
            hud.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(16.0), Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
            )).with_children(|bar| {
                bar.spawn((
                    Text::new("FUEL: 0   SCRAP: 0   MANPOWER: 0"),
                    TextColor(Color::srgb(0.95, 0.90, 0.40)),
                    TextFont { font_size: 18.0, ..default() },
                    ResourceBarText,
                ));
            });

            // Bottom unit info panel (center)
            hud.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(0.0),
                    left: Val::Percent(20.0),
                    width: Val::Percent(60.0),
                    padding: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
            )).with_children(|panel| {
                panel.spawn((
                    Text::new(""),
                    TextColor(Color::srgb(0.85, 0.95, 0.85)),
                    TextFont { font_size: 16.0, ..default() },
                    UnitInfoText,
                ));
            });

            // Bottom-right production queue panel
            hud.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(0.0),
                    right: Val::Px(0.0),
                    width: Val::Px(260.0),
                    padding: UiRect::all(Val::Px(10.0)),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
            )).with_children(|panel| {
                panel.spawn((
                    Text::new(""),
                    TextColor(Color::srgb(0.75, 0.85, 1.0)),
                    TextFont { font_size: 14.0, ..default() },
                    ProductionQueueText,
                ));
            });

            // Bottom-left minimap panel (200×140 px)
            hud.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(0.0),
                    left: Val::Px(0.0),
                    width: Val::Px(200.0),
                    height: Val::Px(140.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.05, 0.05, 0.85)),
                MinimapPanel,
            ));

            // Top-right mission objectives panel
            hud.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(36.0), // below the resource bar
                    right: Val::Px(0.0),
                    width: Val::Px(280.0),
                    padding: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
            )).with_children(|panel| {
                panel.spawn((
                    Text::new(""),
                    TextColor(Color::srgb(1.0, 0.85, 0.3)),
                    TextFont { font_size: 14.0, ..default() },
                    ObjectivesText,
                ));
            });
        });

        // Editor side panel (right side)
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(220.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.05, 0.10, 0.88)),
            Visibility::Hidden,
            EditorPanel,
        )).with_children(|p| {
            p.spawn((
                Text::new("MAP EDITOR"),
                TextColor(Color::srgb(1.0, 0.85, 0.2)),
                TextFont { font_size: 18.0, ..default() },
            ));
            p.spawn((
                Node { margin: UiRect::top(Val::Px(8.0)), ..default() },
                Text::new(""),
                TextColor(Color::srgb(0.85, 0.85, 0.85)),
                TextFont { font_size: 14.0, ..default() },
                EditorPanelText,
            ));
        });

        // Tech tree panel (fullscreen overlay, toggled by T during InMission)
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexStart,
                padding: UiRect::all(Val::Px(40.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.88)),
            Visibility::Hidden,
            TechPanel,
        )).with_children(|p| {
            p.spawn((
                Text::new("TECH TREE — T to close"),
                TextColor(Color::srgb(1.0, 0.85, 0.2)),
                TextFont { font_size: 28.0, ..default() },
            ));
            p.spawn((
                Node { margin: UiRect::top(Val::Px(20.0)), max_width: Val::Px(900.0), ..default() },
                Text::new(""),
                TextColor(Color::srgb(0.85, 0.9, 0.85)),
                TextFont { font_size: 18.0, ..default() },
                TechPanelText,
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
        ClientScreen::InMission | ClientScreen::MapEditor => {
            *vis = Visibility::Hidden;
        }
        ClientScreen::Title => {
            *vis = Visibility::Visible;
            **title = "CINDERTIDE".to_string();
            **body = "a dieselpunk RTS".to_string();
            **hint = "Press Enter to begin  |  E — Map Editor".to_string();
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
    // Only handle UI input when not in mission or editor
    if *screen == ClientScreen::InMission || *screen == ClientScreen::MapEditor {
        return;
    }

    let enter = keys.just_pressed(KeyCode::Enter);
    let up = keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW);
    let down = keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS);

    match screen.clone() {
        ClientScreen::Title => {
            if enter {
                *screen = ClientScreen::FactionPicker { selected: 0 };
            } else if keys.just_pressed(KeyCode::KeyE) {
                // Enter map editor: wipe world, spawn blank 48×28 grass map
                commands.queue(|world: &mut World| {
                    cindertide::wipe_world_entities(world);
                    for y in 0..28_i32 {
                        for x in 0..48_i32 {
                            world.spawn(Tile {
                                pos: GridPos { x, y },
                                terrain_type: cindertide::map::TerrainType::Grass,
                                cover: cindertide::map::CoverDensity::None,
                            });
                        }
                    }
                    *world.resource_mut::<ClientScreen>() = ClientScreen::MapEditor;
                });
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
        ClientScreen::MapEditor => {}
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

    // Persist progress to disk on each debrief transition
    save_progress(&progress);

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
    mut visual_entities: ResMut<VisualEntities>,
    tiles: Query<&Tile, Without<RenderedTile>>,
) {
    for tile in &tiles {
        // Start tiles as nearly black (never-seen) until fog of war reveals them.
        let color = Color::srgb(0.02, 0.02, 0.02);
        let pos = grid_to_world(tile.pos.x, tile.pos.y);
        let mat_handle = materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.9,
            ..default()
        });
        let key = (tile.pos.x, tile.pos.y);
        visual_entities.tile_materials.insert(key, mat_handle.clone());
        commands.spawn((
            Mesh3d(meshes.add(Plane3d::default().mesh().size(0.95, 0.95))),
            MeshMaterial3d(mat_handle),
            Transform::from_translation(pos),
            RenderedTile { pos: tile.pos.clone() },
        ));
    }
}

/// In editor mode, sync rendered tile colors to reflect terrain changes.
fn sync_rendered_tile_colors(
    screen: Res<ClientScreen>,
    tiles: Query<&Tile>,
    mut rendered: Query<(&RenderedTile, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if *screen != ClientScreen::MapEditor {
        return;
    }
    for (rt, mat_handle) in &mut rendered {
        // Find the tile entity with matching pos
        if let Some(tile) = tiles.iter().find(|t| t.pos == rt.pos) {
            let color = terrain_color(&tile.terrain_type);
            if let Some(mat) = materials.get_mut(mat_handle) {
                mat.base_color = color;
            }
        }
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
    if *screen != ClientScreen::InMission && *screen != ClientScreen::MapEditor {
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
    if *screen != ClientScreen::InMission && *screen != ClientScreen::MapEditor {
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

// ── Editor systems ─────────────────────────────────────────────────────────────

/// Handle keyboard commands while in map editor.
fn handle_editor_keyboard(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut screen: ResMut<ClientScreen>,
    mut editor: ResMut<EditorState>,
    time: Res<Time>,
    tiles: Query<(Entity, &Tile)>,
    units: Query<Entity, With<UnitType>>,
    buildings: Query<Entity, With<BuildingType>>,
    mut visual_entities: ResMut<VisualEntities>,
    rendered_tiles: Query<Entity, With<RenderedTile>>,
) {
    if *screen != ClientScreen::MapEditor {
        return;
    }

    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);

    // Escape → return to title
    if keys.just_pressed(KeyCode::Escape) {
        // Wipe editor entities and return to title
        for (entity, _) in &tiles {
            commands.entity(entity).despawn();
        }
        for entity in &units { commands.entity(entity).despawn(); }
        for entity in &buildings { commands.entity(entity).despawn(); }
        for entity in &rendered_tiles { commands.entity(entity).despawn(); }
        visual_entities.units.clear();
        visual_entities.buildings.clear();
        *screen = ClientScreen::Title;
        return;
    }

    // Tool selection
    if keys.just_pressed(KeyCode::Digit1) { editor.tool = EditorTool::PaintTerrain; }
    if keys.just_pressed(KeyCode::Digit2) { editor.tool = EditorTool::PlaceUnit; }
    if keys.just_pressed(KeyCode::Digit3) { editor.tool = EditorTool::PlaceBuilding; }
    if keys.just_pressed(KeyCode::Digit4) { editor.tool = EditorTool::Erase; }

    // Cycle faction (F)
    if keys.just_pressed(KeyCode::KeyF) {
        editor.faction_idx = (editor.faction_idx + 1) % EDITOR_FACTIONS.len();
    }
    // Cycle unit type (T)
    if keys.just_pressed(KeyCode::KeyT) {
        editor.unit_type_idx = (editor.unit_type_idx + 1) % EDITOR_UNIT_TYPES.len();
    }
    // Cycle building type (B)
    if keys.just_pressed(KeyCode::KeyB) {
        editor.building_type_idx = (editor.building_type_idx + 1) % EDITOR_BUILDING_TYPES.len();
    }

    // Del — clear map (confirm with 2nd press within 2s)
    if keys.just_pressed(KeyCode::Delete) {
        let confirmed = if let Some(t) = editor.del_confirm_timer {
            t < 2.0
        } else {
            false
        };

        if confirmed {
            editor.del_confirm_timer = None;
            // Clear all units, buildings, tiles, rendered tiles
            for (entity, _) in &tiles { commands.entity(entity).despawn(); }
            for entity in &units { commands.entity(entity).despawn(); }
            for entity in &buildings { commands.entity(entity).despawn(); }
            for entity in &rendered_tiles { commands.entity(entity).despawn(); }
            visual_entities.units.clear();
            visual_entities.buildings.clear();
            // Respawn blank 48×28 grass map
            for y in 0..28_i32 {
                for x in 0..48_i32 {
                    commands.spawn(Tile {
                        pos: GridPos { x, y },
                        terrain_type: cindertide::map::TerrainType::Grass,
                        cover: cindertide::map::CoverDensity::None,
                    });
                }
            }
        } else {
            editor.del_confirm_timer = Some(0.0);
        }
    }

    // Tick del confirm timer
    if let Some(ref mut t) = editor.del_confirm_timer {
        *t += time.delta_secs();
        if *t >= 2.0 {
            editor.del_confirm_timer = None;
        }
    }

    // Ctrl+S — save map
    if ctrl && keys.just_pressed(KeyCode::KeyS) {
        let mut saved_tiles: Vec<SavedTile> = Vec::new();
        for (_, tile) in &tiles {
            saved_tiles.push(SavedTile {
                x: tile.pos.x,
                y: tile.pos.y,
                terrain: terrain_type_name(&tile.terrain_type).to_string(),
            });
        }

        let saved_units: Vec<SavedUnit> = Vec::new(); // units queried separately
        let saved_buildings: Vec<SavedBuilding> = Vec::new();

        let map = SavedMap {
            tiles: saved_tiles,
            units: saved_units,
            buildings: saved_buildings,
        };

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let path = format!("assets/maps/custom_{}.toml", timestamp);
        if let Ok(content) = toml::to_string(&map) {
            if let Err(e) = std::fs::create_dir_all("assets/maps") {
                eprintln!("editor: failed to create maps dir: {e}");
            } else if let Err(e) = std::fs::write(&path, content) {
                eprintln!("editor: failed to save map: {e}");
            } else {
                info!("editor: saved map to {path}");
            }
        }
    }

    // Ctrl+L — load most recent custom map
    if ctrl && keys.just_pressed(KeyCode::KeyL) {
        let latest = find_latest_custom_map();
        if let Some(path) = latest {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(map) = toml::from_str::<SavedMap>(&content) {
                    // Wipe current
                    for (entity, _) in &tiles { commands.entity(entity).despawn(); }
                    for entity in &units { commands.entity(entity).despawn(); }
                    for entity in &buildings { commands.entity(entity).despawn(); }
                    for entity in &rendered_tiles { commands.entity(entity).despawn(); }
                    visual_entities.units.clear();
                    visual_entities.buildings.clear();

                    // Spawn loaded tiles
                    for st in &map.tiles {
                        if let Some(terrain) = parse_terrain_name(&st.terrain) {
                            commands.spawn(Tile {
                                pos: GridPos { x: st.x, y: st.y },
                                terrain_type: terrain,
                                cover: cindertide::map::CoverDensity::None,
                            });
                        }
                    }

                    // Spawn loaded units
                    for su in &map.units {
                        let faction = parse_faction_name(&su.faction).unwrap_or(Faction::Combine);
                        spawn_editor_unit(&mut commands, su.x, su.y, faction, &su.unit_type);
                    }

                    // Spawn loaded buildings
                    for sb in &map.buildings {
                        let faction = parse_faction_name(&sb.faction).unwrap_or(Faction::Combine);
                        spawn_editor_building(&mut commands, sb.x, sb.y, faction, &sb.building_type);
                    }

                    info!("editor: loaded map from {path}");
                }
            }
        }
    }
}

fn find_latest_custom_map() -> Option<String> {
    let dir = std::fs::read_dir("assets/maps").ok()?;
    let mut entries: Vec<_> = dir
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name().to_string_lossy().starts_with("custom_")
                && e.file_name().to_string_lossy().ends_with(".toml")
        })
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
    entries.first().map(|e| e.path().to_string_lossy().to_string())
}

fn terrain_type_name(t: &cindertide::map::TerrainType) -> &'static str {
    use cindertide::map::TerrainType;
    match t {
        TerrainType::Grass => "Grass",
        TerrainType::Road => "Road",
        TerrainType::Forest => "Forest",
        TerrainType::Rubble => "Rubble",
        TerrainType::Mud => "Mud",
        TerrainType::Corrupted => "Corrupted",
        TerrainType::Void => "Void",
    }
}

fn parse_terrain_name(s: &str) -> Option<cindertide::map::TerrainType> {
    use cindertide::map::TerrainType;
    match s {
        "Grass" => Some(TerrainType::Grass),
        "Road" => Some(TerrainType::Road),
        "Forest" => Some(TerrainType::Forest),
        "Rubble" => Some(TerrainType::Rubble),
        "Mud" => Some(TerrainType::Mud),
        "Corrupted" => Some(TerrainType::Corrupted),
        "Void" => Some(TerrainType::Void),
        _ => None,
    }
}

fn parse_faction_name(s: &str) -> Option<Faction> {
    match s {
        "Combine" => Some(Faction::Combine),
        "Ironborn" => Some(Faction::Ironborn),
        "Covenant" => Some(Faction::Covenant),
        "Hollow" => Some(Faction::Hollow),
        _ => None,
    }
}

fn unit_type_name(t: &UnitType) -> &'static str {
    match t {
        UnitType::Riflemen => "Riflemen",
        UnitType::HeavyWeapons => "HeavyWeapons",
        UnitType::LightVehicle => "LightVehicle",
        UnitType::HeavyArmor => "HeavyArmor",
    }
}

fn building_type_name(t: &BuildingType) -> &'static str {
    match t {
        BuildingType::Barracks => "Barracks",
        BuildingType::Refinery => "Refinery",
        BuildingType::CommandBunker => "CommandBunker",
        BuildingType::MotorPool => "MotorPool",
        BuildingType::Pillbox => "Pillbox",
        BuildingType::Watchtower => "Watchtower",
        BuildingType::Scrapyard => "Scrapyard",
        BuildingType::RecruitmentOffice => "RecruitmentOffice",
        BuildingType::Foundry => "Foundry",
        BuildingType::Airfield => "Airfield",
        BuildingType::Workshop => "Workshop",
        BuildingType::ResearchLab => "ResearchLab",
        BuildingType::SupplyDepot => "SupplyDepot",
        BuildingType::RepairBay => "RepairBay",
        BuildingType::AAGun => "AAGun",
        BuildingType::TankTrap => "TankTrap",
    }
}

fn spawn_editor_unit(commands: &mut Commands, x: i32, y: i32, faction: Faction, type_name: &str) {
    use cindertide::units::*;
    use cindertide::combat::*;
    match type_name {
        "HeavyWeapons" => { commands.spawn(HeavyWeaponsBundle::with_faction(x, y, faction)); }
        "LightVehicle"  => { commands.spawn(LightVehicleBundle::with_faction(x, y, faction)); }
        "HeavyArmor"    => { commands.spawn(HeavyArmorBundle::with_faction(x, y, faction)); }
        _               => { commands.spawn(RiflemanBundle::with_faction(x, y, faction)); }
    }
}

fn spawn_editor_building(commands: &mut Commands, x: i32, y: i32, faction: Faction, type_name: &str) {
    use cindertide::buildings::BuildingBundle;
    let bt = match type_name {
        "Refinery" => BuildingType::Refinery,
        "CommandBunker" => BuildingType::CommandBunker,
        "MotorPool" => BuildingType::MotorPool,
        "Pillbox" => BuildingType::Pillbox,
        "Watchtower" => BuildingType::Watchtower,
        "Scrapyard" => BuildingType::Scrapyard,
        "RecruitmentOffice" => BuildingType::RecruitmentOffice,
        "Foundry" => BuildingType::Foundry,
        "Airfield" => BuildingType::Airfield,
        "Workshop" => BuildingType::Workshop,
        "ResearchLab" => BuildingType::ResearchLab,
        "SupplyDepot" => BuildingType::SupplyDepot,
        "RepairBay" => BuildingType::RepairBay,
        "AAGun" => BuildingType::AAGun,
        "TankTrap" => BuildingType::TankTrap,
        _ => BuildingType::Barracks,
    };
    commands.spawn(BuildingBundle::new(bt, faction, x, y));
}

/// Handle mouse clicks in the map editor.
fn handle_editor_mouse_input(
    mut commands: Commands,
    screen: Res<ClientScreen>,
    editor: Res<EditorState>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<IsometricCamera>>,
    mut tiles: Query<(Entity, &mut Tile)>,
    units: Query<(Entity, &UnitPos), With<UnitType>>,
    buildings: Query<(Entity, &BuildingPos), With<BuildingType>>,
    mut visual_entities: ResMut<VisualEntities>,
    rendered_tiles: Query<(Entity, &RenderedTile)>,
) {
    if *screen != ClientScreen::MapEditor {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_transform)) = cameras.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };
    let Some((gx, gy)) = screen_to_grid(cursor_pos, camera, cam_transform) else { return };

    let left_click = buttons.just_pressed(MouseButton::Left);
    let right_click = buttons.just_pressed(MouseButton::Right);

    match &editor.tool {
        EditorTool::PaintTerrain => {
            if left_click {
                let terrain = EDITOR_TERRAINS[editor.terrain_idx].clone();
                // Update existing tile or spawn new
                if let Some((_, mut tile)) = tiles.iter_mut().find(|(_, t)| t.pos.x == gx && t.pos.y == gy) {
                    tile.terrain_type = terrain;
                } else {
                    commands.spawn(Tile {
                        pos: GridPos { x: gx, y: gy },
                        terrain_type: terrain,
                        cover: cindertide::map::CoverDensity::None,
                    });
                }
            }
            if right_click {
                // Cycle terrain type via commands (just update EditorState)
                // We can't mutate EditorState here due to borrow; cycle is done in keyboard handler.
                // Instead cycle by right-click too — we do that in the keyboard system.
                // This right-click cycles terrain index — but EditorState is immutable here.
                // Workaround: just paint with next terrain without mutating state.
                // Actually we need mutable editor. We'll handle right-click terrain cycling in keyboard.
                // For now, right-click paints with the *next* terrain (preview).
            }
        }
        EditorTool::PlaceUnit => {
            if left_click {
                let faction = EDITOR_FACTIONS[editor.faction_idx].clone();
                let unit_type = EDITOR_UNIT_TYPES[editor.unit_type_idx].clone();
                // Only place if tile exists
                if tiles.iter().any(|(_, t)| t.pos.x == gx && t.pos.y == gy) {
                    spawn_editor_unit(&mut commands, gx, gy, faction, unit_type_name(&unit_type));
                }
            }
        }
        EditorTool::PlaceBuilding => {
            if left_click {
                let faction = EDITOR_FACTIONS[editor.faction_idx].clone();
                let building_type = EDITOR_BUILDING_TYPES[editor.building_type_idx].clone();
                if tiles.iter().any(|(_, t)| t.pos.x == gx && t.pos.y == gy) {
                    spawn_editor_building(&mut commands, gx, gy, faction, building_type_name(&building_type));
                }
            }
        }
        EditorTool::Erase => {
            if left_click {
                // Remove unit or building at tile
                for (entity, pos) in &units {
                    if pos.pos.x == gx && pos.pos.y == gy {
                        if let Some(&vis_entity) = visual_entities.units.get(&entity) {
                            commands.entity(vis_entity).despawn();
                        }
                        visual_entities.units.remove(&entity);
                        commands.entity(entity).despawn();
                    }
                }
                for (entity, pos) in &buildings {
                    if pos.pos.x == gx && pos.pos.y == gy {
                        if let Some(&vis_entity) = visual_entities.buildings.get(&entity) {
                            commands.entity(vis_entity).despawn();
                        }
                        visual_entities.buildings.remove(&entity);
                        commands.entity(entity).despawn();
                    }
                }
            }
            if right_click {
                // Remove tile entirely
                for (entity, tile) in &tiles {
                    if tile.pos.x == gx && tile.pos.y == gy {
                        // Also despawn rendered tile
                        for (rt_entity, rt) in &rendered_tiles {
                            if rt.pos == tile.pos {
                                commands.entity(rt_entity).despawn();
                            }
                        }
                        commands.entity(entity).despawn();
                    }
                }
            }
        }
    }
}

/// Update the editor panel text with current state info.
fn update_editor_panel(
    screen: Res<ClientScreen>,
    editor: Res<EditorState>,
    tiles: Query<&Tile>,
    units: Query<&UnitPos, With<UnitType>>,
    buildings: Query<&BuildingPos, With<BuildingType>>,
    mut panel_vis: Query<&mut Visibility, With<EditorPanel>>,
    mut panel_text: Query<&mut Text, With<EditorPanelText>>,
) {
    let Ok(mut vis) = panel_vis.single_mut() else { return };

    if *screen != ClientScreen::MapEditor {
        *vis = Visibility::Hidden;
        return;
    }
    *vis = Visibility::Visible;

    if !screen.is_changed() && !editor.is_changed() {
        return;
    }

    let Ok(mut text) = panel_text.single_mut() else { return };

    let tool_name = match &editor.tool {
        EditorTool::PaintTerrain => "1: Paint Terrain",
        EditorTool::PlaceUnit    => "2: Place Unit",
        EditorTool::PlaceBuilding => "3: Place Building",
        EditorTool::Erase        => "4: Erase",
    };

    let terrain_name = terrain_type_name(&EDITOR_TERRAINS[editor.terrain_idx]);
    let faction_name_str = match &EDITOR_FACTIONS[editor.faction_idx] {
        Faction::Combine  => "Combine",
        Faction::Ironborn => "Ironborn",
        Faction::Covenant => "Covenant",
        Faction::Hollow   => "Hollow",
    };
    let unit_name = unit_type_name(&EDITOR_UNIT_TYPES[editor.unit_type_idx]);
    let building_name = building_type_name(&EDITOR_BUILDING_TYPES[editor.building_type_idx]);

    let tile_count = tiles.iter().count();
    let unit_count = units.iter().count();
    let building_count = buildings.iter().count();

    let del_hint = if editor.del_confirm_timer.is_some() {
        "\n[Del again to confirm clear]"
    } else {
        ""
    };

    **text = format!(
        "Tool: {tool_name}\n\nTerrain: {terrain_name}\nFaction: {faction_name_str}\nUnit: {unit_name}\nBuilding: {building_name}\n\nTiles: {tile_count}\nUnits: {unit_count}\nBuildings: {building_count}\n\n--- Keys ---\n1-4: tool\nF: faction\nT: unit type\nB: building\nR-click: cycle terrain\nDel: clear map\nCtrl+S: save\nCtrl+L: load\nEsc: exit{del_hint}"
    );
}

// ── HUD systems ───────────────────────────────────────────────────────────────

/// Show/hide the entire HUD root based on ClientScreen.
fn update_hud_visibility(
    screen: Res<ClientScreen>,
    mut hud_vis: Query<&mut Visibility, With<HudRoot>>,
) {
    if !screen.is_changed() {
        return;
    }
    let in_mission = *screen == ClientScreen::InMission;
    for mut vis in &mut hud_vis {
        *vis = if in_mission { Visibility::Visible } else { Visibility::Hidden };
    }
}

/// Update the top resource bar with player faction's current resources.
fn update_resource_bar(
    screen: Res<ClientScreen>,
    player_faction: Option<Res<PlayerFaction>>,
    faction_entities: Query<(&FactionEntity, &ResourcePool)>,
    mut text_q: Query<&mut Text, With<ResourceBarText>>,
) {
    if *screen != ClientScreen::InMission {
        return;
    }
    let Ok(mut text) = text_q.single_mut() else { return };
    let Some(pf) = player_faction else { return };

    // Find the FactionEntity that matches the player faction
    for (fe, pool) in &faction_entities {
        if fe.faction == pf.0 {
            **text = format!(
                "FUEL: {:.0}   SCRAP: {:.0}   MANPOWER: {:.0}",
                pool.fuel, pool.scrap, pool.manpower
            );
            return;
        }
    }
}

/// Update the bottom unit info panel for selected units.
fn update_unit_info_panel(
    screen: Res<ClientScreen>,
    selected: Res<SelectedUnits>,
    units: Query<(
        &UnitType,
        Option<&Health>,
        Option<&MoveTarget>,
        Option<&AttackTarget>,
        Option<&HoldPosition>,
        Option<&AbilityCooldowns>,
        Option<&Suppressed>,
    ), With<UnitType>>,
    mut text_q: Query<&mut Text, With<UnitInfoText>>,
) {
    if *screen != ClientScreen::InMission {
        return;
    }
    let Ok(mut text) = text_q.single_mut() else { return };

    if selected.entities.is_empty() {
        **text = String::new();
        return;
    }

    let count = selected.entities.len();

    if count == 1 {
        let entity = selected.entities[0];
        if let Ok((unit_type, health, move_target, attack_target, hold, ability_cds, suppressed)) = units.get(entity) {
            let type_name = match unit_type {
                UnitType::Riflemen => "Riflemen",
                UnitType::HeavyWeapons => "Heavy Weapons",
                UnitType::LightVehicle => "Light Vehicle",
                UnitType::HeavyArmor => "Heavy Armor",
            };

            let health_str = if let Some(h) = health {
                let frac = (h.current / h.max).clamp(0.0, 1.0);
                let filled = (frac * 20.0).round() as usize;
                let empty = 20 - filled;
                format!(
                    "HP: {:.0}/{:.0} [{}{}]",
                    h.current, h.max,
                    "#".repeat(filled),
                    "-".repeat(empty)
                )
            } else {
                "HP: --".to_string()
            };

            let order = if hold.is_some() {
                "Holding"
            } else if attack_target.is_some() {
                "Attacking"
            } else if move_target.is_some() {
                "Moving"
            } else {
                "Idle"
            };

            let q_cd_str = if let Some(cds) = ability_cds {
                if cds.slots[0] <= 0.0 { "Q:ready".to_string() }
                else { format!("Q:{:.1}s", cds.slots[0]) }
            } else {
                "Q:ready".to_string()
            };
            let suppressed_str = if suppressed.is_some() { " [SUPPRESSED]" } else { "" };

            **text = format!("{}{}\n{}\nOrder: {}  {}", type_name, suppressed_str, health_str, order, q_cd_str);
        } else {
            **text = String::new();
        }
    } else {
        // Multiple units selected: show count + aggregate health
        let mut total_hp = 0.0f32;
        let mut total_max = 0.0f32;
        let mut valid = 0usize;

        for &entity in &selected.entities {
            if let Ok((_, health, _, _, _, _, _)) = units.get(entity) {
                if let Some(h) = health {
                    total_hp += h.current;
                    total_max += h.max;
                    valid += 1;
                }
            }
        }

        let health_str = if valid > 0 && total_max > 0.0 {
            let frac = (total_hp / total_max).clamp(0.0, 1.0);
            let filled = (frac * 20.0).round() as usize;
            let empty = 20 - filled;
            format!(
                "HP: {:.0}/{:.0} [{}{}]",
                total_hp, total_max,
                "#".repeat(filled),
                "-".repeat(empty)
            )
        } else {
            "HP: --".to_string()
        };

        **text = format!("{} units selected\n{}", count, health_str);
    }
}

/// Update the bottom-right production queue panel for player-faction buildings.
fn update_production_queue(
    screen: Res<ClientScreen>,
    player_faction: Option<Res<PlayerFaction>>,
    buildings: Query<(&BuildingType, &Faction, &ProductionQueue), With<Built>>,
    mut text_q: Query<&mut Text, With<ProductionQueueText>>,
) {
    if *screen != ClientScreen::InMission {
        return;
    }
    let Ok(mut text) = text_q.single_mut() else { return };
    let Some(pf) = player_faction else { return };

    let mut lines: Vec<String> = Vec::new();

    for (bt, faction, queue) in &buildings {
        if *faction != pf.0 {
            continue;
        }
        if queue.jobs.is_empty() {
            continue;
        }

        let building_name = match bt {
            BuildingType::Barracks => "Barracks",
            BuildingType::MotorPool => "Motor Pool",
            _ => continue, // only show producing buildings
        };

        let producing = match &queue.jobs[0] {
            UnitType::Riflemen => "Riflemen",
            UnitType::HeavyWeapons => "Heavy Weapons",
            UnitType::LightVehicle => "Light Vehicle",
            UnitType::HeavyArmor => "Heavy Armor",
        };

        let duration = unit_production_seconds(&queue.jobs[0]);
        let frac = (queue.progress / duration).clamp(0.0, 1.0);
        let filled = (frac * 16.0).round() as usize;
        let empty = 16 - filled;
        let bar = format!("[{}{}]", "#".repeat(filled), "-".repeat(empty));

        let queue_count = queue.jobs.len();
        let queue_str = if queue_count > 1 {
            format!(" (+{})", queue_count - 1)
        } else {
            String::new()
        };

        lines.push(format!(
            "{}: {}{}\n{} {:.0}%",
            building_name, producing, queue_str,
            bar, frac * 100.0
        ));
    }

    **text = if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n\n")
    };
}

// ── Minimap system ────────────────────────────────────────────────────────────

const MINIMAP_W: f32 = 200.0;
const MINIMAP_H: f32 = 140.0;
const DOT_SIZE: f32 = 2.0;

/// Rebuild minimap dots at most every 0.5 seconds.
fn update_minimap(
    mut commands: Commands,
    screen: Res<ClientScreen>,
    time: Res<Time>,
    mut timer: ResMut<MinimapTimer>,
    panel_q: Query<Entity, With<MinimapPanel>>,
    dot_q: Query<Entity, With<MinimapDot>>,
    tiles: Query<&Tile>,
    units: Query<(&UnitPos, &Faction), With<UnitType>>,
    buildings: Query<(&BuildingPos, &Faction), With<BuildingType>>,
    player_faction: Option<Res<PlayerFaction>>,
) {
    if *screen != ClientScreen::InMission {
        return;
    }

    timer.0 += time.delta_secs();
    if timer.0 < 0.5 {
        return;
    }
    timer.0 = 0.0;

    // Despawn all existing dots
    for dot_entity in &dot_q {
        commands.entity(dot_entity).despawn();
    }

    let Ok(panel_entity) = panel_q.single() else { return };

    // Determine map bounds
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    for tile in &tiles {
        min_x = min_x.min(tile.pos.x);
        min_y = min_y.min(tile.pos.y);
        max_x = max_x.max(tile.pos.x);
        max_y = max_y.max(tile.pos.y);
    }
    if min_x == i32::MAX {
        return;
    }
    let map_w = (max_x - min_x + 1) as f32;
    let map_h = (max_y - min_y + 1) as f32;
    if map_w <= 0.0 || map_h <= 0.0 {
        return;
    }

    let scale_x = MINIMAP_W / map_w;
    let scale_y = MINIMAP_H / map_h;

    // Helper to convert grid pos to minimap pixel offset
    let to_px = |gx: i32, gy: i32| -> (f32, f32) {
        let px = (gx - min_x) as f32 * scale_x;
        let py = (gy - min_y) as f32 * scale_y;
        (px, py)
    };

    let player_f = player_faction.as_ref().map(|pf| pf.0.clone());

    // Spawn terrain dots
    let mut tile_dots: Vec<(f32, f32, Color)> = Vec::new();
    for tile in &tiles {
        let base = terrain_color_dark(&tile.terrain_type);
        let (px, py) = to_px(tile.pos.x, tile.pos.y);
        tile_dots.push((px, py, base));
    }

    // Spawn building dots (white)
    let mut building_dots: Vec<(f32, f32, Color)> = Vec::new();
    for (bpos, _faction) in &buildings {
        let (px, py) = to_px(bpos.pos.x, bpos.pos.y);
        building_dots.push((px, py, Color::srgb(1.0, 1.0, 1.0)));
    }

    // Spawn unit dots
    let mut unit_dots: Vec<(f32, f32, Color)> = Vec::new();
    for (upos, faction) in &units {
        let color = if Some(faction.clone()) == player_f {
            Color::srgb(1.0, 1.0, 0.0) // yellow for player
        } else {
            Color::srgb(1.0, 0.15, 0.15) // red for enemy
        };
        let (px, py) = to_px(upos.pos.x, upos.pos.y);
        unit_dots.push((px, py, color));
    }

    // Spawn all dots as children of the minimap panel
    let all_dots: Vec<(f32, f32, Color)> = tile_dots
        .into_iter()
        .chain(building_dots)
        .chain(unit_dots)
        .collect();

    commands.entity(panel_entity).with_children(|parent| {
        for (px, py, color) in all_dots {
            parent.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(px),
                    top: Val::Px(py),
                    width: Val::Px(DOT_SIZE),
                    height: Val::Px(DOT_SIZE),
                    ..default()
                },
                BackgroundColor(color),
                MinimapDot,
            ));
        }
    });
}

/// Handle left-clicks on the minimap to move the camera to that map position.
fn handle_minimap_click(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    screen: Res<ClientScreen>,
    tiles: Query<&Tile>,
    mut cameras: Query<&mut Transform, With<IsometricCamera>>,
) {
    if *screen != ClientScreen::InMission {
        return;
    }
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };

    // The minimap is at bottom-left: x in [0, MINIMAP_W], y in [window.height - MINIMAP_H, window.height]
    let win_h = window.height();
    let minimap_top = win_h - MINIMAP_H;

    if cursor.x < 0.0 || cursor.x > MINIMAP_W || cursor.y < minimap_top || cursor.y > win_h {
        return;
    }

    // Map bounds
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    for tile in &tiles {
        min_x = min_x.min(tile.pos.x);
        min_y = min_y.min(tile.pos.y);
        max_x = max_x.max(tile.pos.x);
        max_y = max_y.max(tile.pos.y);
    }
    if min_x == i32::MAX {
        return;
    }
    let map_w = (max_x - min_x + 1) as f32;
    let map_h = (max_y - min_y + 1) as f32;

    let frac_x = cursor.x / MINIMAP_W;
    let frac_y = (cursor.y - minimap_top) / MINIMAP_H;

    let grid_x = min_x as f32 + frac_x * map_w;
    let grid_y = min_y as f32 + frac_y * map_h;

    let world_x = grid_x;
    let world_z = grid_y;

    // Move camera to the clicked position (preserve Y / height)
    if let Ok(mut cam_transform) = cameras.single_mut() {
        cam_transform.translation.x = world_x;
        cam_transform.translation.z = world_z;
    }
}

// ── Mission objectives HUD ────────────────────────────────────────────────────

/// Update the top-right objectives panel each frame during InMission.
fn update_mission_objectives(
    screen: Res<ClientScreen>,
    active: Res<ActiveRun>,
    missions: Query<&Mission>,
    units: Query<&Faction, With<UnitType>>,
    buildings: Query<(&Faction, &BuildingType), With<BuildingPos>>,
    player_faction: Option<Res<PlayerFaction>>,
    mut text_q: Query<&mut Text, With<ObjectivesText>>,
) {
    if *screen != ClientScreen::InMission {
        return;
    }
    let Ok(mut text) = text_q.single_mut() else { return };

    let Some(mission_entity) = active.current_mission_entity else {
        **text = String::new();
        return;
    };
    let Ok(mission) = missions.get(mission_entity) else {
        **text = String::new();
        return;
    };

    let remaining = (mission.deadline - mission.elapsed).max(0.0);
    let remaining_secs = remaining as u32;

    let player_f = player_faction.as_ref().map(|pf| pf.0.clone());
    let enemy_f = Some(mission.opponent_faction.clone());

    let obj_text = match &mission.mission_type {
        MissionType::Control => {
            format!("HOLD THE CENTER\n{} seconds remaining", remaining_secs)
        }
        MissionType::Assault => {
            // Count enemy buildings alive
            let enemy_buildings = buildings
                .iter()
                .filter(|(f, _)| Some((*f).clone()) == enemy_f)
                .count();
            format!("DESTROY ENEMY HQ\nbuildings remaining: {}", enemy_buildings)
        }
        MissionType::Defense => {
            format!("SURVIVE\n{} seconds remaining", remaining_secs)
        }
        MissionType::Extraction => {
            let player_units = units
                .iter()
                .filter(|f| Some((*f).clone()) == player_f)
                .count();
            format!("EXTRACT UNITS\nget {} units to extraction zone", player_units)
        }
        MissionType::Survival => {
            format!("SURVIVE\n{} seconds remaining", remaining_secs)
        }
    };

    **text = obj_text;
}

// ── Fog of War system ─────────────────────────────────────────────────────────

/// Vision radius in tiles for each entity type.
const VISION_INFANTRY: i32 = 6;
const VISION_VEHICLE: i32 = 10;
const VISION_BUILDING: i32 = 8;

/// Update fog of war every 0.25 s while InMission.
fn update_fog_of_war(
    screen: Res<ClientScreen>,
    time: Res<Time>,
    player_faction: Option<Res<PlayerFaction>>,
    units: Query<(&UnitPos, &Faction, &UnitKind), With<UnitType>>,
    buildings: Query<(&BuildingPos, &Faction), With<BuildingType>>,
    mut fog: ResMut<FogOfWar>,
    tiles: Query<&Tile>,
    visual_entities: Res<VisualEntities>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    unit_visuals: Query<(&Faction, &UnitPos), With<UnitType>>,
    building_visuals: Query<(&Faction, &BuildingPos), With<BuildingType>>,
    mut vis_query: Query<(&mut Visibility, Entity)>,
) {
    if *screen != ClientScreen::InMission {
        return;
    }

    fog.timer -= time.delta_secs();
    if fog.timer > 0.0 {
        return;
    }
    fog.timer = 0.25;

    let Some(pf) = player_faction else { return };
    let player_f = &pf.0;

    // Recompute visible set from all player units + buildings.
    let mut new_visible: HashSet<(i32, i32)> = HashSet::new();

    for (pos, faction, kind) in &units {
        if faction != player_f {
            continue;
        }
        let radius = match kind {
            UnitKind::Vehicle => VISION_VEHICLE,
            UnitKind::Infantry => VISION_INFANTRY,
        };
        let cx = pos.pos.x;
        let cy = pos.pos.y;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= radius * radius {
                    new_visible.insert((cx + dx, cy + dy));
                }
            }
        }
    }

    for (bpos, faction) in &buildings {
        if faction != player_f {
            continue;
        }
        let radius = VISION_BUILDING;
        let cx = bpos.pos.x;
        let cy = bpos.pos.y;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= radius * radius {
                    new_visible.insert((cx + dx, cy + dy));
                }
            }
        }
    }

    fog.visible = new_visible;
    // Explored is a superset — never shrinks.
    let newly_seen: Vec<(i32, i32)> = fog.visible.iter().copied().collect();
    for pos in newly_seen {
        fog.explored.insert(pos);
    }

    // Update tile material colors based on fog state.
    for tile in &tiles {
        let key = (tile.pos.x, tile.pos.y);
        let Some(mat_handle) = visual_entities.tile_materials.get(&key) else { continue };
        let Some(mat) = materials.get_mut(mat_handle) else { continue };

        if fog.visible.contains(&key) {
            // Fully visible — normal terrain color.
            mat.base_color = terrain_color(&tile.terrain_type);
        } else if fog.explored.contains(&key) {
            // Shrouded — darkened version of terrain color.
            let c = terrain_color(&tile.terrain_type);
            let LinearRgba { red, green, blue, alpha } = c.to_linear();
            mat.base_color = Color::linear_rgba(red * 0.4, green * 0.4, blue * 0.4, alpha);
        } else {
            // Never seen — nearly black.
            mat.base_color = Color::srgb(0.02, 0.02, 0.02);
        }
    }

    // Hide enemy units/buildings not in visible set; always show player units/buildings.
    // We iterate the visual entity map to find the visual entity and toggle its Visibility.
    // Unit visuals
    for (logic_entity, &vis_entity) in &visual_entities.units {
        // Try to get faction + pos for this logic entity
        if let Ok((faction, pos)) = unit_visuals.get(*logic_entity) {
            let should_show = if faction == player_f {
                true // player units always visible
            } else {
                fog.visible.contains(&(pos.pos.x, pos.pos.y))
            };
            if let Ok((mut visibility, _)) = vis_query.get_mut(vis_entity) {
                *visibility = if should_show { Visibility::Visible } else { Visibility::Hidden };
            }
        }
    }

    // Building visuals
    for (logic_entity, &vis_entity) in &visual_entities.buildings {
        if let Ok((faction, bpos)) = building_visuals.get(*logic_entity) {
            let should_show = if faction == player_f {
                true
            } else {
                fog.visible.contains(&(bpos.pos.x, bpos.pos.y))
            };
            if let Ok((mut visibility, _)) = vis_query.get_mut(vis_entity) {
                *visibility = if should_show { Visibility::Visible } else { Visibility::Hidden };
            }
        }
    }
}

// ── Color helpers ──────────────────────────────────────────────────────────────

fn terrain_color_dark(terrain: &cindertide::map::TerrainType) -> Color {
    use cindertide::map::TerrainType;
    match terrain {
        TerrainType::Grass     => Color::srgb(0.12, 0.22, 0.08),
        TerrainType::Road      => Color::srgb(0.18, 0.18, 0.18),
        TerrainType::Forest    => Color::srgb(0.05, 0.15, 0.05),
        TerrainType::Rubble    => Color::srgb(0.22, 0.20, 0.18),
        TerrainType::Mud       => Color::srgb(0.20, 0.13, 0.06),
        TerrainType::Corrupted => Color::srgb(0.28, 0.04, 0.28),
        _                      => Color::srgb(0.20, 0.20, 0.20),
    }
}

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

// ── Unit ability system ───────────────────────────────────────────────────────

/// Q/W/E ability cooldown durations in seconds per UnitType.
fn ability_q_cooldown(unit_type: &UnitType) -> f32 {
    match unit_type {
        UnitType::Riflemen    => 15.0,
        UnitType::HeavyWeapons => 20.0,
        UnitType::LightVehicle => 10.0,
        UnitType::HeavyArmor  => 8.0,
    }
}

/// Fire Q ability for the given unit.
fn fire_ability_q(
    commands: &mut Commands,
    entity: Entity,
    unit_type: &UnitType,
    unit_pos: &cindertide::units::UnitPos,
    enemies: &[(Entity, cindertide::units::UnitPos)],
) {
    match unit_type {
        UnitType::Riflemen => {
            // Suppressing Fire: apply Suppressed for 5s to the nearest enemy
            let nearest = enemies.iter().min_by_key(|(_, epos)| {
                let dx = (epos.pos.x - unit_pos.pos.x).abs();
                let dy = (epos.pos.y - unit_pos.pos.y).abs();
                dx.max(dy)
            });
            if let Some((target_entity, _)) = nearest {
                if let Ok(mut e) = commands.get_entity(*target_entity) {
                    e.insert(Suppressed { remaining: 5.0 });
                }
            }
            info!("Riflemen: Suppressing Fire");
        }
        UnitType::HeavyWeapons => {
            // Grenade: deal 3× damage to all units on the nearest enemy tile
            let nearest = enemies.iter().min_by_key(|(_, epos)| {
                let dx = (epos.pos.x - unit_pos.pos.x).abs();
                let dy = (epos.pos.y - unit_pos.pos.y).abs();
                dx.max(dy)
            });
            if let Some((_, target_pos)) = nearest {
                let tx = target_pos.pos.x;
                let ty = target_pos.pos.y;
                // Apply Suppressed (as a grenade effect stand-in) to all on that tile
                for (enemy_entity, epos) in enemies {
                    if epos.pos.x == tx && epos.pos.y == ty {
                        if let Ok(mut e) = commands.get_entity(*enemy_entity) {
                            e.insert(Suppressed { remaining: 3.0 });
                        }
                    }
                }
            }
            info!("HeavyWeapons: Grenade");
        }
        UnitType::LightVehicle => {
            // Scout Dash: move 3 tiles in current facing direction (just teleport for now)
            let new_pos = cindertide::map::GridPos {
                x: unit_pos.pos.x,
                y: unit_pos.pos.y - 3, // default: north
            };
            if let Ok(mut e) = commands.get_entity(entity) {
                e.insert(cindertide::units::UnitPos { pos: new_pos });
            }
            info!("LightVehicle: Scout Dash");
        }
        UnitType::HeavyArmor => {
            // Rally: remove Suppressed/Routing from self
            if let Ok(mut e) = commands.get_entity(entity) {
                e.remove::<Suppressed>()
                 .remove::<cindertide::combat::Routing>();
            }
            info!("HeavyArmor: Rally");
        }
    }
}

/// Handle Q/W/E ability keys for selected units.
fn handle_ability_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    screen: Res<ClientScreen>,
    selected: Res<SelectedUnits>,
    tech_vis: Res<TechPanelVisible>,
    mut units: Query<(
        &UnitType,
        &cindertide::units::UnitPos,
        Option<&mut AbilityCooldowns>,
    )>,
    enemies_q: Query<(Entity, &cindertide::units::UnitPos, &Faction), With<UnitType>>,
    player_faction: Option<Res<PlayerFaction>>,
) {
    if *screen != ClientScreen::InMission || tech_vis.visible {
        return;
    }
    if selected.entities.is_empty() {
        return;
    }

    let q_pressed = keys.just_pressed(KeyCode::KeyQ);
    // W and E reserved for future abilities; add placeholders
    if !q_pressed {
        return;
    }

    let Some(pf) = player_faction else { return };

    // Collect enemy positions for ability targeting
    let enemies: Vec<(Entity, cindertide::units::UnitPos)> = enemies_q
        .iter()
        .filter(|(_, _, f)| **f != pf.0)
        .map(|(e, pos, _)| (e, pos.clone()))
        .collect();

    // Clone selected list to avoid borrow issues
    let selected_ents: Vec<Entity> = selected.entities.clone();

    for &sel_entity in &selected_ents {
        // We need to handle the mutable borrow carefully
        let Ok((unit_type, unit_pos, ability_cds)) = units.get_mut(sel_entity) else { continue };
        let unit_type = unit_type.clone();
        let unit_pos = unit_pos.clone();

        let cooldown_dur = ability_q_cooldown(&unit_type);

        // Check if cooldown is ready
        let ready = if let Some(ref cds) = ability_cds {
            cds.slots[0] <= 0.0
        } else {
            true // no AbilityCooldowns component = always ready
        };

        if !ready {
            info!("Ability Q not ready");
            continue;
        }

        // Fire the ability
        fire_ability_q(&mut commands, sel_entity, &unit_type, &unit_pos, &enemies);

        // Set cooldown - insert or update component
        if let Some(mut cds) = ability_cds {
            cds.slots[0] = cooldown_dur;
        } else {
            let mut cds = AbilityCooldowns::default();
            cds.slots[0] = cooldown_dur;
            if let Ok(mut e) = commands.get_entity(sel_entity) {
                e.insert(cds);
            }
        }
    }
}

// ── Tech tree panel ───────────────────────────────────────────────────────────

/// All available tech items displayed in the tech tree panel.
struct TechItem {
    name: &'static str,
    description: &'static str,
    target: TechResearchTarget,
    cost_fuel: f32,
    cost_scrap: f32,
}

#[derive(Clone)]
enum TechResearchTarget {
    TierTwo,
    TierThree,
    DoctrineAssault,
    DoctrineFortification,
    DoctrineSalvage,
}

fn all_tech_items() -> Vec<TechItem> {
    vec![
        TechItem {
            name: "Tier II Upgrades",
            description: "Unlock tier 2 units and buildings",
            target: TechResearchTarget::TierTwo,
            cost_fuel: 200.0,
            cost_scrap: 200.0,
        },
        TechItem {
            name: "Tier III Upgrades",
            description: "Unlock tier 3 units and buildings (requires Tier II)",
            target: TechResearchTarget::TierThree,
            cost_fuel: 500.0,
            cost_scrap: 400.0,
        },
        TechItem {
            name: "Assault Doctrine",
            description: "+25% attack damage; units more aggressive",
            target: TechResearchTarget::DoctrineAssault,
            cost_fuel: 100.0,
            cost_scrap: 150.0,
        },
        TechItem {
            name: "Fortification Doctrine",
            description: "+50% cover effectiveness; buildings more durable",
            target: TechResearchTarget::DoctrineFortification,
            cost_fuel: 100.0,
            cost_scrap: 150.0,
        },
        TechItem {
            name: "Salvage Doctrine",
            description: "+30% resource income from kills",
            target: TechResearchTarget::DoctrineSalvage,
            cost_fuel: 100.0,
            cost_scrap: 150.0,
        },
    ]
}

/// Get status string for a tech item given current Tech state.
fn tech_item_status(item: &TechItem, tier: Tier, doctrine: Option<Doctrine>, researching: bool, fuel: f32, scrap: f32) -> String {
    let is_done = match &item.target {
        TechResearchTarget::TierTwo => tier == Tier::Two || tier == Tier::Three,
        TechResearchTarget::TierThree => tier == Tier::Three,
        TechResearchTarget::DoctrineAssault => doctrine == Some(Doctrine::Assault),
        TechResearchTarget::DoctrineFortification => doctrine == Some(Doctrine::Fortification),
        TechResearchTarget::DoctrineSalvage => doctrine == Some(Doctrine::Salvage),
    };
    if is_done {
        return "[DONE]".to_string();
    }

    let locked = match &item.target {
        TechResearchTarget::TierTwo => false,
        TechResearchTarget::TierThree => tier == Tier::One,
        TechResearchTarget::DoctrineAssault |
        TechResearchTarget::DoctrineFortification |
        TechResearchTarget::DoctrineSalvage => doctrine.is_some(),
    };
    if locked {
        return "[LOCKED]".to_string();
    }

    if researching {
        return "[RESEARCHING...]".to_string();
    }

    let can_afford = fuel >= item.cost_fuel && scrap >= item.cost_scrap;
    if can_afford {
        "[AVAILABLE — press Enter]".to_string()
    } else {
        "[NEED MORE RESOURCES]".to_string()
    }
}

/// Toggle tech panel on T key and handle navigation/research.
fn update_tech_panel(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    screen: Res<ClientScreen>,
    mut tech_vis: ResMut<TechPanelVisible>,
    mut panel_query: Query<&mut Visibility, With<TechPanel>>,
    mut text_query: Query<&mut Text, With<TechPanelText>>,
    player_faction: Option<Res<PlayerFaction>>,
    mut tech_query: Query<(Entity, &mut Tech, Option<&ResearchInProgress>, &cindertide::resources::FactionEntity, &mut ResourcePool)>,
) {
    if *screen != ClientScreen::InMission {
        if tech_vis.visible {
            tech_vis.visible = false;
            for mut vis in &mut panel_query { *vis = Visibility::Hidden; }
        }
        return;
    }

    let up = keys.just_pressed(KeyCode::ArrowUp);
    let down = keys.just_pressed(KeyCode::ArrowDown);
    let enter = keys.just_pressed(KeyCode::Enter);
    let t_key = keys.just_pressed(KeyCode::KeyT);

    if t_key {
        tech_vis.visible = !tech_vis.visible;
        let vis = if tech_vis.visible { Visibility::Visible } else { Visibility::Hidden };
        for mut v in &mut panel_query { *v = vis; }
    }

    if !tech_vis.visible {
        return;
    }

    let items = all_tech_items();
    let count = items.len();

    if up && tech_vis.selected_idx > 0 {
        tech_vis.selected_idx -= 1;
    }
    if down && tech_vis.selected_idx < count - 1 {
        tech_vis.selected_idx += 1;
    }

    let Some(pf) = player_faction else { return };

    // Collect read-only state first
    struct FactionTechState {
        entity: Entity,
        tier: Tier,
        doctrine: Option<Doctrine>,
        researching: bool,
        fuel: f32,
        scrap: f32,
    }

    let state: Option<FactionTechState> = tech_query.iter().find_map(|(ent, tech, rp, fe, pool)| {
        if fe.faction == pf.0 {
            Some(FactionTechState {
                entity: ent,
                tier: tech.tier,
                doctrine: tech.doctrine,
                researching: rp.is_some(),
                fuel: pool.fuel,
                scrap: pool.scrap,
            })
        } else {
            None
        }
    });

    // Handle Enter to research
    if enter {
        if let Some(ref s) = state {
            let item = &items[tech_vis.selected_idx];
            let can_afford = s.fuel >= item.cost_fuel && s.scrap >= item.cost_scrap;
            if can_afford && !s.researching {
                if let Ok((ent, mut tech, _, _, mut pool)) = tech_query.get_mut(s.entity) {
                    let target = match &item.target {
                        TechResearchTarget::TierTwo => ResearchTarget::Tier(Tier::Two),
                        TechResearchTarget::TierThree => ResearchTarget::Tier(Tier::Three),
                        TechResearchTarget::DoctrineAssault => ResearchTarget::Doctrine(Doctrine::Assault),
                        TechResearchTarget::DoctrineFortification => ResearchTarget::Doctrine(Doctrine::Fortification),
                        TechResearchTarget::DoctrineSalvage => ResearchTarget::Doctrine(Doctrine::Salvage),
                    };
                    match start_research(&mut pool, &tech, false, target) {
                        Ok(new_rp) => {
                            commands.entity(ent).insert(new_rp);
                            info!("Research started successfully");
                        }
                        Err(e) => {
                            info!("Research failed: {:?}", e);
                        }
                    }
                }
            }
        }
    }

    // Build panel text
    let Ok(mut text) = text_query.single_mut() else { return };
    let mut lines = Vec::new();

    if let Some(s) = state {
        let tier_str = match s.tier {
            Tier::One => "I",
            Tier::Two => "II",
            Tier::Three => "III",
        };
        let doctrine_str = match s.doctrine {
            None => "None",
            Some(Doctrine::Assault) => "Assault",
            Some(Doctrine::Fortification) => "Fortification",
            Some(Doctrine::Salvage) => "Salvage",
        };
        lines.push(format!("Tier: {}  Doctrine: {}  |  FUEL: {:.0}  SCRAP: {:.0}", tier_str, doctrine_str, s.fuel, s.scrap));
        lines.push(String::new());

        for (i, item) in items.iter().enumerate() {
            let selected = i == tech_vis.selected_idx;
            let prefix = if selected { "> " } else { "  " };
            let status = tech_item_status(item, s.tier, s.doctrine, s.researching, s.fuel, s.scrap);
            lines.push(format!(
                "{}{} ({:.0}F/{:.0}S) — {}",
                prefix, item.name, item.cost_fuel, item.cost_scrap, status
            ));
            if selected {
                lines.push(format!("    {}", item.description));
            }
        }
        lines.push(String::new());
        lines.push("Up/Down: navigate  |  Enter: research  |  T: close".to_string());
    } else {
        lines.push("No faction tech data found.".to_string());
    }

    **text = lines.join("\n");
}
