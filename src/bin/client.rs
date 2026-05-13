use bevy::asset::io::{AssetSource as BevyAssetSource, file::FileAssetReader};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::render::camera::ScalingMode;
use cindertide::map::{Faction, GridPos, Tile};
use cindertide::units::{UnitPos, UnitTypeId, UnitBundle};
use cindertide::buildings::{BuildingPos, BuildingTypeId, BuildingBundle, building_cost, building_produces, VisionProvider};
use cindertide::factions::LoadedFactions;
use cindertide::campaign::{CampaignRun, CampaignDef, PlayableFaction, GlobalProgress, apply_mission_outcome, next_mission_type, save_progress, load_progress};
use cindertide::game::{ActiveRun, GameState};
use cindertide::mission::{Mission, MissionStatus};
use cindertide::mapgen::MissionType;
use cindertide::mapgen::{ArchetypeDef, SpawnZone, SpawnLayout, generate_from_archetype, scan_archetypes};
use cindertide::narrative::NarrativeData;
use cindertide::resources::{FactionBundle, FactionEntity, ResourcePool};
use cindertide::tech::{Tech, ResearchInProgress, ResearchTarget, Tier, Doctrine, start_research};
use cindertide::combat::{PlayerAttackOrder, AttackMoveOrder, HoldPosition, AttackTarget, Health, Suppressed, AbilityCooldowns, JustDied, Dead};
use cindertide::units::{MoveTarget, MoveProgress, UnitKind};
use cindertide::buildings::Built;
use cindertide::production::{ProductionQueue, unit_production_seconds};
use cindertide::mission_script::{MissionScriptPlugin, ScriptState};
use cindertide::camera::{framing_for, CameraShake, CameraTarget, CinematicFraming};
use cindertide::cutscene_editor::{AssetCacheRoot, CutsceneEditorPlugin};
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
use std::io::{Read as IoRead, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, Receiver, Sender};
use serde::{Serialize, Deserialize};

fn main() {
    // Resolve asset roots from env BEFORE App build — register_asset_source must
    // run before DefaultPlugins so AssetPlugin sees the "cache" source in its registry.
    let (local_root, http_base) = resolve_asset_config();
    let _ = std::fs::create_dir_all(&local_root);
    let reader_root = local_root.clone();

    App::new()
        // WebAssetPlugin is kept for ad-hoc `http://` loads (e.g., debug tools).
        // Game assets flow through the "cache" source instead — see ModelAssets::asset_path.
        .add_plugins(bevy_web_asset::WebAssetPlugin::default())
        .register_asset_source(
            CACHE_SOURCE,
            BevyAssetSource::build()
                .with_reader(move || Box::new(FileAssetReader::new(&reader_root))),
        )
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
        .add_plugins(MissionScriptPlugin)
        .add_plugins(CutsceneEditorPlugin)
        .init_resource::<VisualEntities>()
        .init_resource::<SelectedUnits>()
        .init_resource::<DragState>()
        .init_resource::<AttackMoveMode>()
        .init_resource::<Paused>()
        .init_resource::<ClientScreen>()
        .init_resource::<EditorState>()
        .init_resource::<TechPanelVisible>()
        .init_resource::<FogOfWar>()
        .init_resource::<AudioEventQueue>()
        .init_resource::<AudioAssets>()
        .init_resource::<CameraSnapped>()
        .init_resource::<MultiplayerRole>()
        .init_resource::<NetIdCounter>()
        .init_resource::<RemoteGameState>()
        .insert_resource(AssetCacheRoot(local_root.clone()))
        .insert_resource(ModelAssets { local_root, http_base })
        .init_resource::<EditorEnteredFromGame>()
        .init_resource::<LoadedCampaigns>()
        .init_resource::<CampaignEditorState>()
        .init_resource::<GeneratePanel>()
        .init_resource::<LobbyConfig>()
        .init_resource::<LobbyPreviewState>()
        .init_resource::<PlayerCheats>()
        .insert_resource(MinimapTimer(0.0))
        .insert_resource(NetBroadcastTimer(0.0))
        .insert_resource(cindertide::factions::LoadedFactions::load_from_dir("assets/factions"))
        .add_systems(Startup, setup_scene)
        .add_systems(Startup, setup_ui)
        .add_systems(Startup, load_narrative)
        .add_systems(Startup, startup_load_progress)
        // Prefetch fills the cache dir before any downstream system tries to load a model/audio.
        .add_systems(Startup, prefetch_http_assets)
        .add_systems(Startup, load_audio_assets.after(prefetch_http_assets))
        .add_systems(Startup, startup_load_campaigns)
        .add_systems(Update, render_tiles)
        .add_systems(Update, sync_rendered_tile_colors)
        .add_systems(Update, spawn_unit_visuals)
        .add_systems(Update, sync_unit_positions)
        .add_systems(Update, spawn_building_visuals)
        .add_systems(Update, camera_pan_zoom)
        // attach_cinematic_framing must run before any camera resolver consults the component,
        // and before script_tick_system (which is in MissionScriptPlugin's Update set).
        .add_systems(Update, attach_cinematic_framing)
        // snap_camera_on_mission_start must run before camera_follow_target so the
        // initial CameraTarget is honored on the same frame the home base spawns.
        .add_systems(Update, snap_camera_on_mission_start)
        .add_systems(Update, camera_follow_target.after(snap_camera_on_mission_start).after(camera_pan_zoom))
        // apply_camera_shake jitters the camera AFTER follow_target writes the smoothed pose.
        .add_systems(Update, apply_camera_shake.after(camera_follow_target))
        .add_systems(Update, edge_scroll)
        .add_systems(Update, handle_mouse_input)
        .add_systems(Update, handle_editor_mouse_input)
        .add_systems(Update, sync_selection_rings)
        .add_systems(Update, update_drag_rect)
        .add_systems(Update, handle_keyboard_commands)
        .add_systems(Update, handle_paused_menu_input)
        .add_systems(Update, handle_editor_keyboard)
        .add_systems(Update, handle_editor_open_campaign)
        .add_systems(Update, handle_generate_panel)
        .add_systems(Update, handle_campaign_editor_keyboard)
        .add_systems(Update, update_campaign_editor_overlay)
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
        .add_systems(Update, update_lobby_map_preview)
        .add_systems(Update, update_mission_objectives)
        .add_systems(Update, update_dialogue_bar)
        .add_systems(Update, handle_ability_input)
        .add_systems(Update, update_tech_panel)
        .add_systems(Update, update_fog_of_war)
        .add_systems(Update, handle_unit_death)
        .add_systems(Update, tick_death_flashes)
        .add_systems(Update, process_audio_events)
        .add_systems(Update, host_broadcast_game_state)
        .add_systems(Update, receive_net_messages)
        .add_systems(Update, apply_player_cheat_trickle)
        .run();
}

// ── ClientScreen resource ────────────────────────────────────────────────────

#[derive(Resource, Debug, Clone, PartialEq)]
enum ClientScreen {
    Title,
    MultiplayerMenu { hosting: bool, ip_input: String },
    MultiplayerLobby { hosting: bool, ip_input: String },
    /// Campaign picker — replaces old hardcoded FactionPicker.
    FactionPicker { selected: usize },
    /// Mission select screen — shown when the player has already beaten at least one mission
    /// and can choose which mission to start from.
    MissionSelect { campaign_idx: usize, selected: usize },
    Briefing { title: String, briefing: String },
    InMission,
    /// Running a mission that was started directly from the map editor (P key).
    /// When it ends, return to MapEditor with the saved map path reloaded.
    TestMission { saved_map_path: String },
    Debrief { title: String, text: String, won: bool },
    GameOver { won: bool, handler_unlocked: bool },
    MapEditor,
    CampaignEditor,
    /// Single-player bot match setup — uses LobbyConfig resource, same as MultiplayerLobby but no networking.
    Skirmish,
    PlayerSettings { selected_field: PlayerSettingsField },
}

impl Default for ClientScreen {
    fn default() -> Self {
        ClientScreen::Title
    }
}

// ── Loaded campaigns resource ────────────────────────────────────────────────

/// All campaign definitions loaded from `assets/campaigns/*.toml` at startup.
#[derive(Resource, Default)]
struct LoadedCampaigns(Vec<CampaignDef>);

// ── Campaign editor state ────────────────────────────────────────────────────

#[derive(Resource, Default)]
struct CampaignEditorState {
    /// Index of the selected campaign in the list.
    campaign_selected: usize,
    /// Index of the selected mission within the selected campaign.
    mission_selected: usize,
    /// If true, focus is on the mission list; if false, focus is on the campaign list.
    focus_missions: bool,
    /// Text input buffer (for new campaign id/name or new map path).
    input_buffer: String,
    /// If Some, we are currently prompting for this field.
    input_prompt: Option<CampaignEditorPrompt>,
    /// Status line shown at bottom.
    status: String,
    /// Mutable copy of loaded campaigns for editing.
    campaigns: Vec<CampaignDef>,
    /// Whether a delete confirmation is pending.
    delete_confirm: bool,
    /// New campaign name buffer (used during N: new campaign flow).
    new_name_buffer: String,
}

#[derive(Debug, Clone, PartialEq)]
enum CampaignEditorPrompt {
    NewCampaignId,
    NewCampaignName,
    AddMapPath,
}

// ── Generate panel resource ────────────────────────────────────────────────────

#[derive(Resource)]
struct GeneratePanel {
    open: bool,
    selected: usize,
    archetypes: Vec<ArchetypeDef>,
    width: i32,
    height: i32,
    seed: u64,
    last_spawn_zones: Vec<SpawnZone>,
}

impl Default for GeneratePanel {
    fn default() -> Self {
        Self {
            open: false,
            selected: 0,
            archetypes: Vec::new(),
            width: 128,
            height: 80,
            seed: 42,
            last_spawn_zones: Vec::new(),
        }
    }
}

/// Tag component for spawn zone marker entities in the 3D world.
#[derive(Component)]
struct SpawnMarker {
    zone_id: usize,
}

// ── Map editor tool ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum EditorTool {
    PaintTerrain,
    PlaceUnit,
    PlaceBuilding,
    Erase,
    ScriptEditor,
    CampaignEditor,
}

impl Default for EditorTool {
    fn default() -> Self { EditorTool::PaintTerrain }
}

// ── Script editor data types ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ScriptEventDef {
    id: String,
    trigger_type: String,    // "time" or "beat"
    trigger_seconds: f32,    // used if trigger_type == "time"
    trigger_beat: String,    // used if trigger_type == "beat" e.g. "LastStand"
    actions: Vec<ActionDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ActionDef {
    action_type: String,  // "dialogue", "spawn_units", "objective", "win_mission", "lose_mission"
    text: String,         // for dialogue / objective
    #[serde(default)]
    speaker: String,      // for dialogue (optional speaker tag)
    faction: String,      // for spawn_units
    unit_type: String,    // for spawn_units
    count: u32,
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, PartialEq)]
enum ScriptField {
    TriggerSeconds,
    TriggerBeat,
    ActionType,
    ActionText,
    ActionFaction,
    ActionUnitType,
    ActionCount,
    ActionX,
    ActionY,
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
const EDITOR_FACTION_IDS: &[&str] = &["combine", "ironborn", "covenant", "hollow"];

/// Unit types cycled with T in editor.
const EDITOR_UNIT_TYPE_IDS: &[&str] = &["riflemen", "heavy_weapons", "light_vehicle", "heavy_armor"];

/// Building types cycled with B in editor.
const EDITOR_BUILDING_TYPE_IDS: &[&str] = &[
    "barracks", "refinery", "command_bunker", "motor_pool", "pillbox", "watchtower",
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
    // Script editor state
    script_events: Vec<ScriptEventDef>,
    script_selected: usize,
    script_action_selected: usize,
    script_editing_field: Option<ScriptField>,
    script_field_buffer: String,
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
            script_events: Vec::new(),
            script_selected: 0,
            script_action_selected: 0,
            script_editing_field: None,
            script_field_buffer: String::new(),
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

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct SavedSpawnZone {
    id: usize,
    x: i32,
    y: i32,
    clear_radius: i32,
    suggested_team: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct MapGrid {
    /// One string per row (y). Each char maps to a terrain via the fixed
    /// legend: `.`→grass, `M`→mud, `R`→road, `F`→forest, `U`→rubble,
    /// `C`→corrupted, `V`→void. Unknown chars are skipped.
    rows: Vec<String>,
}

fn grid_char_to_terrain(c: char) -> Option<&'static str> {
    match c {
        '.' => Some("grass"),
        'M' => Some("mud"),
        'R' => Some("road"),
        'F' => Some("forest"),
        'U' => Some("rubble"),
        'C' => Some("corrupted"),
        'V' => Some("void"),
        _ => None,
    }
}

impl MapGrid {
    fn to_tiles(&self) -> Vec<SavedTile> {
        let mut out = Vec::new();
        for (y, row) in self.rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                if let Some(name) = grid_char_to_terrain(ch) {
                    out.push(SavedTile {
                        x: x as i32,
                        y: y as i32,
                        terrain: name.to_string(),
                    });
                }
            }
        }
        out
    }
}

#[derive(Deserialize)]
struct RawSavedMap {
    #[serde(default)]
    tiles: Vec<SavedTile>,
    #[serde(default)]
    grid: Option<MapGrid>,
    #[serde(default)]
    units: Vec<SavedUnit>,
    #[serde(default)]
    buildings: Vec<SavedBuilding>,
    #[serde(default)]
    mission_type: Option<String>,
    #[serde(default)]
    player_faction: Option<String>,
    #[serde(default)]
    opponent_faction: Option<String>,
    #[serde(default)]
    deadline_seconds: Option<f32>,
    #[serde(default)]
    briefing_override: Option<String>,
    #[serde(default)]
    win_override: Option<String>,
    #[serde(default)]
    loss_override: Option<String>,
    #[serde(default)]
    mission_index_override: Option<usize>,
    #[serde(default)]
    script_events: Vec<ScriptEventDef>,
    #[serde(default)]
    spawn_zones: Vec<SavedSpawnZone>,
}

impl From<RawSavedMap> for SavedMap {
    fn from(raw: RawSavedMap) -> Self {
        let mut tiles = raw.tiles;
        if let Some(grid) = raw.grid {
            tiles.extend(grid.to_tiles());
        }
        SavedMap {
            tiles,
            units: raw.units,
            buildings: raw.buildings,
            mission_type: raw.mission_type,
            player_faction: raw.player_faction,
            opponent_faction: raw.opponent_faction,
            deadline_seconds: raw.deadline_seconds,
            briefing_override: raw.briefing_override,
            win_override: raw.win_override,
            loss_override: raw.loss_override,
            mission_index_override: raw.mission_index_override,
            script_events: raw.script_events,
            spawn_zones: raw.spawn_zones,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(from = "RawSavedMap")]
struct SavedMap {
    tiles: Vec<SavedTile>,
    units: Vec<SavedUnit>,
    buildings: Vec<SavedBuilding>,
    /// Optional campaign scripting fields — if set, override mission config.
    #[serde(default)]
    mission_type: Option<String>,
    #[serde(default)]
    player_faction: Option<String>,
    #[serde(default)]
    opponent_faction: Option<String>,
    #[serde(default)]
    deadline_seconds: Option<f32>,
    #[serde(default)]
    briefing_override: Option<String>,
    #[serde(default)]
    win_override: Option<String>,
    #[serde(default)]
    loss_override: Option<String>,
    #[serde(default)]
    mission_index_override: Option<usize>,
    #[serde(default)]
    script_events: Vec<ScriptEventDef>,
    #[serde(default)]
    spawn_zones: Vec<SavedSpawnZone>,
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

/// Marker on the logical Tile entity once its mesh has been spawned.
/// Keeps render_tiles from re-processing tiles every frame without touching
/// the RenderedTile component (which lives on mesh entities and is used by
/// editor/cleanup systems to find and despawn visual meshes).
#[derive(Component)]
struct TileHasMesh;

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

/// Root node of the dialogue bar (above the unit info panel, InMission only).
#[derive(Component)]
struct DialogueBar;

/// Text inside the dialogue bar.
#[derive(Component)]
struct DialogueBarText;

/// Root node of the tech tree overlay panel.
#[derive(Component)]
struct TechPanel;

/// Text inside the tech tree panel.
#[derive(Component)]
struct TechPanelText;

/// Root panel of the lobby map preview (240×160 px, right side of lobby screen).
#[derive(Component)]
struct LobbyMapPreview;

/// Colored dot/tile node inside the lobby map preview.
#[derive(Component)]
struct LobbyMapPreviewDot;

/// Tracks the last rendered map path so preview only rebuilds on map change.
#[derive(Resource, Default)]
struct LobbyPreviewState {
    last_map_path: String,
}

/// Resource tracking tech panel open/close state and selected index.
#[derive(Resource, Default)]
struct TechPanelVisible {
    visible: bool,
    selected_idx: usize,
}

/// Visual death flash spawned when a unit dies. Fades/shrinks over `timer` seconds.
#[derive(Component)]
struct DeathFlash {
    timer: f32,
}

// ── Audio event infrastructure ────────────────────────────────────────────────
// Sounds map to kenney_aio packs hosted on the asset server. Paths are relative
// to the `AssetSource` base resolved at startup. Each AudioEvent gets a single
// pre-loaded Handle<AudioSource>; `process_audio_events` spawns short-lived
// AudioPlayer entities (PlaybackSettings::DESPAWN) for each queued event.

#[derive(Debug, Clone, Copy)]
enum AudioEvent {
    UnitSelected,
    UnitMoved,
    Combat,
    BuildingComplete,
    UiClick,
    MissionStart,
    MissionEnd,
}

#[derive(Resource, Default)]
struct AudioEventQueue(Vec<AudioEvent>);

/// Pre-loaded handles for each `AudioEvent`. Populated by `load_audio_assets` at startup.
/// Stays empty when `AssetSource::Placeholder` — `process_audio_events` no-ops gracefully.
#[derive(Resource, Default)]
struct AudioAssets {
    unit_selected: Option<Handle<AudioSource>>,
    unit_moved: Option<Handle<AudioSource>>,
    combat: Option<Handle<AudioSource>>,
    building_complete: Option<Handle<AudioSource>>,
    ui_click: Option<Handle<AudioSource>>,
    mission_start: Option<Handle<AudioSource>>,
    mission_end: Option<Handle<AudioSource>>,
}

impl AudioAssets {
    fn for_event(&self, event: AudioEvent) -> Option<&Handle<AudioSource>> {
        match event {
            AudioEvent::UnitSelected => self.unit_selected.as_ref(),
            AudioEvent::UnitMoved => self.unit_moved.as_ref(),
            AudioEvent::Combat => self.combat.as_ref(),
            AudioEvent::BuildingComplete => self.building_complete.as_ref(),
            AudioEvent::UiClick => self.ui_click.as_ref(),
            AudioEvent::MissionStart => self.mission_start.as_ref(),
            AudioEvent::MissionEnd => self.mission_end.as_ref(),
        }
    }
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
    /// Fog state from the previous tick — used to skip unchanged tiles.
    prev_visible: HashSet<(i32, i32)>,
    /// Explored set from the previous tick.
    prev_explored: HashSet<(i32, i32)>,
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

// ── Player cheat options ──────────────────────────────────────────────────────

/// Player-side cheat multipliers applied to the human player's faction only.
/// AI uses its own difficulty-config cheats.
#[derive(Resource, Clone, Debug)]
pub struct PlayerCheats {
    pub resource_multiplier: f32,    // 0.5, 0.75, 1.0, 1.5, 2.0
    pub build_speed_multiplier: f32, // 0.5, 0.75, 1.0, 1.5, 2.0
    pub starting_resource_bonus: f32, // 0, 250, 500, 1000
    pub fog_of_war: bool,
}

impl Default for PlayerCheats {
    fn default() -> Self {
        Self {
            resource_multiplier: 1.0,
            build_speed_multiplier: 1.0,
            starting_resource_bonus: 0.0,
            fog_of_war: true,
        }
    }
}

impl PlayerCheats {
    const RESOURCE_STEPS: &'static [f32] = &[0.5, 0.75, 1.0, 1.5, 2.0];
    const BUILD_SPEED_STEPS: &'static [f32] = &[0.5, 0.75, 1.0, 1.5, 2.0];
    const STARTING_BONUS_STEPS: &'static [f32] = &[0.0, 250.0, 500.0, 1000.0];

    fn cycle_resource_multiplier(&mut self, forward: bool) {
        let idx = Self::RESOURCE_STEPS.iter().position(|&v| (v - self.resource_multiplier).abs() < 0.01).unwrap_or(2);
        let new_idx = if forward {
            (idx + 1) % Self::RESOURCE_STEPS.len()
        } else {
            (idx + Self::RESOURCE_STEPS.len() - 1) % Self::RESOURCE_STEPS.len()
        };
        self.resource_multiplier = Self::RESOURCE_STEPS[new_idx];
    }

    fn cycle_build_speed(&mut self, forward: bool) {
        let idx = Self::BUILD_SPEED_STEPS.iter().position(|&v| (v - self.build_speed_multiplier).abs() < 0.01).unwrap_or(2);
        let new_idx = if forward {
            (idx + 1) % Self::BUILD_SPEED_STEPS.len()
        } else {
            (idx + Self::BUILD_SPEED_STEPS.len() - 1) % Self::BUILD_SPEED_STEPS.len()
        };
        self.build_speed_multiplier = Self::BUILD_SPEED_STEPS[new_idx];
    }

    fn cycle_starting_bonus(&mut self, forward: bool) {
        let idx = Self::STARTING_BONUS_STEPS.iter().position(|&v| (v - self.starting_resource_bonus).abs() < 0.01).unwrap_or(0);
        let new_idx = if forward {
            (idx + 1) % Self::STARTING_BONUS_STEPS.len()
        } else {
            (idx + Self::STARTING_BONUS_STEPS.len() - 1) % Self::STARTING_BONUS_STEPS.len()
        };
        self.starting_resource_bonus = Self::STARTING_BONUS_STEPS[new_idx];
    }
}

/// Which field is selected on the PlayerSettings screen.
#[derive(Default, Clone, PartialEq, Debug)]
enum PlayerSettingsField {
    #[default]
    ResourceMultiplier,
    BuildSpeed,
    StartingBonus,
    FogOfWar,
}

// ── Multiplayer networking ─────────────────────────────────────────────────────

const LAN_PORT: u16 = 5555;
const NET_STATE_INTERVAL: f32 = 0.1; // 10Hz game state broadcasts

/// Serializable snapshot of a unit for network transmission.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct NetUnit {
    id: u64,
    x: i32,
    y: i32,
    faction: String,
    unit_type: String,
    hp: f32,
    hp_max: f32,
}

/// Serializable snapshot of a building for network transmission.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct NetBuilding {
    id: u64,
    x: i32,
    y: i32,
    faction: String,
    building_type: String,
    hp: f32,
    hp_max: f32,
    built: bool,
}

/// Full game state snapshot sent from host to clients.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct NetGameState {
    units: Vec<NetUnit>,
    buildings: Vec<NetBuilding>,
    mission_status: String,   // "Active", "Won", "Lost"
    elapsed: f32,
    deadline: f32,
}

/// Command sent from joining client to host.
#[derive(Serialize, Deserialize, Debug, Clone)]
enum ClientCommand {
    MoveOrder { unit_ids: Vec<u64>, target_x: i32, target_y: i32 },
    AttackOrder { unit_ids: Vec<u64>, target_id: u64 },
    BuildOrder { building_type: String, x: i32, y: i32 },
}

/// Wraps a command or state in a length-prefixed frame.
#[derive(Serialize, Deserialize, Debug, Clone)]
enum NetMessage {
    State(NetGameState),
    Command(ClientCommand),
    LobbyState(LobbyConfig),
    LobbyReady,
}

// ── Lobby slot types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SlotController {
    Human,
    Ai(String),   // "easy", "normal", "hard"
    Open,         // waiting for a player to join
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbySlot {
    pub id: usize,
    pub team: String,          // "Alpha", "Bravo", "Charlie", "Delta", or "FFA-N"
    pub faction: String,       // "Combine", "Ironborn", "Covenant", "Hollow"
    pub controller: SlotController,
    pub spawn_zone: usize,     // index into map's spawn_zones list
}

#[derive(Resource, Clone, Default, Debug, Serialize, Deserialize)]
pub struct LobbyConfig {
    pub slots: Vec<LobbySlot>,
    pub map_path: Option<String>,    // which map to load
    pub win_condition: String,        // "Control", "Assault", "Ffa", "KingOfTheHill", "Assassination"
    pub selected_slot: usize,         // cursor in the lobby UI
    pub selected_field: LobbyField,   // which field is being edited
    pub available_maps: Vec<String>,  // relative paths like "maps/combine_m0.toml"
    pub selected_map: usize,          // index into available_maps
}

#[derive(Default, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum LobbyField {
    #[default] Slot,
    Team,
    Faction,
    Controller,
    Map,
    WinCondition,
}

