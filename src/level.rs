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
    /// Extra rectangles of a compound object (a house's wings), overlapping
    /// the main rect and sharing its rotation. Empty for a simple rect.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<ObjectPart>,
    /// A disc inscribed in the rect instead of the rect (trees).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub round: bool,
}

/// One extra rectangle of a compound object, in the object's own frame: the
/// center offset (`u_m` along the width / `angle_deg` direction, `v_m` along
/// the depth) from the object's center, and its size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ObjectPart {
    pub u_m: f32,
    pub v_m: f32,
    pub w_m: f32,
    pub h_m: f32,
}

impl PlacedObject {
    pub fn center(&self) -> (f32, f32) {
        (self.x_m + self.w_m / 2.0, self.y_m + self.h_m / 2.0)
    }

    /// Is the point (meters) inside the rotated rect or any of its parts?
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let (cx, cy) = self.center();
        // Markers (mob spawns, gates) and round objects are circles
        // inscribed in their rect.
        if self.round || matches!(self.category, Category::MobSpawn | Category::Gate) {
            let r = self.w_m.min(self.h_m) / 2.0;
            return (x - cx).powi(2) + (y - cy).powi(2) <= r * r;
        }
        let (s, c) = self.angle_deg.to_radians().sin_cos();
        let (dx, dy) = (x - cx, y - cy);
        let (u, v) = (dx * c + dy * s, dy * c - dx * s);
        let inside = |pu: f32, pv: f32, w: f32, h: f32| (u - pu).abs() <= w / 2.0 && (v - pv).abs() <= h / 2.0;
        inside(0.0, 0.0, self.w_m, self.h_m) || self.parts.iter().any(|p| inside(p.u_m, p.v_m, p.w_m, p.h_m))
    }

    /// The four corners of the main rotated rect, in meters, clockwise on
    /// screen.
    pub fn corners(&self) -> [(f32, f32); 4] {
        self.rect_corners(0.0, 0.0, self.w_m, self.h_m)
    }

    /// Corners of every rect: the main one first, then each part.
    pub fn all_corners(&self) -> Vec<[(f32, f32); 4]> {
        std::iter::once(self.corners())
            .chain(self.parts.iter().map(|p| self.rect_corners(p.u_m, p.v_m, p.w_m, p.h_m)))
            .collect()
    }

    fn rect_corners(&self, pu: f32, pv: f32, w: f32, h: f32) -> [(f32, f32); 4] {
        let (cx, cy) = self.center();
        let (s, c) = self.angle_deg.to_radians().sin_cos();
        let (hw, hh) = (w / 2.0, h / 2.0);
        [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]
            .map(|(u, v)| (pu + u, pv + v))
            .map(|(u, v)| (cx + u * c - v * s, cy + u * s + v * c))
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
    /// This road's width in cells (main roads widest, alleys narrowest).
    #[serde(default)]
    pub width_cells: u32,
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
    /// Widest road width actually used, in cells (≥ 1).
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
    /// Houses built from 2–3 overlapping rectangles.
    #[serde(default)]
    pub complex_houses: u32,
    /// Share of the buildable ground covered by houses: reached / asked, %.
    pub coverage_pct: f32,
    pub target_pct: f32,
}

/// What the props node built (the props themselves are `PlacedObject`s in
/// `GeneratedLevel::objects`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropLayout {
    /// Cells covered by props.
    pub mask: CellMask,
    /// Props placed against houses / at road edges / scattered.
    pub near_houses: u32,
    pub roadside: u32,
    pub scattered: u32,
    /// Swamp maps: against trees / on the shore / in the water / beside
    /// paths (scattered ones count in `scattered`).
    #[serde(default)]
    pub near_trees: u32,
    #[serde(default)]
    pub shore: u32,
    #[serde(default)]
    pub in_water: u32,
    #[serde(default)]
    pub path_edge: u32,
    /// The quantity asked for, and the cap the density allowed.
    pub requested: u32,
    pub density_cap: u32,
}

/// The interactive node's output: footprint, groups and per-type counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InteractiveLayout {
    /// Cells covered by interactive objects.
    pub mask: CellMask,
    pub groups: Vec<InteractiveGroup>,
    /// One entry per configured type, in config order.
    pub types: Vec<InteractiveTypeStats>,
}

/// One group of identical interactive objects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InteractiveGroup {
    /// Index of the type in `config.interactive`.
    pub type_index: usize,
    pub type_name: String,
    /// Group center in meters (the first member stands on it).
    pub center_m: [f32; 2],
    /// Indices of the members in `GeneratedLevel::objects`.
    pub members: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InteractiveTypeStats {
    pub name: String,
    pub requested: u32,
    pub placed: u32,
    pub groups: u32,
}

