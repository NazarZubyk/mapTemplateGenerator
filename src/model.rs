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
    /// Decorative, non-interactive props (step 8, every map kind).
    pub props: PropSettings,
    /// Interactive object types (step 9, every map kind) — herbs to gather,
    /// chests to open... each type placed in groups.
    pub interactive: Vec<InteractiveType>,
    /// Hidden rooms with one small access path each (step 10, every map kind).
    pub hidden_rooms: HiddenRoomSettings,
    /// Mob spawn points bought from a budget (step 11, every map kind).
    pub mobs: MobSettings,
    /// Entry/exit gates on the outer wall (step 12, every map kind; runs
    /// right after the region walls so later nodes avoid the gate roads).
    pub gates: GateSettings,
    /// The swamp node chain (swamp maps only; docs/swamp-plan.md).
    pub swamp: SwampSettings,
    /// The cave node chain (cave maps only; docs/cave-plan.md).
    pub cave: CaveSettings,
    /// The forest node chain (forest maps only; docs/forest-plan.md).
    pub forest: ForestSettings,
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
            props: PropSettings::default(),
            interactive: vec![InteractiveType::default()],
            hidden_rooms: HiddenRoomSettings::default(),
            mobs: MobSettings::default(),
            gates: GateSettings::default(),
            swamp: SwampSettings::default(),
            cave: CaveSettings::default(),
            forest: ForestSettings::default(),
            cell_size_m: 0.5,
            seed: 0,
        }
    }
}

impl LevelConfig {
    /// The map kind the app (and the MCP server, with no config file yet)
    /// starts on — the kind being built right now. `default()` stays Town,
    /// the baseline the tests are written against.
    pub const START_KIND: MapKind = MapKind::Forest;

    /// The defaults with [`Self::START_KIND`] and its shape defaults.
    pub fn starting() -> Self {
        Self { kind: Self::START_KIND, shape: ShapeSettings::for_kind(Self::START_KIND), ..Self::default() }
    }

    /// The defaults on the old 64 m map: the town-chain tests were written
    /// for it (and stay fast).
    #[cfg(test)]
    pub fn test_64() -> Self {
        let mut c = Self::default();
        c.map.width_m = 64.0;
        c.map.height_m = 64.0;
        c
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

/// Settings of the props node (docs/workflow.md, step 8; every map kind):
/// decorative, non-interactive objects (crates, barrels, rocks, logs, …).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PropSettings {
    /// How many props to place.
    pub quantity: u32,
    /// Coverage cap, %: props never cover more than this share of the free
    /// ground (town minus walls, roads and houses). Whichever of quantity and
    /// density is lower wins.
    pub density_pct: f32,
    /// Smallest prop side in meters (width and depth rolled independently).
    pub size_min_m: f32,
    /// Largest prop side in meters.
    pub size_max_m: f32,
    /// Share of props, %, set against house walls (town maps; elsewhere they
    /// are scattered instead).
    pub near_houses_pct: f32,
    /// Share of props, %, set at the road edges. What is left over from the
    /// two shares is scattered over open ground.
    pub roadside_pct: f32,
    /// Prop color on map.png, the preview and the legend.
    pub color: [u8; 3],
    /// Swamp maps: shares of props, %, against trees / on the shore (land
    /// next to water) / in the water (within reach) / beside paths and
    /// boardwalks; the rest are scattered over the islands.
    pub near_trees_pct: f32,
    pub shore_pct: f32,
    pub in_water_pct: f32,
    pub path_edge_pct: f32,
    /// Cave maps: shares of props, %, along the cave walls / in tunnels /
    /// at pool edges; the rest are scattered over the chamber floors.
    pub near_walls_pct: f32,
    pub tunnel_pct: f32,
    pub pool_edge_pct: f32,
}

impl Default for PropSettings {
    fn default() -> Self {
        Self {
            quantity: 40,
            density_pct: 5.0,
            size_min_m: 0.5,
            size_max_m: 1.5,
            near_houses_pct: 50.0,
            roadside_pct: 25.0,
            color: [92, 110, 140],
            near_trees_pct: 25.0,
            shore_pct: 25.0,
            in_water_pct: 20.0,
            path_edge_pct: 15.0,
            near_walls_pct: 40.0,
            tunnel_pct: 15.0,
            pool_edge_pct: 10.0,
        }
    }
}

/// How a swamp gate meets the map border (swamp maps only; towns ignore it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateStyle {
    /// On a gate island that touches the map border.
    #[default]
    Shore,
    /// At the border end of a boardwalk that runs out from an island.
    Bridge,
}

impl GateStyle {
    pub const ALL: [GateStyle; 2] = [GateStyle::Shore, GateStyle::Bridge];

