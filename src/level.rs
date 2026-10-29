use serde::{Deserialize, Serialize};

use crate::model::LevelConfig;

/// Object category — doubles as the layer id in the exported layer PNG. The
/// category set grows back as the generator sections land (docs/workflow.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    Structure,
    Prop,
    Interactive,
    HiddenRoom,
    HiddenRoomPath,
    MobSpawn,
    Gate,
}

impl Category {
    /// Stable id used in the machine-readable layer PNG (R channel).
    pub fn layer_id(self) -> u8 {
        match self {
            Category::Structure => 1,
            Category::Prop => 2,
            Category::Interactive => 3,
            Category::HiddenRoom => 4,
            Category::HiddenRoomPath => 5,
            Category::MobSpawn => 6,
            Category::Gate => 7,
        }
    }

    pub const ALL: [Category; 7] = [
        Category::Structure,
        Category::Prop,
        Category::Interactive,
        Category::HiddenRoom,
        Category::HiddenRoomPath,
        Category::MobSpawn,
        Category::Gate,
    ];
}

/// One placed object: a rect in meters, rotated by `angle_deg` around its
/// center. (x, y) is the top-left corner of the UNROTATED rect, origin at the
/// map's top-left, +y pointing down the map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacedObject {
    pub category: Category,
    pub type_name: String,
    /// RGB color used on map.png and in the legend.
    pub color: [u8; 3],
    pub x_m: f32,
    pub y_m: f32,
    pub w_m: f32,
    pub h_m: f32,
    /// Clockwise rotation in degrees around the rect center (screen +y down).
    #[serde(default)]
    pub angle_deg: f32,
}

impl PlacedObject {
    pub fn center(&self) -> (f32, f32) {
        (self.x_m + self.w_m / 2.0, self.y_m + self.h_m / 2.0)
    }

    /// Is the point (meters) inside the rotated rect?
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let (cx, cy) = self.center();
        let (s, c) = self.angle_deg.to_radians().sin_cos();
        let (dx, dy) = (x - cx, y - cy);
        let (u, v) = (dx * c + dy * s, dy * c - dx * s);
        u.abs() <= self.w_m / 2.0 && v.abs() <= self.h_m / 2.0
    }

    /// The four corners of the rotated rect, in meters, clockwise on screen.
    pub fn corners(&self) -> [(f32, f32); 4] {
        let (cx, cy) = self.center();
        let (s, c) = self.angle_deg.to_radians().sin_cos();
        let (hw, hh) = (self.w_m / 2.0, self.h_m / 2.0);
        [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)].map(|(u, v)| (cx + u * c - v * s, cy + u * s + v * c))
    }
}

/// A boolean grid aligned to the level grid: `cells[y * width + x]`. The
/// region shape lives in one of these; placement flags will reuse the same
/// type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellMask {
    pub width: usize,
    pub height: usize,
    pub cell_size_m: f32,
    pub cells: Vec<bool>,
}

impl CellMask {
    pub fn new(width: usize, height: usize, cell_size_m: f32) -> Self {
        Self {
            width,
            height,
            cell_size_m,
            cells: vec![false; width * height],
        }
    }

    pub fn get(&self, x: usize, y: usize) -> bool {
        self.cells[y * self.width + x]
    }

    pub fn set(&mut self, x: usize, y: usize, value: bool) {
        self.cells[y * self.width + x] = value;
    }

    pub fn set_if_in_bounds(&mut self, x: i64, y: i64, value: bool) {
        if x >= 0 && y >= 0 {
            let (x, y) = (x as usize, y as usize);
            if x < self.width && y < self.height {
                self.set(x, y, value);
            }
        }
    }

    pub fn count_true(&self) -> usize {
        self.cells.iter().filter(|c| **c).count()
    }
}

/// Grayscale snapshot of a noise field, one value per cell (0 = black,
/// 255 = white). Bench preview data, not part of the exported bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoiseTexture {
    pub width: usize,
    pub height: usize,
    pub cell_size_m: f32,
    pub values: Vec<u8>,
}

/// What the shape phase did; shown in the workflow bench.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShapeStats {
    /// Connected pieces right after the threshold cut.
    pub raw_blobs: u32,
    /// Cells dropped by the minimum-blob-size filter.
    pub speckle_cells_removed: u32,
    /// Bridges carved to join the pieces into one shape.
    pub bridges: u32,
    /// Cells claimed by filling fully enclosed holes.
    pub hole_cells_filled: u32,
    /// Sections (islands) the shape was built from.
    #[serde(default)]
    pub sections: u32,
    /// Wide bridges carved to join the sections.
    #[serde(default)]
    pub section_bridges: u32,
    /// Share of the map covered by the final shape, 0..100.
    pub coverage_pct: f32,
}

/// The generated region footprint (town/swamp/forest/cave): one connected
/// blob of cells (or empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelShape {
    pub mask: CellMask,
    pub stats: ShapeStats,
}