impl LobbyConfig {
    fn default_2_slot() -> Self {
        LobbyConfig {
            slots: vec![
                LobbySlot { id: 0, team: "Alpha".into(), faction: "Combine".into(), controller: SlotController::Human, spawn_zone: 0 },
                LobbySlot { id: 1, team: "Bravo".into(), faction: "Ironborn".into(), controller: SlotController::Ai("normal".into()), spawn_zone: 1 },
            ],
            map_path: None,
            win_condition: "Assault".into(),
            selected_slot: 0,
            selected_field: LobbyField::Slot,
            available_maps: Vec::new(),
            selected_map: 0,
        }
    }
}

/// Build lobby slots from a map's spawn_zones. Falls back to 2 default slots if
/// the map has no spawn zones or cannot be loaded.
fn lobby_slots_from_map(map_path: &str) -> Vec<LobbySlot> {
    let full_path = format!("assets/{}", map_path);
    if let Ok(content) = std::fs::read_to_string(&full_path) {
        if let Ok(saved) = toml::from_str::<SavedMap>(&content) {
            if !saved.spawn_zones.is_empty() {
                return saved.spawn_zones.iter().map(|zone| {
                    let team_idx = zone.suggested_team.unwrap_or(zone.id % 8);
                    LobbySlot {
                        id: zone.id,
                        spawn_zone: zone.id,
                        team: LOBBY_TEAMS[team_idx.min(LOBBY_TEAMS.len() - 1)].to_string(),
                        faction: LOBBY_FACTIONS[zone.id % LOBBY_FACTIONS.len()].to_string(),
                        controller: if zone.id == 0 {
                            SlotController::Human
                        } else {
                            SlotController::Ai("normal".into())
                        },
                    }
                }).collect();
            }
        }
    }
    // fallback: 2 default slots
    vec![
        LobbySlot { id: 0, team: "Alpha".into(), faction: "Combine".into(), controller: SlotController::Human, spawn_zone: 0 },
        LobbySlot { id: 1, team: "Bravo".into(), faction: "Ironborn".into(), controller: SlotController::Ai("normal".into()), spawn_zone: 1 },
    ]
}

/// Scan assets/maps/*.toml and return sorted relative paths (maps/foo.toml).
fn scan_available_maps() -> Vec<String> {
    let mut maps: Vec<String> = std::fs::read_dir("assets/maps")
        .map(|rd| {
            rd.filter_map(|e| e.ok())
              .filter_map(|e| {
                  let p = e.path();
                  if p.extension().and_then(|x| x.to_str()) == Some("toml") {
                      p.file_name()
                       .and_then(|n| n.to_str())
                       .map(|n| format!("maps/{}", n))
                  } else {
                      None
                  }
              })
              .collect()
        })
        .unwrap_or_default();
    maps.sort();
    maps
}

const LOBBY_TEAMS: &[&str] = &["Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel"];
const SKIRMISH_DIFFICULTIES: &[&str] = &["easy", "normal", "hard"];
const LOBBY_FACTIONS: &[&str] = &["Combine", "Ironborn", "Covenant", "Hollow"];
const LOBBY_WIN_CONDITIONS: &[&str] = &["Assault", "Control", "Ffa", "KingOfTheHill", "Assassination", "Defense"];
const LOBBY_CONTROLLERS: &[SlotController] = &[
    SlotController::Human,
    SlotController::Ai(String::new()), // placeholder, handled via cycle_controller
    SlotController::Open,
];

/// Role of this process in a multiplayer session.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
enum MultiplayerRole {
    #[default]
    None,
    Host,
    Client { server_ip: String },
}

/// Shared channel ends for the networking thread.
/// `outbox` — messages to send to peers; `inbox` — messages received from peers.
/// Both are wrapped in `Arc<Mutex<>>` to satisfy Bevy's `Resource: Sync` requirement.
#[derive(Resource, Clone)]
struct NetChannels {
    outbox: Arc<Mutex<Sender<NetMessage>>>,
    inbox: Arc<Mutex<Receiver<NetMessage>>>,
}

/// Timer for state broadcast cadence (host only).
#[derive(Resource)]
struct NetBroadcastTimer(f32);

// ── 3D model asset loading ────────────────────────────────────────────────────

/// Default upstream: nginx instance hosting Synty GLB packs + kenney_aio audio.
const DEFAULT_ASSET_BASE: &str = "http://srv:49200/assets";

/// Bevy AssetSource name registered for the on-disk asset root. Asset references
/// flow through `cache://<relative_path>` so Bevy's FileAssetReader resolves
/// against `ModelAssets.local_root` instead of the default `assets/` directory.
const CACHE_SOURCE: &str = "cache";

/// Holds the resolved asset roots for the running session.
///
/// `local_root` is the on-disk directory that backs the `"cache"` AssetSource,
/// registered at App build via `register_asset_source`. Every GLB/OGG load goes
/// through this source — either populated by `prefetch_http_assets` (when
/// `http_base` is `Some`) or pre-populated by the user (when an env var points
/// at a directory).
#[derive(Resource, Clone)]
struct ModelAssets {
    local_root: PathBuf,
    http_base: Option<String>,
}

impl Default for ModelAssets {
    fn default() -> Self {
        Self { local_root: cache_dir(), http_base: None }
    }
}

impl ModelAssets {
    /// Bevy asset path for a relative file (e.g., `POLYGON_*/foo.glb#Scene0`).
    /// `relative` may carry a `#Scene0` label fragment — Bevy parses it correctly
    /// once routed through the named source.
    fn asset_path(&self, relative: &str) -> String {
        format!("{}://{}", CACHE_SOURCE, relative)
    }

    /// On-disk path to the bare file (without `#Scene0`), used to check existence
    /// before issuing `AssetServer::load` for paths that fall back to non-existent
    /// `unit_<id>.glb` names.
    fn local_file(&self, relative: &str) -> PathBuf {
        let bare = relative.split('#').next().unwrap_or(relative);
        self.local_root.join(bare)
    }
}

/// Map a unit type id to its expected GLB scene filename (used when faction TOML
/// lacks an explicit `model_file`).
fn unit_model_name(unit_type: &UnitTypeId) -> String {
    format!("unit_{}.glb#Scene0", unit_type.id())
}

/// Map a building type id to its expected GLB scene filename.
fn building_model_name(building_type: &BuildingTypeId) -> String {
    format!("building_{}.glb#Scene0", building_type.id())
}

/// On-disk prefetch cache for HTTP-sourced assets. Lives under $XDG_CACHE_HOME (or ~/.cache).
fn cache_dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("cindertide/assets")
}

/// Resolve the asset configuration from env vars before App startup.
/// Returns `(local_root, optional_http_base_for_prefetch)`.
///
/// Resolution order:
///   1. `CINDERTIDE_ASSET_BASE` — accepts http(s) URL (cache_dir backs it; prefetch runs)
///      or a local directory (used directly; no prefetch).
///   2. `CINDERTIDE_MODEL_PATH` — legacy filesystem-only override.
///   3. Default: `cache_dir()` + prefetch from `DEFAULT_ASSET_BASE`.
fn resolve_asset_config() -> (PathBuf, Option<String>) {
    if let Ok(val) = std::env::var("CINDERTIDE_ASSET_BASE") {
        let trimmed = val.trim_end_matches('/').to_string();
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            return (cache_dir(), Some(trimmed));
        }
        let path = PathBuf::from(&val);
        if path.is_dir() {
            return (path, None);
        }
        eprintln!("CINDERTIDE_ASSET_BASE='{}' is neither URL nor directory — falling back", val);
    }
    if let Ok(val) = std::env::var("CINDERTIDE_MODEL_PATH") {
        let path = PathBuf::from(&val);
        if path.is_dir() {
            return (path, None);
        }
        eprintln!("CINDERTIDE_MODEL_PATH='{}' is not a directory — falling back", val);
    }
    (cache_dir(), Some(DEFAULT_ASSET_BASE.to_string()))
}

/// Percent-encode chars that appear in Synty/kenney paths but are illegal in raw URLs.
/// Used only when building prefetch HTTP URLs — Bevy's `cache://` paths take the raw form.
fn url_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            ',' => out.push_str("%2C"),
            _ => out.push(c),
        }
    }
    out
}

/// Startup system: when `http_base` is set, prefetch every referenced GLB + OGG to
/// `local_root` via reqwest. Subsequent `cache://<rel>` loads then hit the filesystem.
/// Workaround for `bevy_web_asset`/surf's burst-load flakiness.
fn prefetch_http_assets(
    model_assets: Res<ModelAssets>,
    loaded: Res<LoadedFactions>,
) {
    let Some(base) = &model_assets.http_base else {
        info!("Assets loaded from local dir: {}", model_assets.local_root.display());
        return;
    };

    let cache_root = &model_assets.local_root;
    if let Err(e) = std::fs::create_dir_all(cache_root) {
        warn!("Cache dir create failed ({}) — visuals will fall back to placeholders", e);
        return;
    }

    // Collect unique relative paths. `loaded.units`/`buildings` are HashMap<id, Def>
    // and dedupe across factions, so iterate per-faction Vecs instead. Strip `#Scene0`
    // for the download URL; the label is re-attached at load time.
    let mut paths: std::collections::HashSet<String> = std::collections::HashSet::new();
    for faction in loaded.factions.values() {
        for unit in &faction.units {
            if !unit.model_file.is_empty() {
                let bare = unit.model_file.split('#').next().unwrap_or(&unit.model_file);
                paths.insert(bare.to_string());
            }
        }
        for building in &faction.buildings {
            if !building.model_file.is_empty() {
                let bare = building.model_file.split('#').next().unwrap_or(&building.model_file);
                paths.insert(bare.to_string());
            }
        }
    }
    for (_, rel) in AUDIO_PATHS {
        paths.insert((*rel).to_string());
    }
    for rel in cindertide::cutscene_editor::FONT_PATHS {
        paths.insert((*rel).to_string());
    }

    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("reqwest client build failed ({})", e);
            return;
        }
    };

    let mut fetched = 0usize;
    let mut cached = 0usize;
    let mut failed = 0usize;
    for rel in &paths {
        let dest = cache_root.join(rel);
        if dest.exists() {
            cached += 1;
            continue;
        }
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let url = format!("{}/{}", base, url_encode_path(rel));
        let bytes = match client.get(&url).send().and_then(|r| r.error_for_status()).and_then(|r| r.bytes()) {
            Ok(b) => b,
            Err(e) => {
                warn!("Prefetch failed for {}: {}", rel, e);
                failed += 1;
                continue;
            }
        };
        if let Err(e) = std::fs::write(&dest, &bytes) {
            warn!("Cache write failed for {}: {}", rel, e);
            failed += 1;
            continue;
        }
        fetched += 1;
    }

    info!(
        "Asset prefetch: {} downloaded, {} cached, {} failed ({} total) → {}",
        fetched, cached, failed, paths.len(), cache_root.display()
    );
}

/// Latest game state received from host (client only).
#[derive(Resource, Default)]
struct RemoteGameState(Option<NetGameState>);

/// When true, the editor was entered from a paused game — Escape returns to InMission.
#[derive(Resource, Default)]
struct EditorEnteredFromGame(bool);

/// Stable u64 IDs assigned to entities for network identity.
#[derive(Component)]
struct NetId(u64);

/// Counter for assigning NetIds.
#[derive(Resource, Default)]
struct NetIdCounter(u64);

fn next_net_id(counter: &mut NetIdCounter) -> u64 {
    counter.0 += 1;
    counter.0
}

/// Send a length-prefixed JSON message over a TCP stream (non-blocking write).
fn send_net_message(stream: &mut TcpStream, msg: &NetMessage) -> std::io::Result<()> {
    let data = serde_json::to_vec(msg).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let len = data.len() as u32;
    stream.write_all(&len.to_be_bytes())?;
    stream.write_all(&data)?;
    Ok(())
}

/// Read one length-prefixed JSON message from a TCP stream (blocking).
fn read_net_message(stream: &mut TcpStream) -> std::io::Result<NetMessage> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 4 * 1024 * 1024 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "message too large"));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}

