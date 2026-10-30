//! The gates node (every map kind): entry/exit points of the level — where
//! the game spawns the player or teleports between levels.
//!
//! Runs right after the region walls, so houses, props and the rest are
//! placed around the gate roads, and mobs keep away from the gates.
//!
//! 1. **Where** — a ray from the map center at the gate's direction (0° =
//!    east, 90° = north, counterclockwise on screen). The gate sits on the
//!    OUTERMOST wall crossing whose inner side is town ground. A ray that
//!    meets no such wall (it slips between islands, say) is turned 1°, 2°, …
//!    up to 45° each way; maps without an outer wall use the outermost town
//!    cell instead.
//! 2. **Opening** — outer wall cells within `opening_m / 2` of the crossing
//!    become town ground of the region behind them; the gate point is the
//!    middle of the crossing.
//! 3. **Road** — with `road` set, a breadth-first path from the gate through
//!    town ground (never through region walls) to the nearest road, stamped
//!    `road_width_m` wide into the road network as one more link.
//!
//! No randomness: gates depend only on the map.

use std::collections::VecDeque;

use crate::generator::region_walls::recount_stats;
use crate::generator::roads::stamp_path;
use crate::level::{
    Category, CellMask, GateInfo, GateLayout, OuterWall, PlacedObject, RegionSplit, RoadLink, RoadNetwork, RoadNode,
};
use crate::model::LevelConfig;

pub const GATE_NAME: &str = "gate";
/// Gate marker diameter, meters.
pub const MARKER_M: f32 = 1.0;
/// Farthest a gate is turned from its asked direction, degrees.
const MAX_TURN_DEG: i32 = 45;

pub struct GateBuild {
    pub layout: GateLayout,
    /// One marker per gate; `layout.gates[..].object` indexes here.
    pub objects: Vec<PlacedObject>,
    pub notes: Vec<String>,
}

