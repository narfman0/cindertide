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
            // Flat maps are fallbacks; faction-scoped lookup is preferred.
            for unit in &def.units {
                result.units.entry(unit.id.clone()).or_insert_with(|| unit.clone());
            }
            for building in &def.buildings {
                result.buildings.entry(building.id.clone()).or_insert_with(|| building.clone());
            }
            result.factions.insert(def.id.clone(), def);
        }
        result
    }

    /// Look up a unit by (faction, role-id). Checks the faction's own roster
    /// first, then falls back to the global flat map.
    pub fn faction_unit<'a>(&'a self, faction_id: &str, unit_id: &str) -> Option<&'a UnitDef> {
        self.factions
            .get(faction_id)
            .and_then(|f| f.units.iter().find(|u| u.id == unit_id))
            .or_else(|| self.units.get(unit_id))
    }

    /// Which unit IDs can a building produce, scoped to a faction's roster.
    pub fn faction_building_produces<'a>(&'a self, faction_id: &str, building_id: &str) -> Vec<&'a str> {
        self.factions
            .get(faction_id)
            .and_then(|f| f.buildings.iter().find(|b| b.id == building_id))
            .map(|b| b.produces.iter().map(|s| s.as_str()).collect())
            .unwrap_or_else(|| {
                self.buildings.get(building_id)
                    .map(|b| b.produces.iter().map(|s| s.as_str()).collect())
                    .unwrap_or_default()
            })
    }

    /// Look up a building definition scoped to a faction, falling back to global.
    pub fn faction_building<'a>(&'a self, faction_id: &str, building_id: &str) -> Option<&'a BuildingDef> {
        self.factions
            .get(faction_id)
            .and_then(|f| f.buildings.iter().find(|b| b.id == building_id))
            .or_else(|| self.buildings.get(building_id))
    }
}

pub struct FactionsPlugin;

impl Plugin for FactionsPlugin {
    fn build(&self, _app: &mut App) {}
}
