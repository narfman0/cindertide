use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::render::camera::ScalingMode;
use cindertide::map::{Faction, GridPos, Tile};
use cindertide::units::{UnitPos, UnitType};
use cindertide::buildings::{BuildingPos, BuildingType};
use cindertide::campaign::{CampaignRun, PlayableFaction};
use cindertide::game::{ActiveRun, GameState};
use cindertide::mission::{Mission, MissionStatus};
use cindertide::mapgen::MissionType;
use cindertide::resources::FactionBundle;
use cindertide::combat::PlayerAttackOrder;
use cindertide::units::{MoveTarget, MoveProgress, UnitKind};
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
        .add_plugins((MapPlugin, UnitPlugin, CombatPlugin, ResourcesPlugin, ControlPlugin))
        .add_plugins((BuildingsPlugin, ProductionPlugin, HeroPlugin, TechPlugin, UnitAiPlugin))
        .add_plugins((RepairPlugin, AiPlugin, MissionPlugin, CampaignPlugin, BeatsPlugin))
        .add_plugins((HollowPlugin, SavePlugin, GamePlugin))
        .init_resource::<VisualEntities>()
        .init_resource::<SelectedUnits>()
        .add_systems(Startup, setup_scene)
        .add_systems(PostStartup, auto_start_mission)
        .add_systems(Update, render_tiles)
        .add_systems(Update, spawn_unit_visuals)
        .add_systems(Update, sync_unit_positions)
        .add_systems(Update, spawn_building_visuals)
        .add_systems(Update, camera_pan_zoom)
        .add_systems(Update, edge_scroll)
        .add_systems(Update, handle_mouse_input)
        .add_systems(Update, sync_selection_rings)
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

#[derive(Resource, Default)]
struct SelectedUnits {
    entities: Vec<Entity>,
}

#[derive(Component)]
struct SelectionRing {
    unit_entity: Entity,
}

#[derive(Resource)]
struct PlayerFaction(Faction);

fn grid_to_world(x: i32, y: i32) -> Vec3 {
    Vec3::new(x as f32, 0.0, y as f32)
}

fn screen_to_grid(
    cursor_pos: Vec2,
    camera: &Camera,
    camera_transform: &GlobalTransform,
) -> Option<(i32, i32)> {
    let ray = camera.viewport_to_world(camera_transform, cursor_pos).ok()?;
    if ray.direction.y.abs() < 0.0001 {
        return None;
    }
    let t = -ray.origin.y / ray.direction.y;
    if t <= 0.0 {
        return None;
    }
    let hit = ray.origin + ray.direction * t;
    Some((hit.x.round() as i32, hit.z.round() as i32))
}

fn setup_scene(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        Projection::Orthographic(OrthographicProjection {
            scale: 14.0,
            scaling_mode: ScalingMode::FixedVertical { viewport_height: 1.0 },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(32.0, 30.0, 32.0).looking_at(Vec3::new(20.0, 0.0, 12.0), Vec3::Y),
        IsometricCamera { pan_speed: 20.0, zoom_speed: 2.0 },
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 15000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.4, 0.0)),
    ));
}

fn auto_start_mission(world: &mut World) {
    let player = Faction::Combine;

    cindertide::wipe_world_entities(world);

    *world.resource_mut::<ActiveRun>() = ActiveRun {
        run: Some(CampaignRun {
            faction: PlayableFaction::Combine,
            current_mission: 0,
            outcomes: Vec::new(),
            complete: false,
        }),
        current_mission_entity: None,
        missions_won: 0,
        missions_lost: 0,
    };

    world.spawn(FactionBundle::new(player.clone()));

    let mission_entity = world.spawn(Mission {
        mission_type: MissionType::Control,
        player_faction: player.clone(),
        opponent_faction: Faction::Ironborn,
        status: MissionStatus::Active,
        elapsed: 0.0,
        deadline: 300.0,
    }).id();

    cindertide::setup_demo_scenario(world, &player);

    world.resource_mut::<ActiveRun>().current_mission_entity = Some(mission_entity);
    *world.resource_mut::<GameState>() = GameState::InMission;

    world.insert_resource(PlayerFaction(player.clone()));
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
                perceptual_roughness: 0.9,
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
        let world_pos = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.75;
        let visual = commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.6, 1.5, 0.6))),
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
                let target = grid_to_world(pos.pos.x, pos.pos.y) + Vec3::Y * 0.75;
                transform.translation = transform.translation.lerp(target, 0.15);
            }
        }
    }
}

