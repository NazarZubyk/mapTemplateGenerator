//! Swamp node 3, islands (step S2 in docs/swamp-plan.md): the firm ground.
//! Everything else on a swamp map is water or mud; later nodes link the
//! islands with paths and fill the water around them by depth.
//!
//! 1. **Centers** — gate islands at their anchors first (from the gate
//!    anchors node), then `hub_count` hubs, then the small islands (a count
//!    or a density). Each one takes the best of 32 random spots: the one
//!    farthest from the islands placed so far (edge to edge, plus `gap_m`),
//!    inside the `border_m` margin. No spot clear → the island shrinks
//!    (×0.8, up to 3 times), then it is skipped with a note.
//! 2. **Shape** — an ellipse of the island's area (random axis, stretch up
//!    to 2× at 100% `irregularity_pct`), sampled at domain-warped
//!    coordinates and roughened by fBm, then cut to its target area: the
//!    best-scoring cells of its window are kept.
//! 3. **Gap** — two islands never come closer than `gap_m`: of two cells of
//!    different islands within the gap, the one with the weaker score goes.
//!    Hub and small islands never enter the border margin.
//! 4. **Tidy** — each island keeps its largest piece and gets its enclosed
//!    holes filled. An island left with under a fifth of its target is
//!    dropped (note).
//!
//! The islands are NOT connected yet — the paths node does that.

use std::f32::consts::PI;

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::grid;
use crate::generator::noise::NoiseField;
use crate::level::{CellMask, GateAnchor, Island, IslandKind, IslandLayout};
use crate::model::{LevelConfig, NodeAmount};

pub struct IslandBuild {
    pub layout: IslandLayout,
    pub notes: Vec<String>,
}

/// Most small islands, whatever the density says.
const MAX_SMALL: u32 = 400;
const CANDIDATES: usize = 32;