    pub fn label(self) -> &'static str {
        match self {
            GateStyle::Shore => "Shore",
            GateStyle::Bridge => "Bridge",
        }
    }
}

/// One gate: where on the map it is generated.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GateSpec {
    /// Direction from the map center, degrees: 0 = east (right), 90 = north
    /// (up), 180 = west, 270 = south — counterclockwise on screen.
    pub direction_deg: f32,
    /// Swamp maps: shore island or bridge (towns ignore it).
    pub style: GateStyle,
}

impl GateSpec {
    pub fn at(direction_deg: f32) -> Self {
        Self { direction_deg, ..Self::default() }
    }
}

impl Default for GateSpec {
    fn default() -> Self {
        Self { direction_deg: 90.0, style: GateStyle::Shore }
    }
}

/// Settings of the gates node (docs/workflow.md, step 12).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GateSettings {
    pub gates: Vec<GateSpec>,
    /// Width of the opening cut into the outer wall at each gate, meters.
    pub opening_m: f32,
    /// Lay a road from each gate to the nearest road of the network.
    pub road: bool,
    /// Width of that gate road, meters.
    pub road_width_m: f32,
    /// Gate marker color on map.png, the preview and the legend.
    pub color: [u8; 3],
}

impl Default for GateSettings {
    fn default() -> Self {
        Self {
            gates: vec![GateSpec::at(90.0), GateSpec::at(270.0)],
            opening_m: 3.0,
            road: true,
            road_width_m: 2.5,
            color: [50, 110, 230],
        }
    }
}

/// Settings of the cave node chain (docs/cave-plan.md), one struct per
/// node.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CaveSettings {
    /// Node 2: the natural / hand-made style field and its zones.
    pub style: CaveStyleSettings,
    /// Node 3: halls and grottos.
    pub chambers: ChamberSettings,
    /// Node 4: tunnels linking the chambers, gate tunnels.
    pub tunnels: TunnelSettings,
    /// Node 5: carving the floor out, natural walls, the wall band.
    pub carve: CarveSettings,
    /// Node 6: chasms and pools.
    pub hazards: HazardSettings,
}

/// Settings of the cave hazards node (step C5): chasms in halls (with
/// optional rope bridges) and pools in chambers. Neither may split the
/// floor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HazardSettings {
    /// Share of the halls, %, with a chasm.
    pub chasm_pct: f32,
    /// Chasm size, % of its hall's floor.
    pub chasm_size_pct: f32,
    /// Share of the chasms, %, with a rope bridge across.
    pub bridge_pct: f32,
    pub bridge_width_m: f32,
    /// Share of the chambers without a chasm, %, with a pool.
    pub pool_pct: f32,
    /// Pool size, % of its chamber's floor.
    pub pool_size_pct: f32,
    /// Walkable shallow band along the pool's edge, meters; the rest is
    /// deep (impassable).
    pub shallow_m: f32,
    pub chasm_color: [u8; 3],
    pub bridge_color: [u8; 3],
    pub shallow_color: [u8; 3],
    pub deep_color: [u8; 3],
}

impl Default for HazardSettings {
    fn default() -> Self {
        Self {
            chasm_pct: 40.0,
            chasm_size_pct: 25.0,
            bridge_pct: 50.0,
            bridge_width_m: 1.5,
            pool_pct: 30.0,
            pool_size_pct: 30.0,
            shallow_m: 1.5,
            chasm_color: [10, 8, 12],
            bridge_color: [150, 110, 60],
            shallow_color: [74, 116, 138],
            deep_color: [30, 62, 98],
        }
    }
}

/// Settings of the cave carve & smooth node (step C4): natural walls get
/// rough and organic, hand-made walls stay as dug.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CarveSettings {
    /// How ragged the natural walls get, % (noise pushes the wall in and
    /// out by up to ~2.5 m at 100%).
    pub roughness_pct: f32,
    /// Cellular-automaton passes (4-5 rule) smoothing the natural walls.
    pub smooth_passes: u32,
    /// Rock specks smaller than this, m², inside the floor are dug out.
    pub min_rock_m2: f32,
    /// Width of the wall band (rock touching the floor), meters.
    pub wall_band_m: f32,
    pub wall_color: [u8; 3],
}

impl Default for CarveSettings {
    fn default() -> Self {
        Self { roughness_pct: 50.0, smooth_passes: 4, min_rock_m2: 4.0, wall_band_m: 1.0, wall_color: [30, 26, 24] }
    }
}

