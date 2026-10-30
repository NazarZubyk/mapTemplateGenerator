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
//! - Hidden rooms (before the interactive objects): islets in thicket
//!   ("outside") or deep water ("inside"), no walls, one ford no longer than
//!   the wading limit.
//! - Interactive types: `spawn_on` terrains (land / path / shallow / mud /
//!   hidden room), water optionally within `shore_dist_m` of an island.
//! - Mobs: per-type spawn terrains (land / path / shallow / mud / hidden
//!   room); dead-end
//!   islands may get a guard pack first;
//!   `hub_pct` of the mobs stand on hub islands, the rest anywhere.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::hidden_rooms::{self, HiddenRoomBuild, RoomArea};
use crate::generator::interactive::{self, Area, InteractiveBuild, SwampTerrain};
use crate::generator::mobs::{self, MobArea, MobBuild};
use crate::generator::placement::{dilate, footprint, TerrainSets};
use crate::generator::props::{PropBuild, PROP_NAME, TRIES};
use crate::generator::Stage;
use crate::level::{
    Category, CellMask, DepthLayout, GateAnchor, HiddenRoomLayout, IslandKind, IslandLayout, PathLayout, PlacedObject,
    PropLayout, TreeLayout,
};
use crate::model::LevelConfig;

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
            near_walls: 0,
            in_tunnels: 0,
            pool_edge: 0,
        },
        objects,
        notes,
    }
}

