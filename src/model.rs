use serde::{Deserialize, Serialize};

/// What kind of map this is. The kind picks the shape defaults in the shape
/// node and — as sections come back — which nodes the pipeline gets at all
/// (a forest has no buildings node, a cave has no props-on-the-way node, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MapKind {
    Town,
    Swamp,
    Forest,
    Cave,
}

impl MapKind {
    pub const ALL: [MapKind; 4] =
        [MapKind::Town, MapKind::Swamp, MapKind::Forest, MapKind::Cave];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            MapKind::Town => "Town",
            MapKind::Swamp => "Swamp",
            MapKind::Forest => "Forest",
            MapKind::Cave => "Cave",
        }
    }

    /// Lowercase stable id used in legends and spec.json.
    pub fn slug(self) -> &'static str {
        match self {
            MapKind::Town => "town",
            MapKind::Swamp => "swamp",
            MapKind::Forest => "forest",
            MapKind::Cave => "cave",
        }
    }
}

/// The app config. Each finished workflow section adds its fields back
/// (docs/workflow.md); `#[serde(default)]` keeps older config files loading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelConfig {
    pub map: MapSettings,
    /// What the map is: picks the shape defaults and the node workflow.
    pub kind: MapKind,
    /// The noise-derived region shape (step 1).
    pub shape: ShapeSettings,
    /// The outer wall ring around the region (step 2).
    pub outer_wall: OuterWallSettings,
    /// The region split of the town area (step 3).
    pub regions: RegionsSettings,
    pub cell_size_m: f32,
    pub seed: u64,
}

impl Default for LevelConfig {
    fn default() -> Self {
        Self {
            map: MapSettings::default(),
            kind: MapKind::Town,
            shape: ShapeSettings::for_kind(MapKind::Town),
            outer_wall: OuterWallSettings::default(),
            regions: RegionsSettings::default(),
            cell_size_m: 0.5,
            seed: 0,
        }
    }
}

/// Settings of the regions node (docs/workflow.md, step 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegionsSettings {
    /// How many regions the town area is split into.
    pub count: u32,
    /// Each region's share of the town area, %. The UI keeps the sum at 100
    /// (raising one lowers the rest); the generator normalizes anyway.
    pub shares: Vec<f32>,
    /// Chance, %, that a region starts deep inside the town instead of
    /// against the wall — the regions placed after it then grow around it,
    /// nesting it inside them. 0 = every region on the wall, 100 = every
    /// region nested.
    pub nesting_pct: f32,
}

impl Default for RegionsSettings {
    fn default() -> Self {
        Self {
            count: 3,
            shares: vec![20.0, 30.0, 50.0],
            nesting_pct: 35.0,
        }
    }
}

/// Settings of the outer wall node (docs/workflow.md, step 2).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OuterWallSettings {
    /// Shortest wall segment in meters: the wall is built point by point,
    /// each segment taking a random length between min and max while it walks
    /// the region border.
    pub segment_min_m: f32,
    /// Longest wall segment in meters. With min == max every segment has the
    /// same length and the loop closes with a chord that can pass inside the
    /// region.
    pub segment_max_m: f32,
    /// Wall thickness in meters, quantized to whole grid cells (rounded).
    /// 0.5 m is a one-cell wall at the default cell size.
    pub thickness_m: f32,
}

