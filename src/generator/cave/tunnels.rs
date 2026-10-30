//! Cave node 4, tunnels (step C3 in docs/cave-plan.md): links the chambers
//! into one network, from a single route to a maze, and digs the gate
//! tunnels in from the map edge.
//!
//! 1. **Junctions** — tunnel crossings with no chamber, in open rock
//!    (best of 32 spots farthest from the chambers). How many: `junctions`,
//!    or from the labyrinth slider (none below 50%).
//! 2. **Tree** — grown from the chamber nearest the first gate. Each step
//!    rolls `branching_pct` (of the last node's style): miss → the nearest
//!    free node hangs on the **last** node (a chain); hit → the nearest
//!    free node hangs on the nearest node of the network (a tree). Below
//!    50% mixing a node in the same style zone always wins, so the zones
//!    are crossed as rarely as possible.
//! 3. **Loops** — extra links between near neighbors, chance `loops_pct`
//!    (average of both ends' styles), no crossings, no sharp angles. Below
//!    50% mixing, at most `zone_links` tunnels cross a zone boundary.
//! 4. **Widths** — by traffic (the roads node's hierarchy) for natural
//!    tunnels; hand-made tunnels are `mine_width_m` wide.
//! 5. **Digging** — a link takes the style at its midpoint:
//!    - natural: A* over a noise cost (`wiggle_pct`), own chambers cheap,
//!      other chambers and the rock margin dear, tunnels already dug a
//!      little cheaper; round brush; `squeeze_pct` of them get a short
//!      stretch at `squeeze_width_m`;
//!    - hand-made: straight legs with up to `max_turns` turns (or one 45°
//!      leg, `diagonal_pct`); square brush, so the walls stay straight.
//!
//!    The carving node later shapes every cell by the zone it lies in, so a
//!    straight tunnel through a natural zone gets rough walls there.
//! 6. **Dead ends** — blind stubs (`stub_min_m..stub_max_m`) from random
//!    chambers into open rock.
//! 7. **Gates** — per gate a point at the map edge (ray from the center),
//!    a tunnel to the nearest chamber in the style of the zone there, and
//!    a marker.
//!
//! Chambers + tunnels are one connected floor afterwards.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::gates::{GATE_NAME, MARKER_M};
use crate::generator::grid;
use crate::generator::noise::NoiseField;
use crate::generator::roads::{cos_angle, link_traffic, segments_cross, widths_from_traffic};
use crate::generator::swamp::gate_anchors::edge_hit;
use crate::generator::swamp::paths::{astar, center, polyline_cells};
use crate::level::{
    CaveGate, CaveStyle, Category, CellMask, ChamberLayout, Junction, PlacedObject, RoadNode, StyleLayout, Tunnel,
    TunnelEnd, TunnelLayout,
};
use crate::model::{LabyrinthParams, LevelConfig};

pub struct TunnelBuild {
    pub layout: TunnelLayout,
    /// One marker per gate; `layout.gates[..].object` indexes here.
    pub objects: Vec<PlacedObject>,
    pub notes: Vec<String>,
}

/// Share of the links, %, at the full natural width.
const MAIN_PCT: f32 = 30.0;
/// Step cost in the chambers a tunnel links (rock = 1 before the wiggle).
const OWN_CHAMBER_COST: f32 = 0.3;
/// Step cost multiplier in other chambers.
const OTHER_CHAMBER_COST: f32 = 3.0;
/// Step cost multiplier inside the rock margin.
const MARGIN_COST: f32 = 4.0;
/// Step cost multiplier on tunnels already dug.
const ON_TUNNEL_COST: f32 = 0.6;
const WIGGLE_SCALE_M: f32 = 14.0;
const WIGGLE_CONTRAST: f32 = 7.0;
/// Squeeze length, meters.
const SQUEEZE_M: f32 = 4.0;
/// Natural width wander feature size, meters.
const NATURAL_VARY_SCALE_M: f32 = 8.0;
/// fBm rarely leaves 0.5 ± 0.25; this stretches it so `width_variation_pct`
/// is about the real ± swing.
const VARY_GAIN: f32 = 2.0;
/// Funnel into a squeeze: width grows by this per meter away from it.
const SQUEEZE_FUNNEL: f32 = 0.6;
/// Hand-dug sway of the centerline at 100% roughness, meters, and its
/// feature size; plus a finer jitter.
const MINE_SWAY_M: f32 = 2.2;
const MINE_SWAY_SCALE_M: f32 = 9.0;
const MINE_JITTER_M: f32 = 0.6;
const MINE_JITTER_SCALE_M: f32 = 2.5;
/// The sway fades out over this distance at both ends, meters.
const MINE_TAPER_M: f32 = 3.0;
/// Hand-dug width change at 100% roughness, ± share.
const MINE_WIDTH_VARY: f32 = 0.6;
/// Neighbors per node considered for loops.
const NEAREST: usize = 4;
const MIN_LINK_ANGLE_DEG: f32 = 30.0;
/// Least distance between two junctions, meters.
const JUNCTION_GAP_M: f32 = 12.0;
const CANDIDATES: usize = 32;