/// Spawn a host networking thread: accepts one client and bridges the channels.
fn spawn_host_thread(tx: Sender<NetMessage>, rx: Arc<Mutex<Receiver<NetMessage>>>) {
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(format!("0.0.0.0:{LAN_PORT}")) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[net/host] bind failed: {e}");
                return;
            }
        };
        info!("[net/host] listening on port {LAN_PORT}");
        // Accept one client
        let (mut stream, addr) = match listener.accept() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[net/host] accept failed: {e}");
                return;
            }
        };
        info!("[net/host] client connected from {addr}");
        stream.set_nodelay(true).ok();
        stream.set_read_timeout(Some(std::time::Duration::from_millis(5))).ok();

        let mut stream_write = stream.try_clone().expect("clone stream");

        // Spawn read thread for incoming commands
        let tx_clone = tx.clone();
        std::thread::spawn(move || {
            loop {
                match read_net_message(&mut stream) {
                    Ok(msg) => { let _ = tx_clone.send(msg); }
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut
                           || e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => break,
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });

        // Write loop: forward outbox to client
        loop {
            let msg = {
                let rx_guard = rx.lock().unwrap();
                rx_guard.try_recv().ok()
            };
            if let Some(msg) = msg {
                if send_net_message(&mut stream_write, &msg).is_err() {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
}

/// Spawn a client networking thread: connects to host and bridges channels.
fn spawn_client_thread(server_ip: String, tx: Sender<NetMessage>, rx: Arc<Mutex<Receiver<NetMessage>>>) {
    std::thread::spawn(move || {
        let addr = format!("{server_ip}:{LAN_PORT}");
        info!("[net/client] connecting to {addr}");
        let mut stream = match TcpStream::connect(&addr) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[net/client] connect failed: {e}");
                return;
            }
        };
        stream.set_nodelay(true).ok();
        stream.set_read_timeout(Some(std::time::Duration::from_millis(5))).ok();
        info!("[net/client] connected to host");

        let mut stream_write = stream.try_clone().expect("clone stream");

        // Spawn read thread for incoming state
        let tx_clone = tx.clone();
        std::thread::spawn(move || {
            loop {
                match read_net_message(&mut stream) {
                    Ok(msg) => { let _ = tx_clone.send(msg); }
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut
                           || e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => break,
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });

        // Write loop: forward commands to host
        loop {
            let msg = {
                let rx_guard = rx.lock().unwrap();
                rx_guard.try_recv().ok()
            };
            if let Some(msg) = msg {
                if send_net_message(&mut stream_write, &msg).is_err() {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
}

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
        PlayableFaction::Combine => Faction::combine(),
        PlayableFaction::Ironborn => Faction::ironborn(),
        PlayableFaction::Handler => Faction::combine(), // Handler uses Combine visuals
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

/// On startup, load all campaign definitions from `assets/campaigns/*.toml`.
fn startup_load_campaigns(mut loaded: ResMut<LoadedCampaigns>) {
    loaded.0 = CampaignDef::load_all();
    info!("Loaded {} campaign(s)", loaded.0.len());
}

fn setup_scene(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        Projection::Orthographic(OrthographicProjection {
            scale: 28.0,
            scaling_mode: ScalingMode::FixedVertical { viewport_height: 1.0 },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(64.0 + 10.0, 10.0, 40.0 + 10.0).looking_at(Vec3::new(64.0, 0.0, 40.0), Vec3::Y),
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
                left: Val::Percent(30.0),
                top: Val::Px(12.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
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
            p.spawn((
                Node { margin: UiRect::top(Val::Px(8.0)), ..default() },
                Text::new("[Space] Resume   [E] Open Editor   [Q] Quit to Title"),
                TextColor(Color::srgb(0.75, 0.75, 0.75)),
                TextFont { font_size: 16.0, ..default() },
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

            // Dialogue bar — above the unit info panel (bottom-center)
            hud.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(72.0), // above the unit info panel (~60px tall)
                    left: Val::Percent(15.0),
                    width: Val::Percent(70.0),
                    padding: UiRect::axes(Val::Px(16.0), Val::Px(10.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.78)),
                Visibility::Hidden,
                DialogueBar,
            )).with_children(|panel| {
                panel.spawn((
                    Text::new(""),
                    TextColor(Color::srgb(0.95, 0.95, 0.80)),
                    TextFont { font_size: 17.0, ..default() },
                    DialogueBarText,
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

        // Lobby map preview panel (240×160 px, top-right, visible only in MultiplayerLobby)
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(20.0),
                top: Val::Px(80.0),
                width: Val::Px(244.0),
                height: Val::Px(164.0),
                overflow: Overflow::clip(),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.05, 0.05, 0.9)),
            BorderColor(Color::srgb(0.4, 0.4, 0.4)),
            Visibility::Hidden,
            LobbyMapPreview,
        ));
    });
}

// ── Screen overlay update ─────────────────────────────────────────────────────

fn update_screen_overlay(
    screen: Res<ClientScreen>,
    progress: Res<GlobalProgress>,
    active: Res<ActiveRun>,
    loaded_campaigns: Res<LoadedCampaigns>,
    lobby: Res<LobbyConfig>,
    mp_role: Res<MultiplayerRole>,
    player_cheats: Res<PlayerCheats>,
    mut overlay_vis: Query<&mut Visibility, With<ScreenOverlay>>,
    mut title_text: Query<&mut Text, (With<OverlayTitleText>, Without<OverlayBodyText>, Without<OverlayHintText>)>,
    mut body_text: Query<&mut Text, (With<OverlayBodyText>, Without<OverlayTitleText>, Without<OverlayHintText>)>,
    mut hint_text: Query<&mut Text, (With<OverlayHintText>, Without<OverlayTitleText>, Without<OverlayBodyText>)>,
    narrative: Option<Res<NarrativeData>>,
) {
    if !screen.is_changed() && !active.is_changed() && !progress.is_changed() && !lobby.is_changed() && !player_cheats.is_changed() {
        return;
    }

    let Ok(mut vis) = overlay_vis.single_mut() else { return };
    let Ok(mut title) = title_text.single_mut() else { return };
    let Ok(mut body) = body_text.single_mut() else { return };
    let Ok(mut hint) = hint_text.single_mut() else { return };

    match screen.as_ref() {
        ClientScreen::InMission | ClientScreen::TestMission { .. } | ClientScreen::MapEditor | ClientScreen::CampaignEditor => {
            *vis = Visibility::Hidden;
        }
        ClientScreen::Title => {
            *vis = Visibility::Visible;
            **title = "CINDERTIDE".to_string();
            **body = "a dieselpunk RTS".to_string();
            // Show first unlocked campaign name in the hint
            let first_campaign_name = loaded_campaigns.0.iter()
                .find(|c| c.is_unlocked(&progress))
                .map(|c| c.name.as_str())
                .unwrap_or("Campaign");
            **hint = format!("Enter — Start [{}]  |  S — Skirmish  |  M — Multiplayer  |  E — Editor  |  C — Settings", first_campaign_name);
        }
        ClientScreen::MultiplayerMenu { hosting, ip_input } => {
            *vis = Visibility::Visible;
            **title = "MULTIPLAYER".to_string();
            let status = if *hosting {
                format!("Hosting on port {LAN_PORT}\nWaiting for a player to join…\n\nH — Host  |  J — Join at {}\n\nEsc — Back", ip_input)
            } else {
                format!("Join IP: {}\n\nH — Host  |  J — Join\n\nEsc — Back", ip_input)
            };
            **body = status;
            **hint = "H = host on port 5555  |  J = join  |  Enter = start singleplayer".to_string();
        }
        ClientScreen::MultiplayerLobby { hosting, ip_input } => {
            *vis = Visibility::Visible;
            **title = "MULTIPLAYER LOBBY".to_string();

            let mut lines = String::new();

            // Map selector row
            let map_marker = if lobby.selected_field == LobbyField::Map { ">" } else { " " };
            let map_display = lobby.map_path.as_deref().unwrap_or("(none)");
            let slot_count = lobby.slots.len();
            lines.push_str(&format!("{}  Map: [{}]  ←→ change ({} slots)\n\n", map_marker, map_display, slot_count));

            // Win condition row
            let wc = &lobby.win_condition;
            let wc_marker = if lobby.selected_field == LobbyField::WinCondition { ">" } else { " " };
            lines.push_str(&format!("{}  Win Condition: [{}]  ←→ to change\n\n", wc_marker, wc));

            // Slot table header
            lines.push_str("  Slot  Team      Faction    Controller    Zone\n");
            lines.push_str("  ─────────────────────────────────────────────\n");

            for slot in &lobby.slots {
                let selected = slot.id == lobby.selected_slot;
                let row_marker = if selected { ">" } else { " " };
                let ctrl_str = match &slot.controller {
                    SlotController::Human => "YOU".to_string(),
                    SlotController::Ai(level) => format!("AI:{}", level),
                    SlotController::Open => "Open".to_string(),
                };
                lines.push_str(&format!("{}  [{}]   {:<8}  {:<9}  {:<12}  {}\n",
                    row_marker, slot.id, slot.team, slot.faction, ctrl_str, slot.spawn_zone));
            }

            lines.push('\n');

            let net_status = if *hosting {
                "[HOSTING — waiting for players...]".to_string()
            } else if matches!(*mp_role, MultiplayerRole::Client { .. }) {
                format!("[CONNECTED TO {}]", ip_input)
            } else {
                format!("H: Host on :{LAN_PORT}  J: Join {}", ip_input)
            };
            lines.push_str(&net_status);

            **body = lines;
            **hint = "↑↓ slot  ←→ cycle field  Tab: next field  ←→ on Map: change map  H: host  J: join  Enter: start  Esc: back".to_string();
        }
        ClientScreen::Skirmish => {
            *vis = Visibility::Visible;
            **title = "SKIRMISH".to_string();

            let mut lines = String::new();

            let map_marker = if lobby.selected_field == LobbyField::Map { ">" } else { " " };
            let map_display = lobby.map_path.as_deref().unwrap_or("(none)");
            let slot_count = lobby.slots.len();
            lines.push_str(&format!("{}  Map: [{}]  ←→ change ({} slots)\n\n", map_marker, map_display, slot_count));

            let wc_marker = if lobby.selected_field == LobbyField::WinCondition { ">" } else { " " };
            lines.push_str(&format!("{}  Win Condition: [{}]  ←→ to change\n\n", wc_marker, &lobby.win_condition));

            lines.push_str("  Slot  Team      Faction    Controller    Zone\n");
            lines.push_str("  ─────────────────────────────────────────────\n");

            for slot in &lobby.slots {
                let selected = slot.id == lobby.selected_slot;
                let row_marker = if selected && matches!(lobby.selected_field, LobbyField::Slot | LobbyField::Team | LobbyField::Faction | LobbyField::Controller) { ">" } else { " " };
                let ctrl_str = match &slot.controller {
                    SlotController::Human    => "YOU".to_string(),
                    SlotController::Ai(lvl) => format!("AI:{}", lvl),
                    SlotController::Open     => "Open".to_string(),
                };
                lines.push_str(&format!("{}  [{}]   {:<8}  {:<9}  {:<12}  {}\n",
                    row_marker, slot.id, slot.team, slot.faction, ctrl_str, slot.spawn_zone));
            }

            **body = lines;
            **hint = "↑↓ slot  Tab: next field  ←→ change  Enter: start  Esc: back".to_string();
        }
        ClientScreen::FactionPicker { selected } => {
            *vis = Visibility::Visible;
            **title = "Choose Your Campaign".to_string();

            let mut lines = String::new();
            for (i, campaign) in loaded_campaigns.0.iter().enumerate() {
                let is_unlocked = campaign.is_unlocked(&progress);
                let marker = if i == *selected { "> " } else { "  " };
                let lock_str = if is_unlocked { "" } else { " [LOCKED]" };
                lines.push_str(&format!("{}{}{}\n", marker, campaign.name, lock_str));
                if i == *selected {
                    // Show description for selected campaign
                    if is_unlocked {
                        lines.push_str(&format!("  {}\n", campaign.description));
                    } else {
                        let req = &campaign.unlock_requires;
                        lines.push_str(&format!("  Requires: {}\n", req));
                    }
                }
            }
            if loaded_campaigns.0.is_empty() {
                lines.push_str("No campaigns found.\n");
            }
            **body = lines.trim_end().to_string();
            **hint = "W/S or Arrow keys to select, Enter to confirm".to_string();
        }
        ClientScreen::MissionSelect { campaign_idx, selected } => {
            *vis = Visibility::Visible;
            **title = "SELECT MISSION".to_string();

            let campaign = loaded_campaigns.0.get(*campaign_idx);
            let campaign_id = campaign.map(|c| c.id.as_str()).unwrap_or("");
            let faction = campaign.map(|c| c.playable_faction()).unwrap_or(PlayableFaction::Combine);
            let missions_reached = progress.missions_reached.get(campaign_id).copied().unwrap_or(0);

            let mut lines = String::new();
            for i in 0..=missions_reached {
                let cursor = if i == *selected { ">" } else { " " };
                let status = if i < missions_reached {
                    "[DONE]"
                } else {
                    "[CURRENT]"
                };
                let (mission_title, _) = get_mission_narrative(&narrative, faction, i);
                lines.push_str(&format!("{} {}. {} {}\n", cursor, i + 1, mission_title, status));
            }
            **body = lines.trim_end().to_string();
            **hint = "↑↓ select  Enter: start  Esc: back".to_string();
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
        ClientScreen::PlayerSettings { selected_field } => {
            *vis = Visibility::Visible;
            **title = "PLAYER SETTINGS".to_string();

            let sel = selected_field;
            let rm_marker = if *sel == PlayerSettingsField::ResourceMultiplier { ">" } else { " " };
            let bs_marker = if *sel == PlayerSettingsField::BuildSpeed { ">" } else { " " };
            let sb_marker = if *sel == PlayerSettingsField::StartingBonus { ">" } else { " " };
            let fw_marker = if *sel == PlayerSettingsField::FogOfWar { ">" } else { " " };

            let fog_str = if player_cheats.fog_of_war { "ON" } else { "OFF" };

            let body_str = format!(
                "── PLAYER SETTINGS ──────────────\n\
                 {}  Resource income:    [{:.2}×]  ←→\n\
                 {}  Build speed:        [{:.2}×]  ←→\n\
                 {}  Starting resources: [+{}]    ←→\n\
                 {}  Fog of war:         [{}]    ←→\n\
                 \nThese apply to YOUR faction only.\nAI uses its own difficulty settings.",
                rm_marker, player_cheats.resource_multiplier,
                bs_marker, player_cheats.build_speed_multiplier,
                sb_marker, player_cheats.starting_resource_bonus as i32,
                fw_marker, fog_str,
            );
            **body = body_str;
            **hint = "↑↓ select field  ←→ change value  Esc: back".to_string();
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
    loaded_campaigns: Res<LoadedCampaigns>,
    mut mp_role: ResMut<MultiplayerRole>,
    mut lobby: ResMut<LobbyConfig>,
    net_channels: Option<Res<NetChannels>>,
    mut commands: Commands,
    mut player_cheats: ResMut<PlayerCheats>,
) {
    // Only handle UI input when not in mission or editor
    if matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. } | ClientScreen::MapEditor | ClientScreen::CampaignEditor) {
        return;
    }

    let enter = keys.just_pressed(KeyCode::Enter);
    let up = keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW);
    let down = keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS);

    match screen.clone() {
        ClientScreen::Title => {
            if enter {
                // Find index of first unlocked campaign
                let first_unlocked = loaded_campaigns.0.iter()
                    .enumerate()
                    .find(|(_, c)| c.is_unlocked(&progress))
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                *screen = ClientScreen::FactionPicker { selected: first_unlocked };
            } else if keys.just_pressed(KeyCode::KeyM) {
                let available_maps = scan_available_maps();
                let map_path = available_maps.first().cloned();
                let slots = map_path.as_deref()
                    .map(lobby_slots_from_map)
                    .unwrap_or_else(|| LobbyConfig::default_2_slot().slots);
                *lobby = LobbyConfig {
                    slots,
                    map_path,
                    win_condition: "Assault".into(),
                    selected_slot: 0,
                    selected_field: LobbyField::Slot,
                    available_maps,
                    selected_map: 0,
                };
                *screen = ClientScreen::MultiplayerLobby {
                    hosting: false,
                    ip_input: "127.0.0.1".to_string(),
                };
            } else if keys.just_pressed(KeyCode::KeyS) {
                let available_maps = scan_available_maps();
                let map_path = available_maps.first().cloned();
                let slots = map_path.as_deref()
                    .map(lobby_slots_from_map)
                    .unwrap_or_else(|| LobbyConfig::default_2_slot().slots);
                *lobby = LobbyConfig {
                    slots,
                    map_path,
                    win_condition: "Assault".into(),
                    selected_slot: 0,
                    selected_field: LobbyField::Slot,
                    available_maps,
                    selected_map: 0,
                };
                *screen = ClientScreen::Skirmish;
            } else if keys.just_pressed(KeyCode::KeyC) {
                *screen = ClientScreen::PlayerSettings { selected_field: PlayerSettingsField::ResourceMultiplier };
            } else if keys.just_pressed(KeyCode::KeyE) {
                // Enter map editor: try loading first campaign's first map; fallback to blank map
                let first_map_path = loaded_campaigns.0.iter()
                    .find(|c| c.is_unlocked(&progress))
                    .and_then(|c| c.missions.first())
                    .map(|m| format!("assets/{}", m.map));

                commands.queue(move |world: &mut World| {
                    cindertide::wipe_world_entities(world);

                    let mut loaded = false;
                    if let Some(ref path) = first_map_path {
                        if let Ok(content) = std::fs::read_to_string(path) {
                            if let Ok(saved) = toml::from_str::<SavedMap>(&content) {
                                for st in &saved.tiles {
                                    if let Some(terrain) = parse_terrain_name(&st.terrain) {
                                        world.spawn(Tile {
                                            pos: GridPos { x: st.x, y: st.y },
                                            terrain_type: terrain,
                                            cover: cindertide::map::CoverDensity::None,
                                        });
                                    }
                                }
                                for su in &saved.units {
                                    let faction = parse_faction_name(&su.faction).unwrap_or_else(Faction::combine);
                                    spawn_unit_world(world, su.x, su.y, faction, &su.unit_type);
                                }
                                for sb in &saved.buildings {
                                    let faction = parse_faction_name(&sb.faction).unwrap_or_else(Faction::combine);
                                    spawn_building_world(world, sb.x, sb.y, faction, &sb.building_type);
                                }
                                loaded = true;
                                info!("Editor: loaded first campaign map from {}", path);
                            }
                        }
                    }

                    if !loaded {
                        // Blank 48×28 grass map
                        for y in 0..28_i32 {
                            for x in 0..48_i32 {
                                world.spawn(Tile {
                                    pos: GridPos { x, y },
                                    terrain_type: cindertide::map::TerrainType::Grass,
                                    cover: cindertide::map::CoverDensity::None,
                                });
                            }
                        }
                    }
                    *world.resource_mut::<ClientScreen>() = ClientScreen::MapEditor;
                });
            }
        }

        ClientScreen::MultiplayerMenu { hosting: _, ip_input } => {
            if keys.just_pressed(KeyCode::Escape) {
                *screen = ClientScreen::Title;
                *mp_role = MultiplayerRole::None;
            } else if keys.just_pressed(KeyCode::KeyH) {
                // Host: spawn networking thread and go to faction picker
                let (tx_in, rx_in) = mpsc::channel::<NetMessage>();
                let (tx_out, rx_out) = mpsc::channel::<NetMessage>();
                let rx_out_arc = Arc::new(Mutex::new(rx_out));
                spawn_host_thread(tx_in.clone(), rx_out_arc);
                commands.insert_resource(NetChannels {
                    outbox: Arc::new(Mutex::new(tx_out)),
                    inbox: Arc::new(Mutex::new(rx_in)),
                });
                *mp_role = MultiplayerRole::Host;
                *screen = ClientScreen::MultiplayerMenu { hosting: true, ip_input: ip_input.clone() };
            } else if keys.just_pressed(KeyCode::KeyJ) {
                // Join: connect to IP
                let ip = ip_input.clone();
                let (tx_in, rx_in) = mpsc::channel::<NetMessage>();
                let (tx_out, rx_out) = mpsc::channel::<NetMessage>();
                let rx_out_arc = Arc::new(Mutex::new(rx_out));
                spawn_client_thread(ip.clone(), tx_in.clone(), rx_out_arc);
                commands.insert_resource(NetChannels {
                    outbox: Arc::new(Mutex::new(tx_out)),
                    inbox: Arc::new(Mutex::new(rx_in)),
                });
                *mp_role = MultiplayerRole::Client { server_ip: ip };
                // Client goes to faction picker too (will sync from host)
                *screen = ClientScreen::FactionPicker { selected: 0 };
            } else if enter {
                // Enter without hosting = just go to singleplayer campaign picker
                let first_unlocked = loaded_campaigns.0.iter()
                    .enumerate()
                    .find(|(_, c)| c.is_unlocked(&progress))
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                *screen = ClientScreen::FactionPicker { selected: first_unlocked };
            }
        }

        ClientScreen::MultiplayerLobby { hosting, ip_input } => {
            let hosting = hosting.clone();
            let ip_input = ip_input.clone();

            if keys.just_pressed(KeyCode::Escape) {
                *screen = ClientScreen::Title;
                *mp_role = MultiplayerRole::None;
                return;
            }

            let left = keys.just_pressed(KeyCode::ArrowLeft);
            let right = keys.just_pressed(KeyCode::ArrowRight);
            let tab = keys.just_pressed(KeyCode::Tab);

            // Up/Down: move selected_slot
            if up {
                if lobby.selected_slot > 0 { lobby.selected_slot -= 1; }
            }
            if down {
                let max_slot = lobby.slots.len().saturating_sub(1);
                if lobby.selected_slot < max_slot { lobby.selected_slot += 1; }
            }

            // Tab: cycle selected_field
            if tab {
                lobby.selected_field = match lobby.selected_field {
                    LobbyField::Slot        => LobbyField::Team,
                    LobbyField::Team        => LobbyField::Faction,
                    LobbyField::Faction     => LobbyField::Controller,
                    LobbyField::Controller  => LobbyField::Map,
                    LobbyField::Map         => LobbyField::WinCondition,
                    LobbyField::WinCondition => LobbyField::Slot,
                };
            }

            // Left/Right: cycle value of selected_field
            if left || right {
                let step: i32 = if right { 1 } else { -1 };
                match lobby.selected_field {
                    LobbyField::Map => {
                        if !lobby.available_maps.is_empty() {
                            let new_idx = ((lobby.selected_map as i32 + step)
                                .rem_euclid(lobby.available_maps.len() as i32)) as usize;
                            lobby.selected_map = new_idx;
                            let map_path = lobby.available_maps[new_idx].clone();
                            lobby.slots = lobby_slots_from_map(&map_path);
                            lobby.map_path = Some(map_path);
                            lobby.selected_slot = 0;
                        }
                    }
                    LobbyField::WinCondition => {
                        let idx = LOBBY_WIN_CONDITIONS.iter().position(|&w| w == lobby.win_condition.as_str()).unwrap_or(0);
                        let new_idx = ((idx as i32 + step).rem_euclid(LOBBY_WIN_CONDITIONS.len() as i32)) as usize;
                        lobby.win_condition = LOBBY_WIN_CONDITIONS[new_idx].to_string();
                    }
                    LobbyField::Team => {
                        let sel = lobby.selected_slot;
                        if let Some(slot) = lobby.slots.get_mut(sel) {
                            let idx = LOBBY_TEAMS.iter().position(|&t| t == slot.team.as_str()).unwrap_or(0);
                            let new_idx = ((idx as i32 + step).rem_euclid(LOBBY_TEAMS.len() as i32)) as usize;
                            slot.team = LOBBY_TEAMS[new_idx].to_string();
                        }
                    }
                    LobbyField::Faction => {
                        let sel = lobby.selected_slot;
                        if let Some(slot) = lobby.slots.get_mut(sel) {
                            let idx = LOBBY_FACTIONS.iter().position(|&f| f == slot.faction.as_str()).unwrap_or(0);
                            let new_idx = ((idx as i32 + step).rem_euclid(LOBBY_FACTIONS.len() as i32)) as usize;
                            slot.faction = LOBBY_FACTIONS[new_idx].to_string();
                        }
                    }
                    LobbyField::Controller => {
                        let sel = lobby.selected_slot;
                        if let Some(slot) = lobby.slots.get_mut(sel) {
                            let controllers = vec![
                                SlotController::Human,
                                SlotController::Ai("easy".into()),
                                SlotController::Ai("normal".into()),
                                SlotController::Ai("hard".into()),
                                SlotController::Open,
                            ];
                            let idx = controllers.iter().position(|c| c == &slot.controller).unwrap_or(0);
                            let new_idx = ((idx as i32 + step).rem_euclid(controllers.len() as i32)) as usize;
                            slot.controller = controllers[new_idx].clone();
                        }
                    }
                    LobbyField::Slot => {
                        // Left/Right on Slot field moves selected_slot
                        if left && lobby.selected_slot > 0 { lobby.selected_slot -= 1; }
                        if right {
                            let max_slot = lobby.slots.len().saturating_sub(1);
                            if lobby.selected_slot < max_slot { lobby.selected_slot += 1; }
                        }
                    }
                }
            }

            // H: host
            if keys.just_pressed(KeyCode::KeyH) && !hosting {
                let (tx_in, rx_in) = mpsc::channel::<NetMessage>();
                let (tx_out, rx_out) = mpsc::channel::<NetMessage>();
                let rx_out_arc = Arc::new(Mutex::new(rx_out));
                spawn_host_thread(tx_in.clone(), rx_out_arc);
                commands.insert_resource(NetChannels {
                    outbox: Arc::new(Mutex::new(tx_out)),
                    inbox: Arc::new(Mutex::new(rx_in)),
                });
                *mp_role = MultiplayerRole::Host;
                *screen = ClientScreen::MultiplayerLobby { hosting: true, ip_input: ip_input.clone() };
                return;
            }

            // J: join
            if keys.just_pressed(KeyCode::KeyJ) {
                let ip = ip_input.clone();
                let (tx_in, rx_in) = mpsc::channel::<NetMessage>();
                let (tx_out, rx_out) = mpsc::channel::<NetMessage>();
                let rx_out_arc = Arc::new(Mutex::new(rx_out));
                spawn_client_thread(ip.clone(), tx_in.clone(), rx_out_arc);
                commands.insert_resource(NetChannels {
                    outbox: Arc::new(Mutex::new(tx_out)),
                    inbox: Arc::new(Mutex::new(rx_in)),
                });
                *mp_role = MultiplayerRole::Client { server_ip: ip };
                *screen = ClientScreen::MultiplayerLobby { hosting: false, ip_input: ip_input.clone() };
                return;
            }

            // After state changes, broadcast lobby state to clients if hosting
            if hosting {
                if let Some(ref channels) = net_channels {
                    let _ = channels.outbox.lock().unwrap().send(NetMessage::LobbyState(lobby.clone()));
                }
            }

            // Enter: start mission (host) or singleplayer
            if enter {
                let lobby_clone = lobby.clone();
                commands.queue(move |world: &mut World| {
                    start_mission_from_lobby(world, &lobby_clone);
                });
            }
        }

        ClientScreen::Skirmish => {
            if keys.just_pressed(KeyCode::Escape) {
                *screen = ClientScreen::Title;
                return;
            }

            let left = keys.just_pressed(KeyCode::ArrowLeft);
            let right = keys.just_pressed(KeyCode::ArrowRight);
            let tab = keys.just_pressed(KeyCode::Tab);

            if up {
                if lobby.selected_slot > 0 { lobby.selected_slot -= 1; }
            }
            if down {
                let max_slot = lobby.slots.len().saturating_sub(1);
                if lobby.selected_slot < max_slot { lobby.selected_slot += 1; }
            }

            if tab {
                lobby.selected_field = match lobby.selected_field {
                    LobbyField::Slot         => LobbyField::Team,
                    LobbyField::Team         => LobbyField::Faction,
                    LobbyField::Faction      => LobbyField::Controller,
                    LobbyField::Controller   => LobbyField::Map,
                    LobbyField::Map          => LobbyField::WinCondition,
                    LobbyField::WinCondition => LobbyField::Slot,
                };
            }

            if left || right {
                let step: i32 = if right { 1 } else { -1 };
                match lobby.selected_field {
                    LobbyField::Map => {
                        if !lobby.available_maps.is_empty() {
                            let new_idx = ((lobby.selected_map as i32 + step)
                                .rem_euclid(lobby.available_maps.len() as i32)) as usize;
                            lobby.selected_map = new_idx;
                            let map_path = lobby.available_maps[new_idx].clone();
                            lobby.slots = lobby_slots_from_map(&map_path);
                            lobby.map_path = Some(map_path);
                            lobby.selected_slot = 0;
                        }
                    }
                    LobbyField::WinCondition => {
                        let idx = LOBBY_WIN_CONDITIONS.iter().position(|&w| w == lobby.win_condition.as_str()).unwrap_or(0);
                        let new_idx = ((idx as i32 + step).rem_euclid(LOBBY_WIN_CONDITIONS.len() as i32)) as usize;
                        lobby.win_condition = LOBBY_WIN_CONDITIONS[new_idx].to_string();
                    }
                    LobbyField::Team => {
                        let sel = lobby.selected_slot;
                        if let Some(slot) = lobby.slots.get_mut(sel) {
                            let idx = LOBBY_TEAMS.iter().position(|&t| t == slot.team.as_str()).unwrap_or(0);
                            let new_idx = ((idx as i32 + step).rem_euclid(LOBBY_TEAMS.len() as i32)) as usize;
                            slot.team = LOBBY_TEAMS[new_idx].to_string();
                        }
                    }
                    LobbyField::Faction => {
                        let sel = lobby.selected_slot;
                        if let Some(slot) = lobby.slots.get_mut(sel) {
                            let idx = LOBBY_FACTIONS.iter().position(|&f| f == slot.faction.as_str()).unwrap_or(0);
                            let new_idx = ((idx as i32 + step).rem_euclid(LOBBY_FACTIONS.len() as i32)) as usize;
                            slot.faction = LOBBY_FACTIONS[new_idx].to_string();
                        }
                    }
                    LobbyField::Controller => {
                        let sel = lobby.selected_slot;
                        if let Some(slot) = lobby.slots.get_mut(sel) {
                            let controllers = vec![
                                SlotController::Human,
                                SlotController::Ai("easy".into()),
                                SlotController::Ai("normal".into()),
                                SlotController::Ai("hard".into()),
                            ];
                            let idx = controllers.iter().position(|c| c == &slot.controller).unwrap_or(0);
                            let new_idx = ((idx as i32 + step).rem_euclid(controllers.len() as i32)) as usize;
                            slot.controller = controllers[new_idx].clone();
                        }
                    }
                    LobbyField::Slot => {
                        if left && lobby.selected_slot > 0 { lobby.selected_slot -= 1; }
                        if right {
                            let max_slot = lobby.slots.len().saturating_sub(1);
                            if lobby.selected_slot < max_slot { lobby.selected_slot += 1; }
                        }
                    }
                }
            }

            if enter {
                let lobby_clone = lobby.clone();
                commands.queue(move |world: &mut World| {
                    start_mission_from_lobby(world, &lobby_clone);
                });
            }
        }

        ClientScreen::FactionPicker { selected } => {
            let count = loaded_campaigns.0.len().max(1);
            if up {
                let new = if selected == 0 { count - 1 } else { selected - 1 };
                *screen = ClientScreen::FactionPicker { selected: new };
            } else if down {
                *screen = ClientScreen::FactionPicker { selected: (selected + 1) % count };
            } else if enter {
                if let Some(campaign) = loaded_campaigns.0.get(selected) {
                    if !campaign.is_unlocked(&progress) {
                        // Can't select a locked campaign
                        return;
                    }
                    let faction = campaign.playable_faction();
                    let mission_maps: Vec<String> = campaign.missions.iter()
                        .map(|m| m.map.clone())
                        .collect();

                    // Create a new campaign run
                    let run = CampaignRun {
                        faction,
                        current_mission: 0,
                        outcomes: Vec::new(),
                        complete: false,
                        campaign_id: campaign.id.clone(),
                        mission_maps,
                    };
                    *active = ActiveRun {
                        run: Some(run),
                        current_mission_entity: None,
                        missions_won: 0,
                        missions_lost: 0,
                    };

                    // If the player has already beaten at least one mission in this campaign,
                    // show the mission select screen so they can resume or replay.
                    let missions_reached = progress.missions_reached.get(&campaign.id).copied().unwrap_or(0);
                    if missions_reached > 0 {
                        *screen = ClientScreen::MissionSelect { campaign_idx: selected, selected: missions_reached };
                    } else {
                        // Transition to briefing for mission 0
                        let (title, briefing) = get_mission_narrative(&narrative, faction, 0);
                        *screen = ClientScreen::Briefing { title, briefing };
                    }
                }
            }
        }

        ClientScreen::MissionSelect { campaign_idx, selected } => {
            let campaign_id = loaded_campaigns.0.get(campaign_idx).map(|c| c.id.as_str()).unwrap_or("");
            let missions_reached = progress.missions_reached.get(campaign_id).copied().unwrap_or(0);
            let max_sel = missions_reached;
            if up {
                let new = if selected == 0 { 0 } else { selected - 1 };
                *screen = ClientScreen::MissionSelect { campaign_idx, selected: new };
            } else if down {
                let new = if selected >= max_sel { max_sel } else { selected + 1 };
                *screen = ClientScreen::MissionSelect { campaign_idx, selected: new };
            } else if enter {
                let mission_idx = selected;
                if let Some(ref mut run) = active.run {
                    run.current_mission = mission_idx;
                }
                let faction = active.run.as_ref().map(|r| r.faction).unwrap_or(PlayableFaction::Combine);
                let (title, briefing) = get_mission_narrative(&narrative, faction, mission_idx);
                *screen = ClientScreen::Briefing { title, briefing };
            } else if keys.just_pressed(KeyCode::Escape) {
                *screen = ClientScreen::FactionPicker { selected: campaign_idx };
            }
        }

        ClientScreen::Briefing { .. } => {
            if enter {
                // Populate LobbyConfig with single-player defaults before starting
                {
                    let active = active.as_ref();
                    let player_faction_str = active.run.as_ref()
                        .map(|r| map_faction(r.faction).id().to_string())
                        .unwrap_or_else(|| "combine".to_string());
                    let map_path_str = active.run.as_ref().and_then(|r| {
                        let idx = r.current_mission;
                        r.mission_maps.get(idx).cloned()
                    });
                    let wc = active.run.as_ref()
                        .and_then(|r| next_mission_type(r))
                        .map(|mt| format!("{:?}", mt))
                        .unwrap_or_else(|| "Assault".to_string());

                    let opp_faction = if player_faction_str == "combine" { "ironborn" } else { "combine" };

                    *lobby = LobbyConfig {
                        slots: vec![
                            LobbySlot { id: 0, team: "Alpha".into(), faction: player_faction_str, controller: SlotController::Human, spawn_zone: 0 },
                            LobbySlot { id: 1, team: "Bravo".into(), faction: opp_faction.into(), controller: SlotController::Ai("normal".into()), spawn_zone: 1 },
                        ],
                        map_path: map_path_str,
                        win_condition: wc,
                        selected_slot: 0,
                        selected_field: LobbyField::Slot,
                        available_maps: Vec::new(),
                        selected_map: 0,
                    };
                }

                commands.queue(|world: &mut World| {
                    cindertide::wipe_world_entities(world);

                    let (player, mission_index, map_path) = {
                        let active = world.resource::<ActiveRun>();
                        let p = active.run.as_ref()
                            .map(|r| map_faction(r.faction))
                            .unwrap_or_else(Faction::combine);
                        let idx = active.run.as_ref().map(|r| r.current_mission).unwrap_or(0);
                        let mp = active.run.as_ref()
                            .and_then(|r| r.mission_maps.get(idx))
                            .cloned();
                        (p, idx, mp)
                    };

                    let opponent = if player.id() == "combine" {
                        Faction::ironborn()
                    } else {
                        Faction::combine()
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
                        hill_timer: 0.0,
                        hill_threshold: 180.0,
                        assassination_target: None,
                        ffa_check_timer: 0.0,
                    }).id();

                    // Try to load map from file; fall back to procedural demo
                    let mut map_loaded = false;
                    if let Some(ref rel_path) = map_path {
                        let full_path = format!("assets/{}", rel_path);
                        if let Ok(content) = std::fs::read_to_string(&full_path) {
                            if let Ok(saved) = toml::from_str::<SavedMap>(&content) {
                                for st in &saved.tiles {
                                    if let Some(terrain) = parse_terrain_name(&st.terrain) {
                                        world.spawn(Tile {
                                            pos: GridPos { x: st.x, y: st.y },
                                            terrain_type: terrain,
                                            cover: cindertide::map::CoverDensity::None,
                                        });
                                    }
                                }
                                for su in &saved.units {
                                    let faction = parse_faction_name(&su.faction).unwrap_or_else(Faction::combine);
                                    spawn_unit_world(world, su.x, su.y, faction, &su.unit_type);
                                }
                                for sb in &saved.buildings {
                                    let faction = parse_faction_name(&sb.faction).unwrap_or_else(Faction::combine);
                                    spawn_building_world(world, sb.x, sb.y, faction, &sb.building_type);
                                }
                                map_loaded = !saved.tiles.is_empty();
                                info!("Loaded mission map from {} ({} tiles)", full_path, saved.tiles.len());
                            }
                        }
                        if !map_loaded {
                            info!("Map file '{}' not found or invalid, falling back to procedural", full_path);
                        }
                    }

                    if !map_loaded {
                        cindertide::setup_demo_scenario(world, &player, mission_index);
                    } else {
                        // setup_demo_scenario also loads `assets/scripts/<faction>_m<idx>.toml`;
                        // when a SavedMap was used we skip that path, so load the script
                        // explicitly here so MissionScriptPlugin (and the cutscene editor)
                        // see the typed events including the camera_* action variants.
                        let script_name = format!("{}_m{}", player.id(), mission_index);
                        if let Some(mut state) = world.get_resource_mut::<ScriptState>() {
                            state.load_script(&script_name);
                        }
                    }
                    cindertide::bake_navmesh(world);

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
                    let total = if run.mission_maps.is_empty() { 5 } else { run.mission_maps.len() };
                    run.current_mission >= total || run.complete
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
                let first_unlocked = loaded_campaigns.0.iter()
                    .enumerate()
                    .find(|(_, c)| c.is_unlocked(&progress))
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                *screen = ClientScreen::FactionPicker { selected: first_unlocked };
                // Reset the active run
                *active = ActiveRun::default();
                *game_state = GameState::Title;
            }
        }

        ClientScreen::InMission => {}
        ClientScreen::TestMission { .. } => {}
        ClientScreen::MapEditor => {}
        ClientScreen::CampaignEditor => {}
        // MultiplayerMenu and MultiplayerLobby handled above.

        ClientScreen::PlayerSettings { selected_field } => {
            let left = keys.just_pressed(KeyCode::ArrowLeft);
            let right = keys.just_pressed(KeyCode::ArrowRight);

            if keys.just_pressed(KeyCode::Escape) {
                *screen = ClientScreen::Title;
                return;
            }

            // Cycle selected field with up/down
            if up {
                let new_field = match selected_field {
                    PlayerSettingsField::ResourceMultiplier => PlayerSettingsField::FogOfWar,
                    PlayerSettingsField::BuildSpeed => PlayerSettingsField::ResourceMultiplier,
                    PlayerSettingsField::StartingBonus => PlayerSettingsField::BuildSpeed,
                    PlayerSettingsField::FogOfWar => PlayerSettingsField::StartingBonus,
                };
                *screen = ClientScreen::PlayerSettings { selected_field: new_field };
                return;
            }
            if down {
                let new_field = match selected_field {
                    PlayerSettingsField::ResourceMultiplier => PlayerSettingsField::BuildSpeed,
                    PlayerSettingsField::BuildSpeed => PlayerSettingsField::StartingBonus,
                    PlayerSettingsField::StartingBonus => PlayerSettingsField::FogOfWar,
                    PlayerSettingsField::FogOfWar => PlayerSettingsField::ResourceMultiplier,
                };
                *screen = ClientScreen::PlayerSettings { selected_field: new_field };
                return;
            }

            // Change values with left/right
            if left || right {
                let forward = right;
                match selected_field {
                    PlayerSettingsField::ResourceMultiplier => player_cheats.cycle_resource_multiplier(forward),
                    PlayerSettingsField::BuildSpeed => player_cheats.cycle_build_speed(forward),
                    PlayerSettingsField::StartingBonus => player_cheats.cycle_starting_bonus(forward),
                    PlayerSettingsField::FogOfWar => player_cheats.fog_of_war = !player_cheats.fog_of_war,
                }
            }
        }
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

// ── Lobby mission start ────────────────────────────────────────────────────────

/// Parse a lobby win_condition string into a MissionType.
fn parse_win_condition(s: &str) -> MissionType {
    match s {
        "Control"        => MissionType::Control,
        "Defense"        => MissionType::Defense,
        "Ffa"            => MissionType::Ffa,
        "KingOfTheHill"  => MissionType::KingOfTheHill,
        "Assassination"  => MissionType::Assassination,
        _                => MissionType::Assault,
    }
}

/// Parse a faction string to Faction.
fn lobby_faction_to_map(s: &str) -> Faction {
    Faction::new(&s.to_lowercase())
}

/// Start a mission from a LobbyConfig (used by multiplayer lobby and single-player shortcut).
fn start_mission_from_lobby(world: &mut World, lobby: &LobbyConfig) {
    cindertide::wipe_world_entities(world);

    // Determine player faction (first Human slot, fallback Combine)
    let player_slot = lobby.slots.iter().find(|s| s.controller == SlotController::Human);
    let player_faction = player_slot.map(|s| lobby_faction_to_map(&s.faction)).unwrap_or_else(Faction::combine);

    // Opponent faction: first non-Human slot, fallback Ironborn
    let opponent_faction = lobby.slots.iter()
        .find(|s| s.controller != SlotController::Human)
        .map(|s| lobby_faction_to_map(&s.faction))
        .unwrap_or_else(Faction::ironborn);

    let mission_type = parse_win_condition(&lobby.win_condition);

    world.spawn(FactionBundle::new(player_faction.clone()));

    let mission_entity = world.spawn(Mission {
        mission_type,
        player_faction: player_faction.clone(),
        opponent_faction: opponent_faction.clone(),
        status: MissionStatus::Active,
        elapsed: 0.0,
        deadline: 300.0,
        hill_timer: 0.0,
        hill_threshold: 180.0,
        assassination_target: None,
        ffa_check_timer: 0.0,
    }).id();

    // Try to load map file
    let mut map_loaded = false;
    let map_path = lobby.map_path.clone();

    if let Some(ref rel_path) = map_path {
        let full_path = format!("assets/{}", rel_path);
        if let Ok(content) = std::fs::read_to_string(&full_path) {
            if let Ok(saved) = toml::from_str::<SavedMap>(&content) {
                for st in &saved.tiles {
                    if let Some(terrain) = parse_terrain_name(&st.terrain) {
                        world.spawn(Tile {
                            pos: GridPos { x: st.x, y: st.y },
                            terrain_type: terrain,
                            cover: cindertide::map::CoverDensity::None,
                        });
                    }
                }
                // Spawn units from the map, but also place faction starting units at spawn zones
                for su in &saved.units {
                    let faction = parse_faction_name(&su.faction).unwrap_or_else(Faction::combine);
                    spawn_unit_world(world, su.x, su.y, faction, &su.unit_type);
                }
                for sb in &saved.buildings {
                    let faction = parse_faction_name(&sb.faction).unwrap_or_else(Faction::combine);
                    spawn_building_world(world, sb.x, sb.y, faction, &sb.building_type);
                }

                // Place faction starting units at spawn zone positions from LobbyConfig
                let spawn_zones = &saved.spawn_zones;
                if !spawn_zones.is_empty() {
                    for slot in &lobby.slots {
                        let zone_idx = slot.spawn_zone.min(spawn_zones.len().saturating_sub(1));
                        if let Some(zone) = spawn_zones.get(zone_idx) {
                            let faction = lobby_faction_to_map(&slot.faction);
                            // Spawn a starting unit bundle at the zone position
                            spawn_unit_world(world, zone.x, zone.y, faction.clone(), "Riflemen");
                            spawn_building_world(world, zone.x + 1, zone.y, faction, "CommandBunker");
                        }
                    }
                }

                map_loaded = true;
                info!("Lobby: loaded map from {}", full_path);
            }
        }
    }

    if !map_loaded {
        // Default: place player and opponent at corners
        let player_zone_x = 4;
        let player_zone_y = 4;
        let opp_zone_x: i32;
        let opp_zone_y: i32;

        // Try to use spawn zone data if lobby has zone indices
        if let Some(player_slot) = player_slot {
            let pzx = (player_slot.spawn_zone as i32 * 20 + 4).min(100);
            let pzy = 4;
            opp_zone_x = pzx + 40;
            opp_zone_y = 20;
            cindertide::setup_demo_scenario(world, &player_faction, 0);
            let _ = (player_zone_x, player_zone_y, pzx, pzy); // used above
        } else {
            opp_zone_x = 44;
            opp_zone_y = 20;
            cindertide::setup_demo_scenario(world, &player_faction, 0);
        }
        let _ = (opp_zone_x, opp_zone_y); // suppress warning
    }

    cindertide::bake_navmesh(world);

    let mut active = world.resource_mut::<ActiveRun>();
    active.current_mission_entity = Some(mission_entity);
    drop(active);

    *world.resource_mut::<GameState>() = GameState::InMission;
    world.insert_resource(PlayerFaction(player_faction));
    *world.resource_mut::<ClientScreen>() = ClientScreen::InMission;

    // Broadcast LobbyReady to clients if hosting
    if let Some(channels) = world.get_resource::<NetChannels>() {
        let _ = channels.outbox.lock().unwrap().send(NetMessage::LobbyReady);
    }
}

// ── Poll mission end ──────────────────────────────────────────────────────────

fn poll_mission_end(
    mut screen: ResMut<ClientScreen>,
    mut active: ResMut<ActiveRun>,
    mut progress: ResMut<GlobalProgress>,
    missions: Query<&Mission>,
    narrative: Option<Res<NarrativeData>>,
    mut audio_queue: ResMut<AudioEventQueue>,
) {
    // Check if we're in a test mission — if so, handle end by returning to the map editor.
    if let ClientScreen::TestMission { saved_map_path } = &*screen {
        let Some(mission_entity) = active.current_mission_entity else { return };
        let Ok(m) = missions.get(mission_entity) else { return };
        if m.status == MissionStatus::Active { return; }
        let won = m.status == MissionStatus::Won;
        let result_str = if won { "WON" } else { "LOST" };
        info!("Test mission ended: {result_str}. Returning to map editor.");
        active.current_mission_entity = None;
        audio_queue.0.push(AudioEvent::MissionEnd);
        *screen = ClientScreen::MapEditor;
        return;
    }

    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) {
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

    audio_queue.0.push(AudioEvent::MissionEnd);

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
    tiles: Query<(Entity, &Tile), Without<TileHasMesh>>,
) {
    for (tile_entity, tile) in &tiles {
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
        // Mark the tile entity so render_tiles skips it next frame.
        // TileHasMesh lives on the logical Tile entity; RenderedTile lives on
        // the mesh entity so editor/cleanup queries (With<RenderedTile>) still
        // find the visual meshes, not the game-logic entities.
        commands.entity(tile_entity).insert(TileHasMesh);
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
    model_assets: Res<ModelAssets>,
    asset_server: Res<AssetServer>,
    loaded: Res<LoadedFactions>,
    units: Query<(Entity, &UnitPos, &Faction, &UnitTypeId), Added<UnitTypeId>>,
) {
    for (entity, pos, faction, unit_type) in &units {
        let world_pos = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.75;

        let glb_name = loaded.faction_unit(faction.id(), unit_type.id())
            .map(|def| def.model_file.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| unit_model_name(unit_type));

        // Existence check guards against the `unit_<id>.glb` fallback path, which has
        // no corresponding file. Scale 0.01 assumes Synty-style centimetre-unit exports.
        let visual = if model_assets.local_file(&glb_name).exists() {
            commands.spawn((
                SceneRoot(asset_server.load(model_assets.asset_path(&glb_name))),
                Transform::from_translation(world_pos).with_scale(Vec3::splat(0.01)),
            )).id()
        } else {
            let color = faction_color(faction);
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.6, 1.5, 0.6))),
                MeshMaterial3d(materials.add(StandardMaterial { base_color: color, ..default() })),
                Transform::from_translation(world_pos),
            )).id()
        };

        visual_entities.units.insert(entity, visual);
    }
}

fn sync_unit_positions(
    units: Query<(Entity, &UnitPos, Option<&AttackTarget>, Option<&MoveTarget>), With<UnitTypeId>>,
    unit_positions: Query<&UnitPos, With<UnitTypeId>>,
    visual_entities: Res<VisualEntities>,
    mut transforms: Query<&mut Transform>,
) {
    for (entity, pos, attack_target, move_target) in &units {
        let Some(&visual) = visual_entities.units.get(&entity) else { continue };
        let Ok(mut transform) = transforms.get_mut(visual) else { continue };

        let target_world = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.75;
        transform.translation = transform.translation.lerp(target_world, 0.15);

        // Determine facing direction: attack target takes priority over movement.
        let desired_rot: Option<Quat> = if let Some(at) = attack_target {
            // Face toward the attack target's world position.
            if let Ok(target_pos) = unit_positions.get(at.entity) {
                let target_w = grid_to_world(target_pos.pos.x, target_pos.pos.y) + Vec3::Y * 0.75;
                let dir = (target_w - transform.translation).with_y(0.0);
                if dir.length_squared() > 0.0001 {
                    Some(Quat::from_rotation_arc(Vec3::Z, dir.normalize()))
                } else {
                    None
                }
            } else {
                None
            }
        } else if move_target.is_some() {
            // Face toward movement destination.
            let dir = (target_world - transform.translation).with_y(0.0);
            if dir.length_squared() > 0.0001 {
                Some(Quat::from_rotation_arc(Vec3::Z, dir.normalize()))
            } else {
                None
            }
        } else {
            None
        };

        if let Some(rot) = desired_rot {
            transform.rotation = transform.rotation.slerp(rot, 0.15);
        }
    }
}

fn spawn_building_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut visual_entities: ResMut<VisualEntities>,
    model_assets: Res<ModelAssets>,
    asset_server: Res<AssetServer>,
    loaded: Res<LoadedFactions>,
    buildings: Query<(Entity, &BuildingPos, &Faction, &BuildingTypeId), Added<BuildingTypeId>>,
) {
    for (entity, pos, faction, building_type) in &buildings {
        if visual_entities.buildings.contains_key(&entity) {
            continue;
        }
        let world_pos = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.5;

        let glb_name = loaded.faction_building(faction.id(), building_type.id())
            .map(|def| def.model_file.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| building_model_name(building_type));

        // Existence check guards against the `building_<id>.glb` fallback path.
        // Scale 0.015 assumes Synty-style centimetre-unit exports.
        let visual = if model_assets.local_file(&glb_name).exists() {
            commands.spawn((
                SceneRoot(asset_server.load(model_assets.asset_path(&glb_name))),
                Transform::from_translation(world_pos).with_scale(Vec3::splat(0.015)),
            )).id()
        } else {
            let color = faction_color(faction).mix(&Color::WHITE, 0.25);
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.9, 1.0, 0.9))),
                MeshMaterial3d(materials.add(StandardMaterial { base_color: color, ..default() })),
                Transform::from_translation(world_pos),
            )).id()
        };

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
    units: Query<(Entity, &UnitPos, &Faction), With<UnitTypeId>>,
    mut selected: ResMut<SelectedUnits>,
    player_faction: Option<Res<PlayerFaction>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    selection_rings: Query<Entity, With<SelectionRing>>,
    mut drag_state: ResMut<DragState>,
    attack_move_mode: Res<AttackMoveMode>,
    screen: Res<ClientScreen>,
    mut audio_queue: ResMut<AudioEventQueue>,
    nav: Option<Res<cindertide::map::NavMesh>>,
) {
    // Only handle mouse input during mission (or test mission)
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) {
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
                        audio_queue.0.push(AudioEvent::UnitSelected);
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

        // Verify target is within the map. Use NavMesh bounds when available.
        let tile_in_bounds = nav.as_ref().map_or(true, |n| {
            gx >= 0 && gy >= 0 && gx < n.width && gy < n.height
        });
        if !tile_in_bounds && enemy_at_target.is_none() {
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
            // Build occupied set from all unit positions.
            let all_occupied: HashSet<(i32, i32)> = units.iter()
                .map(|(_, pos, _)| (pos.pos.x, pos.pos.y))
                .collect();

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
                // Exclude the moving unit itself from occupied.
                let mut occupied = all_occupied.clone();
                occupied.remove(&(start.x, start.y));

                if let Some(ref nav) = nav {
                    if let Some(path) = cindertide::map::pathfinding::PathfindingGrid::find_path_on_navmesh(
                        nav,
                        start.clone(),
                        target_pos.clone(),
                        &pf_kind,
                        &occupied,
                    ) {
                        commands.entity(unit_entity)
                            .remove::<HoldPosition>()
                            .insert(MoveTarget { target: target_pos.clone() })
                            .insert(MoveProgress { path, current_step: 0, elapsed: 0.0 });
                    }
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
            // Build occupied set from all unit positions.
            let all_occupied: HashSet<(i32, i32)> = units.iter()
                .map(|(_, pos, _)| (pos.pos.x, pos.pos.y))
                .collect();

            let mut any_moved = false;
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
                // Exclude the moving unit itself from occupied.
                let mut occupied = all_occupied.clone();
                occupied.remove(&(start.x, start.y));

                if let Some(ref nav) = nav {
                    if let Some(path) = cindertide::map::pathfinding::PathfindingGrid::find_path_on_navmesh(
                        nav,
                        start.clone(),
                        target_pos.clone(),
                        &pf_kind,
                        &occupied,
                    ) {
                        commands.entity(unit_entity)
                            .remove::<HoldPosition>()
                            .insert(MoveTarget { target: target_pos.clone() })
                            .insert(MoveProgress { path, current_step: 0, elapsed: 0.0 });
                        any_moved = true;
                    }
                }
            }
            if any_moved {
                audio_queue.0.push(AudioEvent::UnitMoved);
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
    units: Query<(Entity, &UnitPos, &Faction), With<UnitTypeId>>,
    player_faction: Option<Res<PlayerFaction>>,
    mut attack_move_mode: ResMut<AttackMoveMode>,
    mut paused: ResMut<Paused>,
    mut time: ResMut<Time<Virtual>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    screen: Res<ClientScreen>,
) {
    // Only handle gameplay keys during mission (or test mission)
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) {
        return;
    }

    // A key — toggle attack-move mode (only when not paused)
    if !paused.0 {
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

/// Handle E (open editor) and Q (quit to title) while paused.
fn handle_paused_menu_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    paused: Res<Paused>,
    mut screen: ResMut<ClientScreen>,
    mut time: ResMut<Time<Virtual>>,
    mut visual_entities: ResMut<VisualEntities>,
    mut editor: ResMut<EditorState>,
    mut entered_from_game: ResMut<EditorEnteredFromGame>,
    tiles: Query<(Entity, &Tile)>,
    rendered_tiles: Query<Entity, With<RenderedTile>>,
) {
    if !paused.0 {
        return;
    }
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) {
        return;
    }

    // E — open editor from paused game
    if keys.just_pressed(KeyCode::KeyE) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let save_path = format!("assets/maps/ingame_edit_{}.toml", timestamp);
        let saved_tiles: Vec<SavedTile> = tiles.iter().map(|(_, t)| SavedTile {
            x: t.pos.x,
            y: t.pos.y,
            terrain: terrain_type_name(&t.terrain_type).to_string(),
        }).collect();
        let map = SavedMap {
            tiles: saved_tiles,
            units: Vec::new(),
            buildings: Vec::new(),
            mission_type: None,
            player_faction: None,
            opponent_faction: None,
            deadline_seconds: None,
            briefing_override: None,
            win_override: None,
            loss_override: None,
            mission_index_override: None,
            script_events: Vec::new(),
            spawn_zones: Vec::new(),
        };
        if let Ok(content) = toml::to_string(&map) {
            let _ = std::fs::create_dir_all("assets/maps");
            let _ = std::fs::write(&save_path, content);
        }
        *editor = EditorState::default();
        entered_from_game.0 = true;
        // Clear rendered tile visuals so editor can re-render
        for entity in &rendered_tiles {
            commands.entity(entity).despawn();
        }
        visual_entities.units.clear();
        visual_entities.buildings.clear();
        *screen = ClientScreen::MapEditor;
        // Don't unpause the Paused resource — handle_editor_keyboard will see entered_from_game=true
        // and restore paused on Escape
    }

    // Q — quit to title while paused
    if keys.just_pressed(KeyCode::KeyQ) {
        entered_from_game.0 = false;
        visual_entities.units.clear();
        visual_entities.buildings.clear();
        commands.queue(move |world: &mut World| {
            cindertide::wipe_world_entities(world);
            *world.resource_mut::<Paused>() = Paused(false);
            world.resource_mut::<Time<Virtual>>().unpause();
            *world.resource_mut::<ClientScreen>() = ClientScreen::Title;
        });
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
    units: Query<&UnitPos, With<UnitTypeId>>,
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
    mut camera_target: ResMut<CameraTarget>,
) {
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. } | ClientScreen::MapEditor) {
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
    if pan != Vec3::ZERO {
        // Edge-scroll cancels scripted focus the same way WASD does.
        if !matches!(*camera_target, CameraTarget::Free) {
            *camera_target = CameraTarget::Free;
        }
        transform.translation += pan * cam.pan_speed * dt;
    }
}

fn camera_pan_zoom(
    mut query: Query<(&mut Transform, &mut Projection, &IsometricCamera)>,
    keys: Res<ButtonInput<KeyCode>>,
    mut scroll: EventReader<MouseWheel>,
    time: Res<Time>,
    screen: Res<ClientScreen>,
    mut camera_target: ResMut<CameraTarget>,
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
    if pan != Vec3::ZERO {
        // User-driven pan cancels any scripted camera focus.
        if !matches!(*camera_target, CameraTarget::Free) {
            *camera_target = CameraTarget::Free;
        }
        transform.translation += pan * cam.pan_speed * dt;
    }

    if let Projection::Orthographic(ref mut ortho) = *projection {
        for ev in scroll.read() {
            ortho.scale = (ortho.scale - ev.y * cam.zoom_speed).clamp(4.0, 120.0);
        }
    }
}

/// Tracks whether the camera has been snapped to the player home base for the
/// current mission run. Reset when leaving the in-mission screen so the next
/// mission gets its own snap.
#[derive(Resource, Default)]
struct CameraSnapped(bool);

/// One-shot system: on first frame of `ClientScreen::InMission` with a player
/// `command_bunker` present, set `CameraTarget::LookAt(avg base position)`.
/// Uses the player faction's `cinematic_framing` default for the initial framing.
/// Reset the latch when the screen leaves InMission so the next mission re-snaps.
fn snap_camera_on_mission_start(
    screen: Res<ClientScreen>,
    mut snapped: ResMut<CameraSnapped>,
    mut camera_target: ResMut<CameraTarget>,
    player_faction: Option<Res<PlayerFaction>>,
    loaded: Res<LoadedFactions>,
    buildings: Query<(&BuildingPos, &Faction, &BuildingTypeId)>,
) {
    let in_mission = matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. });
    if !in_mission {
        if snapped.0 { snapped.0 = false; }
        return;
    }
    if snapped.0 { return; }
    let Some(pf) = player_faction.as_deref() else { return };

    let mut sum_x = 0i64;
    let mut sum_y = 0i64;
    let mut count = 0i64;
    for (pos, faction, bt) in &buildings {
        if *faction == pf.0 && bt.id() == "command_bunker" {
            sum_x += pos.pos.x as i64;
            sum_y += pos.pos.y as i64;
            count += 1;
        }
    }
    if count == 0 { return; }
    let cx = (sum_x as f32) / (count as f32);
    let cy = (sum_y as f32) / (count as f32);
    // Use the gameplay isometric framing for the initial mission view — never
    // start the player in a dramatic cutscene angle. Cutscenes apply their own
    // framing via script CameraFocus actions.
    let _ = loaded; // reserved for future per-mission framing
    *camera_target = CameraTarget::LookAt {
        point: Vec3::new(cx, 0.0, cy),
        framing: framing_for("isometric"),
    };
    snapped.0 = true;
}

/// Auto-attach `CinematicFraming` to newly-spawned units/buildings whose def
/// declares a `cinematic_framing` value. Lets per-entity camera presets flow
/// from TOML → ECS without touching every spawn site in lib.rs / client.rs.
fn attach_cinematic_framing(
    mut commands: Commands,
    loaded: Res<LoadedFactions>,
    units: Query<(Entity, &Faction, &UnitTypeId), Added<UnitTypeId>>,
    buildings: Query<(Entity, &Faction, &BuildingTypeId), Added<BuildingTypeId>>,
) {
    for (e, faction, ut) in &units {
        if let Some(def) = loaded.faction_unit(&faction.0, ut.id()) {
            if !def.cinematic_framing.is_empty() {
                commands.entity(e).insert(CinematicFraming(def.cinematic_framing.clone()));
            }
        }
    }
    for (e, faction, bt) in &buildings {
        if let Some(def) = loaded.faction_building(&faction.0, bt.id()) {
            if !def.cinematic_framing.is_empty() {
                commands.entity(e).insert(CinematicFraming(def.cinematic_framing.clone()));
            }
        }
    }
}

/// Apply `CameraShake` as a per-frame world-space jitter added to the camera
/// transform AFTER `camera_follow_target` writes the smoothed position.
/// Intensity decays linearly toward zero over `total` seconds.
fn apply_camera_shake(
    mut shake: ResMut<CameraShake>,
    mut cam_q: Query<&mut Transform, With<IsometricCamera>>,
    time: Res<Time>,
) {
    if shake.remaining <= 0.0 {
        if shake.intensity != 0.0 {
            shake.intensity = 0.0;
            shake.total = 0.0;
        }
        return;
    }
    let amp = shake.amplitude();
    let dt = time.delta_secs();
    shake.remaining = (shake.remaining - dt).max(0.0);
    let Ok(mut cam_xf) = cam_q.single_mut() else { return };
    // Cheap deterministic noise from elapsed time. Not seeded — fine for
    // a brief visual shake. Each axis decorrelates via different multipliers.
    let t = time.elapsed_secs() * 30.0;
    let jx = (t.sin() * 1.7 + (t * 1.3).cos() * 0.5) * amp;
    let jy = ((t * 1.1).sin() * 0.6 + (t * 0.7).cos()) * amp * 0.5;
    let jz = ((t * 0.9).cos() * 1.5 + (t * 1.5).sin() * 0.4) * amp;
    cam_xf.translation += Vec3::new(jx, jy, jz);
}

/// Tween the camera toward `CameraTarget` each frame. `Free` is a no-op
/// (user-driven panning lives in `camera_pan_zoom`). For `LookAt` / `Follow`,
/// we target `subject + framing.offset` for translation and `framing.scale`
/// for orthographic zoom; both lerp exponentially.
///
/// For `Follow`, units/buildings carry their world position in `UnitPos` /
/// `BuildingPos` grid components rather than a `Transform` (visual smoothing
/// happens on a separate render entity); fall through to `Transform` only for
/// hypothetical non-grid followables.
fn camera_follow_target(
    target: Res<CameraTarget>,
    mut cam_q: Query<(&mut Transform, &mut Projection), With<IsometricCamera>>,
    unit_pos_q: Query<&UnitPos>,
    building_pos_q: Query<&BuildingPos>,
    transforms_q: Query<&Transform, Without<IsometricCamera>>,
    time: Res<Time>,
) {
    let (look_at_world, framing) = match &*target {
        CameraTarget::Free => return,
        CameraTarget::LookAt { point, framing } => (*point, *framing),
        CameraTarget::Follow { entity, framing } => {
            if let Ok(p) = unit_pos_q.get(*entity) {
                (grid_to_world(p.pos.x, p.pos.y), *framing)
            } else if let Ok(p) = building_pos_q.get(*entity) {
                (grid_to_world(p.pos.x, p.pos.y), *framing)
            } else if let Ok(t) = transforms_q.get(*entity) {
                (t.translation, *framing)
            } else {
                return;
            }
        }
    };

    let Ok((mut cam_xf, mut projection)) = cam_q.single_mut() else { return };
    let desired = look_at_world + framing.offset;
    // Frame-rate-independent exponential lerp.
    let alpha = (1.0 - (-time.delta_secs() * 6.0).exp()).clamp(0.0, 1.0);
    cam_xf.translation = cam_xf.translation.lerp(desired, alpha);
    cam_xf.look_at(look_at_world, Vec3::Y);

    if let Projection::Orthographic(ref mut ortho) = *projection {
        ortho.scale = ortho.scale + (framing.scale - ortho.scale) * alpha;
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
    gen: Res<GeneratePanel>,
    tiles: Query<(Entity, &Tile)>,
    units: Query<Entity, With<UnitTypeId>>,
    buildings: Query<Entity, With<BuildingTypeId>>,
    units_full: Query<(&UnitPos, &Faction, &UnitTypeId)>,
    buildings_full: Query<(&BuildingPos, &Faction, &BuildingTypeId)>,
    mut visual_entities: ResMut<VisualEntities>,
    rendered_tiles: Query<Entity, With<RenderedTile>>,
    mut entered_from_game: ResMut<EditorEnteredFromGame>,
    mut paused: ResMut<Paused>,
    mut time_virtual: ResMut<Time<Virtual>>,
) {
    if *screen != ClientScreen::MapEditor {
        return;
    }

    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);

    // Escape → return to title (or back to paused game if entered from game)
    if keys.just_pressed(KeyCode::Escape) {
        if editor.script_editing_field.is_some() {
            // Cancel field editing
            editor.script_editing_field = None;
            editor.script_field_buffer.clear();
            return;
        }
        if entered_from_game.0 {
            // Return to paused in-game state
            entered_from_game.0 = false;
            paused.0 = true;
            time_virtual.pause();
            *screen = ClientScreen::InMission;
            return;
        }
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

    // Handle script editor field input mode
    if editor.script_editing_field.is_some() {
        // Accept character input into buffer
        // Handle backspace
        if keys.just_pressed(KeyCode::Backspace) {
            editor.script_field_buffer.pop();
        }
        // Handle Enter to confirm
        if keys.just_pressed(KeyCode::Enter) {
            let buf = editor.script_field_buffer.clone();
            let field = editor.script_editing_field.take().unwrap();
            let sel = editor.script_selected;
            let action_sel = editor.script_action_selected;
            if sel < editor.script_events.len() {
                match field {
                    ScriptField::TriggerSeconds => {
                        if let Ok(v) = buf.parse::<f32>() {
                            editor.script_events[sel].trigger_seconds = v;
                        }
                    }
                    ScriptField::TriggerBeat => {
                        editor.script_events[sel].trigger_beat = buf.clone();
                    }
                    ScriptField::ActionType => {
                        if action_sel < editor.script_events[sel].actions.len() {
                            editor.script_events[sel].actions[action_sel].action_type = buf.clone();
                        }
                    }
                    ScriptField::ActionText => {
                        if action_sel < editor.script_events[sel].actions.len() {
                            editor.script_events[sel].actions[action_sel].text = buf.clone();
                        }
                    }
                    ScriptField::ActionFaction => {
                        if action_sel < editor.script_events[sel].actions.len() {
                            editor.script_events[sel].actions[action_sel].faction = buf.clone();
                        }
                    }
                    ScriptField::ActionUnitType => {
                        if action_sel < editor.script_events[sel].actions.len() {
                            editor.script_events[sel].actions[action_sel].unit_type = buf.clone();
                        }
                    }
                    ScriptField::ActionCount => {
                        if let Ok(v) = buf.parse::<u32>() {
                            if action_sel < editor.script_events[sel].actions.len() {
                                editor.script_events[sel].actions[action_sel].count = v;
                            }
                        }
                    }
                    ScriptField::ActionX => {
                        if let Ok(v) = buf.parse::<i32>() {
                            if action_sel < editor.script_events[sel].actions.len() {
                                editor.script_events[sel].actions[action_sel].x = v;
                            }
                        }
                    }
                    ScriptField::ActionY => {
                        if let Ok(v) = buf.parse::<i32>() {
                            if action_sel < editor.script_events[sel].actions.len() {
                                editor.script_events[sel].actions[action_sel].y = v;
                            }
                        }
                    }
                }
            }
            editor.script_field_buffer.clear();
        }
        // Note: character typing is handled via a separate approach — we just use keyboard keys
        // Since we can't get char events here easily, we rely on the user pressing keys
        // and map them to characters in a limited way
        return;
    }

    // Tool selection
    if keys.just_pressed(KeyCode::Digit1) { editor.tool = EditorTool::PaintTerrain; }
    if keys.just_pressed(KeyCode::Digit2) { editor.tool = EditorTool::PlaceUnit; }
    if keys.just_pressed(KeyCode::Digit3) { editor.tool = EditorTool::PlaceBuilding; }
    if keys.just_pressed(KeyCode::Digit4) { editor.tool = EditorTool::Erase; }
    if keys.just_pressed(KeyCode::Digit5) { editor.tool = EditorTool::ScriptEditor; }
    if keys.just_pressed(KeyCode::Digit6) { editor.tool = EditorTool::CampaignEditor; }

    // G — open generate panel (handled in handle_generate_panel system)

    // ── Script editor controls (only when tool 5 is active) ───────────────────
    if editor.tool == EditorTool::ScriptEditor {
        let event_count = editor.script_events.len();

        // Up/Down: navigate event list
        if keys.just_pressed(KeyCode::ArrowUp) && editor.script_selected > 0 {
            editor.script_selected -= 1;
            editor.script_action_selected = 0;
        }
        if keys.just_pressed(KeyCode::ArrowDown) && event_count > 0 && editor.script_selected < event_count - 1 {
            editor.script_selected += 1;
            editor.script_action_selected = 0;
        }

        // N: create new event
        if keys.just_pressed(KeyCode::KeyN) && !ctrl {
            let id = format!("event_{}", editor.script_events.len());
            editor.script_events.push(ScriptEventDef {
                id,
                trigger_type: "time".to_string(),
                trigger_seconds: 0.0,
                trigger_beat: String::new(),
                actions: vec![ActionDef {
                    action_type: "dialogue".to_string(),
                    text: String::new(),
                    ..Default::default()
                }],
            });
            editor.script_selected = editor.script_events.len() - 1;
            editor.script_action_selected = 0;
        }

        // Delete: remove selected event
        if keys.just_pressed(KeyCode::Delete) && event_count > 0 && editor.script_selected < event_count {
            let sel = editor.script_selected;
            editor.script_events.remove(sel);
            let new_count = editor.script_events.len();
            if sel > 0 && sel >= new_count {
                editor.script_selected = new_count.saturating_sub(1);
            }
            editor.script_action_selected = 0;
        }

        // Enter: start editing trigger field for selected event
        if keys.just_pressed(KeyCode::Enter) && editor.script_selected < event_count {
            let sel = editor.script_selected;
            let (ttype, tseconds, tbeat) = {
                let ev = &editor.script_events[sel];
                (ev.trigger_type.clone(), ev.trigger_seconds, ev.trigger_beat.clone())
            };
            if ttype == "beat" {
                editor.script_field_buffer = tbeat;
                editor.script_editing_field = Some(ScriptField::TriggerBeat);
            } else {
                editor.script_field_buffer = tseconds.to_string();
                editor.script_editing_field = Some(ScriptField::TriggerSeconds);
            }
        }

        // A: add new action to selected event
        if keys.just_pressed(KeyCode::KeyA) && editor.script_selected < event_count {
            let sel = editor.script_selected;
            editor.script_events[sel].actions.push(ActionDef {
                action_type: "dialogue".to_string(),
                ..Default::default()
            });
        }

        // X: remove selected action
        if keys.just_pressed(KeyCode::KeyX) && editor.script_selected < event_count {
            let sel = editor.script_selected;
            let action_sel = editor.script_action_selected;
            let action_count = editor.script_events[sel].actions.len();
            if action_sel < action_count {
                editor.script_events[sel].actions.remove(action_sel);
                let new_count = editor.script_events[sel].actions.len();
                if action_sel > 0 && action_sel >= new_count {
                    editor.script_action_selected = new_count.saturating_sub(1);
                }
            }
        }

        // Tab: cycle selected action
        if keys.just_pressed(KeyCode::Tab) && editor.script_selected < event_count {
            let sel = editor.script_selected;
            let action_count = editor.script_events[sel].actions.len();
            if action_count > 0 {
                editor.script_action_selected = (editor.script_action_selected + 1) % action_count;
            }
        }
    }

    // Cycle faction (F)
    if keys.just_pressed(KeyCode::KeyF) {
        editor.faction_idx = (editor.faction_idx + 1) % EDITOR_FACTION_IDS.len();
    }
    // Cycle unit type (T)
    if keys.just_pressed(KeyCode::KeyT) {
        editor.unit_type_idx = (editor.unit_type_idx + 1) % EDITOR_UNIT_TYPE_IDS.len();
    }
    // Cycle building type (B)
    if keys.just_pressed(KeyCode::KeyB) {
        editor.building_type_idx = (editor.building_type_idx + 1) % EDITOR_BUILDING_TYPE_IDS.len();
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

        let saved_spawn_zones: Vec<SavedSpawnZone> = gen.last_spawn_zones.iter().map(|z| SavedSpawnZone {
            id: z.id,
            x: z.x,
            y: z.y,
            clear_radius: z.clear_radius,
            suggested_team: z.suggested_team,
        }).collect();

        let map = SavedMap {
            tiles: saved_tiles,
            units: saved_units,
            buildings: saved_buildings,
            mission_type: None,
            player_faction: None,
            opponent_faction: None,
            deadline_seconds: None,
            briefing_override: None,
            win_override: None,
            loss_override: None,
            mission_index_override: None,
            script_events: editor.script_events.clone(),
            spawn_zones: saved_spawn_zones,
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
                        let faction = parse_faction_name(&su.faction).unwrap_or_else(Faction::combine);
                        spawn_editor_unit(&mut commands, su.x, su.y, faction, &su.unit_type);
                    }

                    // Spawn loaded buildings
                    for sb in &map.buildings {
                        let faction = parse_faction_name(&sb.faction).unwrap_or_else(Faction::combine);
                        spawn_editor_building(&mut commands, sb.x, sb.y, faction, &sb.building_type);
                    }

                    // Load script events
                    editor.script_events = map.script_events.clone();
                    editor.script_selected = 0;
                    editor.script_action_selected = 0;

                    info!("editor: loaded map from {path}");
                }
            }
        }
    }

    // P — Test Mission: save current editor state to a temp file and start a mission from it.
    if keys.just_pressed(KeyCode::KeyP) {
        // Collect current map state into a SavedMap.
        let saved_tiles: Vec<SavedTile> = tiles.iter().map(|(_, t)| SavedTile {
            x: t.pos.x,
            y: t.pos.y,
            terrain: terrain_type_name(&t.terrain_type).to_string(),
        }).collect();
        let saved_units: Vec<SavedUnit> = units_full.iter().map(|(pos, fac, ut)| SavedUnit {
            x: pos.pos.x,
            y: pos.pos.y,
            faction: map_faction_name(fac).to_string(),
            unit_type: unit_type_name(ut).to_string(),
        }).collect();
        let saved_buildings: Vec<SavedBuilding> = buildings_full.iter().map(|(pos, fac, bt)| SavedBuilding {
            x: pos.pos.x,
            y: pos.pos.y,
            faction: map_faction_name(fac).to_string(),
            building_type: building_type_name(bt).to_string(),
        }).collect();

        let map = SavedMap {
            tiles: saved_tiles,
            units: saved_units,
            buildings: saved_buildings,
            mission_type: None,
            player_faction: None,
            opponent_faction: None,
            deadline_seconds: None,
            briefing_override: None,
            win_override: None,
            loss_override: None,
            mission_index_override: None,
            script_events: editor.script_events.clone(),
            spawn_zones: gen.last_spawn_zones.iter().map(|z| SavedSpawnZone {
                id: z.id,
                x: z.x,
                y: z.y,
                clear_radius: z.clear_radius,
                suggested_team: z.suggested_team,
            }).collect(),
        };

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = format!("assets/maps/test_{}.toml", timestamp);

        // Also write a script TOML if there are any events
        let script_events_clone = editor.script_events.clone();
        let script_path = if !script_events_clone.is_empty() {
            let sp = format!("assets/scripts/editor_{}.toml", timestamp);
            let script_toml = script_events_to_toml(&script_events_clone);
            if let Err(e) = std::fs::create_dir_all("assets/scripts") {
                eprintln!("editor: failed to create scripts dir: {e}");
                None
            } else if let Err(e) = std::fs::write(&sp, &script_toml) {
                eprintln!("editor: failed to save script: {e}");
                None
            } else {
                info!("editor: saved script to {sp}");
                Some(sp)
            }
        } else {
            None
        };

        if let Ok(content) = toml::to_string(&map) {
            if let Err(e) = std::fs::create_dir_all("assets/maps") {
                eprintln!("editor: failed to create maps dir: {e}");
            } else if let Err(e) = std::fs::write(&path, &content) {
                eprintln!("editor: failed to save test map: {e}");
            } else {
                info!("editor: starting test mission from {path}");
                let saved_path = path.clone();
                // Queue world command to load tiles into the world and start a mission.
                commands.queue(move |world: &mut World| {
                    cindertide::wipe_world_entities(world);

                    // Load the saved map into the world.
                    if let Ok(file_content) = std::fs::read_to_string(&saved_path) {
                        if let Ok(saved) = toml::from_str::<SavedMap>(&file_content) {
                            for st in &saved.tiles {
                                if let Some(terrain) = parse_terrain_name(&st.terrain) {
                                    world.spawn(Tile {
                                        pos: GridPos { x: st.x, y: st.y },
                                        terrain_type: terrain,
                                        cover: cindertide::map::CoverDensity::None,
                                    });
                                }
                            }
                        }
                    }

                    // Use mission_type / faction from saved fields or defaults.
                    let player = Faction::combine();
                    let opponent = Faction::ironborn();
                    let mission_index = 0usize;

                    world.spawn(FactionBundle::new(player.clone()));

                    let mission_entity = world.spawn(Mission {
                        mission_type: cindertide::mapgen::MissionType::Assault,
                        player_faction: player.clone(),
                        opponent_faction: opponent.clone(),
                        status: MissionStatus::Active,
                        elapsed: 0.0,
                        deadline: 300.0,
                        hill_timer: 0.0,
                        hill_threshold: 180.0,
                        assassination_target: None,
                        ffa_check_timer: 0.0,
                    }).id();

                    // Spawn faction loadouts for both sides.
                    cindertide::setup_demo_scenario(world, &player, mission_index);
                    cindertide::bake_navmesh(world);

                    // Load the editor script if one was written
                    if let Some(ref sp) = script_path {
                        // Extract just the stem (filename without path prefix and .toml)
                        let script_name = std::path::Path::new(sp)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("editor_0")
                            .to_string();
                        world.resource_mut::<ScriptState>().load_script(&script_name);
                    }

                    world.resource_mut::<ActiveRun>().current_mission_entity = Some(mission_entity);
                    *world.resource_mut::<GameState>() = GameState::InMission;
                    world.insert_resource(PlayerFaction(player));
                    *world.resource_mut::<ClientScreen>() = ClientScreen::TestMission {
                        saved_map_path: saved_path,
                    };
                });
            }
        }
    }
}

