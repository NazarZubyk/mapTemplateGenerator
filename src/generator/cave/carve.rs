//! Cave node 5, carve & smooth (step C4 in docs/cave-plan.md): the final
//! floor. Natural parts get ragged, organic walls; hand-made parts stay as
//! they were dug.
//!
//! 1. **Carve** — floor = chambers + tunnels.
//! 2. **Roughen** — in natural zones only: a cell is floor when its signed
//!    distance to the wall (inside +, outside −) plus noise (`roughness_pct`,
//!    up to ±`ROUGH_M`, `ROUGH_SCALE_M` features) is positive — the wall
//!    is pushed in and out.
//! 3. **Smooth** — `smooth_passes` of the 4-5 cellular automaton on natural
//!    cells (rock with ≥ 5 rock neighbors of 8, floor with ≤ 3; outside the
//!    map counts as rock). Hand-made cells never change; tunnel centerlines
//!    (natural tunnels) and gate points stay floor, so no tunnel closes.
//! 4. **Clean up** — a floor piece cut off from the main floor is joined
//!    back if it matters (hand-made floor, or `POCKET_M2` and up): the
//!    shortest carved path to the main floor is reopened. Smaller pockets
//!    are dropped. The floor is one piece and keeps every chamber. Enclosed rock
//!    specks under `min_rock_m2` are dug out. The outermost cells stay as
//!    they were (only gates open the edge).
//! 5. **Wall band** — rock within `wall_band_m` of the floor.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::cave::tunnels::distance_field;
use crate::generator::grid;
use crate::generator::noise::NoiseField;
use crate::generator::swamp::paths::polyline_cells;
use crate::level::{CarveLayout, CaveStyle, CellMask, ChamberLayout, StyleLayout, TunnelLayout};
use crate::model::LevelConfig;

pub struct CarveBuild {
    pub layout: CarveLayout,
    pub notes: Vec<String>,
}

