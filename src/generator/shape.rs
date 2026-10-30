//! The region shape (the raw form of a town, swamp, forest or cave): sample a
//! general fBm noise field over the map — domain-warped (coordinates bent by
//! a second noise field: flowing forms) and lowered with distance from a
//! random stretched, rotated center (silhouette: one tilted organic mass, not
//! the map rectangle) — stretch it to full contrast, cut it
//! so the region covers the configured share of the buildable area (everything
//! below the cut is deleted, everything above is kept), drop specks, bridge
//! the remaining pieces into one connected shape (winding bridges that
//! follow noise valleys), and fill fully enclosed
//! holes. The map kind sets the defaults: a TOWN keeps most of the map and
//! cuts the rest away (eroded edges, solid center); the island kinds scatter
//! blobs that get bridged together. Walls, sections and roads build on this
//! in later steps.

use rand::Rng;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::generator::{grid, Stage};
use crate::generator::noise::NoiseField;
use crate::level::{CellMask, NoiseTexture, LevelShape, ShapeStats};
use crate::model::LevelConfig;

/// Map side the size-relative shape settings are authored for, in meters.
pub const REFERENCE_MAP_M: f32 = 64.0;

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeBuild {
    pub shape: LevelShape,
    /// Grayscale snapshot of the normalized noise field, one value per cell
    /// (bench preview: "noise texture" view).
    pub noise_preview: NoiseTexture,
}

/// `build_with_progress` without progress reporting.
pub fn build(config: &LevelConfig, seed: u64) -> ShapeBuild {
    build_with_progress(config, seed, &Stage { progress: &|_, _| {}, from: 0.0, to: 1.0 })
}