// ── Campaign editor systems ───────────────────────────────────────────────────

/// Handle C key (or active CampaignEditor tool + Enter) in map editor to open campaign editor.
fn handle_editor_open_campaign(
    keys: Res<ButtonInput<KeyCode>>,
    mut screen: ResMut<ClientScreen>,
    mut campaign_editor: ResMut<CampaignEditorState>,
    loaded_campaigns: Res<LoadedCampaigns>,
    editor: Res<EditorState>,
) {
    if *screen != ClientScreen::MapEditor {
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let open = (keys.just_pressed(KeyCode::KeyC) && !ctrl)
        || (editor.tool == EditorTool::CampaignEditor && keys.just_pressed(KeyCode::Enter));
    if open {
        campaign_editor.campaigns = loaded_campaigns.0.clone();
        campaign_editor.campaign_selected = 0;
        campaign_editor.mission_selected = 0;
        campaign_editor.focus_missions = false;
        campaign_editor.input_buffer.clear();
        campaign_editor.input_prompt = None;
        campaign_editor.delete_confirm = false;
        campaign_editor.status = String::new();
        *screen = ClientScreen::CampaignEditor;
    }
}

/// Handle the generate-from-archetype panel (G key toggle, navigation, Enter to generate).
fn handle_generate_panel(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    screen: Res<ClientScreen>,
    mut gen: ResMut<GeneratePanel>,
    tiles: Query<(Entity, &Tile)>,
    units: Query<Entity, With<UnitTypeId>>,
    buildings: Query<Entity, With<BuildingTypeId>>,
    rendered_tiles: Query<Entity, With<RenderedTile>>,
    spawn_markers: Query<Entity, With<SpawnMarker>>,
    mut visual_entities: ResMut<VisualEntities>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if *screen != ClientScreen::MapEditor {
        // If we leave the editor while panel is open, close it.
        if gen.open { gen.open = false; }
        return;
    }

    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);

    // G — toggle panel (don't open when ctrl is held)
    if keys.just_pressed(KeyCode::KeyG) && !ctrl {
        if !gen.open {
            gen.archetypes = scan_archetypes();
            gen.open = true;
            gen.selected = 0;
        } else {
            gen.open = false;
        }
        return;
    }

    if !gen.open {
        return;
    }

    // Esc — close panel
    if keys.just_pressed(KeyCode::Escape) {
        gen.open = false;
        return;
    }

    let arch_count = gen.archetypes.len();

    // Up/Down: select archetype
    if keys.just_pressed(KeyCode::ArrowUp) && gen.selected > 0 {
        gen.selected -= 1;
    }
    if keys.just_pressed(KeyCode::ArrowDown) && arch_count > 0 && gen.selected + 1 < arch_count {
        gen.selected += 1;
    }

    // Left/Right: adjust width and height
    if keys.just_pressed(KeyCode::ArrowLeft) {
        if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
            gen.height = (gen.height - 8).max(20);
        } else {
            gen.width = (gen.width - 8).max(20);
        }
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
            gen.height = (gen.height + 8).min(256);
        } else {
            gen.width = (gen.width + 8).min(256);
        }
    }

    // R — randomize seed
    if keys.just_pressed(KeyCode::KeyR) {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(12345);
        gen.seed = t ^ (t >> 17) ^ (t << 31);
    }

    // Enter — generate
    if keys.just_pressed(KeyCode::Enter) && arch_count > 0 {
        let sel = gen.selected.min(arch_count - 1);
        let def = gen.archetypes[sel].clone();
        let width = gen.width;
        let height = gen.height;
        let seed = gen.seed;

        // Wipe existing world entities
        for (entity, _) in &tiles { commands.entity(entity).despawn(); }
        for entity in &units { commands.entity(entity).despawn(); }
        for entity in &buildings { commands.entity(entity).despawn(); }
        for entity in &rendered_tiles { commands.entity(entity).despawn(); }
        for entity in &spawn_markers { commands.entity(entity).despawn(); }
        visual_entities.units.clear();
        visual_entities.buildings.clear();

        // Generate the map
        let generated = generate_from_archetype(&def, &std::collections::HashMap::new(), width, height, seed);

        // Spawn tiles
        for y in 0..height {
            for x in 0..width {
                let terrain = generated.tiles.get(&(x, y))
                    .cloned()
                    .unwrap_or(cindertide::map::TerrainType::Grass);
                let cover = match &terrain {
                    cindertide::map::TerrainType::Forest | cindertide::map::TerrainType::Rubble
                        => cindertide::map::CoverDensity::Heavy,
                    cindertide::map::TerrainType::Road | cindertide::map::TerrainType::Grass
                        => cindertide::map::CoverDensity::None,
                    _ => cindertide::map::CoverDensity::Light,
                };
                commands.spawn(Tile {
                    pos: GridPos { x, y },
                    terrain_type: terrain,
                    cover,
                });
            }
        }

        // Spawn 3D marker cylinders for each spawn zone
        let tile_size = 1.0f32;
        for zone in &generated.spawn_zones {
            let color = match zone.suggested_team {
                Some(0) => Color::srgb(1.0, 1.0, 0.0),   // yellow
                Some(1) => Color::srgb(1.0, 0.2, 0.2),   // red
                Some(2) => Color::srgb(0.2, 0.4, 1.0),   // blue
                Some(3) => Color::srgb(0.2, 0.9, 0.2),   // green
                _       => Color::srgb(1.0, 1.0, 1.0),   // white (FFA)
            };
            let wx = zone.x as f32 * tile_size;
            let wz = zone.y as f32 * tile_size;
            commands.spawn((
                Mesh3d(meshes.add(Cylinder::new(zone.clear_radius as f32 * tile_size, 0.3))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: color.with_alpha(0.5),
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                })),
                Transform::from_xyz(wx, 0.2, wz),
                SpawnMarker { zone_id: zone.id },
            ));
        }

        info!(
            "Generated map from archetype '{}' ({}x{}, seed {}), {} spawn zones",
            def.name, width, height, seed, generated.spawn_zones.len()
        );
        gen.last_spawn_zones = generated.spawn_zones;
        gen.open = false;
    }
}

