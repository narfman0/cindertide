// Scripted camera state, shared between the client (which owns the IsometricCamera
// Transform) and the mission_script system (which can request camera focus during
// dialogue / cutscenes).
//
// `CameraTarget` is a resource read by the client's camera-follow system each frame.
// When `Free`, WASD / edge-scroll input pans the camera as normal. When `LookAt` or
// `Follow`, the camera tweens toward the requested world position; user input still
// overrides it (setting the target back to `Free`).

use bevy::prelude::*;
use serde::Deserialize;

#[derive(Resource, Clone, Default, Debug)]
pub enum CameraTarget {
    /// User-controlled camera. WASD / edge-scroll / scroll-zoom apply normally.
    #[default]
    Free,
    /// Tween camera to look at the given world point.
    LookAt(Vec3),
    /// Tween camera to follow the given entity's Transform translation.
    Follow(Entity),
}

/// Script-level camera focus target. Resolved against the live ECS in
/// `script_tick_system` and written into the global `CameraTarget` resource.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CameraFocusTarget {
    /// Center the camera on the player's home base (`command_bunker` buildings).
    HomeBase,
    /// Grid tile coordinates.
    Position { x: i32, y: i32 },
    /// First unit matching the given (faction, unit_type) pair. Used for "follow speaker"
    /// during dialogue. `unit_type` is the canonical id (e.g. "riflemen").
    Unit { faction: String, unit_type: String },
}
