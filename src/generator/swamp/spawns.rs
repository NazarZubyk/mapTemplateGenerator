//! Swamp nodes 7–10, spawns (step S6 in docs/swamp-plan.md): props,
//! interactive objects, hidden rooms and mobs on swamp ground. The town
//! nodes' cores do the placing (`interactive::place_types`,
//! `hidden_rooms::place_rooms`, `mobs::spend`); this module only works out
//! where things may go. Props get their own swamp spots here.
//!
//! - **Land** = island ground (hummocks too). **Water** = shallow water and
//!   deep mud whose breath cost is within the spawn reach (depth node).
//!   Deep water and thicket never get spawns.
//! - Solid objects keep off mud paths, boardwalks and trees, keep one free
//!   cell (Chebyshev) from every blocker (deep water, thicket, trees, other
//!   objects) and stay clear of the gates — the walkable ground stays one
//!   piece.
//! - Props: shares near trees / on the shore / in the water / beside paths,
//!   the rest scattered over the islands.
//! - Interactive types: `terrain` land / water / both / shore water.
//! - Hidden rooms: islets in thicket ("outside") or deep water ("inside"),
//!   no walls, one ford no longer than the wading limit.
//! - Mobs: per-type habitat; dead-end islands may get a guard pack first.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::hidden_rooms::{self, HiddenRoomBuild, RoomArea};
use crate::generator::interactive::{self, Area, InteractiveBuild, TerrainMasks};
use crate::generator::mobs::{self, HabitatMasks, MobArea, MobBuild};
use crate::generator::placement::{dilate, footprint};
use crate::generator::props::{PropBuild, PROP_NAME, TRIES};
use crate::generator::Stage;
use crate::level::{
    Category, CellMask, DepthLayout, GateAnchor, HiddenRoomLayout, IslandLayout, PathLayout, PlacedObject, PropLayout,
    TreeLayout,
};
use crate::model::LevelConfig;

/// Shoreline half-width for land + water mob packs, meters.
const SHORE_M: f32 = 2.0;

/// The swamp after the layout nodes and the trees: what every spawn node
/// starts from.
pub struct SwampGround<'a> {
    pub w: usize,
    pub h: usize,
    pub cell: f32,
    pub land: Vec<bool>,
    /// Shallow water and deep mud within the spawn reach.
    pub water: Vec<bool>,
    /// Mud path and boardwalk cells.
    pub path: Vec<bool>,
    /// Walkable terrain (no deep water, no thicket); objects not counted.
    pub walkable: Vec<bool>,
    pub islands: &'a IslandLayout,
    pub paths: &'a PathLayout,
    pub depth: &'a DepthLayout,
    pub trees: &'a TreeLayout,
    pub gates: Vec<[f32; 2]>,
}

impl<'a> SwampGround<'a> {
    pub fn new(
        config: &LevelConfig,
        anchors: &[GateAnchor],
        islands: &'a IslandLayout,
        paths: &'a PathLayout,
        depth: &'a DepthLayout,
        trees: &'a TreeLayout,
    ) -> Self {
        let (w, h, cell) = (islands.width, islands.height, islands.cell_size_m);
        let limit = config.swamp.depth.wade_limit_m.max(cell);
        // The depth value is 1 + breath / limit · 253 in walkable water.
        let reach_depth = 1.0 + depth.reach_m / limit * 253.0;
        Self {
            w,
            h,
            cell,
            land: islands.ids.iter().map(|&i| i > 0).collect(),
            water: (0..w * h)
                .map(|c| (depth.shallow.cells[c] || depth.mud.cells[c]) && depth.depth[c] as f32 <= reach_depth + 1e-3)
                .collect(),
            path: (0..w * h).map(|c| paths.mud.cells[c] || paths.boardwalk.cells[c]).collect(),
            walkable: (0..w * h).map(|c| !depth.deep.cells[c] && !depth.thicket.cells[c]).collect(),
            islands,
            paths,
            depth,
            trees,
            gates: anchors.iter().map(|a| a.point_m).collect(),
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
                self.gates.iter().any(|g| (g[0] - x).hypot(g[1] - y) < r)
            })
            .collect()
    }

    /// Blockers: deep water, thicket, trees and the given object masks.
    fn blockers(&self, objects: &[&CellMask]) -> Vec<bool> {
        (0..self.w * self.h)
            .map(|c| !self.walkable[c] || self.trees.mask.cells[c] || objects.iter().any(|m| m.cells[c]))
            .collect()
    }

    /// Distance from the nearest land cell, meters (8 directions).
    fn to_land_m(&self) -> Vec<f32> {
        let (w, h) = (self.w, self.h);
        let mut dist = vec![f32::INFINITY; w * h];
        let mut queue = std::collections::VecDeque::new();
        for (c, d) in dist.iter_mut().enumerate() {
            if self.land[c] {
                *d = 0.0;
                queue.push_back(c);
            }
        }
        // Label-correcting search: a cell re-enters the queue whenever its
        // distance drops.
        while let Some(c) = queue.pop_front() {
            let (x, y) = ((c % w) as i64, (c / w) as i64);
            for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                    continue;
                }
                let n = ny as usize * w + nx as usize;
                let step = if dx != 0 && dy != 0 { std::f32::consts::SQRT_2 } else { 1.0 } * self.cell;
                if dist[c] + step + 1e-4 < dist[n] {
                    dist[n] = dist[c] + step;
                    queue.push_back(n);
                }
            }
        }
        dist
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    NearTrees,
    Shore,
    InWater,
    PathEdge,
    Scattered,
}

