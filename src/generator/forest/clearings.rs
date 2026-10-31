//! Forest node 7, clearings & trails (step F6 in docs/forest-plan.md): what
//! makes an area a place — glades to fight in, and trails that lead the
//! player from the gate through the crossings to the glades.
//!
//! 1. **Glades** — per area `glades_min..=glades_max` (camp grounds: one
//!    big glade; meadows: none, they are open already), as far from each
//!    other and from the area border as the area allows (best of
//!    `TRIES` spots). Wild: an organic blob (radius pushed in and out by
//!    noise). Managed: a cut glade — a rotated rounded rectangle. A glade
//!    turns forest floor and inner thickets into open ground; water and
//!    rock stay.
//! 2. **Trail ends** per area: its glades, the points where its links cross
//!    into the neighbors, its gates.
//! 3. **Trail tree** per area: nearest-first spanning tree over the ends,
//!    plus loops (`loops_pct`) between near ends.
//! 4. **Trails** — the cheapest way inside the area (open ground cheap,
//!    forest floor a bit dearer, shallow water dear, inner thickets dearest, rock,
//!    deep water and sealed cells never) with a noise wiggle (`wiggle_pct`;
//!    cart tracks a quarter of it), smoothed, drawn `footpath_m` wide in
//!    wild areas and `track_m` in managed ones. Shallow water stays (a
//!    ford).
//!
//! Everything stays inside one area and off the sealed cells, so the area
//! graph of the passages node is unchanged: the walkable ground only grows.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::forest::distance_field;
use crate::generator::forest::passages::cheapest;
use crate::generator::noise::NoiseField;
use crate::level::{
    AreaLayout, ClearingLayout, ForestCharacter, ForestGlade, ForestGround, ForestStyle, ForestTrail, PassageLayout, TrailKind,
};
use crate::model::LevelConfig;