/// Settings of the cave tunnels node (step C3): how the chambers are
/// linked (straightforward → labyrinth) and what the tunnels look like.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TunnelSettings {
    /// 0 = one route, chamber after chamber; 40 = half labyrinth; 100 =
    /// dense maze with crossings. Sets branching / loops / dead ends /
    /// junctions while `auto` is on.
    pub labyrinth_pct: f32,
    /// Hand-made parts get their own labyrinth value.
    pub per_style: bool,
    /// Labyrinth % of the hand-made parts (with `per_style`).
    pub handmade_labyrinth_pct: f32,
    /// Branching / loops / dead ends / junctions follow the labyrinth
    /// slider; off = the values below are used as set.
    pub auto: bool,
    /// Chance, %, a new chamber hangs on the nearest chamber of the network
    /// instead of the last one (0 = a chain, 100 = a tree).
    pub branching_pct: f32,
    /// Chance per pair of neighboring chambers of an extra link closing a
    /// loop, %.
    pub loops_pct: f32,
    /// Blind tunnel stubs.
    pub dead_ends: u32,
    /// Tunnel crossings with no chamber.
    pub junctions: u32,
    /// Natural tunnel width range, meters (by traffic).
    pub width_min_m: f32,
    pub width_max_m: f32,
    /// Natural tunnels: how much the width changes along a tunnel, %
    /// (± around its width).
    pub width_variation_pct: f32,
    /// Natural tunnels: how much they wind, %.
    pub wiggle_pct: f32,
    /// Share of the natural tunnels, %, with a short narrow stretch.
    pub squeeze_pct: f32,
    pub squeeze_width_m: f32,
    /// Hand-made tunnel width, meters.
    pub mine_width_m: f32,
    /// Hand-made tunnels: how uneven the hand-dug walls are, % (0 =
    /// ruler-straight).
    pub mine_roughness_pct: f32,
    /// Hand-made tunnels: chance, %, of a 45° leg.
    pub diagonal_pct: f32,
    /// Hand-made tunnels: most turns.
    pub max_turns: u32,
    /// Dead-end stub length range, meters.
    pub stub_min_m: f32,
    pub stub_max_m: f32,
}

impl Default for TunnelSettings {
    fn default() -> Self {
        let mut s = Self {
            labyrinth_pct: 40.0,
            per_style: false,
            handmade_labyrinth_pct: 70.0,
            auto: true,
            branching_pct: 0.0,
            loops_pct: 0.0,
            dead_ends: 0,
            junctions: 0,
            width_min_m: 3.5,
            width_max_m: 7.0,
            width_variation_pct: 40.0,
            wiggle_pct: 50.0,
            squeeze_pct: 30.0,
            squeeze_width_m: 2.0,
            mine_width_m: 2.0,
            mine_roughness_pct: 50.0,
            diagonal_pct: 20.0,
            max_turns: 2,
            stub_min_m: 6.0,
            stub_max_m: 16.0,
        };
        let d = LabyrinthParams::from_pct(s.labyrinth_pct);
        (s.branching_pct, s.loops_pct, s.dead_ends) = (d.branching_pct, d.loops_pct, d.dead_ends);
        s
    }
}

/// What the labyrinth slider sets (docs/cave-plan.md, "Complexity").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabyrinthParams {
    pub branching_pct: f32,
    pub loops_pct: f32,
    pub dead_ends: u32,
    /// Junctions per chamber.
    pub junction_share: f32,
}

impl LabyrinthParams {
    pub fn from_pct(pct: f32) -> Self {
        let l = (pct / 100.0).clamp(0.0, 1.0);
        Self {
            branching_pct: (l * 150.0).min(100.0),
            loops_pct: ((l - 0.2) * 125.0).clamp(0.0, 100.0),
            dead_ends: (l * 8.0).round() as u32,
            junction_share: ((l - 0.5) * 1.2).max(0.0),
        }
    }
}

/// Settings of the cave chambers node (step C2): the halls and grottos the
/// tunnels will link. A chamber takes the style of the zone at its center.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChamberSettings {
    /// Number of big chambers (arenas, camps).
    pub hall_count: u32,
    /// Hall size range, meters (diameter of a circle of the same area).
    pub hall_min_m: f32,
    pub hall_max_m: f32,
    /// Grottos as a total count or a density per 1000 m².
    pub grotto_amount: NodeAmount,
    pub grotto_count: u32,
    pub grotto_per_1000m2: f32,
    /// Grotto size range, meters.
    pub grotto_min_m: f32,
    pub grotto_max_m: f32,
    /// Natural chambers: how far the outline strays from an ellipse, %.
    pub irregularity_pct: f32,
    /// Natural chambers: stretch, % (100 = up to 2.5× longer than wide).
    pub elongation_pct: f32,
    /// Hand-made rooms: most length / width.
    pub room_aspect_max: f32,
    /// Hand-made rooms: most rotation off the map axes, degrees (0 =
    /// always axis-aligned).
    pub room_rotation_deg: f32,
    /// Hand-made rooms: how uneven the hand-dug walls are, % (0 = exact
    /// rectangles).
    pub room_roughness_pct: f32,
    /// Least rock between two chambers, meters.
    pub gap_m: f32,
    /// Chambers keep this far from the map edge, meters (solid rock).
    pub rock_margin_m: f32,
    /// Rock color (tinted a little by the style zone under it).
    pub rock_color: [u8; 3],
}