/// Swamp props: quantity capped by density, split into the swamp shares.
pub fn props(config: &LevelConfig, rng: &mut ChaCha8Rng, g: &SwampGround) -> PropBuild {
    let s = &config.props;
    let (w, h, cell) = (g.w, g.h, g.cell);
    let lo = s.size_min_m.min(s.size_max_m).max(cell);
    let hi = s.size_min_m.max(s.size_max_m).max(cell);
    let near_gate = g.near_gates(config.swamp.trees.gate_clear_m.max(0.0));
    let ground: Vec<bool> = (0..w * h)
        .map(|c| (g.land[c] || g.water[c]) && !g.path[c] && !g.trees.mask.cells[c] && !near_gate[c])
        .collect();
    let ground_cells = ground.iter().filter(|&&v| v).count();
    let near_blocker = dilate(&g.blockers(&[]), w, h, 1);
    let mut free: Vec<bool> = (0..w * h).map(|c| ground[c] && !near_blocker[c]).collect();

    let avg_cells = ((lo + hi) / 2.0 / cell).powi(2).max(1.0);
    let density_cap = (s.density_pct.clamp(0.0, 100.0) / 100.0 * ground_cells as f32 / avg_cells).floor() as u32;
    let wanted = s.quantity.min(density_cap);

    // Spots per mode, and the terrain a prop of that mode stands on.
    let near_trees = dilate(&g.trees.mask.cells, w, h, 2);
    let not_land: Vec<bool> = g.land.iter().map(|&l| !l).collect();
    let near_water = dilate(&not_land, w, h, 2);
    let near_path = dilate(&g.path, w, h, 2);
    let spot = |mode: Mode, c: usize| match mode {
        Mode::NearTrees => g.land[c] && near_trees[c],
        Mode::Shore => g.land[c] && near_water[c],
        Mode::InWater => g.water[c],
        Mode::PathEdge => near_path[c] && (g.land[c] || g.water[c]),
        Mode::Scattered => g.land[c],
    };
    let terrain = |mode: Mode, c: usize| match mode {
        Mode::InWater => g.water[c],
        Mode::PathEdge => g.land[c] || g.water[c],
        _ => g.land[c],
    };
    let shares = [
        (Mode::NearTrees, s.near_trees_pct),
        (Mode::Shore, s.shore_pct),
        (Mode::InWater, s.in_water_pct),
        (Mode::PathEdge, s.path_edge_pct),
    ];
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
    let mut pools: Vec<Option<Vec<usize>>> = vec![None; 5];
    let mut counts = [0u32; 5];
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
            requested: s.quantity,
            density_cap,
            near_trees: counts[Mode::NearTrees as usize],
            shore: counts[Mode::Shore as usize],
            in_water: counts[Mode::InWater as usize],
            path_edge: counts[Mode::PathEdge as usize],
        },
        objects,
        notes,
    }
}