/// Spots tried per glade.
const TRIES: usize = 32;
/// Least gap between a glade and the area border, meters.
const BORDER_GAP_M: f32 = 5.0;
/// Camp grounds' one glade, × the max glade size.
const CAMP_SCALE: f32 = 1.6;
/// Step costs.
const COST_OPEN: u32 = 10;
const COST_TRAIL: u32 = 7;
const COST_FLOOR: u32 = 14;
const COST_SHALLOW: u32 = 45;
const COST_DENSE: u32 = 120;
/// Wiggle: extra cost at 100%, and its feature size, meters.
const WIGGLE_COST: f32 = 90.0;
const WIGGLE_SCALE_M: f32 = 16.0;
/// Trail ends closer than this, meters, need no trail of their own.
const SAME_END_M: f32 = 4.0;

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, areas: &AreaLayout, passages: &PassageLayout) -> ClearingLayout {
    let s = &config.forest.clearings;
    let (w, h, cell) = (passages.width, passages.height, passages.cell_size_m);
    let n = w * h;
    let ids = &areas.ids;
    let sealed = &passages.sealed.cells;
    let mut ground: Vec<ForestGround> = passages.ground.iter().map(|&g| ForestGround::from_code(g)).collect();
    let shape_noise = NoiseField::new(rng.random::<u64>());
    let wiggle_noise = NoiseField::new(rng.random::<u64>());
    let xy_m = |c: usize| [((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell];
    // Distance to the nearest other area or the map edge.
    let (to_border, _) = distance_field(
        w,
        h,
        cell,
        |c| {
            let (x, y) = (c % w, c / w);
            x == 0 || y == 0 || x + 1 == w || y + 1 == h || crate::generator::forest::neighbors4(c, w, h).any(|nb| ids[nb] != ids[c])
        },
        (s.glade_max_m.max(s.glade_min_m) * CAMP_SCALE + BORDER_GAP_M * 2.0).max(10.0),
    );

    // 1. Glades.
    let (gmin, gmax) = (s.glade_min_m.min(s.glade_max_m).max(cell * 2.0), s.glade_max_m.max(s.glade_min_m).max(cell * 2.0));
    let mut glades: Vec<ForestGlade> = Vec::new();
    for area in &areas.areas {
        let (count, scale) = match area.character {
            ForestCharacter::Meadow => (0, 1.0),
            ForestCharacter::CampGrounds => (1, CAMP_SCALE),
            _ => {
                let (lo, hi) = (s.glades_min.min(s.glades_max), s.glades_max.max(s.glades_min));
                (rng.random_range(lo..=hi), 1.0)
            }
        };
        let cells: Vec<usize> = (0..n).filter(|&c| ids[c] == area.id && ground[c].walkable() && ground[c] != ForestGround::Shallow).collect();
        let cut = area.style == ForestStyle::Managed;
        for _ in 0..count {
            let size = rng.random_range(gmin..=gmax) * scale;
            let r = size / 2.0;
            let angle: f32 = rng.random_range(0.0..std::f32::consts::PI);
            let aspect: f32 = rng.random_range(0.6..=0.9);
            // Best spot: room to the border, far from the other glades.
            let mut best: Option<(usize, f32)> = None;
            for _ in 0..TRIES {
                let Some(&c) = cells.get(rng.random_range(0..cells.len().max(1))) else { break };
                let room = to_border[c] - r - BORDER_GAP_M;
                if room < 0.0 {
                    continue;
                }
                let p = xy_m(c);
                let apart = glades
                    .iter()
                    .filter(|g| g.area == area.id)
                    .map(|g| (g.center_m[0] - p[0]).hypot(g.center_m[1] - p[1]))
                    .fold(f32::INFINITY, f32::min)
                    .min(1000.0);
                let score = apart + room * 0.5;
                if best.is_none_or(|b| score > b.1) {
                    best = Some((c, score));
                }
            }
            let Some((c0, _)) = best else { continue };
            let ctr = xy_m(c0);
            let reach = (r * 1.5 / cell).ceil() as i64;
            let (x0, y0) = ((c0 % w) as i64, (c0 / w) as i64);
            let mut opened = 0usize;
            for dy in -reach..=reach {
                for dx in -reach..=reach {
                    let (x, y) = (x0 + dx, y0 + dy);
                    if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                        continue;
                    }
                    let c = y as usize * w + x as usize;
                    if ids[c] != area.id || sealed[c] || matches!(ground[c], ForestGround::Deep | ForestGround::Rock | ForestGround::Shallow | ForestGround::Bridge) {
                        continue;
                    }
                    let p = xy_m(c);
                    let (px, py) = (p[0] - ctr[0], p[1] - ctr[1]);
                    let inside = if cut {
                        // Rotated rounded rectangle (signed distance ≤ 0).
                        let (u, v) = (px * angle.cos() + py * angle.sin(), -px * angle.sin() + py * angle.cos());
                        let round = r * 0.35;
                        let (hx, hy) = (r - round, r * aspect - round);
                        let (qx, qy) = (u.abs() - hx, v.abs() - hy);
                        qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - round <= 0.0
                    } else {
                        let d = px.hypot(py) / r;
                        let wob = (shape_noise.fbm(p[0] / 6.0, p[1] / 6.0, 3) - 0.5) * 2.0 * 0.45;
                        d <= 1.0 + wob
                    };
                    if inside && to_border[c] >= BORDER_GAP_M * 0.5 {
                        if ground[c] != ForestGround::Open {
                            ground[c] = ForestGround::Open;
                        }
                        opened += 1;
                    }
                }
            }
            glades.push(ForestGlade { id: glades.len() as u32 + 1, area: area.id, center_m: ctr, area_m2: opened as f32 * cell * cell, cut });
        }
    }

    // 2–4. Trails per area.
    let wiggle = (s.wiggle_pct / 100.0).clamp(0.0, 1.0);
    let mut trails: Vec<ForestTrail> = Vec::new();
    for area in &areas.areas {
        let managed = area.style == ForestStyle::Managed;
        let (kind, width) = if managed { (TrailKind::CartTrack, s.track_m) } else { (TrailKind::Footpath, s.footpath_m) };
        let width = width.max(cell);
        // Trail ends: glade centers, link crossings, gates (as cells of this
        // area that are walkable).
        let mut ends: Vec<[f32; 2]> = glades.iter().filter(|g| g.area == area.id).map(|g| g.center_m).collect();
        for l in passages.links.iter().filter(|l| l.linked && (l.a == area.id || l.b == area.id)) {
            if let Some(p) = l.point_m {
                ends.push(p);
            }
        }
        for g in passages.gates.iter().filter(|g| g.area == area.id) {
            ends.push(g.point_m);
        }
        // Snap each end to the nearest walkable cell of the area; merge near ones.
        let own: Vec<usize> = (0..n).filter(|&c| ids[c] == area.id && ground[c].walkable() && !sealed[c]).collect();
        let mut nodes: Vec<usize> = Vec::new();
        for e in ends {
            let Some(&c) = own.iter().min_by(|&&p, &&q| {
                let d = |c: usize| (xy_m(c)[0] - e[0]).hypot(xy_m(c)[1] - e[1]);
                d(p).total_cmp(&d(q)).then(p.cmp(&q))
            }) else {
                continue;
            };
            let near = nodes.iter().any(|&q| (xy_m(q)[0] - xy_m(c)[0]).hypot(xy_m(q)[1] - xy_m(c)[1]) < SAME_END_M);
            if !near {
                nodes.push(c);
            }
        }
        if nodes.len() < 2 {
            continue;
        }
        let d = |a: usize, b: usize| (xy_m(nodes[a])[0] - xy_m(nodes[b])[0]).hypot(xy_m(nodes[a])[1] - xy_m(nodes[b])[1]);
        // Spanning tree, nearest first (Prim from the first end).
        let mut pairs: Vec<(usize, usize, bool)> = Vec::new();
        let mut in_tree = vec![false; nodes.len()];
        in_tree[0] = true;
        for _ in 1..nodes.len() {
            let best = (0..nodes.len())
                .filter(|&a| in_tree[a])
                .flat_map(|a| (0..nodes.len()).filter(|&b| !in_tree[b]).map(move |b| (a, b)))
                .min_by(|x, y| d(x.0, x.1).total_cmp(&d(y.0, y.1)).then(x.cmp(y)));
            let Some((a, b)) = best else { break };
            in_tree[b] = true;
            pairs.push((a, b, false));
        }
        // Loops: each end to its nearest end it has no trail with.
        for a in 0..nodes.len() {
            let roll = rng.random::<f32>() * 100.0;
            let near = (0..nodes.len())
                .filter(|&b| b != a && !pairs.iter().any(|&(x, y, _)| (x, y) == (a, b) || (x, y) == (b, a)))
                .min_by(|&x, &y| d(a, x).total_cmp(&d(a, y)).then(x.cmp(&y)));
            if let Some(b) = near
                && roll < s.loops_pct
                && nodes.len() >= 3
            {
                pairs.push((a, b, true));
            }
        }
        let amp = if managed { wiggle * 0.35 } else { wiggle };
        for (a, b, extra) in pairs {
            let target = nodes[b];
            let path = cheapest(w, h, &[nodes[a]], |c| c == target, |c| {
                if ids[c] != area.id || sealed[c] {
                    return None;
                }
                let base = match ground[c] {
                    ForestGround::Open => COST_OPEN,
                    ForestGround::Trail | ForestGround::Bridge => COST_TRAIL,
                    ForestGround::Floor => COST_FLOOR,
                    ForestGround::Shallow => COST_SHALLOW,
                    ForestGround::Thicket => COST_DENSE,
                    ForestGround::Rock | ForestGround::Deep => return None,
                };
                let p = xy_m(c);
                Some(base + (amp * WIGGLE_COST * wiggle_noise.fbm(p[0] / WIGGLE_SCALE_M, p[1] / WIGGLE_SCALE_M, 3).powi(2)) as u32)
            });
            let Some(path) = path else { continue };
            // Every 3rd cell, smoothed, then drawn.
            let mut pts: Vec<[f32; 2]> = path.iter().enumerate().filter(|(i, _)| i % 4 == 0 || *i == path.len() - 1).map(|(_, &c)| xy_m(c)).collect();
            for _ in 0..3 {
                pts = chaikin(&pts);
            }
            for seg in pts.windows(2) {
                stamp(&mut ground, ids, sealed, w, h, cell, seg[0], seg[1], width / 2.0, area.id);
            }
            let length_m = pts.windows(2).map(|p| (p[0][0] - p[1][0]).hypot(p[0][1] - p[1][1])).sum();
            trails.push(ForestTrail { id: trails.len() as u32 + 1, area: area.id, kind, width_m: width, length_m, extra, path_m: pts });
        }
    }
    // A glade can open a patch beyond water or rock: whatever is not joined
    // to the walkable ground of the passages goes back to what it was.
    let mask = crate::level::CellMask { width: w, height: h, cell_size_m: cell, cells: ground.iter().map(|g| g.walkable()).collect() };
    let (labels, pieces) = crate::generator::grid::components(&mask);
    let mut keep = vec![false; pieces as usize + 1];
    for c in 0..n {
        if ForestGround::from_code(passages.ground[c]).walkable() {
            keep[labels[c] as usize] = true;
        }
    }
    for c in 0..n {
        if labels[c] > 0 && !keep[labels[c] as usize] {
            ground[c] = ForestGround::from_code(passages.ground[c]);
        }
    }
    let walkable = ground.iter().filter(|g| g.walkable()).count();
    ClearingLayout {
        ground: ground.iter().map(|g| g.code()).collect(),
        glades,
        trails,
        walkable_pct: walkable as f32 / n.max(1) as f32 * 100.0,
    }
}

fn chaikin(pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut out = vec![pts[0]];
    for p in pts.windows(2) {
        let (a, b) = (p[0], p[1]);
        out.push([a[0] * 0.75 + b[0] * 0.25, a[1] * 0.75 + b[1] * 0.25]);
        out.push([a[0] * 0.25 + b[0] * 0.75, a[1] * 0.25 + b[1] * 0.75]);
    }
    out.push(pts[pts.len() - 1]);
    out
}

/// Turns the cells within `r` of segment a–b into trail (inside `area`,
/// off sealed cells; water and rock stay, shallow water is the ford).
#[allow(clippy::too_many_arguments)]
fn stamp(ground: &mut [ForestGround], ids: &[u16], sealed: &[bool], w: usize, h: usize, cell: f32, a: [f32; 2], b: [f32; 2], r: f32, area: u16) {
    let (x0, x1) = (((a[0].min(b[0]) - r) / cell).floor().max(0.0) as usize, (((a[0].max(b[0]) + r) / cell).ceil() as usize).min(w));
    let (y0, y1) = (((a[1].min(b[1]) - r) / cell).floor().max(0.0) as usize, (((a[1].max(b[1]) + r) / cell).ceil() as usize).min(h));
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = (dx * dx + dy * dy).max(1e-9);
    for y in y0..y1 {
        for x in x0..x1 {
            let c = y * w + x;
            if ids[c] != area || sealed[c] || matches!(ground[c], ForestGround::Deep | ForestGround::Rock | ForestGround::Shallow | ForestGround::Bridge) {
                continue;
            }
            let p = ((x as f32 + 0.5) * cell, (y as f32 + 0.5) * cell);
            let t = (((p.0 - a[0]) * dx + (p.1 - a[1]) * dy) / len2).clamp(0.0, 1.0);
            if (p.0 - (a[0] + dx * t)).hypot(p.1 - (a[1] + dy * t)) <= r {
                ground[c] = ForestGround::Trail;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::generator::forest::{areas, passages, terrain, thickets, water};
    use crate::generator::grid;
    use crate::level::CellMask;
    use crate::model::MapKind;

    fn run(seed: u64) -> (AreaLayout, PassageLayout, ClearingLayout) {
        let mut c = LevelConfig { kind: MapKind::Forest, ..LevelConfig::default() };
        c.map.width_m = 200.0;
        c.map.height_m = 160.0;
        c.cell_size_m = 1.0;
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let a = areas::build(&c, &mut rng).layout;
        let t = terrain::build(&c, &mut rng, &a);
        let wa = water::build(&c, &mut rng, &t).layout;
        let wo = thickets::build(&c, &mut rng, &a, &t, &wa);
        let p = passages::build(&c, &mut rng, &a, &wo).layout;
        let cl = build(&c, &mut rng, &a, &p);
        (a, p, cl)
    }

    fn walk_between(a: &AreaLayout, ground: &[u8], w: usize, h: usize, x: u16, y: u16) -> bool {
        let inside = CellMask {
            width: w,
            height: h,
            cell_size_m: 1.0,
            cells: (0..ground.len()).map(|c| (a.ids[c] == x || a.ids[c] == y) && ForestGround::from_code(ground[c]).walkable()).collect(),
        };
        let (labels, _) = grid::components(&inside);
        (0..labels.len()).any(|c| a.ids[c] == x && labels[c] > 0 && (0..labels.len()).any(|d| a.ids[d] == y && labels[d] == labels[c]))
    }

    #[test]
    fn walkable_only_grows_and_the_graph_holds() {
        for seed in 0..4 {
            let (a, p, cl) = run(seed);
            for c in 0..p.ground.len() {
                if ForestGround::from_code(p.ground[c]).walkable() {
                    assert!(ForestGround::from_code(cl.ground[c]).walkable(), "seed {seed}: cell {c} closed");
                }
            }
            let mask = CellMask { width: p.width, height: p.height, cell_size_m: 1.0, cells: cl.ground.iter().map(|&g| ForestGround::from_code(g).walkable()).collect() };
            assert!(grid::is_connected(&mask), "seed {seed}");
            for l in &p.links {
                assert_eq!(walk_between(&a, &cl.ground, p.width, p.height, l.a, l.b), l.linked, "seed {seed}: {l:?}");
            }
            assert!(!cl.glades.is_empty() && !cl.trails.is_empty(), "seed {seed}");
        }
    }

    #[test]
    fn glades_are_open_and_trails_drawn() {
        let (_, p, cl) = run(5);
        for g in &cl.glades {
            let c = ((g.center_m[1] / p.cell_size_m) as usize) * p.width + (g.center_m[0] / p.cell_size_m) as usize;
            assert!(ForestGround::from_code(cl.ground[c]).walkable(), "{g:?}");
            assert!(g.area_m2 > 40.0, "{g:?}");
        }
        assert!(cl.ground.contains(&ForestGround::Trail.code()));
        assert!(cl.trails.iter().any(|t| t.kind == TrailKind::Footpath));
    }

    #[test]
    fn same_seed_same_clearings() {
        assert_eq!(run(6).2, run(6).2);
    }
}
