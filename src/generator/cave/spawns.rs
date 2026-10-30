//! Cave nodes 7–10, spawns (step C7 in docs/cave-plan.md): props, hidden
//! rooms, interactive objects and mobs on cave ground. The shared cores do
//! the placing (`interactive::place_types`, `hidden_rooms::place_rooms`,
//! `mobs::spend`); this module works out where things may go. Props get
//! their own cave spots here.
//!
//! - **Chamber** = dry walkable chamber floor (no bridge); **tunnel** =
//!   walkable floor off the chambers; **shallow** = the pools' shallow
//!   edge. Rock, chasms and deep water never get spawns. The spawn
//!   terrains read land = chamber, path = tunnel (+ rope bridges for
//!   mobs), shallow = shallow pool; mud is unused. `SpawnOn::natural` /
//!   `hand_made` limit a type to those style zones.
//! - Solid objects keep a free cell (Chebyshev) from rock, chasms, deep
//!   water and each other, stay off water and bridges and clear of the
//!   gates — the walkable floor stays one piece.
//! - Props: shares along the walls / in tunnels / at pool edges, the rest
//!   scattered over the chambers.
//! - Hidden rooms (before the interactive objects): pockets in the rock
//!   (no walls of their own — the rock is the wall), one narrow tunnel to
//!   the floor, a **cracked wall** marker where it breaks through.
//! - Mobs: halls are the hubs (`hub_pct`); dead-end chambers (one tunnel)
//!   may get a guard pack first.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::cave::tunnels::distance_field;
use crate::generator::hidden_rooms::{self, HiddenRoomBuild, RoomArea};
use crate::generator::interactive::{self, Area, InteractiveBuild, SwampTerrain};
use crate::generator::mobs::{self, MobArea, MobBuild};
use crate::generator::placement::{dilate, footprint, TerrainSets};
use crate::generator::props::{PropBuild, PROP_NAME, TRIES};
use crate::generator::Stage;
use crate::level::{
    CarveLayout, CaveStyle, Category, CellMask, ChamberKind, ChamberLayout, HazardLayout, HiddenRoomLayout, PlacedObject,
    PropLayout, StyleLayout, TunnelEnd, TunnelLayout,
};
use crate::model::LevelConfig;

/// Props, hidden rooms and interactive objects keep this far from a gate
/// point, meters.
pub const GATE_CLEAR_M: f32 = 6.0;
/// "Along the walls" for props: floor within this of rock, meters.
const NEAR_WALL_M: f32 = 2.5;
/// "Pool edge" for props: dry floor within this of water, meters.
const NEAR_POOL_M: f32 = 2.0;
pub const CRACKED_WALL_NAME: &str = "cracked wall";
const CRACKED_WALL_COLOR: [u8; 3] = [230, 150, 60];

/// The cave after the layout nodes: what every spawn node starts from.
pub struct CaveGround<'a> {
    pub w: usize,
    pub h: usize,
    pub cell: f32,
    /// Walkable after the hazards (shallow water and bridges included).
    pub walk: Vec<bool>,
    /// Dry chamber floor, no bridge.
    pub chamber: Vec<bool>,
    /// Walkable floor off the chambers.
    pub tunnel: Vec<bool>,
    pub bridge: Vec<bool>,
    pub shallow: Vec<bool>,
    /// Natural style zone per cell.
    pub natural: Vec<bool>,
    pub chambers: &'a ChamberLayout,
    pub tunnels: &'a TunnelLayout,
    pub carve: &'a CarveLayout,
    pub hazards: &'a HazardLayout,
}

impl<'a> CaveGround<'a> {
    pub fn new(
        style: &StyleLayout,
        chambers: &'a ChamberLayout,
        tunnels: &'a TunnelLayout,
        carve: &'a CarveLayout,
        hazards: &'a HazardLayout,
    ) -> Self {
        let (w, h, cell) = (chambers.width, chambers.height, chambers.cell_size_m);
        let n = w * h;
        let walk: Vec<bool> = (0..n).map(|c| hazards.walkable(&carve.floor, c)).collect();
        let dry = |c: usize| walk[c] && !hazards.shallow.cells[c] && !hazards.bridge.cells[c];
        Self {
            w,
            h,
            cell,
            chamber: (0..n).map(|c| dry(c) && chambers.ids[c] > 0).collect(),
            tunnel: (0..n).map(|c| dry(c) && chambers.ids[c] == 0).collect(),
            bridge: hazards.bridge.cells.clone(),
            shallow: (0..n).map(|c| walk[c] && hazards.shallow.cells[c]).collect(),
            natural: (0..n).map(|c| style.style_at(c) == CaveStyle::Natural).collect(),
            walk,
            chambers,
            tunnels,
            carve,
            hazards,
        }
    }

