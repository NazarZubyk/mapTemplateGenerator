//! Generation shell. The generator is being rebuilt section by section under the
//! "Current workflow" panel (see docs/workflow.md). The previous monolithic
//! implementation is preserved under `reference/` and will be deleted once the
//! new build replaces it.

use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};

use crate::level::{GeneratedLevel, LevelShape};
use crate::model::LevelConfig;

pub mod grid;
pub mod noise;
pub mod regions;
pub mod shape;
pub mod walls;

/// Progress callback: (stage label, overall fraction 0..1). Called from the
/// generating thread; must be cheap.
pub type Progress<'a> = &'a dyn Fn(&'static str, f32);

/// A sub-range of the overall progress: phase-local fractions 0..1 are
/// mapped into `from..to` of the whole generation.
pub(crate) struct Stage<'a> {
    pub progress: Progress<'a>,
    pub from: f32,
    pub to: f32,
}

impl Stage<'_> {
    pub fn report(&self, label: &'static str, local: f32) {
        (self.progress)(label, self.from + (self.to - self.from) * local.clamp(0.0, 1.0));
    }
}

/// `generate_with_progress` without progress reporting.
pub fn generate(config: &LevelConfig, seed: u64) -> GeneratedLevel {
    generate_with_progress(config, seed, &|_, _| {})
}

/// Generate a level deterministically: same (config, seed) always yields the
/// same result. `progress` is told the current stage as generation runs. Currently implemented: step 1 (the noise-derived region
/// shape), step 2 (the outer wall ring) and step 3 (the region split);
/// phases run in fixed order and share the one RNG stream (docs/workflow.md).
pub fn generate_with_progress(
    config: &LevelConfig,
    seed: u64,
    progress: Progress,
) -> GeneratedLevel {
    let stage = |from, to| Stage { progress, from, to };
    let built = shape::build_with_progress(config, seed, &stage(0.0, 0.55));
    let mut notes = Vec::new();
    if built.shape.mask.count_true() == 0 {
        notes.push(format!(
            "{} shape came out empty — lower the coverage target or the min blob size",
            config.kind.label()
        ));
    }

    // One RNG stream for every placement phase, in fixed order.
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    // Step 2: outer wall. Redefines the region as the wall's interior.
    let mut region_mask = built.shape.mask;
    let mut walls = None;
    if region_mask.count_true() > 0 {
        stage(0.55, 0.65).report("Building outer wall", 0.0);
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

    // Step 3: split the town area into regions (smallest placed first).
    let regions = if region_mask.count_true() > 0 {
        regions::build_with_progress(config, &mut rng, &region_mask, &stage(0.65, 0.99))
    } else {
        None
    };

    let total = region_mask.width * region_mask.height;
    let mut stats = built.shape.stats;
    stats.coverage_pct = region_mask.count_true() as f32 / total.max(1) as f32 * 100.0;

    progress("Done", 1.0);
    GeneratedLevel {
        config: config.clone(),
        seed,
        objects: Vec::new(),
        shape: Some(LevelShape {
            mask: region_mask,
            stats,
        }),
        walls,
        regions,
        noise_preview: Some(built.noise_preview),
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_only_moves_forward_and_ends_at_one() {
        let seen = std::cell::RefCell::new(Vec::new());
        let mut config = LevelConfig::default();
        config.shape.sections = 3;
        let level = generate_with_progress(&config, 7, &|label, f| seen.borrow_mut().push((label, f)));
        let seen = seen.into_inner();
        assert!(seen.len() > 5, "too few progress reports: {}", seen.len());
        for pair in seen.windows(2) {
            assert!(pair[1].1 >= pair[0].1, "progress went back: {pair:?}");
        }
        assert_eq!(seen.last().copied(), Some(("Done", 1.0)));
        // Reporting must not change the result.
        assert_eq!(level, generate(&config, 7));
    }

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
