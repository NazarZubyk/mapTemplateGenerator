//! Forest node 3, terrain (step F2 in docs/forest-plan.md): the land the
//! rest of the forest follows. Never drawn as such (only a soft relief
//! shading), but water flows over it and rock stands on it.
//!
//! 1. **Height** — fBm hills (`hill_scale_m`) on noise-warped coordinates,
//!    tilted to one random side (`tilt_pct`), so the water has a main way
//!    off the map.
//! 2. **Ridges** — ridged noise: the lines where a second noise field
//!    crosses its middle value are long, winding, branching lines, like
//!    rock spines. A low-frequency mask keeps only some stretches, so
//!    ridges start and end naturally and taper at the ends. The ridges
//!    raise the height (water flows around them).
//! 3. **Rock** — the top `rock_pct` of the ridge value, roughened by a fine
//!    noise (jagged outcrop edges); specks are dropped.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::grid;
use crate::generator::noise::NoiseField;
use crate::level::{AreaLayout, CellMask, TerrainLayout};
use crate::model::LevelConfig;

/// Height share of the ridges (on top of the hills).
const RIDGE_LIFT: f32 = 0.35;
/// Rock outcrops smaller than this, m², are dropped.
const MIN_ROCK_M2: f32 = 20.0;

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, areas: &AreaLayout) -> TerrainLayout {
    let s = &config.forest.terrain;
    let (w, h, cell) = (areas.width, areas.height, areas.cell_size_m);
    let n = w * h;
    let (wm, hm) = (w as f32 * cell, h as f32 * cell);
    let hills = NoiseField::new(rng.random::<u64>());
    let warp_x = NoiseField::new(rng.random::<u64>());
    let warp_y = NoiseField::new(rng.random::<u64>());
    let ridges = NoiseField::new(rng.random::<u64>());
    let ridge_mask = NoiseField::new(rng.random::<u64>());
    let rough = NoiseField::new(rng.random::<u64>());
    let dir = rng.random_range(0.0..360.0f32).to_radians();

    let scale = s.hill_scale_m.max(5.0);
    let rscale = s.ridge_scale_m.max(5.0);
    let tilt = (s.tilt_pct / 100.0).clamp(0.0, 1.0) * 0.6;
    let diag = (wm * wm + hm * hm).sqrt();
    let mut height = vec![0.0f32; n];
    let mut ridge = vec![0.0f32; n];
    for c in 0..n {
        let (x, y) = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
        let warp = scale * 0.5;
        let wx = x + warp * (warp_x.fbm(x / scale, y / scale, 3) - 0.5) * 2.0;
        let wy = y + warp * (warp_y.fbm(x / scale, y / scale, 3) - 0.5) * 2.0;
        let base = hills.fbm(wx / scale, wy / scale, 5);
        let side = ((x - wm / 2.0) * dir.cos() + (hm / 2.0 - y) * dir.sin()) / diag + 0.5;
        // Ridged: 1 on the middle line of the ridge noise, falling off fast.
        let r = ridges.fbm(wx / rscale, wy / rscale, 4);
        let line = (1.0 - (2.0 * r - 1.0).abs() * 2.2).max(0.0).powi(2);
        let mask = smoothstep(0.45, 0.62, ridge_mask.fbm(x / (rscale * 1.6), y / (rscale * 1.6), 3));
        let rv = line * mask;
        ridge[c] = rv + (rough.fbm(x / 4.0, y / 4.0, 2) - 0.5) * 0.8 * rv.min(0.5) * 2.0;
        height[c] = base * (1.0 - tilt) + side * tilt + RIDGE_LIFT * rv;
    }
    // Normalize to 0..1.
    let (lo, hi) = height.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    let span = (hi - lo).max(1e-6);
    for v in &mut height {
        *v = (*v - lo) / span;
    }

    // Rock: the top share of the ridge value.
    let want = (s.rock_pct / 100.0).clamp(0.0, 0.5);
    let mut rock = CellMask::new(w, h, cell);
    if want > 0.0 {
        let mut sorted: Vec<f32> = ridge.iter().copied().filter(|&v| v > 0.0).collect();
        sorted.sort_by(|a, b| b.total_cmp(a));
        let k = ((want * n as f32) as usize).min(sorted.len());
        if k > 0 {
            let cut = sorted[k - 1].max(0.3);
            for (r, &v) in rock.cells.iter_mut().zip(&ridge) {
                *r = v >= cut;
            }
        }
        grid::remove_small_components(&mut rock, (MIN_ROCK_M2 / (cell * cell)).ceil() as u32);
    }
    let rock_pct = rock.count_true() as f32 / n.max(1) as f32 * 100.0;
    TerrainLayout { width: w, height: h, cell_size_m: cell, height_m: height, relief_m: s.relief_m.max(0.0), rock, rock_pct }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::generator::forest::areas;
    use crate::model::MapKind;

    #[test]
    fn rock_share_is_close_and_height_spans() {
        let mut c = LevelConfig { kind: MapKind::Forest, ..LevelConfig::default() };
        c.map.width_m = 160.0;
        c.map.height_m = 120.0;
        c.cell_size_m = 1.0;
        let mut rng = ChaCha8Rng::seed_from_u64(3);
        let a = areas::build(&c, &mut rng).layout;
        let t = build(&c, &mut rng, &a);
        assert!(t.rock_pct > 0.5 && t.rock_pct < 6.0, "{}", t.rock_pct);
        let max = t.height_m.iter().copied().fold(0.0, f32::max);
        assert!((max - 1.0).abs() < 1e-4);
        c.forest.terrain.rock_pct = 0.0;
        let mut rng = ChaCha8Rng::seed_from_u64(3);
        let a = areas::build(&c, &mut rng).layout;
        assert_eq!(build(&c, &mut rng, &a).rock.count_true(), 0);
    }
}
