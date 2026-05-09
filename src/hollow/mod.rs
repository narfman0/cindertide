use bevy::prelude::*;
use crate::map::{Faction, GridPos};
use crate::units::{UnitBundle, HomeBase};

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

pub fn hollow_spawn_system(
    mut commands: Commands,
    time: Res<Time>,
    mut spawners: Query<&mut HollowSpawner>,
) {
    let dt = time.delta_secs();
    for mut s in &mut spawners {
        s.elapsed += dt;
        if s.elapsed >= s.interval {
            s.elapsed = 0.0;
            let id = commands
                .spawn(UnitBundle::default_riflemen(Faction::hollow(), s.pos.x, s.pos.y))
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
    fn spawner_intervals_are_positive() {
        let modes = [HollowMode::Consuming, HollowMode::Subsuming, HollowMode::Indifferent];
        for m in modes {
            let s = HollowSpawner::new(m, 0, 0);
            assert!(s.interval > 0.0);
        }
    }
}