    fn center(&self, c: usize) -> (f32, f32) {
        (((c % self.w) as f32 + 0.5) * self.cell, ((c / self.w) as f32 + 0.5) * self.cell)
    }

    /// Cells within `r` meters of a gate point.
    fn near_gates(&self, r: f32) -> Vec<bool> {
        (0..self.w * self.h)
            .map(|c| {
                let (x, y) = self.center(c);
                self.tunnels.gates.iter().any(|g| (g.point_m[0] - x).hypot(g.point_m[1] - y) < r)
            })
            .collect()
    }

    /// Blockers: everything not walkable (rock, chasms, deep water) and
    /// the given object masks.
    fn blockers(&self, objects: &[&CellMask]) -> Vec<bool> {
        (0..self.w * self.h).map(|c| !self.walk[c] || objects.iter().any(|m| m.cells[c])).collect()
    }

    /// The four spawn terrains (+ style) restricted to `ok`.
    fn sets(&self, ok: &[bool], room_of: &[u32]) -> TerrainSets {
        let n = self.w * self.h;
        TerrainSets {
            land: (0..n).map(|c| ok[c] && self.chamber[c] && room_of[c] == 0).collect(),
            path: (0..n).map(|c| ok[c] && (self.tunnel[c] || self.bridge[c]) && room_of[c] == 0).collect(),
            shallow: (0..n).map(|c| ok[c] && self.shallow[c]).collect(),
            mud: vec![false; n],
            room: (0..n).map(|c| ok[c] && room_of[c] != 0).collect(),
            natural: Some(self.natural.clone()),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    NearWalls,
    InTunnels,
    PoolEdge,
    Scattered,
}

/// Cave props: quantity capped by density, split into the cave shares.
pub fn props(config: &LevelConfig, rng: &mut ChaCha8Rng, g: &CaveGround) -> PropBuild {
    let s = &config.props;
    let (w, h, cell) = (g.w, g.h, g.cell);
    let lo = s.size_min_m.min(s.size_max_m).max(cell);
    let hi = s.size_min_m.max(s.size_max_m).max(cell);
    let near_gate = g.near_gates(GATE_CLEAR_M);
    let ground: Vec<bool> = (0..w * h).map(|c| (g.chamber[c] || g.tunnel[c]) && !near_gate[c]).collect();
    let ground_cells = ground.iter().filter(|&&v| v).count();
    // Water and bridges are walkable but no prop stands next to them on
    // the ring either (keeps bridges and pool edges passable).
    let wet: Vec<bool> = (0..w * h).map(|c| !g.walk[c] || g.shallow[c] || g.bridge[c]).collect();
    let near_blocker = dilate(&wet, w, h, 1);
    let mut free: Vec<bool> = (0..w * h).map(|c| ground[c] && !near_blocker[c]).collect();

    let avg_cells = ((lo + hi) / 2.0 / cell).powi(2).max(1.0);
    let density_cap = (s.density_pct.clamp(0.0, 100.0) / 100.0 * ground_cells as f32 / avg_cells).floor() as u32;
    let wanted = s.quantity.min(density_cap);

    let to_rock = distance_field(w, h, |c| !g.carve.floor.cells[c]);
    let near_pool = dilate(&g.hazards.shallow.cells, w, h, (NEAR_POOL_M / cell).ceil() as i64);
    let spot = |mode: Mode, c: usize| match mode {
        Mode::NearWalls => (g.chamber[c] || g.tunnel[c]) && to_rock[c] * cell <= NEAR_WALL_M,
        Mode::InTunnels => g.tunnel[c],
        Mode::PoolEdge => g.chamber[c] && near_pool[c],
        Mode::Scattered => g.chamber[c],
    };
    let terrain = |mode: Mode, c: usize| match mode {
        Mode::NearWalls => g.chamber[c] || g.tunnel[c],
        Mode::InTunnels => g.tunnel[c],
        _ => g.chamber[c],
    };
    let shares = [(Mode::NearWalls, s.near_walls_pct), (Mode::InTunnels, s.tunnel_pct), (Mode::PoolEdge, s.pool_edge_pct)];
    let sum: f32 = shares.iter().map(|&(_, p)| p.clamp(0.0, 100.0)).sum();
    let scale = if sum > 100.0 { 100.0 / sum } else { 1.0 };
    let mut plan: Vec<Mode> = Vec::new();
    for (mode, pct) in shares {
        let n = (wanted as f32 * pct.clamp(0.0, 100.0) * scale / 100.0).round() as usize;
        plan.extend(std::iter::repeat_n(mode, n.min(wanted as usize - plan.len())));
    }
    plan.extend(std::iter::repeat_n(Mode::Scattered, wanted as usize - plan.len()));

    let mut mask = CellMask::new(w, h, cell);
    let mut objects = Vec::new();
    let mut pools: Vec<Option<Vec<usize>>> = vec![None; 4];
    let mut counts = [0u32; 4];
    for mode in plan {
        let size = (rng.random_range(lo..=hi), rng.random_range(lo..=hi));
        let modes: &[Mode] = if mode == Mode::Scattered { &[Mode::Scattered] } else { &[mode, Mode::Scattered] };
        'modes: for &m in modes {
            let pool = pools[m as usize].get_or_insert_with(|| (0..w * h).filter(|&c| free[c] && spot(m, c)).collect());
            let mut tries = 0;
            while tries < TRIES && !pool.is_empty() {
                let k = rng.random_range(0..pool.len());
                let c = pool[k];
                if !free[c] {
                    pool.swap_remove(k);
                    continue;
                }
                tries += 1;
                let angle = rng.random_range(0.0..180.0);
                let center = g.center(c);
                let Some(cells) = footprint(center, size, &[], angle, w, h, cell) else { continue };
                if cells.is_empty() || !cells.iter().all(|&f| free[f] && terrain(m, f)) {
                    continue;
                }
                for &f in &cells {
                    mask.cells[f] = true;
                    let (x, y) = ((f % w) as i64, (f / w) as i64);
                    for dy in -1..=1i64 {
                        for dx in -1..=1i64 {
                            let (nx, ny) = (x + dx, y + dy);
                            if nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 {
                                free[ny as usize * w + nx as usize] = false;
                            }
                        }
                    }
                }
                objects.push(PlacedObject {
                    category: Category::Prop,
                    type_name: PROP_NAME.to_owned(),
                    color: s.color,
                    x_m: center.0 - size.0 / 2.0,
                    y_m: center.1 - size.1 / 2.0,
                    w_m: size.0,
                    h_m: size.1,
                    angle_deg: angle,
                    parts: Vec::new(),
                    round: false,
                });
                counts[m as usize] += 1;
                break 'modes;
            }
        }
    }

    let total: u32 = counts.iter().sum();
    let mut notes = Vec::new();
    if s.quantity > density_cap {
        notes.push(format!("Props: the {:.0}% density cap allows ~{density_cap} of the {} asked", s.density_pct, s.quantity));
    }
    if total < wanted {
        notes.push(format!("Props: only {total} of {wanted} fit — no free spot left for the rest"));
    }
    PropBuild {
        layout: PropLayout {
            mask,
            near_houses: 0,
            roadside: 0,
            scattered: counts[Mode::Scattered as usize],
            near_trees: 0,
            shore: 0,
            in_water: 0,
            path_edge: 0,
            near_walls: counts[Mode::NearWalls as usize],
            in_tunnels: counts[Mode::InTunnels as usize],
            pool_edge: counts[Mode::PoolEdge as usize],
            requested: s.quantity,
            density_cap,
        },
        objects,
        notes,
    }
}

/// Cave hidden rooms: pockets in the rock (the rock is their wall), each
/// with one narrow tunnel to the floor and a cracked-wall marker where it
/// breaks through.
pub(crate) fn hidden_rooms(config: &LevelConfig, rng: &mut ChaCha8Rng, g: &CaveGround, objects: &[&CellMask], stage: &Stage) -> HiddenRoomBuild {
    let (w, h, cell) = (g.w, g.h, g.cell);
    let object = |c: usize| objects.iter().any(|m| m.cells[c]);
    let walkable: Vec<bool> = (0..w * h).map(|c| g.walk[c] && !object(c)).collect();
    let near_gate = g.near_gates(GATE_CLEAR_M.max(hidden_rooms::GATE_CLEARANCE_M));
    let edge = |c: usize| {
        let (x, y) = (c % w, c / w);
        x < 2 || y < 2 || x + 2 >= w || y + 2 >= h
    };
    // A pocket sits in solid rock a cell clear of any floor.
    let near_floor = dilate(&g.carve.floor.cells, w, h, 1);
    let ok: Vec<bool> = (0..w * h).map(|c| !near_floor[c] && !near_gate[c] && !edge(c)).collect();
    let s = &config.hidden_rooms;
    let area = RoomArea {
        w,
        h,
        cell,
        walkable,
        outside_ok: ok.clone(),
        inside_ok: ok,
        // The tunnel digs rock only: never through objects, chasms, deep
        // water or the map edge.
        blocked: (0..w * h).map(|c| object(c) || g.hazards.chasm.cells[c] || g.hazards.deep.cells[c] || edge(c)).collect(),
        wall_cells: 0,
        max_len: (s.path_max_m / cell).round().max(1.0) as u32,
    };
    let mut built = hidden_rooms::place_rooms(config, rng, area, stage);

    // Cracked wall: the dug tunnel cell next to the open floor.
    let side = (s.path_width_m.max(cell) * 1.4).max(1.0);
    for room in &mut built.layout.rooms {
        let at = room.path_m.iter().rev().find_map(|p| {
            let c = ((p[1] / cell) as usize).min(h - 1) * w + ((p[0] / cell) as usize).min(w - 1);
            if g.walk[c] {
                return None;
            }
            let (x, y) = (c % w, c / w);
            let touches = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)]
                .into_iter()
                .any(|(nx, ny)| nx < w && ny < h && g.walk[ny * w + nx]);
            touches.then_some(*p)
        });
        if let Some(p) = at {
            built.objects.push(PlacedObject {
                category: Category::HiddenRoomPath,
                type_name: CRACKED_WALL_NAME.to_owned(),
                color: CRACKED_WALL_COLOR,
                x_m: p[0] - side / 2.0,
                y_m: p[1] - side / 2.0,
                w_m: side,
                h_m: side,
                angle_deg: 0.0,
                parts: Vec::new(),
                round: false,
            });
            room.objects.push(built.objects.len() - 1);
        }
    }
    built
}

