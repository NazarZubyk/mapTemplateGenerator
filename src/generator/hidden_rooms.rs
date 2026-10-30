//! The hidden rooms node (every map kind): small walled rooms the player
//! reaches through ONE narrow access path.
//!
//! 1. **Where** — `place` picks the spot:
//!    - *outside the wall*: in the unused ground beyond the outer wall, a
//!      cell clear of it; the path tunnels through the outer wall (the wall
//!      opens there);
//!    - *inside*: on free ground (no road, a free cell from every obstacle,
//!      like props), walled in; the path is a short doorway;
//!    - *either*: outside where it fits, inside otherwise.
//! 2. **The room** — floor `size_min_m..size_max_m` per side, ringed by its
//!    own wall `wall_thickness_m` thick. Rooms keep a free cell from each
//!    other and from other rooms' paths.
//! 3. **The path** — breadth-first from the floor through the room's wall,
//!    empty ground and the outer wall to the nearest walkable cell (roads
//!    count). It never digs through houses, props, interactives, region walls
//!    or other rooms, every cell has room for the full `path_width_m` brush
//!    (≥ one cell, so the player always fits), and a spot whose path would be
//!    longer than `path_max_m` is skipped. Each room rolls
//!    `mob_passable_pct` for whether mobs may use its path.
//!
//! Neither kind comes within 6 m of a gate.
//!
//! Inside rooms sit on ground with a free ring all around, so the walkable
//! area stays one piece; outside rooms only add ground.

use std::collections::VecDeque;

use rand::seq::SliceRandom;
use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::placement::dilate;
use crate::generator::Stage;
use crate::level::{Category, CellMask, HiddenRoom, HiddenRoomLayout, PlacedObject, RegionMap, RoadNetwork};
use crate::model::{HiddenRoomPlace, LevelConfig};

pub const ROOM_NAME: &str = "hidden room";
pub const PATH_NAME: &str = "hidden room path";
/// No room block closer than this to a gate, meters — the gate's way in
/// stays clear.
pub(crate) const GATE_CLEARANCE_M: f32 = 6.0;
/// Spots tried per room (per side of the wall).
const TRIES: usize = 300;

pub struct HiddenRoomBuild {
    pub layout: HiddenRoomLayout,
    /// Room floors and path pieces; `layout.rooms[..].objects` index here.
    pub objects: Vec<PlacedObject>,
    pub notes: Vec<String>,
}

/// What the rooms are dug into: the map after every earlier node.
pub struct Ground<'a> {
    pub regions: &'a RegionMap,
    pub outer_wall: Option<&'a CellMask>,
    pub region_walls: Option<&'a CellMask>,
    pub roads: Option<&'a RoadNetwork>,
    /// Footprints of placed objects (houses, props, interactives).
    pub objects: &'a [&'a CellMask],
    /// Gate points, meters: rooms keep `GATE_CLEARANCE_M` away from them.
    pub gates: &'a [[f32; 2]],
}

/// `build_with_progress` without progress reporting.
pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, ground: &Ground) -> Option<HiddenRoomBuild> {
    build_with_progress(config, rng, ground, &Stage { progress: &|_, _| {}, from: 0.0, to: 1.0 })
}

