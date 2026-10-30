//! Swamp node 4, paths (step S3 in docs/swamp-plan.md): links the islands
//! into one walkable network — a half labyrinth: a tree, a few loops and a
//! few dead ends.
//!
//! 1. **Dead ends** — `dead_ends` small islands (random) are set aside.
//! 2. **Graph** — over the other islands' centers: a minimum spanning tree
//!    plus extra links between near neighbors (`extra_links_pct`, no
//!    crossings, no sharp angles) — the roads node's `choose_links`. Every
//!    dead-end island then gets exactly one link, to the nearest island in
//!    the graph.
//! 3. **Widths** — by traffic over the network (roads node's hierarchy):
//!    the busiest `main_paths_pct` get `width_max_m`, quiet ones narrow down
//!    to `width_min_m`.
//! 4. **Routing** — A* between the two island centers, 8 directions, in a
//!    window around them. Island ground is cheap (paths cross islands
//!    instead of hugging them), water cost is bent by a noise field
//!    (`wiggle_pct`), the border margin costs extra and existing mud is a
//!    little cheaper (paths merge). Only the cells off the islands are
//!    stamped as mud.
//! 5. **Boardwalks** — each link rolls `boardwalk_pct`; a boardwalk keeps
//!    the routed path's shore points but crosses the water in straight
//!    segments with up to `boardwalk_kinks` kinks.
//! 6. **Gate bridges** — every bridge gate gets a straight boardwalk from
//!    its island's nearest shore cell out to the gate point.
//!
//! Islands + mud + boardwalks are one connected piece afterwards.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use rand::seq::SliceRandom;
use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::grid;
use crate::generator::noise::NoiseField;
use crate::generator::roads::{choose_links, link_traffic, stamp_path, widths_from_traffic};
use crate::level::{
    CellMask, GateAnchor, GateBridge, IslandKind, IslandLayout, PathKind, PathLayout, RoadNode, SwampPath,
};
use crate::model::{GateStyle, LevelConfig};

pub struct PathBuild {
    pub layout: PathLayout,
    pub notes: Vec<String>,
}

