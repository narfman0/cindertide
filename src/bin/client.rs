use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::render::camera::ScalingMode;
use cindertide::map::{Faction, GridPos, Tile};
use cindertide::units::{UnitPos, UnitType, unit_stats};
use cindertide::buildings::{BuildingPos, BuildingType};
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
        .add_plugins(MissionScriptPlugin)
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
        .init_resource::<MultiplayerRole>()
        .init_resource::<NetIdCounter>()
        .init_resource::<RemoteGameState>()
        .init_resource::<ModelAssets>()
        .init_resource::<EditorEnteredFromGame>()
        .init_resource::<LoadedCampaigns>()
        .init_resource::<CampaignEditorState>()
        .init_resource::<GeneratePanel>()
        .insert_resource(MinimapTimer(0.0))
        .insert_resource(NetBroadcastTimer(0.0))
        .add_systems(Startup, setup_scene)
        .add_systems(Startup, setup_ui)
        .add_systems(Startup, load_narrative)
        .add_systems(Startup, startup_load_progress)
        .add_systems(Startup, load_model_assets)
        .add_systems(Startup, startup_load_campaigns)
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
        .run();
}

// ── ClientScreen resource ────────────────────────────────────────────────────

#[derive(Resource, Debug, Clone, PartialEq)]
enum ClientScreen {
    Title,
    MultiplayerMenu { hosting: bool, ip_input: String },
    /// Campaign picker — replaces old hardcoded FactionPicker.
    FactionPicker { selected: usize },
    Briefing { title: String, briefing: String },
    InMission,
    /// Running a mission that was started directly from the map editor (P key).
    /// When it ends, return to MapEditor with the saved map path reloaded.
    TestMission { saved_map_path: String },
    Debrief { title: String, text: String, won: bool },
    GameOver { won: bool, handler_unlocked: bool },
    MapEditor,
    CampaignEditor,
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

#[derive(Serialize, Deserialize, Debug, Clone)]
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
// Load .ogg files into `assets/audio/` and add `asset_server.load(...)` calls
// here once real audio files are available. Wire each AudioEvent variant to a
// corresponding Handle<AudioSource> stored in a resource, then play it from
// `process_audio_events`.

#[derive(Debug, Clone)]
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
}

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

/// Holds the resolved directory path for GLB model files.
/// `path` is `Some` only when `CINDERTIDE_MODEL_PATH` is set and the directory exists.
/// When `None`, all visuals fall back to colored placeholder cuboids.
#[derive(Resource, Default)]
struct ModelAssets {
    path: Option<PathBuf>,
}

/// Map a `UnitType` to its expected GLB scene filename (relative to the model directory).
fn unit_model_name(unit_type: &UnitType) -> &'static str {
    match unit_type {
        UnitType::Riflemen     => "unit_riflemen.glb#Scene0",
        UnitType::HeavyWeapons => "unit_heavy_weapons.glb#Scene0",
        UnitType::LightVehicle => "unit_light_vehicle.glb#Scene0",
        UnitType::HeavyArmor   => "unit_heavy_armor.glb#Scene0",
    }
}

/// Map a `BuildingType` to its expected GLB scene filename.
fn building_model_name(building_type: &BuildingType) -> &'static str {
    match building_type {
        BuildingType::CommandBunker       => "building_command_bunker.glb#Scene0",
        BuildingType::Barracks            => "building_barracks.glb#Scene0",
        BuildingType::Refinery            => "building_refinery.glb#Scene0",
        BuildingType::Scrapyard           => "building_scrapyard.glb#Scene0",
        BuildingType::RecruitmentOffice   => "building_recruitment_office.glb#Scene0",
        BuildingType::MotorPool           => "building_motor_pool.glb#Scene0",
        BuildingType::Foundry             => "building_foundry.glb#Scene0",
        BuildingType::Airfield            => "building_airfield.glb#Scene0",
        BuildingType::Workshop            => "building_workshop.glb#Scene0",
        BuildingType::ResearchLab         => "building_research_lab.glb#Scene0",
        BuildingType::SupplyDepot         => "building_supply_depot.glb#Scene0",
        BuildingType::Watchtower          => "building_watchtower.glb#Scene0",
        BuildingType::RepairBay           => "building_repair_bay.glb#Scene0",
        BuildingType::Pillbox             => "building_pillbox.glb#Scene0",
        BuildingType::AAGun               => "building_aa_gun.glb#Scene0",
        BuildingType::TankTrap            => "building_tank_trap.glb#Scene0",
    }
}