impl Default for OuterWallSettings {
    fn default() -> Self {
        Self {
            segment_min_m: 4.0,
            segment_max_m: 10.0,
            thickness_m: 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapSettings {
    pub width_m: f32,
    pub height_m: f32,
    /// Share of the map that must remain walkable; governs the occupancy
    /// budget of everything the generator places.
    pub walkable_pct: f32,
}

impl Default for MapSettings {
    fn default() -> Self {
        Self {
            width_m: 64.0,
            height_m: 64.0,
            walkable_pct: 60.0,
        }
    }
}

/// Settings of the noise-derived region shape (docs/workflow.md, step 1).
///
/// Two characters, one pipeline:
/// - **Town** ("cut the rest"): the cut keeps MOST of the buildable area, so
///   the map starts full and the texture erodes the edges and rare interior
///   spots — the center stays solid.
/// - **Swamp / Forest / Cave** ("islands"): a lower cut scatters organic
///   blobs that get bridged into one connected shape.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShapeSettings {
    /// Feature size of the noise in meters: bigger = fewer, larger blobs.
    pub noise_scale_m: f32,
    /// Noise layers stacked on top of each other; more = more detailed edges.
    pub octaves: u32,
    /// Share of the buildable area the region should cover. The cut level in
    /// the noise is picked automatically so this share survives the cut.
    pub coverage_target_pct: f32,
    /// Pieces smaller than this many cells are dropped before bridging.
    pub min_blob_cells: u32,
    /// Width of the corridors bridging the pieces together, in cells.
    pub bridge_width_cells: u32,
    /// Distance the region keeps from the map border, in meters, cut with a
    /// meandering noise line. 0 = the region may touch the border.
    pub border_margin_m: f32,
}

impl ShapeSettings {
    /// Starting points per map kind. Town keeps most of the map (cut-the-rest
    /// character); the island kinds scatter blobs of different sizes and
    /// connectedness.
    pub fn for_kind(kind: MapKind) -> Self {
        let (scale, octaves, coverage, min_blob, bridge, margin) = match kind {
            MapKind::Town => (24.0, 4, 85.0, 4, 2, 6.0),
            MapKind::Swamp => (18.0, 4, 35.0, 4, 2, 2.0),
            MapKind::Forest => (12.0, 5, 50.0, 6, 2, 4.0),
            MapKind::Cave => (16.0, 4, 30.0, 3, 1, 8.0),
        };
        Self {
            noise_scale_m: scale,
            octaves,
            coverage_target_pct: coverage,
            min_blob_cells: min_blob,
            bridge_width_cells: bridge,
            border_margin_m: margin,
        }
    }
}

impl Default for ShapeSettings {
    fn default() -> Self {
        Self::for_kind(MapKind::Town)
    }
}

/// A validation warning; `field` names the offending input when there is one.
#[derive(Debug, Clone, PartialEq)]
pub struct Warning {
    pub field: Option<String>,
    pub message: String,
    /// Blocking warnings prevent generation until resolved.
    pub blocking: bool,
}

impl Warning {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            field: None,
            message: message.into(),
            blocking: false,
        }
    }

    pub fn blocking(message: impl Into<String>) -> Self {
        Self {
            field: None,
            message: message.into(),
            blocking: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_json_round_trip() {
        let config = LevelConfig::default();
        let json = serde_json::to_string_pretty(&config).unwrap();
        let back: LevelConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, back);
    }

    #[test]
    fn old_config_files_load_with_defaults() {
        // A config saved by an older build (with removed fields) must still load.
        let old = r#"{
            "map": {"width_m": 128.0, "height_m": 96.0, "walkable_pct": 60.0},
            "cell_size_m": 0.5,
            "seed": 7,
            "structures": {},
            "mobs": {"budget": 100.0}
        }"#;
        let config: LevelConfig = serde_json::from_str(old).unwrap();
        assert_eq!(config.map.width_m, 128.0);
        assert_eq!(config.seed, 7);
        assert_eq!(config.kind, MapKind::Town);
    }

    #[test]
    fn kind_serializes_lowercase_and_picks_distinct_defaults() {
        assert_eq!(serde_json::to_string(&MapKind::Cave).unwrap(), "\"cave\"");
        assert_eq!(MapKind::Swamp.label(), "Swamp");
        assert_eq!(MapKind::Swamp.slug(), "swamp");
        let town = ShapeSettings::for_kind(MapKind::Town);
        let swamp = ShapeSettings::for_kind(MapKind::Swamp);
        // Town cuts the rest away (keeps most of the map); islands scatter.
        assert!(town.coverage_target_pct > swamp.coverage_target_pct);
    }
}