/// Step cost on island ground (water = 1 before the wiggle).
const LAND_COST: f32 = 0.3;
/// Step cost multiplier inside the border margin.
const BORDER_COST: f32 = 4.0;
/// Step cost multiplier on mud already laid.
const ON_MUD_COST: f32 = 0.6;
/// Wiggle noise feature size, meters.
const WIGGLE_SCALE_M: f32 = 16.0;
/// Water step cost spans exp(±contrast/2) at 100% wiggle.
const WIGGLE_CONTRAST: f32 = 7.0;
/// Farthest a boardwalk kink strays from the straight line, as a share of
/// its length.
const KINK_OFFSET: f32 = 0.15;

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, anchors: &[GateAnchor], islands: &IslandLayout) -> PathBuild {
    let s = &config.swamp.paths;
    let (w, h, cell) = (islands.width, islands.height, islands.cell_size_m);
    let ids = &islands.ids;
    let land: Vec<bool> = ids.iter().map(|&i| i > 0).collect();
    let water: Vec<bool> = land.iter().map(|&l| !l).collect();
    let mut notes = Vec::new();
    let mut mud = CellMask::new(w, h, cell);
    let mut boardwalk = CellMask::new(w, h, cell);
    let noise = NoiseField::new(rng.random::<u64>());
    let n = islands.islands.len();

    // Node of each island: its cell nearest the centroid.
    let nodes: Vec<RoadNode> = islands
        .islands
        .iter()
        .map(|isl| {
            let (cx, cy) = (isl.center_m[0] / cell, isl.center_m[1] / cell);
            let c = (0..w * h)
                .filter(|&c| ids[c] == isl.id)
                .min_by(|&p, &q| {
                    let d = |c: usize| ((c % w) as f32 + 0.5 - cx).powi(2) + ((c / w) as f32 + 0.5 - cy).powi(2);
                    d(p).total_cmp(&d(q)).then(p.cmp(&q))
                })
                .unwrap_or(0);
            RoadNode { x: c % w, y: c / w, region: 0 }
        })
        .collect();

    // 1–2. Dead ends set aside, graph over the rest, then one link each.
    let mut small: Vec<usize> = (0..n).filter(|&k| islands.islands[k].kind == IslandKind::Small).collect();
    small.shuffle(rng);
    let dead: Vec<usize> = small.into_iter().take((s.dead_ends as usize).min(n.saturating_sub(2))).collect();
    let core: Vec<usize> = (0..n).filter(|k| !dead.contains(k)).collect();
    let core_nodes: Vec<RoadNode> = core.iter().map(|&k| nodes[k]).collect();
    let mut links: Vec<(usize, usize, bool, bool)> = choose_links(rng, &core_nodes, s.extra_links_pct)
        .into_iter()
        .map(|(a, b, extra)| (core[a], core[b], extra, false))
        .collect();
    let d2 = |a: usize, b: usize| {
        let (p, q) = (&nodes[a], &nodes[b]);
        (p.x as i64 - q.x as i64).pow(2) + (p.y as i64 - q.y as i64).pow(2)
    };
    for &d in &dead {
        if let Some(&to) = core.iter().min_by_key(|&&k| (d2(d, k), k)) {
            links.push((d, to, false, true));
        }
    }

    // 3. Widths by traffic.
    let plain: Vec<(usize, usize, bool)> = links.iter().map(|&(a, b, e, _)| (a, b, e)).collect();
    let widths = widths_from_traffic(
        &link_traffic(&nodes, &plain),
        s.main_paths_pct,
        s.width_min_m.min(s.width_max_m).max(cell),
        s.width_max_m.max(s.width_min_m).max(cell),
    );

    // 4–5. Route every link; boardwalks replace the water part.
    let border = (config.swamp.islands.border_m.max(0.0) / cell).round() as usize;
    let wiggle = s.wiggle_pct.clamp(0.0, 100.0) / 100.0;
    let base: Vec<f32> = (0..w * h)
        .map(|c| {
            let (x, y) = (c % w, c / w);
            if land[c] {
                return LAND_COST;
            }
            let n = noise.fbm(x as f32 * cell / WIGGLE_SCALE_M, y as f32 * cell / WIGGLE_SCALE_M, 2);
            let mut f = (wiggle * (n - 0.5) * WIGGLE_CONTRAST).exp();
            if x < border || y < border || x + border >= w || y + border >= h {
                f *= BORDER_COST;
            }
            f
        })
        .collect();
    let cheapest = LAND_COST.min((-wiggle * WIGGLE_CONTRAST / 2.0).exp() * ON_MUD_COST);
    let mut paths = Vec::new();
    for (k, &(a, b, extra, dead_end)) in links.iter().enumerate() {
        let is_boardwalk = rng.random::<f32>() < s.boardwalk_pct.clamp(0.0, 100.0) / 100.0;
        let from = nodes[a].y * w + nodes[a].x;
        let to = nodes[b].y * w + nodes[b].x;
        let route = astar(w, h, from, to, |c| base[c] * if mud.cells[c] { ON_MUD_COST } else { 1.0 }, cheapest);
        let (id_a, id_b) = (islands.islands[a].id, islands.islands[b].id);
        let width_m = if is_boardwalk { s.boardwalk_width_m.max(cell) } else { widths[k] };
        if is_boardwalk {
            // Shore points: leaving island a, arriving on island b.
            let start = route.iter().position(|&c| ids[c] != id_a).map_or(from, |i| route[i.saturating_sub(1)]);
            let end = route.iter().position(|&c| ids[c] == id_b).unwrap_or(route.len() - 1);
            let (p, q) = (center(start, w, cell), center(route[end], w, cell));
            let len = (q[0] - p[0]).hypot(q[1] - p[1]);
            let kinks = rng.random_range(0..=s.boardwalk_kinks.min(4));
            let mut ts: Vec<f32> = (0..kinks).map(|_| rng.random_range(0.2..0.8)).collect();
            ts.sort_by(f32::total_cmp);
            let (nx, ny) = (-(q[1] - p[1]) / len.max(1e-6), (q[0] - p[0]) / len.max(1e-6));
            let mut corners = vec![p];
            for t in ts {
                let off = rng.random_range(-KINK_OFFSET..=KINK_OFFSET) * len;
                corners.push([p[0] + (q[0] - p[0]) * t + nx * off, p[1] + (q[1] - p[1]) * t + ny * off]);
            }
            corners.push(q);
            let line = polyline_cells(&corners, w, h, cell);
            stamp_path(&mut boardwalk, &water, &line, width_m / cell / 2.0);
            let length_m = corners.windows(2).map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1])).sum();
            paths.push(SwampPath { a: id_a, b: id_b, kind: PathKind::Boardwalk, extra, dead_end, width_m, length_m, path_m: corners });
        } else {
            stamp_path(&mut mud, &water, &route, width_m / cell / 2.0);
            let wet: Vec<usize> = route.iter().copied().filter(|&c| !land[c]).collect();
            let length_m = route
                .windows(2)
                .filter(|p| !land[p[1]])
                .map(|p| if p[0] % w != p[1] % w && p[0] / w != p[1] / w { std::f32::consts::SQRT_2 } else { 1.0 })
                .sum::<f32>()
                * cell;
            let every = ((1.0 / cell).round() as usize).max(1);
            let path_m = wet.iter().step_by(every).map(|&c| center(c, w, cell)).collect();
            paths.push(SwampPath { a: id_a, b: id_b, kind: PathKind::Mud, extra, dead_end, width_m, length_m, path_m });
        }
    }

    // 6. Gate bridges.
    let mut bridges = Vec::new();
    for anchor in anchors.iter().filter(|a| a.style == GateStyle::Bridge) {
        let Some(isl) = islands.islands.iter().find(|i| i.gate == Some(anchor.id)) else {
            notes.push(format!("Gate #{}: no gate island left for its bridge", anchor.id));
            continue;
        };
        let (px, py) = (anchor.point_m[0], anchor.point_m[1]);
        let shore = (0..w * h)
            .filter(|&c| ids[c] == isl.id)
            .min_by(|&p, &q| {
                let d = |c: usize| {
                    let m = center(c, w, cell);
                    (m[0] - px).powi(2) + (m[1] - py).powi(2)
                };
                d(p).total_cmp(&d(q)).then(p.cmp(&q))
            })
            .unwrap_or(0);
        let (from, to) = (center(shore, w, cell), [px, py]);
        let line = polyline_cells(&[from, to], w, h, cell);
        stamp_path(&mut boardwalk, &water, &line, s.boardwalk_width_m.max(cell) / cell / 2.0);
        bridges.push(GateBridge {
            gate: anchor.id,
            island: isl.id,
            from_m: from,
            to_m: to,
            length_m: (to[0] - from[0]).hypot(to[1] - from[1]),
        });
    }

    // Boardwalks win where they cross mud.
    for (m, &b) in mud.cells.iter_mut().zip(&boardwalk.cells) {
        *m &= !b;
    }
    let walkable = CellMask {
        width: w,
        height: h,
        cell_size_m: cell,
        cells: (0..w * h).map(|c| land[c] || mud.cells[c] || boardwalk.cells[c]).collect(),
    };
    let (_, pieces) = grid::components(&walkable);
    if pieces > 1 {
        notes.push(format!("Paths: the walkable ground is in {pieces} pieces"));
    }
    PathBuild { layout: PathLayout { mud, boardwalk, paths, bridges }, notes }
}