/// Cave interactive objects: the shared core on cave ground, with terrain
/// and style. Runs after the hidden rooms, so types may spawn inside them
/// (never in their access tunnel).
pub(crate) fn interactive(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    g: &CaveGround,
    props: &CellMask,
    rooms: &HiddenRoomLayout,
    stage: &Stage,
) -> InteractiveBuild {
    let (w, h, cell) = (g.w, g.h, g.cell);
    let (room_of, _) = mobs::room_cells(Some(rooms), w, h, cell);
    let access: Vec<bool> = (0..w * h).map(|c| rooms.floor_mask.cells[c] && room_of[c] == 0).collect();
    let near_gate = g.near_gates(GATE_CLEAR_M);
    // Hidden floors and their tunnels are walkable, not blockers; bridges
    // are kept clear like blockers.
    let blockers: Vec<bool> =
        g.blockers(&[props]).iter().enumerate().map(|(c, &b)| (b && !rooms.floor_mask.cells[c]) || g.bridge[c]).collect();
    let near = dilate(&blockers, w, h, 1);
    let ground = |c: usize| g.chamber[c] || g.tunnel[c] || g.shallow[c] || room_of[c] != 0;
    let free: Vec<bool> = (0..w * h).map(|c| ground(c) && !access[c] && !near[c] && !near_gate[c]).collect();
    let along_walls = dilate(&blockers, w, h, interactive::ALONG_WALLS_CELLS);
    let busy: Vec<bool> = (0..w * h).map(|c| blockers[c] || g.tunnel[c] || access[c]).collect();
    let crowded = dilate(&busy, w, h, (interactive::OPEN_GROUND_M / cell).ceil().max(1.0) as i64);
    let all = vec![true; w * h];
    let sets = g.sets(&all, &room_of);
    // "Near island" for caves: distance from dry floor.
    let to_dry: Vec<f32> = distance_field(w, h, |c| g.chamber[c] || g.tunnel[c]).into_iter().map(|d| d * cell).collect();
    let terrain = SwampTerrain { sets, to_land_m: to_dry, room_of };
    let area = Area { w, h, cell, free, along_walls, crowded, terrain: Some(terrain) };
    interactive::place_types(config, rng, area, stage)
}

