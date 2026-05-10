use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResourceCostDef {
    #[serde(default)] pub fuel: f32,
    #[serde(default)] pub scrap: f32,
    #[serde(default)] pub manpower: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UnitDef {
    pub id: String,
    pub display_name: String,
    /// "infantry" or "vehicle"
    pub unit_kind: String,
    pub attack_range: f32,
    pub move_speed: f32,
    pub health: f32,
    pub attack_damage: f32,
    #[serde(default)] pub suppression_value: f32,
    #[serde(default)] pub suppression_resistance: f32,
    pub attack_speed: f32,
    pub vision_range: f32,
    #[serde(default)] pub production_cost: ResourceCostDef,
    /// Building IDs that can produce this unit
    #[serde(default)] pub produced_at: Vec<String>,
    #[serde(default = "default_build_time")] pub build_time_seconds: f32,
    #[serde(default)] pub ability_q: String,
    #[serde(default)] pub model_file: String,
    #[serde(default)] pub description: String,
}

fn default_build_time() -> f32 { 10.0 }

#[derive(Debug, Clone, Deserialize)]
pub struct BuildingDef {
    pub id: String,
    pub display_name: String,
    pub health: f32,
    #[serde(default)] pub cost: ResourceCostDef,
    #[serde(default = "default_build_time")] pub build_time_seconds: f32,
    /// Unit IDs this building can produce
    #[serde(default)] pub produces: Vec<String>,
    #[serde(default)] pub model_file: String,
    #[serde(default)] pub description: String,
    #[serde(default)] pub trickle_fuel: f32,
    #[serde(default)] pub trickle_scrap: f32,
    #[serde(default)] pub trickle_manpower: f32,
    #[serde(default)] pub vision_radius: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FactionLoadoutBuilding {
    pub building: String,
    pub dx: i32,
    pub dy: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FactionLoadoutUnit {
    pub unit: String,
    pub dx: i32,
    pub dy: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FactionLoadoutDef {
    /// List of buildings for starting base
    #[serde(default)] pub buildings: Vec<FactionLoadoutBuilding>,
    /// List of units for starting army
    #[serde(default)] pub units: Vec<FactionLoadoutUnit>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FactionDef {
    pub id: String,
    pub display_name: String,
    /// RGB components 0.0-1.0
    pub color: [f32; 3],
    #[serde(default)] pub ai_script: String,
    #[serde(default)] pub default_opponent: String,
    #[serde(default)] pub units: Vec<UnitDef>,
    #[serde(default)] pub buildings: Vec<BuildingDef>,
    #[serde(default)] pub loadouts: Vec<FactionLoadoutDef>,
}

#[derive(Resource, Default, Clone)]
pub struct LoadedFactions {
    pub factions: HashMap<String, FactionDef>,
    /// All units across all factions, keyed by unit id
    pub units: HashMap<String, UnitDef>,
    /// All buildings across all factions, keyed by building id
    pub buildings: HashMap<String, BuildingDef>,
}

impl LoadedFactions {
    /// Look up a unit definition for a specific faction.
    /// Falls back to the global units map.
    pub fn faction_unit(&self, faction_id: &str, unit_id: &str) -> Option<&UnitDef> {
        if let Some(faction) = self.factions.get(faction_id) {
            if let Some(def) = faction.units.iter().find(|u| u.id == unit_id) {
                return Some(def);
            }
        }
        self.units.get(unit_id)
    }

    /// Look up a building definition for a specific faction.
    /// Falls back to the global buildings map (since building IDs are shared across factions).
    pub fn faction_building(&self, faction_id: &str, building_id: &str) -> Option<&BuildingDef> {
        // First try faction-specific definition
        if let Some(faction) = self.factions.get(faction_id) {
            if let Some(def) = faction.buildings.iter().find(|b| b.id == building_id) {
                return Some(def);
            }
        }
        // Fall back to global buildings map
        self.buildings.get(building_id)
    }

    pub fn load_from_dir(dir: &str) -> Self {
        let mut result = LoadedFactions::default();
        let Ok(entries) = std::fs::read_dir(dir) else { return result };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") { continue; }
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            let Ok(def): Result<FactionDef, _> = toml::from_str(&content) else {
                eprintln!("Warning: failed to parse faction file {:?}", path);
                continue;
            };
            for unit in &def.units {
                result.units.insert(unit.id.clone(), unit.clone());
            }
            for building in &def.buildings {
                result.buildings.insert(building.id.clone(), building.clone());
            }
            result.factions.insert(def.id.clone(), def);
        }
        result
    }
}

pub struct FactionsPlugin;

impl Plugin for FactionsPlugin {
    fn build(&self, _app: &mut App) {}
}
