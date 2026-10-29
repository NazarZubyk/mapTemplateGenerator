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
    /// The road network: crossroad nodes and the links between them (step 4).
    pub roads: RoadsSettings,
    /// Walls along the borders between regions, open where roads cross (step 5).
    pub region_walls: RegionWallSettings,
    /// Houses along the streets and in the yards (step 6, town maps).
    pub houses: HouseSettings,
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
            roads: RoadsSettings::default(),
            region_walls: RegionWallSettings::default(),
            houses: HouseSettings::default(),
            cell_size_m: 0.5,
            seed: 0,
        }
    }
}

/// Settings of the houses node (docs/workflow.md, step 6; town maps).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HouseSettings {
    /// Share of the buildable ground (town minus walls and roads) covered by
    /// houses, %. Street fronts fill first, the yards behind them after.
    pub density_pct: f32,
    /// Smallest house side in meters (width and depth are rolled
    /// independently between min and max).
    pub size_min_m: f32,
    /// Largest house side in meters.
    pub size_max_m: f32,
    /// Free ground kept around every house (to other houses, walls and the
    /// town edge), meters — at least one cell, so no pocket is ever sealed.
    pub spacing_m: f32,
    /// Gap between a street-front house and the road edge, meters.
    pub setback_m: f32,
    /// Chance, %, that a yard house turns to face the nearest road; the rest
    /// get a random angle (organic old-town look).
    pub face_road_pct: f32,
    /// Share of houses, %, built from more than one rectangle (a main block
    /// plus overlapping wings → L, T, U, Z and cross shapes).
    pub complex_pct: f32,
    /// Most rectangles in one complex house (2 or 3).
    pub max_parts: u32,
    /// Wing size as a share of the main block's side, % (min..max): how far
    /// along the main block a wing runs and how far it sticks out.
    pub wing_min_pct: f32,
    pub wing_max_pct: f32,
    /// House color on map.png, the preview and the legend.
    pub color: [u8; 3],
}

impl Default for HouseSettings {
    fn default() -> Self {
        Self {
            density_pct: 25.0,
            size_min_m: 4.0,
            size_max_m: 9.0,
            spacing_m: 1.0,
            setback_m: 0.5,
            face_road_pct: 80.0,
            complex_pct: 35.0,
            max_parts: 3,
            wing_min_pct: 40.0,
            wing_max_pct: 75.0,
            color: [158, 96, 64],
        }
    }
}

/// Settings of the region walls node (docs/workflow.md, step 5): built like
/// the outer wall, but along the borders between regions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegionWallSettings {
    /// Shortest wall segment in meters (the border is walked point by point,
    /// each segment a random length between min and max).
    pub segment_min_m: f32,
    /// Longest wall segment in meters.
    pub segment_max_m: f32,
    /// Wall thickness in meters, quantized to whole grid cells (rounded).
    pub thickness_m: f32,
    /// Extra clearance on each side of a road where it passes through a
    /// wall, in meters: the gap is the road width plus twice this.
    pub gap_margin_m: f32,
}

impl Default for RegionWallSettings {
    fn default() -> Self {
        Self {
            segment_min_m: 3.0,
            segment_max_m: 8.0,
            thickness_m: 0.5,
            gap_margin_m: 0.5,
        }
    }
}

/// How the number of crossroad nodes is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeAmount {
    /// A fixed total number of nodes (`RoadsSettings::node_count`).
    Count,
    /// Nodes per 1000 m² of town area (`RoadsSettings::nodes_per_1000m2`).
    Density,
}

/// Settings of the roads node (docs/workflow.md, step 4).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoadsSettings {
    /// Whether the node amount is a total count or a density.
    pub amount: NodeAmount,
    /// Total crossroad nodes (Count mode). Never fewer than the region count:
    /// every region gets at least one node.
    pub node_count: u32,
    /// Crossroad nodes per 1000 m² of town area (Density mode). Also never
    /// fewer than one per region.
    pub nodes_per_1000m2: f32,
    /// Width of the main roads in meters — the widest a road gets.
    pub width_m: f32,
    /// Width of the quietest roads (dead-end alleys) in meters. Roads in
    /// between get wider with the traffic they carry. Equal to `width_m` =
    /// every road the same width.
    pub min_width_m: f32,
    /// Share of the roads, %, that count as main roads (the busiest ones) and
    /// get the full `width_m`.
    pub main_roads_pct: f32,
    /// How much links bend away from the straight line, %. Kept gentle: 100%
    /// is a lazy meander, not a maze.
    pub wiggle_pct: f32,
    /// Chance, %, that each candidate link beyond the minimum network is
    /// added — loops and alternative routes. 0 = a tree (one route between
    /// any two nodes).
    pub extra_links_pct: f32,
}

impl Default for RoadsSettings {
    fn default() -> Self {
        Self {
            amount: NodeAmount::Count,
            node_count: 8,
            nodes_per_1000m2: 3.0,
            width_m: 2.5,
            min_width_m: 1.0,
            main_roads_pct: 30.0,
            wiggle_pct: 35.0,
            extra_links_pct: 35.0,
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
    /// How irregular region shapes are, %: 0 = compact round blobs; higher
    /// stretches each region along a random direction and lets a strong
    /// noise field carve lobes, bays and fingers into its border.
    pub irregularity_pct: f32,
}

impl Default for RegionsSettings {
    fn default() -> Self {
        Self {
            count: 3,
            shares: vec![20.0, 30.0, 50.0],
            nesting_pct: 35.0,
            irregularity_pct: 60.0,
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
    /// Domain warp, %: the noise is sampled at coordinates bent by a second
    /// noise field — swirling, flowing forms instead of plain noise blobs.
    pub warp_pct: f32,
    /// Silhouette pull, %: the field is lowered with distance from a randomly
    /// placed, stretched and rotated center — the region gathers into one
    /// tilted organic mass instead of filling the map rectangle.
    pub silhouette_pct: f32,
    /// Bridge wander, %: bridges follow the valleys of a noise field instead
    /// of the straight shortest line.
    pub bridge_wander_pct: f32,
    /// Treat feature size, border margin, min blob, bridge width and the
    /// section gap / bridge widths as values
    /// for a 64 m map and scale them with the actual map size, so a bigger
    /// map gets the same shape character, only larger.
    pub scale_with_map: bool,
    /// How many sections (islands) the shape is built from. Each section is
    /// its own organic mass, separated from the others by a channel and
    /// joined to them by wide bridges. 1 = one mass (no channels).
    pub sections: u32,
    /// Width of the channel between sections, in meters.
    pub section_gap_m: f32,
    /// Width of the bridges joining sections, in meters.
    pub section_bridge_m: f32,
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
        let (warp, silhouette, wander) = match kind {
            MapKind::Town => (45.0, 70.0, 60.0),
            MapKind::Swamp => (60.0, 35.0, 70.0),
            MapKind::Forest => (40.0, 25.0, 60.0),
            MapKind::Cave => (70.0, 45.0, 80.0),
        };
        Self {
            noise_scale_m: scale,
            octaves,
            coverage_target_pct: coverage,
            min_blob_cells: min_blob,
            bridge_width_cells: bridge,
            border_margin_m: margin,
            warp_pct: warp,
            silhouette_pct: silhouette,
            bridge_wander_pct: wander,
            scale_with_map: true,
            sections: 1,
            section_gap_m: 10.0,
            section_bridge_m: 4.0,
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
