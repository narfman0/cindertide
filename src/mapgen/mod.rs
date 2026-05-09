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
//   - Defense / Survival : small   80×52
//   - Assault / Control  : medium 128×80
//   - Extraction         : large  160×100

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
        MissionType::Defense | MissionType::Survival => (80, 52),
        MissionType::Assault | MissionType::Control  => (128, 80),
        MissionType::Extraction                      => (160, 100),
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
    let n2 = hash_noise(x * 7 + 3, y * 2 + 17, seed ^ 0xabcdef01) as f32 / 255.0; // tertiary

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

    // ---- Open killing ground: centre 20% of the half-width is mostly clear ----
    // This is the ~10% each side of the true centre — the pitched-battle zone.
    if nx > 0.80 {
        // A few roads and light rubble only; mostly clear grass.
        if n0 < 0.15 {
            return TerrainType::Road;
        }
        if n0 < 0.22 {
            return TerrainType::Rubble;
        }
        return TerrainType::Grass;
    }

    // ---- Winding river: diagonal band of Mud ~35-45% across the half-width ----
    // The river meanders: offset by noise to create a winding effect.
    // It crosses diagonally (x-based), 2-3 tiles wide.
    // Bridge crossings: Road tiles cut through the river at fixed intervals.
    {
        let river_cx = (half_w as f32 * 0.40) as i32; // river centreline x
        // Winding: shift x centre by a noise-based sine-like offset in y.
        let wiggle = ((hash_noise(0, y, seed ^ 0x11223344) as f32 / 255.0) - 0.5) * 6.0;
        let dist_from_river = (x as f32 - (river_cx as f32 + wiggle)).abs();
        if dist_from_river < 1.5 {
            // Bridge crossing: every ~(height/3) rows a Road cuts through.
            let bridge_interval = (height / 3).max(8);
            let in_bridge = (y % bridge_interval).abs() <= 1;
            if in_bridge {
                return TerrainType::Road;
            }
            return TerrainType::Mud;
        }
    }

    // ---- Industrial ruins: 2-4 clusters of 3×3 Rubble in mid-map ----
    // Cluster centres derived from seed, placed in x range [25%..75%] of half.
    {
        let ruin_count = 2 + ((seed >> 8) & 0x3) as i32; // 2-5; cap at 4
        let ruin_count = ruin_count.min(4);
        let mut rng_r = lcg(seed ^ 0xfeed0ff);
        for _ in 0..ruin_count {
            let raw = rng_r();
            let rcx = (half_w / 4) + (raw % (half_w / 2).max(1) as u64) as i32;
            let raw2 = rng_r();
            let rcy = (height / 4) + (raw2 % (height / 2).max(1) as u64) as i32;
            if (x - rcx).abs() <= 1 && (y - rcy).abs() <= 1 {
                // Hollow centre of ruins (cover, but not impassable), edges are rubble.
                if (x - rcx).abs() == 1 || (y - rcy).abs() == 1 {
                    return TerrainType::Rubble;
                }
                // Interior is rubble too (3×3 solid ruin).
                return TerrainType::Rubble;
            }
        }
    }

    // ---- Ridgelines: 1-2 diagonal bands of Forest acting as cover lines ----
    {
        let ridge_count = 1 + ((seed >> 12) & 0x1) as i32; // 1-2
        for i in 0..ridge_count {
            // Each ridge runs diagonally: y = slope * x + offset
            let offset_base = (height / (ridge_count + 1)) * (i + 1);
            let offset_noise = ((seed >> (i * 4)) & 0xf) as i32 - 8; // ±8 row jitter
            let ridge_y = offset_base + offset_noise;
            // Diagonal: tilt by ~0.5 tiles-per-x (gentle slope)
            let tilted_y = ridge_y + (x as f32 * 0.5) as i32;
            let ridge_hw = 1; // 2-tile wide band
            if (y - tilted_y).abs() <= ridge_hw && n2 < 0.80 {
                return TerrainType::Forest;
            }
        }
    }

    // ---- Central contested zone (within ~10-20% of centre column) ----
    // Roads and rubble near the midline, weighted by noise.
    if nx > 0.65 {
        if n0 < 0.30 {
            return TerrainType::Road;
        }
        if n0 < 0.50 {
            return TerrainType::Rubble;
        }
    }

    // ---- Choke corridors: Forest/Rubble bands spanning mid to centre ----
    // We define choke corridors as diagonal-ish bands across y.
    // Corridor positions are derived from the seed so they vary per map.
    // Scale corridor count the same as chokepoint count.
    let choke_count = {
        let base: i32 = match mission_type {
            MissionType::Defense | MissionType::Survival => 2,
            MissionType::Assault | MissionType::Control  => 3,
            MissionType::Extraction                      => 4,
        };
        (base + ((seed & 0x3) as i32).min(2)).max(2)
    };
    for i in 0..choke_count {
        // Each corridor occupies a band of y-space.
        let band_frac = (i as f32 + 0.5) / (choke_count as f32 + 1.0);
        let band_y = (band_frac * height as f32) as i32;
        // Width of corridor shrinks toward map edges (narrower at flanks).
        let corridor_hw = if (nx * 100.0) as i32 % 2 == 0 { 1 } else { 2 };
        // Corridor is active in x range [half_w/3 .. half_w*0.65] (mid-map to near centre).
        if x >= half_w / 3 && nx < 0.65 && (y - band_y).abs() <= corridor_hw {
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
    if ny < 0.15 || ny > 0.85 {
        if n0 < 0.55 {
            return TerrainType::Forest;
        }
    }

    // ---- Scattered Forest / Road noise across the field ----
    if n0 < 0.05 {
        return TerrainType::Road;
    }
    if n0 < 0.15 {
        return TerrainType::Forest;
    }
    if n0 < 0.19 {
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
    // 6-10 total resource nodes: 3-5 per half (mirrored).
    let count = 3 + ((seed >> 4) & 0x3) as usize; // 3-6 per half; cap at 5
    let count = count.min(5);
    let mut positions = Vec::with_capacity(count);
    let mut rng = lcg(seed ^ 0x1234567890abcdef);

    let mut attempts = 0usize;
    while positions.len() < count && attempts < 400 {
        attempts += 1;
        // Weight toward centre and flanks, not near the base corners (x < 6).
        let rx = 6 + (rng() % (half_w - 6).max(1) as u64) as i32;
        let ry = 3 + (rng() % (height - 6).max(1) as u64) as i32;

        // Keep at least 6 tiles away from other resource nodes (larger maps need spread).
        let too_close = positions.iter().any(|&(ox, oy): &(i32, i32)| {
            (rx - ox).abs().max((ry - oy).abs()) < 6
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
    // Scale choke count with map size: small 2-4, medium 3-5, large 4-6.
    let half_w = width / 2;
    let choke_base: i32 = match mission_type {
        MissionType::Defense | MissionType::Survival => 2,
        MissionType::Assault | MissionType::Control  => 3,
        MissionType::Extraction                      => 4,
    };
    let choke_count = (choke_base + ((seed & 0x3) as i32).min(2)) as usize;
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
    fn chokepoints_between_three_and_five() {
        for seed in [1u64, 42, 99, 777] {
            let m = generate(128, 80, seed, MissionType::Assault);
            assert!(
                m.chokepoints.len() >= 3 && m.chokepoints.len() <= 5,
                "seed {seed}: expected 3-5 chokepoints, got {}",
                m.chokepoints.len()
            );
        }
    }

    #[test]
    fn base_corners_are_grass() {
        let m = generate(128, 80, 42, MissionType::Assault);
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
        let m = generate(128, 80, 42, MissionType::Control);
        // Check interior tiles are mirrored.
        for y in 2..78 {
            for x in 1..62 {
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
        let m = generate(128, 80, 42, MissionType::Control);
        let corrupted: Vec<_> = m.tiles.iter().filter(|t| t.terrain == TerrainType::Corrupted).collect();
        assert!(
            corrupted.len() >= 6,
            "expected at least 6 resource nodes (Corrupted tiles), got {}",
            corrupted.len()
        );
    }

    #[test]
    fn map_dims_vary_by_mission() {
        assert_eq!(map_dims(&MissionType::Defense), (80, 52));
        assert_eq!(map_dims(&MissionType::Assault), (128, 80));
        assert_eq!(map_dims(&MissionType::Extraction), (160, 100));
    }

    #[test]
    fn generate_for_mission_uses_correct_dims() {
        let m = generate_for_mission(42, MissionType::Extraction);
        assert_eq!(m.width, 160);
        assert_eq!(m.height, 100);
        assert_eq!(m.tiles.len(), 160 * 100);
    }
}
