// Procedural map generation per `world.md`.
// Mission type shapes the layout; chokepoints and base locations are
// authored constraints; everything else flows from a deterministic seed.
//
// Layout philosophy (2-player symmetric maps):
//   - Each base corner has a clear 5×5 Grass area.
//   - A contested central zone with Road/Rubble mix.
//   - 2-4 Forest/Rubble choke corridors between bases and center.
//   - Open Grass flanking fields for manoeuvring.
//   - Mud near river-like bands.
//   - 4-6 Corrupted resource nodes scattered mid-map and on flanks.
//   - Left half is generated, right half is mirrored for symmetry.
//
// Map sizes by mission:
//   - Defense / Survival : small  32×20
//   - Assault / Control  : medium 48×28
//   - Extraction         : large  64×36

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

// ---------------------------------------------------------------------------
// Deterministic RNG — linear congruential, no external dep.
// ---------------------------------------------------------------------------

fn lcg(seed: u64) -> impl FnMut() -> u64 {
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    move || {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        s
    }
}

/// Hash-based noise that returns a value in [0, 255] for grid coordinates.
/// Used to produce spatially coherent noise without any external crate.
fn hash_noise(x: i32, y: i32, seed: u64) -> u8 {
    let mut h = seed
        .wrapping_add(x as u64 * 2654435761)
        .wrapping_add(y as u64 * 2246822519);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51afd7ed558ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ceb9fe1a85ec53);
    h ^= h >> 33;
    (h & 0xff) as u8
}

// ---------------------------------------------------------------------------
// Terrain helpers
// ---------------------------------------------------------------------------

fn cover_for_terrain(t: &TerrainType) -> CoverDensity {
    match t {
        TerrainType::Forest | TerrainType::Rubble => CoverDensity::Heavy,
        TerrainType::Road | TerrainType::Grass => CoverDensity::None,
        _ => CoverDensity::Light,
    }
}

/// Map sizes keyed by mission type.
fn map_dims(mt: &MissionType) -> (i32, i32) {
    match mt {
        MissionType::Defense | MissionType::Survival => (32, 20),
        MissionType::Assault | MissionType::Control  => (48, 28),
        MissionType::Extraction                      => (64, 36),
    }
}

// ---------------------------------------------------------------------------
// Zone classification helpers
// ---------------------------------------------------------------------------

/// Is (x,y) inside the 5×5 base clearance zone for a given base corner?
fn in_base_zone(x: i32, y: i32, base: &GridPos) -> bool {
    (x - base.x).abs() <= 2 && (y - base.y).abs() <= 2
}

/// Distance from the map centre (Manhattan-ish, normalised to 0-1).
fn centre_dist_norm(x: i32, y: i32, width: i32, height: i32) -> f32 {
    let cx = width as f32 / 2.0;
    let cy = height as f32 / 2.0;
    let dx = (x as f32 - cx).abs() / cx;
    let dy = (y as f32 - cy).abs() / cy;
    (dx * dx + dy * dy).sqrt() / 2_f32.sqrt()
}

// ---------------------------------------------------------------------------
// Core generation
// ---------------------------------------------------------------------------

