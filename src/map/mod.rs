use bevy::prelude::*;

pub mod pathfinding;

// Terrain types matching docs/world.md
#[derive(Debug, Clone, PartialEq)]
pub enum TerrainType {
    Road,
    Grass,
    Forest,
    Rubble,
    Mud,
    Corrupted,
    Void,
}

// Cover density per mechanics.md
#[derive(Debug, Clone, PartialEq)]
pub enum CoverDensity {
    None,
    Light,
    Heavy,
}

// Control point types per mechanics.md
#[derive(Debug, Clone, PartialEq)]
pub enum ControlPointType {
    Strategic,
    FuelDepot,
    ScrapField,
    HighGround,
    AncientRuins,
}

// Faction owner
#[derive(Debug, Clone, PartialEq)]
pub enum Faction {
    Combine,
    Covenant,
    Ironborn,
    Hollow,
}

// Grid position
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GridPos {
    pub x: i32,
    pub y: i32,
}

// Bevy components
#[derive(Component)]
pub struct Zone {
    pub name: String,
    pub terrain_type: TerrainType,
}

#[derive(Component)]
pub struct Tile {
    pub pos: GridPos,
    pub terrain_type: TerrainType,
    pub cover: CoverDensity,
}

#[derive(Component)]
pub struct ControlPoint {
    pub point_type: ControlPointType,
    pub owner: Option<Faction>,
    pub capture_progress: f32,
}

// Movement cost per terrain type
pub fn movement_cost(terrain: &TerrainType) -> f32 {
    match terrain {
        TerrainType::Road => 0.5,
        TerrainType::Grass => 1.0,
        TerrainType::Forest => 2.0,
        TerrainType::Rubble => 2.0,
        TerrainType::Mud => 3.0,
        TerrainType::Corrupted => 2.5,
        TerrainType::Void => 10.0,
    }
}

// Whether vehicles can pass through this terrain
pub fn vehicle_passable(terrain: &TerrainType) -> bool {
    !matches!(terrain, TerrainType::Forest | TerrainType::Rubble)
}

// Damage reduction from cover
pub fn damage_reduction(cover: &CoverDensity) -> f32 {
    match cover {
        CoverDensity::None => 0.0,
        CoverDensity::Light => 0.25,
        CoverDensity::Heavy => 0.5,
    }
}

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, _app: &mut App) {
        // Register component types with the Bevy app.
        // Components are registered automatically when inserted;
        // explicit registration here for discoverability.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_movement_costs() {
        assert_eq!(movement_cost(&TerrainType::Road), 0.5);
        assert_eq!(movement_cost(&TerrainType::Grass), 1.0);
        assert_eq!(movement_cost(&TerrainType::Forest), 2.0);
        assert_eq!(movement_cost(&TerrainType::Rubble), 2.0);
        assert_eq!(movement_cost(&TerrainType::Mud), 3.0);
        assert_eq!(movement_cost(&TerrainType::Corrupted), 2.5);
        assert_eq!(movement_cost(&TerrainType::Void), 10.0);
    }

    #[test]
    fn test_vehicle_passable() {
        assert!(vehicle_passable(&TerrainType::Road));
        assert!(vehicle_passable(&TerrainType::Grass));
        assert!(!vehicle_passable(&TerrainType::Forest));
        assert!(!vehicle_passable(&TerrainType::Rubble));
        assert!(vehicle_passable(&TerrainType::Mud));
        assert!(vehicle_passable(&TerrainType::Corrupted));
        assert!(vehicle_passable(&TerrainType::Void));
    }

    #[test]
    fn test_damage_reduction() {
        assert_eq!(damage_reduction(&CoverDensity::None), 0.0);
        assert_eq!(damage_reduction(&CoverDensity::Light), 0.25);
        assert_eq!(damage_reduction(&CoverDensity::Heavy), 0.5);
    }

    #[test]
    fn test_grid_pos_equality() {
        let a = GridPos { x: 3, y: 7 };
        let b = GridPos { x: 3, y: 7 };
        let c = GridPos { x: 0, y: 0 };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_grid_pos_clone() {
        let a = GridPos { x: 5, y: -2 };
        let b = a.clone();
        assert_eq!(a, b);
    }
}
