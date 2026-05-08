use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct MissionNarrative {
    pub title: String,
    pub briefing: String,
    pub win: String,
    pub loss: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FactionNarrative {
    pub missions: Vec<MissionNarrative>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Finales {
    pub combine_first: String,
    pub ironborn_first: String,
}

#[derive(Debug, Clone, Deserialize, Resource)]
pub struct NarrativeData {
    pub factions: HashMap<String, FactionNarrative>,
    pub finales: Finales,
}

impl NarrativeData {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }

    pub fn mission(&self, faction: &str, index: usize) -> Option<&MissionNarrative> {
        self.factions.get(faction)?.missions.get(index)
    }

    pub fn finale(&self, combine_beaten_first: bool) -> &str {
        if combine_beaten_first {
            &self.finales.combine_first
        } else {
            &self.finales.ironborn_first
        }
    }
}
