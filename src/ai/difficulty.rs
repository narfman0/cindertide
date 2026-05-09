// AI difficulty configuration — loaded from assets/ai/<name>.toml.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AiParams {
    pub attack_wave_interval: f32,
    pub reaction_time: f32,
    pub aggression: f32,
    pub scout_radius: i32,
    pub focus_fire: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CheatMultipliers {
    pub resource_multiplier: f32,
    pub build_time_multiplier: f32,
    pub starting_resource_bonus: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AiDifficultyConfig {
    pub params: AiParams,
    pub cheats: CheatMultipliers,
}

impl AiDifficultyConfig {
    /// Load difficulty config from `assets/ai/<name>.toml`. Falls back to
    /// normal defaults if the file is missing or cannot be parsed.
    pub fn load(name: &str) -> Self {
        let path = format!("assets/ai/{}.toml", name);
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_else(Self::normal)
    }

    /// Hard-coded normal defaults used as a fallback.
    pub fn normal() -> Self {
        Self {
            params: AiParams {
                attack_wave_interval: 90.0,
                reaction_time: 1.5,
                aggression: 0.6,
                scout_radius: 10,
                focus_fire: true,
            },
            cheats: CheatMultipliers {
                resource_multiplier: 1.0,
                build_time_multiplier: 1.0,
                starting_resource_bonus: 0.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_fallback_has_sensible_defaults() {
        let cfg = AiDifficultyConfig::normal();
        assert_eq!(cfg.params.attack_wave_interval, 90.0);
        assert_eq!(cfg.cheats.resource_multiplier, 1.0);
        assert_eq!(cfg.cheats.starting_resource_bonus, 0.0);
    }

    #[test]
    fn load_unknown_name_falls_back_to_normal() {
        let cfg = AiDifficultyConfig::load("__nonexistent__");
        assert_eq!(cfg.params.attack_wave_interval, 90.0);
    }
}
