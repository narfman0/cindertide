use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::render::camera::ScalingMode;
use cindertide::map::{Faction, Tile};
use cindertide::units::{UnitPos, UnitType};
use cindertide::buildings::{BuildingPos, Built};
use cindertide::{
    map::MapPlugin,
    units::UnitPlugin,
    combat::CombatPlugin,
    resources::ResourcesPlugin,
    control::ControlPlugin,
    buildings::BuildingsPlugin,
    production::ProductionPlugin,
    heroes::HeroPlugin,
    tech::TechPlugin,
    unit_ai::UnitAiPlugin,
    repair::RepairPlugin,
    ai::AiPlugin,
    mission::MissionPlugin,
    campaign::CampaignPlugin,
    beats::BeatsPlugin,
    hollow::HollowPlugin,
    save::SavePlugin,
    game::GamePlugin,
};
use std::collections::HashMap;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Cindertide".into(),
                resolution: (1280.0, 720.0).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(MapPlugin)
        .add_plugins(UnitPlugin)
        .add_plugins(CombatPlugin)
        .add_plugins(ResourcesPlugin)
        .add_plugins(ControlPlugin)
        .add_plugins(BuildingsPlugin)
        .add_plugins(ProductionPlugin)
        .add_plugins(HeroPlugin)
        .add_plugins(TechPlugin)
        .add_plugins(UnitAiPlugin)
        .add_plugins(RepairPlugin)
        .add_plugins(AiPlugin)
        .add_plugins(MissionPlugin)
        .add_plugins(CampaignPlugin)
        .add_plugins(BeatsPlugin)
        .add_plugins(HollowPlugin)
        .add_plugins(SavePlugin)
        .add_plugins(GamePlugin)
        .init_resource::<VisualEntities>()
        .add_systems(Startup, setup_scene)
        .add_systems(Update, render_tiles)
        .add_systems(Update, spawn_unit_visuals)
        .add_systems(Update, sync_unit_positions)
        .add_systems(Update, spawn_building_visuals)
        .add_systems(Update, camera_pan_zoom)
        .run();
}

#[derive(Component)]
struct IsometricCamera {
    pan_speed: f32,
    zoom_speed: f32,
}

#[derive(Component)]
struct RenderedTile;

#[derive(Resource, Default)]
struct VisualEntities {
    units: HashMap<Entity, Entity>,
    buildings: HashMap<Entity, Entity>,
}

fn grid_to_world(x: i32, y: i32) -> Vec3 {
    Vec3::new(x as f32, 0.0, y as f32)
}

fn setup_scene(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Projection::Orthographic(OrthographicProjection {
            scale: 0.05,
            scaling_mode: ScalingMode::FixedVertical { viewport_height: 10.0 },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(20.0, 20.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
        IsometricCamera { pan_speed: 10.0, zoom_speed: 0.1 },
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.7, 0.5, 0.0)),
    ));
}

fn render_tiles(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    tiles: Query<&Tile, Without<RenderedTile>>,
) {
    for tile in &tiles {
        let color = terrain_color(&tile.terrain_type);
        let pos = grid_to_world(tile.pos.x, tile.pos.y);
        commands.spawn((
            Mesh3d(meshes.add(Plane3d::default().mesh().size(0.95, 0.95))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                ..default()
            })),
            Transform::from_translation(pos),
            RenderedTile,
        ));
    }
}

fn spawn_unit_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut visual_entities: ResMut<VisualEntities>,
    units: Query<(Entity, &UnitPos, &Faction), Added<UnitType>>,
) {
    for (entity, pos, faction) in &units {
        let color = faction_color(faction);
        let world_pos = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.4;
        let visual = commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.5, 0.8, 0.5))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                ..default()
            })),
            Transform::from_translation(world_pos),
        )).id();
        visual_entities.units.insert(entity, visual);
    }
}

fn sync_unit_positions(
    units: Query<(Entity, &UnitPos), With<UnitType>>,
    visual_entities: Res<VisualEntities>,
    mut transforms: Query<&mut Transform>,
) {
    for (entity, pos) in &units {
        if let Some(&visual) = visual_entities.units.get(&entity) {
            if let Ok(mut transform) = transforms.get_mut(visual) {
                transform.translation = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.4;
            }
        }
    }
}

fn spawn_building_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut visual_entities: ResMut<VisualEntities>,
    buildings: Query<(Entity, &BuildingPos, &Faction), Added<Built>>,
) {
    for (entity, pos, faction) in &buildings {
        let color = faction_color(faction).mix(&Color::WHITE, 0.3);
        let world_pos = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.5;
        let visual = commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.9, 1.0, 0.9))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                ..default()
            })),
            Transform::from_translation(world_pos),
        )).id();
        visual_entities.buildings.insert(entity, visual);
    }
}

fn camera_pan_zoom(
    mut query: Query<(&mut Transform, &mut Projection, &IsometricCamera)>,
    keys: Res<ButtonInput<KeyCode>>,
    mut scroll: EventReader<MouseWheel>,
    time: Res<Time>,
) {
    let Ok((mut transform, mut projection, cam)) = query.single_mut() else {
        return;
    };

    let dt = time.delta_secs();
    let mut pan = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        pan += Vec3::new(-1.0, 0.0, -1.0).normalize();
    }
    if keys.pressed(KeyCode::KeyS) {
        pan += Vec3::new(1.0, 0.0, 1.0).normalize();
    }
    if keys.pressed(KeyCode::KeyA) {
        pan += Vec3::new(-1.0, 0.0, 1.0).normalize();
    }
    if keys.pressed(KeyCode::KeyD) {
        pan += Vec3::new(1.0, 0.0, -1.0).normalize();
    }
    transform.translation += pan * cam.pan_speed * dt;

    if let Projection::Orthographic(ref mut ortho) = *projection {
        for ev in scroll.read() {
            ortho.scale = (ortho.scale - ev.y * cam.zoom_speed).clamp(0.01, 0.5);
        }
    }
}

fn terrain_color(terrain: &cindertide::map::TerrainType) -> Color {
    use cindertide::map::TerrainType;
    match terrain {
        TerrainType::Grass => Color::srgb(0.3, 0.5, 0.2),
        TerrainType::Road => Color::srgb(0.4, 0.4, 0.4),
        TerrainType::Forest => Color::srgb(0.1, 0.35, 0.1),
        TerrainType::Rubble => Color::srgb(0.5, 0.45, 0.4),
        TerrainType::Mud => Color::srgb(0.45, 0.3, 0.15),
        TerrainType::Corrupted => Color::srgb(0.6, 0.1, 0.6),
        _ => Color::srgb(0.5, 0.5, 0.5),
    }
}

fn faction_color(faction: &Faction) -> Color {
    match faction {
        Faction::Combine => Color::srgb(0.9, 0.75, 0.1),
        Faction::Ironborn => Color::srgb(0.6, 0.6, 0.65),
        Faction::Covenant => Color::srgb(0.2, 0.4, 0.9),
        Faction::Hollow => Color::srgb(0.7, 0.1, 0.7),
    }
}
