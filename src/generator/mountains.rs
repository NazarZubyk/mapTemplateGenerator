//! A mountain region of a mixed map (docs/mixed-plan.md, M7): a cave
//! region is a rock massif with the cave system inside it and passes over
//! it.
//!
//! 1. **Passes** — between pairs of the region's openings, the farthest
//!    apart first (each opening used once); with too few openings, a climb
//!    from one up toward the heart of the massif, stopping partway at a
//!    cave entrance. Along the low ground: a route
//!    over the region's own ground whose cost grows with the height (the
//!    saddles and valleys are cheap) and wanders with a noise; it keeps
//!    clear of the openings left for cave mouths. Stamped `width_m` wide.
//! 2. **The caves' rock** — the region minus the passes and `buffer_m` of
//!    solid rock around them: the caves never break into a pass.
//! 3. **Entrances** — off each pass, `entrances` short spurs through the
//!    buffer to the caves' rock, at a spot where the rock beyond is deep
//!    enough; each spur's end is a gate of the caves (a way down).
//! 4. **The caves** — the cave chain inside that rock, its gates the
//!    openings no pass took (cave mouths in the rock face) and the pass
//!    entrances. So the whole mountain is one piece: the passes, and the
//!    caves reached from them and from their own mouths.

use rand::Rng;
use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};

use crate::generator::mixed::connections::capsule_cells;
use crate::generator::noise::NoiseField;
use crate::generator::region::{edge_distance, FixedGate, FixedGateKind, RegionCtx};
use crate::generator::swamp::paths::astar;
use crate::generator::Progress;
use crate::level::{CellMask, GeneratedLevel, MountainPass, MountainPasses};
use crate::model::{GateStyle, LevelConfig};

/// Openings left for cave mouths: a pass keeps this far from them, meters.
const MOUTH_CLEAR_M: f32 = 10.0;
/// Width of the spur from a pass down to a cave entrance, meters.
const SPUR_M: f32 = 2.5;
/// The rock beyond an entrance must be at least this deep, meters.
const ENTRANCE_ROOM_M: f32 = 5.0;
/// How far the caves' edge distance is computed, meters.
const EDGE_LIMIT_M: f32 = 80.0;