/// One island before rasterizing.
struct Seed {
    kind: IslandKind,
    gate: Option<u32>,
    center: (f32, f32),
    /// Radius of the target area, meters.
    r: f32,
    angle: f32,
    stretch: f32,
    /// Hub and small islands stay out of the border margin.
    inland: bool,
}

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, anchors: &[GateAnchor]) -> IslandBuild {
    let s = &config.swamp.islands;
    let cell = config.cell_size_m.max(0.05);
    let (w, h) = (
        ((config.map.width_m / cell).round() as usize).max(1),
        ((config.map.height_m / cell).round() as usize).max(1),
    );
    let (wm, hm) = (w as f32 * cell, h as f32 * cell);
    let irr = (s.irregularity_pct / 100.0).clamp(0.0, 1.0);
    let mut notes = Vec::new();
    let noise = NoiseField::new(rng.random::<u64>());

    // 1. Centers.
    let shape = |rng: &mut ChaCha8Rng| (rng.random_range(0.0..PI), 1.0 + irr * rng.random_range(0.0..=1.0f32));
    let mut seeds: Vec<Seed> = Vec::new();
    for a in anchors {
        let (angle, stretch) = shape(rng);
        seeds.push(Seed {
            kind: IslandKind::Gate,
            gate: Some(a.id),
            center: (a.land_m[0], a.land_m[1]),
            r: a.island_m / 2.0,
            angle,
            stretch,
            inland: false,
        });
    }
    let small = match s.small_amount {
        NodeAmount::Count => s.small_count,
        NodeAmount::Density => (s.small_per_1000m2.max(0.0) * wm * hm / 1000.0).round() as u32,
    }
    .min(MAX_SMALL);
    let wanted = (0..s.hub_count)
        .map(|_| (IslandKind::Hub, s.hub_min_m, s.hub_max_m))
        .chain((0..small).map(|_| (IslandKind::Small, s.small_min_m, s.small_max_m)));
    let mut skipped = 0;
    for (kind, lo, hi) in wanted {
        let (lo, hi) = (lo.min(hi).max(cell), lo.max(hi).max(cell));
        let mut r = if hi - lo < 1e-6 { lo } else { rng.random_range(lo..=hi) } / 2.0;
        let (angle, stretch) = shape(rng);
        let mut placed = None;
        for _ in 0..4 {
            let margin = s.border_m.max(0.0) + r;
            let best = (0..CANDIDATES)
                .map(|_| {
                    let mut span = |size: f32| {
                        if size - 2.0 * margin > 0.0 { rng.random_range(margin..size - margin) } else { size / 2.0 }
                    };
                    let p = (span(wm), span(hm));
                    let clear = seeds
                        .iter()
                        .map(|o| (p.0 - o.center.0).hypot(p.1 - o.center.1) - r - o.r - s.gap_m)
                        .fold(f32::INFINITY, f32::min);
                    (p, clear)
                })
                .max_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((p, clear)) = best
                && clear >= 0.0
                && wm >= 2.0 * margin
                && hm >= 2.0 * margin
            {
                placed = Some(p);
                break;
            }
            r *= 0.8;
        }
        match placed {
            Some(center) => seeds.push(Seed { kind, gate: None, center, r, angle, stretch, inland: true }),
            None => skipped += 1,
        }
    }
    if skipped > 0 {
        notes.push(format!("Islands: {skipped} did not fit — fewer or smaller islands, a smaller gap or border"));
    }

    // 2. Shape: per island, its best cells by score; a contested cell goes
    // to the higher score.
    let border = (s.border_m.max(0.0) / cell).round() as usize;
    let mut owner = vec![0usize; w * h]; // seed index + 1
    let mut score = vec![f32::NEG_INFINITY; w * h];
    for (i, seed) in seeds.iter().enumerate() {
        let a = seed.r * seed.stretch.sqrt();
        let b = seed.r / seed.stretch.sqrt();
        let reach = a * 1.6 + cell;
        let (sin, cos) = seed.angle.sin_cos();
        let feature = (seed.r * 0.7).max(2.0);
        let salt = i as f32 * 37.13;
        let x0 = ((seed.center.0 - reach) / cell).floor().max(0.0) as usize;
        let x1 = (((seed.center.0 + reach) / cell).ceil() as usize).min(w);
        let y0 = ((seed.center.1 - reach) / cell).floor().max(0.0) as usize;
        let y1 = (((seed.center.1 + reach) / cell).ceil() as usize).min(h);
        let mut cells: Vec<(f32, usize)> = Vec::new();
        for y in y0..y1 {
            for x in x0..x1 {
                if seed.inland && (x < border || y < border || x + border >= w || y + border >= h) {
                    continue;
                }
                let (px, py) = ((x as f32 + 0.5) * cell, (y as f32 + 0.5) * cell);
                let (nx, ny) = (px / feature + salt, py / feature - salt);
                let warp = irr * 0.5 * seed.r;
                let wx = px + warp * (noise.fbm(nx + 5.2, ny + 1.3, 3) - 0.5) * 2.0;
                let wy = py + warp * (noise.fbm(nx - 3.7, ny + 8.1, 3) - 0.5) * 2.0;
                let (dx, dy) = (wx - seed.center.0, wy - seed.center.1);
                let (u, v) = (dx * cos + dy * sin, -dx * sin + dy * cos);
                let d2 = (u / a).powi(2) + (v / b).powi(2);
                let rough = irr * 0.8 * (noise.fbm(nx * 2.0, ny * 2.0, 4) - 0.5) * 2.0;
                cells.push((1.0 - d2 + rough, y * w + x));
            }
        }
        cells.sort_by(|p, q| q.0.total_cmp(&p.0).then(p.1.cmp(&q.1)));
        let target = (PI * seed.r * seed.r / (cell * cell)).round().max(1.0) as usize;
        for &(sc, c) in cells.iter().take(target) {
            if sc > score[c] {
                score[c] = sc;
                owner[c] = i + 1;
            }
        }
    }

    // 3. Gap: the weaker cell of every too-close pair goes.
    let g = (s.gap_m.max(0.0) / cell).ceil() as i64;
    let gm = s.gap_m.max(0.0) / cell;
    let mut drop = vec![false; w * h];
    if g > 0 {
        for c in 0..w * h {
            let i = owner[c];
            if i == 0 {
                continue;
            }
            let (cx, cy) = ((c % w) as i64, (c / w) as i64);
            'scan: for dy in -g..=g {
                for dx in -g..=g {
                    let (nx, ny) = (cx + dx, cy + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 || ((dx * dx + dy * dy) as f32) > gm * gm {
                        continue;
                    }
                    let n = ny as usize * w + nx as usize;
                    let j = owner[n];
                    if j != 0 && j != i && (score[n] > score[c] || (score[n] == score[c] && j < i)) {
                        drop[c] = true;
                        break 'scan;
                    }
                }
            }
        }
    }
    for c in 0..w * h {
        if drop[c] {
            owner[c] = 0;
        }
    }

    // 4. Tidy, per island inside its bounding box: largest piece, holes.
    let mut ids = vec![0u16; w * h];
    let mut islands = Vec::new();
    let mut dropped = 0;
    for (i, seed) in seeds.iter().enumerate() {
        let mine: Vec<usize> = (0..w * h).filter(|&c| owner[c] == i + 1).collect();
        let target = PI * seed.r * seed.r / (cell * cell);
        let Some(cells) = tidy(&mine, &owner, i + 1, w, h, cell) else {
            dropped += 1;
            continue;
        };
        if (cells.len() as f32) < (target * 0.2).max(4.0) {
            dropped += 1;
            continue;
        }
        let id = islands.len() as u16 + 1;
        let (mut sx, mut sy) = (0.0f64, 0.0f64);
        for &c in &cells {
            ids[c] = id;
            sx += (c % w) as f64 + 0.5;
            sy += (c / w) as f64 + 0.5;
        }
        let n = cells.len() as f64;
        islands.push(Island {
            id,
            kind: seed.kind,
            gate: seed.gate,
            center_m: [(sx / n) as f32 * cell, (sy / n) as f32 * cell],
            size_m: seed.r * 2.0,
            cells: cells.len() as u32,
            area_m2: cells.len() as f32 * cell * cell,
        });
    }
    if dropped > 0 {
        notes.push(format!("Islands: {dropped} squeezed to almost nothing by their neighbors and dropped"));
    }
    let land = ids.iter().filter(|&&i| i > 0).count();
    IslandBuild {
        layout: IslandLayout {
            width: w,
            height: h,
            cell_size_m: cell,
            ids,
            islands,
            requested: s.hub_count + small,
            land_pct: land as f32 / (w * h) as f32 * 100.0,
        },
        notes,
    }
}