/// Handle keyboard input for the campaign editor overlay.
fn handle_campaign_editor_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut screen: ResMut<ClientScreen>,
    mut ce: ResMut<CampaignEditorState>,
    mut loaded_campaigns: ResMut<LoadedCampaigns>,
) {
    if *screen != ClientScreen::CampaignEditor {
        return;
    }

    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    // Handle input prompt mode
    if ce.input_prompt.is_some() {
        if keys.just_pressed(KeyCode::Escape) {
            ce.input_prompt = None;
            ce.input_buffer.clear();
            ce.new_name_buffer.clear();
            ce.status = "Cancelled.".to_string();
            return;
        }
        if keys.just_pressed(KeyCode::Backspace) {
            ce.input_buffer.pop();
            return;
        }
        if keys.just_pressed(KeyCode::Enter) {
            let buf = ce.input_buffer.clone();
            match ce.input_prompt.clone().unwrap() {
                CampaignEditorPrompt::NewCampaignId => {
                    // Move to name prompt
                    ce.new_name_buffer = buf.clone();
                    ce.input_buffer.clear();
                    ce.input_prompt = Some(CampaignEditorPrompt::NewCampaignName);
                    ce.status = format!("Enter name for campaign '{}':", buf);
                    return;
                }
                CampaignEditorPrompt::NewCampaignName => {
                    let id = ce.new_name_buffer.clone();
                    let name = buf.clone();
                    if !id.is_empty() {
                        let new_campaign = CampaignDef {
                            id: id.clone(),
                            name,
                            faction: "Combine".to_string(),
                            description: String::new(),
                            unlock_requires: String::new(),
                            missions: Vec::new(),
                        };
                        ce.campaigns.push(new_campaign);
                        ce.campaigns.sort_by(|a, b| a.id.cmp(&b.id));
                        ce.campaign_selected = ce.campaigns.iter().position(|c| c.id == id).unwrap_or(0);
                        ce.status = format!("Created campaign '{}'.", id);
                    }
                    ce.input_buffer.clear();
                    ce.new_name_buffer.clear();
                    ce.input_prompt = None;
                    return;
                }
                CampaignEditorPrompt::AddMapPath => {
                    if !buf.is_empty() {
                        let cam_idx = ce.campaign_selected;
                        if cam_idx < ce.campaigns.len() {
                            ce.campaigns[cam_idx].missions.push(cindertide::campaign::CampaignMissionDef { map: buf.clone() });
                            let new_len = ce.campaigns[cam_idx].missions.len();
                            ce.mission_selected = new_len.saturating_sub(1);
                            ce.status = format!("Added map: {}", buf);
                        }
                    }
                    ce.input_buffer.clear();
                    ce.input_prompt = None;
                    return;
                }
            }
        }
        // Character input — map key codes to characters (basic ASCII)
        let char_input = campaign_editor_char_input(&keys);
        if let Some(ch) = char_input {
            ce.input_buffer.push(ch);
        }
        return;
    }

    // Escape — back to map editor
    if keys.just_pressed(KeyCode::Escape) {
        ce.delete_confirm = false;
        *screen = ClientScreen::MapEditor;
        return;
    }

    let campaign_count = ce.campaigns.len();

    // Tab — toggle focus between campaign list and mission list
    if keys.just_pressed(KeyCode::Tab) {
        if campaign_count > 0 {
            ce.focus_missions = !ce.focus_missions;
        }
        return;
    }

    // Navigation
    if keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW) {
        if !ce.focus_missions {
            if ce.campaign_selected > 0 {
                ce.campaign_selected -= 1;
                ce.mission_selected = 0;
            }
        } else {
            let cam_idx = ce.campaign_selected;
            let mission_count = ce.campaigns.get(cam_idx).map(|c| c.missions.len()).unwrap_or(0);
            if !shift {
                if ce.mission_selected > 0 {
                    ce.mission_selected -= 1;
                }
            } else if ce.mission_selected > 0 {
                // Shift+Up: reorder mission up
                let idx = ce.mission_selected;
                if let Some(c) = ce.campaigns.get_mut(cam_idx) {
                    c.missions.swap(idx, idx - 1);
                }
                ce.mission_selected -= 1;
                ce.status = "Moved mission up.".to_string();
            }
            let _ = mission_count;
        }
    }

    if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS) {
        if !ce.focus_missions {
            if ce.campaign_selected + 1 < campaign_count {
                ce.campaign_selected += 1;
                ce.mission_selected = 0;
            }
        } else {
            let cam_idx = ce.campaign_selected;
            let mission_count = ce.campaigns.get(cam_idx).map(|c| c.missions.len()).unwrap_or(0);
            if !shift {
                if ce.mission_selected + 1 < mission_count {
                    ce.mission_selected += 1;
                }
            } else if ce.mission_selected + 1 < mission_count {
                // Shift+Down: reorder mission down
                let idx = ce.mission_selected;
                if let Some(c) = ce.campaigns.get_mut(cam_idx) {
                    c.missions.swap(idx, idx + 1);
                }
                ce.mission_selected += 1;
                ce.status = "Moved mission down.".to_string();
            }
        }
    }

    // N — new campaign
    if keys.just_pressed(KeyCode::KeyN) && !ctrl {
        ce.input_prompt = Some(CampaignEditorPrompt::NewCampaignId);
        ce.input_buffer.clear();
        ce.status = "Enter new campaign id (e.g. 'combine'):".to_string();
        return;
    }

    // Delete — delete selected campaign (with confirm)
    if keys.just_pressed(KeyCode::Delete) && !ce.focus_missions {
        if ce.delete_confirm {
            if ce.campaign_selected < ce.campaigns.len() {
                let cam_sel = ce.campaign_selected;
                let removed_id = ce.campaigns[cam_sel].id.clone();
                ce.campaigns.remove(cam_sel);
                if cam_sel > 0 && cam_sel >= ce.campaigns.len() {
                    ce.campaign_selected -= 1;
                }
                ce.mission_selected = 0;
                ce.status = format!("Deleted campaign '{}'.", removed_id);
            }
            ce.delete_confirm = false;
        } else {
            ce.delete_confirm = true;
            ce.status = "Press Del again to confirm deletion.".to_string();
        }
        return;
    } else if keys.just_pressed(KeyCode::Delete) && ce.focus_missions {
        // X or Del in mission focus — remove selected mission
        let cam_idx = ce.campaign_selected;
        let mis_idx = ce.mission_selected;
        let mission_count = ce.campaigns.get(cam_idx).map(|c| c.missions.len()).unwrap_or(0);
        if mis_idx < mission_count {
            let removed_map = ce.campaigns[cam_idx].missions.remove(mis_idx).map;
            ce.status = format!("Removed mission: {}", removed_map);
            let new_count = ce.campaigns[cam_idx].missions.len();
            if mis_idx > 0 && mis_idx >= new_count {
                ce.mission_selected -= 1;
            }
        }
        return;
    }
    ce.delete_confirm = false;

    // M — add map entry to selected campaign
    if keys.just_pressed(KeyCode::KeyM) {
        if campaign_count > 0 {
            ce.input_prompt = Some(CampaignEditorPrompt::AddMapPath);
            ce.input_buffer.clear();
            ce.status = "Enter map path (e.g. maps/combine_m0.toml):".to_string();
        }
        return;
    }

    // X — remove selected mission
    if keys.just_pressed(KeyCode::KeyX) && ce.focus_missions {
        let cam_idx = ce.campaign_selected;
        let mis_idx = ce.mission_selected;
        let mission_count = ce.campaigns.get(cam_idx).map(|c| c.missions.len()).unwrap_or(0);
        if mis_idx < mission_count {
            let removed_map = ce.campaigns[cam_idx].missions.remove(mis_idx).map;
            ce.status = format!("Removed mission: {}", removed_map);
            let new_count = ce.campaigns[cam_idx].missions.len();
            if mis_idx > 0 && mis_idx >= new_count {
                ce.mission_selected -= 1;
            }
        }
        return;
    }

    // Ctrl+S — save selected campaign
    if ctrl && keys.just_pressed(KeyCode::KeyS) {
        let cam_idx = ce.campaign_selected;
        if cam_idx < ce.campaigns.len() {
            let (path, serialized) = {
                let campaign = &ce.campaigns[cam_idx];
                let path = format!("assets/campaigns/{}.toml", campaign.id);
                (path, toml::to_string(campaign))
            };
            match serialized {
                Ok(content) => {
                    if let Err(e) = std::fs::create_dir_all("assets/campaigns") {
                        ce.status = format!("Error: {e}");
                    } else if let Err(e) = std::fs::write(&path, content) {
                        ce.status = format!("Error saving: {e}");
                    } else {
                        loaded_campaigns.0 = CampaignDef::load_all();
                        ce.status = format!("Saved to {path}");
                    }
                }
                Err(e) => ce.status = format!("Serialize error: {e}"),
            }
        }
        return;
    }
}