/// Cave mobs: the shared spending core on cave ground, halls as hubs and
/// guard packs in dead-end chambers.
pub(crate) fn mobs(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    g: &CaveGround,
    objects: &[&CellMask],
    rooms: Option<&HiddenRoomLayout>,
    stage: &Stage,
) -> MobBuild {
    let (w, h, cell) = (g.w, g.h, g.cell);
    let s = &config.mobs;
    let near_gate = g.near_gates(s.gate_clearance_m.max(0.0));
    let (room_of, room_ok) = mobs::room_cells(rooms, w, h, cell);
    let hidden_floor = |c: usize| rooms.is_some_and(|l| l.floor_mask.cells[c]);
    let spawnable: Vec<bool> = (0..w * h)
        .map(|c| {
            if near_gate[c] || objects.iter().any(|m| m.cells[c]) {
                return false;
            }
            if room_of[c] != 0 {
                return room_ok[c];
            }
            g.walk[c] && !hidden_floor(c)
        })
        .collect();
    let terrain = g.sets(&spawnable, &room_of);

    // Dead-end chambers: one tunnel (stubs not counted, gate tunnels are).
    let mut degree = vec![0u32; g.chambers.chambers.len() + 1];
    for t in &g.tunnels.tunnels {
        if t.b == TunnelEnd::Blind {
            continue;
        }
        for e in [t.a, t.b] {
            if let TunnelEnd::Chamber(id) = e {
                degree[id as usize] += 1;
            }
        }
    }
    let chance = s.dead_end_guard_pct.clamp(0.0, 100.0) / 100.0;
    let guards = g
        .chambers
        .chambers
        .iter()
        .filter(|ch| degree[ch.id as usize] == 1)
        .filter(|_| rng.random::<f32>() < chance)
        .map(|ch| (0..w * h).map(|c| spawnable[c] && g.chambers.ids[c] == ch.id).collect())
        .collect();
    // Halls are the hubs.
    let hub = (s.hub_pct > 0.0).then(|| {
        (0..w * h)
            .map(|c| {
                let id = g.chambers.ids[c];
                spawnable[c] && id > 0 && g.chambers.chambers[id as usize - 1].kind == ChamberKind::Hall
            })
            .collect()
    });
    let area = MobArea {
        w,
        h,
        cell,
        spawnable,
        room_of,
        road: (0..w * h).map(|c| g.tunnel[c] || g.bridge[c]).collect(),
        terrain: Some(terrain),
        guards,
        hub,
    };
    mobs::spend(config, rng, &area, stage)
}

