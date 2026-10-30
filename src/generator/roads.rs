//! The roads node: a network of crossroad nodes linked by gently winding
//! roads through the town area (every map kind gets it — paths in a forest,
//! tunnels in a cave). Pipeline:
//!
//! 1. **Node budget** — `roads.node_count` (or a density per 1000 m²), never
//!    fewer than the region count: every region gets one node, the rest go
//!    to the regions with the most area per node (D'Hondt), so big regions
//!    get more crossroads.
//! 2. **Node placement** — inside its region, away from the region border,
//!    best of 24 random candidates: the one farthest from every node already
//!    placed, so crossroads spread out.
//! 3. **Links** — a minimum spanning tree over all nodes (shortest total
//!    length; never crosses itself) guarantees one connected network; extra
//!    links between near neighbors are added with `extra_links_pct` chance
//!    for loops, skipped when they would cross a link or leave a node at a
//!    sharp angle to one.
//! 4. **Routing** — each link is a shortest path over the town cells (8
//!    directions) whose step cost is bent by a smooth noise field
//!    (`wiggle_pct`, kept gentle), raised near the outer wall (roads do not
//!    hug it) and lowered on existing road (links merge instead of running
//!    side by side).
//! 5. **Widths** — a road hierarchy by traffic: for every pair of nodes the
//!    shortest route through the network is found and every link on it
//!    counts one trip. The busiest `main_roads_pct` of the links are main
//!    roads at the full `width_m`; the rest get narrower with their traffic,
//!    down to `min_width_m` for quiet dead-end alleys.
//! 6. **Passages** — wherever a road crosses from one region into another,
//!    a passage is recorded: the gap the region walls (next step) leave open.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::noise::NoiseField;
use crate::generator::Stage;
use crate::level::{CellMask, Passage, RegionMap, RegionSplit, RoadLink, RoadNetwork, RoadNode};
use crate::model::{LevelConfig, NodeAmount};

/// Hard cap on crossroad nodes (above the region count).
pub const MAX_NODES: u32 = 64;
/// Extra links may connect each node only to its this-many nearest nodes.
const NEAREST: usize = 4;
/// An extra link is skipped if it leaves a node closer than this to an
/// existing link (degrees).
const MIN_LINK_ANGLE_DEG: f32 = 30.0;
/// Noise contrast at 100% wiggle: step cost × exp(±3) at the extremes.
const WIGGLE_CONTRAST: f32 = 2.5;
/// Noise feature size for the wiggle, in meters.
const WIGGLE_SCALE_M: f32 = 10.0;
/// Step cost multiplier next to the outer wall / town edge.
const NEAR_EDGE_COST: f32 = 3.0;
/// Step cost multiplier next to a border between two regions: roads cross
/// borders briskly instead of running along them (one clean passage).
const NEAR_BORDER_COST: f32 = 2.5;
/// Step cost multiplier on cells that are already road.
const ON_ROAD_COST: f32 = 0.6;

pub struct RoadBuild {
    pub network: RoadNetwork,
    /// Generation compromises (links that could not be routed).
    pub notes: Vec<String>,
}

/// `build_with_progress` without progress reporting.
pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, split: &RegionSplit) -> Option<RoadBuild> {
    build_with_progress(config, rng, split, &Stage { progress: &|_, _| {}, from: 0.0, to: 1.0 })
}

