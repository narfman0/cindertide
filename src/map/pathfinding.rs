use std::collections::{BinaryHeap, HashMap, HashSet};
use std::cmp::Ordering;
use crate::map::{GridPos, TerrainType, movement_cost, vehicle_passable, NavMesh};

#[derive(Debug, Clone, PartialEq)]
pub enum UnitKind {
    Infantry,
    Vehicle,
}

pub struct PathfindingGrid {
    pub width: i32,
    pub height: i32,
    pub tiles: HashMap<(i32, i32), TerrainType>,
    pub unit_type: UnitKind,
    /// Tiles occupied by other units — treated as impassable (except the destination).
    pub occupied: std::collections::HashSet<(i32, i32)>,
    /// The destination tile is excluded from occupied blocking.
    pub destination: Option<(i32, i32)>,
}

// Node used in the A* priority queue (min-heap via Reverse ordering)
#[derive(Debug, Clone)]
struct Node {
    f: f32,
    g: f32,
    pos: GridPos,
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.f == other.f
    }
}
impl Eq for Node {}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        // min-heap: smaller f first
        other.f.partial_cmp(&self.f).unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn manhattan(a: &GridPos, b: &GridPos) -> f32 {
    ((a.x - b.x).abs() + (a.y - b.y).abs()) as f32
}

