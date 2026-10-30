//! Swamp node 5, depth & border (step S4 in docs/swamp-plan.md): turns the
//! water around the islands and paths into shallow water, deep mud and deep
//! water, keeps every crossing within one breath, and closes the map edge.
//!
//! 1. **Bands** — distance from islands and mud paths (8 directions,
//!    meters), pushed ± `wobble_m` by noise: under `shallow_m` shallow
//!    water, then `mud_m` of deep mud, deep water beyond. Boardwalks are
//!    not a source: the water around them may be deep.
//! 2. **Border** — water within `border_m` (swinging by up to its full width,
//!    by a slow noise) of the map edge
//!    becomes deep. Islands, mud paths and boardwalks are never overwritten
//!    (shore gate islands and gate bridges reach the edge).
//! 3. **Breath** — cost of wading to each cell from safe ground (islands,
//!    boardwalks): 1 per meter of shallow water or mud path, `mud_cost` per
//!    meter of deep mud; deep water and thicket stop it.
//! 4. **Hummocks** — while a mud path cell is more than half the wading
//!    limit from safe ground (so some crossing through it is longer than
//!    the limit), a hummock of `hummock_m` goes on the farthest such cell.
//! 5. **Deep cut** — walkable water the breath cannot reach within the
//!    limit becomes deep water: wading there means drowning anyway.
//! 6. **Thicket** — the edge is walked in segments of `segment_min_m` to
//!    `segment_max_m`; each rolls `thicket_pct` to turn its stretch of the
//!    border band into thicket / rock.
//! 7. **Depth** — 0 on safe ground, the breath scaled 1..=254 in the
//!    water, 255 on deep water and thicket.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::grid;
use crate::generator::noise::NoiseField;
use crate::level::{CellMask, DepthLayout, Island, IslandKind, IslandLayout, PathLayout};
use crate::model::LevelConfig;

