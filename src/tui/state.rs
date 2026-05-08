//! TUI state — view enum + cached server snapshot + key handling.

use crossterm::event::KeyCode;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Title { selected: usize },
    FactionPicker { selected: usize },  // 0=Combine, 1=Ironborn
    LoadPicker { selected: usize, slots: Vec<String> },
    Campaign,
    InMission,
    GameOver { won: bool, handler_unlocked: bool },
}

#[derive(Debug, Default, Clone)]
pub struct ServerSnapshot {
    pub state_label: String,
    pub paused: bool,
    pub player: Option<String>,
    pub missions_won: u32,
    pub missions_lost: u32,
    pub current_mission_index: Option<usize>,
    pub handler_unlocked: bool,

    /// Campaign view data
    pub current_mission_type: Option<String>,
    pub past_outcomes: Vec<OutcomeSummary>,

    /// Mission view data
    pub mission: Option<MissionSummary>,
    pub resources: Option<ResourceSummary>,
    pub units: Vec<UnitSummary>,
    pub buildings: Vec<BuildingSummary>,
    pub tiles: Vec<TileSummary>,
}

#[derive(Debug, Clone)]
pub struct OutcomeSummary {
    pub mission_index: usize,
    pub won: bool,
    pub mission_type: String,
}

#[derive(Debug, Clone)]
pub struct MissionOption {
    pub mission_type: String,
    pub mission_index: usize,
}

#[derive(Debug, Clone)]
pub struct MissionSummary {
    pub mission_type: String,
    pub player: String,
    pub opponent: String,
    pub elapsed: f32,
    pub deadline: f32,
    pub status: String,
}

#[derive(Debug, Default, Clone)]
pub struct ResourceSummary {
    pub fuel: f32,
    pub scrap: f32,
    pub manpower: f32,
    pub fuel_trickle: f32,
    pub scrap_trickle: f32,
    pub manpower_trickle: f32,
    pub pop_current: u32,
    pub pop_max: u32,
}

#[derive(Debug, Clone)]
pub struct UnitSummary {
    pub entity_id: u64,
    pub unit_type: String,
    pub faction: String,
    pub x: i32,
    pub y: i32,
    pub health: f32,
    pub health_max: f32,
}

#[derive(Debug, Clone)]
pub struct BuildingSummary {
    pub entity_id: u64,
    pub building_type: String,
    pub faction: String,
    pub x: i32,
    pub y: i32,
    pub built: bool,
    pub health: f32,
    pub health_max: f32,
}

#[derive(Debug, Clone)]
pub struct TileSummary {
    pub x: i32,
    pub y: i32,
    pub terrain: String,
    pub cover: String,
}

pub struct TuiApp {
    pub screen: Screen,
    pub snapshot: ServerSnapshot,
    pub should_quit: bool,
    pub last_error: Option<String>,
}

impl TuiApp {
    pub fn new() -> Self {
        Self {
            screen: Screen::Title { selected: 0 },
            snapshot: ServerSnapshot::default(),
            should_quit: false,
            last_error: None,
        }
    }

    /// Single poll tick — fetches whatever data the current screen needs.
    pub fn poll(&mut self) {
        if let Some(v) = super::call("game/state", serde_json::json!({})) {
            if let Some(r) = v.get("result") {
                self.snapshot.state_label = r["state"].as_str().unwrap_or("?").to_string();
                self.snapshot.player = r["player"].as_str().map(String::from);
                self.snapshot.missions_won = r["missions_won"].as_u64().unwrap_or(0) as u32;
                self.snapshot.missions_lost = r["missions_lost"].as_u64().unwrap_or(0) as u32;
                self.snapshot.current_mission_index = r["current_mission_index"].as_u64().map(|n| n as usize);
                self.snapshot.handler_unlocked = self.snapshot.state_label.contains("handler_unlocked=true");
            }
        }

        match (&self.screen, self.snapshot.state_label.as_str()) {
            (Screen::FactionPicker { .. }, "Campaign") => {
                self.screen = Screen::Campaign;
            }
            (Screen::Campaign, "InMission") => {
                self.screen = Screen::InMission;
            }
            (Screen::InMission, "Campaign") => {
                self.screen = Screen::Campaign;
            }
            (_, s) if s.starts_with("GameOver") => {
                let won = s.contains("won");
                let handler_unlocked = s.contains("handler_unlocked=true");
                if !matches!(self.screen, Screen::GameOver { .. }) {
                    self.screen = Screen::GameOver { won, handler_unlocked };
                }
            }
            _ => {}
        }

        // Pull pause flag.
        if let Some(v) = super::call("game/pause_status", serde_json::json!({})) {
            if let Some(r) = v.get("result") {
                self.snapshot.paused = r["paused"].as_bool().unwrap_or(false);
            }
        }

        match &self.screen {
            Screen::Campaign => {
                self.refresh_campaign();
            }
            Screen::InMission => {
                self.refresh_mission();
            }
            _ => {}
        }
    }