pub(crate) fn build_with_progress(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    split: &RegionSplit,
    stage: &Stage,
) -> Option<RoadBuild> {
    let map = &split.map;
    let (w, h) = (map.width, map.height);
    let town: Vec<bool> = map.ids.iter().map(|&id| id != 0).collect();
    let total = town.iter().filter(|&&t| t).count();
    if total == 0 {
        return None;
    }
    let settings = &config.roads;

    // 1. Node budget and its split over the regions.
    let mut area = [0u32; 256];
    for &id in &map.ids {
        area[id as usize] += 1;
    }
    let present: Vec<u8> = (1..=255u8).filter(|&id| area[id as usize] > 0).collect();
    let wanted = node_budget(config, total);
    let n_nodes = (wanted as usize).clamp(present.len(), (MAX_NODES as usize).max(present.len()));
    let mut per_region = vec![1usize; present.len()];
    for _ in present.len()..n_nodes {
        // D'Hondt: the next node goes to the most area per node (ties: lower id).
        let best = (0..present.len())
            .max_by_key(|&i| {
                let quotient = area[present[i] as usize] as u64 * 1_000_000 / (per_region[i] as u64 + 1);
                (quotient, Reverse(i))
            })
            .expect("at least one region");
        per_region[best] += 1;
    }

    // 2. Node placement.
    stage.report("Placing crossroads", 0.0);
    let to_cells = |m: f32| ((m / map.cell_size_m).round() as u32).max(1);
    let width_cells = to_cells(settings.width_m.max(settings.min_width_m));
    let radius = width_cells as f32 / 2.0;
    let region_depth = region_depth(map);
    let mut nodes: Vec<RoadNode> = Vec::with_capacity(n_nodes);
    for (ri, &id) in present.iter().enumerate() {
        let cells: Vec<usize> = (0..w * h).filter(|&i| map.ids[i] == id).collect();
        let deepest = cells.iter().map(|&i| region_depth[i]).max().unwrap_or(0);
        let need = (radius.ceil() as u32 + 2).min(deepest);
        let candidates: Vec<usize> = cells.into_iter().filter(|&i| region_depth[i] >= need).collect();
        for _ in 0..per_region[ri] {
            let mut best: Option<(u64, usize)> = None;
            for _ in 0..24 {
                let c = candidates[rng.random_range(0..candidates.len())];
                let d = nodes
                    .iter()
                    .map(|n| dist2((n.x, n.y), (c % w, c / w)))
                    .min()
                    .unwrap_or(u64::MAX);
                if best.is_none_or(|(bd, _)| d > bd) {
                    best = Some((d, c));
                }
            }
            if let Some((d, c)) = best
                && d > 0
            {
                nodes.push(RoadNode { x: c % w, y: c / w, region: id });
            }
        }
    }

    // 3. Links: minimum spanning tree + optional extras.
    let chosen = choose_links(rng, &nodes, settings.extra_links_pct);

    // 5. (before routing, so each road is stamped at its own width) Widths
    //    from traffic over the straight-line network.
    let link_widths = {
        let traffic = link_traffic(&nodes, &chosen);
        let lo = settings.min_width_m.min(settings.width_m);
        let hi = settings.width_m.max(settings.min_width_m);
        widths_from_traffic(&traffic, settings.main_roads_pct, lo, hi)
            .into_iter()
            .map(to_cells)
            .collect::<Vec<u32>>()
    };

    // 4. Routing, MST links first (extras then reuse their road).
    let wiggle = settings.wiggle_pct.clamp(0.0, 100.0) / 100.0 * WIGGLE_CONTRAST;
    let noise = NoiseField::new(rng.random());
    let router = Router {
        w,
        h,
        town: &town,
        edge_depth: town_depth(&town, w, h),
        border_depth: border_depth(map),
        near_edge: radius.ceil() as u32 + 1,
        noise,
        noise_scale: (WIGGLE_SCALE_M / map.cell_size_m).max(4.0),
        wiggle,
    };
    let mut road = CellMask::new(w, h, map.cell_size_m);
    let mut links: Vec<RoadLink> = Vec::with_capacity(chosen.len());
    let mut notes = Vec::new();
    let mut node_width = vec![1u32; nodes.len()];
    for (k, &(a, b, extra)) in chosen.iter().enumerate() {
        stage.report("Routing roads", 0.1 + 0.85 * k as f32 / chosen.len().max(1) as f32);
        let from = nodes[a].y * w + nodes[a].x;
        let to = nodes[b].y * w + nodes[b].x;
        match router.route(from, to, &road.cells) {
            Some(path) => {
                let lw = link_widths[k];
                stamp_path(&mut road, &town, &path, lw as f32 / 2.0);
                node_width[a] = node_width[a].max(lw);
                node_width[b] = node_width[b].max(lw);
                links.push(RoadLink { a, b, path, extra, width_cells: lw });
            }
            None => notes.push(format!(
                "Road between crossroads {a} and {b} skipped: no way through the town"
            )),
        }
    }
    // Crossroads themselves are always road, as wide as their widest road.
    for (n, &nw) in nodes.iter().zip(&node_width) {
        stamp_path(&mut road, &town, &[n.y * w + n.x], nw as f32 / 2.0);
    }

    // 6. Passages between regions.
    stage.report("Finding passages", 0.95);
    let passages = find_passages(map, &links, width_cells);

    Some(RoadBuild {
        network: RoadNetwork {
            mask: road,
            nodes,
            links,
            passages,
            width_cells,
        },
        notes,
    })
}

