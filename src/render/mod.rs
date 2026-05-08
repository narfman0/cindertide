//! Rendering scaffold.
//!
//! Compiled only with the `render` Cargo feature. The headless build
//! (default) skips this module entirely so tests and CI stay fast.
//!
//! When `render` is enabled, full rendering also requires DefaultPlugins
//! and the corresponding bevy feature flags (`bevy_render`, `bevy_winit`,
//! `bevy_pbr`, `default_font`). Toggle those in Cargo.toml when adopting
//! Synty assets per `world.md`.
//!
//! For now this module exposes a marker plugin that callers can register
//! conditionally.

use bevy::prelude::*;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, _app: &mut App) {
        // Future:
        //   app.add_systems(Startup, spawn_camera);
        //   app.add_systems(Update, draw_units);
        //
        // spawn_camera inserts a Camera3dBundle (or Camera2dBundle for
        // top-down RTS view).
        // draw_units iterates UnitPos entities and spawns or updates a
        // Mesh3d / Material3d child.
        //
        // Without DefaultPlugins those types resolve but no actual
        // rendering happens — registration is the structural placeholder.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_registers_without_panic() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_plugins(RenderPlugin);
        app.update();
    }
}