#[derive(Clone, Copy)]
struct Node {
    cell: usize,
    end: TunnelEnd,
    zone: u16,
    style: CaveStyle,
    /// Chamber id, 0 for a junction.
    chamber: u16,
}

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, style: &StyleLayout, chambers: &ChamberLayout) -> TunnelBuild {
    let s = &config.cave.tunnels;
    let (w, h, cell) = (chambers.width, chambers.height, chambers.cell_size_m);
    let ch_ids = &chambers.ids;
    let mixing = (config.cave.style.mixing_pct / 100.0).clamp(0.0, 1.0);
    let strict_zones = mixing < 0.5;
    let mut notes = Vec::new();
    let noise = NoiseField::new(rng.random::<u64>());

    let lab_of = |st: CaveStyle| {
        let pct = if s.per_style && st == CaveStyle::HandMade { s.handmade_labyrinth_pct } else { s.labyrinth_pct };
        LabyrinthParams::from_pct(pct)
    };
    let branching_of = |st| if s.auto { lab_of(st).branching_pct } else { s.branching_pct };
    let loops_of = |st| if s.auto { lab_of(st).loops_pct } else { s.loops_pct };
    let xy = |c: usize| ((c % w) as f32, (c / w) as f32);

    // Chamber nodes: the chamber's cell nearest its centroid.
    let mut nodes: Vec<Node> = chambers
        .chambers
        .iter()
        .map(|ch| {
            let (cx, cy) = (ch.center_m[0] / cell - 0.5, ch.center_m[1] / cell - 0.5);
            let c = (0..w * h)
                .filter(|&c| ch_ids[c] == ch.id)
                .min_by(|&p, &q| {
                    let d = |c: usize| (xy(c).0 - cx).powi(2) + (xy(c).1 - cy).powi(2);
                    d(p).total_cmp(&d(q)).then(p.cmp(&q))
                })
                .unwrap_or(0);
            Node { cell: c, end: TunnelEnd::Chamber(ch.id), zone: ch.zone, style: ch.style, chamber: ch.id }
        })
        .collect();
    let n_chambers = nodes.len();

    // 1. Junctions.
    let margin = (config.cave.chambers.rock_margin_m.max(0.0) / cell).round() as usize;
    let gap_cells = config.cave.chambers.gap_m.max(0.0) / cell;
    let want_junctions = if s.auto {
        nodes.iter().map(|n| lab_of(n.style).junction_share).sum::<f32>().round() as u32
    } else {
        s.junctions
    };
    let rock_dist = distance_field(w, h, |c| ch_ids[c] > 0);
    let mut junctions = Vec::new();
    for _ in 0..want_junctions {
        let lo = margin as f32 + gap_cells;
        let best = (0..CANDIDATES)
            .map(|_| {
                let (x, y) = if (w as f32) > 2.0 * lo + 1.0 && (h as f32) > 2.0 * lo + 1.0 {
                    (rng.random_range(lo..w as f32 - lo) as usize, rng.random_range(lo..h as f32 - lo) as usize)
                } else {
                    (w / 2, h / 2)
                };
                let c = y * w + x;
                let to_junction = junctions
                    .iter()
                    .map(|j: &Junction| (j.point_m[0] / cell - x as f32).hypot(j.point_m[1] / cell - y as f32))
                    .fold(f32::INFINITY, f32::min);
                let clear = rock_dist[c].min(to_junction * gap_cells.max(1.0) / (JUNCTION_GAP_M / cell));
                (c, clear, to_junction)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));
        match best {
            Some((c, _, to_j)) if rock_dist[c] >= gap_cells && to_j >= JUNCTION_GAP_M / cell => {
                let id = junctions.len() as u16 + 1;
                junctions.push(Junction { id, point_m: center(c, w, cell) });
                nodes.push(Node { cell: c, end: TunnelEnd::Junction(id), zone: style.ids[c], style: style.style_at(c), chamber: 0 });
            }
            _ => {}
        }
    }
    if (junctions.len() as u32) < want_junctions {
        notes.push(format!("Tunnels: {} of {want_junctions} junctions found open rock", junctions.len()));
    }

    // Gate points (needed for the tree's start).
    let inset = (cell / 2.0).max(MARKER_M / 2.0);
    let (wm, hm) = (w as f32 * cell, h as f32 * cell);
    let gate_points: Vec<((f32, f32), (f32, f32))> = config
        .gates
        .gates
        .iter()
        .map(|g| {
            let (p, nrm) = edge_hit((wm / 2.0, hm / 2.0), g.direction_deg.rem_euclid(360.0), wm, hm);
            ((p.0.clamp(inset, wm - inset), p.1.clamp(inset, hm - inset)), nrm)
        })
        .collect();

    // 2. Tree.
    let n = nodes.len();
    let dist = |a: usize, b: usize| {
        let (p, q) = (xy(nodes[a].cell), xy(nodes[b].cell));
        (p.0 - q.0).hypot(p.1 - q.1)
    };
    // Below 50% mixing: (crosses a zone, distance) — lexicographic. Above:
    // a crossing only costs a little extra distance.
    let cost = |a: usize, b: usize| {
        let cross = nodes[a].zone != nodes[b].zone;
        if strict_zones {
            (cross as u8, dist(a, b))
        } else {
            (0, dist(a, b) * if cross { 1.0 + 4.0 * (1.0 - mixing) } else { 1.0 })
        }
    };
    let less = |x: (u8, f32), y: (u8, f32)| x.0 < y.0 || (x.0 == y.0 && x.1 < y.1);
    let mut links: Vec<(usize, usize, bool)> = Vec::new();
    if n > 0 {
        let start = gate_points
            .first()
            .and_then(|&(p, _)| {
                (0..n_chambers).min_by(|&a, &b| {
                    let d = |k: usize| {
                        let q = center(nodes[k].cell, w, cell);
                        (q[0] - p.0).hypot(q[1] - p.1)
                    };
                    d(a).total_cmp(&d(b)).then(a.cmp(&b))
                })
            })
            .unwrap_or(0);
        let mut in_tree = vec![false; n];
        in_tree[start] = true;
        let mut tail = start;
        for _ in 1..n {
            let branch = rng.random::<f32>() * 100.0 < branching_of(nodes[tail].style);
            let mut best: Option<(usize, usize, (u8, f32))> = None;
            let froms: Vec<usize> = if branch { (0..n).filter(|&k| in_tree[k]).collect() } else { vec![tail] };
            for &t in &froms {
                for u in (0..n).filter(|&k| !in_tree[k]) {
                    let c = cost(t, u);
                    if best.is_none_or(|b| less(c, b.2)) {
                        best = Some((t, u, c));
                    }
                }
            }
            let Some((t, u, _)) = best else { break };
            links.push((t, u, false));
            in_tree[u] = true;
            tail = u;
        }
    }
    let tree_crossings = links.iter().filter(|&&(a, b, _)| nodes[a].zone != nodes[b].zone).count() as u32;

    // 3. Loops.
    let pos = |k: usize| xy(nodes[k].cell);
    let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
    for a in 0..n {
        let mut others: Vec<(f32, usize)> = (0..n).filter(|&b| b != a).map(|b| (dist(a, b), b)).collect();
        others.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        for &(d, b) in others.iter().take(NEAREST) {
            let (x, y) = (a.min(b), a.max(b));
            if !pairs.iter().any(|p| (p.1, p.2) == (x, y)) {
                pairs.push((d, x, y));
            }
        }
    }
    pairs.sort_by(|x, y| x.0.total_cmp(&y.0).then((x.1, x.2).cmp(&(y.1, y.2))));
    let min_cos = MIN_LINK_ANGLE_DEG.to_radians().cos();
    let mut crossings = tree_crossings;
    let cross_cap = config.cave.style.zone_links.max(tree_crossings);
    for &(_, a, b) in &pairs {
        // Always roll, so the stream does not depend on earlier outcomes.
        let roll = rng.random::<f32>() * 100.0;
        if links.iter().any(|&(x, y, _)| (x.min(y), x.max(y)) == (a, b)) {
            continue;
        }
        if roll >= (loops_of(nodes[a].style) + loops_of(nodes[b].style)) / 2.0 {
            continue;
        }
        let cross = nodes[a].zone != nodes[b].zone;
        if cross && strict_zones && crossings >= cross_cap {
            continue;
        }
        let crosses = links.iter().any(|&(x, y, _)| {
            ![x, y].contains(&a) && ![x, y].contains(&b) && segments_cross(pos(a), pos(b), pos(x), pos(y))
        });
        let sharp = |at: usize, to: usize| {
            links.iter().any(|&(x, y, _)| {
                let other = if x == at { y } else if y == at { x } else { return false };
                cos_angle(pos(at), pos(to), pos(other)) > min_cos
            })
        };
        if !crosses && !sharp(a, b) && !sharp(b, a) {
            links.push((a, b, true));
            crossings += cross as u32;
        }
    }

    // 4. Widths by traffic.
    let road_nodes: Vec<RoadNode> = nodes.iter().map(|nd| RoadNode { x: nd.cell % w, y: nd.cell / w, region: 0 }).collect();
    let (wlo, whi) = (s.width_min_m.min(s.width_max_m).max(cell), s.width_max_m.max(s.width_min_m).max(cell));
    let widths = widths_from_traffic(&link_traffic(&road_nodes, &links), MAIN_PCT, wlo, whi);

    // 5. Digging.
    let wiggle = s.wiggle_pct.clamp(0.0, 100.0) / 100.0;
    let in_margin = |c: usize| {
        let (x, y) = (c % w, c / w);
        x < margin || y < margin || x + margin >= w || y + margin >= h
    };
    let base: Vec<f32> = (0..w * h)
        .map(|c| {
            let (x, y) = xy(c);
            let nz = noise.fbm(x * cell / WIGGLE_SCALE_M, y * cell / WIGGLE_SCALE_M, 2);
            (wiggle * (nz - 0.5) * WIGGLE_CONTRAST).exp()
        })
        .collect();
    let cheapest = OWN_CHAMBER_COST.min((-wiggle * WIGGLE_CONTRAST / 2.0).exp() * ON_TUNNEL_COST);
    // Everything but the outermost cells (gate tunnels may reach the edge).
    let inner: Vec<bool> = (0..w * h).map(|c| !(c % w == 0 || c / w == 0 || c % w == w - 1 || c / w == h - 1)).collect();
    let everywhere = vec![true; w * h];
    let mut dug = CellMask::new(w, h, cell);
    let mine_w = s.mine_width_m.max(cell);
    let mine_rough = (s.mine_roughness_pct / 100.0).clamp(0.0, 1.0);
    let vary = (s.width_variation_pct / 100.0).clamp(0.0, 1.0);
    let mut tunnels = Vec::new();
    let mut degree = vec![0u32; n];

    // One natural route from `from` to `to`: `own` chambers are cheap,
    // `margin_ok` lets it cross the rock margin at the plain price.
    let route_natural = |dug: &CellMask, from: usize, to: usize, own: [u16; 2], margin_ok: bool| {
        astar(
            w,
            h,
            from,
            to,
            |c| {
                let id = ch_ids[c];
                if id > 0 && own.contains(&id) {
                    return OWN_CHAMBER_COST;
                }
                let mut f = base[c];
                if id > 0 {
                    f *= OTHER_CHAMBER_COST;
                }
                if in_margin(c) && !margin_ok {
                    f *= MARGIN_COST;
                }
                if dug.cells[c] {
                    f *= ON_TUNNEL_COST;
                }
                f
            },
            cheapest,
        )
    };
    let route_len = |route: &[usize]| {
        route
            .windows(2)
            .map(|p| if p[0] % w != p[1] % w && p[0] / w != p[1] / w { std::f32::consts::SQRT_2 } else { 1.0 })
            .sum::<f32>()
            * cell
    };
    let sample = |route: &[usize]| {
        let every = ((1.0 / cell).round() as usize).max(1);
        let mut pts: Vec<[f32; 2]> = route.iter().step_by(every).map(|&c| center(c, w, cell)).collect();
        if let Some(&last) = route.last()
            && route.len() % every != 1
        {
            pts.push(center(last, w, cell));
        }
        pts
    };

    for (k, &(a, b, extra)) in links.iter().enumerate() {
        let (pa, pb) = (xy(nodes[a].cell), xy(nodes[b].cell));
        let mid = ((pb.1 + pa.1) / 2.0) as usize * w + ((pa.0 + pb.0) / 2.0) as usize;
        let st = style.style_at(mid.min(w * h - 1));
        let squeeze_roll = rng.random::<f32>() * 100.0;
        let squeeze_at: f32 = rng.random_range(0.3..0.7);
        let own = [nodes[a].chamber, nodes[b].chamber];
        let (width_m, squeeze_m, length_m, path_m) = match st {
            CaveStyle::Natural => {
                let route = route_natural(&dug, nodes[a].cell, nodes[b].cell, own, false);
                let squeeze = squeeze_roll < s.squeeze_pct.clamp(0.0, 100.0)
                    && s.squeeze_width_m < widths[k]
                    && route_len(&route) > SQUEEZE_M * 3.0;
                let squeeze_span = squeeze.then(|| {
                    let len = route.len();
                    let half = ((SQUEEZE_M / cell) as usize / 2).max(1);
                    let at = ((len as f32 * squeeze_at) as usize).clamp(half, len.saturating_sub(half + 1));
                    (at.saturating_sub(half), (at + half).min(len), s.squeeze_width_m.max(cell))
                });
                dig_natural(&mut dug, &inner, &route, widths[k], squeeze_span, vary, &noise, k as f32 * 31.7);
                (widths[k], squeeze.then_some(s.squeeze_width_m.max(cell)), route_len(&route), sample(&route))
            }
            CaveStyle::HandMade => {
                let (p, q) = (center(nodes[a].cell, w, cell), center(nodes[b].cell, w, cell));
                let corners = mine_corners(rng, p, q, None, s.max_turns, s.diagonal_pct);
                dig_mine(&mut dug, &inner, &corners, mine_w, mine_rough, &noise, k as f32 * 31.7);
                (mine_w, None, polyline_len(&corners), corners)
            }
        };
        degree[a] += 1;
        degree[b] += 1;
        tunnels.push(Tunnel {
            a: nodes[a].end,
            b: nodes[b].end,
            style: st,
            extra,
            cross_zone: nodes[a].zone != nodes[b].zone,
            width_m,
            squeeze_m,
            length_m,
            path_m,
        });
    }

    // 6. Dead-end stubs.
    let want_stubs = if s.auto {
        if n_chambers == 0 {
            0
        } else {
            (nodes[..n_chambers].iter().map(|nd| lab_of(nd.style).dead_ends as f32).sum::<f32>() / n_chambers as f32).round() as u32
        }
    } else {
        s.dead_ends
    };
    let mut stubs = 0;
    let (slo, shi) = (s.stub_min_m.min(s.stub_max_m).max(cell), s.stub_max_m.max(s.stub_min_m).max(cell));
    for _ in 0..want_stubs {
        if n_chambers == 0 {
            break;
        }
        let from = rng.random_range(0..n_chambers);
        let floor_dist = distance_field(w, h, |c| ch_ids[c] > 0 || dug.cells[c]);
        for _ in 0..12 {
            let angle: f32 = rng.random_range(0.0..std::f32::consts::TAU);
            let len = if shi - slo < 1e-6 { slo } else { rng.random_range(slo..=shi) };
            let ch = &chambers.chambers[from];
            // From the chamber's edge in that direction, `len` into the rock.
            let reach = ch.size_m / 2.0 * 1.3 + len;
            let t = [ch.center_m[0] + angle.cos() * reach, ch.center_m[1] + angle.sin() * reach];
            let (tx, ty) = ((t[0] / cell).floor(), (t[1] / cell).floor());
            if tx < margin as f32 || ty < margin as f32 || tx >= (w - margin) as f32 || ty >= (h - margin) as f32 {
                continue;
            }
            let target = ty as usize * w + tx as usize;
            if floor_dist[target] < gap_cells.max(2.0) {
                continue;
            }
            let nd = nodes[from];
            let (path_m, width_m, length_m) = match nd.style {
                CaveStyle::Natural => {
                    let route = route_natural(&dug, nd.cell, target, [nd.chamber, 0], false);
                    if route.iter().any(|&c| ch_ids[c] > 0 && ch_ids[c] != nd.chamber) {
                        continue;
                    }
                    dig_natural(&mut dug, &inner, &route, wlo, None, vary, &noise, 500.0 + stubs as f32 * 17.3);
                    (sample(&route), wlo, route_len(&route))
                }
                CaveStyle::HandMade => {
                    let corners = mine_corners(rng, center(nd.cell, w, cell), center(target, w, cell), None, s.max_turns.min(1), 0.0);
                    let line = polyline_cells(&corners, w, h, cell);
                    if line.iter().any(|&c| ch_ids[c] > 0 && ch_ids[c] != nd.chamber) {
                        continue;
                    }
                    dig_mine(&mut dug, &inner, &corners, mine_w, mine_rough, &noise, 500.0 + stubs as f32 * 17.3);
                    (corners.clone(), mine_w, polyline_len(&corners))
                }
            };
            tunnels.push(Tunnel {
                a: nd.end,
                b: TunnelEnd::Blind,
                style: nd.style,
                extra: false,
                cross_zone: false,
                width_m,
                squeeze_m: None,
                length_m,
                path_m,
            });
            stubs += 1;
            break;
        }
    }
    if stubs < want_stubs {
        notes.push(format!("Tunnels: {stubs} of {want_stubs} dead ends found open rock"));
    }

    // 7. Gates.
    let mut gates = Vec::new();
    let mut objects = Vec::new();
    for (i, (&(p, nrm), spec)) in gate_points.iter().zip(&config.gates.gates).enumerate() {
        let id = i as u32 + 1;
        let Some(to) = (0..n_chambers).min_by(|&a, &b| {
            let d = |k: usize| {
                let q = center(nodes[k].cell, w, cell);
                (q[0] - p.0).hypot(q[1] - p.1)
            };
            d(a).total_cmp(&d(b)).then(a.cmp(&b))
        }) else {
            notes.push(format!("Gate #{id}: no chamber to lead to"));
            continue;
        };
        let from = ((p.1 / cell) as usize).min(h - 1) * w + ((p.0 / cell) as usize).min(w - 1);
        let st = style.style_at(from);
        let nd = nodes[to];
        let (path_m, width_m, length_m) = match st {
            CaveStyle::Natural => {
                let route = route_natural(&dug, from, nd.cell, [nd.chamber, 0], true);
                dig_natural(&mut dug, &everywhere, &route, whi, None, vary, &noise, 900.0 + id as f32 * 13.1);
                (sample(&route), whi, route_len(&route))
            }
            CaveStyle::HandMade => {
                let corners = mine_corners(rng, [p.0, p.1], center(nd.cell, w, cell), Some(nrm), s.max_turns, 0.0);
                dig_mine(&mut dug, &everywhere, &corners, mine_w, mine_rough, &noise, 900.0 + id as f32 * 13.1);
                (corners.clone(), mine_w, polyline_len(&corners))
            }
        };
        degree[to] += 1;
        tunnels.push(Tunnel {
            a: TunnelEnd::Gate(id),
            b: nd.end,
            style: st,
            extra: false,
            cross_zone: style.ids[from] != nd.zone,
            width_m,
            squeeze_m: None,
            length_m,
            path_m,
        });
        objects.push(PlacedObject {
            category: Category::Gate,
            type_name: GATE_NAME.to_owned(),
            color: config.gates.color,
            x_m: p.0 - MARKER_M / 2.0,
            y_m: p.1 - MARKER_M / 2.0,
            w_m: MARKER_M,
            h_m: MARKER_M,
            angle_deg: 0.0,
            parts: Vec::new(),
            round: false,
        });
        gates.push(CaveGate { id, direction_deg: spec.direction_deg, point_m: [p.0, p.1], chamber: nd.chamber, object: objects.len() - 1 });
    }

    // Tunnel floor = dug cells off the chambers.
    for (c, d) in dug.cells.iter_mut().enumerate() {
        *d &= ch_ids[c] == 0;
    }
    let floor = CellMask { width: w, height: h, cell_size_m: cell, cells: (0..w * h).map(|c| ch_ids[c] > 0 || dug.cells[c]).collect() };
    let (_, pieces) = grid::components(&floor);
    if pieces > 1 {
        notes.push(format!("Tunnels: the floor is in {pieces} pieces"));
    }
    let sources: Vec<usize> = if gates.is_empty() {
        nodes.first().map(|nd| vec![nd.cell]).unwrap_or_default()
    } else {
        gates
            .iter()
            .map(|g| ((g.point_m[1] / cell) as usize).min(h - 1) * w + ((g.point_m[0] / cell) as usize).min(w - 1))
            .collect()
    };
    let main_route_m = walk_farthest(&floor, &sources) * cell;
    let dead_end_chambers = degree[..n_chambers].iter().filter(|&&d| d == 1).count() as u32;

    TunnelBuild {
        layout: TunnelLayout { mask: dug, tunnels, junctions, gates, main_route_m, dead_end_chambers },
        objects,
        notes,
    }
}