/// Startup system: resolve `CINDERTIDE_MODEL_PATH` and populate `ModelAssets`.
fn load_model_assets(mut model_assets: ResMut<ModelAssets>) {
    match std::env::var("CINDERTIDE_MODEL_PATH") {
        Ok(val) => {
            let path = PathBuf::from(&val);
            if path.is_dir() {
                info!("3D models loaded from: {}", val);
                model_assets.path = Some(path);
            } else {
                info!("CINDERTIDE_MODEL_PATH set but directory not found — using placeholder geometry");
            }
        }
        Err(_) => {
            info!("CINDERTIDE_MODEL_PATH not set — using placeholder geometry");
        }
    }
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
    });
}

// ── Screen overlay update ─────────────────────────────────────────────────────

fn update_screen_overlay(
    screen: Res<ClientScreen>,
    progress: Res<GlobalProgress>,
    active: Res<ActiveRun>,
    loaded_campaigns: Res<LoadedCampaigns>,
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
            **hint = format!("Enter — Start [{}]  |  E — Map Editor  |  M — Multiplayer", first_campaign_name);
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
    loaded_campaigns: Res<LoadedCampaigns>,
    mut mp_role: ResMut<MultiplayerRole>,
    mut commands: Commands,
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
                *screen = ClientScreen::MultiplayerMenu {
                    hosting: false,
                    ip_input: "127.0.0.1".to_string(),
                };
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
                                    let faction = parse_faction_name(&su.faction).unwrap_or(Faction::Combine);
                                    spawn_unit_world(world, su.x, su.y, faction, &su.unit_type);
                                }
                                for sb in &saved.buildings {
                                    let faction = parse_faction_name(&sb.faction).unwrap_or(Faction::Combine);
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

                    // Transition to briefing for mission 0
                    let (title, briefing) = get_mission_narrative(&narrative, faction, 0);
                    *screen = ClientScreen::Briefing { title, briefing };
                }
            }
        }

        ClientScreen::Briefing { .. } => {
            if enter {
                commands.queue(|world: &mut World| {
                    cindertide::wipe_world_entities(world);

                    let (player, mission_index, map_path) = {
                        let active = world.resource::<ActiveRun>();
                        let p = active.run.as_ref()
                            .map(|r| map_faction(r.faction))
                            .unwrap_or(Faction::Combine);
                        let idx = active.run.as_ref().map(|r| r.current_mission).unwrap_or(0);
                        let mp = active.run.as_ref()
                            .and_then(|r| r.mission_maps.get(idx))
                            .cloned();
                        (p, idx, mp)
                    };

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
                                    let faction = parse_faction_name(&su.faction).unwrap_or(Faction::Combine);
                                    spawn_unit_world(world, su.x, su.y, faction, &su.unit_type);
                                }
                                for sb in &saved.buildings {
                                    let faction = parse_faction_name(&sb.faction).unwrap_or(Faction::Combine);
                                    spawn_building_world(world, sb.x, sb.y, faction, &sb.building_type);
                                }
                                map_loaded = true;
                                info!("Loaded mission map from {}", full_path);
                            }
                        }
                        if !map_loaded {
                            info!("Map file '{}' not found or invalid, falling back to procedural", full_path);
                        }
                    }

                    if !map_loaded {
                        cindertide::setup_demo_scenario(world, &player, mission_index);
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
        // MultiplayerMenu handled above; this arm exists so the compiler is happy
        // if we ever reach it again from a re-match (shouldn't happen).
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
    model_assets: Res<ModelAssets>,
    asset_server: Res<AssetServer>,
    units: Query<(Entity, &UnitPos, &Faction, &UnitType), Added<UnitType>>,
) {
    for (entity, pos, faction, unit_type) in &units {
        let world_pos = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.75;

        // Attempt to load a GLB model if CINDERTIDE_MODEL_PATH is set.
        // Scale factor 0.01 assumes Synty-style centimetre-unit exports — tune per asset pack.
        let visual = if let Some(ref dir) = model_assets.path {
            let glb_name = unit_model_name(unit_type);
            // Strip the "#Scene0" fragment to get the bare file name for existence check.
            let file_name = glb_name.split('#').next().unwrap_or(glb_name);
            let full_path = dir.join(file_name);
            if full_path.exists() {
                commands.spawn((
                    SceneRoot(asset_server.load(format!("{}/{}", dir.display(), glb_name))),
                    Transform::from_translation(world_pos).with_scale(Vec3::splat(0.01)),
                )).id()
            } else {
                // File missing — fall back to cuboid placeholder.
                let color = faction_color(faction);
                commands.spawn((
                    Mesh3d(meshes.add(Cuboid::new(0.6, 1.5, 0.6))),
                    MeshMaterial3d(materials.add(StandardMaterial { base_color: color, ..default() })),
                    Transform::from_translation(world_pos),
                )).id()
            }
        } else {
            // No model path configured — use colored cuboid.
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
    units: Query<(Entity, &UnitPos, Option<&AttackTarget>, Option<&MoveTarget>), With<UnitType>>,
    unit_positions: Query<&UnitPos, With<UnitType>>,
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
    buildings: Query<(Entity, &BuildingPos, &Faction, &BuildingType), Added<BuildingType>>,
) {
    for (entity, pos, faction, building_type) in &buildings {
        if visual_entities.buildings.contains_key(&entity) {
            continue;
        }
        let world_pos = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.5;

        // Attempt to load a GLB model if CINDERTIDE_MODEL_PATH is set.
        // Scale factor 0.015 assumes Synty-style centimetre-unit exports — tune per asset pack.
        let visual = if let Some(ref dir) = model_assets.path {
            let glb_name = building_model_name(building_type);
            let file_name = glb_name.split('#').next().unwrap_or(glb_name);
            let full_path = dir.join(file_name);
            if full_path.exists() {
                commands.spawn((
                    SceneRoot(asset_server.load(format!("{}/{}", dir.display(), glb_name))),
                    Transform::from_translation(world_pos).with_scale(Vec3::splat(0.015)),
                )).id()
            } else {
                // File missing — fall back to cuboid placeholder.
                let color = faction_color(faction).mix(&Color::WHITE, 0.25);
                commands.spawn((
                    Mesh3d(meshes.add(Cuboid::new(0.9, 1.0, 0.9))),
                    MeshMaterial3d(materials.add(StandardMaterial { base_color: color, ..default() })),
                    Transform::from_translation(world_pos),
                )).id()
            }
        } else {
            // No model path configured — use colored cuboid.
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
    units: Query<(Entity, &UnitPos, &Faction), With<UnitType>>,
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
    units: Query<(Entity, &UnitPos, &Faction), With<UnitType>>,
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
            ortho.scale = (ortho.scale - ev.y * cam.zoom_speed).clamp(4.0, 120.0);
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
    gen: Res<GeneratePanel>,
    tiles: Query<(Entity, &Tile)>,
    units: Query<Entity, With<UnitType>>,
    buildings: Query<Entity, With<BuildingType>>,
    units_full: Query<(&UnitPos, &Faction, &UnitType)>,
    buildings_full: Query<(&BuildingPos, &Faction, &BuildingType)>,
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
                        let faction = parse_faction_name(&su.faction).unwrap_or(Faction::Combine);
                        spawn_editor_unit(&mut commands, su.x, su.y, faction, &su.unit_type);
                    }

                    // Spawn loaded buildings
                    for sb in &map.buildings {
                        let faction = parse_faction_name(&sb.faction).unwrap_or(Faction::Combine);
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
                    let player = Faction::Combine;
                    let opponent = Faction::Ironborn;
                    let mission_index = 0usize;

                    world.spawn(FactionBundle::new(player.clone()));

                    let mission_entity = world.spawn(Mission {
                        mission_type: cindertide::mapgen::MissionType::Assault,
                        player_faction: player.clone(),
                        opponent_faction: opponent.clone(),
                        status: MissionStatus::Active,
                        elapsed: 0.0,
                        deadline: 300.0,
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
    units: Query<Entity, With<UnitType>>,
    buildings: Query<Entity, With<BuildingType>>,
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
                    out.push_str(&format!("  {{ type = \"dialogue\", text = {:?} }},\n", action.text));
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

fn map_faction_name(f: &Faction) -> &'static str {
    match f {
        Faction::Combine  => "Combine",
        Faction::Ironborn => "Ironborn",
        Faction::Covenant => "Covenant",
        Faction::Hollow   => "Hollow",
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

/// Spawn a unit directly into the World (used for map loading during mission start).
fn spawn_unit_world(world: &mut World, x: i32, y: i32, faction: Faction, type_name: &str) {
    use cindertide::units::*;
    use cindertide::combat::*;
    match type_name {
        "HeavyWeapons" => { world.spawn(HeavyWeaponsBundle::with_faction(x, y, faction)); }
        "LightVehicle"  => { world.spawn(LightVehicleBundle::with_faction(x, y, faction)); }
        "HeavyArmor"    => { world.spawn(HeavyArmorBundle::with_faction(x, y, faction)); }
        _               => { world.spawn(RiflemanBundle::with_faction(x, y, faction)); }
    }
}

/// Spawn a building directly into the World (used for map loading during mission start).
fn spawn_building_world(world: &mut World, x: i32, y: i32, faction: Faction, type_name: &str) {
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
    world.spawn(BuildingBundle::new(bt, faction, x, y));
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

            let stats = unit_stats(unit_type);
            **text = format!(
                "{}{}\n{}\nOrder: {}  {}\nRange: {:.0}  Speed: {:.1}",
                type_name, suppressed_str, health_str, order, q_cd_str,
                stats.attack_range, stats.move_speed
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
    buildings: Query<(&BuildingType, &Faction, &ProductionQueue), With<Built>>,
    mut text_q: Query<&mut Text, With<ProductionQueueText>>,
) {
    if !matches!(*screen, ClientScreen::InMission | ClientScreen::TestMission { .. }) {
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
    units: Query<&Faction, With<UnitType>>,
    buildings: Query<(&Faction, &BuildingType), With<BuildingPos>>,
    player_faction: Option<Res<PlayerFaction>>,
    script_state: Option<Res<ScriptState>>,
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
            if let Some(msg) = ss.dialogue_queue.front() {
                **text = msg.clone();
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
    units: Query<(&UnitPos, &Faction, &UnitType), With<UnitType>>,
    buildings: Query<(&BuildingPos, &Faction), With<BuildingType>>,
    mut fog: ResMut<FogOfWar>,
    tiles: Query<&Tile>,
    visual_entities: Res<VisualEntities>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    unit_visuals: Query<(&Faction, &UnitPos), With<UnitType>>,
    building_visuals: Query<(&Faction, &BuildingPos), With<BuildingType>>,
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

    // Recompute visible set from all player units + buildings.
    let mut new_visible: HashSet<(i32, i32)> = HashSet::new();

    for (pos, faction, unit_type) in &units {
        if faction != player_f {
            continue;
        }
        let radius = unit_stats(unit_type).vision_range as i32;
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
    dying_units: Query<(Entity, &UnitPos), (With<JustDied>, With<Dead>, With<UnitType>)>,
    dying_buildings: Query<(Entity, &BuildingPos), (With<JustDied>, With<Dead>, With<BuildingType>)>,
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
// Load .ogg files into `assets/audio/` and add `asset_server.load(...)` calls
// here once real audio files are available. For each AudioEvent variant, store
// a Handle<AudioSource> in a resource and call `commands.spawn(AudioPlayer(handle))`
// (or equivalent Bevy audio API) in the match below.

fn process_audio_events(mut queue: ResMut<AudioEventQueue>) {
    for event in queue.0.drain(..) {
        trace!("audio event: {:?}", event);
        // TODO: match event { AudioEvent::Combat => play combat_sfx, ... }
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
    units: Query<(Entity, &UnitPos, &Faction, &UnitType, Option<&Health>, Option<&NetId>)>,
    buildings: Query<(Entity, &BuildingPos, &Faction, &BuildingType, Option<&Health>, Option<&Built>, Option<&NetId>)>,
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
            faction: format!("{:?}", faction),
            unit_type: format!("{:?}", utype),
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
            faction: format!("{:?}", faction),
            building_type: format!("{:?}", btype),
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
    mut commands: Commands,
    units: Query<(Entity, &NetId, &UnitPos), With<UnitType>>,
    tiles: Query<&Tile>,
) {
    let Some(channels) = net_channels else { return };

    // Drain all pending messages
    loop {
        let msg = match channels.inbox.lock().unwrap().try_recv() {
            Ok(m) => m,
            Err(_) => break,
        };

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
        }
    }
}

/// Apply a ClientCommand to the ECS (host only).
fn apply_client_command(
    commands: &mut Commands,
    cmd: ClientCommand,
    units: &Query<(Entity, &NetId, &UnitPos), With<UnitType>>,
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
            // Parse building type and spawn placement command
            let bt = match building_type.as_str() {
                "Refinery" => Some(BuildingType::Refinery),
                "Barracks" => Some(BuildingType::Barracks),
                "CommandBunker" => Some(BuildingType::CommandBunker),
                "MotorPool" => Some(BuildingType::MotorPool),
                _ => None,
            };
            if let Some(bt) = bt {
                use cindertide::buildings::BuildingBundle;
                commands.spawn(BuildingBundle::new(bt, Faction::Ironborn, x, y));
            }
        }
    }
}
