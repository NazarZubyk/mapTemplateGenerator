//! Cave node 2, style (step C1 in docs/cave-plan.md): which parts of the
//! map are natural caverns and which are hand-made passages.
//!
//! 1. **Style field** — per cell a value mixed from two parts:
//!    - a **gradient** across the map in a random direction, with a gentle
//!      wobble, which splits the map into two sides;
//!    - a **patch noise** whose feature size shrinks from the map size
//!      (mixing 0%) to `PATCH_MIN_M` (mixing 100%).
//!
//!    `mixing_pct` fades from the gradient to the noise, so 0% gives two
//!    zones split by one wavy line and 100% a fine patchwork.
//! 2. **Cut** — the lowest `natural_pct` of the values are natural, the
//!    rest hand-made, so the share is hit exactly before the cleanup.
//! 3. **Cleanup** — patches smaller than `min_zone_m2` join the style
//!    around them (natural specks first, then hand-made ones). That eats
//!    into the minority style, so the cut is corrected by the drift and
//!    redone (`CUT_PASSES`).
//! 4. **Zones** — the connected patches of one style, numbered.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::grid;
use crate::generator::noise::NoiseField;
use crate::level::{CaveStyle, CaveZone, CellMask, StyleLayout};
use crate::model::LevelConfig;

/// Zone ids are 16-bit.
const MAX_ZONE: u32 = u16::MAX as u32 - 1;

/// Patch size at mixing 100%, meters (about one chamber).
const PATCH_MIN_M: f32 = 16.0;
/// Wobble of the boundary line at low mixing, as a share of the gradient.
const WOBBLE: f32 = 0.35;
/// Weight of the patch noise against the gradient (the noise spreads much
/// less than the gradient's -1..1).
const PATCH_WEIGHT: f32 = 3.0;

/// Cut corrections after the cleanup.
const CUT_PASSES: usize = 4;

/// Flips every `target` patch smaller than `min_cells` to the other style.
fn flip_small(mask: &mut CellMask, target: bool, min_cells: u32) {
    let probe = CellMask { cells: mask.cells.iter().map(|&v| v == target).collect(), ..mask.clone() };
    let (labels, count) = grid::components(&probe);
    let mut size = vec![0u32; count as usize + 1];
    for &l in &labels {
        size[l as usize] += 1;
    }
    for (k, &l) in labels.iter().enumerate() {
        if l > 0 && size[l as usize] < min_cells {
            mask.cells[k] = !target;
        }
    }
}