impl Default for ChamberSettings {
    fn default() -> Self {
        Self {
            hall_count: 5,
            hall_min_m: 22.0,
            hall_max_m: 36.0,
            grotto_amount: NodeAmount::Density,
            grotto_count: 14,
            grotto_per_1000m2: 0.4,
            grotto_min_m: 6.0,
            grotto_max_m: 12.0,
            irregularity_pct: 50.0,
            elongation_pct: 40.0,
            room_aspect_max: 1.8,
            room_rotation_deg: 0.0,
            room_roughness_pct: 40.0,
            gap_m: 6.0,
            rock_margin_m: 6.0,
            rock_color: [52, 47, 44],
        }
    }
}

/// Settings of the cave style node (step C1): how much of the cave is
/// natural caverns vs hand-made passages, and how the two are arranged.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaveStyleSettings {
    /// Share of the map, %, in the natural style; the rest is hand-made.
    pub natural_pct: f32,
    /// How the two styles are arranged, %: 0 = two zones split by one wavy
    /// line, 100 = a fine patchwork (about one chamber per patch).
    pub mixing_pct: f32,
    /// Tunnels allowed across a natural / hand-made boundary (used by the
    /// tunnels node at low mixing, so the zones stay separate regions).
    pub zone_links: u32,
    /// Patches of one style smaller than this, m², join the style around
    /// them.
    pub min_zone_m2: f32,
    pub natural_color: [u8; 3],
    pub handmade_color: [u8; 3],
}

impl Default for CaveStyleSettings {
    fn default() -> Self {
        Self {
            natural_pct: 60.0,
            mixing_pct: 30.0,
            zone_links: 2,
            min_zone_m2: 300.0,
            natural_color: [122, 100, 78],
            handmade_color: [138, 140, 150],
        }
    }
}

/// Settings of the forest node chain (docs/forest-plan.md), one struct per
/// node.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ForestSettings {
    /// Node 2: the big areas, their style (wild / managed) and character.
    pub areas: ForestAreaSettings,
}

/// Settings of the forest areas node (step F1): the map split into a few
/// large areas, each wild or managed and with its own character.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ForestAreaSettings {
    /// How many areas the map is split into.
    pub count: u32,
    /// How much the area sizes differ, %: 0 = all about the same size.
    pub size_variation_pct: f32,
    /// How far the borders between areas stray from straight lines, %.
    pub irregularity_pct: f32,
    /// Share of the map, %, in wild areas; the rest is managed.
    pub wild_pct: f32,
    /// How the styles are arranged, %: 0 = managed areas gather on one
    /// side (the style changes from managed to wild across the map),
    /// 100 = styles alternate area by area.
    pub mixing_pct: f32,
    /// The managed side starts at the first gate (else a random side).
    pub managed_near_gate: bool,
    /// Weight of each area character (the style picks which apply).
    pub characters: CharacterWeights,
}

impl Default for ForestAreaSettings {
    fn default() -> Self {
        Self {
            count: 6,
            size_variation_pct: 40.0,
            irregularity_pct: 50.0,
            wild_pct: 60.0,
            mixing_pct: 20.0,
            managed_near_gate: true,
            characters: CharacterWeights::default(),
        }
    }
}

/// Weights of the forest area characters. Wild areas roll meadow, open
/// woods, dense woods or old growth; managed areas meadow, logged,
/// plantation or camp grounds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterWeights {
    pub meadow: f32,
    pub open_woods: f32,
    pub dense_woods: f32,
    pub old_growth: f32,
    pub logged: f32,
    pub plantation: f32,
    pub camp_grounds: f32,
}

impl Default for CharacterWeights {
    fn default() -> Self {
        Self { meadow: 20.0, open_woods: 30.0, dense_woods: 35.0, old_growth: 20.0, logged: 35.0, plantation: 25.0, camp_grounds: 15.0 }
    }
}

impl CharacterWeights {
    pub fn of(&self, c: crate::level::ForestCharacter) -> f32 {
        use crate::level::ForestCharacter as F;
        match c {
            F::Meadow => self.meadow,
            F::OpenWoods => self.open_woods,
            F::DenseWoods => self.dense_woods,
            F::OldGrowth => self.old_growth,
            F::Logged => self.logged,
            F::Plantation => self.plantation,
            F::CampGrounds => self.camp_grounds,
        }
    }
}