/// How many nodes the settings ask for (before the one-per-region minimum).
pub fn node_budget(config: &LevelConfig, town_cells: usize) -> u32 {
    let s = &config.roads;
    match s.amount {
        NodeAmount::Count => s.node_count,
        NodeAmount::Density => {
            let area_m2 = town_cells as f32 * config.cell_size_m * config.cell_size_m;
            (area_m2 / 1000.0 * s.nodes_per_1000m2.max(0.0)).round() as u32
        }
    }
}

fn dist2(a: (usize, usize), b: (usize, usize)) -> u64 {
    let dx = a.0 as i64 - b.0 as i64;
    let dy = a.1 as i64 - b.1 as i64;
    (dx * dx + dy * dy) as u64
}

/// The 4-neighbors of `cell` inside the grid.
fn neighbors4(w: usize, h: usize, cell: usize) -> impl Iterator<Item = usize> {
    let (x, y) = (cell % w, cell / w);
    [
        (x > 0).then(|| cell - 1),
        (x + 1 < w).then(|| cell + 1),
        (y > 0).then(|| cell - w),
        (y + 1 < h).then(|| cell + w),
    ]
    .into_iter()
    .flatten()
}

/// Multi-source BFS distance (4-steps) from a set of start cells over `inside`.
fn depth_from(w: usize, h: usize, inside: impl Fn(usize) -> bool, is_edge: impl Fn(usize) -> bool) -> Vec<u32> {
    let mut depth = vec![0u32; w * h];
    let mut queue = VecDeque::new();
    for (i, d) in depth.iter_mut().enumerate() {
        if inside(i) && is_edge(i) {
            *d = 1;
            queue.push_back(i);
        }
    }
    while let Some(c) = queue.pop_front() {
        for n in neighbors4(w, h, c) {
            if inside(n) && depth[n] == 0 {
                depth[n] = depth[c] + 1;
                queue.push_back(n);
            }
        }
    }
    depth
}

/// Distance of every region cell from its region's border (1 = touches
/// another region, the wall or the outside; 0 = not town).
fn region_depth(map: &RegionMap) -> Vec<u32> {
    let (w, h) = (map.width, map.height);
    let ids = &map.ids;
    depth_from(
        w,
        h,
        |i| ids[i] != 0,
        |i| neighbors4(w, h, i).filter(|&n| ids[n] == ids[i]).count() < 4,
    )
}

/// Distance of every town cell from the nearest border between two regions
/// (1 = touches another region). Unlike `region_depth`, the outer wall side
/// does not count as a border here.
fn border_depth(map: &RegionMap) -> Vec<u32> {
    let (w, h) = (map.width, map.height);
    let ids = &map.ids;
    depth_from(
        w,
        h,
        |i| ids[i] != 0,
        |i| neighbors4(w, h, i).any(|n| ids[n] != 0 && ids[n] != ids[i]),
    )
}