/// Wall push at 100% roughness, meters (± around the carved wall).
const ROUGH_M: f32 = 2.5;
/// Roughness noise feature size, meters.
const ROUGH_SCALE_M: f32 = 2.5;
/// Cut-off floor pieces from this size, m², are joined back (smaller ones
/// are dropped).
const POCKET_M2: f32 = 6.0;
/// fBm rarely leaves 0.5 ± 0.25; stretched so `ROUGH_M` is about the swing.
const ROUGH_GAIN: f32 = 2.0;

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, style: &StyleLayout, chambers: &ChamberLayout, tunnels: &TunnelLayout) -> CarveBuild {
    let s = &config.cave.carve;
    let (w, h, cell) = (chambers.width, chambers.height, chambers.cell_size_m);
    let n = w * h;
    let notes = Vec::new();
    let noise = NoiseField::new(rng.random::<u64>());
    let natural: Vec<bool> = (0..n).map(|c| style.style_at(c) == CaveStyle::Natural).collect();
    let edge = |c: usize| c.is_multiple_of(w) || c / w == 0 || c % w == w - 1 || c / w == h - 1;

    // 1. Carve.
    let original: Vec<bool> = (0..n).map(|c| chambers.ids[c] > 0 || tunnels.mask.cells[c]).collect();
    let mut floor = original.clone();

    // Cells that must stay floor: natural tunnel centerlines (thickened by
    // one cell) and the gate points.
    let mut core = vec![false; n];
    for t in &tunnels.tunnels {
        if t.style != CaveStyle::Natural {
            continue;
        }
        for c in polyline_cells(&t.path_m, w, h, cell) {
            let (x, y) = ((c % w) as i64, (c / w) as i64);
            for (dx, dy) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 {
                    let k = ny as usize * w + nx as usize;
                    core[k] |= original[k];
                }
            }
        }
    }
    for g in &tunnels.gates {
        let c = ((g.point_m[1] / cell) as usize).min(h - 1) * w + ((g.point_m[0] / cell) as usize).min(w - 1);
        core[c] = original[c];
    }

    // 2. Roughen (natural zones).
    let rough = (s.roughness_pct / 100.0).clamp(0.0, 1.0);
    if rough > 0.0 {
        let inside = distance_field(w, h, |c| !original[c]);
        let outside = distance_field(w, h, |c| original[c]);
        for c in 0..n {
            if !natural[c] || edge(c) {
                continue;
            }
            // Signed distance to the wall in meters, cell centers.
            let d = if original[c] { inside[c] - 0.5 } else { 0.5 - outside[c] } * cell;
            if d.abs() > ROUGH_M * rough + cell {
                continue;
            }
            let (x, y) = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
            let push = rough * ROUGH_M * ((noise.fbm(x / ROUGH_SCALE_M, y / ROUGH_SCALE_M, 3) - 0.5) * 2.0 * ROUGH_GAIN).clamp(-1.0, 1.0);
            floor[c] = d + push > 0.0;
        }
    }

    // 3. Smooth (natural cells, core stays floor).
    for _ in 0..s.smooth_passes.min(12) {
        let prev = floor.clone();
        for c in 0..n {
            if !natural[c] || edge(c) || core[c] {
                continue;
            }
            let (x, y) = ((c % w) as i64, (c / w) as i64);
            let mut rock = 0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 || !prev[ny as usize * w + nx as usize] {
                        rock += 1;
                    }
                }
            }
            if rock >= 5 {
                floor[c] = false;
            } else if rock <= 3 {
                floor[c] = true;
            }
        }
    }
    for c in 0..n {
        floor[c] |= core[c];
    }

    // 4. Clean up: keep the main floor. A cut-off piece that matters (hand-
    // made floor, or bigger than a pocket) is joined back along the carved
    // floor: the shortest carved path to the main floor is reopened.
    let mut mask = CellMask { width: w, height: h, cell_size_m: cell, cells: floor };
    let (labels, count) = grid::components(&mask);
    let mut size = vec![0u32; count as usize + 1];
    let mut matters = vec![false; count as usize + 1];
    for (c, &l) in labels.iter().enumerate() {
        size[l as usize] += 1;
        matters[l as usize] |= original[c] && !natural[c];
    }
    let main = (1..=count).max_by_key(|&l| (size[l as usize], std::cmp::Reverse(l))).unwrap_or(0);
    let pocket = (POCKET_M2 / (cell * cell)).round() as u32;
    for l in 1..=count {
        matters[l as usize] |= size[l as usize] >= pocket;
    }
    let mut rejoined = 0;
    if count > 1 {
        // BFS over the carved floor from the main piece.
        let mut parent = vec![usize::MAX; n];
        let mut queue = std::collections::VecDeque::new();
        for c in 0..n {
            if labels[c] == main {
                parent[c] = c;
                queue.push_back(c);
            }
        }
        while let Some(c) = queue.pop_front() {
            let (x, y) = (c % w, c / w);
            for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                    continue;
                }
                let nb = ny as usize * w + nx as usize;
                if parent[nb] == usize::MAX && (original[nb] || mask.cells[nb]) {
                    parent[nb] = c;
                    queue.push_back(nb);
                }
            }
        }
        let mut done = vec![false; count as usize + 1];
        for c in 0..n {
            let l = labels[c] as usize;
            if l == 0 || l == main as usize || done[l] || !matters[l] || parent[c] == usize::MAX {
                continue;
            }
            done[l] = true;
            rejoined += 1;
            // Reopen the path, one cell thicker where it was carved.
            let mut at = c;
            while labels[at] != main {
                let (x, y) = ((at % w) as i64, (at / w) as i64);
                for (dx, dy) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 {
                        let k = ny as usize * w + nx as usize;
                        if dx == 0 && dy == 0 || original[k] {
                            mask.cells[k] = true;
                        }
                    }
                }
                at = parent[at];
            }
        }
    }
    // Whatever is still apart is a small pocket: drop it.
    let (labels, count) = grid::components(&mask);
    let mut dropped = 0;
    if count > 1 {
        let mut size = vec![0u32; count as usize + 1];
        for &l in &labels {
            size[l as usize] += 1;
        }
        let main = (1..=count).max_by_key(|&l| (size[l as usize], std::cmp::Reverse(l))).unwrap_or(0);
        for (cell, &l) in mask.cells.iter_mut().zip(&labels) {
            if l != 0 && l != main {
                *cell = false;
                dropped += 1;
            }
        }
    }
    // Enclosed rock specks.
    let min_rock = (s.min_rock_m2.max(0.0) / (cell * cell)).round() as u32;
    if min_rock > 0 {
        let rock = CellMask { cells: mask.cells.iter().map(|&f| !f).collect(), ..mask.clone() };
        let (labels, count) = grid::components(&rock);
        let mut size = vec![0u32; count as usize + 1];
        let mut touches_edge = vec![false; count as usize + 1];
        for (c, &l) in labels.iter().enumerate() {
            size[l as usize] += 1;
            touches_edge[l as usize] |= edge(c);
        }
        for (c, &l) in labels.iter().enumerate() {
            if l > 0 && !touches_edge[l as usize] && size[l as usize] < min_rock {
                mask.cells[c] = true;
            }
        }
    }

    // 5. Wall band.
    let band = (s.wall_band_m.max(0.0) / cell).max(if s.wall_band_m > 0.0 { 1.0 } else { 0.0 });
    let to_floor = distance_field(w, h, |c| mask.cells[c]);
    let wall = CellMask {
        width: w,
        height: h,
        cell_size_m: cell,
        cells: (0..n).map(|c| !mask.cells[c] && to_floor[c] <= band + 1e-3).collect(),
    };

    let floor_n = mask.count_true();
    let natural_n = (0..n).filter(|&c| mask.cells[c] && natural[c]).count();
    CarveBuild {
        layout: CarveLayout {
            floor_pct: floor_n as f32 / n as f32 * 100.0,
            natural_floor_pct: natural_n as f32 / floor_n.max(1) as f32 * 100.0,
            dropped_cells: dropped,
            rejoined,
            floor: mask,
            wall,
        },
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::generator::cave::{chambers, style, tunnels};
    use crate::model::MapKind;

    fn config() -> LevelConfig {
        let mut c = LevelConfig { kind: MapKind::Cave, ..LevelConfig::default() };
        c.map.width_m = 160.0;
        c.map.height_m = 140.0;
        c.cell_size_m = 0.5;
        c
    }

    struct All {
        style: StyleLayout,
        chambers: ChamberLayout,
        tunnels: TunnelLayout,
        carve: CarveBuild,
    }

    fn run(c: &LevelConfig, seed: u64) -> All {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let st = style::build(c, &mut rng).layout;
        let ch = chambers::build(c, &mut rng, &st).layout;
        let t = tunnels::build(c, &mut rng, &st, &ch).layout;
        let carve = build(c, &mut rng, &st, &ch, &t);
        All { style: st, chambers: ch, tunnels: t, carve }
    }

    #[test]
    fn floor_is_one_piece_with_every_chamber_and_gate() {
        for (rough, passes, seed) in [(50.0, 4, 0), (100.0, 6, 1), (100.0, 0, 2), (20.0, 2, 3)] {
            let mut c = config();
            c.cave.carve.roughness_pct = rough;
            c.cave.carve.smooth_passes = passes;
            let a = run(&c, seed);
            let f = &a.carve.layout.floor;
            assert!(grid::is_connected(f), "seed {seed}");
            for ch in &a.chambers.chambers {
                assert!((0..f.cells.len()).any(|k| a.chambers.ids[k] == ch.id && f.cells[k]), "chamber {} lost", ch.id);
            }
            for g in &a.tunnels.gates {
                let k = (g.point_m[1] / 0.5) as usize * f.width + (g.point_m[0] / 0.5) as usize;
                assert!(f.cells[k], "gate {} closed", g.id);
            }
        }
    }

    #[test]
    fn hand_made_cells_stay_as_dug() {
        let a = run(&config(), 4);
        let f = &a.carve.layout.floor;
        for k in 0..f.cells.len() {
            if a.style.style_at(k) == CaveStyle::HandMade {
                let dug = a.chambers.ids[k] > 0 || a.tunnels.mask.cells[k];
                // Only rock specks may be dug out.
                assert!(f.cells[k] || !dug, "hand-made floor lost at {k}");
            }
        }
    }

    #[test]
    fn roughness_makes_natural_walls_ragged() {
        let edge_cells = |a: &All| {
            let f = &a.carve.layout.floor;
            (0..f.cells.len())
                .filter(|&k| {
                    f.cells[k] && a.style.style_at(k) == CaveStyle::Natural && {
                        let (x, y) = (k % f.width, k / f.width);
                        x > 0 && y > 0 && x + 1 < f.width && y + 1 < f.height && [k - 1, k + 1, k - f.width, k + f.width].iter().any(|&n| !f.cells[n])
                    }
                })
                .count() as f32
                / f.count_true() as f32
        };
        let mut c = config();
        c.cave.carve.roughness_pct = 0.0;
        c.cave.carve.smooth_passes = 0;
        let smooth = edge_cells(&run(&c, 5));
        c.cave.carve.roughness_pct = 100.0;
        c.cave.carve.smooth_passes = 2;
        let rough = edge_cells(&run(&c, 5));
        assert!(rough > smooth * 1.1, "{smooth} vs {rough}");
    }

    #[test]
    fn nothing_changes_without_roughness_and_smoothing() {
        let mut c = config();
        c.cave.carve.roughness_pct = 0.0;
        c.cave.carve.smooth_passes = 0;
        c.cave.carve.min_rock_m2 = 0.0;
        let a = run(&c, 6);
        for k in 0..a.carve.layout.floor.cells.len() {
            assert_eq!(a.carve.layout.floor.cells[k], a.chambers.ids[k] > 0 || a.tunnels.mask.cells[k]);
        }
    }

    #[test]
    fn wall_band_hugs_the_floor() {
        let a = run(&config(), 7);
        let l = &a.carve.layout;
        assert!(l.wall.count_true() > 0);
        for k in 0..l.wall.cells.len() {
            assert!(!(l.wall.cells[k] && l.floor.cells[k]));
        }
    }

    #[test]
    fn same_seed_same_floor() {
        let c = config();
        assert_eq!(run(&c, 8).carve.layout, run(&c, 8).carve.layout);
    }
}