/// The island's largest 4-connected piece with its enclosed holes filled
/// (only cells no other island owns), worked inside its bounding box.
fn tidy(mine: &[usize], owner: &[usize], me: usize, w: usize, h: usize, cell: f32) -> Option<Vec<usize>> {
    let x0 = mine.iter().map(|&c| c % w).min()?.saturating_sub(1);
    let y0 = mine.iter().map(|&c| c / w).min()?.saturating_sub(1);
    let x1 = (mine.iter().map(|&c| c % w).max()? + 2).min(w);
    let y1 = (mine.iter().map(|&c| c / w).max()? + 2).min(h);
    let (bw, bh) = (x1 - x0, y1 - y0);
    let mut local = CellMask::new(bw, bh, cell);
    for &c in mine {
        local.set(c % w - x0, c / w - y0, true);
    }
    let (labels, count) = grid::components(&local);
    let mut sizes = vec![0u32; count as usize + 1];
    for &l in &labels {
        sizes[l as usize] += 1;
    }
    let keep = (1..=count as usize).max_by_key(|&l| (sizes[l], std::cmp::Reverse(l)))? as u32;
    for (cell, &l) in local.cells.iter_mut().zip(&labels) {
        *cell = l == keep;
    }
    let before = local.cells.clone();
    grid::fill_holes(&mut local);
    let mut out = Vec::new();
    for ly in 0..bh {
        for lx in 0..bw {
            let k = ly * bw + lx;
            let c = (y0 + ly) * w + x0 + lx;
            if before[k] || (local.cells[k] && owner[c] == 0) || (local.cells[k] && owner[c] == me) {
                out.push(c);
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::generator::swamp::gate_anchors;
    use crate::model::{GateSpec, GateStyle, MapKind};

    fn config() -> LevelConfig {
        let mut c = LevelConfig { kind: MapKind::Swamp, ..LevelConfig::default() };
        c.map.width_m = 100.0;
        c.map.height_m = 80.0;
        c.gates.gates = vec![
            GateSpec { direction_deg: 90.0, style: GateStyle::Shore },
            GateSpec { direction_deg: 270.0, style: GateStyle::Bridge },
        ];
        c.swamp.islands.small_amount = NodeAmount::Count;
        c.swamp.islands.small_count = 8;
        c
    }

    fn islands(c: &LevelConfig, seed: u64) -> IslandBuild {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let anchors = gate_anchors::build(c, &mut rng).anchors;
        build(c, &mut rng, &anchors)
    }

    fn cell_xy(l: &IslandLayout, c: usize) -> (i64, i64) {
        ((c % l.width) as i64, (c / l.width) as i64)
    }

    #[test]
    fn requested_islands_appear_by_kind() {
        let c = config();
        for seed in 0..5 {
            let b = islands(&c, seed);
            let count = |k| b.layout.islands.iter().filter(|i| i.kind == k).count();
            assert_eq!(count(IslandKind::Gate), 2, "seed {seed}: {:?}", b.notes);
            assert_eq!(count(IslandKind::Hub), 3, "seed {seed}: {:?}", b.notes);
            assert_eq!(count(IslandKind::Small), 8, "seed {seed}: {:?}", b.notes);
            assert!(b.notes.is_empty(), "seed {seed}: {:?}", b.notes);
        }
    }

    #[test]
    fn islands_are_single_pieces_with_area_near_target() {
        let c = config();
        let b = islands(&c, 3);
        let l = &b.layout;
        for isl in &l.islands {
            let mut m = CellMask::new(l.width, l.height, l.cell_size_m);
            for (k, &id) in l.ids.iter().enumerate() {
                m.cells[k] = id == isl.id;
            }
            assert_eq!(grid::components(&m).1, 1, "island {} is in pieces", isl.id);
            let target = PI * (isl.size_m / 2.0).powi(2);
            assert!(isl.area_m2 > target * 0.4 && isl.area_m2 < target * 1.3, "{isl:?} target {target}");
            assert_eq!(isl.cells as usize, m.count_true());
        }
    }

    #[test]
    fn islands_keep_the_gap() {
        let c = config();
        let l = islands(&c, 1).layout;
        let g = (c.swamp.islands.gap_m / l.cell_size_m) as i64;
        for a in 0..l.ids.len() {
            if l.ids[a] == 0 {
                continue;
            }
            let (ax, ay) = cell_xy(&l, a);
            for dy in -g..=g {
                for dx in -g..=g {
                    let (x, y) = (ax + dx, ay + dy);
                    if x < 0 || y < 0 || x >= l.width as i64 || y >= l.height as i64 || (dx * dx + dy * dy) > (g - 1) * (g - 1) {
                        continue;
                    }
                    let id = l.ids[y as usize * l.width + x as usize];
                    assert!(id == 0 || id == l.ids[a], "islands {} and {id} closer than the gap", l.ids[a]);
                }
            }
        }
    }

    #[test]
    fn only_shore_gate_islands_touch_the_border() {
        let c = config();
        let l = islands(&c, 2).layout;
        let border = (c.swamp.islands.border_m / l.cell_size_m).round() as usize;
        let mut shore_touches = false;
        for (k, &id) in l.ids.iter().enumerate() {
            if id == 0 {
                continue;
            }
            let (x, y) = (k % l.width, k / l.width);
            let isl = &l.islands[id as usize - 1];
            if x < border || y < border || x + border >= l.width || y + border >= l.height {
                assert_eq!(isl.kind, IslandKind::Gate, "{isl:?} in the border margin");
            }
            if (y == 0) && isl.gate == Some(1) {
                shore_touches = true;
            }
        }
        assert!(shore_touches, "the north shore gate island must reach the edge");
    }

    #[test]
    fn density_scales_small_islands_and_crowding_gives_a_note() {
        let mut c = config();
        c.swamp.islands.small_amount = NodeAmount::Density;
        c.swamp.islands.small_per_1000m2 = 1.0; // 8000 m² → 8
        assert_eq!(islands(&c, 4).layout.requested, 3 + 8);
        let mut crowded = config();
        crowded.swamp.islands.hub_count = 40;
        let b = islands(&crowded, 4);
        assert!(b.layout.islands.iter().filter(|i| i.kind == IslandKind::Hub).count() < 40);
        assert!(!b.notes.is_empty());
    }

    #[test]
    fn defaults_fit_the_default_map() {
        let c = LevelConfig { kind: MapKind::Swamp, ..LevelConfig::default() };
        let fitted = (0..10).filter(|&seed| islands(&c, seed).notes.is_empty()).count();
        assert!(fitted >= 9, "only {fitted}/10 seeds fit every default island");
    }

    #[test]
    fn same_seed_same_islands() {
        let c = config();
        assert_eq!(islands(&c, 9).layout, islands(&c, 9).layout);
        assert_ne!(islands(&c, 9).layout.ids, islands(&c, 10).layout.ids);
    }
}