/// Distance of every town cell from the town edge (the outer wall side).
fn town_depth(town: &[bool], w: usize, h: usize) -> Vec<u32> {
    depth_from(
        w,
        h,
        |i| town[i],
        |i| neighbors4(w, h, i).filter(|&n| town[n]).count() < 4,
    )
}

/// The links of the network: (node a, node b, extra). Kruskal's minimum
/// spanning tree over all node pairs first; then each near-neighbor pair not
/// in the tree, shortest first, is rolled against `extra_pct` and kept if it
/// neither crosses a chosen link nor leaves either node at a sharp angle to
/// one. Ties by node index — deterministic.
pub(crate) fn choose_links(rng: &mut ChaCha8Rng, nodes: &[RoadNode], extra_pct: f32) -> Vec<(usize, usize, bool)> {
    let n = nodes.len();
    let pos = |i: usize| (nodes[i].x as f32, nodes[i].y as f32);
    let mut pairs: Vec<(u64, usize, usize)> = Vec::new();
    for a in 0..n {
        for b in a + 1..n {
            pairs.push((dist2((nodes[a].x, nodes[a].y), (nodes[b].x, nodes[b].y)), a, b));
        }
    }
    pairs.sort();

    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut chosen: Vec<(usize, usize, bool)> = Vec::new();
    for &(_, a, b) in &pairs {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra != rb {
            parent[ra] = rb;
            chosen.push((a, b, false));
        }
    }

    // Near-neighbor candidates for extra links.
    let mut near = vec![false; n * n];
    for a in 0..n {
        let mut others: Vec<(u64, usize)> = (0..n)
            .filter(|&b| b != a)
            .map(|b| (dist2((nodes[a].x, nodes[a].y), (nodes[b].x, nodes[b].y)), b))
            .collect();
        others.sort();
        for &(_, b) in others.iter().take(NEAREST) {
            near[a.min(b) * n + a.max(b)] = true;
        }
    }
    let chance = extra_pct.clamp(0.0, 100.0) / 100.0;
    let min_cos = MIN_LINK_ANGLE_DEG.to_radians().cos();
    for &(_, a, b) in &pairs {
        if !near[a * n + b] || chosen.iter().any(|&(x, y, _)| (x, y) == (a, b)) {
            continue;
        }
        // Always roll, so the stream does not depend on earlier outcomes.
        if rng.random::<f32>() >= chance {
            continue;
        }
        let crosses = chosen.iter().any(|&(x, y, _)| {
            ![x, y].contains(&a) && ![x, y].contains(&b) && segments_cross(pos(a), pos(b), pos(x), pos(y))
        });
        let sharp = |at: usize, to: usize| {
            chosen.iter().any(|&(x, y, _)| {
                let other = if x == at { y } else if y == at { x } else { return false };
                cos_angle(pos(at), pos(to), pos(other)) > min_cos
            })
        };
        if !crosses && !sharp(a, b) && !sharp(b, a) {
            chosen.push((a, b, true));
        }
    }
    chosen
}