/// Swamp interactive objects: the town core on swamp ground, with terrain.
pub(crate) fn interactive(config: &LevelConfig, rng: &mut ChaCha8Rng, g: &SwampGround, props: &CellMask, stage: &Stage) -> InteractiveBuild {
    let (w, h, cell) = (g.w, g.h, g.cell);
    let near_gate = g.near_gates(config.swamp.trees.gate_clear_m.max(0.0));
    let blockers = g.blockers(&[props]);
    let near = dilate(&blockers, w, h, 1);
    let free = (0..w * h).map(|c| (g.land[c] || g.water[c]) && !g.path[c] && !near[c] && !near_gate[c]).collect();
    let along_walls = dilate(&blockers, w, h, interactive::ALONG_WALLS_CELLS);
    let busy: Vec<bool> = (0..w * h).map(|c| blockers[c] || g.path[c]).collect();
    let crowded = dilate(&busy, w, h, (interactive::OPEN_GROUND_M / cell).ceil().max(1.0) as i64);
    let terrain = TerrainMasks { land: g.land.clone(), water: g.water.clone(), to_land_m: g.to_land_m() };
    let area = Area { w, h, cell, free, along_walls, crowded, terrain: Some(terrain) };
    interactive::place_types(config, rng, area, stage)
}

/// Swamp hidden rooms: unwalled islets in thicket (outside) or deep water
/// (inside), each with one ford no longer than the wading limit.
pub(crate) fn hidden_rooms(config: &LevelConfig, rng: &mut ChaCha8Rng, g: &SwampGround, objects: &[&CellMask], stage: &Stage) -> HiddenRoomBuild {
    let (w, h, cell) = (g.w, g.h, g.cell);
    let object = |c: usize| g.trees.mask.cells[c] || objects.iter().any(|m| m.cells[c]);
    let walkable: Vec<bool> = (0..w * h).map(|c| g.walkable[c] && !object(c)).collect();
    let near_gate = g.near_gates(hidden_rooms::GATE_CLEARANCE_M);
    // An islet sits in deep water / thicket a cell clear of anything else.
    let not_void: Vec<bool> = g.walkable.clone();
    let near_open = dilate(&not_void, w, h, 1);
    let ok = |want: &CellMask| -> Vec<bool> { (0..w * h).map(|c| want.cells[c] && !near_open[c] && !near_gate[c]).collect() };
    let s = &config.hidden_rooms;
    let max_m = s.path_max_m.min(config.swamp.depth.wade_limit_m);
    let area = RoomArea {
        w,
        h,
        cell,
        walkable,
        outside_ok: ok(&g.depth.thicket),
        inside_ok: ok(&g.depth.deep),
        blocked: (0..w * h).map(object).collect(),
        wall_cells: 0,
        max_len: (max_m / cell).round().max(1.0) as u32,
    };
    hidden_rooms::place_rooms(config, rng, area, stage)
}

/// Swamp mobs: the town spending core on swamp ground, with habitats and
/// guard packs on dead-end islands.
pub(crate) fn mobs(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    g: &SwampGround,
    objects: &[&CellMask],
    rooms: Option<&HiddenRoomLayout>,
    stage: &Stage,
) -> MobBuild {
    let (w, h, cell) = (g.w, g.h, g.cell);
    let s = &config.mobs;
    let near_gate = g.near_gates(s.gate_clearance_m.max(0.0));
    let (room_of, room_ok) = mobs::room_cells(rooms, w, h, cell);
    let hidden = |m: fn(&HiddenRoomLayout) -> &CellMask, c: usize| rooms.is_some_and(|l| m(l).cells[c]);
    let spawnable: Vec<bool> = (0..w * h)
        .map(|c| {
            if near_gate[c] {
                return false;
            }
            if room_of[c] != 0 {
                return room_ok[c];
            }
            (g.land[c] || g.water[c] || (s.on_roads && g.path[c]))
                && !g.trees.mask.cells[c]
                && !objects.iter().any(|m| m.cells[c])
                && !hidden(|l| &l.floor_mask, c)
        })
        .collect();
    let boardwalk = &g.paths.boardwalk.cells;
    let land: Vec<bool> = (0..w * h).map(|c| spawnable[c] && (g.land[c] || boardwalk[c] || room_of[c] != 0)).collect();
    let water: Vec<bool> = (0..w * h).map(|c| spawnable[c] && !land[c]).collect();
    let r = (SHORE_M / cell).ceil() as i64;
    let (near_land, near_water) = (dilate(&land, w, h, r), dilate(&water, w, h, r));
    let shore = (0..w * h).map(|c| spawnable[c] && near_land[c] && near_water[c]).collect();

    // Guarded dead ends: the island at the far end of each dead-end link.
    let chance = s.dead_end_guard_pct.clamp(0.0, 100.0) / 100.0;
    let guards = g
        .paths
        .paths
        .iter()
        .filter(|p| p.dead_end)
        .filter(|_| rng.random::<f32>() < chance)
        .map(|p| (0..w * h).map(|c| spawnable[c] && g.islands.ids[c] == p.a).collect())
        .collect();

    let area = MobArea {
        w,
        h,
        cell,
        spawnable,
        room_of,
        road: g.path.clone(),
        habitat: Some(HabitatMasks { land, water, shore }),
        guards,
    };
    mobs::spend(config, rng, &area, stage)
}

