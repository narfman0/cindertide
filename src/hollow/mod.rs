// Hollow — corruption-zone spawners that produce Hollow-faction units on a
// timer. Spawn rate scales with mode and (in Act 3) the United Voice.

use bevy::prelude::*;
use crate::map::{Faction, GridPos};
use crate::units::{RiflemanBundle, HomeBase};
use crate::campaign::{CampaignState, Act};

#[derive(Component, Debug, Clone, PartialEq)]
pub enum HollowMode {
    Consuming,
    Subsuming,
    Indifferent,
}

#[derive(Component, Debug, Clone)]
pub struct HollowSpawner {
    pub mode: HollowMode,
    pub pos: GridPos,
    pub elapsed: f32,
    pub interval: f32,
}

impl HollowSpawner {
    pub fn new(mode: HollowMode, x: i32, y: i32) -> Self {
        let interval = match mode {
            HollowMode::Consuming => 6.0,
            HollowMode::Subsuming => 8.0,
            HollowMode::Indifferent => 10.0,
        };
        Self { mode, pos: GridPos { x, y }, elapsed: 0.0, interval }
    }
}

// --- Pure helper ---

/// Effective interval given Act-3 doubling.
pub fn effective_interval(base: f32, act: Act) -> f32 {
    match act {
        Act::Three => base * 0.5, // United Voice: doubled rate = halved interval
        _ => base,
    }
}

// --- System ---

pub fn hollow_spawn_system(
    mut commands: Commands,
    time: Res<Time>,
    mut spawners: Query<&mut HollowSpawner>,
    campaign: Option<Res<CampaignState>>,
) {
    let dt = time.delta_secs();
    let act = campaign.as_ref().map(|c| c.act).unwrap_or(Act::One);

    for mut s in &mut spawners {
        s.elapsed += dt;
        let interval = effective_interval(s.interval, act);
        if s.elapsed >= interval {
            s.elapsed = 0.0;
            // Spawn a Hollow rifleman at the spawner's position.
            let id = commands
                .spawn(RiflemanBundle::with_faction(s.pos.x, s.pos.y, Faction::Hollow))
                .id();
            commands.entity(id).insert(HomeBase { pos: s.pos.clone() });
        }
    }
}

pub struct HollowPlugin;

impl Plugin for HollowPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, hollow_spawn_system);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consuming_mode_has_shorter_interval() {
        let c = HollowSpawner::new(HollowMode::Consuming, 0, 0);
        let s = HollowSpawner::new(HollowMode::Subsuming, 0, 0);
        let i = HollowSpawner::new(HollowMode::Indifferent, 0, 0);
        assert!(c.interval < s.interval);
        assert!(s.interval < i.interval);
    }

    #[test]
    fn united_voice_halves_interval() {
        assert_eq!(effective_interval(8.0, Act::Three), 4.0);
        assert_eq!(effective_interval(8.0, Act::One), 8.0);
        assert_eq!(effective_interval(8.0, Act::Two), 8.0);
    }
}