/// Trips per link: for every node pair, the shortest route over the links
/// (straight-line lengths) counts one trip on each link it uses. Dijkstra
/// from every node; ties go to the lower node index — deterministic.
pub(crate) fn link_traffic(nodes: &[RoadNode], links: &[(usize, usize, bool)]) -> Vec<u32> {
    let n = nodes.len();
    let length = |a: usize, b: usize| (dist2((nodes[a].x, nodes[a].y), (nodes[b].x, nodes[b].y)) as f64).sqrt();
    let mut adjacent: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n]; // (neighbor, link)
    for (k, &(a, b, _)) in links.iter().enumerate() {
        adjacent[a].push((b, k));
        adjacent[b].push((a, k));
    }
    let mut traffic = vec![0u32; links.len()];
    for source in 0..n {
        let mut dist = vec![f64::INFINITY; n];
        let mut via = vec![usize::MAX; n]; // link used to reach the node
        let mut done = vec![false; n];
        dist[source] = 0.0;
        for _ in 0..n {
            let Some(u) = (0..n).filter(|&i| !done[i] && dist[i].is_finite()).min_by(|&i, &j| {
                dist[i].total_cmp(&dist[j]).then(i.cmp(&j))
            }) else {
                break;
            };
            done[u] = true;
            for &(v, k) in &adjacent[u] {
                let d = dist[u] + length(u, v);
                if d < dist[v] - 1e-9 {
                    dist[v] = d;
                    via[v] = k;
                }
            }
        }
        // Each pair once: only targets after the source.
        for target in source + 1..n {
            let mut v = target;
            while v != source && via[v] != usize::MAX {
                let k = via[v];
                traffic[k] += 1;
                let (a, b, _) = links[k];
                v = if a == v { b } else { a };
            }
        }
    }
    traffic
}

/// Road widths (meters) from traffic: the busiest `main_pct` of the links
/// get `hi`; the others scale with their traffic relative to the quietest
/// main road, down to `lo`.
pub(crate) fn widths_from_traffic(traffic: &[u32], main_pct: f32, lo: f32, hi: f32) -> Vec<f32> {
    if traffic.is_empty() {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..traffic.len()).collect();
    order.sort_by_key(|&k| (std::cmp::Reverse(traffic[k]), k));
    let mains = ((main_pct.clamp(0.0, 100.0) / 100.0 * traffic.len() as f32).ceil() as usize).min(traffic.len());
    let reference = if mains > 0 { traffic[order[mains - 1]] } else { traffic[order[0]] }.max(1) as f32;
    let mut widths = vec![lo; traffic.len()];
    for (rank, &k) in order.iter().enumerate() {
        widths[k] = if rank < mains {
            hi
        } else {
            lo + (hi - lo) * (traffic[k] as f32 / reference).min(1.0)
        };
    }
    widths
}

/// Cosine of the angle at `at` between the directions to `p` and `q`.
fn cos_angle(at: (f32, f32), p: (f32, f32), q: (f32, f32)) -> f32 {
    let (ux, uy) = (p.0 - at.0, p.1 - at.1);
    let (vx, vy) = (q.0 - at.0, q.1 - at.1);
    let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt().max(1e-6);
    (ux * vx + uy * vy) / len
}

