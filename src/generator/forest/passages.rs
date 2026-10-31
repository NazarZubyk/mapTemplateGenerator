//! Forest node 6, passages (step F5 in docs/forest-plan.md): which areas
//! the player can walk between, and the ways in from the gates. The
//! barriers are the land itself (thicket, rock, deep water); this node
//! only decides where they hold and where they open.
//!
//! 1. **Area graph** — a spanning tree over the touching areas from the
//!    first gate's area (labyrinth slider: chain → tree), then loops.
//! 2. **Seal** — along every border that is *not* linked, a strip
//!    (`seal_width_m`, wobbled) closes any natural gap: land grows dense,
//!    a brook turns deep there (the water stays one line). These cells are
//!    locked: nothing later cuts them.
//! 3. **One piece per area** — walkable pockets inside an area are joined
//!    to its main piece along the cheapest way (or filled if tiny).
//! 4. **Links** — per linked pair the cheapest way from one area into the
//!    other through both (walkable ground cheap, thicket dearer, deep
//!    water dearer still, rock dearest): it takes a natural gap when there
//!    is one, else the thinnest spot of the barrier, and is cut there — a
//!    trail through forest, a pass through rock, a ford or bridge over deep
//!    water (`bridge_pct`).
//! 5. **Gates** — per gate a point on the map edge (slid off water, rock and
//!    sealed strips) and a trail from it into its area's ground.
//! 6. **Cleanup** — anything the gates cannot reach is filled.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::forest::{distance_field, neighbors4};
use crate::generator::forest::water::neighbors8;
use crate::generator::gates::{GATE_NAME, MARKER_M};
use crate::generator::grid;
use crate::generator::noise::NoiseField;
use crate::generator::swamp::gate_anchors::edge_hit;
use crate::level::{AreaLayout, AreaLink, Category, CellMask, CrossingKind, ForestGate, ForestGround, PassageLayout, PlacedObject, ThicketLayout};
use crate::model::{LabyrinthParams, LevelConfig};

pub struct PassageBuild {
    pub layout: PassageLayout,
    /// One marker per gate; `layout.gates[..].object` indexes here.
    pub objects: Vec<PlacedObject>,
    pub notes: Vec<String>,
}