/// Corners of a hand-made tunnel from `p` to `q` (meters): straight legs
/// with 1..=`max_turns` turns along the map axes, or (`diagonal_pct`) one
/// 45° leg and one straight leg. `first` = the first leg's direction (a
/// gate tunnel leaves the edge straight in).
fn mine_corners(rng: &mut ChaCha8Rng, p: [f32; 2], q: [f32; 2], first: Option<(f32, f32)>, max_turns: u32, diagonal_pct: f32) -> Vec<[f32; 2]> {
    let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
    if max_turns == 0 || dx.abs() < 1e-3 || dy.abs() < 1e-3 {
        return vec![p, q];
    }
    if first.is_none() && rng.random::<f32>() * 100.0 < diagonal_pct {
        let d = dx.abs().min(dy.abs());
        let diag = [dx.signum() * d, dy.signum() * d];
        let corner = if rng.random_bool(0.5) { [p[0] + diag[0], p[1] + diag[1]] } else { [q[0] - diag[0], q[1] - diag[1]] };
        return vec![p, corner, q];
    }
    let turns = rng.random_range(1..=max_turns.min(4)) as usize;
    let x_first = match first {
        Some(n) => n.0.abs() > n.1.abs(),
        None => dx.abs() >= dy.abs(),
    };
    // Legs alternate axes; split each axis' run into random pieces.
    let legs = turns + 1;
    let (n_first, n_second) = (legs.div_ceil(2), legs / 2);
    let mut split = |run: f32, pieces: usize| {
        let mut cuts: Vec<f32> = (1..pieces).map(|_| rng.random_range(0.2..0.8)).collect();
        cuts.sort_by(f32::total_cmp);
        let mut out = Vec::new();
        let mut last = 0.0;
        for c in cuts {
            out.push(run * (c - last));
            last = c;
        }
        out.push(run * (1.0 - last));
        out
    };
    let (run_first, run_second) = if x_first { (dx, dy) } else { (dy, dx) };
    let (mut a, mut b) = (split(run_first, n_first).into_iter(), split(run_second, n_second).into_iter());
    let mut corners = vec![p];
    let mut at = p;
    for leg in 0..legs {
        let along_first = leg % 2 == 0;
        let step = if along_first { a.next() } else { b.next() }.unwrap_or(0.0);
        let x_leg = along_first == x_first;
        at = if x_leg { [at[0] + step, at[1]] } else { [at[0], at[1] + step] };
        corners.push(at);
    }
    *corners.last_mut().unwrap() = q;
    corners
}