#[cfg(test)]
mod tests {
    use crate::generator::grid;
    use crate::level::{Category, CellMask, GeneratedLevel};
    use crate::model::{InteractiveType, LevelConfig, MapKind, MobType, SpawnOn};

    fn config() -> LevelConfig {
        let mut c = LevelConfig { kind: MapKind::Cave, ..LevelConfig::default() };
        c.shape = crate::model::ShapeSettings::for_kind(MapKind::Cave);
        c.map.width_m = 160.0;
        c.map.height_m = 140.0;
        c.cell_size_m = 0.5;
        c.props.quantity = 80;
        c.hidden_rooms.count = 2;
        c.mobs.budget = 600.0;
        c.mobs.max_entities = 120;
        c
    }

    fn level(c: &LevelConfig, seed: u64) -> GeneratedLevel {
        crate::generator::generate(c, seed)
    }

    /// Walkable floor + hidden floors, minus solid objects.
    fn open(l: &GeneratedLevel) -> CellMask {
        let cave = l.cave.as_ref().unwrap();
        let (f, hz) = (&cave.carve.as_ref().unwrap().floor, cave.hazards.as_ref().unwrap());
        let rooms = l.hidden_rooms.as_ref().unwrap();
        let solid = [&l.props.as_ref().unwrap().mask, &l.interactive.as_ref().unwrap().mask];
        CellMask {
            cells: (0..f.cells.len())
                .map(|k| (hz.walkable(f, k) || rooms.floor_mask.cells[k]) && !solid.iter().any(|m| m.cells[k]))
                .collect(),
            ..f.clone()
        }
    }

    fn cells_of(l: &GeneratedLevel, o: &crate::level::PlacedObject) -> Vec<usize> {
        let f = &l.cave.as_ref().unwrap().carve.as_ref().unwrap().floor;
        let (w, cell) = (f.width, f.cell_size_m);
        (0..f.cells.len()).filter(|&k| o.contains(((k % w) as f32 + 0.5) * cell, ((k / w) as f32 + 0.5) * cell)).collect()
    }