fn spawn_building_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut visual_entities: ResMut<VisualEntities>,
    buildings: Query<(Entity, &BuildingPos, &Faction), Added<BuildingType>>,
) {
    for (entity, pos, faction) in &buildings {
        if visual_entities.buildings.contains_key(&entity) {
            continue;
        }
        let color = faction_color(faction).mix(&Color::WHITE, 0.25);
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

fn handle_mouse_input(
    mut commands: Commands,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<IsometricCamera>>,
    units: Query<(Entity, &UnitPos, &Faction), With<UnitType>>,
    mut selected: ResMut<SelectedUnits>,
    player_faction: Option<Res<PlayerFaction>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    selection_rings: Query<Entity, With<SelectionRing>>,
    tiles: Query<&Tile>,
) {
    let Some(player_faction) = player_faction else { return };
    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_transform)) = cameras.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };

    if buttons.just_pressed(MouseButton::Left) {
        let Some((gx, gy)) = screen_to_grid(cursor_pos, camera, cam_transform) else { return };

        let clicked_unit = units.iter().find(|(_, pos, faction)| {
            pos.pos.x == gx && pos.pos.y == gy && **faction == player_faction.0
        }).map(|(e, _, _)| e);

        for ring in &selection_rings {
            commands.entity(ring).despawn();
        }
        selected.entities.clear();

        if let Some(unit_entity) = clicked_unit {
            selected.entities.push(unit_entity);
            if let Ok((_, pos, _)) = units.get(unit_entity) {
                let ring_pos = grid_to_world(pos.pos.x, pos.pos.y) - Vec3::Y * 0.35;
                commands.spawn((
                    Mesh3d(meshes.add(Cylinder::new(0.4, 0.05))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgb(1.0, 1.0, 0.0),
                        emissive: LinearRgba::new(1.0, 1.0, 0.0, 1.0),
                        ..default()
                    })),
                    Transform::from_translation(ring_pos),
                    SelectionRing { unit_entity },
                ));
            }
        }
    }

    if buttons.just_pressed(MouseButton::Right) {
        if selected.entities.is_empty() {
            return;
        }
        let Some((gx, gy)) = screen_to_grid(cursor_pos, camera, cam_transform) else { return };

        let target_pos = GridPos { x: gx, y: gy };

        let enemy_at_target = units.iter().find(|(_, pos, faction)| {
            pos.pos.x == gx && pos.pos.y == gy && **faction != player_faction.0
        }).map(|(e, _, _)| e);

        let tile_exists = tiles.iter().any(|t| t.pos.x == gx && t.pos.y == gy);
        if !tile_exists && enemy_at_target.is_none() {
            return;
        }

        let selected_entities: Vec<Entity> = selected.entities.clone();

        if let Some(target_entity) = enemy_at_target {
            for &unit_entity in &selected_entities {
                commands.entity(unit_entity)
                    .remove::<MoveTarget>()
                    .remove::<MoveProgress>()
                    .insert(PlayerAttackOrder { target: target_entity });
            }
        } else {
            let tile_map: HashMap<(i32, i32), cindertide::map::TerrainType> = tiles
                .iter()
                .map(|t| ((t.pos.x, t.pos.y), t.terrain_type.clone()))
                .collect();
            let max_x = tile_map.keys().map(|(x, _)| *x).max().unwrap_or(40);
            let max_y = tile_map.keys().map(|(_, y)| *y).max().unwrap_or(25);

            for &unit_entity in &selected_entities {
                let unit_kind = units.get(unit_entity).ok().and_then(|(_, _, _)| {
                    None::<cindertide::units::UnitKind>
                });
                let start = units.get(unit_entity).ok().map(|(_, pos, _)| pos.pos.clone());
                let Some(start) = start else { continue };

                let pf_kind = match unit_kind {
                    Some(UnitKind::Vehicle) => cindertide::map::pathfinding::UnitKind::Vehicle,
                    _ => cindertide::map::pathfinding::UnitKind::Infantry,
                };
                let grid = cindertide::map::pathfinding::PathfindingGrid {
                    width: max_x + 1,
                    height: max_y + 1,
                    tiles: tile_map.clone(),
                    unit_type: pf_kind,
                };
                if let Some(path) = grid.find_path(start, target_pos.clone()) {
                    commands.entity(unit_entity)
                        .insert(MoveTarget { target: target_pos.clone() })
                        .insert(MoveProgress { path, current_step: 0, elapsed: 0.0 });
                }
            }
        }
    }
}