    fn refresh_campaign(&mut self) {
        if let Some(v) = super::call("campaign/state", serde_json::json!({})) {
            if let Some(r) = v.get("result") {
                self.snapshot.past_outcomes.clear();
                if let Some(outcomes) = r["outcomes"].as_array() {
                    for o in outcomes {
                        self.snapshot.past_outcomes.push(OutcomeSummary {
                            mission_index: o["mission_index"].as_u64().unwrap_or(0) as usize,
                            won: o["won"].as_bool().unwrap_or(false),
                            mission_type: o["mission_type"].as_str().unwrap_or("").to_string(),
                        });
                    }
                }
            }
        }
        if let Some(v) = super::call("campaign/options", serde_json::json!({})) {
            if let Some(r) = v.get("result") {
                self.snapshot.current_mission_type = r["options"]
                    .as_array()
                    .and_then(|opts| opts.first())
                    .and_then(|o| o["mission_type"].as_str())
                    .map(String::from);
            }
        }
    }

    fn refresh_mission(&mut self) {
        if self.snapshot.current_mission_index.is_some() {
            if let Some(v) = super::call(
                "world/list",
                serde_json::json!({ "kind": "missions" }),
            ) {
                if let Some(items) = v["result"]["items"].as_array() {
                    if let Some(m) = items.first() {
                        self.snapshot.mission = Some(MissionSummary {
                            mission_type: m["mission_type"].as_str().unwrap_or("").to_string(),
                            player: m["player"].as_str().unwrap_or("").to_string(),
                            opponent: m["opponent"].as_str().unwrap_or("").to_string(),
                            elapsed: m["elapsed"].as_f64().unwrap_or(0.0) as f32,
                            deadline: m["deadline"].as_f64().unwrap_or(0.0) as f32,
                            status: m["status"].as_str().unwrap_or("").to_string(),
                        });
                    }
                }
            }
        }

        // Player faction resources
        if let Some(v) = super::call("world/list", serde_json::json!({ "kind": "factions" })) {
            if let Some(items) = v["result"]["items"].as_array() {
                let player = self.snapshot.player.clone().unwrap_or_default();
                if let Some(my) = items.iter().find(|f| {
                    f["faction"].as_str().map(|s| s == player).unwrap_or(false)
                }) {
                    self.snapshot.resources = Some(ResourceSummary {
                        fuel: my["fuel"].as_f64().unwrap_or(0.0) as f32,
                        scrap: my["scrap"].as_f64().unwrap_or(0.0) as f32,
                        manpower: my["manpower"].as_f64().unwrap_or(0.0) as f32,
                        fuel_trickle: my["fuel_trickle"].as_f64().unwrap_or(0.0) as f32,
                        scrap_trickle: my["scrap_trickle"].as_f64().unwrap_or(0.0) as f32,
                        manpower_trickle: my["manpower_trickle"].as_f64().unwrap_or(0.0) as f32,
                        pop_current: my["pop_current"].as_u64().unwrap_or(0) as u32,
                        pop_max: my["pop_max"].as_u64().unwrap_or(0) as u32,
                    });
                }
            }
        }

        // Units
        if let Some(v) = super::call("world/list", serde_json::json!({ "kind": "units" })) {
            self.snapshot.units = parse_units(&v);
        }
        // Buildings
        if let Some(v) = super::call("world/list", serde_json::json!({ "kind": "buildings" })) {
            self.snapshot.buildings = parse_buildings(&v);
        }
        // Tiles
        if let Some(v) = super::call("world/list", serde_json::json!({ "kind": "tiles" })) {
            self.snapshot.tiles = parse_tiles(&v);
        }
    }

    pub fn handle_key(&mut self, key: KeyCode) {
        match self.screen.clone() {
            Screen::Title { selected } => self.handle_title(selected, key),
            Screen::FactionPicker { selected } => self.handle_faction_picker(selected, key),
            Screen::LoadPicker { selected, slots } => self.handle_load_picker(selected, slots, key),
            Screen::Campaign => self.handle_campaign(key),
            Screen::InMission => self.handle_in_mission(key),
            Screen::GameOver { .. } => self.handle_game_over(key),
        }
    }

    fn handle_title(&mut self, selected: usize, key: KeyCode) {
        match key {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Up => {
                let new = if selected == 0 { 2 } else { selected - 1 };
                self.screen = Screen::Title { selected: new };
            }
            KeyCode::Down => {
                self.screen = Screen::Title { selected: (selected + 1) % 3 };
            }
            KeyCode::Enter => match selected {
                0 => self.screen = Screen::FactionPicker { selected: 0 },
                1 => {
                    // Load Game — fetch slot list
                    let slots = self.list_save_slots();
                    self.screen = Screen::LoadPicker { selected: 0, slots };
                }
                2 => self.should_quit = true,
                _ => {}
            },
            _ => {}
        }
    }