fn polyline_len(corners: &[[f32; 2]]) -> f32 {
    corners.windows(2).map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1])).sum()
}

/// Digs a natural tunnel along `route` (cells): a round brush whose
/// width wanders ±`vary` around `width_m` along the tunnel (noise, ~8 m
/// features), narrowing into the squeeze `(from, to, width)` with a
/// funnel on both sides. `salt` decorrelates the tunnels.
#[allow(clippy::too_many_arguments)]
fn dig_natural(
    mask: &mut CellMask,
    allowed: &[bool],
    route: &[usize],
    width_m: f32,
    squeeze: Option<(usize, usize, f32)>,
    vary: f32,
    noise: &NoiseField,
    salt: f32,
) {
    let cell = mask.cell_size_m;
    let radii: Vec<f32> = (0..route.len())
        .map(|i| {
            let s = i as f32 * cell;
            let swing = (vary * (noise.fbm(s / NATURAL_VARY_SCALE_M + salt, salt * 0.7, 2) - 0.5) * 2.0 * VARY_GAIN).clamp(-0.6, 0.6);
            let mut r = width_m / 2.0 * (1.0 + swing);
            if let Some((a, b, sw)) = squeeze {
                let d = if i < a { a - i } else { i.saturating_sub(b) } as f32 * cell;
                r = r.min(sw / 2.0 + d * SQUEEZE_FUNNEL);
            }
            // At least a plus-shaped brush: diagonal steps stay 4-connected.
            (r / cell).max(1.0)
        })
        .collect();
    for (k, &c) in route.iter().enumerate() {
        disc(mask, allowed, ((c % mask.width) as f32, (c / mask.width) as f32), radii[k]);
    }
}