/// Generate a tile for the LEFT half of a symmetric map (x in 0..=half_w).
/// Returns the TerrainType for that position given noise, base zones, etc.
fn gen_left_half_tile(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    seed: u64,
    mission_type: &MissionType,
    bases: &[GridPos],
) -> TerrainType {
    let half_w = width / 2;

    // ---- Base clearance: always Grass ----
    for base in bases {
        if in_base_zone(x, y, base) {
            return TerrainType::Grass;
        }
    }

    // ---- Noise layers ----
    let n0 = hash_noise(x, y, seed) as f32 / 255.0;              // primary
    let n1 = hash_noise(x * 3 + 7, y * 5 + 13, seed ^ 0xdeadbeef) as f32 / 255.0; // secondary

    // Normalised coordinates (0-1)
    let nx = x as f32 / half_w as f32;         // 0 = left edge, 1 = centre
    let ny = y as f32 / height as f32;

    // ---- Mission-specific overrides ----
    match mission_type {
        MissionType::Survival => {
            // Outer rim is rubble arena wall.
            if x == 0 || y == 0 || y == height - 1 {
                return TerrainType::Rubble;
            }
        }
        MissionType::Defense => {
            // Clear central area for the single defender base.
            let cx = width / 2;
            let cy = height / 2;
            if (x - cx).abs().max((y - cy).abs()) <= 1 {
                return TerrainType::Grass;
            }
        }
        _ => {}
    }

    // ---- Central contested zone (within ~20% of centre column) ----
    // Roads and rubble near the midline, weighted by noise.
    if nx > 0.75 {
        if n0 < 0.30 {
            return TerrainType::Road;
        }
        if n0 < 0.55 {
            return TerrainType::Rubble;
        }
    }

    // ---- Choke corridors: 2-4 Forest/Rubble bands ----
    // We define choke corridors as diagonal-ish bands across y.
    // Corridor positions are derived from the seed so they vary per map.
    let choke_count = 2 + ((seed & 0x3) as i32).min(2); // 2-4
    for i in 0..choke_count {
        // Each corridor occupies a band of y-space.
        let band_frac = (i as f32 + 0.5) / (choke_count as f32 + 1.0);
        let band_y = (band_frac * height as f32) as i32;
        // Width of corridor shrinks toward map edges (narrower at flanks).
        let corridor_hw = if (nx * 100.0) as i32 % 2 == 0 { 1 } else { 2 };
        // Corridor is active in x range [half_w/2 .. half_w] (mid-map to centre).
        if x >= half_w / 2 && (y - band_y).abs() <= corridor_hw {
            // Alternate forest / rubble by corridor index.
            return if i % 2 == 0 {
                TerrainType::Forest
            } else {
                TerrainType::Rubble
            };
        }
    }

    // ---- Flanking forest strips (outer edges) ----
    // Forest corridors along top and bottom edges to create natural funnels.
    if ny < 0.18 || ny > 0.82 {
        if n0 < 0.55 {
            return TerrainType::Forest;
        }
    }

    // ---- Mud river bands (horizontal stripes in the mid-field) ----
    if ny > 0.35 && ny < 0.65 && n1 < 0.12 {
        return TerrainType::Mud;
    }

    // ---- Scattered Forest / Road noise across the field ----
    if n0 < 0.05 {
        return TerrainType::Road;
    }
    if n0 < 0.18 {
        return TerrainType::Forest;
    }
    if n0 < 0.22 {
        return TerrainType::Rubble;
    }

    // ---- Default: open grass field ----
    TerrainType::Grass
}