/// Settings of the swamp node chain (docs/swamp-plan.md), one struct per
/// node.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SwampSettings {
    /// Node 2: where each gate meets the map border.
    pub gate_anchors: GateAnchorSettings,
    /// Node 3: the islands of firm ground.
    pub islands: IslandSettings,
    /// Node 4: mud paths and boardwalks linking the islands.
    pub paths: PathSettings,
    /// Node 5: water depth, the wading limit and the map border.
    pub depth: DepthSettings,
    /// Node 6: trees, the swamp's structure.
    pub trees: TreeSettings,
}

/// Settings of the swamp trees node (step S5).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TreeSettings {
    /// Share of the grove zones, %, covered by trees.
    pub density_pct: f32,
    /// Share of the island ground, %, that is grove zone (noise-picked
    /// patches; the rest stays clearing).
    pub grove_area_pct: f32,
    /// Trunk + crown diameter range, meters.
    pub size_min_m: f32,
    pub size_max_m: f32,
    /// No tree this close to a gate point, meters.
    pub gate_clear_m: f32,
    /// Single trees standing in shallow water and deep mud.
    pub water_trees: bool,
    /// Water trees per 1000 m² of walkable water.
    pub water_per_1000m2: f32,
    pub color: [u8; 3],
    pub water_color: [u8; 3],
}

impl Default for TreeSettings {
    fn default() -> Self {
        Self {
            density_pct: 30.0,
            grove_area_pct: 55.0,
            size_min_m: 1.5,
            size_max_m: 4.0,
            gate_clear_m: 6.0,
            water_trees: true,
            water_per_1000m2: 1.5,
            color: [38, 92, 40],
            water_color: [56, 96, 70],
        }
    }
}

/// Settings of the swamp depth & border node (step S4).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DepthSettings {
    /// Shallow water reaches this far from islands and mud paths, meters.
    pub shallow_m: f32,
    /// Then deep mud for this many meters; deep water beyond.
    pub mud_m: f32,
    /// Random push of the band edges, ± meters (organic shores).
    pub wobble_m: f32,
    /// How far the player can wade through shallow water away from safe
    /// ground (islands, boardwalks) before drowning, meters.
    pub wade_limit_m: f32,
    /// Deep mud drains faster: 1 m of mud counts as this many meters.
    pub mud_cost: f32,
    /// Things in the water must be reachable AND leavable: at most half the
    /// wading limit from safe ground (used by the spawn nodes).
    pub return_trip: bool,
    /// Diameter of a hummock (a small safe island put on a mud path that is
    /// too long to wade in one breath), meters.
    pub hummock_m: f32,
    /// Width of the impassable border along the map edge, meters.
    pub border_m: f32,
    /// Share of the border, %, that is thicket / rock instead of deep water.
    pub thicket_pct: f32,
    /// The border is walked in segments of a random length in this range,
    /// each rolling thicket or water, meters.
    pub segment_min_m: f32,
    pub segment_max_m: f32,
    pub shallow_color: [u8; 3],
    pub mud_color: [u8; 3],
    pub deep_color: [u8; 3],
    pub thicket_color: [u8; 3],
}

impl Default for DepthSettings {
    fn default() -> Self {
        Self {
            shallow_m: 3.0,
            mud_m: 4.0,
            wobble_m: 3.0,
            wade_limit_m: 20.0,
            mud_cost: 2.0,
            return_trip: true,
            hummock_m: 3.0,
            border_m: 8.0,
            thicket_pct: 35.0,
            segment_min_m: 15.0,
            segment_max_m: 40.0,
            shallow_color: [150, 186, 182],
            mud_color: [104, 96, 66],
            deep_color: [46, 78, 104],
            thicket_color: [52, 76, 44],
        }
    }
}

/// Settings of the swamp paths node (step S3).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PathSettings {
    /// Mud path width range, meters: the busiest `main_paths_pct` of the
    /// links get the max, quiet ones narrow down to the min.
    pub width_min_m: f32,
    pub width_max_m: f32,
    pub main_paths_pct: f32,
    /// How much mud paths wind, % (0 = shortest line).
    pub wiggle_pct: f32,
    /// Chance per near-neighbor pair of an extra link (a loop), %. 0 = a
    /// pure tree: one way between any two islands.
    pub extra_links_pct: f32,
    /// Small islands linked by exactly one path and never by a loop.
    pub dead_ends: u32,
    /// Chance per link that it is a boardwalk instead of a mud path, %.
    pub boardwalk_pct: f32,
    /// Boardwalk width, meters (links and gate bridges).
    pub boardwalk_width_m: f32,
    /// Most kinks in one boardwalk (0 = straight).
    pub boardwalk_kinks: u32,
    pub mud_color: [u8; 3],
    pub boardwalk_color: [u8; 3],
}

