//! Cave node 6, hazards (step C5 in docs/cave-plan.md): chasms and pools.
//!
//! 1. **Which chambers** — per chamber in id order: a hall rolls
//!    `chasm_pct` for a chasm; a chamber without one rolls `pool_pct` for
//!    a pool. At most one hazard per chamber.
//! 2. **Shape** — a blob of `chasm_size_pct` / `pool_size_pct` of the
//!    chamber's floor, centered near the chamber's middle (pools further
//!    off, in a "low spot"): a rough ellipse along the chamber's long
//!    axis — natural in hand-made rooms too (nobody digs a chasm). Only floor at least
//!    `MARGIN_M` from the wall and `GATE_CLEAR_M` from a gate is used, so a
//!    walkable ring stays around it. The largest piece is kept.
//! 3. **Pools** — the cells within `shallow_m` of the pool's edge are
//!    shallow water (walkable), the rest deep (impassable).
//! 4. **Guard** — the walkable floor must stay one piece; otherwise the
//!    hazard shrinks (×0.7, up to 3 times), then it is dropped (note).
//! 5. **Rope bridges** — `bridge_pct` of the chasms get a straight bridge
//!    (`bridge_width_m`) across their narrow axis through the middle.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::cave::tunnels::distance_field;
use crate::generator::grid;
use crate::generator::noise::NoiseField;
use crate::level::{
    CarveLayout, CellMask, Chamber, ChamberKind, ChamberLayout, Hazard, HazardKind, HazardLayout, TunnelLayout,
};
use crate::model::LevelConfig;

pub struct HazardBuild {
    pub layout: HazardLayout,
    pub notes: Vec<String>,
}

/// Least floor between a hazard and the wall, meters.
const MARGIN_M: f32 = 2.0;
/// Hazards keep this far from a gate point, meters.
const GATE_CLEAR_M: f32 = 6.0;
/// How far the center may stray from the chamber's middle, as a share of
/// the chamber radius.
const CHASM_OFFSET: f32 = 0.2;
const POOL_OFFSET: f32 = 0.35;
const SHRINK: f32 = 0.7;
/// Largest share of the usable floor (off walls and gates) a hazard takes.
const MAX_FILL: f32 = 0.6;