#[cfg(test)]
mod tests {
    use crate::generator::grid;
    use crate::level::{Category, CellMask, GeneratedLevel};
    use crate::model::{GateSpec, GateStyle, Habitat, InteractiveTerrain, LevelConfig, MapKind, MobType};

    fn config() -> LevelConfig {
        let mut c = LevelConfig { kind: MapKind::Swamp, ..LevelConfig::default() };
        c.cell_size_m = 1.0;
        c.gates.gates = vec![
            GateSpec { direction_deg: 90.0, style: GateStyle::Shore },
            GateSpec { direction_deg: 270.0, style: GateStyle::Bridge },
        ];
        c.props.quantity = 80;
        c.hidden_rooms.count = 2;
        c.mobs.budget = 1500.0;
        c.mobs.max_entities = 200;
        c
    }

    fn level(c: &LevelConfig, seed: u64) -> GeneratedLevel {
        crate::generator::generate(c, seed)
    }

    struct Terrain {
        land: Vec<bool>,
        water: Vec<bool>,
        path: Vec<bool>,
        blocked: Vec<bool>,
    }

    fn terrain(l: &GeneratedLevel) -> Terrain {
        let s = l.swamp.as_ref().unwrap();
        let (isl, p, d) = (s.islands.as_ref().unwrap(), s.paths.as_ref().unwrap(), s.depth.as_ref().unwrap());
        let n = isl.ids.len();
        Terrain {
            land: isl.ids.iter().map(|&i| i > 0).collect(),
            water: (0..n).map(|k| d.shallow.cells[k] || d.mud.cells[k]).collect(),
            path: (0..n).map(|k| p.mud.cells[k] || p.boardwalk.cells[k]).collect(),
            blocked: (0..n).map(|k| d.deep.cells[k] || d.thicket.cells[k]).collect(),
        }
    }

    fn cells_of(l: &GeneratedLevel, o: &crate::level::PlacedObject) -> Vec<usize> {
        let isl = l.swamp.as_ref().unwrap().islands.as_ref().unwrap();
        let (w, cell) = (isl.width, isl.cell_size_m);
        (0..isl.ids.len())
            .filter(|&k| o.contains(((k % w) as f32 + 0.5) * cell, ((k / w) as f32 + 0.5) * cell))
            .collect()
    }

    #[test]
    fn everything_spawns_and_walkable_ground_stays_one_piece() {
        let c = config();
        for seed in 0..3 {
            let l = level(&c, seed);
            assert!(l.notes.is_empty(), "seed {seed}: {:?}", l.notes);
            assert!(l.props.as_ref().unwrap().mask.count_true() > 0);
            assert_eq!(l.interactive.as_ref().unwrap().types[0].placed, 30);
            assert_eq!(l.hidden_rooms.as_ref().unwrap().rooms.len(), 2, "seed {seed}");
            assert!(l.mobs.as_ref().unwrap().spent >= 1490.0, "seed {seed}");
            // Walkable terrain + hidden floors minus solid objects: one piece.
            let t = terrain(&l);
            let s = l.swamp.as_ref().unwrap();
            let rooms = l.hidden_rooms.as_ref().unwrap();
            let solid = [&s.trees.as_ref().unwrap().mask, &l.props.as_ref().unwrap().mask, &l.interactive.as_ref().unwrap().mask];
            let isl = s.islands.as_ref().unwrap();
            let open = CellMask {
                width: isl.width,
                height: isl.height,
                cell_size_m: isl.cell_size_m,
                cells: (0..t.land.len())
                    .map(|k| (!t.blocked[k] || rooms.floor_mask.cells[k]) && !solid.iter().any(|m| m.cells[k]))
                    .collect(),
            };
            assert!(grid::is_connected(&open), "seed {seed}: spawns split the walkable ground");
        }
    }