impl Default for PathSettings {
    fn default() -> Self {
        Self {
            width_min_m: 1.5,
            width_max_m: 3.5,
            main_paths_pct: 30.0,
            wiggle_pct: 50.0,
            extra_links_pct: 25.0,
            dead_ends: 3,
            boardwalk_pct: 20.0,
            boardwalk_width_m: 1.5,
            boardwalk_kinks: 2,
            mud_color: [150, 128, 92],
            boardwalk_color: [176, 120, 64],
        }
    }
}

/// Settings of the swamp islands node (step S2). Sizes are diameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IslandSettings {
    /// Big islands (clearings, camps, arenas).
    pub hub_count: u32,
    pub hub_min_m: f32,
    pub hub_max_m: f32,
    /// Whether the small islands are a total count or a density.
    pub small_amount: NodeAmount,
    pub small_count: u32,
    /// Small islands per 1000 m² of map.
    pub small_per_1000m2: f32,
    pub small_min_m: f32,
    pub small_max_m: f32,
    /// How far shapes stray from a circle, %: stretch, warp and a rough coast.
    pub irregularity_pct: f32,
    /// Least water between two islands, meters (they never merge).
    pub gap_m: f32,
    /// Hub and small islands keep this far from the map edge, meters (gate
    /// islands reach over it).
    pub border_m: f32,
    /// Island color on map.png, the preview and the legend.
    pub color: [u8; 3],
}

impl Default for IslandSettings {
    fn default() -> Self {
        Self {
            hub_count: 6,
            hub_min_m: 26.0,
            hub_max_m: 40.0,
            small_amount: NodeAmount::Density,
            small_count: 30,
            small_per_1000m2: 0.6,
            small_min_m: 7.0,
            small_max_m: 14.0,
            irregularity_pct: 50.0,
            gap_m: 6.0,
            border_m: 10.0,
            color: [126, 138, 78],
        }
    }
}

/// Settings of the swamp gate anchors node (step S1). The gate list itself
/// (directions + style) is `LevelConfig::gates`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GateAnchorSettings {
    /// A gate whose direction is within this many degrees of a map corner
    /// moves into that corner. 0 = never.
    pub corner_snap_deg: f32,
    /// Gate island diameter range, meters (shore gates: the island at the
    /// border; bridge gates: the island the bridge starts from).
    pub island_min_m: f32,
    pub island_max_m: f32,
    /// Bridge length range for bridge gates, meters (island shore → border).
    pub bridge_min_m: f32,
    pub bridge_max_m: f32,
}

impl Default for GateAnchorSettings {
    fn default() -> Self {
        Self { corner_snap_deg: 20.0, island_min_m: 18.0, island_max_m: 26.0, bridge_min_m: 10.0, bridge_max_m: 20.0 }
    }
}

/// One mob type of the mobs node (docs/workflow.md, step 11).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MobType {
    /// Display name (map legend, spec.json).
    pub name: String,
    /// Cost of one mob of this type, taken from the budget.
    pub price: f32,
    /// Body size in meters (diameter).
    pub size_m: f32,
    /// Desired share of this type among all mobs, % (the UI keeps the
    /// shares at 100 in total: raising one lowers the rest).
    pub balance_pct: f32,
    /// Color on map.png, the preview and the legend.
    pub color: [u8; 3],
    /// Swamp maps: the terrains this type may spawn on.
    pub spawn_on: SpawnOn,
}

impl Default for MobType {
    fn default() -> Self {
        Self {
            name: "Goblin".to_owned(),
            price: 10.0,
            size_m: 0.8,
            balance_pct: 100.0,
            color: [200, 60, 50],
            spawn_on: SpawnOn::default(),
        }
    }
}

/// Which swamp terrains a mob type or an interactive type may spawn on
/// (towns ignore it). Any mix; none at all = never placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpawnOn {
    /// Islands and hummocks.
    pub land: bool,
    /// Mud paths and boardwalks.
    pub path: bool,
    /// Shallow water within the spawn reach.
    pub shallow: bool,
    /// Deep mud within the spawn reach.
    pub mud: bool,
    /// Hidden islet floors (mobs: only islets with a mob-passable ford).
    pub hidden_room: bool,
    /// Cave maps: in natural zones / in hand-made zones (by the style zone
    /// under the spot). Swamps ignore them.
    pub natural: bool,
    pub hand_made: bool,
}

impl Default for SpawnOn {
    fn default() -> Self {
        Self { land: true, path: true, shallow: true, mud: true, hidden_room: true, natural: true, hand_made: true }
    }
}

impl SpawnOn {
    pub fn any(self) -> bool {
        self.land || self.path || self.shallow || self.mud || self.hidden_room
    }

