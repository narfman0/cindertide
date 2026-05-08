use bevy::prelude::*;
use crate::map::GridPos;

pub mod movement;

// Current grid position of a unit
#[derive(Component, Debug, Clone)]
pub struct UnitPos {
    pub pos: GridPos,
}

// Destination when moving
#[derive(Component, Debug, Clone)]
pub struct MoveTarget {
    pub target: GridPos,
}

// Movement speed in tiles per second
#[derive(Component, Debug, Clone)]
pub struct MovementSpeed {
    pub tiles_per_second: f32,
}

// Time accumulator for movement
#[derive(Component, Debug, Clone)]
pub struct MoveProgress {
    pub elapsed: f32,
    pub path: Vec<GridPos>,
    pub current_step: usize,
}

impl MoveProgress {
    pub fn new(path: Vec<GridPos>) -> Self {
        Self {
            elapsed: 0.0,
            path,
            current_step: 0,
        }
    }
}

pub struct UnitPlugin;

impl Plugin for UnitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, movement::move_units_system);
    }
}
