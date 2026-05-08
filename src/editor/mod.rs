// Level editor — pure helpers for serializing the current map to/from JSON
// and validating editor placements per `world.md`.

use crate::map::{TerrainType, CoverDensity};

#[derive(Debug, Clone, PartialEq)]
pub struct EditorTile {
    pub x: i32,
    pub y: i32,
    pub terrain: TerrainType,
    pub cover: CoverDensity,
}

pub fn parse_terrain(s: &str) -> Option<TerrainType> {
    match s {
        "Road" | "road" => Some(TerrainType::Road),
        "Grass" | "grass" => Some(TerrainType::Grass),
        "Forest" | "forest" => Some(TerrainType::Forest),
        "Rubble" | "rubble" => Some(TerrainType::Rubble),
        "Mud" | "mud" => Some(TerrainType::Mud),
        "Corrupted" | "corrupted" => Some(TerrainType::Corrupted),
        "Void" | "void" => Some(TerrainType::Void),
        _ => None,
    }
}

pub fn parse_cover(s: &str) -> Option<CoverDensity> {
    match s {
        "None" | "none" => Some(CoverDensity::None),
        "Light" | "light" => Some(CoverDensity::Light),
        "Heavy" | "heavy" => Some(CoverDensity::Heavy),
        _ => None,
    }
}

pub fn terrain_name(t: &TerrainType) -> &'static str {
    match t {
        TerrainType::Road => "Road",
        TerrainType::Grass => "Grass",
        TerrainType::Forest => "Forest",
        TerrainType::Rubble => "Rubble",
        TerrainType::Mud => "Mud",
        TerrainType::Corrupted => "Corrupted",
        TerrainType::Void => "Void",
    }
}

pub fn cover_name(c: &CoverDensity) -> &'static str {
    match c {
        CoverDensity::None => "None",
        CoverDensity::Light => "Light",
        CoverDensity::Heavy => "Heavy",
    }
}

/// Validate an editor map before save: per world.md, 3..=6 zones expected.
/// We approximate: width*height tile count between 3*3 and 60*60.
pub fn validate_map_dimensions(width: i32, height: i32) -> Result<(), String> {
    if width < 6 || height < 6 {
        return Err(format!("map too small: {width}x{height}"));
    }
    if width > 60 || height > 60 {
        return Err(format!("map too large: {width}x{height}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_terrain_roundtrips() {
        for t in &[
            TerrainType::Road,
            TerrainType::Grass,
            TerrainType::Forest,
            TerrainType::Rubble,
            TerrainType::Mud,
            TerrainType::Corrupted,
            TerrainType::Void,
        ] {
            let s = terrain_name(t);
            assert_eq!(parse_terrain(s), Some(t.clone()));
        }
    }

    #[test]
    fn parse_cover_roundtrips() {
        for c in &[CoverDensity::None, CoverDensity::Light, CoverDensity::Heavy] {
            let s = cover_name(c);
            assert_eq!(parse_cover(s), Some(c.clone()));
        }
    }

    #[test]
    fn parse_unknown_returns_none() {
        assert_eq!(parse_terrain("nonsense"), None);
        assert_eq!(parse_cover("nonsense"), None);
    }

    #[test]
    fn validate_dimensions_rejects_too_small() {
        assert!(validate_map_dimensions(2, 10).is_err());
    }

    #[test]
    fn validate_dimensions_rejects_too_large() {
        assert!(validate_map_dimensions(100, 100).is_err());
    }

    #[test]
    fn validate_dimensions_accepts_typical() {
        assert!(validate_map_dimensions(20, 20).is_ok());
    }
}