/// Digs a hand-made tunnel along the `corners` (meters): straight legs, but
/// dug by hand — the centerline wanders sideways and the width changes a
/// little, by `rough` (0 = ruler-straight, constant width). Square brush,
/// so the walls stay flat-ish with small jogs.
fn dig_mine(mask: &mut CellMask, allowed: &[bool], corners: &[[f32; 2]], width_m: f32, rough: f32, noise: &NoiseField, salt: f32) {
    let (w, h, cell) = (mask.width, mask.height, mask.cell_size_m);
    let n = |s: f32, scale: f32, plane: f32| (noise.fbm(s / scale + salt, plane - salt, 2) - 0.5) * 2.0;
    let total = polyline_len(corners);
    let mut s = 0.0f32;
    for seg in corners.windows(2) {
        let (p, q) = (seg[0], seg[1]);
        let len = (q[0] - p[0]).hypot(q[1] - p[1]);
        let (nx, ny) = (-(q[1] - p[1]) / len.max(1e-6), (q[0] - p[0]) / len.max(1e-6));
        let steps = ((len / (cell * 0.5)).ceil() as usize).max(1);
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let at = s + len * t;
            // Taper the wander to 0 at the tunnel's ends so it meets the
            // chambers where it should.
            let taper = (at / MINE_TAPER_M).min((total - at) / MINE_TAPER_M).clamp(0.0, 1.0);
            let off = taper * rough * (MINE_SWAY_M * n(at, MINE_SWAY_SCALE_M, 3.1) + MINE_JITTER_M * n(at, MINE_JITTER_SCALE_M, 7.7));
            let width = width_m * (1.0 + rough * MINE_WIDTH_VARY * n(at, MINE_SWAY_SCALE_M * 0.6, 11.3));
            let (x, y) = (p[0] + (q[0] - p[0]) * t + nx * off, p[1] + (q[1] - p[1]) * t + ny * off);
            // At least 2 cells: a swaying 1-cell line could step diagonally.
            let k = ((width / cell).round() as i64).max(2);
            let (cx, cy) = ((x / cell).floor() as i64, (y / cell).floor() as i64);
            for dy in -(k / 2)..=(k - k / 2 - 1) {
                for dx in -(k / 2)..=(k - k / 2 - 1) {
                    let (gx, gy) = (cx + dx, cy + dy);
                    if gx >= 0 && gy >= 0 && gx < w as i64 && gy < h as i64 {
                        let c = gy as usize * w + gx as usize;
                        if allowed[c] {
                            mask.cells[c] = true;
                        }
                    }
                }
            }
        }
        s += len;
    }
}