/// Do segments p1–p2 and q1–q2 properly cross (not just touch)?
fn segments_cross(p1: (f32, f32), p2: (f32, f32), q1: (f32, f32), q2: (f32, f32)) -> bool {
    let orient = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| {
        (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
    };
    let (d1, d2) = (orient(q1, q2, p1), orient(q1, q2, p2));
    let (d3, d4) = (orient(p1, p2, q1), orient(p1, p2, q2));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// Shortest-path routing of one link over the town cells.
struct Router<'a> {
    w: usize,
    h: usize,
    town: &'a [bool],
    edge_depth: Vec<u32>,
    /// Distance to the nearest border between two regions.
    border_depth: Vec<u32>,
    /// Cells this close to the town edge (or a region border) cost more.
    near_edge: u32,
    noise: NoiseField,
    noise_scale: f32,
    wiggle: f32,
}

impl Router<'_> {
    /// Cost multiplier of entering `cell` (fixed point, 1024 = 1.0).
    fn cell_factor(&self, cell: usize, road: &[bool]) -> f32 {
        let (x, y) = ((cell % self.w) as f32, (cell / self.w) as f32);
        let n = self.noise.fbm(x / self.noise_scale, y / self.noise_scale, 2);
        let mut f = (self.wiggle * (n - 0.5) * 4.0).exp();
        if self.edge_depth[cell] <= self.near_edge {
            f *= NEAR_EDGE_COST;
        }
        if self.border_depth[cell] <= self.near_edge {
            f *= NEAR_BORDER_COST;
        }
        if road[cell] {
            f *= ON_ROAD_COST;
        }
        f
    }

    /// A* from `from` to `to` over town cells, 8 directions (diagonals only
    /// between two town cells, so roads never squeeze through a corner).
    /// Returns the centerline cells, `from` first.
    fn route(&self, from: usize, to: usize, road: &[bool]) -> Option<Vec<usize>> {
        let (w, h) = (self.w, self.h);
        let goal = ((to % w) as f32, (to / w) as f32);
        // Heuristic: straight distance at the cheapest possible step cost —
        // admissible, so the path is the true cheapest one.
        let cheapest = (-self.wiggle * 2.0).exp() * ON_ROAD_COST;
        let heuristic = |c: usize| {
            let (dx, dy) = ((c % w) as f32 - goal.0, (c / w) as f32 - goal.1);
            ((dx * dx + dy * dy).sqrt() * cheapest * 1024.0) as u64
        };
        let mut best = vec![u64::MAX; w * h];
        let mut parent = vec![usize::MAX; w * h];
        let mut heap = BinaryHeap::new();
        best[from] = 0;
        heap.push(Reverse((heuristic(from), from)));
        while let Some(Reverse((_, cell))) = heap.pop() {
            if cell == to {
                let mut path = vec![to];
                let mut c = to;
                while c != from {
                    c = parent[c];
                    path.push(c);
                }
                path.reverse();
                return Some(path);
            }
            let g = best[cell];
            let (cx, cy) = ((cell % w) as i64, (cell / w) as i64);
            for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                let (nx, ny) = (cx + dx, cy + dy);
                if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                    continue;
                }
                let n = ny as usize * w + nx as usize;
                if !self.town[n] {
                    continue;
                }
                let diagonal = dx != 0 && dy != 0;
                if diagonal
                    && !(self.town[cy as usize * w + nx as usize] && self.town[ny as usize * w + cx as usize])
                {
                    continue;
                }
                let length = if diagonal { std::f32::consts::SQRT_2 } else { 1.0 };
                let step = ((length * self.cell_factor(n, road) * 1024.0) as u64).max(1);
                let ng = g + step;
                if ng < best[n] {
                    best[n] = ng;
                    parent[n] = cell;
                    heap.push(Reverse((ng + heuristic(n), n)));
                }
            }
        }
        None
    }
}

/// Paint the road along `path`: a disc of `radius` cells around every
/// centerline cell (town cells only), plus one orthogonal cell at each
/// diagonal step so even a 1-cell road stays 4-connected.
pub(crate) fn stamp_path(road: &mut CellMask, town: &[bool], path: &[usize], radius: f32) {
    let (w, h) = (road.width, road.height);
    let r = radius.ceil() as i64;
    let r2 = radius * radius;
    let mut set = |x: i64, y: i64| {
        if x >= 0 && y >= 0 && x < w as i64 && y < h as i64 {
            let i = y as usize * w + x as usize;
            if town[i] {
                road.cells[i] = true;
            }
        }
    };
    for (k, &c) in path.iter().enumerate() {
        let (cx, cy) = ((c % w) as i64, (c / w) as i64);
        for dy in -r..=r {
            for dx in -r..=r {
                if (dx * dx + dy * dy) as f32 <= r2.max(0.25) {
                    set(cx + dx, cy + dy);
                }
            }
        }
        if let Some(&next) = path.get(k + 1) {
            let (nx, ny) = ((next % w) as i64, (next / w) as i64);
            if nx != cx && ny != cy {
                let i = cy as usize * w + nx as usize;
                if town[i] {
                    set(nx, cy);
                } else {
                    set(cx, ny);
                }
            }
        }
    }
}

