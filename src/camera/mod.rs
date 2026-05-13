// Cinematic camera state, shared between the client (which owns the IsometricCamera
// Transform + Projection) and the mission_script system (which can request camera
// focus during dialogue / cutscenes).
//
// `CameraTarget` is the resource read by the client's camera-follow system each
// frame. When `Free`, WASD / edge-scroll input pans the camera as normal. When
// `LookAt` or `Follow`, the camera tweens toward the requested world position
// AND tweens its orthographic scale toward the framing preset's value. User
// input still overrides scripted focus (resets the target to `Free`).
//
// Framing presets are hardcoded math primitives in `framing_for`. Faction TOML
// files reference them by name (e.g., `cinematic_framing = "low_angle_hero"`)
// to set per-faction cinematography defaults; individual unit/building defs
// can override.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// A single cinematographer recipe: where the camera sits relative to its subject,
/// and how zoomed-in it is (orthographic scale).
#[derive(Clone, Copy, Debug)]
pub struct FramingPreset {
    /// Camera world-position offset from the subject's world position.
    /// Y is height; X/Z are horizontal. Magnitude doesn't matter for an orthographic
    /// camera (only direction), so use small values to keep clipping planes happy.
    pub offset: Vec3,
    /// Bevy `OrthographicProjection.scale`. Smaller = more zoomed in.
    pub scale: f32,
}

impl Default for FramingPreset {
    fn default() -> Self {
        FramingPreset { offset: Vec3::new(10.0, 10.0, 10.0), scale: 28.0 }
    }
}

/// Resolve a named framing preset. Unknown / empty names fall back to "isometric"
/// (the standard RTS view used during normal gameplay).
///
/// Preset gallery:
///   - **isometric**: standard gameplay framing — equal X/Y/Z offset, wide scale
///   - **low_angle_hero**: low, pulled back; subject looms — Ironborn signature
///   - **close_up**: tight zoom, eye-level — Architect log-entry feel
///   - **over_shoulder**: high-up corporate POV — Combine "Field Comms" feel
///   - **off_kilter**: skewed asymmetric angle — Hollow corruption feel
///   - **wide_establishing**: pulled way back, sees the whole battlefield
pub fn framing_for(name: &str) -> FramingPreset {
    match name.to_lowercase().as_str() {
        "low_angle_hero" => FramingPreset { offset: Vec3::new(8.0, 3.0, 8.0), scale: 18.0 },
        "close_up" => FramingPreset { offset: Vec3::new(4.0, 3.0, 4.0), scale: 8.0 },
        "over_shoulder" => FramingPreset { offset: Vec3::new(6.0, 8.0, 6.0), scale: 14.0 },
        "off_kilter" => FramingPreset { offset: Vec3::new(12.0, 5.0, -6.0), scale: 16.0 },
        "wide_establishing" => FramingPreset { offset: Vec3::new(25.0, 20.0, 25.0), scale: 45.0 },
        // "isometric" — and anything unknown — uses the gameplay default.
        _ => FramingPreset::default(),
    }
}

#[derive(Resource, Clone, Default, Debug)]
pub enum CameraTarget {
    /// User-controlled camera. WASD / edge-scroll / scroll-zoom apply normally.
    #[default]
    Free,
    /// Tween camera to look at the given world point with the given framing.
    LookAt { point: Vec3, framing: FramingPreset },
    /// Tween camera to follow the given entity's translation with the given framing.
    Follow { entity: Entity, framing: FramingPreset },
}

/// Optional component on unit/building entities: when the camera follows this
/// entity without an explicit script-level framing, the resolver uses this name
/// before falling back to faction defaults.
#[derive(Component, Clone, Debug)]
pub struct CinematicFraming(pub String);

/// Active camera-shake state. Decays toward zero over `duration` seconds.
/// `intensity` is the world-space amplitude of the per-frame jitter.
///
/// Typical values: 0.05 for a heavy footstep, 0.2 for an explosion, 0.5 for
/// a Hollow corruption-zone reveal. Above ~1.0 the camera feels broken.
#[derive(Resource, Default, Clone, Debug)]
pub struct CameraShake {
    pub intensity: f32,
    pub remaining: f32,
    pub total: f32,
}

impl CameraShake {
    pub fn trigger(&mut self, intensity: f32, duration: f32) {
        // Take the max so a stronger shake overrides a weaker one in progress
        // rather than fading mid-event.
        if intensity > self.intensity {
            self.intensity = intensity;
        }
        self.remaining = self.remaining.max(duration);
        self.total = self.total.max(duration);
    }

    /// Linear decay multiplier 0..1 based on remaining/total.
    pub fn amplitude(&self) -> f32 {
        if self.total <= 0.0 || self.remaining <= 0.0 { return 0.0; }
        self.intensity * (self.remaining / self.total)
    }
}

/// Script-level camera focus target. Resolved against the live ECS in
/// `script_tick_system` and written into the global `CameraTarget` resource.
#[derive(Debug, Clone, Deserialize, Serialize)]
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
