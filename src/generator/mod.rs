//! Generation shell. The generator is being rebuilt section by section under the
//! "Current workflow" panel (see docs/workflow.md). The previous monolithic
//! implementation is preserved under `reference/` and will be deleted once the
//! new build replaces it.

use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};

use crate::level::{GeneratedLevel, LevelShape};
use crate::model::LevelConfig;

pub mod grid;
pub mod noise;
pub mod shape;
pub mod walls;

/// Generate a level deterministically: same (config, seed) always yields the
/// same result. Currently implemented: step 1 (the noise-derived region
/// shape) and step 2 (the outer wall ring); phases run in fixed order and
/// share the one RNG stream (docs/workflow.md).
pub fn generate(config: &LevelConfig, seed: u64) -> GeneratedLevel {
    let built = shape::build(config, seed);
    let mut notes = Vec::new();
    if built.shape.mask.count_true() == 0 {
        notes.push(format!(
            "{} shape came out empty — lower the coverage target or the min blob size",
            config.kind.label()
        ));
    }

    // Step 2: outer wall. Redefines the region as the wall's interior.
    let mut region_mask = built.shape.mask;
    let mut walls = None;
    if region_mask.count_true() > 0 {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        match walls::build(config, &mut rng, &region_mask) {
            Some(w) => {
                if w.wall.stats.extra_loops > 0 {
                    notes.push(format!(
                        "Outer wall: {} extra boundary loop(s) discarded (border pinch \
                         points)",
                        w.wall.stats.extra_loops
                    ));
                }
                region_mask = w.region;
                walls = Some(w.wall);
            }
            None => notes.push(
                "Outer wall skipped: the region is too small for these segment lengths"
                    .to_owned(),
            ),
        }
    }

    let total = region_mask.width * region_mask.height;
    let mut stats = built.shape.stats;
    stats.coverage_pct = region_mask.count_true() as f32 / total.max(1) as f32 * 100.0;

    GeneratedLevel {
        config: config.clone(),
        seed,
        objects: Vec::new(),
        shape: Some(LevelShape {
            mask: region_mask,
            stats,
        }),
        walls,
        noise_preview: Some(built.noise_preview),
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_result() {
        let config = LevelConfig::default();
        assert_eq!(generate(&config, 42), generate(&config, 42));
        let other = LevelConfig {
            map: crate::model::MapSettings {
                width_m: 32.0,
                ..crate::model::MapSettings::default()
            },
            ..LevelConfig::default()
        };
        assert_ne!(generate(&config, 42), generate(&other, 42));
    }
}