pub(crate) fn build_with_progress(config: &LevelConfig, seed: u64, stage: &Stage) -> ShapeBuild {
    let settings = &config.shape;
    let cell = config.cell_size_m.max(0.05);
    let width = ((config.map.width_m / cell).round() as usize).max(1);
    let height = ((config.map.height_m / cell).round() as usize).max(1);

    // 1. Sample the RAW noise field at every cell center. The border margin
    //    does not bend this field (that would soften the interior edges);
    //    it is a separate cut applied on top (see below).
    // Size-relative settings: authored for a 64 m map, scaled linearly (area
    // for min blob) with the shorter map side when scale_with_map is on.
    let k = if settings.scale_with_map {
        (config.map.width_m.min(config.map.height_m) / REFERENCE_MAP_M).max(0.05)
    } else {
        1.0
    };
    let margin = settings.border_margin_m.max(0.0) * k;
    let field = NoiseField::new(seed);
    let octaves = settings.octaves.clamp(1, 8);
    let scale = (settings.noise_scale_m * k).max(1.0);
    let map_w = config.map.width_m;
    let map_h = config.map.height_m;
    // Warp: displacement up to ~±2 feature sizes at 100%.
    let warp = settings.warp_pct.clamp(0.0, 100.0) / 100.0 * 6.0;
    // Silhouette: its own small RNG off the seed (the shape phase does not
    // touch the main generation stream). Center offset ±15% of the map,
    // random axis, stretch 1..1.8.
    let pull = settings.silhouette_pct.clamp(0.0, 100.0) / 100.0;
    let mut srng = ChaCha8Rng::seed_from_u64(seed ^ 0x5111_0E77_E5EE_D001);
    let center = (
        map_w * (0.5 + srng.random_range(-0.15..=0.15)),
        map_h * (0.5 + srng.random_range(-0.15..=0.15)),
    );
    let angle = srng.random::<f32>() * std::f32::consts::PI;
    let (cos, sin) = (angle.cos(), angle.sin());
    let stretch = srng.random_range(1.0f32..=1.8).sqrt();
    let half = (map_w.min(map_h) * 0.5).max(cell);

    // Sections: 1 = the single silhouette above. More: centers spread over
    // the map, each with its own axis and stretch; every cell belongs to the
    // nearest center (on warped coordinates → wavy borders), and a channel
    // of `section_gap_m` along those borders keeps the sections apart.
    let section_count = settings.sections.clamp(1, 8) as usize;
    let sections = if section_count == 1 {
        vec![Section { center, cos, sin, stretch, half }]
    } else {
        place_sections(&mut srng, section_count, map_w, map_h, cell)
    };
    let gap_half = settings.section_gap_m.max(0.0) * k * 0.5;
    let mut section_of = vec![0u8; width * height];
    let mut channel = vec![false; width * height];

    let mut values = vec![0.0f32; width * height];
    // Distance to the border per cell, shifted by a low-frequency wobble so
    // the margin line itself meanders — deep bites in some places, shallow in
    // others — instead of forming a rounded square. INFINITY = far inland.
    let mut border_dist = vec![f32::INFINITY; width * height];
    let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
    for y in 0..height {
        if y % 16 == 0 {
            stage.report("Sampling noise", 0.7 * y as f32 / height as f32);
        }
        for x in 0..width {
            let wx = (x as f32 + 0.5) * cell;
            let wy = (y as f32 + 0.5) * cell;
            let (px, py) = (wx / scale, wy / scale);
            let (px, py) = if warp > 0.0 {
                (
                    px + warp * (field.fbm(px + 5.2, py + 1.3, 3) - 0.5),
                    py + warp * (field.fbm(px + 9.7, py + 2.8, 3) - 0.5),
                )
            } else {
                (px, py)
            };
            let mut v = field.fbm(px, py, octaves);
            // Nearest section (and the runner-up, for the channel), measured
            // on a gently wobbled position: section borders meander, but the
            // full shape warp would squeeze the channels to nothing.
            let (qx, qy) = if sections.len() > 1 {
                let (ux, uy) = (wx / scale, wy / scale);
                (
                    wx + (field.fbm(ux + 31.1, uy + 7.7, 2) - 0.5) * scale * 0.6,
                    wy + (field.fbm(ux + 2.9, uy + 53.3, 2) - 0.5) * scale * 0.6,
                )
            } else {
                (wx, wy)
            };
            let dist2 = |s: &Section| (qx - s.center.0).powi(2) + (qy - s.center.1).powi(2);
            let mut near = (0usize, f32::INFINITY);
            let mut second = (0usize, f32::INFINITY);
            for (i, s) in sections.iter().enumerate() {
                let d = dist2(s);
                if d < near.1 {
                    second = near;
                    near = (i, d);
                } else if d < second.1 {
                    second = (i, d);
                }
            }
            section_of[y * width + x] = near.0 as u8;
            if sections.len() > 1 {
                // Distance to the bisector between the two nearest centers.
                let (a, b) = (&sections[near.0], &sections[second.0]);
                let span = ((a.center.0 - b.center.0).powi(2) + (a.center.1 - b.center.1).powi(2))
                    .sqrt()
                    .max(1e-3);
                channel[y * width + x] = (second.1 - near.1) / (2.0 * span) < gap_half;
            }
            if pull > 0.0 {
                // Normalized elliptical distance from the section's center:
                // 1 ≈ the section's half-size.
                let s = &sections[near.0];
                let (dx, dy) = (wx - s.center.0, wy - s.center.1);
                let along = (dx * s.cos + dy * s.sin) / (s.half * s.stretch);
                let across = (dy * s.cos - dx * s.sin) * s.stretch / s.half;
                v -= pull * 0.8 * (along * along + across * across);
            }
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
    stage.report("Cutting shape", 0.7);
    let range = (max - min).max(1e-6);
    let target = settings.coverage_target_pct.clamp(1.0, 99.0) / 100.0;
    let mut buildable: Vec<f32> = values
        .iter()
        .zip(&border_dist)
        .zip(&channel)
        .filter_map(|((v, d), c)| (*d >= margin && !c).then_some(*v))
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
    for ((((value, keep), texel), dist), in_channel) in values
        .iter()
        .zip(&mut mask.cells)
        .zip(&mut noise_preview.values)
        .zip(&border_dist)
        .zip(&channel)
    {
        *texel = if *in_channel {
            0
        } else {
            (((value - min) / range).clamp(0.0, 1.0) * 255.0).round() as u8
        };
        let penalty = 1.0 - smoothstep(0.0, margin, *dist);
        *keep = !in_channel && *value >= threshold && *value - penalty >= threshold;
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
        grid::remove_small_components(
            &mut mask,
            ((settings.min_blob_cells as f32 * k * k).round() as u32).max(1),
        );

    // 3. Inside each section: bridge its pieces into one, then fill its
    //    enclosed holes. Bridges are kept inside their section (leaving it
    //    costs a fortune), so sections never merge through thin bridges.
    let bridge_width = ((settings.bridge_width_cells.clamp(1, 8) as f32 * k).round() as u32).clamp(1, 32);
    // Wander: entering a cell costs exp(±contrast) by a smooth noise field,
    // so corridors bend along its valleys; 0% = straight shortest lines.
    let wander = settings.bridge_wander_pct.clamp(0.0, 100.0) / 100.0 * 5.0;
    let wander_field = NoiseField::new(seed ^ 0xB71D_6E5A_77E1_0C3F);
    let wander_scale = (scale * 0.25 / cell).max(4.0);
    let step_cost = |i: usize| {
        let (x, y) = ((i % width) as f32, (i / width) as f32);
        let n = wander_field.fbm(x / wander_scale, y / wander_scale, 3);
        ((wander * (n - 0.5) * 4.0).exp() * 64.0) as u32
    };
    let mut bridges = 0u32;
    let mut holes_filled = 0u32;
    let mut built_sections = 0u32;
    for s in 0..sections.len() as u8 {
        stage.report("Bridging pieces", 0.75 + 0.15 * s as f32 / sections.len() as f32);
        let mut part = mask.clone();
        for (keep, &owner) in part.cells.iter_mut().zip(&section_of) {
            *keep &= owner == s;
        }
        let pieces = grid::components(&part).1;
        if pieces == 0 {
            continue;
        }
        built_sections += 1;
        let inside_cost = |i: usize| {
            let c = step_cost(i);
            if section_of[i] == s && !channel[i] { c } else { c.saturating_mul(1000) }
        };
        for _ in 0..pieces {
            if !grid::bridge_closest_pair_weighted(&mut part, bridge_width, inside_cost) {
                break;
            }
            bridges += 1;
        }
        holes_filled += grid::fill_holes(&mut part);
        for (keep, &p) in mask.cells.iter_mut().zip(&part.cells) {
            *keep |= p;
        }
    }

    // 4. Join the sections with wide bridges (closest pair first, so they
    //    form a spanning tree). Lagoons the bridges enclose stay open water.
    let wide = ((settings.section_bridge_m.max(0.0) * k / cell).round() as u32).clamp(1, 64);
    let mut section_bridges = 0u32;
    if built_sections > 1 {
        stage.report("Joining sections", 0.9);
    }
    for _ in 1..built_sections.max(1) {
        if !grid::bridge_closest_pair_weighted(&mut mask, wide, step_cost) {
            break;
        }
        section_bridges += 1;
    }

    let coverage_pct = mask.count_true() as f32 / (width * height) as f32 * 100.0;
    ShapeBuild {
        shape: LevelShape {
            mask,
            stats: ShapeStats {
                raw_blobs,
                speckle_cells_removed: speckle_removed,
                bridges,
                hole_cells_filled: holes_filled,
                sections: built_sections,
                section_bridges,
                coverage_pct,
            },
        },
        noise_preview,
    }
}

/// One section (island) of the shape: its center and silhouette ellipse.
struct Section {
    center: (f32, f32),
    cos: f32,
    sin: f32,
    /// √ of the length-to-width ratio.
    stretch: f32,
    /// Half-size of the silhouette, meters.
    half: f32,
}

/// Spread `n` section centers over the map (inset 20% from the border):
/// every next center is the best of 32 random candidates — the one farthest
/// from the centers already placed — so sections share the map evenly.
fn place_sections(rng: &mut ChaCha8Rng, n: usize, map_w: f32, map_h: f32, cell: f32) -> Vec<Section> {
    let mut centers: Vec<(f32, f32)> = Vec::with_capacity(n);
    let pick = |rng: &mut ChaCha8Rng| {
        (map_w * rng.random_range(0.2..=0.8), map_h * rng.random_range(0.2..=0.8))
    };
    centers.push(pick(rng));
    while centers.len() < n {
        let mut best = (f32::NEG_INFINITY, (0.0, 0.0));
        for _ in 0..32 {
            let c = pick(rng);
            let d = centers
                .iter()
                .map(|o| (c.0 - o.0).powi(2) + (c.1 - o.1).powi(2))
                .fold(f32::INFINITY, f32::min);
            if d > best.0 {
                best = (d, c);
            }
        }
        centers.push(best.1);
    }
    // Each section's silhouette is sized to its share of the map, a bit
    // smaller than its cell so open ground rings every island.
    let half = (map_w.min(map_h) * 0.5 / (n as f32).sqrt() * 0.9).max(cell);
    centers
        .into_iter()
        .map(|center| {
            let angle = rng.random::<f32>() * std::f32::consts::PI;
            Section {
                center,
                cos: angle.cos(),
                sin: angle.sin(),
                stretch: rng.random_range(1.0f32..=1.8).sqrt(),
                half,
            }
        })
        .collect()
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
        let built = build(&LevelConfig::test_64(), 42);
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
        let mut config = LevelConfig::test_64();
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
        let config = LevelConfig::test_64();
        assert_eq!(build(&config, 7), build(&config, 7));
    }

    #[test]
    fn different_seed_changes_the_shape() {
        let config = LevelConfig::test_64();
        let a = build(&config, 1).shape.mask.cells;
        let b = build(&config, 2).shape.mask.cells;
        assert_ne!(a, b);
    }

    #[test]
    fn tiny_coverage_target_yields_almost_nothing() {
        let mut config = LevelConfig::test_64();
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
        let mut config = LevelConfig::test_64();
        // Larger than any possible piece: everything gets dropped.
        config.shape.min_blob_cells = 100_000;
        let built = build(&config, 42);
        assert_eq!(built.shape.mask.count_true(), 0);
        assert_eq!(built.shape.stats.coverage_pct, 0.0);
    }

    #[test]
    fn noise_preview_matches_mask_dimensions() {
        let built = build(&LevelConfig::test_64(), 42);
        assert_eq!(built.noise_preview.width, built.shape.mask.width);
        assert_eq!(built.noise_preview.height, built.shape.mask.height);
        assert_eq!(built.noise_preview.values.len(), built.shape.mask.cells.len());
    }

    #[test]
    fn margin_keeps_town_off_the_border_even_when_nearly_full() {
        let mut config = LevelConfig::test_64();
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
        let mut tight = LevelConfig::test_64();
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
        let built = build(&LevelConfig::test_64(), 42);
        for y in 0..built.shape.mask.height {
            let row: String = (0..built.shape.mask.width)
                .map(|x| if built.shape.mask.get(x, y) { '#' } else { '.' })
                .collect();
            println!("{row}");
        }
        println!("{:?}", built.shape.stats);
    }
}
#[cfg(test)]
mod scaling {
    use super::*;

    /// With scale_with_map, a 3× larger map is the same shape, 3× larger.
    #[test]
    fn bigger_map_keeps_the_shape_character() {
        let small = LevelConfig::test_64();
        let mut big = LevelConfig::test_64();
        big.map.width_m *= 3.0;
        big.map.height_m *= 3.0;
        for seed in [1u64, 2, 3] {
            let a = build(&small, seed).shape.mask;
            let b = build(&big, seed).shape.mask;
            let same = (0..a.height)
                .flat_map(|y| (0..a.width).map(move |x| (x, y)))
                .filter(|&(x, y)| a.get(x, y) == b.get(x * 3 + 1, y * 3 + 1))
                .count();
            let agreement = same as f32 / (a.width * a.height) as f32;
            assert!(agreement > 0.9, "seed {seed}: only {agreement:.3} of cells agree");
            assert!(grid::is_connected(&b), "seed {seed}: big map shape is not one piece");
        }
    }
}

#[cfg(test)]
mod sections {
    use super::*;

    #[test]
    fn sections_are_islands_joined_by_wide_bridges() {
        for n in 2..=4u32 {
            for seed in 1..=5u64 {
                let mut config = LevelConfig::test_64();
                config.shape.sections = n;
                let built = build(&config, seed);
                let stats = built.shape.stats;
                assert!(grid::is_connected(&built.shape.mask), "n {n} seed {seed}: not one piece");
                assert_eq!(stats.sections, n, "n {n} seed {seed}: a section came out empty");
                assert_eq!(stats.section_bridges, n - 1, "n {n} seed {seed}: bridges");
            }
        }
    }

    #[test]
    fn one_section_has_no_channels_or_wide_bridges() {
        let built = build(&LevelConfig::test_64(), 3);
        assert_eq!(built.shape.stats.sections, 1);
        assert_eq!(built.shape.stats.section_bridges, 0);
    }
}

#[cfg(test)]
mod gallery {
    use super::*;
    use crate::model::{MapKind, ShapeSettings};
    /// `cargo test shape_gallery -- --nocapture`: 4×4-cell blocks per char.
    #[test]
    fn shape_gallery() {
        for kind in [MapKind::Town, MapKind::Swamp, MapKind::Forest, MapKind::Cave] {
            let mut rows = vec![String::new(); 32];
            for seed in 1..=4u64 {
                let mut config = LevelConfig {
                    kind,
                    shape: ShapeSettings::for_kind(kind),
                    ..Default::default()
                };
                if let Ok(n) = std::env::var("SECTIONS") {
                    config.shape.sections = n.parse().unwrap();
                }
                let m = build(&config, seed).shape.mask;
                for (by, row) in rows.iter_mut().enumerate() {
                    for bx in 0..m.width / 4 {
                        let n = (0..16).filter(|k| m.get(bx * 4 + k % 4, by * 4 + k / 4)).count();
                        row.push(if n >= 8 { '#' } else if n > 0 { '+' } else { '.' });
                    }
                    row.push_str("  ");
                }
            }
            println!("== {kind:?}");
            for r in rows { println!("{r}"); }
        }
    }
}