pub struct DepthBuild {
    pub layout: DepthLayout,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Terrain {
    Land,
    Path,
    Board,
    Shallow,
    Mud,
    Deep,
    Thicket,
}

/// Shore wobble noise feature size, meters.
const WOBBLE_SCALE_M: f32 = 10.0;
/// Border line noise feature size, meters.
const BORDER_SCALE_M: f32 = 25.0;
/// Most hummocks one generation adds.
const MAX_HUMMOCKS: u32 = 500;

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, islands: &mut IslandLayout, paths: &mut PathLayout) -> DepthBuild {
    let s = &config.swamp.depth;
    let (w, h, cell) = (islands.width, islands.height, islands.cell_size_m);
    let noise = NoiseField::new(rng.random::<u64>());
    let mut notes = Vec::new();
    let wobble = |c: usize, salt: f32| {
        let (x, y) = ((c % w) as f32 * cell / WOBBLE_SCALE_M, (c / w) as f32 * cell / WOBBLE_SCALE_M);
        s.wobble_m.max(0.0) * (noise.fbm(x + salt, y - salt, 3) - 0.5) * 2.0
    };

    let mut t: Vec<Terrain> = (0..w * h)
        .map(|c| {
            if islands.ids[c] > 0 {
                Terrain::Land
            } else if paths.boardwalk.cells[c] {
                Terrain::Board
            } else if paths.mud.cells[c] {
                Terrain::Path
            } else {
                Terrain::Deep
            }
        })
        .collect();

    // 1. Bands by distance from islands and mud paths.
    let sources: Vec<usize> = (0..w * h).filter(|&c| matches!(t[c], Terrain::Land | Terrain::Path)).collect();
    let mut dist = vec![f32::INFINITY; w * h];
    spread(w, h, cell, &mut dist, &sources, |_| Some(1.0));
    let (shallow, mud) = (s.shallow_m.max(0.0), s.shallow_m.max(0.0) + s.mud_m.max(0.0));
    for c in 0..w * h {
        if t[c] == Terrain::Deep {
            let d = dist[c] + wobble(c, 0.0);
            t[c] = if d < shallow {
                Terrain::Shallow
            } else if d < mud {
                Terrain::Mud
            } else {
                Terrain::Deep
            };
        }
    }

    // 2. Border band: water there turns deep.
    let mut border = vec![false; w * h];
    for c in 0..w * h {
        let (x, y) = (c % w, c / w);
        let edge = (x.min(y).min(w - 1 - x).min(h - 1 - y) as f32 + 0.5) * cell;
        let swing = noise.fbm(x as f32 * cell / BORDER_SCALE_M + 41.0, y as f32 * cell / BORDER_SCALE_M - 41.0, 3) - 0.5;
        let line = s.border_m.max(0.0) * (1.0 + 2.0 * swing);
        if edge < line && matches!(t[c], Terrain::Shallow | Terrain::Mud | Terrain::Deep) {
            t[c] = Terrain::Deep;
            border[c] = true;
        }
    }

    // 3–4. Breath from safe ground; hummocks on too-long crossings.
    let step = |t: &[Terrain], c: usize| match t[c] {
        Terrain::Path | Terrain::Shallow => Some(1.0),
        Terrain::Mud => Some(s.mud_cost.max(0.0)),
        _ => None,
    };
    let safe: Vec<usize> = (0..w * h).filter(|&c| matches!(t[c], Terrain::Land | Terrain::Board)).collect();
    let mut breath = vec![f32::INFINITY; w * h];
    {
        let snapshot = t.clone();
        spread(w, h, cell, &mut breath, &safe, |c| step(&snapshot, c));
    }
    let limit = s.wade_limit_m.max(cell);
    let r = (s.hummock_m.max(cell) / cell / 2.0).max(0.5);
    let mut hummocks = 0;
    while hummocks < MAX_HUMMOCKS {
        let worst = (0..w * h)
            .filter(|&c| t[c] == Terrain::Path && breath[c] > limit / 2.0)
            .max_by(|&a, &b| breath[a].total_cmp(&breath[b]).then(b.cmp(&a)));
        let Some(at) = worst else { break };
        let id = islands.islands.len() as u16 + 1;
        let (ax, ay) = ((at % w) as i64, (at / w) as i64);
        let ri = r.ceil() as i64;
        let mut cells = Vec::new();
        for dy in -ri..=ri {
            for dx in -ri..=ri {
                let (x, y) = (ax + dx, ay + dy);
                if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 || (dx * dx + dy * dy) as f32 > r * r {
                    continue;
                }
                let c = y as usize * w + x as usize;
                if matches!(t[c], Terrain::Path | Terrain::Shallow | Terrain::Mud) {
                    cells.push(c);
                }
            }
        }
        for &c in &cells {
            t[c] = Terrain::Land;
            islands.ids[c] = id;
            paths.mud.cells[c] = false;
        }
        islands.islands.push(Island {
            id,
            kind: IslandKind::Hummock,
            gate: None,
            center_m: [(ax as f32 + 0.5) * cell, (ay as f32 + 0.5) * cell],
            size_m: s.hummock_m.max(cell),
            cells: cells.len() as u32,
            area_m2: cells.len() as f32 * cell * cell,
        });
        hummocks += 1;
        let snapshot = t.clone();
        spread(w, h, cell, &mut breath, &cells, |c| step(&snapshot, c));
    }
    if hummocks == MAX_HUMMOCKS {
        notes.push(format!("Depth: stopped at {MAX_HUMMOCKS} hummocks — raise the wading limit"));
    }

    // 5. Deep cut beyond the limit.
    for c in 0..w * h {
        if matches!(t[c], Terrain::Shallow | Terrain::Mud) && breath[c] > limit {
            t[c] = Terrain::Deep;
        }
    }

    // 6. Thicket stretches of the border band.
    let (wm, hm) = (w as f32 * cell, h as f32 * cell);
    let perimeter = 2.0 * (wm + hm);
    let (lo, hi) = (s.segment_min_m.min(s.segment_max_m).max(cell), s.segment_max_m.max(s.segment_min_m).max(cell));
    let mut cuts = Vec::new(); // (segment end along the perimeter, thicket?)
    let mut along = 0.0;
    while along < perimeter {
        along += if hi - lo < 1e-6 { lo } else { rng.random_range(lo..=hi) };
        cuts.push((along, rng.random::<f32>() < s.thicket_pct.clamp(0.0, 100.0) / 100.0));
    }
    for c in 0..w * h {
        if !border[c] || t[c] != Terrain::Deep {
            continue;
        }
        let (x, y) = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
        // Position of the nearest edge point, clockwise from the top-left.
        let (top, right, bottom, left) = (y, wm - x, hm - y, x);
        let m = top.min(right).min(bottom).min(left);
        let pos = if m == top {
            x
        } else if m == right {
            wm + y
        } else if m == bottom {
            wm + hm + (wm - x)
        } else {
            2.0 * wm + hm + (hm - y)
        };
        let k = cuts.partition_point(|&(end, _)| end <= pos).min(cuts.len() - 1);
        if cuts[k].1 {
            t[c] = Terrain::Thicket;
        }
    }

    // 7. Depth, masks, stats.
    let depth: Vec<u8> = (0..w * h)
        .map(|c| match t[c] {
            Terrain::Land | Terrain::Board => 0,
            Terrain::Path | Terrain::Shallow | Terrain::Mud => (1.0 + breath[c] / limit * 253.0).clamp(1.0, 254.0) as u8,
            Terrain::Deep | Terrain::Thicket => 255,
        })
        .collect();
    let mask = |want: Terrain| CellMask { width: w, height: h, cell_size_m: cell, cells: t.iter().map(|&x| x == want).collect() };
    let pct = |m: &CellMask| m.count_true() as f32 / (w * h) as f32 * 100.0;
    let walkable = CellMask {
        width: w,
        height: h,
        cell_size_m: cell,
        cells: t.iter().map(|&x| !matches!(x, Terrain::Deep | Terrain::Thicket)).collect(),
    };
    let (_, pieces) = grid::components(&walkable);
    if pieces > 1 {
        notes.push(format!("Depth: the walkable ground is in {pieces} pieces"));
    }
    let (shallow, mud, deep, thicket) = (mask(Terrain::Shallow), mask(Terrain::Mud), mask(Terrain::Deep), mask(Terrain::Thicket));
    let layout = DepthLayout {
        reach_m: if s.return_trip { limit / 2.0 } else { limit },
        hummocks,
        walkable_pct: pct(&walkable),
        shallow_pct: pct(&shallow),
        mud_pct: pct(&mud),
        deep_pct: pct(&deep),
        thicket_pct: pct(&thicket),
        shallow,
        mud,
        deep,
        thicket,
        depth,
    };
    DepthBuild { layout, notes }
}