/// The gates node's output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateLayout {
    pub gates: Vec<GateInfo>,
    /// Gates asked for.
    pub requested: u32,
}

/// One placed gate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateInfo {
    /// 1-based gate id (its place in the config list).
    pub id: u32,
    /// Direction asked for, and where the gate ended up (they differ when
    /// the asked ray met no wall and a nearby angle was used).
    pub direction_deg: f32,
    pub actual_deg: f32,
    /// The gate point (middle of the wall opening), meters.
    pub point_m: [f32; 2],
    /// Region the gate opens into.
    pub region: u8,
    /// Width of the wall opening, meters (0 on maps without an outer wall).
    pub opening_m: f32,
    /// Index of the gate road in `RoadNetwork::links`, if one was laid.
    pub road_link: Option<usize>,
    /// Index of its marker in `GeneratedLevel::objects`.
    pub object: usize,
}

/// Where one swamp gate meets the map border (swamp gate anchors node).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateAnchor {
    /// 1-based gate id (its place in the config list).
    pub id: u32,
    /// Direction asked for, and the one used (a corner's direction when the
    /// gate snapped into a corner).
    pub direction_deg: f32,
    pub actual_deg: f32,
    pub style: crate::model::GateStyle,
    /// The gate snapped into a map corner.
    pub corner: bool,
    /// The gate (teleport) point on the map edge, meters.
    pub point_m: [f32; 2],
    /// Where firm ground must be, meters: the gate island's center (shore
    /// gates — the island reaches over the edge) or the center of the
    /// island the bridge starts from (bridge gates).
    pub land_m: [f32; 2],
    /// Diameter of that island, meters.
    pub island_m: f32,
    /// Bridge length from the island's shore to the gate point, meters (0
    /// for shore gates).
    pub bridge_m: f32,
    /// Index of its marker in `GeneratedLevel::objects`.
    pub object: usize,
}

/// What an island is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IslandKind {
    /// At a gate (shore gates: touching the border; bridge gates: where the
    /// bridge starts).
    Gate,
    /// Big island: clearing, camp, arena.
    Hub,
    /// Small island: stepping stone.
    Small,
    /// Tiny safe island the depth node puts on a mud path too long to wade
    /// in one breath.
    Hummock,
}

/// One island of firm ground.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Island {
    /// 1-based id, the value in `IslandLayout::ids`.
    pub id: u16,
    pub kind: IslandKind,
    /// The gate it belongs to (gate islands).
    pub gate: Option<u32>,
    /// Centroid of its cells, meters.
    pub center_m: [f32; 2],
    /// Diameter asked for, meters.
    pub size_m: f32,
    pub cells: u32,
    pub area_m2: f32,
}

/// The swamp islands node's output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IslandLayout {
    pub width: usize,
    pub height: usize,
    pub cell_size_m: f32,
    /// Island id per cell (0 = no island).
    pub ids: Vec<u16>,
    pub islands: Vec<Island>,
    /// Hub + small islands asked for (gate islands not counted).
    pub requested: u32,
    /// Share of the map that is island, %.
    pub land_pct: f32,
}

impl IslandLayout {
    pub fn mask(&self) -> CellMask {
        CellMask { width: self.width, height: self.height, cell_size_m: self.cell_size_m, cells: self.ids.iter().map(|&i| i > 0).collect() }
    }
}

/// What a swamp link is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PathKind {
    /// A winding strip of walkable mud.
    Mud,
    /// Planks on posts over the water: straight segments with kinks.
    Boardwalk,
}

/// One link between two islands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SwampPath {
    /// Island ids at both ends.
    pub a: u16,
    pub b: u16,
    pub kind: PathKind,
    /// Added on top of the spanning tree (it closes a loop).
    pub extra: bool,
    /// Links a dead-end island.
    pub dead_end: bool,
    pub width_m: f32,
    /// Length over water, meters (the part between the islands).
    pub length_m: f32,
    /// Centerline over water, meters: points every ~1 m (mud) or the
    /// corners (boardwalk).
    pub path_m: Vec<[f32; 2]>,
}

/// A boardwalk from a bridge gate's island out to the gate on the border.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateBridge {
    pub gate: u32,
    pub island: u16,
    /// From the island's shore to the gate point, meters.
    pub from_m: [f32; 2],
    pub to_m: [f32; 2],
    pub length_m: f32,
}

/// The swamp paths node's output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathLayout {
    /// Mud path cells (off the islands).
    pub mud: CellMask,
    /// Boardwalk cells (links and gate bridges, off the islands).
    pub boardwalk: CellMask,
    pub paths: Vec<SwampPath>,
    pub bridges: Vec<GateBridge>,
}