/// Sets the `allowed` cells within `r` cells of `(x, y)` (cell coords).
fn disc(mask: &mut CellMask, allowed: &[bool], (x, y): (f32, f32), r: f32) {
    let (w, h) = (mask.width as i64, mask.height as i64);
    let ri = r.ceil() as i64;
    let r2 = r * r;
    for dy in -ri..=ri {
        for dx in -ri..=ri {
            if (dx * dx + dy * dy) as f32 > r2.max(0.5) {
                continue;
            }
            let (gx, gy) = (x as i64 + dx, y as i64 + dy);
            if gx >= 0 && gy >= 0 && gx < w && gy < h {
                let c = (gy * w + gx) as usize;
                if allowed[c] {
                    mask.cells[c] = true;
                }
            }
        }
    }
}

/// Distance in cells (8 directions, diagonal √2) from every cell to the
/// nearest `source` cell.
pub(crate) fn distance_field(w: usize, h: usize, source: impl Fn(usize) -> bool) -> Vec<f32> {
    let mut d = vec![f32::INFINITY; w * h];
    let mut heap = BinaryHeap::new();
    for (c, v) in d.iter_mut().enumerate() {
        if source(c) {
            *v = 0.0;
            heap.push(Reverse((0u32, c)));
        }
    }
    dijkstra(w, h, &mut d, heap, |_| true);
    d
}