pub fn build(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    chambers: &ChamberLayout,
    tunnels: &TunnelLayout,
    carve: &CarveLayout,
) -> HazardBuild {
    let s = &config.cave.hazards;
    let floor = &carve.floor;
    let (w, h, cell) = (floor.width, floor.height, floor.cell_size_m);
    let n = w * h;
    let mut notes = Vec::new();
    let noise = NoiseField::new(rng.random::<u64>());
    let to_rock = distance_field(w, h, |c| !floor.cells[c]);
    let center = |c: usize| (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
    let near_gate = |c: usize| {
        let (x, y) = center(c);
        tunnels.gates.iter().any(|g| (g.point_m[0] - x).hypot(g.point_m[1] - y) < GATE_CLEAR_M)
    };
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); chambers.chambers.len() + 1];
    for c in 0..n {
        let id = chambers.ids[c] as usize;
        if id > 0 && floor.cells[c] {
            members[id].push(c);
        }
    }

    let mut chasm = CellMask::new(w, h, cell);
    let mut bridge = CellMask::new(w, h, cell);
    let mut shallow = CellMask::new(w, h, cell);
    let mut deep = CellMask::new(w, h, cell);
    let mut hazards = Vec::new();
    let mut dropped = 0;
    let margin = MARGIN_M / cell;
    // Walkable floor so far (kept up to date as hazards are placed).
    let mut walk = floor.clone();

    for ch in &chambers.chambers {
        // Always roll everything, so the stream does not depend on outcomes.
        let (chasm_roll, pool_roll, bridge_roll) = (rng.random::<f32>() * 100.0, rng.random::<f32>() * 100.0, rng.random::<f32>() * 100.0);
        let (off_angle, off_dist) = (rng.random_range(0.0..std::f32::consts::TAU), rng.random::<f32>());
        let kind = if ch.kind == ChamberKind::Hall && chasm_roll < s.chasm_pct {
            HazardKind::Chasm
        } else if pool_roll < s.pool_pct {
            HazardKind::Pool
        } else {
            continue;
        };
        let cells = &members[ch.id as usize];
        let allowed: Vec<usize> = cells.iter().copied().filter(|&c| to_rock[c] >= margin && !near_gate(c)).collect();
        if allowed.is_empty() {
            dropped += 1;
            continue;
        }
        let size_pct = if kind == HazardKind::Chasm { s.chasm_size_pct } else { s.pool_size_pct };
        let offset = if kind == HazardKind::Chasm { CHASM_OFFSET } else { POOL_OFFSET };
        // Never fill the usable floor: a blob squeezed into a small room
        // would take the room's rectangular outline.
        let mut target = ((size_pct.clamp(0.0, 90.0) / 100.0 * cells.len() as f32).round() as usize)
            .min((allowed.len() as f32 * MAX_FILL) as usize);
        let mut placed = false;
        for _ in 0..4 {
            if target < 4 {
                break;
            }
            let blob = shape(ch, &allowed, target, (off_angle, off_dist * offset), &noise, w, cell, hazards.len() as f32 * 19.3);
            let (blocked, wet) = match kind {
                HazardKind::Chasm => (blob.clone(), Vec::new()),
                HazardKind::Pool => split_pool(&blob, s.shallow_m, w, h, cell),
            };
            // Guard: the walkable floor stays one piece.
            if !stays_connected(&walk, &blocked) {
                target = (target as f32 * SHRINK) as usize;
                continue;
            }
            for &c in &blocked {
                walk.cells[c] = false;
            }
            let id = hazards.len() as u16 + 1;
            let (mx, my) = blob.iter().fold((0.0, 0.0), |a, &c| (a.0 + center(c).0, a.1 + center(c).1));
            let mid = [mx / blob.len() as f32, my / blob.len() as f32];
            let mut over = None;
            match kind {
                HazardKind::Chasm => {
                    for &c in &blob {
                        chasm.cells[c] = true;
                    }
                    if bridge_roll < s.bridge_pct {
                        over = rope_bridge(&blob, &chasm, floor, &deep, &mut bridge, s.bridge_width_m, w, h, cell);
                    }
                }
                HazardKind::Pool => {
                    for &c in &blocked {
                        deep.cells[c] = true;
                    }
                    for &c in &wet {
                        shallow.cells[c] = true;
                    }
                }
            }
            hazards.push(Hazard {
                id,
                kind,
                chamber: ch.id,
                center_m: mid,
                blocked_m2: blocked.len() as f32 * cell * cell,
                shallow_m2: wet.len() as f32 * cell * cell,
                bridge: over,
            });
            placed = true;
            break;
        }
        if !placed {
            dropped += 1;
        }
    }
    if dropped > 0 {
        notes.push(format!("Hazards: {dropped} found no room that keeps the floor in one piece — smaller hazards or bigger chambers"));
    }
    HazardBuild { layout: HazardLayout { chasm, bridge, shallow, deep, hazards }, notes }
}

/// The hazard's cells: the `target` best of `allowed` by a blob score
/// around the chamber's middle moved by (angle, share of the radius),
/// largest piece only.
#[allow(clippy::too_many_arguments)]
fn shape(ch: &Chamber, allowed: &[usize], target: usize, (angle, off): (f32, f32), noise: &NoiseField, w: usize, cell: f32, salt: f32) -> Vec<usize> {
    let r_ch = (ch.area_m2 / std::f32::consts::PI).sqrt();
    let (cx, cy) = (ch.center_m[0] + angle.cos() * off * r_ch, ch.center_m[1] + angle.sin() * off * r_ch);
    let r = (target as f32 * cell * cell / std::f32::consts::PI).sqrt().max(cell);
    // Long axis of the chamber, map coordinates (y down).
    let a = -ch.angle_deg.to_radians();
    let (sin, cos) = a.sin_cos();
    let mut scored: Vec<(f32, usize)> = allowed
        .iter()
        .map(|&c| {
            let (x, y) = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
            let (dx, dy) = (x - cx, y - cy);
            let (u, v) = (dx * cos + dy * sin, -dx * sin + dy * cos);
            let rough = (noise.fbm(x / 3.0 + salt, y / 3.0 - salt, 3) - 0.5) * 2.0;
            // Natural in every chamber: nobody digs a chasm or a pool.
            (1.0 - (u * u + v * v) / (r * r) + 0.6 * rough, c)
        })
        .collect();
    scored.sort_by(|p, q| q.0.total_cmp(&p.0).then(p.1.cmp(&q.1)));
    let picked: Vec<usize> = scored.iter().take(target).map(|&(_, c)| c).collect();
    largest_piece(&picked, w, allowed.iter().map(|&c| c / w).max().unwrap_or(0) + 1, cell)
}