/// Map key codes to ASCII characters for basic text input in campaign editor.
fn campaign_editor_char_input(keys: &Res<ButtonInput<KeyCode>>) -> Option<char> {
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    // Letters
    macro_rules! letter {
        ($code:ident, $lower:expr, $upper:expr) => {
            if keys.just_pressed(KeyCode::$code) {
                return Some(if shift { $upper } else { $lower });
            }
        };
    }
    letter!(KeyA, 'a', 'A'); letter!(KeyB, 'b', 'B'); letter!(KeyC, 'c', 'C');
    letter!(KeyD, 'd', 'D'); letter!(KeyE, 'e', 'E'); letter!(KeyF, 'f', 'F');
    letter!(KeyG, 'g', 'G'); letter!(KeyH, 'h', 'H'); letter!(KeyI, 'i', 'I');
    letter!(KeyJ, 'j', 'J'); letter!(KeyK, 'k', 'K'); letter!(KeyL, 'l', 'L');
    letter!(KeyM, 'm', 'M'); letter!(KeyN, 'n', 'N'); letter!(KeyO, 'o', 'O');
    letter!(KeyP, 'p', 'P'); letter!(KeyQ, 'q', 'Q'); letter!(KeyR, 'r', 'R');
    letter!(KeyS, 's', 'S'); letter!(KeyT, 't', 'T'); letter!(KeyU, 'u', 'U');
    letter!(KeyV, 'v', 'V'); letter!(KeyW, 'w', 'W'); letter!(KeyX, 'x', 'X');
    letter!(KeyY, 'y', 'Y'); letter!(KeyZ, 'z', 'Z');
    // Digits
    letter!(Digit0, '0', ')'); letter!(Digit1, '1', '!'); letter!(Digit2, '2', '@');
    letter!(Digit3, '3', '#'); letter!(Digit4, '4', '$'); letter!(Digit5, '5', '%');
    letter!(Digit6, '6', '^'); letter!(Digit7, '7', '&'); letter!(Digit8, '8', '*');
    letter!(Digit9, '9', '(');
    // Common punctuation
    if keys.just_pressed(KeyCode::Minus)     { return Some(if shift { '_' } else { '-' }); }
    if keys.just_pressed(KeyCode::Period)    { return Some(if shift { '>' } else { '.' }); }
    if keys.just_pressed(KeyCode::Slash)     { return Some(if shift { '?' } else { '/' }); }
    if keys.just_pressed(KeyCode::Space)     { return Some(' '); }
    if keys.just_pressed(KeyCode::Comma)     { return Some(if shift { '<' } else { ',' }); }
    None
}

/// Update the campaign editor overlay (full-screen text display).
fn update_campaign_editor_overlay(
    screen: Res<ClientScreen>,
    ce: Res<CampaignEditorState>,
    progress: Res<GlobalProgress>,
    mut overlay_vis: Query<&mut Visibility, With<ScreenOverlay>>,
    mut title_text: Query<&mut Text, (With<OverlayTitleText>, Without<OverlayBodyText>, Without<OverlayHintText>)>,
    mut body_text: Query<&mut Text, (With<OverlayBodyText>, Without<OverlayTitleText>, Without<OverlayHintText>)>,
    mut hint_text: Query<&mut Text, (With<OverlayHintText>, Without<OverlayTitleText>, Without<OverlayBodyText>)>,
) {
    if *screen != ClientScreen::CampaignEditor {
        return;
    }

    let Ok(mut vis) = overlay_vis.single_mut() else { return };
    let Ok(mut title) = title_text.single_mut() else { return };
    let Ok(mut body) = body_text.single_mut() else { return };
    let Ok(mut hint) = hint_text.single_mut() else { return };

    *vis = Visibility::Visible;
    **title = "CAMPAIGN EDITOR".to_string();

    let mut lines = Vec::<String>::new();

    // Campaign list
    lines.push(format!("Campaigns:  [N]ew  [Del]ete"));
    lines.push(String::new());

    if ce.campaigns.is_empty() {
        lines.push("  (no campaigns)".to_string());
    } else {
        for (i, campaign) in ce.campaigns.iter().enumerate() {
            let selected = i == ce.campaign_selected;
            let focus_mark = if selected && !ce.focus_missions { ">" } else { " " };
            let mission_count = campaign.missions.len();
            let lock_str = if campaign.is_unlocked(&progress) { "" } else { ", LOCKED" };
            lines.push(format!(
                "{} {:12} \"{}\"\t[{} missions{}]",
                focus_mark, campaign.id, campaign.name, mission_count, lock_str
            ));
        }
    }

    // Show selected campaign details
    if let Some(campaign) = ce.campaigns.get(ce.campaign_selected) {
        lines.push(String::new());
        lines.push(format!("-- Selected: {} --", campaign.id));
        lines.push(format!("Name: {}", campaign.name));
        lines.push(format!("Faction: {}", campaign.faction));
        lines.push(format!("Description: {}", campaign.description));
        let req = if campaign.unlock_requires.is_empty() { "(none)".to_string() } else { campaign.unlock_requires.clone() };
        lines.push(format!("Unlock requires: {}", req));
        lines.push(String::new());
        lines.push("Missions:".to_string());

        if campaign.missions.is_empty() {
            lines.push("  (no missions)".to_string());
        } else {
            for (j, mission) in campaign.missions.iter().enumerate() {
                let sel = j == ce.mission_selected && ce.focus_missions;
                let marker = if sel { ">" } else { " " };
                lines.push(format!("{} [{}] {}", marker, j, mission.map));
            }
        }
    }

    // Input prompt
    if let Some(ref prompt) = ce.input_prompt {
        lines.push(String::new());
        let label = match prompt {
            CampaignEditorPrompt::NewCampaignId => "Campaign ID",
            CampaignEditorPrompt::NewCampaignName => "Campaign Name",
            CampaignEditorPrompt::AddMapPath => "Map Path",
        };
        lines.push(format!("{}: {}_", label, ce.input_buffer));
    }

    // Status line
    if !ce.status.is_empty() {
        lines.push(String::new());
        lines.push(format!("  {}", ce.status));
    }

    **body = lines.join("\n");
    **hint = "[Tab] focus  [↑↓] nav  [N] new  [Del] delete  [M] add map  [X] rm map  [Shift+↑↓] reorder  [Ctrl+S] save  [Esc] back".to_string();
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

/// Serialize a list of ScriptEventDef to TOML matching the combine_m0.toml format.
fn script_events_to_toml(events: &[ScriptEventDef]) -> String {
    let mut out = String::new();
    for ev in events {
        out.push_str("[[events]]\n");
        out.push_str(&format!("id = {:?}\n", ev.id));
        if ev.trigger_type == "beat" {
            out.push_str(&format!(
                "trigger = {{ type = \"condition\", condition = \"beat\", beat_id = {:?} }}\n",
                ev.trigger_beat
            ));
        } else {
            out.push_str(&format!(
                "trigger = {{ type = \"time\", seconds = {} }}\n",
                ev.trigger_seconds
            ));
        }
        out.push_str("actions = [\n");
        for action in &ev.actions {
            match action.action_type.as_str() {
                "dialogue" => {
                    if action.speaker.is_empty() {
                        out.push_str(&format!("  {{ type = \"dialogue\", text = {:?} }},\n", action.text));
                    } else {
                        out.push_str(&format!("  {{ type = \"dialogue\", speaker = {:?}, text = {:?} }},\n", action.speaker, action.text));
                    }
                }
                "spawn_units" => {
                    out.push_str(&format!(
                        "  {{ type = \"spawn_units\", faction = {:?}, unit_type = {:?}, count = {}, x = {}, y = {} }},\n",
                        action.faction, action.unit_type, action.count, action.x, action.y
                    ));
                }
                "objective" => {
                    out.push_str(&format!("  {{ type = \"objective\", text = {:?} }},\n", action.text));
                }
                "change_objective" => {
                    out.push_str(&format!("  {{ type = \"change_objective\", text = {:?} }},\n", action.text));
                }
                "win_mission" => {
                    out.push_str("  { type = \"win_mission\" },\n");
                }
                "lose_mission" => {
                    out.push_str("  { type = \"lose_mission\" },\n");
                }
                _ => {}
            }
        }
        out.push_str("]\n\n");
    }
    out
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
        "Grass" | "grass" => Some(TerrainType::Grass),
        "Road" | "road" => Some(TerrainType::Road),
        "Forest" | "forest" => Some(TerrainType::Forest),
        "Rubble" | "rubble" => Some(TerrainType::Rubble),
        "Mud" | "mud" => Some(TerrainType::Mud),
        "Corrupted" | "corrupted" => Some(TerrainType::Corrupted),
        "Void" | "void" => Some(TerrainType::Void),
        _ => None,
    }
}

