/// seed_maps: generate deterministic terrain for all campaign mission maps.
///
/// For each map TOML with `tiles = []`, picks an archetype based on the
/// mission's narrative context, generates terrain with a fixed seed, and
/// writes the tile array back into the TOML file in-place.
///
/// Run: cargo run --bin seed_maps
///
/// After generation, hand-edit individual maps as needed. Re-running this
/// tool overwrites existing tiles — guard with the `--dry-run` flag to preview.

use std::collections::HashMap;
use std::path::Path;

use cindertide::mapgen::{
    archetype::{generate_from_archetype, scan_archetypes, ArchetypeDef},
    MissionType,
};

// ---------------------------------------------------------------------------
// Archetype assignment
// ---------------------------------------------------------------------------

struct MapSeed {
    path: &'static str,
    archetype: &'static str,
    seed: u64,
    mission_type: MissionType,
}

fn map_seeds() -> Vec<MapSeed> {
    vec![
        // ── Combine: The Iron Pact ───────────────────────────────────────────
        MapSeed { path: "assets/maps/combine_m0.toml", archetype: "industrial-complex", seed: 1001, mission_type: MissionType::Control },
        MapSeed { path: "assets/maps/combine_m1.toml", archetype: "choke-point",        seed: 1002, mission_type: MissionType::Assault },
        MapSeed { path: "assets/maps/combine_m2.toml", archetype: "fortress-valley",    seed: 1003, mission_type: MissionType::Defense },
        MapSeed { path: "assets/maps/combine_m3.toml", archetype: "urban-ruin",         seed: 1004, mission_type: MissionType::Assault },
        MapSeed { path: "assets/maps/combine_m4.toml", archetype: "highland-passes",    seed: 1005, mission_type: MissionType::Assault },
        // ── Ironborn: Forged in Ash ──────────────────────────────────────────
        MapSeed { path: "assets/maps/ironborn_m0.toml", archetype: "fortress-valley",   seed: 2001, mission_type: MissionType::Defense },
        MapSeed { path: "assets/maps/ironborn_m1.toml", archetype: "choke-point",       seed: 2002, mission_type: MissionType::Assault },
        MapSeed { path: "assets/maps/ironborn_m2.toml", archetype: "urban-ruin",        seed: 2003, mission_type: MissionType::Assault },
        MapSeed { path: "assets/maps/ironborn_m3.toml", archetype: "river-crossing",    seed: 2004, mission_type: MissionType::Extraction },
        MapSeed { path: "assets/maps/ironborn_m4.toml", archetype: "open-steppe",       seed: 2005, mission_type: MissionType::Assault },
        // ── Architect: The Long Design ───────────────────────────────────────
        MapSeed { path: "assets/maps/architect_m0.toml", archetype: "island-chain",     seed: 3001, mission_type: MissionType::Survival },
        MapSeed { path: "assets/maps/architect_m1.toml", archetype: "industrial-complex", seed: 3002, mission_type: MissionType::Control },
        MapSeed { path: "assets/maps/architect_m2.toml", archetype: "highland-passes",  seed: 3003, mission_type: MissionType::Assault },
        MapSeed { path: "assets/maps/architect_m3.toml", archetype: "river-crossing",   seed: 3004, mission_type: MissionType::Survival },
        MapSeed { path: "assets/maps/architect_m4.toml", archetype: "urban-ruin",       seed: 3005, mission_type: MissionType::Assault },
    ]
}

// ---------------------------------------------------------------------------
// Map dimensions (mirror mapgen::map_dims)
// ---------------------------------------------------------------------------

fn map_dims(mt: &MissionType) -> (i32, i32) {
    match mt {
        MissionType::Defense | MissionType::Survival => (80, 52),
        MissionType::Extraction                      => (160, 100),
        _                                            => (128, 80),
    }
}

// ---------------------------------------------------------------------------
// TOML tile serialisation
// ---------------------------------------------------------------------------

/// Single-char terrain code for the compact grid format. Mirrors the legend
/// in the loader (`client.rs::grid_char_to_terrain`).
fn terrain_char(t: &cindertide::map::TerrainType) -> char {
    use cindertide::map::TerrainType;
    match t {
        TerrainType::Grass     => '.',
        TerrainType::Road      => 'R',
        TerrainType::Forest    => 'F',
        TerrainType::Rubble    => 'U',
        TerrainType::Mud       => 'M',
        TerrainType::Corrupted => 'C',
        TerrainType::Void      => '.', // legacy: render as grass
        _                      => '.',
    }
}