    fn handle_faction_picker(&mut self, selected: usize, key: KeyCode) {
        let factions = ["combine", "ironborn"];
        match key {
            KeyCode::Esc => self.screen = Screen::Title { selected: 0 },
            KeyCode::Up => {
                let new = if selected == 0 { factions.len() - 1 } else { selected - 1 };
                self.screen = Screen::FactionPicker { selected: new };
            }
            KeyCode::Down => {
                self.screen = Screen::FactionPicker {
                    selected: (selected + 1) % factions.len(),
                };
            }
            KeyCode::Enter => {
                let player = factions[selected];
                let _ = super::call(
                    "game/new",
                    serde_json::json!({ "player": player }),
                );
            }
            _ => {}
        }
    }

    fn handle_load_picker(&mut self, selected: usize, slots: Vec<String>, key: KeyCode) {
        match key {
            KeyCode::Esc => self.screen = Screen::Title { selected: 1 },
            KeyCode::Up if !slots.is_empty() => {
                let new = if selected == 0 { slots.len() - 1 } else { selected - 1 };
                self.screen = Screen::LoadPicker { selected: new, slots };
            }
            KeyCode::Down if !slots.is_empty() => {
                let new = (selected + 1) % slots.len();
                self.screen = Screen::LoadPicker { selected: new, slots };
            }
            KeyCode::Enter if !slots.is_empty() => {
                let slot = slots[selected].clone();
                let _ = super::call(
                    "game/load",
                    serde_json::json!({ "slot": slot }),
                );
            }
            _ => {}
        }
    }

    fn handle_campaign(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => self.screen = Screen::Title { selected: 0 },
            KeyCode::Char('s') => {
                let _ = super::call(
                    "game/save",
                    serde_json::json!({ "slot": "default" }),
                );
            }
            KeyCode::Enter => {
                if self.snapshot.current_mission_type.is_some() {
                    let _ = super::call(
                        "mission/select",
                        serde_json::json!({}),
                    );
                }
            }
            _ => {}
        }
    }

    fn handle_in_mission(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => {
                // Abandon mission — force resolve as loss.
                let _ = super::call("game/abandon_mission", serde_json::json!({}));
            }
            KeyCode::Char(' ') => {
                let _ = super::call(
                    "game/pause",
                    serde_json::json!({ "paused": !self.snapshot.paused }),
                );
            }
            _ => {}
        }
    }

    fn handle_game_over(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter => {
                self.screen = Screen::Title { selected: 0 };
            }
            _ => {}
        }
    }

    fn list_save_slots(&self) -> Vec<String> {
        if let Some(v) = super::call("save/list", serde_json::json!({})) {
            if let Some(arr) = v["result"]["slots"].as_array() {
                return arr.iter().filter_map(|s| s.as_str().map(String::from)).collect();
            }
        }
        Vec::new()
    }
}

fn parse_units(v: &Value) -> Vec<UnitSummary> {
    let mut out = Vec::new();
    if let Some(items) = v["result"]["items"].as_array() {
        for u in items {
            out.push(UnitSummary {
                entity_id: u["entity_id"].as_u64().unwrap_or(0),
                unit_type: u["unit_type"].as_str().unwrap_or("").to_string(),
                faction: u["faction"].as_str().unwrap_or("").to_string(),
                x: u["x"].as_i64().unwrap_or(0) as i32,
                y: u["y"].as_i64().unwrap_or(0) as i32,
                health: u["health"].as_f64().unwrap_or(0.0) as f32,
                health_max: u["health_max"].as_f64().unwrap_or(0.0) as f32,
            });
        }
    }
    out
}

fn parse_buildings(v: &Value) -> Vec<BuildingSummary> {
    let mut out = Vec::new();
    if let Some(items) = v["result"]["items"].as_array() {
        for b in items {
            out.push(BuildingSummary {
                entity_id: b["entity_id"].as_u64().unwrap_or(0),
                building_type: b["building_type"].as_str().unwrap_or("").to_string(),
                faction: b["faction"].as_str().unwrap_or("").to_string(),
                x: b["x"].as_i64().unwrap_or(0) as i32,
                y: b["y"].as_i64().unwrap_or(0) as i32,
                built: b["built"].as_bool().unwrap_or(false),
                health: b["health"].as_f64().unwrap_or(0.0) as f32,
                health_max: b["health_max"].as_f64().unwrap_or(0.0) as f32,
            });
        }
    }
    out
}

fn parse_tiles(v: &Value) -> Vec<TileSummary> {
    let mut out = Vec::new();
    if let Some(items) = v["result"]["items"].as_array() {
        for t in items {
            out.push(TileSummary {
                x: t["x"].as_i64().unwrap_or(0) as i32,
                y: t["y"].as_i64().unwrap_or(0) as i32,
                terrain: t["terrain"].as_str().unwrap_or("Grass").to_string(),
                cover: t["cover"].as_str().unwrap_or("None").to_string(),
            });
        }
    }
    out
}