/// Place resource nodes (TerrainType::Corrupted) on the left half.
/// Returns a set of (x,y) positions that should be resource tiles.
fn resource_node_positions(
    width: i32,
    height: i32,
    seed: u64,
    _bases: &[GridPos],
) -> Vec<(i32, i32)> {
    let half_w = width / 2;
    // 4-6 resource nodes on the left half; mirroring doubles them on the full map.
    let count = 2 + ((seed >> 4) & 0x3) as usize; // 2-5 per half, mirrored = 4-10 total
    let count = count.min(3); // cap per-half at 3 → max 6 total
    let mut positions = Vec::with_capacity(count);
    let mut rng = lcg(seed ^ 0x1234567890abcdef);

    let mut attempts = 0usize;
    while positions.len() < count && attempts < 200 {
        attempts += 1;
        // Weight toward centre and flanks, not near the base corners (x < 4).
        let rx = 4 + (rng() % (half_w - 4) as u64) as i32;
        let ry = 2 + (rng() % (height - 4) as u64) as i32;

        // Keep at least 4 tiles away from other resource nodes.
        let too_close = positions.iter().any(|&(ox, oy): &(i32, i32)| {
            (rx - ox).abs().max((ry - oy).abs()) < 4
        });
        if !too_close {
            positions.push((rx, ry));
        }
    }
    positions
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn generate(width: i32, height: i32, seed: u64, mission_type: MissionType) -> GeneratedMap {
    let mut tiles = Vec::with_capacity((width * height) as usize);

    // Base positions — always 2 for symmetric maps, 1 for defense/survival.
    let bases: Vec<GridPos> = match mission_type {
        MissionType::Assault | MissionType::Control => vec![
            GridPos { x: 2, y: height / 2 },
            GridPos { x: width - 3, y: height / 2 },
        ],
        MissionType::Defense => vec![GridPos { x: width / 2, y: height / 2 }],
        MissionType::Extraction => vec![
            GridPos { x: 2, y: 2 },
            GridPos { x: width - 3, y: height - 3 },
        ],
        MissionType::Survival => vec![GridPos { x: width / 2, y: height / 2 }],
    };

    // Choke points: along the centre column at corridor y positions.
    let half_w = width / 2;
    let choke_count = (2 + ((seed & 0x3) as i32).min(2)) as usize;
    let chokepoints: Vec<GridPos> = (0..choke_count)
        .map(|i| {
            let band_frac = (i as f32 + 0.5) / (choke_count as f32 + 1.0);
            GridPos {
                x: half_w,
                y: (band_frac * height as f32) as i32,
            }
        })
        .collect();

    // Resource node positions on left half (will be mirrored).
    let resource_positions = resource_node_positions(width, height, seed, &bases);
    let resource_set: std::collections::HashSet<(i32, i32)> = resource_positions
        .iter()
        .cloned()
        .chain(resource_positions.iter().map(|&(x, y)| (width - 1 - x, y)))
        .collect();

    // For symmetric maps (2 bases): generate left half, mirror right half.
    // For single-base maps: generate the whole grid with mission-specific rules.
    let symmetric = matches!(
        mission_type,
        MissionType::Assault | MissionType::Control | MissionType::Extraction
    );

    for y in 0..height {
        for x in 0..width {
            let pos = GridPos { x, y };

            // Resource node override takes highest priority (after base zones).
            let is_resource = resource_set.contains(&(x, y))
                && !bases.iter().any(|b| in_base_zone(x, y, b));

            let terrain = if is_resource {
                TerrainType::Corrupted
            } else if symmetric {
                // Mirror: left half is canonical; right half is its reflection.
                let sample_x = if x <= half_w { x } else { width - 1 - x };
                gen_left_half_tile(
                    sample_x, y, width, height, seed, &mission_type, &bases,
                )
            } else {
                // Single-base missions: generate whole grid directly.
                gen_left_half_tile(x, y, width, height, seed, &mission_type, &bases)
            };

            let cover = cover_for_terrain(&terrain);
            tiles.push(GeneratedTile { pos, terrain, cover });
        }
    }

    GeneratedMap {
        width,
        height,
        tiles,
        bases,
        chokepoints,
        mission_type,
    }
}

/// Convenience: generate a map using canonical dimensions for a mission type.
pub fn generate_for_mission(seed: u64, mission_type: MissionType) -> GeneratedMap {
    let (w, h) = map_dims(&mission_type);
    generate(w, h, seed, mission_type)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_correct_tile_count() {
        let m = generate(10, 10, 42, MissionType::Assault);
        assert_eq!(m.tiles.len(), 100);
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
    fn has_at_least_one_chokepoint() {
        let m = generate(20, 20, 42, MissionType::Assault);
        assert!(!m.chokepoints.is_empty());
    }

    #[test]
    fn chokepoints_between_two_and_four() {
        for seed in [1u64, 42, 99, 777] {
            let m = generate(48, 28, seed, MissionType::Assault);
            assert!(
                m.chokepoints.len() >= 2 && m.chokepoints.len() <= 4,
                "seed {seed}: expected 2-4 chokepoints, got {}",
                m.chokepoints.len()
            );
        }
    }

    #[test]
    fn base_corners_are_grass() {
        let m = generate(48, 28, 42, MissionType::Assault);
        for base in &m.bases {
            for dy in -2..=2i32 {
                for dx in -2..=2i32 {
                    let tx = base.x + dx;
                    let ty = base.y + dy;
                    if tx >= 0 && ty >= 0 && tx < m.width && ty < m.height {
                        let tile = m.tiles.iter().find(|t| t.pos.x == tx && t.pos.y == ty).unwrap();
                        assert_eq!(
                            tile.terrain,
                            TerrainType::Grass,
                            "base zone ({tx},{ty}) should be Grass"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn symmetric_map_is_horizontally_mirrored() {
        let m = generate(48, 28, 42, MissionType::Control);
        // Check 50 random interior tiles are mirrored.
        for y in 2..26 {
            for x in 1..22 {
                let mirror_x = m.width - 1 - x;
                let t_left = m.tiles.iter().find(|t| t.pos.x == x && t.pos.y == y).unwrap();
                let t_right = m.tiles.iter().find(|t| t.pos.x == mirror_x && t.pos.y == y).unwrap();
                assert_eq!(
                    t_left.terrain, t_right.terrain,
                    "symmetry broken at ({x},{y}) vs ({mirror_x},{y})"
                );
            }
        }
    }

    #[test]
    fn resource_nodes_exist_on_map() {
        let m = generate(48, 28, 42, MissionType::Control);
        let corrupted: Vec<_> = m.tiles.iter().filter(|t| t.terrain == TerrainType::Corrupted).collect();
        assert!(
            corrupted.len() >= 4,
            "expected at least 4 resource nodes (Corrupted tiles), got {}",
            corrupted.len()
        );
    }

    #[test]
    fn map_dims_vary_by_mission() {
        assert_eq!(map_dims(&MissionType::Defense), (32, 20));
        assert_eq!(map_dims(&MissionType::Assault), (48, 28));
        assert_eq!(map_dims(&MissionType::Extraction), (64, 36));
    }

    #[test]
    fn generate_for_mission_uses_correct_dims() {
        let m = generate_for_mission(42, MissionType::Extraction);
        assert_eq!(m.width, 64);
        assert_eq!(m.height, 36);
        assert_eq!(m.tiles.len(), 64 * 36);
    }
}