/// Longest walk (cells, diagonal √2) over `floor` from the `sources`.
fn walk_farthest(floor: &CellMask, sources: &[usize]) -> f32 {
    let (w, h) = (floor.width, floor.height);
    let mut d = vec![f32::INFINITY; w * h];
    let mut heap = BinaryHeap::new();
    for &c in sources {
        if c < d.len() {
            d[c] = 0.0;
            heap.push(Reverse((0u32, c)));
        }
    }
    dijkstra(w, h, &mut d, heap, |c| floor.cells[c]);
    d.iter().copied().filter(|v| v.is_finite()).fold(0.0, f32::max)
}

fn dijkstra(w: usize, h: usize, d: &mut [f32], mut heap: BinaryHeap<Reverse<(u32, usize)>>, open: impl Fn(usize) -> bool) {
    while let Some(Reverse((g, c))) = heap.pop() {
        if (g as f32 / 1024.0) > d[c] + 1e-3 {
            continue;
        }
        let (cx, cy) = ((c % w) as i64, (c / w) as i64);
        for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
            let (x, y) = (cx + dx, cy + dy);
            if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                continue;
            }
            let nb = y as usize * w + x as usize;
            if !open(nb) {
                continue;
            }
            let nd = d[c] + if dx != 0 && dy != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
            if nd < d[nb] {
                d[nb] = nd;
                heap.push(Reverse(((nd * 1024.0) as u32, nb)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::generator::cave::{chambers, style};
    use crate::level::ChamberKind;
    use crate::model::MapKind;

    fn config(labyrinth: f32, mixing: f32) -> LevelConfig {
        let mut c = LevelConfig { kind: MapKind::Cave, ..LevelConfig::default() };
        c.map.width_m = 160.0;
        c.map.height_m = 140.0;
        c.cell_size_m = 1.0;
        c.cave.style.mixing_pct = mixing;
        c.cave.tunnels.labyrinth_pct = labyrinth;
        c
    }

    struct All {
        style: StyleLayout,
        chambers: ChamberLayout,
        t: TunnelBuild,
    }

    fn run(c: &LevelConfig, seed: u64) -> All {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let st = style::build(c, &mut rng).layout;
        let ch = chambers::build(c, &mut rng, &st).layout;
        let t = build(c, &mut rng, &st, &ch);
        All { style: st, chambers: ch, t }
    }

    fn floor(a: &All) -> CellMask {
        CellMask {
            width: a.chambers.width,
            height: a.chambers.height,
            cell_size_m: 1.0,
            cells: (0..a.chambers.ids.len()).map(|c| a.chambers.ids[c] > 0 || a.t.layout.mask.cells[c]).collect(),
        }
    }

    #[test]
    fn floor_is_one_piece() {
        for (lab, mixing) in [(0.0, 0.0), (40.0, 30.0), (100.0, 100.0), (70.0, 60.0)] {
            for seed in 0..2 {
                let a = run(&config(lab, mixing), seed);
                assert!(grid::is_connected(&floor(&a)), "labyrinth {lab} mixing {mixing} seed {seed}: {:?}", a.t.notes);
            }
        }
    }

    #[test]
    fn labyrinth_zero_is_a_chain() {
        let a = run(&config(0.0, 30.0), 3);
        let l = &a.t.layout;
        let links: Vec<&Tunnel> = l.tunnels.iter().filter(|t| !matches!(t.a, TunnelEnd::Gate(_))).collect();
        assert_eq!(links.len(), a.chambers.chambers.len() - 1, "a spanning chain");
        assert!(links.iter().all(|t| !t.extra && t.b != TunnelEnd::Blind));
        assert!(l.junctions.is_empty());
        let mut degree = vec![0; a.chambers.chambers.len() + 1];
        for t in &links {
            for e in [t.a, t.b] {
                if let TunnelEnd::Chamber(id) = e {
                    degree[id as usize] += 1;
                }
            }
        }
        assert!(degree.iter().all(|&d| d <= 2), "a chain never branches: {degree:?}");
    }

    #[test]
    fn higher_labyrinth_more_loops_junctions_dead_ends() {
        let count = |a: &All| {
            let l = &a.t.layout;
            (
                l.tunnels.iter().filter(|t| t.extra).count(),
                l.junctions.len(),
                l.tunnels.iter().filter(|t| t.b == TunnelEnd::Blind).count(),
            )
        };
        let (mid, maze) = (count(&run(&config(40.0, 30.0), 4)), count(&run(&config(100.0, 30.0), 4)));
        assert!(maze.0 > mid.0, "loops {mid:?} vs {maze:?}");
        assert!(mid.1 == 0 && maze.1 > 0, "junctions {mid:?} vs {maze:?}");
        assert!(maze.2 > mid.2, "dead ends {mid:?} vs {maze:?}");
    }

    #[test]
    fn low_mixing_keeps_zone_crossings_to_zone_links() {
        let mut c = config(100.0, 0.0);
        c.cave.style.zone_links = 1;
        for seed in 0..3 {
            let a = run(&c, seed);
            let with_chambers = a
                .style
                .zones
                .iter()
                .filter(|z| a.chambers.chambers.iter().any(|ch| ch.zone == z.id))
                .count();
            let cross = a.t.layout.tunnels.iter().filter(|t| t.cross_zone && !matches!(t.a, TunnelEnd::Gate(_))).count();
            assert!(cross < with_chambers.max(2), "seed {seed}: {cross} crossings");
        }
    }

    #[test]
    fn gates_open_at_the_edge_into_the_network() {
        let a = run(&config(40.0, 30.0), 5);
        let l = &a.t.layout;
        assert_eq!(l.gates.len(), 2);
        let f = floor(&a);
        for g in &l.gates {
            let c = (g.point_m[1] as usize).min(f.height - 1) * f.width + (g.point_m[0] as usize).min(f.width - 1);
            assert!(f.cells[c], "gate {} is not on the floor", g.id);
            assert_eq!(a.t.objects[g.object].category, Category::Gate);
        }
        assert!(l.main_route_m > 50.0);
    }

    #[test]
    fn hand_made_tunnels_run_straight() {
        let mut c = config(40.0, 30.0);
        c.cave.style.natural_pct = 0.0;
        c.cave.tunnels.diagonal_pct = 0.0;
        let a = run(&c, 6);
        for t in &a.t.layout.tunnels {
            assert_eq!(t.style, CaveStyle::HandMade);
            assert!(t.path_m.len() <= c.cave.tunnels.max_turns as usize + 2);
            for leg in t.path_m.windows(2) {
                let axis = (leg[0][0] - leg[1][0]).abs() < 1e-3 || (leg[0][1] - leg[1][1]).abs() < 1e-3;
                assert!(axis, "{t:?}");
            }
            assert!((t.width_m - c.cave.tunnels.mine_width_m).abs() < 1e-3);
        }
        assert!(a.chambers.chambers.iter().any(|ch| ch.kind == ChamberKind::Hall));
    }

    #[test]
    fn same_seed_same_tunnels() {
        let c = config(60.0, 50.0);
        assert_eq!(run(&c, 7).t.layout, run(&c, 7).t.layout);
    }
}