/// Inject (or replace) the tile block in a TOML string. Recognises both the
/// legacy `tiles = [...]` form and the new `[grid]` table, plus the empty
/// `tiles = []` sentinel used in fresh map skeletons.
fn splice_tiles(original: &str, block: &str) -> String {
    let mut out = original.to_string();

    // Sentinel: a brand-new map file with `tiles = []` on a single line.
    if let Some(pos) = out.find("tiles = []") {
        out.replace_range(pos..pos + "tiles = []".len(), block);
        return out;
    }

    // Existing multi-line `tiles = [ ... ]`.
    if let Some(start) = out.find("tiles = [") {
        let bracket_open = start + "tiles = [".len() - 1;
        let bytes = out.as_bytes();
        let mut depth = 0usize;
        let mut end = bracket_open;
        for i in bracket_open..bytes.len() {
            match bytes[i] {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.replace_range(start..=end, block);
        return out;
    }

    // Existing `[grid]` table. Find its `rows = [ ... ]` closing bracket.
    if let Some(start) = out.find("[grid]") {
        if let Some(rows_pos) = out[start..].find("rows = [").map(|i| start + i) {
            let bracket_open = rows_pos + "rows = [".len() - 1;
            let bytes = out.as_bytes();
            let mut depth = 0usize;
            let mut end = bracket_open;
            for i in bracket_open..bytes.len() {
                match bytes[i] {
                    b'[' => depth += 1,
                    b']' => {
                        depth -= 1;
                        if depth == 0 {
                            end = i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            out.replace_range(start..=end, block);
            return out;
        }
    }

    // No existing block found — prepend.
    format!("{}\n{}", block, out)
}

fn build_grid_toml(
    tiles: &HashMap<(i32, i32), cindertide::map::TerrainType>,
    width: i32,
    height: i32,
) -> String {
    let mut rows: Vec<String> = Vec::with_capacity(height as usize);
    for y in 0..height {
        let mut row = String::with_capacity(width as usize);
        for x in 0..width {
            let ch = tiles
                .get(&(x, y))
                .map(terrain_char)
                .unwrap_or('.');
            row.push(ch);
        }
        // Trim trailing default grass to keep rows tight; loader treats
        // missing tiles as absent (but we want the full grid for now —
        // mapgen emits a full rectangle, so don't trim).
        rows.push(row);
    }

    let mut out = String::from("[grid]\nrows = [\n");
    for row in &rows {
        out.push_str("  \"");
        out.push_str(row);
        out.push_str("\",\n");
    }
    out.push(']');
    out
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let dry_run = std::env::args().any(|a| a == "--dry-run");

    let archetypes = scan_archetypes();
    let archetype_map: HashMap<&str, &ArchetypeDef> =
        archetypes.iter().map(|a| (a.id.as_str(), a)).collect();

    for ms in &map_seeds() {
        let archetype_id = ms.archetype;
        let Some(def) = archetype_map.get(archetype_id) else {
            eprintln!("WARN: archetype '{}' not found, skipping {}", archetype_id, ms.path);
            continue;
        };

        let (w, h) = map_dims(&ms.mission_type);
        let generated = generate_from_archetype(def, &HashMap::new(), w, h, ms.seed);

        let grid_toml = build_grid_toml(&generated.tiles, w, h);

        if dry_run {
            println!("=== {} ({} tiles, archetype={}, seed={}) ===",
                ms.path, generated.tiles.len(), archetype_id, ms.seed);
            println!("{}", &grid_toml[..grid_toml.len().min(400)]);
            println!("...\n");
            continue;
        }

        let original = match std::fs::read_to_string(ms.path) {
            Ok(s) => s,
            Err(e) => { eprintln!("ERROR reading {}: {e}", ms.path); continue; }
        };

        let updated = splice_tiles(&original, &grid_toml);

        match std::fs::write(ms.path, &updated) {
            Ok(_) => println!("OK  {} ({} tiles, archetype={}, seed={})",
                ms.path, generated.tiles.len(), archetype_id, ms.seed),
            Err(e) => eprintln!("ERROR writing {}: {e}", ms.path),
        }
    }

    if dry_run {
        println!("(dry-run — no files written)");
    } else {
        println!("\nDone. Re-run with --dry-run to preview without writing.");
    }
}