fn sync_selection_rings(
    units: Query<&UnitPos, With<UnitType>>,
    mut rings: Query<(&SelectionRing, &mut Transform)>,
) {
    for (ring, mut transform) in &mut rings {
        if let Ok(pos) = units.get(ring.unit_entity) {
            let target = grid_to_world(pos.pos.x, pos.pos.y) - Vec3::Y * 0.35;
            transform.translation = transform.translation.lerp(target, 0.15);
        }
    }
}

fn edge_scroll(
    mut cameras: Query<(&mut Transform, &IsometricCamera)>,
    windows: Query<&Window>,
    time: Res<Time>,
) {
    let Ok(window) = windows.single() else { return };
    let Ok((mut transform, cam)) = cameras.single_mut() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let w = window.width();
    let h = window.height();
    let margin = 20.0;
    let dt = time.delta_secs();
    let mut pan = Vec3::ZERO;
    if cursor.x < margin { pan += Vec3::new(-1.0, 0.0, 1.0).normalize(); }
    if cursor.x > w - margin { pan += Vec3::new(1.0, 0.0, -1.0).normalize(); }
    if cursor.y < margin { pan += Vec3::new(-1.0, 0.0, -1.0).normalize(); }
    if cursor.y > h - margin { pan += Vec3::new(1.0, 0.0, 1.0).normalize(); }
    transform.translation += pan * cam.pan_speed * dt;
}

fn camera_pan_zoom(
    mut query: Query<(&mut Transform, &mut Projection, &IsometricCamera)>,
    keys: Res<ButtonInput<KeyCode>>,
    mut scroll: EventReader<MouseWheel>,
    time: Res<Time>,
) {
    let Ok((mut transform, mut projection, cam)) = query.single_mut() else { return };

    let dt = time.delta_secs();
    let mut pan = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) { pan += Vec3::new(-1.0, 0.0, -1.0).normalize(); }
    if keys.pressed(KeyCode::KeyS) { pan += Vec3::new(1.0, 0.0, 1.0).normalize(); }
    if keys.pressed(KeyCode::KeyA) { pan += Vec3::new(-1.0, 0.0, 1.0).normalize(); }
    if keys.pressed(KeyCode::KeyD) { pan += Vec3::new(1.0, 0.0, -1.0).normalize(); }
    transform.translation += pan * cam.pan_speed * dt;

    if let Projection::Orthographic(ref mut ortho) = *projection {
        for ev in scroll.read() {
            ortho.scale = (ortho.scale - ev.y * cam.zoom_speed).clamp(2.0, 60.0);
        }
    }
}

fn terrain_color(terrain: &cindertide::map::TerrainType) -> Color {
    use cindertide::map::TerrainType;
    match terrain {
        TerrainType::Grass   => Color::srgb(0.30, 0.50, 0.20),
        TerrainType::Road    => Color::srgb(0.40, 0.40, 0.40),
        TerrainType::Forest  => Color::srgb(0.10, 0.35, 0.10),
        TerrainType::Rubble  => Color::srgb(0.50, 0.45, 0.40),
        TerrainType::Mud     => Color::srgb(0.45, 0.30, 0.15),
        TerrainType::Corrupted => Color::srgb(0.60, 0.10, 0.60),
        _                    => Color::srgb(0.50, 0.50, 0.50),
    }
}

fn faction_color(faction: &Faction) -> Color {
    match faction {
        Faction::Combine  => Color::srgb(0.90, 0.75, 0.10),
        Faction::Ironborn => Color::srgb(0.60, 0.60, 0.65),
        Faction::Covenant => Color::srgb(0.20, 0.40, 0.90),
        Faction::Hollow   => Color::srgb(0.70, 0.10, 0.70),
    }
}