    /// No terrain at all: every flag off.
    pub const NONE: SpawnOn =
        SpawnOn { land: false, path: false, shallow: false, mud: false, hidden_room: false, natural: false, hand_made: false };

    /// Short list for stats and MCP, e.g. "land, path".
    pub fn names(self) -> Vec<&'static str> {
        [
            (self.land, "land"),
            (self.path, "path"),
            (self.shallow, "shallow"),
            (self.mud, "mud"),
            (self.hidden_room, "hidden room"),
            (self.natural && !self.hand_made, "natural only"),
            (self.hand_made && !self.natural, "hand-made only"),
        ]
            .into_iter()
            .filter_map(|(on, n)| on.then_some(n))
            .collect()
    }
}

/// Settings of the mobs node (docs/workflow.md, step 11).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MobSettings {
    /// Money for all mobs on the level; the generator spends as much of it
    /// as it can.
    pub budget: f32,
    /// Absolute cap on the number of mobs, on top of the budget.
    pub max_entities: u32,
    /// How the minimum is set: a total count or a density.
    pub min_amount: NodeAmount,
    /// Fewest mobs on the level (Count mode). Beats the budget (buying goes
    /// over it) and the spacing (it relaxes); never beats `max_entities`.
    pub min_entities: u32,
    /// Fewest mobs per 100 m² of walkable ground (Density mode).
    pub min_per_100m2: f32,
    pub types: Vec<MobType>,
    /// `combinations[a][b]`: types a and b may share a spawn point. Kept
    /// symmetric by the UI; a missing entry counts as allowed and a type
    /// always combines with itself.
    pub combinations: Vec<Vec<bool>>,
    /// Mobs at one spawn point (a pack), rolled per spawn between min and max.
    pub pack_min: u32,
    pub pack_max: u32,
    /// Smallest distance between two spawn points, meters.
    pub spacing_m: f32,
    /// Whether spawn points may stand on roads.
    pub on_roads: bool,
    /// No spawn closer than this to a gate, meters (gates are where the
    /// player arrives).
    pub gate_clearance_m: f32,
    /// Swamp maps: chance, %, that a dead-end island gets a guard pack
    /// before the rest are spread (the reward at the end is guarded).
    pub dead_end_guard_pct: f32,
    /// Swamp maps: share of the mobs, %, that must stand on hub islands
    /// (the big islands); the rest go anywhere. 0 = no rule.
    pub hub_pct: f32,
    /// Mob placement has its own random stream, seeded by the level seed
    /// and this number: "Re-roll mobs" changes it, so the mobs move while
    /// the map stays (and a full Generate gives the same mobs again).
    pub reroll: u64,
}

impl Default for MobSettings {
    fn default() -> Self {
        Self {
            budget: 400.0,
            max_entities: 60,
            min_amount: NodeAmount::Count,
            min_entities: 0,
            min_per_100m2: 0.0,
            types: vec![
                MobType { balance_pct: 70.0, ..MobType::default() },
                MobType { name: "Orc".to_owned(), price: 25.0, size_m: 1.2, balance_pct: 30.0, color: [150, 40, 90], ..MobType::default() },
            ],
            combinations: vec![vec![true, true], vec![true, true]],
            pack_min: 1,
            pack_max: 4,
            spacing_m: 5.0,
            on_roads: true,
            gate_clearance_m: 8.0,
            dead_end_guard_pct: 50.0,
            hub_pct: 0.0,
            reroll: 0,
        }
    }
}

impl MobSettings {
    /// Whether types `a` and `b` may share a spawn point.
    pub fn combines(&self, a: usize, b: usize) -> bool {
        a == b || self.combinations.get(a).and_then(|row| row.get(b)).copied().unwrap_or(true)
    }

    /// Set whether `a` and `b` combine (both directions).
    pub fn set_combines(&mut self, a: usize, b: usize, allowed: bool) {
        self.fit_matrix();
        if a < self.types.len() && b < self.types.len() && a != b {
            self.combinations[a][b] = allowed;
            self.combinations[b][a] = allowed;
        }
    }

    /// Size the combination matrix to the types (new entries allowed).
    pub fn fit_matrix(&mut self) {
        let n = self.types.len();
        self.combinations.resize(n, vec![true; n]);
        for row in &mut self.combinations {
            row.resize(n, true);
        }
    }

    /// Add a type; it combines with every other type.
    pub fn add_type(&mut self, mob: MobType) {
        self.fit_matrix();
        self.types.push(mob);
        self.fit_matrix();
    }

    /// Remove a type and its row/column of combinations.
    pub fn remove_type(&mut self, i: usize) {
        if i >= self.types.len() {
            return;
        }
        self.fit_matrix();
        self.types.remove(i);
        self.combinations.remove(i);
        for row in &mut self.combinations {
            row.remove(i);
        }
    }
}

