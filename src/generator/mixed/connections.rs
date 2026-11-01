//! Mixed node 5 · Connections (step M3 in docs/mixed-plan.md): where one
//! map region opens into the next, where the map gates lead, and where a
//! river that cuts a region apart is crossed. Decided once, for the whole
//! map, so the two sides of a border never disagree.
//!
//! 1. **Links** — every touching region pair. A random spanning tree keeps
//!    every region reachable; every other pair is linked with the loops
//!    chance (the labyrinth slider). A nested region only touches the one
//!    around it, so that is its only link.
//! 2. **Portals** — one per link, on the shared border, at the spot that
//!    suits both sides: dry, flat across the border, real ground of each
//!    region a few meters in on both sides (no thin necks), off the map
//!    edge and away from the regions' other portals. A little jitter keeps
//!    them from always taking the same kind of spot.
//! 3. **Map gates** — each gate (direction from the map center) meets the
//!    map edge in some region; that region gets it. A gate on deep water
//!    slides along the edge to the nearest dry spot.
//! 4. **River crossings** — a region minus its deep water may fall into
//!    pieces. The pieces are joined by the shortest crossings over the
//!    water (a tree): bridges in a town, fords over narrow rivers and
//!    bridges over wide ones elsewhere. Tiny pieces are left out (the
//!    region's chain fills them).

use std::collections::{BTreeMap, VecDeque};

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::gates::{GATE_NAME, MARKER_M};
use crate::generator::grid;
use crate::level::{
    Category, CellMask, MixedConnections, MixedCrossing, MixedCrossingLook, MixedGate, MixedLayout, MixedLink, MixedPortal, PlacedObject,
    TerrainLayout, WaterLayout,
};
use crate::model::{LevelConfig, MapKind};

pub struct ConnectionBuild {
    pub layout: MixedConnections,
    /// The gate markers (gate i's marker is `objects[i]`-th of these).
    pub objects: Vec<PlacedObject>,
    pub notes: Vec<String>,
}

