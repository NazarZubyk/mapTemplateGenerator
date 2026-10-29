//! The region shape (the raw form of a town, swamp, forest or cave): sample a
//! general fBm noise field over the map, stretch it to full contrast, cut it
//! so the region covers the configured share of the buildable area (everything
//! below the cut is deleted, everything above is kept), drop specks, bridge
//! the remaining pieces into one connected shape, and fill fully enclosed
//! holes. The map kind sets the defaults: a TOWN keeps most of the map and
//! cuts the rest away (eroded edges, solid center); the island kinds scatter
//! blobs that get bridged together. Walls, sections and roads build on this
//! in later steps.

use crate::generator::grid;
use crate::generator::noise::NoiseField;
use crate::level::{CellMask, NoiseTexture, LevelShape, ShapeStats};
use crate::model::LevelConfig;

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeBuild {
    pub shape: LevelShape,
    /// Grayscale snapshot of the normalized noise field, one value per cell
    /// (bench preview: "noise texture" view).
    pub noise_preview: NoiseTexture,
}

pub fn build(config: &LevelConfig, seed: u64) -> ShapeBuild {
    let settings = &config.shape;
    let cell = config.cell_size_m.max(0.05);
    let width = ((config.map.width_m / cell).round() as usize).max(1);
    let height = ((config.map.height_m / cell).round() as usize).max(1);

    // 1. Sample the RAW noise field at every cell center. The border margin
    //    does not bend this field (that would soften the interior edges);
    //    it is a separate cut applied on top (see below).
    let margin = settings.border_margin_m.max(0.0);
    let field = NoiseField::new(seed);
    let octaves = settings.octaves.clamp(1, 8);
    let scale = settings.noise_scale_m.max(1.0);
    let map_w = config.map.width_m;
    let map_h = config.map.height_m;
    let mut values = vec![0.0f32; width * height];
    // Distance to the border per cell, shifted by a low-frequency wobble so
    // the margin line itself meanders — deep bites in some places, shallow in
    // others — instead of forming a rounded square. INFINITY = far inland.
    let mut border_dist = vec![f32::INFINITY; width * height];
    let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
    for y in 0..height {
        for x in 0..width {
            let wx = (x as f32 + 0.5) * cell;
            let wy = (y as f32 + 0.5) * cell;
            let v = field.fbm(wx / scale, wy / scale, octaves);
            values[y * width + x] = v;
            min = min.min(v);
            max = max.max(v);
            if margin > 0.0 {
                let d = wx.min(map_w - wx).min(wy.min(map_h - wy));
                if d >= cell {
                    let wobble = field.fbm((wx + 137.7) / scale, (wy + 419.3) / scale, 2);
                    border_dist[y * width + x] = d + (wobble - 0.5) * 1.6 * margin;
                }
            }
        }
    }

    // 2. Stretch the raw field to full contrast (preview) and pick the cut
    //    level from the BUILDABLE cells only (outside the margin zone). The
    //    margin therefore changes where the town may sit, but not how hard
    //    the noise is cut — the interior keeps its old organic character.
    let range = (max - min).max(1e-6);
    let target = settings.coverage_target_pct.clamp(1.0, 99.0) / 100.0;
    let mut buildable: Vec<f32> = values
        .iter()
        .zip(&border_dist)
        .filter_map(|(v, d)| (*d >= margin).then_some(*v))
        .collect();
    let threshold = if buildable.len() < 2 {
        f32::INFINITY // nothing outside the margin: the shape comes out empty
    } else {
        buildable.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let cut_index = ((1.0 - target) * (buildable.len() - 1) as f32).round() as usize;
        buildable[cut_index]
    };

    // 3. Apply the cut: a cell must pass the noise cut AND the border cut.
    //    The border subtracts a falloff that ramps from 1 at the wobbled
    //    margin line down to 0 inland, so near the border only the loudest
    //    noise survives — the edge recedes following the noise's own shape,
    //    never as a straight clip.
    let mut mask = CellMask::new(width, height, cell);
    let mut noise_preview = NoiseTexture {
        width,
        height,
        cell_size_m: cell,
        values: vec![0u8; width * height],
    };
    for (((value, keep), texel), dist) in values
        .iter()
        .zip(&mut mask.cells)
        .zip(&mut noise_preview.values)
        .zip(&border_dist)
    {
        *texel = (((value - min) / range).clamp(0.0, 1.0) * 255.0).round() as u8;
        let penalty = 1.0 - smoothstep(0.0, margin, *dist);
        *keep = *value >= threshold && *value - penalty >= threshold;
    }

    // Hard guarantee: with a margin, the outermost cell ring is never town
    // (the wavy cut above already keeps the town well inside; this pins it).
    if margin > 0.0 {
        for x in 0..width {
            mask.set(x, 0, false);
            mask.set(x, height - 1, false);
        }
        for y in 0..height {
            mask.set(0, y, false);
            mask.set(width - 1, y, false);
        }
    }

    let raw_blobs = grid::components(&mask).1;
    let speckle_removed =
        grid::remove_small_components(&mut mask, settings.min_blob_cells.max(1));

    // 3. Bridge the pieces into one connected shape.
    let mut bridges = 0u32;
    let bridge_width = settings.bridge_width_cells.clamp(1, 8);
    for _ in 0..raw_blobs {
        if !grid::bridge_closest_pair(&mut mask, bridge_width) {
            break;
        }
        bridges += 1;
    }

    // 4. Fill fully enclosed holes so the shape reads as one solid form.
    let holes_filled = grid::fill_holes(&mut mask);

    let coverage_pct = mask.count_true() as f32 / (width * height) as f32 * 100.0;
    ShapeBuild {
        shape: LevelShape {
            mask,
            stats: ShapeStats {
                raw_blobs,
                speckle_cells_removed: speckle_removed,
                bridges,
                hole_cells_filled: holes_filled,
                coverage_pct,
            },
        },
        noise_preview,
    }
}