/// The swamp depth & border node's output. Every cell is exactly one of:
/// island, mud path, boardwalk (earlier nodes) or one of the masks here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DepthLayout {
    /// Walkable water near the islands and paths.
    pub shallow: CellMask,
    /// Walkable, drains breath `mud_cost` times faster.
    pub mud: CellMask,
    /// Not walkable: the inner walls of the swamp and most of the border.
    pub deep: CellMask,
    /// Not walkable: thicket / rock stretches of the border.
    pub thicket: CellMask,
    /// 0 = safe ground (island, boardwalk); 1..=254 = breath used to get
    /// here from safe ground, scaled to the wading limit; 255 = deep water or
    /// thicket.
    pub depth: Vec<u8>,
    /// How far into the water things may spawn, meters (the wading limit,
    /// halved with the return trip).
    pub reach_m: f32,
    pub hummocks: u32,
    /// Shares of the map, %.
    pub walkable_pct: f32,
    pub shallow_pct: f32,
    pub mud_pct: f32,
    pub deep_pct: f32,
    pub thicket_pct: f32,
}

/// The swamp trees node's output (the trees themselves are objects).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TreeLayout {
    /// Tree footprints.
    pub mask: CellMask,
    /// Trees on islands, and standing in the water.
    pub grove_trees: u32,
    pub water_trees: u32,
    /// Share of the grove zones covered, %.
    pub grove_cover_pct: f32,
}

/// The swamp node chain's output, filled node by node (docs/swamp-plan.md).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SwampLayout {
    pub gate_anchors: Vec<GateAnchor>,
    #[serde(default)]
    pub islands: Option<IslandLayout>,
    #[serde(default)]
    pub paths: Option<PathLayout>,
    #[serde(default)]
    pub depth: Option<DepthLayout>,
    #[serde(default)]
    pub trees: Option<TreeLayout>,
}

/// The two styles of carved cave space (docs/cave-plan.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaveStyle {
    /// Caverns: noise blobs, winding tunnels, ragged walls.
    Natural,
    /// Dug passages: rectangular rooms, straight tunnels, clean walls.
    HandMade,
}

impl CaveStyle {
    pub fn label(self) -> &'static str {
        match self {
            CaveStyle::Natural => "natural",
            CaveStyle::HandMade => "hand-made",
        }
    }
}

/// One connected patch of one cave style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaveZone {
    /// 1-based, the value in [`StyleLayout::ids`].
    pub id: u16,
    pub style: CaveStyle,
    pub area_m2: f32,
    /// Center of mass, meters (may fall outside a curved zone).
    pub center_m: [f32; 2],
}

/// The cave style node's output: every cell belongs to exactly one zone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleLayout {
    pub width: usize,
    pub height: usize,
    pub cell_size_m: f32,
    /// Zone id per cell (1-based, never 0).
    pub ids: Vec<u16>,
    pub zones: Vec<CaveZone>,
    /// Direction the natural side lies toward at low mixing, degrees (0°
    /// east, 90° north).
    pub natural_side_deg: f32,
    /// The natural share that came out, %.
    pub natural_pct: f32,
}

impl StyleLayout {
    pub fn style_at(&self, c: usize) -> CaveStyle {
        self.zones[self.ids[c] as usize - 1].style
    }

    /// Cells in the given style.
    pub fn mask(&self, style: CaveStyle) -> CellMask {
        CellMask {
            width: self.width,
            height: self.height,
            cell_size_m: self.cell_size_m,
            cells: (0..self.ids.len()).map(|c| self.style_at(c) == style).collect(),
        }
    }
}

/// What a cave chamber is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChamberKind {
    /// A big chamber: arena, camp, the content.
    Hall,
    /// A small chamber / side room.
    Grotto,
}

/// One cave chamber.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chamber {
    /// 1-based, the value in [`ChamberLayout::ids`].
    pub id: u16,
    pub kind: ChamberKind,
    /// The style of the zone at its center: blob (natural) or room
    /// (hand-made).
    pub style: CaveStyle,
    pub zone: u16,
    /// Center of mass, meters.
    pub center_m: [f32; 2],
    /// Rolled size, meters (diameter of a circle of the asked area).
    pub size_m: f32,
    /// Long axis direction, degrees (0° east, 90° north).
    pub angle_deg: f32,
    pub cells: u32,
    pub area_m2: f32,
}