pub struct StyleBuild {
    pub layout: StyleLayout,
    pub notes: Vec<String>,
}

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng) -> StyleBuild {
    let s = &config.cave.style;
    let cell = config.cell_size_m;
    let w = ((config.map.width_m / cell).round() as usize).max(1);
    let h = ((config.map.height_m / cell).round() as usize).max(1);
    let (wm, hm) = (w as f32 * cell, h as f32 * cell);
    let mut notes = Vec::new();

    // RNG: direction, then the two noise seeds.
    let dir_deg: f32 = rng.random_range(0.0..360.0);
    let wobble = NoiseField::new(rng.random::<u64>());
    let patches = NoiseField::new(rng.random::<u64>());

    // 1. Style field.
    let m = (s.mixing_pct / 100.0).clamp(0.0, 1.0);
    let side = wm.max(hm);
    let patch_m = side * (PATCH_MIN_M / side).min(1.0).powf(m);
    let wobble_m = side / 3.0;
    let (cos, sin) = (dir_deg.to_radians().cos(), dir_deg.to_radians().sin());
    let half_extent = ((wm * cos).abs() + (hm * sin).abs()) / 2.0;
    let value = |c: usize| {
        let (x, y) = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
        // y grows down; 90° is north (up).
        let g = ((x - wm / 2.0) * cos + (hm / 2.0 - y) * sin) / half_extent.max(cell);
        let wob = (wobble.fbm(x / wobble_m, y / wobble_m, 3) - 0.5) * 2.0;
        let patch = (patches.fbm(x / patch_m, y / patch_m, 3) - 0.5) * 2.0;
        (1.0 - m) * (g + WOBBLE * wob) + m * PATCH_WEIGHT * patch
    };
    let values: Vec<f32> = (0..w * h).map(value).collect();

    // 2 + 3. Cut at the natural share, then let small patches join the
    // style around them. The cleanup eats into the minority style, so the
    // cut is corrected by the drift and redone a few times.
    let n = w * h;
    let asked = s.natural_pct.clamp(0.0, 100.0) / 100.0;
    let min_cells = (s.min_zone_m2.max(0.0) / (cell * cell)).round() as u32;
    let mut sorted = values.clone();
    sorted.sort_by(f32::total_cmp);
    let cut_at = |share: f32| {
        let want = (share.clamp(0.0, 1.0) * n as f32).round() as usize;
        let mut mask = CellMask::new(w, h, cell);
        if want >= n {
            mask.cells.fill(true);
        } else if want > 0 {
            let cut = sorted[want];
            for (k, v) in values.iter().enumerate() {
                mask.cells[k] = *v < cut;
            }
        }
        flip_small(&mut mask, true, min_cells);
        flip_small(&mut mask, false, min_cells);
        mask
    };
    let mut share = asked;
    let mut natural = cut_at(share);
    if asked > 0.0 && asked < 1.0 {
        for _ in 0..CUT_PASSES {
            let got = natural.count_true() as f32 / n as f32;
            if (got - asked).abs() < 0.01 {
                break;
            }
            share = (share + asked - got).clamp(0.0, 1.0);
            natural = cut_at(share);
        }
    }

    // 4. Zones.
    let mut ids = vec![0u16; n];
    let mut zones = Vec::new();
    let mut merged_zones = 0u32;
    for style in [CaveStyle::Natural, CaveStyle::HandMade] {
        let want = style == CaveStyle::Natural;
        let probe = CellMask { cells: natural.cells.iter().map(|&v| v == want).collect(), ..natural.clone() };
        let (labels, count) = grid::components(&probe);
        // Zone ids are 16-bit: on a huge map, patches past the last id
        // share it (a note says so).
        let base = zones.len().min(MAX_ZONE as usize) as u32;
        let mut sums = vec![(0u32, 0.0f32, 0.0f32); count as usize];
        for (k, &l) in labels.iter().enumerate() {
            if l > 0 {
                ids[k] = (base + l).min(MAX_ZONE) as u16;
                let e = &mut sums[l as usize - 1];
                e.0 += 1;
                e.1 += ((k % w) as f32 + 0.5) * cell;
                e.2 += ((k / w) as f32 + 0.5) * cell;
            }
        }
        for (i, (cells, sx, sy)) in sums.into_iter().enumerate() {
            if base + i as u32 + 1 > MAX_ZONE {
                merged_zones += 1;
                continue;
            }
            zones.push(CaveZone {
                id: (base + i as u32 + 1) as u16,
                style,
                area_m2: cells as f32 * cell * cell,
                center_m: [sx / cells.max(1) as f32, sy / cells.max(1) as f32],
            });
        }
    }

    let natural_pct = natural.count_true() as f32 / n as f32 * 100.0;
    if (natural_pct - s.natural_pct.clamp(0.0, 100.0)).abs() > 5.0 {
        notes.push(format!(
            "Style: natural share came out {natural_pct:.0}% (asked {:.0}%) — small patches joined the other style; lower the min zone size",
            s.natural_pct
        ));
    }

    if merged_zones > 0 {
        notes.push(format!("Style: {merged_zones} zones past the 65534th share its id — raise the min zone size on a map this big"));
    }

    StyleBuild {
        layout: StyleLayout {
            width: w,
            height: h,
            cell_size_m: cell,
            ids,
            zones,
            natural_side_deg: (dir_deg + 180.0) % 360.0,
            natural_pct,
        },
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::model::MapKind;

    fn config(natural: f32, mixing: f32) -> LevelConfig {
        let mut c = LevelConfig { kind: MapKind::Cave, ..LevelConfig::default() };
        c.map.width_m = 120.0;
        c.map.height_m = 100.0;
        c.cell_size_m = 1.0;
        c.cave.style.natural_pct = natural;
        c.cave.style.mixing_pct = mixing;
        c
    }

    fn run(c: &LevelConfig, seed: u64) -> StyleLayout {
        build(c, &mut ChaCha8Rng::seed_from_u64(seed)).layout
    }

    #[test]
    fn every_cell_in_one_connected_zone() {
        for (mixing, seed) in [(0.0, 1), (40.0, 2), (100.0, 3)] {
            let l = run(&config(60.0, mixing), seed);
            assert!(l.ids.iter().all(|&id| id >= 1 && id as usize <= l.zones.len()));
            for z in &l.zones {
                let mask = CellMask {
                    width: l.width,
                    height: l.height,
                    cell_size_m: 1.0,
                    cells: l.ids.iter().map(|&id| id == z.id).collect(),
                };
                assert!(grid::is_connected(&mask), "zone {} split", z.id);
                assert!(z.area_m2 >= 300.0, "zone {} is a speck", z.id);
            }
        }
    }

    #[test]
    fn natural_share_is_hit() {
        for (pct, mixing) in [(60.0, 0.0), (30.0, 50.0), (75.0, 100.0)] {
            let l = run(&config(pct, mixing), 4);
            assert!((l.natural_pct - pct).abs() < 5.0, "asked {pct}, got {}", l.natural_pct);
        }
    }

    #[test]
    fn mixing_zero_splits_in_two_and_full_mixing_patches() {
        for seed in 0..4 {
            let l = run(&config(50.0, 0.0), seed);
            assert_eq!(l.zones.len(), 2, "seed {seed}: {:?}", l.zones);
            let mut big = config(50.0, 100.0);
            big.map.width_m = 200.0;
            big.map.height_m = 200.0;
            let full = run(&big, seed);
            assert!(full.zones.len() >= 5, "seed {seed}: only {} zones", full.zones.len());
        }
    }

    #[test]
    fn one_style_only_at_the_ends() {
        let all = run(&config(100.0, 50.0), 5);
        assert_eq!(all.zones.len(), 1);
        assert_eq!(all.zones[0].style, CaveStyle::Natural);
        let none = run(&config(0.0, 50.0), 5);
        assert_eq!(none.zones.len(), 1);
        assert_eq!(none.zones[0].style, CaveStyle::HandMade);
    }

    #[test]
    fn same_seed_same_style() {
        let c = config(60.0, 30.0);
        assert_eq!(run(&c, 6), run(&c, 6));
    }
}