/// Place the gates, cutting the openings into `walls` (and the region map)
/// and adding the gate roads to `roads`.
pub fn build(
    config: &LevelConfig,
    split: &mut RegionSplit,
    walls: Option<&mut OuterWall>,
    region_walls: Option<&CellMask>,
    roads: Option<&mut RoadNetwork>,
) -> GateBuild {
    let s = &config.gates;
    let (w, h) = (split.map.width, split.map.height);
    let cell = split.map.cell_size_m.max(0.05);
    let mut walls = walls;
    let mut roads = roads;
    let mut layout = GateLayout { gates: Vec::new(), requested: s.gates.len() as u32 };
    let mut objects = Vec::new();
    let mut notes = Vec::new();
    let center = (w as f32 * cell / 2.0, h as f32 * cell / 2.0);

    for (i, spec) in s.gates.iter().enumerate() {
        let id = i as u32 + 1;
        let wall_mask = walls.as_deref().map(|wl| &wl.mask);
        let found = (0..=MAX_TURN_DEG)
            .flat_map(|d| if d == 0 { vec![0] } else { vec![d, -d] })
            .find_map(|turn| {
                let deg = spec.direction_deg + turn as f32;
                crossing(&ray_cells(center, deg, w, h, cell), &split.map.ids, wall_mask).map(|c| (deg, c))
            });
        let Some((deg, cross)) = found else {
            notes.push(format!("Gate #{id}: no town edge within {MAX_TURN_DEG}° of {:.0}°", spec.direction_deg));
            continue;
        };
        let region = split.map.ids[cross.inner];
        let at = |c: usize| (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
        let (p0, p1) = (at(cross.inner), at(cross.outer));
        let point = ((p0.0 + p1.0) / 2.0, (p0.1 + p1.1) / 2.0);

        // 2. The opening.
        let mut opening_m = 0.0;
        if cross.through_wall
            && let Some(wl) = walls.as_deref_mut()
        {
            // Wall cells near the crossing, carved only where they connect
            // to it (not a nearby, unrelated stretch of wall).
            let half = s.opening_m.max(cell) / 2.0;
            let near: Vec<bool> =
                (0..w * h).map(|c| wl.mask.cells[c] && segment_distance(at(c), p0, p1) <= half).collect();
            for c in flood(&[cross.inner], |c| near[c], w, h) {
                if near[c] {
                    wl.mask.cells[c] = false;
                    split.map.ids[c] = region;
                }
            }
            opening_m = s.opening_m.max(cell);
        }
        let gate_cell = (point.1 / cell) as usize * w + (point.0 / cell) as usize;

        // 3. The road.
        let mut road_link = None;
        if s.road
            && let Some(net) = roads.as_deref_mut()
        {
            match road_path(gate_cell, &split.map.ids, region_walls, &net.mask, w, h) {
                Some(path) => {
                    let width_cells = (s.road_width_m / cell).round().max(1.0) as u32;
                    // Town ground only: never into the walls.
                    let town: Vec<bool> = (0..w * h)
                        .map(|c| split.map.ids[c] != 0 && !region_walls.is_some_and(|m| m.cells[c]))
                        .collect();
                    let before = net.mask.clone();
                    stamp_path(&mut net.mask, &town, &path, width_cells as f32 / 2.0);
                    // The round brush can reach across a thin wall: keep only
                    // the new cells joined to the gate road.
                    let joined = flood(&path, |c| net.mask.cells[c], w, h);
                    let mut keep = vec![false; w * h];
                    for c in joined {
                        keep[c] = true;
                    }
                    for ((cell, &was), &kept) in net.mask.cells.iter_mut().zip(&before.cells).zip(&keep) {
                        *cell &= was || kept;
                    }
                    let join = *path.last().unwrap();
                    let (jx, jy) = ((join % w) as i64, (join / w) as i64);
                    let nearest = (0..net.nodes.len())
                        .min_by_key(|&n| (net.nodes[n].x as i64 - jx).pow(2) + (net.nodes[n].y as i64 - jy).pow(2));
                    net.nodes.push(RoadNode { x: gate_cell % w, y: gate_cell / w, region });
                    let a = net.nodes.len() - 1;
                    net.links.push(RoadLink { a, b: nearest.unwrap_or(a), path, extra: false, width_cells });
                    road_link = Some(net.links.len() - 1);
                }
                None => notes.push(format!("Gate #{id}: no road reachable from the gate inside its region")),
            }
        }

        objects.push(PlacedObject {
            category: Category::Gate,
            type_name: GATE_NAME.to_owned(),
            color: s.color,
            x_m: point.0 - MARKER_M / 2.0,
            y_m: point.1 - MARKER_M / 2.0,
            w_m: MARKER_M,
            h_m: MARKER_M,
            angle_deg: 0.0,
            parts: Vec::new(),
        });
        layout.gates.push(GateInfo {
            id,
            direction_deg: spec.direction_deg,
            actual_deg: deg.rem_euclid(360.0),
            point_m: [point.0, point.1],
            region,
            opening_m,
            road_link,
            object: objects.len() - 1,
        });
    }
    recount_stats(split);
    GateBuild { layout, objects, notes }
}

/// The cells under a ray from `center` at `deg` (0 = east, 90 = north) to
/// the map edge, in order, without repeats.
fn ray_cells(center: (f32, f32), deg: f32, w: usize, h: usize, cell: f32) -> Vec<usize> {
    let (dx, dy) = (deg.to_radians().cos(), -deg.to_radians().sin());
    let step = cell / 4.0;
    let mut out: Vec<usize> = Vec::new();
    let mut t = 0.0;
    loop {
        let (x, y) = (center.0 + dx * t, center.1 + dy * t);
        if x < 0.0 || y < 0.0 || x >= w as f32 * cell || y >= h as f32 * cell {
            break;
        }
        let c = (y / cell) as usize * w + (x / cell) as usize;
        if out.last() != Some(&c) {
            out.push(c);
        }
        t += step;
    }
    out
}

/// Where a ray leaves the town.
struct Crossing {
    /// Last town cell before the edge.
    inner: usize,
    /// First cell past the wall (or past the town edge).
    outer: usize,
    through_wall: bool,
}

/// The outermost wall crossing along `ray` with town ground on its inner
/// side; without walls on the ray, the outermost town cell.
fn crossing(ray: &[usize], ids: &[u8], wall: Option<&CellMask>) -> Option<Crossing> {
    let is_wall = |c: usize| wall.is_some_and(|m| m.cells[c]);
    let mut k = ray.len();
    while k > 0 {
        k -= 1;
        if !is_wall(ray[k]) {
            continue;
        }
        let mut j = k;
        while j > 0 && is_wall(ray[j - 1]) {
            j -= 1;
        }
        if j > 0 && ids[ray[j - 1]] != 0 && k + 1 < ray.len() {
            return Some(Crossing { inner: ray[j - 1], outer: ray[k + 1], through_wall: true });
        }
        k = j;
    }
    if wall.is_some_and(|m| m.cells.iter().any(|&v| v)) {
        return None;
    }
    let last = ray.iter().rposition(|&c| ids[c] != 0)?;
    Some(Crossing { inner: ray[last], outer: ray[(last + 1).min(ray.len() - 1)], through_wall: false })
}

/// Every cell reachable (4 neighbours) from `starts` through cells where
/// `open` holds; the starts themselves are always included.
fn flood(starts: &[usize], open: impl Fn(usize) -> bool, w: usize, h: usize) -> Vec<usize> {
    let mut seen = vec![false; w * h];
    let mut out: Vec<usize> = Vec::new();
    let mut queue: VecDeque<usize> = VecDeque::new();
    for &c in starts {
        if !seen[c] {
            seen[c] = true;
            queue.push_back(c);
        }
    }
    while let Some(u) = queue.pop_front() {
        out.push(u);
        let (x, y) = (u % w, u / w);
        let next = [(y > 0).then(|| u - w), (x + 1 < w).then(|| u + 1), (y + 1 < h).then(|| u + w), (x > 0).then(|| u - 1)];
        for v in next.into_iter().flatten() {
            if !seen[v] && open(v) {
                seen[v] = true;
                queue.push_back(v);
            }
        }
    }
    out
}

fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let len2 = abx * abx + aby * aby;
    let t = if len2 > 0.0 { (((p.0 - a.0) * abx + (p.1 - a.1) * aby) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (qx, qy) = (a.0 + abx * t, a.1 + aby * t);
    ((p.0 - qx).powi(2) + (p.1 - qy).powi(2)).sqrt()
}

/// Breadth-first (8 neighbours, no corner cutting) from the gate through
/// town ground, never through a region wall, to the nearest road cell. The path runs from the
/// gate to that road cell.
fn road_path(
    start: usize,
    ids: &[u8],
    region_walls: Option<&CellMask>,
    road: &CellMask,
    w: usize,
    h: usize,
) -> Option<Vec<usize>> {
    let open = |c: usize| ids[c] != 0 && !region_walls.is_some_and(|m| m.cells[c]);
    if !open(start) {
        return None;
    }
    let mut parent = vec![usize::MAX; w * h];
    parent[start] = start;
    let mut queue = VecDeque::from([start]);
    while let Some(u) = queue.pop_front() {
        if road.cells[u] {
            let mut path = vec![u];
            let mut c = u;
            while c != start {
                c = parent[c];
                path.push(c);
            }
            path.reverse();
            return Some(path);
        }
        let (x, y) = ((u % w) as i64, (u / w) as i64);
        for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0), (1, -1), (1, 1), (-1, 1), (-1, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                continue;
            }
            let v = ny as usize * w + nx as usize;
            // A diagonal step needs both side cells open: no squeezing
            // between two wall corners.
            let corner_ok = dx == 0 || dy == 0 || (open(y as usize * w + nx as usize) && open(ny as usize * w + x as usize));
            if parent[v] == usize::MAX && open(v) && corner_ok {
                parent[v] = u;
                queue.push_back(v);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::grid;
    use crate::level::GeneratedLevel;
    use crate::model::{GateSpec, MapKind, ShapeSettings};

    fn level(config: &LevelConfig, seed: u64) -> GeneratedLevel {
        crate::generator::generate(config, seed)
    }

    #[test]
    fn border_rays_follow_the_angle_system() {
        let (w, h, cell) = (40, 20, 1.0);
        let c = (20.0, 10.0);
        let end = |deg: f32| *ray_cells(c, deg, w, h, cell).last().unwrap();
        assert_eq!(end(0.0) % w, w - 1); // east → right edge
        assert_eq!(end(90.0) / w, 0); // north → top edge
        assert_eq!(end(180.0) % w, 0); // west → left edge
        assert_eq!(end(270.0) / w, h - 1); // south → bottom edge
    }

    #[test]
    fn default_gates_open_the_wall_north_and_south() {
        for seed in 1..=4u64 {
            let config = LevelConfig::test_64();
            let lvl = level(&config, seed);
            let gates = lvl.gates.as_ref().unwrap();
            assert_eq!(gates.gates.len(), 2, "seed {seed}: {:?}", lvl.notes);
            let map = &lvl.regions.as_ref().unwrap().map;
            let wall = &lvl.walls.as_ref().unwrap().mask;
            let (w, cell) = (map.width, map.cell_size_m);
            for g in &gates.gates {
                let c = (g.point_m[1] / cell) as usize * w + (g.point_m[0] / cell) as usize;
                assert!(!wall.cells[c], "seed {seed}: gate {} still walled", g.id);
                assert_ne!(map.ids[c], 0, "seed {seed}: gate {} not on town ground", g.id);
                assert!(g.opening_m >= 3.0 - 1e-3);
                // North gate in the top half, south gate in the bottom half.
                let top = g.point_m[1] < map.height as f32 * cell / 2.0;
                assert_eq!(top, g.actual_deg < 180.0, "seed {seed}: gate {} at {:?}", g.id, g.point_m);
                assert_eq!(lvl.objects[g.object].category, Category::Gate);
            }
        }
    }

    #[test]
    fn gate_roads_join_the_network() {
        for seed in 1..=4u64 {
            let lvl = level(&LevelConfig::test_64(), seed);
            let net = lvl.roads.as_ref().unwrap();
            for g in &lvl.gates.as_ref().unwrap().gates {
                let link = &net.links[g.road_link.unwrap_or_else(|| panic!("seed {seed}: gate {} has no road", g.id))];
                assert!(link.path.iter().all(|&c| net.mask.cells[c]));
            }
            assert!(grid::is_connected(&net.mask), "seed {seed}: road network split");
        }
    }

    #[test]
    fn later_nodes_keep_off_the_gate_roads() {
        for seed in 1..=3u64 {
            let lvl = level(&LevelConfig::test_64(), seed);
            let net = lvl.roads.as_ref().unwrap();
            let solid = [
                lvl.houses.as_ref().map(|l| &l.mask),
                lvl.props.as_ref().map(|l| &l.mask),
                lvl.interactive.as_ref().map(|l| &l.mask),
            ];
            for g in &lvl.gates.as_ref().unwrap().gates {
                for &c in &net.links[g.road_link.unwrap()].path {
                    assert!(!solid.iter().flatten().any(|m| m.cells[c]), "seed {seed}: object on a gate road");
                }
            }
        }
    }

    #[test]
    fn mobs_keep_away_from_gates() {
        let mut config = LevelConfig::test_64();
        config.mobs.min_entities = 40;
        config.mobs.max_entities = 40;
        let lvl = level(&config, 2);
        let gates = &lvl.gates.as_ref().unwrap().gates;
        for sp in &lvl.mobs.as_ref().unwrap().spawns {
            for g in gates {
                let d = ((sp.center_m[0] - g.point_m[0]).powi(2) + (sp.center_m[1] - g.point_m[1]).powi(2)).sqrt();
                assert!(d >= config.mobs.gate_clearance_m - 1e-3, "spawn {d} m from gate {}", g.id);
            }
        }
    }

    #[test]
    fn hidden_rooms_keep_clear_of_gates() {
        let mut config = LevelConfig::test_64();
        config.hidden_rooms.count = 6;
        for seed in 1..=4u64 {
            let lvl = level(&config, seed);
            for r in &lvl.hidden_rooms.as_ref().unwrap().rooms {
                let (cx, cy) = (r.rect_m[0] + r.rect_m[2] / 2.0, r.rect_m[1] + r.rect_m[3] / 2.0);
                for g in &lvl.gates.as_ref().unwrap().gates {
                    let d = ((cx - g.point_m[0]).powi(2) + (cy - g.point_m[1]).powi(2)).sqrt();
                    assert!(d >= 6.0, "seed {seed}: room {} {d} m from gate {}", r.id, g.id);
                }
            }
        }
    }

    #[test]
    fn any_direction_finds_the_wall() {
        let mut config = LevelConfig::test_64();
        config.gates.gates = (0..8).map(|k| GateSpec::at(k as f32 * 45.0)).collect();
        config.gates.road = false;
        let lvl = level(&config, 3);
        assert_eq!(lvl.gates.as_ref().unwrap().gates.len(), 8, "{:?}", lvl.notes);
    }

    #[test]
    fn island_maps_get_gates() {
        let mut config = LevelConfig::test_64();
        config.kind = MapKind::Forest;
        config.shape = ShapeSettings::for_kind(config.kind);
        let lvl = level(&config, 2);
        assert!(!lvl.gates.as_ref().unwrap().gates.is_empty(), "{:?}", lvl.notes);
    }

    #[test]
    fn same_seed_same_gates() {
        let config = LevelConfig::test_64();
        assert_eq!(level(&config, 8).gates, level(&config, 8).gates);
    }
}