/// Quintic-equivalent smooth ramp from 0 at `edge0` to 1 at `edge1`.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0).max(f32::EPSILON)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_town_is_one_connected_shape_with_sane_coverage() {
        let built = build(&LevelConfig::default(), 42);
        assert!(grid::is_connected(&built.shape.mask), "must be one piece");
        let coverage = built.shape.stats.coverage_pct;
        // Coverage is the share of the whole map; the default border margin
        // removes a strip from the budget, so ~60% of the buildable area
        // lands around 45% of the map.
        assert!(
            (30.0..=95.0).contains(&coverage),
            "default coverage {coverage}% is not a town-like blob"
        );
    }

    #[test]
    fn coverage_hits_target_on_many_seeds() {
        let mut config = LevelConfig::default();
        config.shape.coverage_target_pct = 45.0;
        config.shape.border_margin_m = 0.0; // pure percentile semantics
        for seed in 1..=10u64 {
            let coverage = build(&config, seed).shape.stats.coverage_pct;
            assert!(
                (38.0..=55.0).contains(&coverage),
                "seed {seed}: coverage {coverage}% far from the 45% target"
            );
        }
    }

    #[test]
    fn same_config_and_seed_give_identical_shape() {
        let config = LevelConfig::default();
        assert_eq!(build(&config, 7), build(&config, 7));
    }

    #[test]
    fn different_seed_changes_the_shape() {
        let config = LevelConfig::default();
        let a = build(&config, 1).shape.mask.cells;
        let b = build(&config, 2).shape.mask.cells;
        assert_ne!(a, b);
    }

    #[test]
    fn tiny_coverage_target_yields_almost_nothing() {
        let mut config = LevelConfig::default();
        config.shape.coverage_target_pct = 1.0;
        let built = build(&config, 42);
        assert!(
            built.shape.stats.coverage_pct < 5.0,
            "a 1% target still kept {:.1}% of the map",
            built.shape.stats.coverage_pct
        );
    }

    #[test]
    fn huge_min_blob_size_yields_empty_shape() {
        let mut config = LevelConfig::default();
        // Larger than any possible piece: everything gets dropped.
        config.shape.min_blob_cells = 100_000;
        let built = build(&config, 42);
        assert_eq!(built.shape.mask.count_true(), 0);
        assert_eq!(built.shape.stats.coverage_pct, 0.0);
    }

    #[test]
    fn noise_preview_matches_mask_dimensions() {
        let built = build(&LevelConfig::default(), 42);
        assert_eq!(built.noise_preview.width, built.shape.mask.width);
        assert_eq!(built.noise_preview.height, built.shape.mask.height);
        assert_eq!(built.noise_preview.values.len(), built.shape.mask.cells.len());
    }

    #[test]
    fn margin_keeps_town_off_the_border_even_when_nearly_full() {
        let mut config = LevelConfig::default();
        // Stress: even a map filled to 99% must leave the border ring empty.
        config.shape.coverage_target_pct = 99.0;
        for seed in 1..=8u64 {
            let mask = build(&config, seed).shape.mask;
            for x in 0..mask.width {
                assert!(
                    !mask.get(x, 0) && !mask.get(x, mask.height - 1),
                    "seed {seed}: town touches the top/bottom border"
                );
            }
            for y in 0..mask.height {
                assert!(
                    !mask.get(0, y) && !mask.get(mask.width - 1, y),
                    "seed {seed}: town touches the left/right border"
                );
            }
        }
    }

    #[test]
    fn border_margin_changes_the_shape() {
        let mut tight = LevelConfig::default();
        tight.shape.border_margin_m = 0.0;
        let mut wide = tight.clone();
        wide.shape.border_margin_m = 10.0;
        assert_ne!(
            build(&tight, 42).shape.mask.cells,
            build(&wide, 42).shape.mask.cells
        );
    }

    /// Eyeball helper (not an assertion): run with
    /// `cargo test ascii_preview -- --nocapture` to see the shape.
    #[test]
    fn ascii_preview_of_default_town() {
        let built = build(&LevelConfig::default(), 42);
        for y in 0..built.shape.mask.height {
            let row: String = (0..built.shape.mask.width)
                .map(|x| if built.shape.mask.get(x, y) { '#' } else { '.' })
                .collect();
            println!("{row}");
        }
        println!("{:?}", built.shape.stats);
    }
}