/// Build a mountain region: its passes, then its caves.
pub fn generate_in(config: &LevelConfig, seed: u64, ctx: &RegionCtx, progress: Progress) -> GeneratedLevel {
    let s = &config.cave.passes;
    // The passes have their own stream: the caves' stays the cave chain's.
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x7061_7373_6573_0000);
    let (w, h, cell) = (ctx.domain.width, ctx.domain.height, ctx.domain.cell_size_m);
    let n = w * h;
    let mut notes = Vec::new();
    let center = |c: usize| [((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell];
    let cell_at = |p: [f32; 2]| ((p[1] / cell).max(0.0) as usize).min(h - 1) * w + ((p[0] / cell).max(0.0) as usize).min(w - 1);
    // A gate's first cell on the region's ground.
    let gate_cell = |g: &FixedGate| {
        (0..12)
            .map(|k| cell_at([g.point_m[0] + g.inward[0] * cell * (k as f32 + 0.5), g.point_m[1] + g.inward[1] * cell * (k as f32 + 0.5)]))
            .find(|&c| ctx.inside(c))
    };

    // 1. Pairs: the farthest apart first, each opening once.
    let gates = &ctx.gates;
    let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
    for i in 0..gates.len() {
        for j in i + 1..gates.len() {
            let d = (gates[i].point_m[0] - gates[j].point_m[0]).hypot(gates[i].point_m[1] - gates[j].point_m[1]);
            pairs.push((d, i, j));
        }
    }
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0).then((a.1, a.2).cmp(&(b.1, b.2))));
    let mut used = vec![false; gates.len()];
    let mut chosen = Vec::new();
    for &(_, i, j) in &pairs {
        if chosen.len() >= s.count as usize {
            break;
        }
        if !used[i] && !used[j] {
            used[i] = true;
            used[j] = true;
            chosen.push((i, j));
        }
    }
    // Not enough openings for the rest: a climb from a free opening up into
    // the massif, ending at a cave entrance (a trail up to the caves).
    let mut climbs = Vec::new();
    while (chosen.len() + climbs.len()) < s.count as usize {
        let free: Vec<usize> = (0..gates.len()).filter(|&i| !used[i]).collect();
        let Some(&i) = free.first() else { break };
        used[i] = true;
        climbs.push(i);
    }
    if ((chosen.len() + climbs.len()) as u32) < s.count {
        notes.push(format!("Passes: {} of {} — every opening already has one", chosen.len() + climbs.len(), s.count));
    }

    // 2. Route and stamp the passes.
    let (lo, hi) = (0..n).filter(|&c| ctx.inside(c)).fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), c| (a.min(ctx.land.height_m[c]), b.max(ctx.land.height_m[c])));
    let span = (hi - lo).max(1e-6);
    let low = (s.low_ground_pct / 100.0).clamp(0.0, 1.0);
    let wiggle = (s.wiggle_pct / 100.0).clamp(0.0, 1.0);
    let noise = NoiseField::new(rng.random::<u64>());
    let mouths: Vec<[f32; 2]> = (0..gates.len()).filter(|&i| !used[i]).map(|i| gates[i].point_m).collect();
    let cost = |c: usize| {
        if !ctx.inside(c) {
            return f32::INFINITY;
        }
        let p = center(c);
        let height = (ctx.land.height_m[c] - lo) / span;
        let wander = (wiggle * (noise.fbm(p[0] / 12.0, p[1] / 12.0, 2) - 0.5) * 2.5).exp();
        let near_mouth = mouths.iter().any(|m| (m[0] - p[0]).hypot(m[1] - p[1]) < MOUTH_CLEAR_M);
        (1.0 + 6.0 * low * height) * wander * if near_mouth { 6.0 } else { 1.0 }
    };
    let cheapest = (-wiggle * 1.25).exp();
    let mut mask = CellMask::new(w, h, cell);
    let mut passes: Vec<MountainPass> = Vec::new();
    let mut routes: Vec<Vec<usize>> = Vec::new();
    // The heart of the massif (deepest from its edge): where a climb heads.
    let heart = (0..n).filter(|&c| ctx.inside(c)).max_by(|&a, &b| ctx.edge_dist[a].total_cmp(&ctx.edge_dist[b]).then(b.cmp(&a)));
    let jobs: Vec<(usize, Option<usize>)> = chosen.iter().map(|&(i, j)| (i, Some(j))).chain(climbs.iter().map(|&i| (i, None))).collect();
    for &(i, j) in &jobs {
        let Some(a) = gate_cell(&gates[i]) else { continue };
        let b = match j {
            Some(j) => gate_cell(&gates[j]),
            None => heart,
        };
        let Some(b) = b else { continue };
        let mut route = astar(w, h, a, b, cost, cheapest);
        if route.first() != Some(&a) {
            notes.push(format!("Passes: no way over the massif from opening {}", i + 1));
            continue;
        }
        // A climb stops partway up: its cave entrance is there.
        if j.is_none() {
            route.truncate((route.len() * 3 / 5).max(2));
        }
        let last = center(*route.last().unwrap());
        let path: Vec<[f32; 2]> = std::iter::once(gates[i].point_m)
            .chain(route.iter().step_by(3).map(|&c| center(c)))
            .chain(std::iter::once(last))
            .chain(j.map(|j| gates[j].point_m))
            .collect();
        for seg in path.windows(2) {
            for c in capsule_cells(w, h, cell, seg[0], seg[1], s.width_m.max(cell) / 2.0) {
                mask.cells[c] |= ctx.inside(c);
            }
        }
        let length_m = path.windows(2).map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1])).sum();
        passes.push(MountainPass { id: passes.len() as u32 + 1, gates: [i as u32 + 1, j.map_or(0, |j| j as u32 + 1)], width_m: s.width_m, length_m, path_m: path, entrances: Vec::new() });
        routes.push(route);
    }

    // 3. The caves' rock: the region minus the passes and their buffer.
    let keep = s.width_m.max(cell) / 2.0 + s.buffer_m.max(0.0);
    let (to_pass, _) = crate::generator::forest::distance_field(w, h, cell, |c| mask.cells[c], keep + cell * 2.0);
    let mut rock = CellMask { width: w, height: h, cell_size_m: cell, cells: (0..n).map(|c| ctx.inside(c) && to_pass[c] > keep).collect() };
    // A pass across the massif cuts its rock apart: the caves live in the
    // biggest piece (the rest stays solid rock).
    let (labels, count) = crate::generator::grid::components(&rock);
    if count > 1 {
        let mut size = vec![0usize; count as usize + 1];
        for &l in &labels {
            size[l as usize] += 1;
        }
        let main = (1..=count as usize).max_by_key(|&l| (size[l], std::cmp::Reverse(l))).unwrap_or(1);
        for (r, &l) in rock.cells.iter_mut().zip(&labels) {
            *r = l as usize == main;
        }
    }
    let rock_edge = edge_distance(&rock, EDGE_LIMIT_M);

    // The openings no pass took: a cave mouth where the caves' rock is
    // there; otherwise a trail to the nearest pass (around the caves' rock).
    let mut cave_gates: Vec<FixedGate> = Vec::new();
    for i in (0..gates.len()).filter(|&i| !used[i]) {
        let g = &gates[i];
        let on_rock = (0..12).any(|k| rock.cells[cell_at([g.point_m[0] + g.inward[0] * cell * (k as f32 + 0.5), g.point_m[1] + g.inward[1] * cell * (k as f32 + 0.5)])]);
        if on_rock || mask.count_true() == 0 {
            cave_gates.push(*g);
            continue;
        }
        let Some(a) = gate_cell(g) else { continue };
        let target = (0..n).filter(|&c| mask.cells[c]).min_by(|&p, &q| {
            let d = |c: usize| (center(c)[0] - g.point_m[0]).hypot(center(c)[1] - g.point_m[1]);
            d(p).total_cmp(&d(q)).then(p.cmp(&q))
        });
        let Some(b) = target else { continue };
        let route = astar(w, h, a, b, |c| if rock.cells[c] { f32::INFINITY } else { cost(c) }, cheapest);
        if route.first() != Some(&a) {
            notes.push(format!("Passes: opening {} has no way to a pass or the caves", i + 1));
            continue;
        }
        let path: Vec<[f32; 2]> = std::iter::once(g.point_m).chain(route.iter().step_by(3).map(|&c| center(c))).chain(std::iter::once(center(b))).collect();
        for seg in path.windows(2) {
            for c in capsule_cells(w, h, cell, seg[0], seg[1], s.width_m.max(cell) / 2.0) {
                mask.cells[c] |= ctx.inside(c) && !rock.cells[c];
            }
        }
    }

    // 4. Entrances: a spur from the pass, across the buffer, onto rock deep
    // enough for a way down.
    for (k, route) in routes.iter().enumerate() {
        let mut made = 0;
        // A climb leads to a cave entrance at its top, whatever the count.
        let climb = passes[k].gates[1] == 0;
        let want = if climb { s.entrances.max(1) } else { s.entrances };
        for attempt in 0..(want * 8) {
            if made >= want || route.len() < 6 {
                break;
            }
            // A climb's first entrance is at its end; the rest anywhere.
            let t = if climb && made == 0 { 1.0 - attempt as f32 * 0.04 } else { rng.random_range(0.25..0.75f32) };
            let at = (t * (route.len() - 1) as f32) as usize;
            let (p0, p1) = (center(route[at.saturating_sub(2)]), center(route[(at + 2).min(route.len() - 1)]));
            let (tx, ty) = (p1[0] - p0[0], p1[1] - p0[1]);
            let tl = tx.hypot(ty).max(1e-6);
            let side = if rng.random::<bool>() { 1.0 } else { -1.0 };
            let nrm = [-ty / tl * side, tx / tl * side];
            let from = center(route[at]);
            // Out along the normal to the first cell of the caves' rock.
            let found = (1..((keep + 4.0) / cell * 2.0) as usize).map(|k| [from[0] + nrm[0] * k as f32 * cell / 2.0, from[1] + nrm[1] * k as f32 * cell / 2.0]).find(|&q| rock.cells[cell_at(q)]);
            let Some(q) = found else { continue };
            let deep = [q[0] + nrm[0] * ENTRANCE_ROOM_M * 1.6, q[1] + nrm[1] * ENTRANCE_ROOM_M * 1.6];
            if !rock.cells[cell_at(deep)] || rock_edge[cell_at(deep)] < ENTRANCE_ROOM_M {
                continue;
            }
            for c in capsule_cells(w, h, cell, from, q, SPUR_M / 2.0) {
                if ctx.inside(c) && !rock.cells[c] {
                    mask.cells[c] = true;
                }
            }
            cave_gates.push(FixedGate { point_m: q, inward: nrm, width_m: SPUR_M, kind: FixedGateKind::PassEntrance { pass: k as u32 + 1 }, style: GateStyle::Shore });
            passes[k].entrances.push(q);
            made += 1;
        }
        if made == 0 && rock.count_true() > 0 {
            // No deep rock beside it: the entrance goes where the caves' rock
            // comes nearest to this pass (passes and caves stay one piece).
            let mine = |c: usize| route.contains(&c);
            let (d, src) = crate::generator::forest::distance_field(w, h, cell, mine, keep * 4.0 + 20.0);
            let best = (0..n).filter(|&c| rock.cells[c] && rock_edge[c] >= 1.5 && d[c].is_finite()).min_by(|&a, &b| d[a].total_cmp(&d[b]).then(a.cmp(&b)));
            if let Some(q) = best {
                let (from, to) = (center(src[q]), center(q));
                let l = (to[0] - from[0]).hypot(to[1] - from[1]).max(1e-6);
                for c in capsule_cells(w, h, cell, from, to, SPUR_M / 2.0) {
                    if ctx.inside(c) && !rock.cells[c] {
                        mask.cells[c] = true;
                    }
                }
                cave_gates.push(FixedGate {
                    point_m: to,
                    inward: [(to[0] - from[0]) / l, (to[1] - from[1]) / l],
                    width_m: SPUR_M,
                    kind: FixedGateKind::PassEntrance { pass: k as u32 + 1 },
                    style: GateStyle::Shore,
                });
                passes[k].entrances.push(to);
                made = 1;
            }
        }
        if made < want {
            notes.push(format!("Passes: pass {} has {made} of {want} cave entrances (no deep rock beside it)", k + 1));
        }
    }
    // A spur cell is no cave rock.
    for (r, &m) in rock.cells.iter_mut().zip(&mask.cells) {
        *r &= !m;
    }

    // 5. The caves.
    let caves = RegionCtx {
        id: ctx.id,
        edge_dist: edge_distance(&rock, EDGE_LIMIT_M),
        domain: rock,
        gates: cave_gates,
        land: ctx.land.clone(),
        water: ctx.water.clone(),
        bridges: ctx.bridges.clone(),
        blend: ctx.blend.clone(),
        neighbour_trees: ctx.neighbour_trees,
        blend_seed: ctx.blend_seed,
        embankment_m: ctx.embankment_m,
    };
    let mut level = crate::generator::cave::generate_in(config, seed, Some(&caves), progress);
    level.notes.splice(0..0, notes);
    if let Some(cave) = level.cave.as_mut() {
        cave.passes = Some(MountainPasses { mask, passes });
    }
    level
}
