use bevy::prelude::*;
use bevy::remote::{RemotePlugin, http::RemoteHttpPlugin};

mod map;
mod units;
mod combat;
mod resources;
mod ai;

use map::MapPlugin;

fn main() {
    App::new()
        .add_plugins(MinimalPlugins)
        .add_plugins(RemotePlugin::default())
        .add_plugins(RemoteHttpPlugin::default().with_port(15703))
        .add_plugins(MapPlugin)
        .add_systems(Startup, on_startup)
        .run();
}

fn on_startup() {
    info!("Cindertide initialized");
}