    #[test]
    fn everything_spawns_and_the_floor_stays_one_piece() {
        let c = config();
        for seed in 0..3 {
            let l = level(&c, seed);
            assert!(l.props.as_ref().unwrap().mask.count_true() > 0, "seed {seed}");
            assert_eq!(l.interactive.as_ref().unwrap().types[0].placed, 30, "seed {seed}: {:?}", l.notes);
            assert_eq!(l.hidden_rooms.as_ref().unwrap().rooms.len(), 2, "seed {seed}: {:?}", l.notes);
            assert!(l.mobs.as_ref().unwrap().spent >= 590.0, "seed {seed}: {:?}", l.notes);
            assert!(grid::is_connected(&open(&l)), "seed {seed}: spawns split the floor");
            // Gate markers stay last.
            let gates = l.cave.as_ref().unwrap().tunnels.as_ref().unwrap().gates.clone();
            for g in &gates {
                assert_eq!(l.objects[g.object].category, Category::Gate);
            }
        }
    }

    #[test]
    fn props_stand_on_dry_floor_with_cave_shares() {
        let l = level(&config(), 1);
        let cave = l.cave.as_ref().unwrap();
        let (f, hz) = (&cave.carve.as_ref().unwrap().floor, cave.hazards.as_ref().unwrap());
        for o in l.objects.iter().filter(|o| o.category == Category::Prop) {
            for k in cells_of(&l, o) {
                assert!(hz.walkable(f, k) && !hz.shallow.cells[k] && !hz.bridge.cells[k], "prop off dry floor");
            }
        }
        let p = l.props.as_ref().unwrap();
        assert!(p.near_walls > 0 && p.in_tunnels > 0 && p.scattered > 0, "{p:?}");
    }

    #[test]
    fn hidden_rooms_are_pockets_in_the_rock_with_a_cracked_wall() {
        let l = level(&config(), 2);
        let f = &l.cave.as_ref().unwrap().carve.as_ref().unwrap().floor;
        let rooms = l.hidden_rooms.as_ref().unwrap();
        assert!(!rooms.rooms.is_empty());
        for r in &rooms.rooms {
            let room = &l.objects[r.objects[0]];
            for k in cells_of(&l, room) {
                assert!(!f.cells[k], "room {} on the cave floor", r.id);
            }
            assert!(
                r.objects.iter().any(|&i| l.objects[i].type_name == super::CRACKED_WALL_NAME),
                "room {} has no cracked wall",
                r.id
            );
        }
        assert_eq!(rooms.wall_mask.count_true(), 0, "the rock is the wall");
    }

    #[test]
    fn types_keep_to_their_terrain_and_style() {
        let mut c = config();
        c.interactive = vec![InteractiveType {
            name: "Ore".into(),
            spawn_on: SpawnOn { land: true, path: true, hand_made: true, ..SpawnOn::NONE },
            ..InteractiveType::default()
        }];
        c.mobs.types = vec![MobType { name: "Bat".into(), spawn_on: SpawnOn { path: true, natural: true, ..SpawnOn::NONE }, ..MobType::default() }];
        c.mobs.combinations = Vec::new();
        let l = level(&c, 3);
        let cave = l.cave.as_ref().unwrap();
        let style = cave.style.as_ref().unwrap();
        let ch = cave.chambers.as_ref().unwrap();
        for g in &l.interactive.as_ref().unwrap().groups {
            for &m in &g.members {
                for k in cells_of(&l, &l.objects[m]) {
                    assert_eq!(style.style_at(k), crate::level::CaveStyle::HandMade, "ore in a natural zone");
                }
            }
        }
        let mobs = l.mobs.as_ref().unwrap();
        assert!(!mobs.spawns.is_empty(), "{:?}", l.notes);
        for sp in &mobs.spawns {
            let k = (sp.center_m[1] / 0.5) as usize * style.width + (sp.center_m[0] / 0.5) as usize;
            assert_eq!(style.style_at(k), crate::level::CaveStyle::Natural, "bat in a hand-made zone");
            assert_eq!(ch.ids[k], 0, "bat in a chamber, not a tunnel");
        }
    }

    #[test]
    fn hub_share_puts_mobs_in_halls() {
        let mut c = config();
        c.mobs.hub_pct = 100.0;
        c.mobs.dead_end_guard_pct = 0.0;
        let l = level(&c, 4);
        let m = l.mobs.as_ref().unwrap();
        assert!(m.in_hubs > 0 && m.in_hubs == m.entities, "{} of {} in hubs: {:?}", m.in_hubs, m.entities, l.notes);
    }

    #[test]
    fn same_seed_same_spawns() {
        let c = config();
        assert_eq!(level(&c, 5).objects, level(&c, 5).objects);
    }
}