    #[test]
    fn objects_keep_off_paths_and_blockers() {
        let c = config();
        let l = level(&c, 1);
        let t = terrain(&l);
        for o in l.objects.iter().filter(|o| matches!(o.category, Category::Prop | Category::Interactive)) {
            for k in cells_of(&l, o) {
                assert!(!t.path[k] && !t.blocked[k], "{} on a path or in deep water", o.type_name);
                assert!(t.land[k] || t.water[k]);
            }
        }
        let p = l.props.as_ref().unwrap();
        assert!(p.near_trees > 0 && p.shore > 0 && p.in_water > 0 && p.path_edge > 0 && p.scattered > 0, "{p:?}");
    }

    #[test]
    fn interactive_terrain_is_respected() {
        let mut c = config();
        c.interactive[0].terrain = InteractiveTerrain::Water;
        let l = level(&c, 2);
        let t = terrain(&l);
        let herbs: Vec<_> = l.objects.iter().filter(|o| o.category == Category::Interactive).collect();
        assert!(!herbs.is_empty());
        for o in &herbs {
            assert!(cells_of(&l, o).iter().all(|&k| t.water[k]), "a water herb on land");
        }
        c.interactive[0].terrain = InteractiveTerrain::Land;
        let l = level(&c, 2);
        let t = terrain(&l);
        for o in l.objects.iter().filter(|o| o.category == Category::Interactive) {
            assert!(cells_of(&l, o).iter().all(|&k| t.land[k]), "a land herb in the water");
        }
    }

    #[test]
    fn mob_habitats_hold() {
        let mut c = config();
        c.mobs.types = vec![
            MobType { name: "Rat".into(), habitat: Habitat::Land, balance_pct: 50.0, ..MobType::default() },
            MobType { name: "Leech".into(), habitat: Habitat::Water, balance_pct: 50.0, color: [20, 90, 200], ..MobType::default() },
        ];
        c.mobs.combinations = vec![vec![true, false], vec![false, true]];
        c.mobs.on_roads = false;
        let l = level(&c, 3);
        let t = terrain(&l);
        let rooms = l.hidden_rooms.as_ref().unwrap();
        let mut seen = [0, 0];
        for sp in &l.mobs.as_ref().unwrap().spawns {
            if sp.hidden_room.is_some() {
                continue;
            }
            let m = &sp.members[0];
            seen[m.type_index] += 1;
            let isl = l.swamp.as_ref().unwrap().islands.as_ref().unwrap();
            for &o in &sp.objects {
                let (x, y) = l.objects[o].center();
                let center = (y / isl.cell_size_m) as usize * isl.width + (x / isl.cell_size_m) as usize;
                if m.type_index == 0 {
                    assert!(t.land[center] || rooms.floor_mask.cells[center], "a rat in the water");
                } else {
                    assert!(t.water[center], "a leech on land");
                }
            }
        }
        assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
    }

    #[test]
    fn dead_ends_get_guards() {
        let mut c = config();
        c.mobs.dead_end_guard_pct = 100.0;
        c.mobs.budget = 400.0;
        let l = level(&c, 4);
        let s = l.swamp.as_ref().unwrap();
        let isl = s.islands.as_ref().unwrap();
        let dead: Vec<u16> = s.paths.as_ref().unwrap().paths.iter().filter(|p| p.dead_end).map(|p| p.a).collect();
        let w = isl.width;
        let guarded = dead
            .iter()
            .filter(|&&id| {
                l.mobs.as_ref().unwrap().spawns.iter().any(|sp| {
                    let (x, y) = ((sp.center_m[0] / isl.cell_size_m) as usize, (sp.center_m[1] / isl.cell_size_m) as usize);
                    isl.ids[y * w + x] == id
                })
            })
            .count();
        assert_eq!(guarded, dead.len(), "{guarded} of {} dead ends guarded", dead.len());
        c.mobs.dead_end_guard_pct = 0.0;
        assert!(level(&c, 4).notes.is_empty());
    }

    #[test]
    fn hidden_islets_are_reached_by_a_short_ford() {
        let c = config();
        let l = level(&c, 0);
        let rooms = l.hidden_rooms.as_ref().unwrap();
        assert_eq!(rooms.wall_mask.count_true(), 0, "islets have no walls");
        for r in &rooms.rooms {
            assert!(!r.path_m.is_empty());
            assert!(r.path_m.len() as f32 * c.cell_size_m <= c.swamp.depth.wade_limit_m + 1.0);
        }
    }
}