/// Does the walkable floor `walk` (one piece) stay one piece without the
/// `blocked` cells? Every remaining cell still reaches the cells around the
/// blocked ones, so it does exactly when those ring cells still reach each
/// other. Checked near the blob first (cheap), then over the whole map.
fn stays_connected(walk: &CellMask, blocked: &[usize]) -> bool {
    let (w, h) = (walk.width, walk.height);
    if blocked.is_empty() {
        return true;
    }
    let mut gone = std::collections::HashSet::with_capacity(blocked.len());
    gone.extend(blocked.iter().copied());
    let open = |c: usize| walk.cells[c] && !gone.contains(&c);
    let n4 = |c: usize| {
        let (x, y) = (c % w, c / w);
        [(x > 0).then(|| c - 1), (x + 1 < w).then(|| c + 1), (y > 0).then(|| c - w), (y + 1 < h).then(|| c + w)].into_iter().flatten()
    };
    let mut ring: Vec<usize> = blocked.iter().flat_map(|&c| n4(c)).filter(|&c| open(c)).collect();
    ring.sort_unstable();
    ring.dedup();
    if ring.len() <= 1 {
        return true;
    }
    let (x0, x1) = blocked.iter().fold((usize::MAX, 0), |a, &c| (a.0.min(c % w), a.1.max(c % w)));
    let (y0, y1) = blocked.iter().fold((usize::MAX, 0), |a, &c| (a.0.min(c / w), a.1.max(c / w)));
    let pad = (x1 - x0).max(y1 - y0).max(8);
    let local = (x0.saturating_sub(pad), y0.saturating_sub(pad), (x1 + pad + 1).min(w), (y1 + pad + 1).min(h));
    let reach_all = |bounds: (usize, usize, usize, usize)| {
        let inside = |c: usize| {
            let (x, y) = (c % w, c / w);
            x >= bounds.0 && y >= bounds.1 && x < bounds.2 && y < bounds.3
        };
        let mut seen = std::collections::HashSet::new();
        let mut stack = vec![ring[0]];
        seen.insert(ring[0]);
        while let Some(c) = stack.pop() {
            for nb in n4(c) {
                if inside(nb) && open(nb) && seen.insert(nb) {
                    stack.push(nb);
                }
            }
        }
        ring.iter().all(|c| seen.contains(c))
    };
    reach_all(local) || reach_all((0, 0, w, h))
}

fn largest_piece(cells: &[usize], w: usize, h: usize, cell: f32) -> Vec<usize> {
    let mut m = CellMask::new(w, h, cell);
    for &c in cells {
        m.cells[c] = true;
    }
    let (labels, count) = grid::components(&m);
    let mut size = vec![0u32; count as usize + 1];
    for &c in cells {
        size[labels[c] as usize] += 1;
    }
    let keep = (1..=count).max_by_key(|&l| (size[l as usize], std::cmp::Reverse(l))).unwrap_or(0);
    cells.iter().copied().filter(|&c| labels[c] == keep).collect()
}

/// Splits a pool into (deep, shallow): shallow = within `shallow_m` of the
/// pool's edge.
fn split_pool(pool: &[usize], shallow_m: f32, w: usize, h: usize, cell: f32) -> (Vec<usize>, Vec<usize>) {
    let _ = h;
    if pool.is_empty() {
        return (Vec::new(), Vec::new());
    }
    // Only the pool's box (one cell of margin) matters.
    let (x0, x1) = pool.iter().fold((usize::MAX, 0), |a, &c| (a.0.min(c % w), a.1.max(c % w)));
    let (y0, y1) = pool.iter().fold((usize::MAX, 0), |a, &c| (a.0.min(c / w), a.1.max(c / w)));
    let (bx, by) = (x0.saturating_sub(1), y0.saturating_sub(1));
    let (lw, lh) = (x1 + 2 - bx, y1 + 2 - by);
    let mut inside = vec![false; lw * lh];
    for &c in pool {
        inside[(c / w - by) * lw + (c % w - bx)] = true;
    }
    let to_edge = distance_field(lw, lh, |c| !inside[c]);
    let band = shallow_m.max(0.0) / cell;
    pool.iter().copied().partition(|&c| to_edge[(c / w - by) * lw + (c % w - bx)] > band + 0.5)
}