/// How far into each region a portal must find its region's ground,
/// meters (no portal at a neck a few cells wide).
const INSIDE_M: f32 = 6.0;
/// Least gap between two portals of one region, meters.
const PORTAL_GAP_M: f32 = 24.0;
/// Most border spots scored per link.
const MAX_CANDIDATES: usize = 600;
/// How far a gate on deep water may slide along the map edge, meters.
const GATE_SLIDE_M: f32 = 40.0;
/// River pieces smaller than this, m², get no crossing.
const MIN_PIECE_M2: f32 = 40.0;

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, m: &MixedLayout, land: &TerrainLayout, water: &WaterLayout) -> ConnectionBuild {
    let s = &config.mixed.connections;
    let (w, h, cell) = (m.map.width, m.map.height, m.map.cell_size_m);
    let ids = &m.map.ids;
    let mut notes = Vec::new();
    let center = |c: usize| [((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell];
    let cell_at = |p: [f32; 2]| {
        let x = ((p[0] / cell).floor().max(0.0) as usize).min(w - 1);
        let y = ((p[1] / cell).floor().max(0.0) as usize).min(h - 1);
        y * w + x
    };
    let kind_of = |id: u8| m.regions.get((id as usize).wrapping_sub(1)).map_or(MapKind::Forest, |r| r.kind);

    // 1. Links: the border cell pairs of every touching region pair.
    let mut borders: BTreeMap<(u8, u8), Vec<(usize, usize)>> = BTreeMap::new();
    for c in 0..w * h {
        let (x, y) = (c % w, c / w);
        for nb in [(x + 1 < w).then(|| c + 1), (y + 1 < h).then(|| c + w)].into_iter().flatten() {
            let (a, b) = (ids[c], ids[nb]);
            if a != b && a > 0 && b > 0 {
                let (lo, cl, cb) = if a < b { (a, c, nb) } else { (b, nb, c) };
                borders.entry((lo, a.max(b))).or_default().push((cl, cb));
            }
        }
    }
    let pairs: Vec<(u8, u8)> = borders.keys().copied().collect();
    let weights: Vec<f32> = pairs.iter().map(|_| rng.random::<f32>()).collect();
    let rolls: Vec<f32> = pairs.iter().map(|_| rng.random::<f32>()).collect();
    let mut order: Vec<usize> = (0..pairs.len()).collect();
    order.sort_by(|&i, &j| weights[i].total_cmp(&weights[j]).then(i.cmp(&j)));
    let mut parent: Vec<usize> = (0..=m.regions.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut tree = vec![false; pairs.len()];
    for &i in &order {
        let (a, b) = pairs[i];
        let (ra, rb) = (find(&mut parent, a as usize), find(&mut parent, b as usize));
        if ra != rb {
            parent[ra] = rb;
            tree[i] = true;
        }
    }
    let loops = (s.loops_pct / 100.0).clamp(0.0, 1.0);
    let mut links: Vec<MixedLink> = pairs
        .iter()
        .enumerate()
        .map(|(i, &(a, b))| MixedLink {
            regions: [a, b],
            border_m: borders[&(a, b)].len() as f32 * cell,
            linked: tree[i] || rolls[i] < loops,
            tree: tree[i],
            portal: None,
        })
        .collect();

    // 2. Portals: tree links first (they must exist), then the loops.
    let width = s.portal_width_m.max(cell);
    let reach = ((INSIDE_M / cell).round() as i64).max(1);
    let window = ((4.0 / cell).round() as i64).max(1);
    let water_r = ((width / 2.0 + 1.5) / cell).ceil() as i64;
    let mut portals: Vec<MixedPortal> = Vec::new();
    let mut link_order: Vec<usize> = (0..links.len()).filter(|&i| links[i].linked).collect();
    link_order.sort_by_key(|&i| (!links[i].tree, i));
    for li in link_order {
        let [a, b] = links[li].regions;
        let edges = &borders[&(a, b)];
        let step = edges.len().div_ceil(MAX_CANDIDATES).max(1);
        let mut best: Option<(f32, [f32; 2], [f32; 2], bool)> = None;
        for &(ca, cb) in edges.iter().step_by(step) {
            let (pa, pb) = (center(ca), center(cb));
            let p = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0];
            // Across the border: away from a's cells, toward b's.
            let (cx, cy) = ((ca % w) as i64, (ca / w) as i64);
            let (mut nx, mut ny) = (0.0f32, 0.0f32);
            for dy in -window..=window {
                for dx in -window..=window {
                    let (x, y) = (cx + dx, cy + dy);
                    if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                        continue;
                    }
                    let id = ids[y as usize * w + x as usize];
                    let sign = if id == b { 1.0 } else if id == a { -1.0 } else { 0.0 };
                    nx += sign * dx as f32;
                    ny += sign * dy as f32;
                }
            }
            let len = nx.hypot(ny);
            let n = if len > 1e-3 { [nx / len, ny / len] } else { [pb[0] - pa[0], pb[1] - pa[1]] };
            let mut score = rng.random::<f32>() * 10.0;
            // Real ground of each region a few meters in.
            let d = reach as f32 * cell;
            let (ia, ib) = (cell_at([p[0] - n[0] * d, p[1] - n[1] * d]), cell_at([p[0] + n[0] * d, p[1] + n[1] * d]));
            if ids[ia] != a {
                score += 300.0;
            }
            if ids[ib] != b {
                score += 300.0;
            }
            // Flat across the border.
            score += (land.height_m[ia] - land.height_m[ib]).abs() * land.relief_m * 20.0;
            // Dry.
            let mut deep = false;
            let (px, py) = ((ca % w) as i64, (ca / w) as i64);
            let mut wet = 0.0;
            for dy in -water_r..=water_r {
                for dx in -water_r..=water_r {
                    let (x, y) = (px + dx, py + dy);
                    if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 || dx * dx + dy * dy > water_r * water_r {
                        continue;
                    }
                    let q = y as usize * w + x as usize;
                    if water.deep.cells[q] {
                        deep = true;
                        wet += 20.0;
                    } else if water.shallow.cells[q] {
                        wet += 2.0;
                    }
                }
            }
            score += wet + if deep { 1000.0 } else { 0.0 };
            // Off the map edge.
            let edge = p[0].min(p[1]).min(w as f32 * cell - p[0]).min(h as f32 * cell - p[1]);
            if edge < 2.0 * width {
                score += 200.0;
            }
            // Away from the regions' other portals.
            for q in portals.iter().filter(|q| q.regions.contains(&a) || q.regions.contains(&b)) {
                let dist = (q.point_m[0] - p[0]).hypot(q.point_m[1] - p[1]);
                if dist < PORTAL_GAP_M {
                    score += (PORTAL_GAP_M - dist) * 20.0;
                }
            }
            if best.is_none_or(|bst| score < bst.0) {
                best = Some((score, p, n, deep));
            }
        }
        let Some((_, p, n, over_water)) = best else { continue };
        let id = portals.len() as u32 + 1;
        if over_water {
            notes.push(format!("Connections: portal {id} (regions {a}–{b}) crosses water — it gets a bridge"));
        }
        portals.push(MixedPortal { id, regions: [a, b], kinds: [kind_of(a), kind_of(b)], point_m: p, normal: n, width_m: width, over_water });
        links[li].portal = Some(id);
    }

    // 3. Map gates.
    let (wm, hm) = (w as f32 * cell, h as f32 * cell);
    let mut gates = Vec::new();
    let mut objects = Vec::new();
    for (i, spec) in config.gates.gates.iter().enumerate() {
        let (p0, inward) = crate::generator::swamp::gate_anchors::edge_hit((wm / 2.0, hm / 2.0), spec.direction_deg.rem_euclid(360.0), wm, hm);
        // Along the edge: perpendicular to the inward direction.
        let along = (-inward.1, inward.0);
        let slide = (GATE_SLIDE_M / cell).ceil() as i32;
        let spot = (0..=slide).flat_map(|k| [k, -k]).find_map(|k| {
            let q = [
                (p0.0 + along.0 * k as f32 * cell + inward.0 * cell * 0.5).clamp(0.0, wm - 0.01),
                (p0.1 + along.1 * k as f32 * cell + inward.1 * cell * 0.5).clamp(0.0, hm - 0.01),
            ];
            let c = cell_at(q);
            (!water.deep.cells[c]).then_some((q, c))
        });
        let Some((q, c)) = spot else {
            notes.push(format!("Gate #{}: deep water all along the map edge there — skipped", i + 1));
            continue;
        };
        let pm = [q[0] + inward.0 * MARKER_M / 2.0, q[1] + inward.1 * MARKER_M / 2.0];
        objects.push(PlacedObject {
            category: Category::Gate,
            type_name: GATE_NAME.to_owned(),
            color: config.gates.color,
            x_m: pm[0] - MARKER_M / 2.0,
            y_m: pm[1] - MARKER_M / 2.0,
            w_m: MARKER_M,
            h_m: MARKER_M,
            angle_deg: 0.0,
            parts: Vec::new(),
            round: false,
        });
        gates.push(MixedGate {
            id: i as u32 + 1,
            direction_deg: spec.direction_deg,
            point_m: q,
            inward: [inward.0, inward.1],
            region: ids[c],
            kind: kind_of(ids[c]),
            object: objects.len() - 1,
        });
    }

    // 4. River crossings.
    let crossings = crossings(s, m, water, &mut notes);

    let loops_n = links.iter().filter(|l| l.linked && !l.tree).count() as u32;
    ConnectionBuild { layout: MixedConnections { links, portals, gates, crossings, loops: loops_n }, objects, notes }
}

/// The shortest way over the water between two pieces: (length, meters;
/// bank cell of the lower piece; bank cell of the other).
type Shortest = (f32, usize, usize);

/// Node 5, part 4: per region, the pieces left by its deep water joined by
/// the shortest crossings (a tree over the pieces).
fn crossings(s: &crate::model::MixedConnectionSettings, m: &MixedLayout, water: &WaterLayout, notes: &mut Vec<String>) -> Vec<MixedCrossing> {
    let (w, h, cell) = (m.map.width, m.map.height, m.map.cell_size_m);
    let ids = &m.map.ids;
    let center = |c: usize| [((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell];
    let mut out = Vec::new();
    for r in &m.regions {
        if water.deep.count_true() == 0 && water.shallow.count_true() == 0 {
            break;
        }
        // A town builds on no water: shallow brooks cut it too (and get a
        // bridge); elsewhere shallow water is walked through.
        let town = r.kind == MapKind::Town;
        let wet = |c: usize| water.deep.cells[c] || (town && water.shallow.cells[c]);
        let ground = CellMask { width: w, height: h, cell_size_m: cell, cells: (0..w * h).map(|c| ids[c] == r.id && !wet(c)).collect() };
        let (labels, count) = grid::components(&ground);
        if count < 2 {
            continue;
        }
        let mut size = vec![0u32; count as usize + 1];
        for &l in &labels {
            size[l as usize] += 1;
        }
        let min_cells = (MIN_PIECE_M2 / (cell * cell)).ceil() as u32;
        let big: Vec<u32> = (1..=count).filter(|&l| size[l as usize] >= min_cells).collect();
        if big.len() < 2 {
            continue;
        }
        // From each big piece, a BFS over the region's water (and its tiny
        // pieces) to the nearest cell of every other big piece.
        let is_big = |l: u32| l > 0 && size[l as usize] >= min_cells;
        let mut found: BTreeMap<(u32, u32), Shortest> = BTreeMap::new();
        let mut origin = vec![usize::MAX; w * h];
        let mut stamp = vec![0u32; w * h];
        for &p in &big {
            let mut queue = VecDeque::new();
            for c in 0..w * h {
                if labels[c] == p {
                    let shore = crate::generator::forest::neighbors4(c, w, h).any(|q| ids[q] == r.id && labels[q] != p);
                    if shore {
                        stamp[c] = p;
                        origin[c] = c;
                        queue.push_back(c);
                    }
                }
            }
            while let Some(c) = queue.pop_front() {
                for q in crate::generator::forest::water::neighbors8(c, w, h) {
                    if ids[q] != r.id || stamp[q] == p {
                        continue;
                    }
                    let lq = labels[q];
                    if is_big(lq) {
                        if lq != p {
                            let (a, b) = (center(origin[c]), center(q));
                            let d = (a[0] - b[0]).hypot(a[1] - b[1]);
                            let key = (p.min(lq), p.max(lq));
                            let (from, to) = if p < lq { (origin[c], q) } else { (q, origin[c]) };
                            if found.get(&key).is_none_or(|f| d < f.0) {
                                found.insert(key, (d, from, to));
                            }
                        }
                        continue;
                    }
                    stamp[q] = p;
                    origin[q] = origin[c];
                    queue.push_back(q);
                }
            }
        }
        let mut edges: Vec<((u32, u32), Shortest)> = found.into_iter().collect();
        edges.sort_by(|x, y| x.1 .0.total_cmp(&y.1 .0).then(x.0.cmp(&y.0)));
        let mut parent: Vec<u32> = (0..=count).collect();
        fn find(p: &mut [u32], mut i: u32) -> u32 {
            while p[i as usize] != i {
                p[i as usize] = p[p[i as usize] as usize];
                i = p[i as usize];
            }
            i
        }
        let mut joined = 1;
        for ((a, b), (d, from, to)) in edges {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra == rb {
                continue;
            }
            parent[ra as usize] = rb;
            joined += 1;
            let look = if r.kind != MapKind::Town && d <= s.ford_max_m { MixedCrossingLook::Ford } else { MixedCrossingLook::Bridge };
            out.push(MixedCrossing { id: out.len() as u32 + 1, region: r.id, look, from_m: center(from), to_m: center(to), width_m: s.crossing_width_m.max(cell) });
        }
        if joined < big.len() {
            notes.push(format!("Connections: region {} ({}) stays cut by water in {} pieces", r.id, r.name, big.len() - joined + 1));
        }
    }
    out
}

/// The cells within `r` meters of the segment a–b (a capsule), meters in,
/// cell indexes out.
pub fn capsule_cells(w: usize, h: usize, cell: f32, a: [f32; 2], b: [f32; 2], r: f32) -> Vec<usize> {
    let x0 = ((a[0].min(b[0]) - r) / cell).floor().max(0.0) as usize;
    let x1 = (((a[0].max(b[0]) + r) / cell).ceil() as usize).min(w);
    let y0 = ((a[1].min(b[1]) - r) / cell).floor().max(0.0) as usize;
    let y1 = (((a[1].max(b[1]) + r) / cell).ceil() as usize).min(h);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = (dx * dx + dy * dy).max(1e-9);
    let mut out = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let p = ((x as f32 + 0.5) * cell, (y as f32 + 0.5) * cell);
            let t = (((p.0 - a[0]) * dx + (p.1 - a[1]) * dy) / len2).clamp(0.0, 1.0);
            if (p.0 - a[0] - dx * t).hypot(p.1 - a[1] - dy * t) <= r {
                out.push(y * w + x);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::GeneratedLevel;

    fn level(seed: u64, tweak: impl Fn(&mut LevelConfig)) -> GeneratedLevel {
        let mut c = LevelConfig::of_kind(MapKind::Mixed);
        c.map.width_m = 140.0;
        c.map.height_m = 140.0;
        c.cell_size_m = 1.0;
        tweak(&mut c);
        crate::generator::generate(&c, seed)
    }

    #[test]
    fn portals_link_every_region_on_dry_borders() {
        for seed in 1..=6 {
            let lvl = level(seed, |_| {});
            let m = lvl.mixed.as_ref().unwrap();
            let cn = m.connections.as_ref().unwrap();
            let (w, cell) = (m.map.width, m.map.cell_size_m);
            // One piece through the portals.
            let mut parent: Vec<usize> = (0..=m.regions.len()).collect();
            fn find(p: &mut [usize], mut i: usize) -> usize {
                while p[i] != i {
                    i = p[i];
                }
                i
            }
            for p in &cn.portals {
                let (a, b) = (find(&mut parent, p.regions[0] as usize), find(&mut parent, p.regions[1] as usize));
                parent[a] = b;
                // On the border: both regions within a few meters.
                let near = |id: u8| {
                    (0..m.map.ids.len()).any(|c| m.map.ids[c] == id && (((c % w) as f32 + 0.5) * cell - p.point_m[0]).hypot(((c / w) as f32 + 0.5) * cell - p.point_m[1]) < 2.0)
                };
                assert!(near(p.regions[0]) && near(p.regions[1]), "seed {seed}: portal {} off its border", p.id);
                assert!(!p.over_water, "seed {seed}: portal {} on water", p.id);
            }
            let root = find(&mut parent, 1);
            for r in &m.regions {
                assert_eq!(find(&mut parent, r.id as usize), root, "seed {seed}: region {} unreachable", r.id);
            }
            for g in &cn.gates {
                let c = ((g.point_m[1] / cell) as usize).min(m.map.height - 1) * w + ((g.point_m[0] / cell) as usize).min(w - 1);
                assert_eq!(m.map.ids[c], g.region, "seed {seed}: gate {}", g.id);
                assert_eq!(lvl.objects[g.object].category, Category::Gate);
            }
        }
    }

    #[test]
    fn labyrinth_slider_runs_from_tree_to_every_pair() {
        let tree = level(3, |c| c.mixed.connections.loops_pct = 0.0);
        let cn = tree.mixed.as_ref().unwrap().connections.as_ref().unwrap();
        assert_eq!(cn.portals.len(), tree.mixed.as_ref().unwrap().regions.len() - 1);
        assert_eq!(cn.loops, 0);
        let all = level(3, |c| c.mixed.connections.loops_pct = 100.0);
        let cn = all.mixed.as_ref().unwrap().connections.as_ref().unwrap();
        assert!(cn.links.iter().all(|l| l.linked && l.portal.is_some()));
    }

    #[test]
    fn crossings_join_every_region_cut_by_a_river() {
        let mut crossed = 0;
        for seed in 1..=8 {
            let lvl = level(seed, |c| {
                c.mixed.water.streams = 4;
                c.mixed.water.deep_from_m = 2.0;
                c.mixed.water.tributaries_pct = 80.0;
            });
            let m = lvl.mixed.as_ref().unwrap();
            let (cn, wl) = (m.connections.as_ref().unwrap(), m.water.as_ref().unwrap());
            let (w, h, cell) = (m.map.width, m.map.height, m.map.cell_size_m);
            let mut walk: Vec<bool> = (0..w * h).map(|c| !wl.deep.cells[c]).collect();
            for x in &cn.crossings {
                crossed += 1;
                for c in capsule_cells(w, h, cell, x.from_m, x.to_m, x.width_m / 2.0) {
                    walk[c] = true;
                }
            }
            for r in &m.regions {
                let ground = CellMask { width: w, height: h, cell_size_m: cell, cells: (0..w * h).map(|c| m.map.ids[c] == r.id && walk[c]).collect() };
                let (labels, count) = grid::components(&ground);
                let mut size = vec![0u32; count as usize + 1];
                for &l in &labels {
                    size[l as usize] += 1;
                }
                let big = size.iter().skip(1).filter(|&&s| s as f32 * cell * cell >= MIN_PIECE_M2).count();
                assert!(big <= 1, "seed {seed}: region {} ({:?}) in {big} pieces; notes {:?}", r.id, r.kind, lvl.notes);
            }
        }
        assert!(crossed > 0, "no river ever cut a region");
    }
}