/// What the outer-wall phase did; shown in the workflow bench.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OuterWallStats {
    /// Wall points placed by the border walk.
    pub points: u32,
    /// The loop closed on itself.
    pub closed: bool,
    /// Boundary loops discarded beyond the outer one (rare pinch artifacts).
    pub extra_loops: u32,
    /// Cells occupied by the wall ring.
    pub wall_cells: u32,
    /// Region cells removed because they ended up outside the wall.
    pub area_cut_cells: u32,
    /// Non-region cells filled because they ended up inside the wall.
    pub area_filled_cells: u32,
}

/// The outer wall ring around the region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OuterWall {
    pub mask: CellMask,
    pub stats: OuterWallStats,
}

/// One id per cell of the town area: 0 = outside the town / wall, 1..=count
/// = region id. Same grid as the region shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegionMap {
    pub width: usize,
    pub height: usize,
    pub cell_size_m: f32,
    pub ids: Vec<u8>,
}

/// Per-region result of the split; shown in the workflow bench.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RegionStat {
    /// Region id, 1-based.
    pub id: u8,
    /// Configured share of the town area, %.
    pub target_pct: f32,
    /// Actually claimed share after the split, %.
    pub actual_pct: f32,
    pub cells: u32,
}

/// The region split of the town area.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegionSplit {
    pub map: RegionMap,
    /// Stats ordered by region id.
    pub stats: Vec<RegionStat>,
}

/// A crossroad node of the road network.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RoadNode {
    /// Cell coordinates on the level grid.
    pub x: usize,
    pub y: usize,
    /// Region the node stands in (1-based).
    pub region: u8,
}

/// A road between two nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoadLink {
    /// Node indices.
    pub a: usize,
    pub b: usize,
    /// Centerline, as cell indices from `a` to `b` (8-connected steps).
    pub path: Vec<usize>,
    /// Added beyond the minimum network (a loop / alternative route).
    pub extra: bool,
}

/// Where a road crosses from one region into another: the future gap in the
/// wall between those regions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Passage {
    /// The first cell on the `to_region` side, in cell coordinates.
    pub x: usize,
    pub y: usize,
    pub from_region: u8,
    pub to_region: u8,
    /// The link it lies on.
    pub link: usize,
}

/// The road network of the town area.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoadNetwork {
    /// Every road cell (the full road width).
    pub mask: CellMask,
    pub nodes: Vec<RoadNode>,
    pub links: Vec<RoadLink>,
    pub passages: Vec<Passage>,
    /// Road width actually used, in cells (≥ 1).
    pub width_cells: u32,
}

/// What the houses node built (the houses themselves are `PlacedObject`s in
/// `GeneratedLevel::objects`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HouseLayout {
    /// Cells covered by houses.
    pub mask: CellMask,
    /// Houses placed along the roads (facing them) / in the yards behind.
    pub street_houses: u32,
    pub yard_houses: u32,
    /// Share of the buildable ground covered by houses: reached / asked, %.
    pub coverage_pct: f32,
    pub target_pct: f32,
}

/// An opening in a region wall where a road passes through.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WallGap {
    /// Center of the opening, in cell coordinates.
    pub x: usize,
    pub y: usize,
    /// The two regions it connects (lower id first).
    pub regions: (u8, u8),
    /// Wall cells removed for this opening.
    pub cells: u32,
}

/// The walls between regions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegionWalls {
    pub mask: CellMask,
    pub gaps: Vec<WallGap>,
    /// Border lines walked (one per stretch of border between two regions).
    pub lines: u32,
    /// Wall points placed over all lines.
    pub points: u32,
    /// Region cells re-assigned so every region ends exactly at its walls.
    #[serde(default)]
    pub recolored_cells: u32,
}

/// A generated level: the config snapshot it came from plus everything placed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedLevel {
    pub config: LevelConfig,
    pub seed: u64,
    pub objects: Vec<PlacedObject>,
    /// The noise-derived region footprint, once the shape section is built.
    pub shape: Option<LevelShape>,
    /// The outer wall ring, once the outer-walls section is built.
    pub walls: Option<OuterWall>,
    /// The region split of the town area, once the regions section is built.
    pub regions: Option<RegionSplit>,
    /// The road network, once the roads section is built.
    #[serde(default)]
    pub roads: Option<RoadNetwork>,
    /// The walls between regions, once the region walls section is built.
    #[serde(default)]
    pub region_walls: Option<RegionWalls>,
    /// The houses node's footprint and stats (town maps).
    #[serde(default)]
    pub houses: Option<HouseLayout>,
    /// Raw noise field behind the region shape, for the bench "noise texture"
    /// view.
    pub noise_preview: Option<NoiseTexture>,
    /// Non-blocking notes from generation (early stops, budget exhaustion, ...).
    /// Shown in the warnings panel with no field marker.
    pub notes: Vec<String>,
}