/// Swamp interactive objects: the town core on swamp ground, with terrain.
/// Runs after the hidden rooms, so types may spawn inside them (never on
/// a ford).
pub(crate) fn interactive(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    g: &SwampGround,
    props: &CellMask,
    rooms: &HiddenRoomLayout,
    stage: &Stage,
) -> InteractiveBuild {
    let (w, h, cell) = (g.w, g.h, g.cell);
    let (room_of, _) = mobs::room_cells(Some(rooms), w, h, cell);
    let ford: Vec<bool> = (0..w * h).map(|c| rooms.floor_mask.cells[c] && room_of[c] == 0).collect();
    let near_gate = g.near_gates(config.swamp.trees.gate_clear_m.max(0.0));
    // Hidden floors and fords are walkable ground, not blockers.
    let blockers: Vec<bool> = g.blockers(&[props]).iter().zip(&rooms.floor_mask.cells).map(|(&b, &floor)| b && !floor).collect();
    let near = dilate(&blockers, w, h, 1);
    let ground = |c: usize| g.land[c] || g.water[c] || g.path[c] || room_of[c] != 0;
    let free = (0..w * h).map(|c| ground(c) && !ford[c] && !near[c] && !near_gate[c]).collect();
    let along_walls = dilate(&blockers, w, h, interactive::ALONG_WALLS_CELLS);
    let busy: Vec<bool> = (0..w * h).map(|c| blockers[c] || g.path[c] || ford[c]).collect();
    let crowded = dilate(&busy, w, h, (interactive::OPEN_GROUND_M / cell).ceil().max(1.0) as i64);
    let sets = TerrainSets {
        land: g.land.clone(),
        path: g.path.clone(),
        shallow: (0..w * h).map(|c| g.water[c] && g.depth.shallow.cells[c]).collect(),
        mud: (0..w * h).map(|c| g.water[c] && g.depth.mud.cells[c]).collect(),
        room: room_of.iter().map(|&r| r != 0).collect(),
        natural: None,
    };
    let terrain = SwampTerrain { sets, to_land_m: g.to_land_m(), room_of };
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
                return room_ok[c] && !objects.iter().any(|m| m.cells[c]);
            }
            (g.land[c] || g.water[c] || g.path[c])
                && !g.trees.mask.cells[c]
                && !objects.iter().any(|m| m.cells[c])
                && !hidden(|l| &l.floor_mask, c)
        })
        .collect();
    // Terrain per cell; open islet floors are their own terrain.
    let terrain = TerrainSets {
        land: (0..w * h).map(|c| spawnable[c] && g.land[c]).collect(),
        path: (0..w * h).map(|c| spawnable[c] && g.path[c] && room_of[c] == 0).collect(),
        shallow: (0..w * h).map(|c| spawnable[c] && g.water[c] && g.depth.shallow.cells[c]).collect(),
        mud: (0..w * h).map(|c| spawnable[c] && g.water[c] && g.depth.mud.cells[c]).collect(),
        room: (0..w * h).map(|c| spawnable[c] && room_of[c] != 0).collect(),
        natural: None,
    };

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

    // Hub island cells (only when a hub share is asked for).
    let hub = (s.hub_pct > 0.0).then(|| {
        (0..w * h)
            .map(|c| {
                let id = g.islands.ids[c];
                spawnable[c] && id > 0 && g.islands.islands[id as usize - 1].kind == IslandKind::Hub
            })
            .collect()
    });
    let area = MobArea {
        w,
        h,
        cell,
        spawnable,
        room_of,
        road: g.path.clone(),
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
    use crate::model::{GateSpec, GateStyle, InteractiveType, LevelConfig, MapKind, MobType, SpawnOn};

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
    fn interactive_types_stand_on_their_terrains() {
        let mut c = config();
        c.interactive = vec![
            InteractiveType { name: "Reed herb".into(), spawn_on: spawn_on(false, false, true, true), ..InteractiveType::default() },
            InteractiveType {
                name: "Mushroom".into(),
                spawn_on: spawn_on(true, false, false, false),
                color: [200, 120, 40],
                ..InteractiveType::default()
            },
        ];
        let l = level(&c, 2);
        let t = terrain(&l);
        let d = l.swamp.as_ref().unwrap().depth.as_ref().unwrap();
        let mut seen = [0, 0];
        for g in &l.interactive.as_ref().unwrap().groups {
            seen[g.type_index] += 1;
            for &m in &g.members {
                for k in cells_of(&l, &l.objects[m]) {
                    match g.type_index {
                        0 => assert!(d.shallow.cells[k] || d.mud.cells[k], "a reed herb out of the water"),
                        _ => assert!(t.land[k], "a mushroom off the islands"),
                    }
                }
            }
        }
        assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
        // Near island: water objects stay close to the shore.
        c.interactive[0].shore_dist_m = 3.0;
        let l = level(&c, 2);
        let isl = l.swamp.as_ref().unwrap().islands.as_ref().unwrap();
        for g in l.interactive.as_ref().unwrap().groups.iter().filter(|g| g.type_index == 0) {
            let (x, y) = (g.center_m[0] / isl.cell_size_m, g.center_m[1] / isl.cell_size_m);
            let near = (-4..=4i64).any(|dy| {
                (-4..=4i64).any(|dx| {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    nx >= 0 && ny >= 0 && isl.ids[ny as usize * isl.width + nx as usize] > 0
                })
            });
            assert!(near, "a reed herb far from every island");
        }
    }

    #[test]
    fn interactive_objects_go_inside_hidden_rooms() {
        let mut c = config();
        c.hidden_rooms.count = 3;
        c.hidden_rooms.size_min_m = 5.0;
        c.hidden_rooms.size_max_m = 7.0;
        c.interactive = vec![InteractiveType {
            name: "Chest".into(),
            quantity: 3,
            group_min: 1,
            group_max: 1,
            size_min_m: 1.0,
            size_max_m: 1.0,
            spawn_on: spawn_on_room(),
            ..InteractiveType::default()
        }];
        let l = level(&c, 1);
        let rooms = l.hidden_rooms.as_ref().unwrap();
        assert!(!rooms.rooms.is_empty());
        let chests = &l.interactive.as_ref().unwrap().types[0];
        assert!(chests.placed > 0, "{:?}", l.notes);
        let isl = l.swamp.as_ref().unwrap().islands.as_ref().unwrap();
        let (room_of, _) = crate::generator::mobs::room_cells(Some(rooms), isl.width, isl.height, isl.cell_size_m);
        for o in l.objects.iter().filter(|o| o.category == Category::Interactive) {
            for k in cells_of(&l, o) {
                assert!(room_of[k] != 0, "a chest outside a hidden room (or on its ford)");
            }
        }
        // Chests are drawn after the room floors.
        let first_chest = l.objects.iter().position(|o| o.category == Category::Interactive).unwrap();
        assert!(rooms.rooms.iter().all(|r| r.objects[0] < first_chest));
    }

    fn spawn_on(land: bool, path: bool, shallow: bool, mud: bool) -> SpawnOn {
        SpawnOn { land, path, shallow, mud, ..SpawnOn::NONE }
    }

    fn spawn_on_room() -> SpawnOn {
        SpawnOn { hidden_room: true, ..SpawnOn::NONE }
    }

    /// Terrain under every mob marker center: (land, path, shallow, mud).
    fn mob_terrain(l: &GeneratedLevel) -> Vec<(usize, [bool; 4])> {
        let s = l.swamp.as_ref().unwrap();
        let (isl, p, d) = (s.islands.as_ref().unwrap(), s.paths.as_ref().unwrap(), s.depth.as_ref().unwrap());
        let rooms = l.hidden_rooms.as_ref().unwrap();
        let mut out = Vec::new();
        for sp in &l.mobs.as_ref().unwrap().spawns {
            for (&o, m) in sp.objects.iter().zip(sp.members.iter().flat_map(|m| std::iter::repeat_n(m.type_index, m.count as usize))) {
                let (x, y) = l.objects[o].center();
                let k = (y / isl.cell_size_m) as usize * isl.width + (x / isl.cell_size_m) as usize;
                let land = isl.ids[k] > 0 || rooms.floor_mask.cells[k];
                out.push((m, [land, p.mud.cells[k] || p.boardwalk.cells[k], d.shallow.cells[k], d.mud.cells[k]]));
            }
        }
        out
    }

    #[test]
    fn mobs_spawn_only_on_their_terrains() {
        let mut c = config();
        c.mobs.types = vec![
            MobType { name: "Rat".into(), spawn_on: spawn_on(true, false, false, false), ..MobType::default() },
            MobType { name: "Leech".into(), spawn_on: spawn_on(false, false, true, true), color: [20, 90, 200], ..MobType::default() },
            MobType { name: "Bandit".into(), spawn_on: spawn_on(false, true, false, false), color: [90, 90, 20], ..MobType::default() },
        ];
        c.mobs.combinations = vec![vec![true, false, false], vec![false, true, false], vec![false, false, true]];
        c.mobs.pack_max = 1;
        c.mobs.dead_end_guard_pct = 0.0;
        let l = level(&c, 3);
        let mut seen = [0; 3];
        for (t, [land, path, shallow, mud]) in mob_terrain(&l) {
            seen[t] += 1;
            match t {
                0 => assert!(land, "a rat off land"),
                1 => assert!(shallow || mud, "a leech out of the water"),
                _ => assert!(path, "a bandit off the paths"),
            }
        }
        assert!(seen.iter().all(|&n| n > 0), "{seen:?}");
        // A type with no terrain is never placed (and a note says so).
        c.mobs.types[2].spawn_on = spawn_on(false, false, false, false);
        let l = level(&c, 3);
        assert_eq!(l.mobs.as_ref().unwrap().per_type[2], 0);
        assert!(l.notes.iter().any(|n| n.contains("'Bandit' is never placed")), "{:?}", l.notes);
    }

    #[test]
    fn hidden_room_mobs_stay_in_open_islets() {
        let mut c = config();
        c.hidden_rooms.count = 3;
        c.hidden_rooms.size_min_m = 6.0;
        c.hidden_rooms.size_max_m = 8.0;
        c.hidden_rooms.mob_passable_pct = 100.0;
        c.mobs.types = vec![MobType { name: "Hermit".into(), spawn_on: spawn_on_room(), ..MobType::default() }];
        c.mobs.pack_max = 1;
        c.mobs.spacing_m = 1.0;
        c.mobs.dead_end_guard_pct = 0.0;
        let l = level(&c, 1);
        let spawns = &l.mobs.as_ref().unwrap().spawns;
        assert!(!spawns.is_empty(), "{:?}", l.notes);
        assert!(spawns.iter().all(|sp| sp.hidden_room.is_some()), "a hermit outside the islets");
        c.hidden_rooms.mob_passable_pct = 0.0;
        assert!(level(&c, 1).mobs.as_ref().unwrap().spawns.is_empty(), "closed islets take no mobs");
    }

    #[test]
    fn mixed_packs_stand_on_the_border() {
        let mut c = config();
        c.mobs.types = vec![
            MobType { name: "Rat".into(), spawn_on: spawn_on(true, false, false, false), ..MobType::default() },
            MobType { name: "Leech".into(), spawn_on: spawn_on(false, false, true, false), color: [20, 90, 200], ..MobType::default() },
        ];
        c.mobs.pack_min = 2;
        c.mobs.pack_max = 2;
        c.mobs.dead_end_guard_pct = 0.0;
        let l = level(&c, 6);
        let isl = l.swamp.as_ref().unwrap().islands.as_ref().unwrap();
        let shallow = &l.swamp.as_ref().unwrap().depth.as_ref().unwrap().shallow;
        let mixed: Vec<_> = l.mobs.as_ref().unwrap().spawns.iter().filter(|sp| sp.members.len() == 2).collect();
        assert!(!mixed.is_empty());
        let near = |sp: &&crate::level::MobSpawn, want: &dyn Fn(usize) -> bool| {
            let (cx, cy) = ((sp.center_m[0] / isl.cell_size_m) as i64, (sp.center_m[1] / isl.cell_size_m) as i64);
            (-3..=3i64).any(|dy| (-3..=3i64).any(|dx| want((cy + dy) as usize * isl.width + (cx + dx) as usize)))
        };
        for sp in mixed {
            assert!(near(&sp, &|k| isl.ids[k] > 0) && near(&sp, &|k| shallow.cells[k]), "mixed pack far from the shore");
        }
    }

    #[test]
    fn reroll_moves_only_the_mobs() {
        let c = config();
        let base = level(&c, 2);
        let mut moved = base.clone();
        moved.config.mobs.reroll = 7;
        crate::generator::place_mobs(&mut moved, &|_, _| {});
        assert_ne!(moved.mobs, base.mobs, "re-roll moves the mobs");
        let non_mob = |l: &GeneratedLevel| l.objects.iter().filter(|o| o.category != Category::MobSpawn).cloned().collect::<Vec<_>>();
        assert_eq!(non_mob(&moved), non_mob(&base), "the rest of the map stays");
        assert_eq!(moved.swamp, base.swamp);
        // Gate markers stay last and their indexes stay right.
        for a in &moved.swamp.as_ref().unwrap().gate_anchors {
            assert_eq!(moved.objects[a.object].category, Category::Gate);
        }
        // A full generation with the same reroll gives the same mobs.
        let mut again = c.clone();
        again.mobs.reroll = 7;
        assert_eq!(level(&again, 2).mobs, moved.mobs);
        assert_eq!(level(&again, 2).objects, moved.objects);
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
    fn hub_share_puts_that_share_of_mobs_on_hubs() {
        let on_hub = |l: &GeneratedLevel| {
            let isl = l.swamp.as_ref().unwrap().islands.as_ref().unwrap();
            let m = l.mobs.as_ref().unwrap();
            let count = m
                .spawns
                .iter()
                .filter(|sp| {
                    let k = (sp.center_m[1] / isl.cell_size_m) as usize * isl.width + (sp.center_m[0] / isl.cell_size_m) as usize;
                    let id = isl.ids[k];
                    id > 0 && isl.islands[id as usize - 1].kind == crate::level::IslandKind::Hub
                })
                .map(|sp| sp.members.iter().map(|x| x.count).sum::<u32>())
                .sum::<u32>();
            (count, m.entities, m.in_hubs)
        };
        let mut c = config();
        c.mobs.dead_end_guard_pct = 0.0;
        c.mobs.hub_pct = 100.0;
        let (hub, all, counted) = on_hub(&level(&c, 5));
        assert_eq!(hub, all, "100%: every mob on a hub");
        assert_eq!(counted, all);
        c.mobs.hub_pct = 40.0;
        let l = level(&c, 5);
        let (_, all, counted) = on_hub(&l);
        let share = counted as f32 / all as f32;
        assert!((0.35..=0.5).contains(&share), "40% asked, got {share:.2}");
        assert!(l.mobs.as_ref().unwrap().spawns.iter().filter(|sp| sp.hub).count() > 0);
        c.mobs.hub_pct = 0.0;
        assert_eq!(on_hub(&level(&c, 5)).2, 0);
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