/// The cave chambers node's output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChamberLayout {
    pub width: usize,
    pub height: usize,
    pub cell_size_m: f32,
    /// Chamber id per cell (0 = rock).
    pub ids: Vec<u16>,
    pub chambers: Vec<Chamber>,
    /// Halls + grottos asked for.
    pub requested: u32,
    /// Share of the map that is chamber floor, %.
    pub floor_pct: f32,
}

impl ChamberLayout {
    pub fn mask(&self) -> CellMask {
        CellMask { width: self.width, height: self.height, cell_size_m: self.cell_size_m, cells: self.ids.iter().map(|&i| i > 0).collect() }
    }
}

/// The cave node chain's output, filled node by node (docs/cave-plan.md).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CaveLayout {
    #[serde(default)]
    pub style: Option<StyleLayout>,
    #[serde(default)]
    pub chambers: Option<ChamberLayout>,
}

/// The mobs node's output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MobLayout {
    pub spawns: Vec<MobSpawn>,
    /// Mobs placed per type, in config order.
    pub per_type: Vec<u32>,
    pub entities: u32,
    /// The minimum asked for (count, or density × walkable area), capped by
    /// the entity cap.
    #[serde(default)]
    pub min_target: u32,
    /// May exceed `budget` when the minimum needed more mobs.
    pub spent: f32,
    pub budget: f32,
    /// Spawn spacing in force at the end (relaxed below the setting when the
    /// minimum did not fit otherwise).
    #[serde(default)]
    pub spacing_used_m: f32,
    /// Swamp maps: mobs placed on hub islands for the hub share.
    #[serde(default)]
    pub in_hubs: u32,
    /// Why spending stopped (budget, entity cap or space).
    pub stop_reason: String,
}

/// One spawn point and the pack that appears there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MobSpawn {
    /// Spawn center in meters.
    pub center_m: [f32; 2],
    /// Radius of the pack's footprint, meters.
    pub radius_m: f32,
    pub members: Vec<MobPackMember>,
    pub cost: f32,
    pub on_road: bool,
    /// The hidden room it stands in (only rooms with a mob-passable path).
    pub hidden_room: Option<u32>,
    /// Swamp maps: placed on a hub island to meet the hub share.
    #[serde(default)]
    pub hub: bool,
    /// Indices of its markers in `GeneratedLevel::objects`: one per mob,
    /// each at its type's body size, inside the pack circle.
    pub objects: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MobPackMember {
    /// Index of the type in `config.mobs.types`.
    pub type_index: usize,
    pub name: String,
    pub count: u32,
}

/// The hidden rooms node's output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HiddenRoomLayout {
    /// The rooms' own walls (painted like the outer wall).
    pub wall_mask: CellMask,
    /// Room floors + access paths: walkable, but only through the path.
    pub floor_mask: CellMask,
    pub rooms: Vec<HiddenRoom>,
    /// Rooms asked for.
    pub requested: u32,
}

/// One hidden room and its access path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HiddenRoom {
    /// 1-based room id.
    pub id: u32,
    /// Floor rect in meters (inside the room's walls): x, y, w, h.
    pub rect_m: [f32; 4],
    /// Dug beyond the outer wall (else inside the area).
    pub outside: bool,
    /// Path centerline from the room to walkable ground, meters (cell
    /// centers, room end first).
    pub path_m: Vec<[f32; 2]>,
    pub path_width_m: f32,
    /// The player always fits; this says whether mobs may use the path.
    pub mob_passable: bool,
    /// Indices of the room's objects (floor first, then path pieces) in
    /// `GeneratedLevel::objects`.
    pub objects: Vec<usize>,
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
    /// The props node's footprint and stats.
    #[serde(default)]
    pub props: Option<PropLayout>,
    /// The interactive node's groups and stats.
    #[serde(default)]
    pub interactive: Option<InteractiveLayout>,
    /// The hidden rooms node's rooms, walls and paths.
    #[serde(default)]
    pub hidden_rooms: Option<HiddenRoomLayout>,
    /// The gates node's gates.
    #[serde(default)]
    pub gates: Option<GateLayout>,
    /// The mobs node's spawn points and spending.
    #[serde(default)]
    pub mobs: Option<MobLayout>,
    /// The swamp chain's output (swamp maps only).
    #[serde(default)]
    pub swamp: Option<SwampLayout>,
    /// The cave chain's output (cave maps only).
    #[serde(default)]
    pub cave: Option<CaveLayout>,
    /// Raw noise field behind the region shape, for the bench "noise texture"
    /// view.
    pub noise_preview: Option<NoiseTexture>,
    /// Non-blocking notes from generation (early stops, budget exhaustion, ...).
    /// Shown in the warnings panel with no field marker.
    pub notes: Vec<String>,
}