/// Center of a cell, meters.
pub(crate) fn center(c: usize, w: usize, cell: f32) -> [f32; 2] {
    [((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell]
}

/// The cells under a polyline (meters), in order, without repeats.
pub(crate) fn polyline_cells(corners: &[[f32; 2]], w: usize, h: usize, cell: f32) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    for seg in corners.windows(2) {
        let (p, q) = (seg[0], seg[1]);
        let steps = (((q[0] - p[0]).hypot(q[1] - p[1]) / (cell * 0.25)).ceil() as usize).max(1);
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let x = ((p[0] + (q[0] - p[0]) * t) / cell).floor().clamp(0.0, (w - 1) as f32) as usize;
            let y = ((p[1] + (q[1] - p[1]) * t) / cell).floor().clamp(0.0, (h - 1) as f32) as usize;
            let c = y * w + x;
            if out.last() != Some(&c) {
                out.push(c);
            }
        }
    }
    out
}

/// A* from `from` to `to` over every cell, 8 directions, inside a window
/// around both ends. `cost` is the step factor of entering a cell; `cheapest`
/// its lowest value (keeps the heuristic admissible). Returns the cells,
/// `from` first.
pub(crate) fn astar(w: usize, h: usize, from: usize, to: usize, cost: impl Fn(usize) -> f32, cheapest: f32) -> Vec<usize> {
    let (fx, fy, tx, ty) = (from % w, from / w, to % w, to / w);
    let span = fx.abs_diff(tx).max(fy.abs_diff(ty));
    let pad = (span * 3 / 5).max(40);
    let (x0, x1) = (fx.min(tx).saturating_sub(pad), (fx.max(tx) + pad).min(w - 1));
    let (y0, y1) = (fy.min(ty).saturating_sub(pad), (fy.max(ty) + pad).min(h - 1));
    let heuristic = |c: usize| {
        let (dx, dy) = ((c % w) as f32 - tx as f32, (c / w) as f32 - ty as f32);
        ((dx * dx + dy * dy).sqrt() * cheapest * 1024.0) as u64
    };
    let mut best = vec![u64::MAX; w * h];
    let mut parent = vec![usize::MAX; w * h];
    let mut heap = BinaryHeap::new();
    best[from] = 0;
    heap.push(Reverse((heuristic(from), from)));
    while let Some(Reverse((_, c))) = heap.pop() {
        if c == to {
            break;
        }
        let g = best[c];
        let (cx, cy) = (c % w, c / w);
        for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
            let (nx, ny) = (cx as i64 + dx, cy as i64 + dy);
            if nx < x0 as i64 || ny < y0 as i64 || nx > x1 as i64 || ny > y1 as i64 {
                continue;
            }
            let nb = ny as usize * w + nx as usize;
            let length = if dx != 0 && dy != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
            let ng = g + ((length * cost(nb) * 1024.0) as u64).max(1);
            if ng < best[nb] {
                best[nb] = ng;
                parent[nb] = c;
                heap.push(Reverse((ng + heuristic(nb), nb)));
            }
        }
    }
    let mut path = vec![to];
    let mut c = to;
    while c != from && parent[c] != usize::MAX {
        c = parent[c];
        path.push(c);
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::generator::swamp::{gate_anchors, islands};
    use crate::model::{GateSpec, MapKind};

    fn config() -> LevelConfig {
        let mut c = LevelConfig { kind: MapKind::Swamp, ..LevelConfig::default() };
        c.map.width_m = 120.0;
        c.map.height_m = 100.0;
        c.cell_size_m = 1.0;
        c.gates.gates = vec![
            GateSpec { direction_deg: 90.0, style: GateStyle::Shore },
            GateSpec { direction_deg: 270.0, style: GateStyle::Bridge },
        ];
        c
    }

    fn build_all(c: &LevelConfig, seed: u64) -> (IslandLayout, PathBuild) {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let anchors = gate_anchors::build(c, &mut rng).anchors;
        let isl = islands::build(c, &mut rng, &anchors).layout;
        let p = build(c, &mut rng, &anchors, &isl);
        (isl, p)
    }

    #[test]
    fn walkable_ground_is_one_piece() {
        let c = config();
        for seed in 0..4 {
            let (_, p) = build_all(&c, seed);
            assert!(p.notes.is_empty(), "seed {seed}: {:?}", p.notes);
        }
    }

    #[test]
    fn tree_plus_loops_and_dead_ends_have_one_link() {
        let c = config();
        let (isl, p) = build_all(&c, 1);
        let paths = &p.layout.paths;
        let tree = paths.iter().filter(|x| !x.extra).count();
        assert_eq!(tree, isl.islands.len() - 1, "tree links incl. dead ends");
        let dead: Vec<u16> = paths.iter().filter(|x| x.dead_end).map(|x| x.a).collect();
        assert_eq!(dead.len(), 3);
        for d in dead {
            assert_eq!(isl.islands[d as usize - 1].kind, IslandKind::Small);
            assert_eq!(paths.iter().filter(|x| x.a == d || x.b == d).count(), 1, "dead end {d} has more links");
        }
        let mut none = config();
        none.swamp.paths.extra_links_pct = 0.0;
        let (_, p) = build_all(&none, 1);
        assert!(p.layout.paths.iter().all(|x| !x.extra));
    }

    #[test]
    fn paths_stay_off_islands_and_kinds_follow_the_chance() {
        let mut c = config();
        c.swamp.paths.boardwalk_pct = 0.0;
        let (isl, p) = build_all(&c, 2);
        assert!(p.layout.paths.iter().all(|x| x.kind == PathKind::Mud));
        for k in 0..isl.ids.len() {
            assert!(!(isl.ids[k] > 0 && (p.layout.mud.cells[k] || p.layout.boardwalk.cells[k])));
        }
        let widths: Vec<f32> = p.layout.paths.iter().map(|x| x.width_m).collect();
        assert!(widths.iter().all(|&x| (1.5..=3.5).contains(&x)), "{widths:?}");
        assert!(widths.contains(&3.5), "main paths at full width");
        c.swamp.paths.boardwalk_pct = 100.0;
        let (_, p) = build_all(&c, 2);
        assert!(p.layout.paths.iter().all(|x| x.kind == PathKind::Boardwalk && x.path_m.len() >= 2));
        assert_eq!(p.layout.mud.count_true(), 0);
        assert!(p.notes.is_empty(), "{:?}", p.notes);
    }

    #[test]
    fn bridge_gates_get_a_boardwalk_to_the_edge() {
        let c = config();
        let (_, p) = build_all(&c, 3);
        let bridges = &p.layout.bridges;
        assert_eq!(bridges.len(), 1);
        let b = &bridges[0];
        assert_eq!(b.gate, 2);
        // The south gate: its boardwalk reaches the bottom row.
        let w = p.layout.boardwalk.width;
        let x = (b.to_m[0] / c.cell_size_m) as usize;
        assert!(p.layout.boardwalk.cells[(p.layout.boardwalk.height - 1) * w + x], "{b:?}");
    }

    #[test]
    fn same_seed_same_paths() {
        let c = config();
        assert_eq!(build_all(&c, 5).1.layout, build_all(&c, 5).1.layout);
    }
}