fn parse_faction_name(s: &str) -> Option<Faction> {
    if s.is_empty() { None } else { Some(Faction::new(&s.to_lowercase())) }
}

fn map_faction_name(f: &Faction) -> String {
    f.id().to_string()
}

fn unit_type_name(t: &UnitTypeId) -> &str {
    t.id()
}

fn building_type_name(t: &BuildingTypeId) -> &str {
    t.id()
}

/// Normalize a TOML/editor building/unit type string into the canonical snake_case id.
///
/// Handles all three input shapes that show up in the wild:
///   - "CommandBunker" (CamelCase from hand-authored map TOMLs) → "command_bunker"
///   - "command_bunker" (snake_case from editor exports)          → "command_bunker"
///   - "Tank Trap"     (space-separated display name)             → "tank_trap"
///
/// Earlier code used `to_lowercase().replace(' ', "_")` which silently produced
/// invalid ids like "commandbunker" (no underscore) for CamelCase inputs, causing
/// `bt.id() == "command_bunker"` checks across mission/beats/camera to never match.
fn normalize_type_id(s: &str) -> String {
    let mut out = String::new();
    let mut prev_was_break = true;
    for c in s.chars() {
        if c == ' ' || c == '-' || c == '_' {
            if !out.is_empty() && !out.ends_with('_') {
                out.push('_');
            }
            prev_was_break = true;
        } else if c.is_uppercase() {
            if !out.is_empty() && !prev_was_break && !out.ends_with('_') {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_was_break = false;
        } else {
            out.push(c);
            prev_was_break = false;
        }
    }
    out
}

/// Spawn a unit directly into the World (used for map loading during mission start).
fn spawn_unit_world(world: &mut World, x: i32, y: i32, faction: Faction, type_name: &str) {
    let unit_id = normalize_type_id(type_name);
    let loaded = world.resource::<LoadedFactions>().clone();
    if let Some(def) = loaded.units.get(&unit_id) {
        world.spawn(UnitBundle::from_def(def, faction, x, y));
    } else {
        world.spawn(UnitBundle::default_riflemen(faction, x, y));
    }
}

/// Spawn a building directly into the World (used for map loading during mission start).
fn spawn_building_world(world: &mut World, x: i32, y: i32, faction: Faction, type_name: &str) {
    let bt_id = normalize_type_id(type_name);
    let bt = BuildingTypeId::new(&bt_id);
    let loaded = world.resource::<LoadedFactions>().clone();
    world.spawn(BuildingBundle::new(bt, faction, x, y, &loaded));
}

fn spawn_editor_unit(commands: &mut Commands, x: i32, y: i32, faction: Faction, type_name: &str) {
    let unit_id = normalize_type_id(type_name);
    commands.spawn(UnitBundle::default_riflemen_id(&unit_id, faction, x, y));
}

fn spawn_editor_building(commands: &mut Commands, x: i32, y: i32, faction: Faction, type_name: &str) {
    let bt_id = normalize_type_id(type_name);
    let bt = BuildingTypeId::new(&bt_id);
    // No LoadedFactions access from Commands — use default health
    commands.spawn(BuildingBundle::new_default(bt, faction, x, y));
}

#[cfg(test)]
mod normalize_tests {
    use super::normalize_type_id;
    #[test] fn camel() { assert_eq!(normalize_type_id("CommandBunker"), "command_bunker"); }
    #[test] fn snake() { assert_eq!(normalize_type_id("command_bunker"), "command_bunker"); }
    #[test] fn spaced() { assert_eq!(normalize_type_id("Tank Trap"), "tank_trap"); }
    #[test] fn lower() { assert_eq!(normalize_type_id("barracks"), "barracks"); }
    #[test] fn hyphen() { assert_eq!(normalize_type_id("motor-pool"), "motor_pool"); }
    #[test] fn camel_3parts() { assert_eq!(normalize_type_id("HeavyWeapons"), "heavy_weapons"); }
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
    units: Query<(Entity, &UnitPos), With<UnitTypeId>>,
    buildings: Query<(Entity, &BuildingPos), With<BuildingTypeId>>,
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
                let faction = Faction::new(EDITOR_FACTION_IDS[editor.faction_idx]);
                let unit_type = EDITOR_UNIT_TYPE_IDS[editor.unit_type_idx];
                // Only place if tile exists
                if tiles.iter().any(|(_, t)| t.pos.x == gx && t.pos.y == gy) {
                    spawn_editor_unit(&mut commands, gx, gy, faction, unit_type);
                }
            }
        }
        EditorTool::PlaceBuilding => {
            if left_click {
                let faction = Faction::new(EDITOR_FACTION_IDS[editor.faction_idx]);
                let building_type = EDITOR_BUILDING_TYPE_IDS[editor.building_type_idx];
                if tiles.iter().any(|(_, t)| t.pos.x == gx && t.pos.y == gy) {
                    spawn_editor_building(&mut commands, gx, gy, faction, building_type);
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
        EditorTool::ScriptEditor => {
            // Mouse clicks are not used in script editor mode
        }
        EditorTool::CampaignEditor => {
            // Mouse clicks are not used in campaign editor tool mode
        }
    }
}

/// Update the editor panel text with current state info.
fn update_editor_panel(
    screen: Res<ClientScreen>,
    editor: Res<EditorState>,
    gen: Res<GeneratePanel>,
    tiles: Query<&Tile>,
    units: Query<&UnitPos, With<UnitTypeId>>,
    buildings: Query<&BuildingPos, With<BuildingTypeId>>,
    mut panel_vis: Query<&mut Visibility, With<EditorPanel>>,
    mut panel_text: Query<&mut Text, With<EditorPanelText>>,
) {
    let Ok(mut vis) = panel_vis.single_mut() else { return };

    if *screen != ClientScreen::MapEditor {
        *vis = Visibility::Hidden;
        return;
    }
    *vis = Visibility::Visible;

    if !screen.is_changed() && !editor.is_changed() && !gen.is_changed() {
        return;
    }

    let Ok(mut text) = panel_text.single_mut() else { return };

    // Generate panel overlay
    if gen.open {
        let mut lines = vec!["── GENERATE MAP ─────────────".to_string()];
        lines.push("Archetypes:".to_string());
        if gen.archetypes.is_empty() {
            lines.push("  (none found in assets/archetypes/)".to_string());
        } else {
            for (i, arch) in gen.archetypes.iter().enumerate() {
                let marker = if i == gen.selected { "> " } else { "  " };
                lines.push(format!("{}[{}] {}", marker, i, arch.name));
            }
        }
        lines.push(String::new());
        lines.push(format!("Width: {}  Height: {}  Seed: {}", gen.width, gen.height, gen.seed));
        lines.push(String::new());
        // Show last spawn zones if any
        if !gen.last_spawn_zones.is_empty() {
            let layout_hint = if gen.last_spawn_zones.len() == 2 {
                let teams: Vec<_> = gen.last_spawn_zones.iter().filter_map(|z| z.suggested_team).collect();
                if teams.len() == 2 && teams[0] != teams[1] { "1v1".to_string() } else { "FFA".to_string() }
            } else if gen.last_spawn_zones.len() == 4 {
                let team0 = gen.last_spawn_zones.iter().filter(|z| z.suggested_team == Some(0)).count();
                let team1 = gen.last_spawn_zones.iter().filter(|z| z.suggested_team == Some(1)).count();
                if team0 == 2 && team1 == 2 { "2v2".to_string() } else { "FFA".to_string() }
            } else {
                "FFA".to_string()
            };
            let team_names = ["Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel"];
            lines.push(format!("Spawn zones: {}  Teams: {}", gen.last_spawn_zones.len(), layout_hint));
            for zone in &gen.last_spawn_zones {
                let team_label = match zone.suggested_team {
                    Some(t) => team_names.get(t).copied().unwrap_or("?"),
                    None => "FFA",
                };
                lines.push(format!("  Zone {} @ ({}, {}) — Team {}", zone.id, zone.x, zone.y, team_label));
            }
            lines.push(String::new());
        }
        lines.push("[↑↓] select archetype".to_string());
        lines.push("[←→] adjust width".to_string());
        lines.push("[Shift+←→] adjust height".to_string());
        lines.push("[R] random seed".to_string());
        lines.push("[Enter] Generate".to_string());
        lines.push("[Esc] Cancel".to_string());
        **text = lines.join("\n");
        return;
    }

    // Campaign editor tool mode: show hint to open campaign editor
    if editor.tool == EditorTool::CampaignEditor {
        **text = "-- CAMPAIGN EDITOR --\n\n[Enter] Open campaign editor\n[C]     Open campaign editor\n\nPress Enter or C to\nlaunch the full campaign\neditor overlay.\n\n[Esc] Back to title".to_string();
        return;
    }

    // Script editor mode: show script event list
    if editor.tool == EditorTool::ScriptEditor {
        let mut lines = vec!["-- SCRIPT EDITOR --".to_string()];
        lines.push("[N] Add Event  [Del] Remove".to_string());
        lines.push(String::new());
        for (i, ev) in editor.script_events.iter().enumerate() {
            let trigger_str = if ev.trigger_type == "beat" {
                format!("beat:{}", ev.trigger_beat)
            } else {
                format!("t={:.1}s", ev.trigger_seconds)
            };
            let first_action = ev.actions.first().map(|a| a.action_type.as_str()).unwrap_or("(none)");
            let marker = if i == editor.script_selected { "> " } else { "  " };
            lines.push(format!("{}[{}] {} -> {}", marker, i, trigger_str, first_action));
        }
        if editor.script_events.is_empty() {
            lines.push("  (no events)".to_string());
        }
        lines.push(String::new());
        if editor.script_selected < editor.script_events.len() {
            let ev = &editor.script_events[editor.script_selected];
            lines.push("-- Selected Event --".to_string());
            let trigger_str = if ev.trigger_type == "beat" {
                format!("beat / {}", ev.trigger_beat)
            } else {
                format!("time / {:.1}s", ev.trigger_seconds)
            };
            lines.push(format!("Trigger: {trigger_str}"));
            lines.push("Actions:".to_string());
            for (j, action) in ev.actions.iter().enumerate() {
                let marker = if j == editor.script_action_selected { ">" } else { " " };
                let action_str = match action.action_type.as_str() {
                    "spawn_units" => format!(
                        "{} SpawnUnits {} {} x{} @({},{})",
                        marker, action.faction, action.unit_type, action.count, action.x, action.y
                    ),
                    "dialogue" => format!("{} Dialogue: {}", marker, &action.text[..action.text.len().min(20)]),
                    "objective" | "change_objective" => format!("{} Objective: {}", marker, &action.text[..action.text.len().min(20)]),
                    other => format!("{} {}", marker, other),
                };
                lines.push(format!("  {}", action_str));
            }
            if ev.actions.is_empty() {
                lines.push("  (no actions)".to_string());
            }
            if let Some(ref field) = editor.script_editing_field {
                lines.push(String::new());
                lines.push(format!("Editing {:?}:", field));
                lines.push(format!("> {}_", editor.script_field_buffer));
                lines.push("[Enter] confirm  [Esc] cancel".to_string());
            } else {
                lines.push(String::new());
                lines.push("[Enter] edit trigger".to_string());
                lines.push("[A] add action  [X] del action".to_string());
                lines.push("[Tab] cycle action".to_string());
                lines.push("[Up/Down] nav events".to_string());
            }
        }
        **text = lines.join("\n");
        return;
    }

    let tool_name = match &editor.tool {
        EditorTool::PaintTerrain => "1: Paint Terrain",
        EditorTool::PlaceUnit    => "2: Place Unit",
        EditorTool::PlaceBuilding => "3: Place Building",
        EditorTool::Erase        => "4: Erase",
        EditorTool::ScriptEditor => "5: Script Editor",
        EditorTool::CampaignEditor => "6: Campaign Editor",
    };

    let terrain_name = terrain_type_name(&EDITOR_TERRAINS[editor.terrain_idx]);
    let faction_name_str = EDITOR_FACTION_IDS[editor.faction_idx];
    let unit_name = EDITOR_UNIT_TYPE_IDS[editor.unit_type_idx];
    let building_name = EDITOR_BUILDING_TYPE_IDS[editor.building_type_idx];

    let tile_count = tiles.iter().count();
    let unit_count = units.iter().count();
    let building_count = buildings.iter().count();

    let del_hint = if editor.del_confirm_timer.is_some() {
        "\n[Del again to confirm clear]"
    } else {
        ""
    };

    **text = format!(
        "Tool: {tool_name}\n\nTerrain: {terrain_name}\nFaction: {faction_name_str}\nUnit: {unit_name}\nBuilding: {building_name}\n\nTiles: {tile_count}\nUnits: {unit_count}\nBuildings: {building_count}\n\n--- Keys ---\n1-6: tool\nF: faction\nT: unit type\nB: building\nR-click: cycle terrain\nDel: clear map\nCtrl+S: save\nCtrl+L: load\nP: test mission\nG: Generate from archetype\nC/6: campaign editor\nEsc: exit{del_hint}"
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
    let in_mission = matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. });
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
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) {
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
        &UnitTypeId,
        Option<&Health>,
        Option<&MoveTarget>,
        Option<&AttackTarget>,
        Option<&HoldPosition>,
        Option<&AbilityCooldowns>,
        Option<&Suppressed>,
    ), With<UnitTypeId>>,
    mut text_q: Query<&mut Text, With<UnitInfoText>>,
) {
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) {
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
            let type_name = unit_type.id();

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

            **text = format!(
                "{}{}\n{}\nOrder: {}  {}",
                type_name, suppressed_str, health_str, order, q_cd_str,
            );
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
    buildings: Query<(&BuildingTypeId, &Faction, &ProductionQueue), With<Built>>,
    mut text_q: Query<&mut Text, With<ProductionQueueText>>,
    loaded: Option<Res<LoadedFactions>>,
) {
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) {
        return;
    }
    let Ok(mut text) = text_q.single_mut() else { return };
    let Some(pf) = player_faction else { return };
    let Some(loaded) = loaded else { return };

    let mut lines: Vec<String> = Vec::new();

    for (bt, faction, queue) in &buildings {
        if *faction != pf.0 {
            continue;
        }
        if queue.jobs.is_empty() {
            continue;
        }

        let building_name = bt.id();
        let producing = &queue.jobs[0];
        let duration = unit_production_seconds(producing, &loaded);
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
    units: Query<(&UnitPos, &Faction), With<UnitTypeId>>,
    buildings: Query<(&BuildingPos, &Faction), With<BuildingTypeId>>,
    player_faction: Option<Res<PlayerFaction>>,
) {
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) {
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

// ── Lobby map preview ─────────────────────────────────────────────────────────

const PREVIEW_W: f32 = 240.0;
const PREVIEW_H: f32 = 160.0;

/// Return the team color for a team name (Alpha=yellow, Bravo=red, Charlie=blue, etc.).
fn lobby_team_color(team: &str) -> Color {
    match team {
        "Alpha"   => Color::srgb(1.0, 0.95, 0.0),
        "Bravo"   => Color::srgb(1.0, 0.15, 0.15),
        "Charlie" => Color::srgb(0.15, 0.35, 1.0),
        "Delta"   => Color::srgb(0.10, 0.80, 0.10),
        "Echo"    => Color::srgb(0.0, 0.90, 0.90),
        "Foxtrot" => Color::srgb(0.85, 0.0, 0.85),
        "Golf"    => Color::srgb(1.0, 0.55, 0.0),
        "Hotel"   => Color::srgb(0.95, 0.95, 0.95),
        _         => Color::srgb(0.6, 0.6, 0.6),
    }
}

/// Brighter version of terrain_color for preview tiles.
fn terrain_color_preview(terrain: &cindertide::map::TerrainType) -> Color {
    use cindertide::map::TerrainType;
    match terrain {
        TerrainType::Grass     => Color::srgb(0.38, 0.62, 0.25),
        TerrainType::Road      => Color::srgb(0.52, 0.52, 0.52),
        TerrainType::Forest    => Color::srgb(0.12, 0.44, 0.12),
        TerrainType::Rubble    => Color::srgb(0.62, 0.56, 0.50),
        TerrainType::Mud       => Color::srgb(0.55, 0.38, 0.18),
        TerrainType::Corrupted => Color::srgb(0.75, 0.12, 0.75),
        _                      => Color::srgb(0.60, 0.60, 0.60),
    }
}

/// Rebuild the 2D map preview whenever the selected map changes in MultiplayerLobby.
fn update_lobby_map_preview(
    mut commands: Commands,
    screen: Res<ClientScreen>,
    lobby: Res<LobbyConfig>,
    mut preview_state: ResMut<LobbyPreviewState>,
    panel_q: Query<Entity, With<LobbyMapPreview>>,
    dot_q: Query<Entity, With<LobbyMapPreviewDot>>,
    mut panel_vis_q: Query<&mut Visibility, With<LobbyMapPreview>>,
) {
    let in_lobby = matches!(*screen, ClientScreen::MultiplayerLobby { .. });

    // Show/hide panel based on screen state
    for mut vis in &mut panel_vis_q {
        *vis = if in_lobby { Visibility::Visible } else { Visibility::Hidden };
    }

    if !in_lobby {
        return;
    }

    // Determine current map path string (empty if none selected)
    let current_map = lobby.map_path.as_deref().unwrap_or("").to_string();

    // Skip rebuild if map hasn't changed
    if current_map == preview_state.last_map_path {
        return;
    }
    preview_state.last_map_path = current_map.clone();

    // Despawn all existing preview dots
    for dot_entity in &dot_q {
        commands.entity(dot_entity).despawn();
    }

    let Ok(panel_entity) = panel_q.single() else { return };

    if current_map.is_empty() {
        return;
    }

    // Load the saved map file
    let full_path = format!("assets/{}", current_map);
    let saved: SavedMap = match std::fs::read_to_string(&full_path)
        .ok()
        .and_then(|content| toml::from_str(&content).ok())
    {
        Some(s) => s,
        None => return,
    };

    // Build tile list: use saved tiles if any, otherwise generate procedurally
    // Each entry: (x, y, terrain_string_or_type)
    struct PreviewTile {
        x: i32,
        y: i32,
        terrain: cindertide::map::TerrainType,
    }

    let preview_tiles: Vec<PreviewTile> = if !saved.tiles.is_empty() {
        saved.tiles.iter().map(|t| {
            use cindertide::map::TerrainType;
            let terrain = match t.terrain.as_str() {
                "Grass"     => TerrainType::Grass,
                "Road"      => TerrainType::Road,
                "Forest"    => TerrainType::Forest,
                "Rubble"    => TerrainType::Rubble,
                "Mud"       => TerrainType::Mud,
                "Corrupted" => TerrainType::Corrupted,
                _           => TerrainType::Grass,
            };
            PreviewTile { x: t.x, y: t.y, terrain }
        }).collect()
    } else {
        // Generate procedurally from mission_type
        let mission_type = match saved.mission_type.as_deref().unwrap_or("Assault") {
            "Defense"       => cindertide::mapgen::MissionType::Defense,
            "Extraction"    => cindertide::mapgen::MissionType::Extraction,
            "Survival"      => cindertide::mapgen::MissionType::Survival,
            "Control"       => cindertide::mapgen::MissionType::Control,
            "Ffa"           => cindertide::mapgen::MissionType::Ffa,
            "KingOfTheHill" => cindertide::mapgen::MissionType::KingOfTheHill,
            "Assassination" => cindertide::mapgen::MissionType::Assassination,
            _               => cindertide::mapgen::MissionType::Assault,
        };
        let gen = cindertide::mapgen::generate_for_mission(42, mission_type);
        gen.tiles.iter().map(|t| PreviewTile {
            x: t.pos.x,
            y: t.pos.y,
            terrain: t.terrain.clone(),
        }).collect()
    };

    if preview_tiles.is_empty() {
        return;
    }

    // Compute bounds
    let min_x = preview_tiles.iter().map(|t| t.x).min().unwrap_or(0);
    let min_y = preview_tiles.iter().map(|t| t.y).min().unwrap_or(0);
    let max_x = preview_tiles.iter().map(|t| t.x).max().unwrap_or(0);
    let max_y = preview_tiles.iter().map(|t| t.y).max().unwrap_or(0);

    let map_w = (max_x - min_x + 1) as f32;
    let map_h = (max_y - min_y + 1) as f32;
    if map_w <= 0.0 || map_h <= 0.0 {
        return;
    }

    let scale_x = PREVIEW_W / map_w;
    let scale_y = PREVIEW_H / map_h;

    // Subsample large maps: skip every other tile if map > 128×80
    let step = if map_w > 128.0 || map_h > 80.0 { 2usize } else { 1usize };

    // Build dots list: terrain tiles
    let mut dots: Vec<(f32, f32, f32, f32, Color)> = Vec::new(); // (left, top, w, h, color)

    for (i, tile) in preview_tiles.iter().enumerate() {
        if step > 1 && i % step != 0 {
            continue;
        }
        let px = (tile.x - min_x) as f32 * scale_x;
        // Flip Y: bottom of map = bottom of preview
        let py = (max_y - tile.y) as f32 * scale_y;
        let tw = scale_x.ceil().max(1.0);
        let th = scale_y.ceil().max(1.0);
        let color = terrain_color_preview(&tile.terrain);
        dots.push((px, py, tw, th, color));
    }

    // Spawn zone dots (6×6 px)
    for zone in &saved.spawn_zones {
        // Find team for this zone from lobby slots
        let team = lobby.slots.iter()
            .find(|s| s.spawn_zone == zone.id)
            .map(|s| s.team.as_str())
            .unwrap_or("Alpha");
        let color = lobby_team_color(team);
        let px = (zone.x - min_x) as f32 * scale_x - 3.0;
        let py = (max_y - zone.y) as f32 * scale_y - 3.0;
        dots.push((px.max(0.0), py.max(0.0), 6.0, 6.0, color));
    }

    // Spawn all dots as children of preview panel
    commands.entity(panel_entity).with_children(|parent| {
        for (left, top, w, h, color) in dots {
            parent.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(left + 2.0), // +2 for border inset
                    top: Val::Px(top + 2.0),
                    width: Val::Px(w),
                    height: Val::Px(h),
                    ..default()
                },
                BackgroundColor(color),
                LobbyMapPreviewDot,
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
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) {
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
    units: Query<&Faction, With<UnitTypeId>>,
    buildings: Query<(&Faction, &BuildingTypeId), With<BuildingPos>>,
    player_faction: Option<Res<PlayerFaction>>,
    script_state: Option<Res<ScriptState>>,
    commanders: Query<(&cindertide::mission::Commander, &cindertide::combat::Health), Without<cindertide::combat::Dead>>,
    all_factions_units: Query<&Faction, (With<UnitTypeId>, Without<cindertide::combat::Dead>)>,
    all_factions_buildings: Query<&Faction, (With<BuildingPos>, With<cindertide::buildings::Built>, Without<cindertide::combat::Dead>)>,
    mut text_q: Query<&mut Text, With<ObjectivesText>>,
) {
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) {
        return;
    }
    let Ok(mut text) = text_q.single_mut() else { return };

    // If the script has set an objective override, use that.
    if let Some(ref ss) = script_state {
        if let Some(ref override_text) = ss.current_objective {
            **text = override_text.clone();
            return;
        }
    }

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
        MissionType::Ffa => {
            // Count distinct factions still alive
            let mut alive_factions = std::collections::HashSet::new();
            for f in &all_factions_units {
                alive_factions.insert(f.clone());
            }
            for f in &all_factions_buildings {
                alive_factions.insert(f.clone());
            }
            format!("LAST FACTION STANDING\n{} factions remain", alive_factions.len())
        }
        MissionType::KingOfTheHill => {
            let threshold = mission.hill_threshold;
            let current = mission.hill_timer;
            let bar_len = 20usize;
            let filled = ((current / threshold) * bar_len as f32).round() as usize;
            let filled = filled.min(bar_len);
            let bar: String = "#".repeat(filled) + &"-".repeat(bar_len - filled);
            format!(
                "HOLD THE HILL\n[{}] {:.0}/{:.0}s",
                bar, current, threshold
            )
        }
        MissionType::Assassination => {
            // Check if the player's commander is under attack (health < 50%)
            let player_cmd_low = if let Some(ref pf) = player_f {
                commanders
                    .iter()
                    .filter(|(cmd, _)| &cmd.faction == pf)
                    .any(|(_, health)| health.current / health.max < 0.5)
            } else {
                false
            };
            if player_cmd_low {
                "ELIMINATE THE ENEMY COMMANDER\n[YOUR COMMANDER IS UNDER ATTACK]".to_string()
            } else {
                "ELIMINATE THE ENEMY COMMANDER".to_string()
            }
        }
    };

    **text = obj_text;
}

/// Show the front dialogue message from `ScriptState.dialogue_queue` for 4 seconds,
/// then pop and show the next. The bar is hidden when the queue is empty.
fn update_dialogue_bar(
    screen: Res<ClientScreen>,
    script_state: Option<Res<ScriptState>>,
    mut bar_q: Query<&mut Visibility, With<DialogueBar>>,
    mut text_q: Query<&mut Text, With<DialogueBarText>>,
) {
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) {
        // Hide the bar outside of missions.
        if let Ok(mut vis) = bar_q.single_mut() {
            *vis = Visibility::Hidden;
        }
        return;
    }

    let Ok(mut vis) = bar_q.single_mut() else { return };
    let Ok(mut text) = text_q.single_mut() else { return };

    match script_state {
        Some(ref ss) if !ss.dialogue_queue.is_empty() => {
            *vis = Visibility::Visible;
            if let Some((speaker, msg)) = ss.dialogue_queue.front() {
                **text = format!("[{}]  {}", speaker, msg);
            }
        }
        _ => {
            *vis = Visibility::Hidden;
            **text = String::new();
        }
    }
}