pub(crate) fn build_with_progress(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    ground: &Ground,
    stage: &Stage,
) -> Option<HiddenRoomBuild> {
    let map = ground.regions;
    let (w, h) = (map.width, map.height);
    let cell = map.cell_size_m.max(0.05);
    let s = &config.hidden_rooms;
    let t = (s.wall_thickness_m / cell).round().max(1.0) as usize;
    let max_len = (s.path_max_m / cell).round().max(1.0) as u32;

    let in_mask = |m: Option<&CellMask>, i: usize| m.is_some_and(|m| m.cells[i]);
    let outer = |i: usize| in_mask(ground.outer_wall, i);
    let region_wall = |i: usize| in_mask(ground.region_walls, i);
    let road = |i: usize| ground.roads.is_some_and(|r| r.mask.cells[i]);
    let object = |i: usize| ground.objects.iter().any(|m| m.cells[i]);
    let walkable: Vec<bool> =
        (0..w * h).map(|i| map.ids[i] != 0 && !outer(i) && !region_wall(i) && !object(i)).collect();
    if !walkable.iter().any(|&v| v) {
        return None;
    }
    let void: Vec<bool> = (0..w * h).map(|i| map.ids[i] == 0 && !outer(i)).collect();
    // Outside: the block sits on empty ground a cell clear of everything else.
    let not_void: Vec<bool> = void.iter().map(|&v| !v).collect();
    let near_gate = |i: usize| {
        let (x, y) = (((i % w) as f32 + 0.5) * cell, ((i / w) as f32 + 0.5) * cell);
        ground.gates.iter().any(|g| (g[0] - x).powi(2) + (g[1] - y).powi(2) < GATE_CLEARANCE_M.powi(2))
    };
    let outside_ok: Vec<bool> = {
        let near = dilate(&not_void, w, h, 1);
        (0..w * h).map(|i| !near[i] && !near_gate(i)).collect()
    };
    // Inside: free ground, no road, a free cell from every obstacle.
    let obstacles: Vec<bool> = (0..w * h).map(|i| !walkable[i]).collect();
    let inside_ok: Vec<bool> = {
        let near = dilate(&obstacles, w, h, 1);
        (0..w * h).map(|i| walkable[i] && !road(i) && !near[i] && !near_gate(i)).collect()
    };
    // Cells a path may never dig through (rooms and paths join this as they
    // are placed).
    let blocked: Vec<bool> = (0..w * h).map(|i| object(i) || region_wall(i)).collect();
    let area = RoomArea { w, h, cell, walkable, outside_ok, inside_ok, blocked, wall_cells: t, max_len };
    Some(place_rooms(config, rng, area, stage))
}

/// Where hidden rooms may go, worked out by the caller (town or swamp).
pub(crate) struct RoomArea {
    pub w: usize,
    pub h: usize,
    pub cell: f32,
    /// Ground a path ends on.
    pub walkable: Vec<bool>,
    /// Cells a whole room block may cover, "outside" and "inside".
    pub outside_ok: Vec<bool>,
    pub inside_ok: Vec<bool>,
    /// Cells a path may never dig through.
    pub blocked: Vec<bool>,
    /// Thickness of the room's own wall in cells (0 = none: swamp islets).
    pub wall_cells: usize,
    /// Longest path, cells.
    pub max_len: u32,
}

