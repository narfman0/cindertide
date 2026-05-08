// Procedural map generation per `world.md`.
// Mission type shapes the layout; chokepoints and base locations are
// authored constraints; everything else flows from a deterministic seed.

use crate::map::{TerrainType, CoverDensity, GridPos};

#[derive(Debug, Clone, PartialEq)]
pub enum MissionType {
    Assault,
    Control,
    Defense,
    Extraction,
    Survival,
}

#[derive(Debug, Clone)]
pub struct GeneratedTile {
    pub pos: GridPos,
    pub terrain: TerrainType,
    pub cover: CoverDensity,
}

#[derive(Debug, Clone)]
pub struct GeneratedMap {
    pub width: i32,
    pub height: i32,
    pub tiles: Vec<GeneratedTile>,
    pub bases: Vec<GridPos>,
    pub chokepoints: Vec<GridPos>,
    pub mission_type: MissionType,
}

/// Linear congruential RNG — deterministic, no external dep.
fn lcg(seed: u64) -> impl FnMut() -> u64 {
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    move || {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        s
    }
}

fn pick_terrain(rng: &mut impl FnMut() -> u64) -> TerrainType {
    match rng() % 100 {
        0..=4 => TerrainType::Road,
        5..=14 => TerrainType::Forest,
        15..=19 => TerrainType::Rubble,
        20..=22 => TerrainType::Mud,
        _ => TerrainType::Grass,
    }
}

fn cover_for_terrain(t: &TerrainType) -> CoverDensity {
    match t {
        TerrainType::Forest | TerrainType::Rubble => CoverDensity::Heavy,
        TerrainType::Road | TerrainType::Grass => CoverDensity::None,
        _ => CoverDensity::Light,
    }
}

pub fn generate(width: i32, height: i32, seed: u64, mission_type: MissionType) -> GeneratedMap {
    let mut rng = lcg(seed);
    let mut tiles = Vec::with_capacity((width * height) as usize);

    for y in 0..height {
        for x in 0..width {
            let pos = GridPos { x, y };
            let mut terrain = pick_terrain(&mut rng);

            // Mission-specific shaping.
            match mission_type {
                MissionType::Assault => {
                    // Linear: a road runs down the middle horizontally.
                    if y == height / 2 {
                        terrain = TerrainType::Road;
                    }
                }
                MissionType::Defense => {
                    // Radial: clear a small ring around the center for the
                    // defender's base.
                    let cx = width / 2;
                    let cy = height / 2;
                    let dx = (x - cx).abs();
                    let dy = (y - cy).abs();
                    if dx.max(dy) <= 1 {
                        terrain = TerrainType::Grass;
                    }
                }
                MissionType::Survival => {
                    // Compact arena: outer rim is rubble.
                    if x == 0 || y == 0 || x == width - 1 || y == height - 1 {
                        terrain = TerrainType::Rubble;
                    }
                }
                _ => {}
            }

            let cover = cover_for_terrain(&terrain);
            tiles.push(GeneratedTile { pos, terrain, cover });
        }
    }

    let bases = match mission_type {
        MissionType::Assault | MissionType::Control => vec![
            GridPos { x: 1, y: height / 2 },
            GridPos { x: width - 2, y: height / 2 },
        ],
        MissionType::Defense => vec![GridPos { x: width / 2, y: height / 2 }],
        MissionType::Extraction => vec![
            GridPos { x: 0, y: 0 },
            GridPos { x: width - 1, y: height - 1 },
        ],
        MissionType::Survival => vec![GridPos { x: width / 2, y: height / 2 }],
    };

    let chokepoints = vec![GridPos { x: width / 2, y: height / 2 }];

    GeneratedMap { width, height, tiles, bases, chokepoints, mission_type }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_correct_tile_count() {
        let m = generate(10, 10, 42, MissionType::Assault);
        assert_eq!(m.tiles.len(), 100);
    }

    #[test]
    fn assault_has_horizontal_road() {
        let m = generate(20, 10, 42, MissionType::Assault);
        // y = 5 should be all road.
        let mid_row: Vec<&GeneratedTile> = m.tiles.iter().filter(|t| t.pos.y == 5).collect();
        assert!(mid_row.iter().all(|t| t.terrain == TerrainType::Road));
    }

    #[test]
    fn assault_has_two_bases() {
        let m = generate(20, 10, 42, MissionType::Assault);
        assert_eq!(m.bases.len(), 2);
    }

    #[test]
    fn defense_has_one_central_base() {
        let m = generate(20, 20, 42, MissionType::Defense);
        assert_eq!(m.bases.len(), 1);
        assert_eq!(m.bases[0], GridPos { x: 10, y: 10 });
    }

    #[test]
    fn deterministic_with_same_seed() {
        let m1 = generate(10, 10, 42, MissionType::Control);
        let m2 = generate(10, 10, 42, MissionType::Control);
        for (a, b) in m1.tiles.iter().zip(m2.tiles.iter()) {
            assert_eq!(a.terrain, b.terrain);
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let m1 = generate(20, 20, 1, MissionType::Control);
        let m2 = generate(20, 20, 999, MissionType::Control);
        let diffs = m1
            .tiles
            .iter()
            .zip(m2.tiles.iter())
            .filter(|(a, b)| a.terrain != b.terrain)
            .count();
        assert!(diffs > 0, "expected divergence between seeds");
    }

    #[test]
    fn survival_has_rubble_rim() {
        let m = generate(10, 10, 42, MissionType::Survival);
        let top_row: Vec<&GeneratedTile> = m.tiles.iter().filter(|t| t.pos.y == 0).collect();
        assert!(top_row.iter().all(|t| t.terrain == TerrainType::Rubble));
    }

    #[test]
    fn always_one_chokepoint() {
        let m = generate(20, 20, 42, MissionType::Assault);
        assert_eq!(m.chokepoints.len(), 1);
    }
}