// ── Fog of War system ─────────────────────────────────────────────────────────

/// Vision radius in tiles for buildings (no UnitType to look up stats from).
const VISION_BUILDING: i32 = 8;

/// Update fog of war every 0.25 s while InMission.
fn update_fog_of_war(
    screen: Res<ClientScreen>,
    time: Res<Time>,
    player_faction: Option<Res<PlayerFaction>>,
    player_cheats: Res<PlayerCheats>,
    units: Query<(&UnitPos, &Faction, &UnitTypeId), With<UnitTypeId>>,
    buildings: Query<(&BuildingPos, &Faction), With<BuildingTypeId>>,
    vision_buildings: Query<(&BuildingPos, &Faction, &VisionProvider)>,
    mut fog: ResMut<FogOfWar>,
    tiles: Query<&Tile>,
    visual_entities: Res<VisualEntities>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    unit_visuals: Query<(&Faction, &UnitPos), With<UnitTypeId>>,
    building_visuals: Query<(&Faction, &BuildingPos), With<BuildingTypeId>>,
    mut vis_query: Query<(&mut Visibility, Entity)>,
) {
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) {
        return;
    }

    fog.timer -= time.delta_secs();
    if fog.timer > 0.0 {
        return;
    }
    fog.timer = 0.25;

    let Some(pf) = player_faction else { return };
    let player_f = &pf.0;

    // If player has disabled fog of war, reveal all tiles.
    if !player_cheats.fog_of_war {
        let all_tiles: HashSet<(i32, i32)> = tiles.iter().map(|t| (t.pos.x, t.pos.y)).collect();
        fog.visible = all_tiles.clone();
        fog.explored = all_tiles;
        // Skip normal computation
        return;
    }

    // Recompute visible set from all player units + buildings.
    let mut new_visible: HashSet<(i32, i32)> = HashSet::new();

    for (pos, faction, unit_type) in &units {
        if faction != player_f {
            continue;
        }
        let radius = match unit_type.id() {
            "heavy_armor" => 4,
            "light_vehicle" => 8,
            _ => 6,
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

    // Watchtowers and other vision buildings (VisionProvider overrides the default radius)
    for (bpos, bfaction, vision) in &vision_buildings {
        if bfaction != player_f { continue; }
        let r = vision.radius as i32;
        let cx = bpos.pos.x;
        let cy = bpos.pos.y;
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    new_visible.insert((cx + dx, cy + dy));
                }
            }
        }
    }

    // Save previous state for change detection.
    let old_visible = std::mem::replace(&mut fog.visible, new_visible);
    let old_explored = fog.explored.clone();

    // Explored is a superset — never shrinks.
    let newly_visible: Vec<(i32, i32)> = fog.visible.iter().copied().collect();
    for pos in newly_visible {
        fog.explored.insert(pos);
    }

    fog.prev_visible = old_visible;
    fog.prev_explored = old_explored;

    // Update tile material colors ONLY for tiles whose fog state changed.
    for tile in &tiles {
        let key = (tile.pos.x, tile.pos.y);

        let was_visible  = fog.prev_visible.contains(&key);
        let now_visible  = fog.visible.contains(&key);
        let was_explored = fog.prev_explored.contains(&key);
        let now_explored = fog.explored.contains(&key);

        // Skip if fog state didn't change.
        if was_visible == now_visible && was_explored == now_explored {
            continue;
        }

        let Some(mat_handle) = visual_entities.tile_materials.get(&key) else { continue };
        let Some(mat) = materials.get_mut(mat_handle) else { continue };

        if now_visible {
            // Fully visible — normal terrain color.
            mat.base_color = terrain_color(&tile.terrain_type);
        } else if now_explored {
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
    match faction.id() {
        "combine"  => Color::srgb(0.98, 0.82, 0.05), // electric gold
        "ironborn" => Color::srgb(0.95, 0.38, 0.05), // forge orange-red
        "covenant" => Color::srgb(0.05, 0.35, 0.95), // deep electric blue
        "hollow"   => Color::srgb(0.75, 0.05, 0.90), // vivid neon purple
        _          => Color::srgb(0.50, 0.50, 0.50),
    }
}

// ── Unit ability system ───────────────────────────────────────────────────────

/// Q/W/E ability cooldown durations in seconds per UnitTypeId.
fn ability_q_cooldown(unit_type: &UnitTypeId) -> f32 {
    match unit_type.id() {
        "riflemen"      => 15.0,
        "heavy_weapons" => 20.0,
        "light_vehicle" => 10.0,
        "heavy_armor"   => 8.0,
        _               => 15.0,
    }
}

/// Fire Q ability for the given unit.
fn fire_ability_q(
    commands: &mut Commands,
    entity: Entity,
    unit_type: &UnitTypeId,
    unit_pos: &cindertide::units::UnitPos,
    enemies: &[(Entity, cindertide::units::UnitPos)],
) {
    match unit_type.id() {
        "riflemen" => {
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
        "heavy_weapons" => {
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
        "light_vehicle" => {
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
        "heavy_armor" => {
            // Rally: remove Suppressed/Routing from self
            if let Ok(mut e) = commands.get_entity(entity) {
                e.remove::<Suppressed>()
                 .remove::<cindertide::combat::Routing>();
            }
            info!("HeavyArmor: Rally");
        }
        _ => {}
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
        &UnitTypeId,
        &cindertide::units::UnitPos,
        Option<&mut AbilityCooldowns>,
    )>,
    enemies_q: Query<(Entity, &cindertide::units::UnitPos, &Faction), With<UnitTypeId>>,
    player_faction: Option<Res<PlayerFaction>>,
) {
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) || tech_vis.visible {
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

// ── Death effects ─────────────────────────────────────────────────────────────

/// Detects units that just died (have `JustDied` marker), spawns a flash sphere,
/// and despawns the unit entity along with its visual.
fn handle_unit_death(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut visual_entities: ResMut<VisualEntities>,
    dying_units: Query<(Entity, &UnitPos), (With<JustDied>, With<Dead>, With<UnitTypeId>)>,
    dying_buildings: Query<(Entity, &BuildingPos), (With<JustDied>, With<Dead>, With<BuildingTypeId>)>,
    mut audio_queue: ResMut<AudioEventQueue>,
) {
    for (entity, pos) in &dying_units {
        let world_pos = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.75;

        // Spawn a brief white/yellow semi-transparent death flash sphere.
        commands.spawn((
            Mesh3d(meshes.add(Sphere::new(0.6))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgba(1.0, 0.95, 0.3, 0.75),
                emissive: LinearRgba::new(2.0, 1.8, 0.2, 1.0),
                alpha_mode: AlphaMode::Blend,
                ..default()
            })),
            Transform::from_translation(world_pos),
            DeathFlash { timer: 0.3 },
        ));

        // Despawn visual entity.
        if let Some(&vis) = visual_entities.units.get(&entity) {
            commands.entity(vis).despawn();
        }
        visual_entities.units.remove(&entity);

        // Despawn logic entity.
        commands.entity(entity).despawn();

        audio_queue.0.push(AudioEvent::Combat);
    }

    for (entity, pos) in &dying_buildings {
        let world_pos = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.5;

        // Replace building visual with a small rubble cube (dark gray/brown).
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.5, 0.3, 0.5))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.28, 0.22, 0.18),
                perceptual_roughness: 1.0,
                ..default()
            })),
            Transform::from_translation(world_pos - Vec3::Y * 0.35),
        ));

        // Despawn visual entity.
        if let Some(&vis) = visual_entities.buildings.get(&entity) {
            commands.entity(vis).despawn();
        }
        visual_entities.buildings.remove(&entity);

        // Despawn logic entity.
        commands.entity(entity).despawn();

        audio_queue.0.push(AudioEvent::Combat);
    }
}

/// Ticks DeathFlash timers, shrinks flashes toward zero, then despawns them.
fn tick_death_flashes(
    mut commands: Commands,
    time: Res<Time>,
    mut flashes: Query<(Entity, &mut DeathFlash, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut flash, mut transform) in &mut flashes {
        flash.timer -= dt;
        if flash.timer <= 0.0 {
            commands.entity(entity).despawn();
        } else {
            // Scale from 1.0 down to 0.0 as timer expires (timer starts at 0.3).
            let frac = (flash.timer / 0.3).clamp(0.0, 1.0);
            transform.scale = Vec3::splat(frac);
        }
    }
}

// ── Audio event processing ────────────────────────────────────────────────────

/// Per-variant kenney_aio path (relative to the asset source root).
const AUDIO_PATHS: &[(AudioEvent, &str)] = &[
    (AudioEvent::UnitSelected, "kenney_aio/Audio/Interface Sounds/Audio/pluck_001.ogg"),
    (AudioEvent::UnitMoved, "kenney_aio/Audio/Voiceover Pack/Audio (Male)/go.ogg"),
    (AudioEvent::Combat, "kenney_aio/Audio/Impact Sounds/Audio/impactPlate_heavy_000.ogg"),
    (AudioEvent::BuildingComplete, "kenney_aio/Audio/Music Jingles/Audio (Steeldrum)/jingles-steel_03.ogg"),
    (AudioEvent::UiClick, "kenney_aio/Audio/UI Audio/Audio/mouseclick1.ogg"),
    (AudioEvent::MissionStart, "kenney_aio/Audio/Synth Voice 1/Audio/begin.ogg"),
    (AudioEvent::MissionEnd, "kenney_aio/Audio/Synth Voice 1/Audio/objective complete.ogg"),
];

/// Startup system: pre-load one `Handle<AudioSource>` per AudioEvent. Skips entries
/// whose underlying OGG isn't on disk (e.g., prefetch failed for that file).
fn load_audio_assets(
    asset_server: Res<AssetServer>,
    model_assets: Res<ModelAssets>,
    mut audio: ResMut<AudioAssets>,
) {
    for (event, rel) in AUDIO_PATHS {
        if !model_assets.local_file(rel).exists() {
            warn!("Audio asset missing on disk: {}", rel);
            continue;
        }
        let handle: Handle<AudioSource> = asset_server.load(model_assets.asset_path(rel));
        match event {
            AudioEvent::UnitSelected => audio.unit_selected = Some(handle),
            AudioEvent::UnitMoved => audio.unit_moved = Some(handle),
            AudioEvent::Combat => audio.combat = Some(handle),
            AudioEvent::BuildingComplete => audio.building_complete = Some(handle),
            AudioEvent::UiClick => audio.ui_click = Some(handle),
            AudioEvent::MissionStart => audio.mission_start = Some(handle),
            AudioEvent::MissionEnd => audio.mission_end = Some(handle),
        }
    }
    info!("Loaded {} audio handles", AUDIO_PATHS.len());
}

fn process_audio_events(
    mut commands: Commands,
    mut queue: ResMut<AudioEventQueue>,
    audio: Res<AudioAssets>,
) {
    for event in queue.0.drain(..) {
        trace!("audio event: {:?}", event);
        if let Some(handle) = audio.for_event(event) {
            commands.spawn((
                AudioPlayer::<AudioSource>(handle.clone()),
                PlaybackSettings::DESPAWN,
            ));
        }
    }
}

// ── Multiplayer systems ───────────────────────────────────────────────────────

/// Host: every NET_STATE_INTERVAL seconds, serialize world state and broadcast to clients.
fn host_broadcast_game_state(
    mp_role: Res<MultiplayerRole>,
    net_channels: Option<Res<NetChannels>>,
    time: Res<Time>,
    mut timer: ResMut<NetBroadcastTimer>,
    screen: Res<ClientScreen>,
    units: Query<(Entity, &UnitPos, &Faction, &UnitTypeId, Option<&Health>, Option<&NetId>)>,
    buildings: Query<(Entity, &BuildingPos, &Faction, &BuildingTypeId, Option<&Health>, Option<&Built>, Option<&NetId>)>,
    missions: Query<&Mission>,
    active: Res<ActiveRun>,
    mut commands: Commands,
    mut net_id_counter: ResMut<NetIdCounter>,
) {
    if *mp_role != MultiplayerRole::Host {
        return;
    }
    let Some(channels) = net_channels else { return };
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) {
        return;
    }

    timer.0 += time.delta_secs();
    if timer.0 < NET_STATE_INTERVAL {
        return;
    }
    timer.0 = 0.0;

    // Assign NetIds to entities that don't have one yet
    let mut newly_assigned: Vec<(Entity, u64)> = Vec::new();
    for (entity, _, _, _, _, net_id) in &units {
        if net_id.is_none() {
            let id = next_net_id(&mut net_id_counter);
            newly_assigned.push((entity, id));
        }
    }
    for (entity, _, _, _, _, _, net_id) in &buildings {
        if net_id.is_none() {
            let id = next_net_id(&mut net_id_counter);
            newly_assigned.push((entity, id));
        }
    }
    for (entity, id) in newly_assigned {
        commands.entity(entity).insert(NetId(id));
    }

    // Build state snapshot
    let net_units: Vec<NetUnit> = units.iter().filter_map(|(_, pos, faction, utype, health, net_id)| {
        let id = net_id.map(|n| n.0).unwrap_or(0);
        Some(NetUnit {
            id,
            x: pos.pos.x,
            y: pos.pos.y,
            faction: faction.id().to_string(),
            unit_type: utype.id().to_string(),
            hp: health.map(|h| h.current).unwrap_or(100.0),
            hp_max: health.map(|h| h.max).unwrap_or(100.0),
        })
    }).collect();

    let net_buildings: Vec<NetBuilding> = buildings.iter().filter_map(|(_, pos, faction, btype, health, built, net_id)| {
        let id = net_id.map(|n| n.0).unwrap_or(0);
        Some(NetBuilding {
            id,
            x: pos.pos.x,
            y: pos.pos.y,
            faction: faction.id().to_string(),
            building_type: btype.id().to_string(),
            hp: health.map(|h| h.current).unwrap_or(100.0),
            hp_max: health.map(|h| h.max).unwrap_or(100.0),
            built: built.is_some(),
        })
    }).collect();

    let (mission_status, elapsed, deadline) = if let Some(entity) = active.current_mission_entity {
        if let Ok(m) = missions.get(entity) {
            let status = match m.status {
                MissionStatus::Active => "Active",
                MissionStatus::Won => "Won",
                MissionStatus::Lost => "Lost",
            };
            (status.to_string(), m.elapsed, m.deadline)
        } else {
            ("Active".to_string(), 0.0, 300.0)
        }
    } else {
        ("Active".to_string(), 0.0, 300.0)
    };

    let state = NetGameState {
        units: net_units,
        buildings: net_buildings,
        mission_status,
        elapsed,
        deadline,
    };

    let _ = channels.outbox.lock().unwrap().send(NetMessage::State(state));
}

/// Receive messages from network thread and handle them.
/// - Host: receives ClientCommands and applies them to the ECS.
/// - Client: receives NetGameState and stores it.
fn receive_net_messages(
    mp_role: Res<MultiplayerRole>,
    net_channels: Option<Res<NetChannels>>,
    mut remote_state: ResMut<RemoteGameState>,
    mut lobby: ResMut<LobbyConfig>,
    mut commands: Commands,
    units: Query<(Entity, &NetId, &UnitPos), With<UnitTypeId>>,
    tiles: Query<&Tile>,
) {
    let Some(channels) = net_channels else { return };

    // Drain all pending messages (collect to avoid holding lock across commands)
    let messages: Vec<NetMessage> = {
        let inbox = channels.inbox.lock().unwrap();
        let mut msgs = Vec::new();
        loop {
            match inbox.try_recv() {
                Ok(m) => msgs.push(m),
                Err(_) => break,
            }
        }
        msgs
    };

    for msg in messages {
        match msg {
            NetMessage::State(state) => {
                // Client stores remote state for rendering
                if matches!(*mp_role, MultiplayerRole::Client { .. }) {
                    remote_state.0 = Some(state);
                }
            }
            NetMessage::Command(cmd) => {
                // Host applies commands from client
                if *mp_role == MultiplayerRole::Host {
                    apply_client_command(&mut commands, cmd, &units, &tiles);
                }
            }
            NetMessage::LobbyState(received_lobby) => {
                // Client updates local lobby display from host
                if matches!(*mp_role, MultiplayerRole::Client { .. }) {
                    // Preserve local selected_slot/field, update the rest
                    let sel_slot = lobby.selected_slot;
                    let sel_field = lobby.selected_field.clone();
                    *lobby = received_lobby;
                    lobby.selected_slot = sel_slot;
                    lobby.selected_field = sel_field;
                }
            }
            NetMessage::LobbyReady => {
                // Client starts mission from current lobby state
                if matches!(*mp_role, MultiplayerRole::Client { .. }) {
                    let lobby_clone = lobby.clone();
                    commands.queue(move |world: &mut World| {
                        start_mission_from_lobby(world, &lobby_clone);
                    });
                }
            }
        }
    }
}

/// Apply a ClientCommand to the ECS (host only).
fn apply_client_command(
    commands: &mut Commands,
    cmd: ClientCommand,
    units: &Query<(Entity, &NetId, &UnitPos), With<UnitTypeId>>,
    tiles: &Query<&Tile>,
) {
    match cmd {
        ClientCommand::MoveOrder { unit_ids, target_x, target_y } => {
            let target_pos = GridPos { x: target_x, y: target_y };
            let tile_map: HashMap<(i32, i32), cindertide::map::TerrainType> = tiles
                .iter()
                .map(|t| ((t.pos.x, t.pos.y), t.terrain_type.clone()))
                .collect();
            let max_x = tile_map.keys().map(|(x, _)| *x).max().unwrap_or(40);
            let max_y = tile_map.keys().map(|(_, y)| *y).max().unwrap_or(25);

            for (entity, net_id, pos) in units.iter() {
                if unit_ids.contains(&net_id.0) {
                    let grid = cindertide::map::pathfinding::PathfindingGrid {
                        width: max_x + 1,
                        height: max_y + 1,
                        tiles: tile_map.clone(),
                        unit_type: cindertide::map::pathfinding::UnitKind::Infantry,
                        occupied: std::collections::HashSet::new(),
                        destination: None,
                    };
                    if let Some(path) = grid.find_path(pos.pos.clone(), target_pos.clone()) {
                        commands.entity(entity)
                            .remove::<HoldPosition>()
                            .insert(MoveTarget { target: target_pos.clone() })
                            .insert(MoveProgress { path, current_step: 0, elapsed: 0.0 });
                    }
                }
            }
        }
        ClientCommand::AttackOrder { unit_ids, target_id } => {
            // Find target entity by NetId
            let target_entity = units.iter()
                .find(|(_, net_id, _)| net_id.0 == target_id)
                .map(|(e, _, _)| e);
            if let Some(target) = target_entity {
                for (entity, net_id, _) in units.iter() {
                    if unit_ids.contains(&net_id.0) {
                        commands.entity(entity)
                            .remove::<MoveTarget>()
                            .remove::<MoveProgress>()
                            .remove::<AttackMoveOrder>()
                            .remove::<HoldPosition>()
                            .insert(PlayerAttackOrder { target });
                    }
                }
            }
        }
        ClientCommand::BuildOrder { building_type, x, y } => {
            // Spawn a building with the given type string (any non-empty type is accepted)
            if !building_type.is_empty() {
                use cindertide::buildings::{BuildingBundle, BuildingTypeId};
                let bt = BuildingTypeId::new(&building_type);
                commands.spawn(BuildingBundle::new_default(bt, Faction::new("combine"), x, y));
            }
        }
    }
}

/// Apply PlayerCheats resource multiplier to the human player's resource pool each tick.
fn apply_player_cheat_trickle(
    time: Res<Time>,
    screen: Res<ClientScreen>,
    player_cheats: Res<PlayerCheats>,
    player_faction: Option<Res<PlayerFaction>>,
    mut pools: Query<(&FactionEntity, &mut ResourcePool)>,
) {
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) {
        return;
    }
    let rm = player_cheats.resource_multiplier;
    if (rm - 1.0).abs() < 0.01 {
        return; // no-op for 1.0x
    }
    let Some(pf) = player_faction else { return };
    let player_f = &pf.0;
    let dt = time.delta_secs();

    for (fe, mut pool) in &mut pools {
        if &fe.faction != player_f {
            continue;
        }
        // Apply multiplier as a fractional bonus/penalty on current pool
        let bonus = pool.fuel * (rm - 1.0) * dt;
        pool.fuel = (pool.fuel + bonus).min(2000.0).max(0.0);
        let sbonus = pool.scrap * (rm - 1.0) * dt;
        pool.scrap = (pool.scrap + sbonus).min(2000.0).max(0.0);
    }
}