/// A border shorter than this many trail widths links only when needed.
const MIN_LINK: f32 = 2.5;
/// Walkable pockets smaller than this, m², are filled instead of joined.
const POCKET_M2: f32 = 30.0;
/// Step costs (×10 per meter-ish cell step).
const COST_WALK: u32 = 10;
const COST_DENSE: u32 = 70;
const COST_DEEP: u32 = 120;
const COST_ROCK: u32 = 350;

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, areas: &AreaLayout, thickets: &ThicketLayout) -> PassageBuild {
    let s = &config.forest.passages;
    let (w, h, cell) = (areas.width, areas.height, areas.cell_size_m);
    let n = w * h;
    let (wm, hm) = (w as f32 * cell, h as f32 * cell);
    let ids = &areas.ids;
    let k = areas.areas.len();
    let mut notes = Vec::new();
    let mut ground: Vec<ForestGround> = thickets.ground.iter().map(|&g| ForestGround::from_code(g)).collect();
    let wiggle = NoiseField::new(rng.random::<u64>());
    let seal_noise = NoiseField::new(rng.random::<u64>());
    let trail_w = s.trail_width_m.max(cell);

    // Gate points on the map edge (inward normal).
    let inset = cell / 2.0;
    let gate_points: Vec<((f32, f32), (f32, f32))> = config
        .gates
        .gates
        .iter()
        .map(|g| {
            let (p, nrm) = edge_hit((wm / 2.0, hm / 2.0), g.direction_deg.rem_euclid(360.0), wm, hm);
            ((p.0.clamp(inset, wm - inset), p.1.clamp(inset, hm - inset)), nrm)
        })
        .collect();
    let cell_at = |p: (f32, f32)| ((p.1 / cell) as usize).min(h - 1) * w + ((p.0 / cell) as usize).min(w - 1);

    // 1. Area graph.
    let lab = LabyrinthParams::from_pct(s.labyrinth_pct);
    let edges = &areas.borders;
    let center_of = |id: u16| areas.areas[id as usize - 1].center_m;
    let dist = |a: u16, b: u16| {
        let (p, q) = (center_of(a), center_of(b));
        (p[0] - q[0]).hypot(p[1] - q[1])
    };
    let short = |e: usize| edges[e].length_m < MIN_LINK * trail_w;
    let mut linked = vec![false; edges.len()];
    let mut extra = vec![false; edges.len()];
    let start = gate_points.first().map(|&(p, _)| ids[cell_at(p)]).unwrap_or(1);
    if k > 0 {
        let mut in_tree = vec![false; k + 1];
        in_tree[start as usize] = true;
        let mut tail = start;
        for _ in 1..k {
            let branch = rng.random::<f32>() * 100.0 < lab.branching_pct;
            let pick = |from_ok: &dyn Fn(u16) -> bool| {
                let mut best: Option<(usize, u16, (u8, f32))> = None;
                for (e, b) in edges.iter().enumerate() {
                    let (from, to) = if from_ok(b.a) && in_tree[b.a as usize] && !in_tree[b.b as usize] {
                        (b.a, b.b)
                    } else if from_ok(b.b) && in_tree[b.b as usize] && !in_tree[b.a as usize] {
                        (b.b, b.a)
                    } else {
                        continue;
                    };
                    let key = (short(e) as u8, dist(from, to));
                    if best.as_ref().is_none_or(|x| key.0 < x.2 .0 || (key.0 == x.2 .0 && key.1 < x.2 .1)) {
                        best = Some((e, to, key));
                    }
                }
                best
            };
            let found = if branch { pick(&|_| true) } else { pick(&|a| a == tail).or_else(|| pick(&|_| true)) };
            let Some((e, to, _)) = found else { break };
            linked[e] = true;
            in_tree[to as usize] = true;
            tail = to;
        }
    }
    for (e, x) in extra.iter_mut().enumerate() {
        // Always roll, so the stream does not depend on earlier outcomes.
        let roll = rng.random::<f32>() * 100.0;
        if !linked[e] && !short(e) && roll < lab.loops_pct {
            linked[e] = true;
            *x = true;
        }
    }
    let is_linked = |a: u16, b: u16| edges.iter().zip(&linked).any(|(e, &l)| l && (e.a, e.b) == (a.min(b), a.max(b)));

    // 2. Seal the unlinked borders.
    let seal_src = |c: usize| neighbors4(c, w, h).any(|nb| ids[nb] != ids[c] && !is_linked(ids[c], ids[nb]));
    let seal_half = s.seal_width_m.max(cell) / 2.0;
    let (to_seal, _) = distance_field(w, h, cell, seal_src, seal_half * 1.4 + cell);
    let mut locked = vec![false; n];
    for c in 0..n {
        let (x, y) = ((c % w) as f32 * cell, (c / w) as f32 * cell);
        let wob = 0.7 + 0.6 * seal_noise.fbm(x / 10.0, y / 10.0, 2);
        if to_seal[c] <= seal_half * wob {
            locked[c] = true;
            ground[c] = blocked(ground[c]);
        }
    }

    let step_cost = |g: ForestGround, c: usize| -> u32 {
        let (x, y) = ((c % w) as f32 * cell, (c / w) as f32 * cell);
        let jitter = (wiggle.fbm(x / 8.0, y / 8.0, 2) * 8.0) as u32;
        match g {
            ForestGround::Thicket => COST_DENSE + jitter * 2,
            ForestGround::Deep => COST_DEEP,
            ForestGround::Rock => COST_ROCK,
            _ => COST_WALK + jitter,
        }
    };
    let pocket = (POCKET_M2 / (cell * cell)).ceil() as usize;
    let r_trail = trail_w / 2.0 / cell;

    // 3. One piece per area.
    for id in 1..=k as u16 {
        for _ in 0..16 {
            let mask = CellMask { width: w, height: h, cell_size_m: cell, cells: (0..n).map(|c| ids[c] == id && ground[c].walkable()).collect() };
            let (labels, pieces) = grid::components(&mask);
            if pieces == 0 {
                // No walkable ground at all: a small clearing at the center.
                let ctr = center_of(id);
                let best = (0..n)
                    .filter(|&c| ids[c] == id && !locked[c] && ground[c] != ForestGround::Deep)
                    .min_by(|&p, &q| {
                        let d = |c: usize| (((c % w) as f32 + 0.5) * cell - ctr[0]).hypot(((c / w) as f32 + 0.5) * cell - ctr[1]);
                        d(p).total_cmp(&d(q)).then(p.cmp(&q))
                    });
                let Some(b) = best else { break };
                carve(&mut ground, &locked, w, h, &[b], (3.0 / cell).max(1.0), |c| ids[c] == id, CrossingKind::Trail);
                notes.push(format!("Passages: area {id} had no walkable ground; a clearing was cut"));
                continue;
            }
            if pieces <= 1 {
                break;
            }
            let mut size = vec![0usize; pieces as usize + 1];
            for &l in &labels {
                size[l as usize] += 1;
            }
            let main = (1..=pieces as usize).max_by_key(|&i| (size[i], Reverse(i))).unwrap_or(1);
            let mut changed = false;
            for (l, &sz) in size.iter().enumerate().skip(1) {
                if l != main && sz < pocket {
                    for c in (0..n).filter(|&c| labels[c] as usize == l) {
                        ground[c] = blocked(ground[c]);
                    }
                    changed = true;
                }
            }
            let other = (1..=pieces as usize).filter(|&l| l != main && size[l] >= pocket).max_by_key(|&l| (size[l], Reverse(l)));
            if let Some(o) = other {
                let src: Vec<usize> = (0..n).filter(|&c| labels[c] as usize == o).collect();
                let path = cheapest(w, h, &src, |c| labels[c] as usize == main, |c| (ids[c] == id && !locked[c]).then(|| step_cost(ground[c], c)));
                if let Some(path) = path {
                    let kind = crossing_kind(&ground, &path, rng, s.bridge_pct);
                    carve(&mut ground, &locked, w, h, &path, r_trail, |c| ids[c] == id, kind);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    // 4. Links.
    let mut links = Vec::with_capacity(edges.len());
    for (e, b) in edges.iter().enumerate() {
        let mut link = AreaLink { a: b.a, b: b.b, linked: linked[e], extra: extra[e], crossing: None, point_m: None };
        if linked[e] {
            let src: Vec<usize> = (0..n).filter(|&c| ids[c] == b.a && ground[c].walkable()).collect();
            let inside = |c: usize| ids[c] == b.a || ids[c] == b.b;
            let target = |c: usize| ids[c] == b.b && ground[c].walkable();
            // Off the sealed strips first; if they close the whole border
            // (a short border between two sealed ones), through them. That
            // is safe: a sealed border is sealed on both sides, and this
            // cuts only inside a and b, so the far side stays closed.
            let path = cheapest(w, h, &src, target, |c| (inside(c) && !locked[c]).then(|| step_cost(ground[c], c)))
                .or_else(|| cheapest(w, h, &src, target, |c| inside(c).then(|| step_cost(ground[c], c))));
            match path {
                Some(path) => {
                    let kind = crossing_kind(&ground, &path, rng, s.bridge_pct);
                    let at = path.windows(2).find(|p| ids[p[0]] != ids[p[1]]).map(|p| p[0]).unwrap_or(path[0]);
                    let free = vec![false; n];
                    carve(&mut ground, if path.iter().any(|&c| locked[c]) { &free } else { &locked }, w, h, &path, r_trail, inside, kind);
                    link.crossing = Some(kind);
                    link.point_m = Some([((at % w) as f32 + 0.5) * cell, ((at / w) as f32 + 0.5) * cell]);
                }
                None => {
                    // Rock or deep water all along: they stay apart.
                    link.linked = false;
                    link.extra = false;
                    notes.push(format!("Passages: areas {} and {} could not be linked (rock or deep water all along)", b.a, b.b));
                }
            }
        }
        links.push(link);
    }

    // 5. Gates.
    let mut gates = Vec::new();
    let mut objects = Vec::new();
    for (i, (&(p0, nrm), spec)) in gate_points.iter().zip(&config.gates.gates).enumerate() {
        let id = i as u32 + 1;
        let along = (-nrm.1, nrm.0);
        let ok = |c: usize| !locked[c] && !matches!(ground[c], ForestGround::Deep | ForestGround::Rock);
        let max_slide = (wm.max(hm) / cell) as i32;
        let Some(gc) = (0..max_slide)
            .flat_map(|k| [k, -k])
            .map(|k| (p0.0 + along.0 * k as f32 * cell, p0.1 + along.1 * k as f32 * cell))
            .filter(|q| q.0 >= 0.0 && q.1 >= 0.0 && q.0 < wm && q.1 < hm)
            .map(cell_at)
            .find(|&c| ok(c))
        else {
            notes.push(format!("Gate #{id}: no open spot on the map edge"));
            continue;
        };
        let area = ids[gc];
        let path = cheapest(w, h, &[gc], |c| ids[c] == area && ground[c].walkable() && !is_edge_cell(c, w, h, 3), |c| {
            (ids[c] == area && !locked[c]).then(|| step_cost(ground[c], c))
        });
        if let Some(path) = path {
            let kind = crossing_kind(&ground, &path, rng, s.bridge_pct);
            carve(&mut ground, &locked, w, h, &path, r_trail, |c| ids[c] == area, kind);
        }
        if !ground[gc].walkable() {
            ground[gc] = ForestGround::Trail;
        }
        let pm = (((gc % w) as f32 + 0.5) * cell, ((gc / w) as f32 + 0.5) * cell);
        objects.push(PlacedObject {
            category: Category::Gate,
            type_name: GATE_NAME.to_owned(),
            color: config.gates.color,
            x_m: pm.0 - MARKER_M / 2.0,
            y_m: pm.1 - MARKER_M / 2.0,
            w_m: MARKER_M,
            h_m: MARKER_M,
            angle_deg: 0.0,
            parts: Vec::new(),
            round: false,
        });
        gates.push(ForestGate { id, direction_deg: spec.direction_deg, point_m: [pm.0, pm.1], area, object: objects.len() - 1 });
    }

    // 6. Cleanup: keep what the first gate (or the biggest piece) reaches.
    let walk = CellMask { width: w, height: h, cell_size_m: cell, cells: ground.iter().map(|g| g.walkable()).collect() };
    let (labels, pieces) = grid::components(&walk);
    let mut size = vec![0usize; pieces as usize + 1];
    for &l in &labels {
        size[l as usize] += 1;
    }
    let main = gates
        .first()
        .map(|g| labels[cell_at((g.point_m[0], g.point_m[1]))] as usize)
        .filter(|&l| l > 0)
        .or_else(|| (1..=pieces as usize).max_by_key(|&i| (size[i], Reverse(i))))
        .unwrap_or(0);
    let mut filled = 0usize;
    for c in 0..n {
        if labels[c] > 0 && labels[c] as usize != main {
            ground[c] = blocked(ground[c]);
            filled += 1;
        }
    }
    let filled_m2 = filled as f32 * cell * cell;
    if filled_m2 > POCKET_M2 * 4.0 {
        notes.push(format!("Passages: {filled_m2:.0} m² the gates cannot reach was filled"));
    }

    // Stats.
    let mut degree = vec![0u32; k + 1];
    for l in links.iter().filter(|l| l.linked) {
        degree[l.a as usize] += 1;
        degree[l.b as usize] += 1;
    }
    let dead_end_areas = degree[1..].iter().filter(|&&d| d == 1).count() as u32;
    let loops = links.iter().filter(|l| l.extra).count() as u32;
    let sources: Vec<u16> = if gates.is_empty() { vec![start] } else { gates.iter().map(|g| g.area).collect() };
    let main_route_areas = farthest_hops(k, &links, &sources) + 1;
    let walkable = ground.iter().filter(|g| g.walkable()).count();
    PassageBuild {
        layout: PassageLayout {
            width: w,
            height: h,
            cell_size_m: cell,
            ground: ground.iter().map(|g| g.code()).collect(),
            sealed: CellMask { width: w, height: h, cell_size_m: cell, cells: locked },
            links,
            gates,
            main_route_areas,
            loops,
            dead_end_areas,
            walkable_pct: walkable as f32 / n.max(1) as f32 * 100.0,
        },
        objects,
        notes,
    }
}

/// The blocked ground a walkable cell turns into when it must close: water
/// deepens (so a brook stays one line of water), land grows dense.
fn blocked(g: ForestGround) -> ForestGround {
    match g {
        ForestGround::Shallow | ForestGround::Bridge => ForestGround::Deep,
        g if g.walkable() => ForestGround::Thicket,
        g => g,
    }
}

fn is_edge_cell(c: usize, w: usize, h: usize, margin: usize) -> bool {
    let (x, y) = (c % w, c / w);
    x < margin || y < margin || x + margin >= w || y + margin >= h
}

/// What a way through the barriers is, by the hardest ground on it.
fn crossing_kind(ground: &[ForestGround], path: &[usize], rng: &mut ChaCha8Rng, bridge_pct: f32) -> CrossingKind {
    let has = |g: ForestGround| path.iter().any(|&c| ground[c] == g);
    // Always roll, so the stream does not depend on the ground.
    let bridge = rng.random::<f32>() * 100.0 < bridge_pct;
    if has(ForestGround::Deep) {
        if bridge { CrossingKind::Bridge } else { CrossingKind::Ford }
    } else if has(ForestGround::Rock) {
        CrossingKind::Pass
    } else if has(ForestGround::Thicket) {
        CrossingKind::Trail
    } else {
        CrossingKind::Open
    }
}

/// Cuts the barrier cells along `path` (and within `r` cells of it, inside
/// `allowed`, never locked ones): forest and rock become trail, deep water
/// a bridge deck or a ford.
#[allow(clippy::too_many_arguments)]
fn carve(ground: &mut [ForestGround], locked: &[bool], w: usize, h: usize, path: &[usize], r: f32, allowed: impl Fn(usize) -> bool, kind: CrossingKind) {
    let ri = r.ceil() as i64;
    let mut cut = Vec::new();
    for &p in path {
        let (x, y) = ((p % w) as i64, (p / w) as i64);
        for dy in -ri..=ri {
            for dx in -ri..=ri {
                let (nx, ny) = (x + dx, y + dy);
                if ((dx * dx + dy * dy) as f32) > r * r + 0.25 || nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                    continue;
                }
                let q = ny as usize * w + nx as usize;
                if (q == p || allowed(q) && !locked[q]) && !ground[q].walkable() {
                    cut.push(q);
                }
            }
        }
    }
    for q in cut {
        ground[q] = match (ground[q], kind) {
            (ForestGround::Deep, CrossingKind::Bridge) => ForestGround::Bridge,
            (ForestGround::Deep, _) => ForestGround::Shallow,
            _ => ForestGround::Trail,
        };
    }
}

/// Cheapest 8-neighbor way from any of `sources` to a cell where `target`
/// holds; `cost(c)` is the price of stepping onto `c` (None = blocked).
/// Returns the cells source → target.
pub(crate) fn cheapest(w: usize, h: usize, sources: &[usize], target: impl Fn(usize) -> bool, cost: impl Fn(usize) -> Option<u32>) -> Option<Vec<usize>> {
    let n = w * h;
    let mut best = vec![u32::MAX; n];
    let mut prev = vec![usize::MAX; n];
    let mut heap = BinaryHeap::new();
    for &c in sources {
        best[c] = 0;
        heap.push(Reverse((0u32, c)));
    }
    while let Some(Reverse((d, c))) = heap.pop() {
        if d > best[c] {
            continue;
        }
        if target(c) && !sources.contains(&c) {
            let mut path = vec![c];
            let mut p = prev[c];
            while p != usize::MAX {
                path.push(p);
                p = prev[p];
            }
            path.reverse();
            return Some(path);
        }
        let (x, y) = (c % w, c / w);
        for nb in neighbors8(c, w, h) {
            let Some(step) = cost(nb) else { continue };
            let diag = nb % w != x && nb / w != y;
            // A diagonal step must not squeeze between two blocked cells.
            if diag && (cost(y * w + nb % w).is_none() || cost((nb / w) * w + x).is_none()) {
                continue;
            }
            let step = if diag { step * 14 / 10 } else { step };
            let nd = d.saturating_add(step);
            if nd < best[nb] {
                best[nb] = nd;
                prev[nb] = c;
                heap.push(Reverse((nd, nb)));
            }
        }
    }
    None
}

/// Most links from any of `sources` to an area over the linked borders.
fn farthest_hops(k: usize, links: &[AreaLink], sources: &[u16]) -> u32 {
    let mut hops = vec![u32::MAX; k + 1];
    let mut queue = VecDeque::new();
    for &s in sources {
        if (s as usize) <= k && hops[s as usize] == u32::MAX {
            hops[s as usize] = 0;
            queue.push_back(s);
        }
    }
    while let Some(a) = queue.pop_front() {
        for l in links.iter().filter(|l| l.linked) {
            let other = if l.a == a { l.b } else if l.b == a { l.a } else { continue };
            if hops[other as usize] == u32::MAX {
                hops[other as usize] = hops[a as usize] + 1;
                queue.push_back(other);
            }
        }
    }
    hops[1..].iter().filter(|&&x| x != u32::MAX).copied().max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::generator::forest::{areas, terrain, thickets, water};
    use crate::model::{GateSpec, MapKind};

    fn config() -> LevelConfig {
        let mut c = LevelConfig { kind: MapKind::Forest, ..LevelConfig::default() };
        c.map.width_m = 200.0;
        c.map.height_m = 160.0;
        c.cell_size_m = 1.0;
        c
    }

    fn run(c: &LevelConfig, seed: u64) -> (AreaLayout, PassageBuild) {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let a = areas::build(c, &mut rng).layout;
        let t = terrain::build(c, &mut rng, &a);
        let wa = water::build(c, &mut rng, &t).layout;
        let wo = thickets::build(c, &mut rng, &a, &t, &wa);
        let p = build(c, &mut rng, &a, &wo);
        (a, p)
    }

    /// Can you walk from area `a` into area `b` staying inside the two?
    fn walk_between(a: &AreaLayout, l: &PassageLayout, x: u16, y: u16) -> bool {
        let inside = CellMask {
            width: l.width,
            height: l.height,
            cell_size_m: 1.0,
            cells: (0..l.ground.len()).map(|c| (a.ids[c] == x || a.ids[c] == y) && l.ground_at(c).walkable()).collect(),
        };
        let (labels, _) = grid::components(&inside);
        (0..labels.len()).any(|c| a.ids[c] == x && labels[c] > 0 && (0..labels.len()).any(|d| a.ids[d] == y && labels[d] == labels[c]))
    }

    #[test]
    fn one_piece_every_area_reached_and_unlinked_stay_closed() {
        for seed in 0..6 {
            let mut c = config();
            c.forest.passages.labyrinth_pct = [0.0, 35.0, 100.0][seed as usize % 3];
            let (a, b) = run(&c, seed);
            let l = &b.layout;
            assert!(grid::is_connected(&l.walkable_mask()), "seed {seed}: {:?}", b.notes);
            for area in &a.areas {
                assert!((0..a.ids.len()).any(|i| a.ids[i] == area.id && l.ground_at(i).walkable()), "seed {seed}: area {} closed", area.id);
            }
            for link in &l.links {
                assert_eq!(walk_between(&a, l, link.a, link.b), link.linked, "seed {seed}: {link:?}");
            }
            assert_eq!(l.gates.len(), 2);
        }
    }

    #[test]
    fn labyrinth_zero_is_a_tree() {
        for seed in 0..3 {
            let mut c = config();
            c.forest.passages.labyrinth_pct = 0.0;
            let (a, b) = run(&c, seed);
            assert_eq!(b.layout.links.iter().filter(|l| l.linked).count(), a.areas.len() - 1);
        }
    }

    #[test]
    fn gates_any_count() {
        for gates in [vec![], vec![GateSpec::at(0.0), GateSpec::at(120.0), GateSpec::at(240.0)]] {
            let mut c = config();
            c.gates.gates = gates.clone();
            let (_, b) = run(&c, 4);
            assert_eq!(b.layout.gates.len(), gates.len());
            assert!(grid::is_connected(&b.layout.walkable_mask()));
        }
    }

    #[test]
    fn same_seed_same_forest() {
        let c = config();
        assert_eq!(run(&c, 8).1.layout, run(&c, 8).1.layout);
    }
}