/// Every point where a road centerline steps from one region into another.
/// Crossings of the same region pair closer than a road width + 2 cells
/// (merged roads crossing together) count once.
fn find_passages(map: &RegionMap, links: &[RoadLink], width_cells: u32) -> Vec<Passage> {
    let w = map.width;
    let near = width_cells as i64 + 2;
    let mut passages: Vec<Passage> = Vec::new();
    for (k, link) in links.iter().enumerate() {
        for pair in link.path.windows(2) {
            let (from, to) = (map.ids[pair[0]], map.ids[pair[1]]);
            if from == 0 || to == 0 || from == to {
                continue;
            }
            let (x, y) = (pair[1] % w, pair[1] / w);
            let duplicate = passages.iter().any(|p| {
                (p.from_region.min(p.to_region), p.from_region.max(p.to_region)) == (from.min(to), from.max(to))
                    && (p.x as i64 - x as i64).abs() <= near
                    && (p.y as i64 - y as i64).abs() <= near
            });
            if !duplicate {
                passages.push(Passage { x, y, from_region: from, to_region: to, link: k });
            }
        }
    }
    passages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::grid;
    use rand_chacha::rand_core::SeedableRng;

    fn split_for(config: &LevelConfig, seed: u64) -> (RegionSplit, ChaCha8Rng) {
        let shape = crate::generator::shape::build(config, seed);
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let wall = crate::generator::walls::build(config, &mut rng, &shape.shape.mask).unwrap();
        let split = crate::generator::regions::build(config, &mut rng, &wall.region).unwrap();
        (split, rng)
    }

    #[test]
    fn every_region_gets_a_node_and_the_network_is_connected() {
        for seed in 1..=6u64 {
            let mut config = LevelConfig::test_64();
            config.regions.count = 4;
            config.regions.shares = vec![10.0, 20.0, 30.0, 40.0];
            config.roads.node_count = 2; // below the region count: raised to 4
            let (split, mut rng) = split_for(&config, seed);
            let built = build(&config, &mut rng, &split).unwrap();
            let net = &built.network;
            for id in 1..=4u8 {
                assert!(net.nodes.iter().any(|n| n.region == id), "seed {seed}: region {id} has no node");
            }
            assert_eq!(net.nodes.len(), 4, "seed {seed}");
            assert!(grid::is_connected(&net.mask), "seed {seed}: roads are not one network");
            assert!(built.notes.is_empty(), "seed {seed}: {:?}", built.notes);
        }
    }

    #[test]
    fn node_count_is_honored_and_bigger_regions_get_more() {
        let mut config = LevelConfig::test_64();
        config.regions.count = 2;
        config.regions.shares = vec![20.0, 80.0];
        config.roads.node_count = 10;
        let (split, mut rng) = split_for(&config, 3);
        let net = build(&config, &mut rng, &split).unwrap().network;
        assert_eq!(net.nodes.len(), 10);
        let small = net.nodes.iter().filter(|n| n.region == 1).count();
        let big = net.nodes.iter().filter(|n| n.region == 2).count();
        assert!(small >= 1 && big > small, "small {small}, big {big}");
    }

    #[test]
    fn roads_cross_region_borders_at_passages() {
        let config = LevelConfig::test_64(); // 3 regions
        let (split, mut rng) = split_for(&config, 5);
        let net = build(&config, &mut rng, &split).unwrap().network;
        assert!(!net.passages.is_empty());
        let w = split.map.width;
        for p in &net.passages {
            assert_ne!(p.from_region, p.to_region);
            assert_eq!(split.map.ids[p.y * w + p.x], p.to_region);
        }
        // Every region is reachable: each one touches at least one passage.
        for id in 1..=3u8 {
            assert!(net.passages.iter().any(|p| p.from_region == id || p.to_region == id));
        }
    }

    #[test]
    fn zero_extra_links_gives_a_tree() {
        let mut config = LevelConfig::test_64();
        config.roads.extra_links_pct = 0.0;
        config.roads.node_count = 9;
        let (split, mut rng) = split_for(&config, 2);
        let net = build(&config, &mut rng, &split).unwrap().network;
        assert_eq!(net.links.len(), net.nodes.len() - 1);
        assert!(net.links.iter().all(|l| !l.extra));
    }

    #[test]
    fn busy_roads_are_wider_than_alleys() {
        let mut config = LevelConfig::test_64();
        config.roads.node_count = 12;
        config.roads.width_m = 3.0;
        config.roads.min_width_m = 1.0;
        let (split, mut rng) = split_for(&config, 4);
        let net = build(&config, &mut rng, &split).unwrap().network;
        let widths: Vec<u32> = net.links.iter().map(|l| l.width_cells).collect();
        assert_eq!(*widths.iter().max().unwrap(), 6, "main roads get the full 3 m");
        assert!(*widths.iter().min().unwrap() < 6, "some roads are narrower: {widths:?}");
        assert!(widths.iter().all(|&w| (2..=6).contains(&w)), "{widths:?}");
        // Equal widths: every road the same.
        config.roads.min_width_m = 3.0;
        let (split, mut rng) = split_for(&config, 4);
        let net = build(&config, &mut rng, &split).unwrap().network;
        assert!(net.links.iter().all(|l| l.width_cells == 6));
    }

    #[test]
    fn traffic_counts_routes_through_a_chain() {
        // A straight chain 0-1-2-3: the middle link carries the most trips.
        let nodes: Vec<RoadNode> = (0..4).map(|i| RoadNode { x: i * 10, y: 0, region: 1 }).collect();
        let links = vec![(0, 1, false), (1, 2, false), (2, 3, false)];
        assert_eq!(link_traffic(&nodes, &links), vec![3, 4, 3]);
        let widths = widths_from_traffic(&[3, 4, 3], 30.0, 1.0, 3.0);
        assert_eq!(widths[1], 3.0);
        assert!((widths[0] - 2.5).abs() < 1e-6);
    }

    #[test]
    fn density_mode_scales_with_area() {
        let mut config = LevelConfig::test_64();
        config.roads.amount = NodeAmount::Density;
        config.roads.nodes_per_1000m2 = 2.0;
        // 4000 cells × 0.25 m² = 1000 m² → 2 nodes.
        assert_eq!(node_budget(&config, 4000), 2);
    }

    #[test]
    fn same_seed_same_roads() {
        let config = LevelConfig::test_64();
        let (split, mut a) = split_for(&config, 9);
        let (_, mut b) = split_for(&config, 9);
        assert_eq!(build(&config, &mut a, &split).unwrap().network, build(&config, &mut b, &split).unwrap().network);
    }

    /// Eyeball helper: `cargo test ascii_roads_preview -- --nocapture`
    /// ('=' road, 'X' crossroad, '|' passage, digits = region ids).
    #[test]
    fn ascii_roads_preview() {
        let config = LevelConfig::test_64();
        let (split, mut rng) = split_for(&config, 42);
        let net = build(&config, &mut rng, &split).unwrap().network;
        let w = split.map.width;
        for y in (0..split.map.height).step_by(2) {
            let row: String = (0..w)
                .map(|x| {
                    let i = y * w + x;
                    if net.nodes.iter().any(|n| n.x.abs_diff(x) <= 1 && n.y.abs_diff(y) <= 1) {
                        'X'
                    } else if net.passages.iter().any(|p| p.x.abs_diff(x) <= 1 && p.y.abs_diff(y) <= 1) {
                        '|'
                    } else if net.mask.cells[i] {
                        '='
                    } else if split.map.ids[i] == 0 {
                        '.'
                    } else {
                        (b'0' + split.map.ids[i]) as char
                    }
                })
                .collect();
            println!("{row}");
        }
        println!("nodes {} links {} passages {}", net.nodes.len(), net.links.len(), net.passages.len());
    }
}
