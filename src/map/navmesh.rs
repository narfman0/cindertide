// Baked navigation mesh for the whole map.
// Precomputes movement costs per tile so A* doesn't need HashMap lookups.

use std::collections::HashMap;
use bevy::prelude::*;
use crate::map::{TerrainType, movement_cost, vehicle_passable};
use crate::map::pathfinding::UnitKind;

#[derive(Resource, Debug, Clone)]
pub struct NavMesh {
    pub width: i32,
    pub height: i32,
    /// Flat index: y * width + x. f32::MAX = impassable.
    pub infantry_cost: Vec<f32>,
    pub vehicle_cost: Vec<f32>,
}

impl NavMesh {
    fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.width + x) as usize
    }

    fn cost_for_terrain_infantry(terrain: &TerrainType) -> f32 {
        match terrain {
            TerrainType::Void | TerrainType::Corrupted => f32::MAX,
            t => movement_cost(t),
        }
    }

    fn cost_for_terrain_vehicle(terrain: &TerrainType) -> f32 {
        match terrain {
            TerrainType::Void | TerrainType::Corrupted => f32::MAX,
            t if !vehicle_passable(t) => f32::MAX,
            t => movement_cost(t),
        }
    }

    /// Build a NavMesh from a tile map.
    pub fn build(width: i32, height: i32, tiles: &HashMap<(i32, i32), TerrainType>) -> Self {
        let size = (width * height) as usize;
        let mut infantry_cost = vec![f32::MAX; size];
        let mut vehicle_cost = vec![f32::MAX; size];

        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) as usize;
                let terrain = tiles.get(&(x, y)).unwrap_or(&TerrainType::Grass);
                infantry_cost[idx] = Self::cost_for_terrain_infantry(terrain);
                vehicle_cost[idx] = Self::cost_for_terrain_vehicle(terrain);
            }
        }

        NavMesh { width, height, infantry_cost, vehicle_cost }
    }

    /// Returns movement cost for (x,y) for the given unit kind.
    /// Returns f32::MAX if impassable or out of bounds.
    pub fn cost(&self, x: i32, y: i32, kind: &UnitKind) -> f32 {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return f32::MAX;
        }
        let idx = self.idx(x, y);
        match kind {
            UnitKind::Infantry => self.infantry_cost[idx],
            UnitKind::Vehicle => self.vehicle_cost[idx],
        }
    }

    /// Returns true if the tile is passable for the given unit kind.
    pub fn is_passable(&self, x: i32, y: i32, kind: &UnitKind) -> bool {
        self.cost(x, y, kind) < f32::MAX
    }

    /// Incrementally update a single tile in the navmesh (e.g. when a building is placed).
    pub fn set_tile(&mut self, x: i32, y: i32, terrain: &TerrainType) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let idx = self.idx(x, y);
        self.infantry_cost[idx] = Self::cost_for_terrain_infantry(terrain);
        self.vehicle_cost[idx] = Self::cost_for_terrain_vehicle(terrain);
    }

    /// Mark a tile as impassable for all unit kinds (e.g. building footprint).
    pub fn set_impassable(&mut self, x: i32, y: i32) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let idx = self.idx(x, y);
        self.infantry_cost[idx] = f32::MAX;
        self.vehicle_cost[idx] = f32::MAX;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grass_navmesh(w: i32, h: i32) -> NavMesh {
        let tiles = HashMap::new(); // defaults to Grass
        NavMesh::build(w, h, &tiles)
    }

    #[test]
    fn build_grass_map_passable() {
        let nav = grass_navmesh(5, 5);
        assert!(nav.is_passable(0, 0, &UnitKind::Infantry));
        assert!(nav.is_passable(4, 4, &UnitKind::Vehicle));
    }

    #[test]
    fn out_of_bounds_is_impassable() {
        let nav = grass_navmesh(5, 5);
        assert!(!nav.is_passable(-1, 0, &UnitKind::Infantry));
        assert!(!nav.is_passable(5, 0, &UnitKind::Infantry));
        assert!(!nav.is_passable(0, 5, &UnitKind::Vehicle));
    }

    #[test]
    fn void_tile_is_impassable() {
        let mut tiles = HashMap::new();
        tiles.insert((2, 2), TerrainType::Void);
        let nav = NavMesh::build(5, 5, &tiles);
        assert!(!nav.is_passable(2, 2, &UnitKind::Infantry));
        assert!(!nav.is_passable(2, 2, &UnitKind::Vehicle));
    }

    #[test]
    fn forest_impassable_for_vehicle() {
        let mut tiles = HashMap::new();
        tiles.insert((1, 1), TerrainType::Forest);
        let nav = NavMesh::build(5, 5, &tiles);
        assert!(nav.is_passable(1, 1, &UnitKind::Infantry));
        assert!(!nav.is_passable(1, 1, &UnitKind::Vehicle));
    }

    #[test]
    fn set_impassable_blocks_tile() {
        let mut nav = grass_navmesh(5, 5);
        assert!(nav.is_passable(3, 3, &UnitKind::Infantry));
        nav.set_impassable(3, 3);
        assert!(!nav.is_passable(3, 3, &UnitKind::Infantry));
    }

    #[test]
    fn set_tile_updates_cost() {
        let mut nav = grass_navmesh(5, 5);
        assert!(nav.is_passable(2, 2, &UnitKind::Vehicle));
        nav.set_tile(2, 2, &TerrainType::Forest);
        assert!(!nav.is_passable(2, 2, &UnitKind::Vehicle));
        assert!(nav.is_passable(2, 2, &UnitKind::Infantry));
    }
}