impl PathfindingGrid {
    /// Returns the terrain at a position, defaulting to Grass for unmapped tiles
    /// that are within bounds.
    fn terrain_at(&self, x: i32, y: i32) -> Option<&TerrainType> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return None;
        }
        Some(self.tiles.get(&(x, y)).unwrap_or(&TerrainType::Grass))
    }

    fn is_passable(&self, x: i32, y: i32) -> bool {
        match self.terrain_at(x, y) {
            None => false,
            Some(terrain) => {
                if *terrain == TerrainType::Void {
                    return false;
                }
                if *terrain == TerrainType::Corrupted {
                    return false;
                }
                if self.unit_type == UnitKind::Vehicle && !vehicle_passable(terrain) {
                    return false;
                }
                // Block occupied tiles unless this is the destination
                let key = (x, y);
                if self.occupied.contains(&key) {
                    if let Some(dest) = self.destination {
                        if key != dest {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                true
            }
        }
    }

    pub fn find_path(&self, from: GridPos, to: GridPos) -> Option<Vec<GridPos>> {
        // Trivial case
        if from == to {
            return Some(vec![from]);
        }

        if !self.is_passable(from.x, from.y) || !self.is_passable(to.x, to.y) {
            return None;
        }

        let mut open: BinaryHeap<Node> = BinaryHeap::new();
        let mut g_score: HashMap<(i32, i32), f32> = HashMap::new();
        let mut came_from: HashMap<(i32, i32), (i32, i32)> = HashMap::new();

        let start_key = (from.x, from.y);
        g_score.insert(start_key, 0.0);
        open.push(Node {
            f: manhattan(&from, &to),
            g: 0.0,
            pos: from.clone(),
        });

        // 8-directional neighbors: cardinals + diagonals
        let neighbors: [(i32, i32); 8] = [
            (0, 1), (0, -1), (1, 0), (-1, 0),
            (1, 1), (1, -1), (-1, 1), (-1, -1),
        ];

        while let Some(current) = open.pop() {
            let cur_key = (current.pos.x, current.pos.y);

            if current.pos == to {
                // Reconstruct path
                let mut path = Vec::new();
                let mut key = cur_key;
                loop {
                    path.push(GridPos { x: key.0, y: key.1 });
                    match came_from.get(&key) {
                        Some(&prev) => key = prev,
                        None => break,
                    }
                }
                path.reverse();
                return Some(path);
            }

            // Skip if we already found a better path to this node
            let best_g = *g_score.get(&cur_key).unwrap_or(&f32::MAX);
            if current.g > best_g {
                continue;
            }

            for &(dx, dy) in &neighbors {
                let nx = current.pos.x + dx;
                let ny = current.pos.y + dy;
                let is_diagonal = dx != 0 && dy != 0;

                // Corner-cutting prevention: both adjacent cardinals must be passable.
                if is_diagonal {
                    if !self.is_passable(current.pos.x + dx, current.pos.y)
                        || !self.is_passable(current.pos.x, current.pos.y + dy)
                    {
                        continue;
                    }
                }

                if !self.is_passable(nx, ny) {
                    continue;
                }
                let terrain = self.terrain_at(nx, ny).unwrap();
                let base_cost = movement_cost(terrain);
                let step_cost = if is_diagonal { base_cost * 1.414 } else { base_cost };
                let tentative_g = current.g + step_cost;
                let nb_key = (nx, ny);

                if tentative_g < *g_score.get(&nb_key).unwrap_or(&f32::MAX) {
                    g_score.insert(nb_key, tentative_g);
                    came_from.insert(nb_key, cur_key);
                    let h = manhattan(&GridPos { x: nx, y: ny }, &to);
                    open.push(Node {
                        f: tentative_g + h,
                        g: tentative_g,
                        pos: GridPos { x: nx, y: ny },
                    });
                }
            }
        }

        None
    }

    /// A* using a precomputed NavMesh instead of a HashMap tile lookup.
    /// `occupied` is the set of tiles blocked by other units; the destination
    /// is always treated as unblocked regardless of whether it appears there.
    pub fn find_path_on_navmesh(
        nav: &NavMesh,
        from: GridPos,
        to: GridPos,
        kind: &UnitKind,
        occupied: &HashSet<(i32, i32)>,
    ) -> Option<Vec<GridPos>> {
        if from == to {
            return Some(vec![from]);
        }

        if !nav.is_passable(from.x, from.y, kind) || !nav.is_passable(to.x, to.y, kind) {
            return None;
        }

        let dest_key = (to.x, to.y);

        let passable = |x: i32, y: i32| -> bool {
            if !nav.is_passable(x, y, kind) {
                return false;
            }
            let key = (x, y);
            if occupied.contains(&key) && key != dest_key {
                return false;
            }
            true
        };

        let mut open: BinaryHeap<Node> = BinaryHeap::new();
        let mut g_score: HashMap<(i32, i32), f32> = HashMap::new();
        let mut came_from: HashMap<(i32, i32), (i32, i32)> = HashMap::new();

        let start_key = (from.x, from.y);
        g_score.insert(start_key, 0.0);
        open.push(Node {
            f: manhattan(&from, &to),
            g: 0.0,
            pos: from,
        });

        let neighbors: [(i32, i32); 8] = [
            (0, 1), (0, -1), (1, 0), (-1, 0),
            (1, 1), (1, -1), (-1, 1), (-1, -1),
        ];

        while let Some(current) = open.pop() {
            let cur_key = (current.pos.x, current.pos.y);

            if current.pos == to {
                let mut path = Vec::new();
                let mut key = cur_key;
                loop {
                    path.push(GridPos { x: key.0, y: key.1 });
                    match came_from.get(&key) {
                        Some(&prev) => key = prev,
                        None => break,
                    }
                }
                path.reverse();
                return Some(path);
            }

            let best_g = *g_score.get(&cur_key).unwrap_or(&f32::MAX);
            if current.g > best_g {
                continue;
            }

            for &(dx, dy) in &neighbors {
                let nx = current.pos.x + dx;
                let ny = current.pos.y + dy;
                let is_diagonal = dx != 0 && dy != 0;

                // Prevent corner-cutting: both adjacent cardinal neighbors must be passable.
                if is_diagonal {
                    if !passable(current.pos.x + dx, current.pos.y)
                        || !passable(current.pos.x, current.pos.y + dy)
                    {
                        continue;
                    }
                }

                if !passable(nx, ny) {
                    continue;
                }

                let base_cost = nav.cost(nx, ny, kind);
                let step_cost = if is_diagonal { base_cost * 1.414 } else { base_cost };
                let tentative_g = current.g + step_cost;
                let nb_key = (nx, ny);

                if tentative_g < *g_score.get(&nb_key).unwrap_or(&f32::MAX) {
                    g_score.insert(nb_key, tentative_g);
                    came_from.insert(nb_key, cur_key);
                    let h = manhattan(&GridPos { x: nx, y: ny }, &to);
                    open.push(Node {
                        f: tentative_g + h,
                        g: tentative_g,
                        pos: GridPos { x: nx, y: ny },
                    });
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_grid(w: i32, h: i32) -> PathfindingGrid {
        PathfindingGrid {
            width: w,
            height: h,
            tiles: HashMap::new(), // all Grass by default
            unit_type: UnitKind::Infantry,
            occupied: std::collections::HashSet::new(),
            destination: None,
        }
    }

    #[test]
    fn test_straight_line_path() {
        let grid = open_grid(5, 5);
        let path = grid.find_path(GridPos { x: 0, y: 0 }, GridPos { x: 4, y: 0 }).unwrap();
        assert_eq!(path.first().unwrap(), &GridPos { x: 0, y: 0 });
        assert_eq!(path.last().unwrap(), &GridPos { x: 4, y: 0 });
        // Path length should be 5 tiles (0..=4)
        assert_eq!(path.len(), 5);
    }

    #[test]
    fn test_path_avoids_void_wall() {
        // Build a 5x3 grid with a wall of Void tiles at x=2 (y=0..=2), except y=2
        let mut tiles = HashMap::new();
        for y in 0..2 {
            tiles.insert((2, y), TerrainType::Void);
        }
        let grid = PathfindingGrid {
            width: 5,
            height: 3,
            tiles,
            unit_type: UnitKind::Infantry,
            occupied: std::collections::HashSet::new(),
            destination: None,
        };
        let path = grid.find_path(GridPos { x: 0, y: 0 }, GridPos { x: 4, y: 0 }).unwrap();
        // Path must not pass through any Void tile
        for pos in &path {
            assert_ne!(grid.tiles.get(&(pos.x, pos.y)), Some(&TerrainType::Void));
        }
        assert_eq!(path.last().unwrap(), &GridPos { x: 4, y: 0 });
    }

    #[test]
    fn test_vehicle_cannot_path_through_forest() {
        // 3x1 grid — only route is through Forest
        let mut tiles = HashMap::new();
        tiles.insert((1, 0), TerrainType::Forest);
        let grid = PathfindingGrid {
            width: 3,
            height: 1,
            tiles,
            unit_type: UnitKind::Vehicle,
            occupied: std::collections::HashSet::new(),
            destination: None,
        };
        let result = grid.find_path(GridPos { x: 0, y: 0 }, GridPos { x: 2, y: 0 });
        assert!(result.is_none());
    }

    #[test]
    fn test_no_path_when_fully_blocked() {
        // 3x1 grid blocked by Void in the middle
        let mut tiles = HashMap::new();
        tiles.insert((1, 0), TerrainType::Void);
        let grid = PathfindingGrid {
            width: 3,
            height: 1,
            tiles,
            unit_type: UnitKind::Infantry,
            occupied: std::collections::HashSet::new(),
            destination: None,
        };
        let result = grid.find_path(GridPos { x: 0, y: 0 }, GridPos { x: 2, y: 0 });
        assert!(result.is_none());
    }

    #[test]
    fn test_path_same_position() {
        let grid = open_grid(5, 5);
        let pos = GridPos { x: 2, y: 2 };
        let path = grid.find_path(pos.clone(), pos.clone()).unwrap();
        assert_eq!(path.len(), 1);
        assert_eq!(path[0], GridPos { x: 2, y: 2 });
    }
}