/// A straight rope bridge across the chasm's narrow axis through its
/// middle; returns its two ends (on walkable floor) or None.
#[allow(clippy::too_many_arguments)]
fn rope_bridge(
    blob: &[usize],
    chasm: &CellMask,
    floor: &CellMask,
    deep: &CellMask,
    bridge: &mut CellMask,
    width_m: f32,
    w: usize,
    h: usize,
    cell: f32,
) -> Option<[[f32; 2]; 2]> {
    let pts: Vec<(f32, f32)> = blob.iter().map(|&c| ((c % w) as f32 + 0.5, (c / w) as f32 + 0.5)).collect();
    let k = pts.len() as f32;
    let (mx, my) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0 / k, a.1 + p.1 / k));
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for &(x, y) in &pts {
        sxx += (x - mx) * (x - mx);
        syy += (y - my) * (y - my);
        sxy += (x - mx) * (y - my);
    }
    // Minor axis of the covariance: the narrow way across.
    let major = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let (dx, dy) = ((major + std::f32::consts::FRAC_PI_2).cos(), (major + std::f32::consts::FRAC_PI_2).sin());
    let walk = |x: f32, y: f32| {
        let (xi, yi) = (x.floor() as i64, y.floor() as i64);
        if xi < 0 || yi < 0 || xi >= w as i64 || yi >= h as i64 {
            return None;
        }
        let c = yi as usize * w + xi as usize;
        Some((chasm.cells[c], floor.cells[c] && !deep.cells[c]))
    };
    // From the middle out both ways until the chasm ends on floor.
    let mut ends = [(0.0f32, 0.0f32); 2];
    for (e, sign) in [1.0f32, -1.0].into_iter().enumerate() {
        let mut t = 0.0;
        loop {
            t += 0.25;
            let (x, y) = (mx + sign * dx * t, my + sign * dy * t);
            match walk(x, y) {
                Some((true, _)) => continue,
                Some((false, true)) => {
                    ends[e] = (x, y);
                    break;
                }
                _ => return None,
            }
        }
    }
    let half = (width_m.max(cell) / cell / 2.0).max(0.5);
    let (ax, ay, bx, by) = (ends[0].0, ends[0].1, ends[1].0, ends[1].1);
    let (lx, ly) = (bx - ax, by - ay);
    let len2 = (lx * lx + ly * ly).max(1e-6);
    for &c in blob {
        let (x, y) = ((c % w) as f32 + 0.5, (c / w) as f32 + 0.5);
        let t = (((x - ax) * lx + (y - ay) * ly) / len2).clamp(0.0, 1.0);
        let (px, py) = (ax + lx * t, ay + ly * t);
        if (x - px).hypot(y - py) <= half {
            bridge.cells[c] = true;
        }
    }
    Some([[ax * cell, ay * cell], [bx * cell, by * cell]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::generator::cave::{carve, chambers, style, tunnels};
    use crate::model::MapKind;

    fn config() -> LevelConfig {
        let mut c = LevelConfig { kind: MapKind::Cave, ..LevelConfig::default() };
        c.map.width_m = 160.0;
        c.map.height_m = 140.0;
        c.cell_size_m = 0.5;
        c
    }

    struct All {
        chambers: ChamberLayout,
        tunnels: TunnelLayout,
        carve: CarveLayout,
        hz: HazardBuild,
    }

    fn run(c: &LevelConfig, seed: u64) -> All {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let st = style::build(c, &mut rng).layout;
        let ch = chambers::build(c, &mut rng, &st).layout;
        let t = tunnels::build(c, &mut rng, &st, &ch).layout;
        let cv = carve::build(c, &mut rng, &st, &ch, &t).layout;
        let hz = build(c, &mut rng, &ch, &t, &cv);
        All { chambers: ch, tunnels: t, carve: cv, hz }
    }

    fn walkable(a: &All) -> CellMask {
        let f = &a.carve.floor;
        CellMask { cells: (0..f.cells.len()).map(|c| a.hz.layout.walkable(f, c)).collect(), ..f.clone() }
    }

    #[test]
    fn walkable_floor_stays_one_piece() {
        let mut c = config();
        c.cave.hazards.chasm_pct = 100.0;
        c.cave.hazards.pool_pct = 100.0;
        c.cave.hazards.chasm_size_pct = 60.0;
        c.cave.hazards.pool_size_pct = 60.0;
        for seed in 0..3 {
            let a = run(&c, seed);
            assert!(grid::is_connected(&walkable(&a)), "seed {seed}");
            assert!(a.hz.layout.hazards.len() >= a.chambers.chambers.len() / 2, "seed {seed}: {:?}", a.hz.notes);
        }
    }

    #[test]
    fn hazards_stay_in_their_chambers_off_walls_and_gates() {
        let mut c = config();
        c.cave.hazards.chasm_pct = 100.0;
        c.cave.hazards.pool_pct = 100.0;
        let a = run(&c, 4);
        let l = &a.hz.layout;
        let w = l.chasm.width;
        for k in 0..l.chasm.cells.len() {
            let hazard = l.chasm.cells[k] || l.shallow.cells[k] || l.deep.cells[k];
            if !hazard {
                continue;
            }
            assert!(a.chambers.ids[k] > 0 && a.carve.floor.cells[k], "hazard off the chamber floor");
            if l.chasm.cells[k] {
                let ch = &a.chambers.chambers[a.chambers.ids[k] as usize - 1];
                assert_eq!(ch.kind, ChamberKind::Hall, "chasms are for halls");
            }
            assert!(!(l.shallow.cells[k] && l.deep.cells[k]));
            let (x, y) = ((k % w) as f32 * 0.5, (k / w) as f32 * 0.5);
            for g in &a.tunnels.gates {
                assert!((g.point_m[0] - x).hypot(g.point_m[1] - y) > GATE_CLEAR_M - 1.0);
            }
        }
        for hz in &l.hazards {
            if hz.kind == HazardKind::Pool {
                assert!(hz.shallow_m2 > 0.0, "{hz:?}");
            }
        }
    }

    #[test]
    fn bridges_cross_chasms() {
        let mut c = config();
        c.cave.hazards.chasm_pct = 100.0;
        c.cave.hazards.bridge_pct = 100.0;
        c.cave.hazards.pool_pct = 0.0;
        let a = run(&c, 5);
        let l = &a.hz.layout;
        assert!(l.hazards.iter().any(|h| h.bridge.is_some()));
        for k in 0..l.bridge.cells.len() {
            if l.bridge.cells[k] {
                assert!(l.chasm.cells[k]);
            }
        }
        for h in l.hazards.iter().filter(|h| h.bridge.is_some()) {
            let [p, q] = h.bridge.unwrap();
            for e in [p, q] {
                let k = (e[1] / 0.5) as usize * l.chasm.width + (e[0] / 0.5) as usize;
                assert!(a.hz.layout.walkable(&a.carve.floor, k), "bridge end not on walkable floor");
            }
        }
    }

    #[test]
    fn chasms_in_hand_made_rooms_are_not_rectangles() {
        let mut c = config();
        c.cave.style.natural_pct = 0.0;
        c.cave.hazards.chasm_pct = 100.0;
        c.cave.hazards.chasm_size_pct = 40.0;
        c.cave.hazards.bridge_pct = 0.0;
        c.cave.hazards.pool_pct = 0.0;
        let a = run(&c, 8);
        let l = &a.hz.layout;
        let w = l.chasm.width;
        assert!(!l.hazards.is_empty());
        for hz in &l.hazards {
            let cells: Vec<usize> = (0..l.chasm.cells.len())
                .filter(|&k| l.chasm.cells[k] && a.chambers.ids[k] == hz.chamber)
                .collect();
            let xs = cells.iter().map(|&k| k % w);
            let ys = cells.iter().map(|&k| k / w);
            let bbox = (xs.clone().max().unwrap() - xs.min().unwrap() + 1) * (ys.clone().max().unwrap() - ys.min().unwrap() + 1);
            assert!((cells.len() as f32) < bbox as f32 * 0.85, "chasm {} looks rectangular", hz.id);
        }
    }

    #[test]
    fn zero_shares_no_hazards() {
        let mut c = config();
        c.cave.hazards.chasm_pct = 0.0;
        c.cave.hazards.pool_pct = 0.0;
        assert!(run(&c, 6).hz.layout.hazards.is_empty());
    }

    #[test]
    fn same_seed_same_hazards() {
        let c = config();
        assert_eq!(run(&c, 7).hz.layout, run(&c, 7).hz.layout);
    }
}