/// Where hidden rooms are dug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HiddenRoomPlace {
    /// Beyond the outer wall, in the unused ground around the map's area;
    /// the access path tunnels through the wall.
    OutsideWall,
    /// On free ground inside the area, walled in, the path a short doorway.
    Inside,
    /// Outside where it fits, inside otherwise.
    Either,
}

impl HiddenRoomPlace {
    pub const ALL: [HiddenRoomPlace; 3] = [HiddenRoomPlace::OutsideWall, HiddenRoomPlace::Inside, HiddenRoomPlace::Either];

    pub fn label(self) -> &'static str {
        match self {
            HiddenRoomPlace::OutsideWall => "Outside the wall",
            HiddenRoomPlace::Inside => "Inside",
            HiddenRoomPlace::Either => "Either",
        }
    }
}

/// Settings of the hidden rooms node (docs/workflow.md, step 10).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HiddenRoomSettings {
    /// How many hidden rooms to place.
    pub count: u32,
    /// Room side in meters (inside its walls), width and depth rolled
    /// independently between min and max.
    pub size_min_m: f32,
    pub size_max_m: f32,
    /// Thickness of the room's own wall, meters (at least one cell).
    pub wall_thickness_m: f32,
    /// Width of the access path, meters — at least one cell, so the player
    /// always fits.
    pub path_width_m: f32,
    /// Longest access path, meters: a room farther from walkable ground is
    /// not placed there.
    pub path_max_m: f32,
    /// Where rooms are dug.
    pub place: HiddenRoomPlace,
    /// Chance, %, that a room's path is passable for mobs too (0 = player
    /// only everywhere, 100 = everyone everywhere).
    pub mob_passable_pct: f32,
    /// Room floor color on map.png, the preview and the legend.
    pub color_room: [u8; 3],
    /// Access path color.
    pub color_path: [u8; 3],
}

impl Default for HiddenRoomSettings {
    fn default() -> Self {
        Self {
            count: 2,
            size_min_m: 3.0,
            size_max_m: 6.0,
            wall_thickness_m: 0.5,
            path_width_m: 1.0,
            path_max_m: 12.0,
            place: HiddenRoomPlace::Either,
            mob_passable_pct: 0.0,
            color_room: [150, 90, 190],
            color_path: [190, 140, 220],
        }
    }
}

/// Where the groups of an interactive type go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveSpot {
    /// Any free ground.
    Anywhere,
    /// Groups start next to an obstacle: an outer or region wall, a house.
    AlongWalls,
    /// Groups start away from every obstacle and road (meadows, squares).
    OpenGround,
}

impl InteractiveSpot {
    pub const ALL: [InteractiveSpot; 3] =
        [InteractiveSpot::Anywhere, InteractiveSpot::AlongWalls, InteractiveSpot::OpenGround];

    pub fn label(self) -> &'static str {
        match self {
            InteractiveSpot::Anywhere => "Anywhere",
            InteractiveSpot::AlongWalls => "Along walls",
            InteractiveSpot::OpenGround => "Open ground",
        }
    }
}

/// One interactive object type of the interactive node (docs/workflow.md,
/// step 9): e.g. a plant that can be gathered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InteractiveType {
    /// Display name (map legend, spec.json).
    pub name: String,
    /// Object side in meters, rolled per object between min and max.
    pub size_min_m: f32,
    pub size_max_m: f32,
    /// Total objects of this type on the map.
    pub quantity: u32,
    /// Objects in one group, rolled per group between min and max.
    pub group_min: u32,
    pub group_max: u32,
    /// How far, meters, the members of a group may stand from its center.
    pub spread_m: f32,
    /// Where the groups go.
    pub spot: InteractiveSpot,
    /// Color on map.png, the preview and the legend.
    pub color: [u8; 3],
    /// Swamp maps: the terrains the groups may stand on.
    pub spawn_on: SpawnOn,
    /// Swamp maps: objects in shallow water / deep mud stay at most this far
    /// from an island, meters (herbs at the water's edge). 0 = no limit.
    pub shore_dist_m: f32,
}

impl Default for InteractiveType {
    fn default() -> Self {
        Self {
            name: "Herb".to_owned(),
            size_min_m: 0.3,
            size_max_m: 0.5,
            quantity: 30,
            group_min: 2,
            group_max: 5,
            spread_m: 2.0,
            spot: InteractiveSpot::Anywhere,
            color: [120, 200, 80],
            spawn_on: SpawnOn::default(),
            shore_dist_m: 0.0,
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

/// How an amount is set: a total count or a density (crossroad nodes,
/// minimum mobs).
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
            width_m: 200.0,
            height_m: 200.0,
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
        let config = LevelConfig::test_64();
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