/// Dig `hidden_rooms.count` rooms with one access path each on `area`.
pub(crate) fn place_rooms(config: &LevelConfig, rng: &mut ChaCha8Rng, area: RoomArea, stage: &Stage) -> HiddenRoomBuild {
    let RoomArea { w, h, cell, walkable, outside_ok, inside_ok, mut blocked, wall_cells: t, max_len } = area;
    let s = &config.hidden_rooms;
    let lo = s.size_min_m.min(s.size_max_m).max(cell);
    let hi = s.size_min_m.max(s.size_max_m).max(cell);
    let pw = (s.path_width_m / cell).round().max(1.0) as usize;
    // Rooms, paths and their rings: no later block may touch them.
    let mut taken = vec![false; w * h];

    let mut wall_mask = CellMask::new(w, h, cell);
    let mut floor_mask = CellMask::new(w, h, cell);
    let mut rooms: Vec<HiddenRoom> = Vec::new();
    let mut objects: Vec<PlacedObject> = Vec::new();
    let mut notes = Vec::new();

    for n in 0..s.count {
        stage.report("Digging hidden rooms", n as f32 / s.count.max(1) as f32);
        let id = n + 1;
        let rw = (rng.random_range(lo..=hi) / cell).round().max(1.0) as usize;
        let rh = (rng.random_range(lo..=hi) / cell).round().max(1.0) as usize;
        let mob_passable = rng.random_bool((s.mob_passable_pct.clamp(0.0, 100.0) / 100.0) as f64);
        let sides: &[bool] = match s.place {
            HiddenRoomPlace::OutsideWall => &[true],
            HiddenRoomPlace::Inside => &[false],
            HiddenRoomPlace::Either => &[true, false],
        };
        let (bw, bh) = (rw + 2 * t, rh + 2 * t);
        let mut found = None;
        let mut any_spot = false;
        'sides: for &outside in sides {
            let ok = if outside { &outside_ok } else { &inside_ok };
            let mut spots = block_spots(ok, &taken, bw, bh, w, h);
            any_spot |= !spots.is_empty();
            for _ in 0..TRIES {
                if spots.is_empty() {
                    break;
                }
                let c = spots.swap_remove(rng.random_range(0..spots.len()));
                let (x0, y0) = (c % w, c / w);
                let floor: Vec<usize> =
                    (y0 + t..y0 + t + rh).flat_map(|y| (x0 + t..x0 + t + rw).map(move |x| y * w + x)).collect();
                let in_block = |i: usize| (x0..x0 + bw).contains(&(i % w)) && (y0..y0 + bh).contains(&(i / w));
                // Shuffled sources and a rolled neighbour order, so doors
                // face any side of the room.
                let mut sources = floor.clone();
                sources.shuffle(rng);
                let turn = rng.random_range(0..4);
                let grid = PathGrid { walkable: &walkable, blocked: &blocked, taken: &taken, w, h };
                if let Some(path) = dig_path(&sources, turn, &in_block, &grid, pw, max_len) {
                    found = Some((outside, x0, y0, floor, path));
                    break 'sides;
                }
            }
        }
        let Some((outside, x0, y0, floor, path)) = found else {
            notes.push(if any_spot {
                format!("Hidden room #{id}: no spot with a path of at most {:.1} m", s.path_max_m)
            } else {
                format!(
                    "Hidden room #{id}: no free ground for a {:.1}×{:.1} m room",
                    rw as f32 * cell,
                    rh as f32 * cell
                )
            });
            continue;
        };

        // The block: wall ring + floor; then the path brush.
        let mut dug: Vec<usize> = Vec::new();
        for y in y0..y0 + bh {
            for x in x0..x0 + bw {
                wall_mask.cells[y * w + x] = true;
            }
        }
        for &c in &floor {
            wall_mask.cells[c] = false;
            floor_mask.cells[c] = true;
        }
        for &c in &path {
            for b in brush(c, pw, w, h) {
                // Dig everything not already open ground — the room's own wall
                // included (an inside room's wall stands on walkable ground).
                if (!walkable[b] || wall_mask.cells[b]) && !floor_mask.cells[b] {
                    wall_mask.cells[b] = false;
                    floor_mask.cells[b] = true;
                    dug.push(b);
                }
            }
        }
        dug.sort_unstable();
        dug.dedup();
        let mut area = vec![false; w * h];
        for y in y0..y0 + bh {
            for x in x0..x0 + bw {
                area[y * w + x] = true;
            }
        }
        for &c in &dug {
            area[c] = true;
        }
        for (i, on) in dilate(&area, w, h, 1).into_iter().enumerate() {
            taken[i] |= on;
        }
        for (i, &on) in area.iter().enumerate() {
            blocked[i] |= on;
        }

        let first = objects.len();
        let rect_m = [(x0 + t) as f32 * cell, (y0 + t) as f32 * cell, rw as f32 * cell, rh as f32 * cell];
        objects.push(PlacedObject {
            category: Category::HiddenRoom,
            type_name: ROOM_NAME.to_owned(),
            color: s.color_room,
            x_m: rect_m[0],
            y_m: rect_m[1],
            w_m: rect_m[2],
            h_m: rect_m[3],
            angle_deg: 0.0,
            parts: Vec::new(),
            round: false,
        });
        // Path pieces: one rect per horizontal run of dug cells.
        let mut k = 0;
        while k < dug.len() {
            let mut end = k + 1;
            while end < dug.len() && dug[end] == dug[end - 1] + 1 && dug[end] / w == dug[k] / w {
                end += 1;
            }
            let (x, y) = (dug[k] % w, dug[k] / w);
            objects.push(PlacedObject {
                category: Category::HiddenRoomPath,
                type_name: PATH_NAME.to_owned(),
                color: s.color_path,
                x_m: x as f32 * cell,
                y_m: y as f32 * cell,
                w_m: (end - k) as f32 * cell,
                h_m: cell,
                angle_deg: 0.0,
                parts: Vec::new(),
                round: false,
            });
            k = end;
        }
        rooms.push(HiddenRoom {
            id,
            rect_m,
            outside,
            path_m: path.iter().map(|&c| [((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell]).collect(),
            path_width_m: pw as f32 * cell,
            mob_passable,
            objects: (first..objects.len()).collect(),
        });
    }

    HiddenRoomBuild {
        layout: HiddenRoomLayout { wall_mask, floor_mask, rooms, requested: s.count },
        objects,
        notes,
    }
}

/// Every top-left cell where a `bw`×`bh` block lies wholly on `ok` cells
/// and off `taken` ones (summed-area table over the bad cells).
fn block_spots(ok: &[bool], taken: &[bool], bw: usize, bh: usize, w: usize, h: usize) -> Vec<usize> {
    if bw > w || bh > h {
        return Vec::new();
    }
    let mut sum = vec![0u32; (w + 1) * (h + 1)];
    for y in 0..h {
        for x in 0..w {
            let bad = (!ok[y * w + x] || taken[y * w + x]) as u32;
            sum[(y + 1) * (w + 1) + x + 1] = bad + sum[y * (w + 1) + x + 1] + sum[(y + 1) * (w + 1) + x] - sum[y * (w + 1) + x];
        }
    }
    let at = |x: usize, y: usize| sum[y * (w + 1) + x];
    let mut spots = Vec::new();
    for y in 0..=h - bh {
        for x in 0..=w - bw {
            if at(x + bw, y + bh) + at(x, y) == at(x + bw, y) + at(x, y + bh) {
                spots.push(y * w + x);
            }
        }
    }
    spots
}

/// The cells of a `pw`-wide square brush centered on `c` (clipped to the map).
fn brush(c: usize, pw: usize, w: usize, h: usize) -> impl Iterator<Item = usize> {
    let (x, y) = ((c % w) as i64, (c / w) as i64);
    let off = (pw as i64 - 1) / 2;
    (0..pw as i64).flat_map(move |dy| {
        (0..pw as i64).filter_map(move |dx| {
            let (nx, ny) = (x - off + dx, y - off + dy);
            (nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64).then(|| ny as usize * w + nx as usize)
        })
    })
}

/// The maps a path is dug through.
#[derive(Clone, Copy)]
struct PathGrid<'a> {
    walkable: &'a [bool],
    blocked: &'a [bool],
    taken: &'a [bool],
    w: usize,
    h: usize,
}