/// Dijkstra over the grid, 8 directions: lowers `dist` from `sources` (set
/// to 0). `factor(c)` is the cost per meter of entering cell `c`, `None` =
/// blocked. Existing values stay where they are lower, so it can add new
/// sources to an earlier result.
fn spread(w: usize, h: usize, cell: f32, dist: &mut [f32], sources: &[usize], factor: impl Fn(usize) -> Option<f32>) {
    // Fixed point (1/1024 m) keeps the heap order exact and deterministic.
    let mut heap = BinaryHeap::new();
    for &c in sources {
        dist[c] = 0.0;
        heap.push(Reverse((0u64, c)));
    }
    while let Some(Reverse((d, c))) = heap.pop() {
        let dm = d as f32 / 1024.0;
        if dm > dist[c] + 1e-3 {
            continue;
        }
        let (cx, cy) = ((c % w) as i64, (c / w) as i64);
        for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
            let (nx, ny) = (cx + dx, cy + dy);
            if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                continue;
            }
            let n = ny as usize * w + nx as usize;
            let Some(f) = factor(n) else { continue };
            let len = if dx != 0 && dy != 0 { std::f32::consts::SQRT_2 } else { 1.0 } * cell;
            let nd = dm + len * f;
            if nd + 1e-4 < dist[n] {
                dist[n] = nd;
                heap.push(Reverse(((nd * 1024.0) as u64, n)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::generator::swamp::{gate_anchors, islands, paths};
    use crate::model::{GateSpec, GateStyle, MapKind};

    fn config() -> LevelConfig {
        let mut c = LevelConfig { kind: MapKind::Swamp, ..LevelConfig::default() };
        c.map.width_m = 140.0;
        c.map.height_m = 120.0;
        c.cell_size_m = 1.0;
        c.gates.gates = vec![
            GateSpec { direction_deg: 90.0, style: GateStyle::Shore },
            GateSpec { direction_deg: 270.0, style: GateStyle::Bridge },
        ];
        c
    }

    struct All {
        islands: IslandLayout,
        paths: PathLayout,
        depth: DepthBuild,
    }

    fn build_all(c: &LevelConfig, seed: u64) -> All {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let anchors = gate_anchors::build(c, &mut rng).anchors;
        let mut isl = islands::build(c, &mut rng, &anchors).layout;
        let mut p = paths::build(c, &mut rng, &anchors, &isl).layout;
        let depth = build(c, &mut rng, &mut isl, &mut p);
        All { islands: isl, paths: p, depth }
    }

    #[test]
    fn every_cell_is_exactly_one_terrain_and_walkable_is_one_piece() {
        let c = config();
        for seed in 0..3 {
            let a = build_all(&c, seed);
            let d = &a.depth.layout;
            for k in 0..d.depth.len() {
                let kinds = [
                    a.islands.ids[k] > 0,
                    a.paths.mud.cells[k],
                    a.paths.boardwalk.cells[k],
                    d.shallow.cells[k],
                    d.mud.cells[k],
                    d.deep.cells[k],
                    d.thicket.cells[k],
                ];
                assert_eq!(kinds.iter().filter(|&&x| x).count(), 1, "seed {seed} cell {k}: {kinds:?}");
            }
            assert!(a.depth.notes.is_empty(), "seed {seed}: {:?}", a.depth.notes);
            assert!(d.deep_pct > 10.0 && d.shallow_pct > 1.0 && d.mud_pct > 1.0, "{} {} {}", d.deep_pct, d.shallow_pct, d.mud_pct);
        }
    }

    #[test]
    fn crossings_fit_in_one_breath_and_no_water_is_past_the_limit() {
        let mut c = config();
        c.swamp.depth.wade_limit_m = 10.0;
        let a = build_all(&c, 4);
        let d = &a.depth.layout;
        assert!(d.hummocks > 0, "a 10 m limit needs hummocks");
        for k in 0..d.depth.len() {
            if a.paths.mud.cells[k] {
                assert!(d.depth[k] <= 1 + 253 / 2 + 1, "path cell {k} at depth {}", d.depth[k]);
            }
            if d.shallow.cells[k] || d.mud.cells[k] {
                assert!(d.depth[k] <= 254);
            }
        }
        let hummocks = a.islands.islands.iter().filter(|i| i.kind == IslandKind::Hummock).count();
        assert_eq!(hummocks as u32, d.hummocks);
        assert!(a.depth.notes.is_empty(), "{:?}", a.depth.notes);
        let mut generous = config();
        generous.swamp.depth.wade_limit_m = 500.0;
        assert_eq!(build_all(&generous, 4).depth.layout.hummocks, 0);
    }

    #[test]
    fn border_is_deep_or_thicket_except_gate_ground() {
        let mut c = config();
        c.swamp.depth.thicket_pct = 0.0;
        let a = build_all(&c, 2);
        let d = &a.depth.layout;
        let (w, h) = (d.deep.width, d.deep.height);
        let edge: Vec<usize> = (0..w * h).filter(|&k| k % w == 0 || k / w == 0 || k % w == w - 1 || k / w == h - 1).collect();
        assert_eq!(d.thicket.count_true(), 0);
        for &k in &edge {
            let gate_ground = a.islands.ids[k] > 0 || a.paths.boardwalk.cells[k] || a.paths.mud.cells[k];
            assert!(d.deep.cells[k] || gate_ground, "edge cell {k} is walkable water");
        }
        c.swamp.depth.thicket_pct = 100.0;
        let a = build_all(&c, 2);
        let d = &a.depth.layout;
        assert!(edge.iter().all(|&k| !d.deep.cells[k]), "100% thicket leaves no deep edge cell");
        assert!(d.thicket.count_true() > 0);
    }

    #[test]
    fn return_trip_halves_the_reach() {
        let mut c = config();
        assert_eq!(build_all(&c, 1).depth.layout.reach_m, 10.0);
        c.swamp.depth.return_trip = false;
        assert_eq!(build_all(&c, 1).depth.layout.reach_m, 20.0);
    }

    #[test]
    fn same_seed_same_depth() {
        let c = config();
        assert_eq!(build_all(&c, 6).depth.layout, build_all(&c, 6).depth.layout);
    }
}