/// Breadth-first from the room floor to the nearest walkable cell. Returns
/// the path's centerline (room end first, without the floor and the walkable
/// cell it reaches), or `None` if no path within `max_len` cells fits.
fn dig_path(
    floor: &[usize],
    turn: usize,
    in_block: &dyn Fn(usize) -> bool,
    grid: &PathGrid,
    pw: usize,
    max_len: u32,
) -> Option<Vec<usize>> {
    let PathGrid { walkable, blocked, taken, w, h } = *grid;
    let fits = |c: usize| {
        brush(c, pw, w, h).count() == pw * pw
            && brush(c, pw, w, h).all(|b| in_block(b) || (!blocked[b] && (walkable[b] || !taken[b])))
    };
    let mut parent: Vec<usize> = vec![usize::MAX; w * h];
    let mut dist: Vec<u32> = vec![u32::MAX; w * h];
    let mut queue = VecDeque::new();
    for &c in floor {
        parent[c] = c;
        dist[c] = 0;
        queue.push_back(c);
    }
    while let Some(u) = queue.pop_front() {
        let (x, y) = (u % w, u / w);
        let mut next = [
            (y > 0).then(|| u - w),
            (x + 1 < w).then(|| u + 1),
            (y + 1 < h).then(|| u + w),
            (x > 0).then(|| u - 1),
        ];
        next.rotate_left(turn);
        for v in next.into_iter().flatten() {
            if dist[v] != u32::MAX {
                continue;
            }
            if walkable[v] && !blocked[v] && !in_block(v) {
                // Reached the map: walk back to the floor.
                let mut path = Vec::new();
                let mut c = u;
                while dist[c] > 0 {
                    path.push(c);
                    c = parent[c];
                }
                path.reverse();
                return Some(path);
            }
            if dist[u] + 1 > max_len || !fits(v) {
                continue;
            }
            parent[v] = u;
            dist[v] = dist[u] + 1;
            queue.push_back(v);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::grid;
    use crate::level::GeneratedLevel;
    use crate::model::{MapKind, ShapeSettings};

    fn level(config: &LevelConfig, seed: u64) -> GeneratedLevel {
        crate::generator::generate(config, seed)
    }

    /// Every walkable cell after all nodes, hidden floors included.
    fn open_ground(lvl: &GeneratedLevel) -> CellMask {
        let map = &lvl.regions.as_ref().unwrap().map;
        let rooms = lvl.hidden_rooms.as_ref().unwrap();
        let blocked = [
            lvl.houses.as_ref().map(|l| &l.mask),
            lvl.props.as_ref().map(|l| &l.mask),
            lvl.interactive.as_ref().map(|l| &l.mask),
            lvl.walls.as_ref().map(|l| &l.mask),
            lvl.region_walls.as_ref().map(|l| &l.mask),
            Some(&rooms.wall_mask),
        ];
        let mut open = CellMask::new(map.width, map.height, map.cell_size_m);
        for i in 0..open.cells.len() {
            let ground = map.ids[i] != 0 || rooms.floor_mask.cells[i];
            open.cells[i] = ground && !blocked.iter().flatten().any(|m| m.cells[i]);
        }
        open
    }

    #[test]
    fn rooms_are_placed_and_reachable() {
        for seed in 1..=4u64 {
            let mut config = LevelConfig::test_64();
            config.hidden_rooms.count = 3;
            let lvl = level(&config, seed);
            let layout = lvl.hidden_rooms.as_ref().unwrap();
            assert_eq!(layout.rooms.len(), 3, "seed {seed}: {:?}", lvl.notes);
            // Floors + paths + the map are one walkable piece.
            assert!(grid::is_connected(&open_ground(&lvl)), "seed {seed}: a room is cut off");
            for r in &layout.rooms {
                assert!(!r.path_m.is_empty(), "seed {seed}: room {} has no path", r.id);
                assert_eq!(lvl.objects[r.objects[0]].category, Category::HiddenRoom);
            }
        }
    }

    #[test]
    fn outside_rooms_tunnel_through_the_wall() {
        let mut config = LevelConfig::test_64();
        config.hidden_rooms.place = HiddenRoomPlace::OutsideWall;
        config.hidden_rooms.count = 2;
        let lvl = level(&config, 3);
        let layout = lvl.hidden_rooms.as_ref().unwrap();
        let map = &lvl.regions.as_ref().unwrap().map;
        assert!(!layout.rooms.is_empty(), "{:?}", lvl.notes);
        for r in &layout.rooms {
            assert!(r.outside);
            // The floor lies beyond the town.
            let (x, y) = ((r.rect_m[0] / map.cell_size_m) as usize, (r.rect_m[1] / map.cell_size_m) as usize);
            assert_eq!(map.ids[y * map.width + x], 0);
        }
        // The outer wall is opened where a path passes.
        let wall = &lvl.walls.as_ref().unwrap().mask;
        assert!((0..wall.cells.len()).all(|i| !(wall.cells[i] && layout.floor_mask.cells[i])));
        assert!(grid::is_connected(&open_ground(&lvl)));
    }

    #[test]
    fn inside_rooms_keep_the_ground_one_piece() {
        for seed in 1..=4u64 {
            let mut config = LevelConfig::test_64();
            config.hidden_rooms.place = HiddenRoomPlace::Inside;
            config.hidden_rooms.count = 4;
            // A packed town has few big clear spots; small rooms fit anywhere.
            config.hidden_rooms.size_min_m = 1.5;
            config.hidden_rooms.size_max_m = 2.5;
            let lvl = level(&config, seed);
            let layout = lvl.hidden_rooms.as_ref().unwrap();
            assert!(!layout.rooms.is_empty(), "seed {seed}: {:?}", lvl.notes);
            assert!(layout.rooms.iter().all(|r| !r.outside));
            assert!(grid::is_connected(&open_ground(&lvl)), "seed {seed}");
        }
    }

    #[test]
    fn paths_never_dig_through_objects_or_region_walls() {
        for seed in 1..=4u64 {
            let mut config = LevelConfig::test_64();
            config.hidden_rooms.count = 4;
            config.hidden_rooms.path_width_m = 1.5;
            let lvl = level(&config, seed);
            let floor = &lvl.hidden_rooms.as_ref().unwrap().floor_mask;
            let masks = [
                lvl.houses.as_ref().map(|l| &l.mask),
                lvl.props.as_ref().map(|l| &l.mask),
                lvl.interactive.as_ref().map(|l| &l.mask),
                lvl.region_walls.as_ref().map(|l| &l.mask),
            ];
            for i in 0..floor.cells.len() {
                if floor.cells[i] {
                    assert!(!masks.iter().flatten().any(|m| m.cells[i]), "seed {seed}: path through an obstacle");
                }
            }
        }
    }

    #[test]
    fn path_width_and_mob_flag_follow_settings() {
        let mut config = LevelConfig::test_64();
        config.hidden_rooms.path_width_m = 1.5;
        config.hidden_rooms.mob_passable_pct = 100.0;
        let lvl = level(&config, 2);
        for r in &lvl.hidden_rooms.as_ref().unwrap().rooms {
            assert!(r.mob_passable);
            assert!((r.path_width_m - 1.5).abs() < 1e-4);
        }
    }

    #[test]
    fn impossible_rooms_leave_a_note() {
        let mut config = LevelConfig::test_64();
        config.hidden_rooms.size_min_m = 60.0;
        config.hidden_rooms.size_max_m = 60.0;
        let lvl = level(&config, 1);
        assert!(lvl.hidden_rooms.as_ref().unwrap().rooms.is_empty());
        assert!(lvl.notes.iter().any(|n| n.starts_with("Hidden room #1")), "{:?}", lvl.notes);
    }

    #[test]
    fn non_town_maps_get_rooms() {
        let mut config = LevelConfig::test_64();
        config.kind = MapKind::Cave;
        config.shape = ShapeSettings::for_kind(config.kind);
        let lvl = level(&config, 2);
        assert!(!lvl.hidden_rooms.as_ref().unwrap().rooms.is_empty(), "{:?}", lvl.notes);
        assert!(grid::is_connected(&open_ground(&lvl)));
    }

    #[test]
    fn same_seed_same_rooms() {
        let config = LevelConfig::test_64();
        assert_eq!(level(&config, 8).hidden_rooms, level(&config, 8).hidden_rooms);
    }
}
