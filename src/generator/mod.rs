//! Generation shell. The generator is being rebuilt section by section under the
//! "Current workflow" panel (see docs/workflow.md). The previous monolithic
//! implementation is preserved under `reference/` and will be deleted once the
//! new build replaces it.

use crate::level::GeneratedLevel;
use crate::model::LevelConfig;

pub mod grid;
pub mod noise;
pub mod shape;

/// Generate a level deterministically: same (config, seed) always yields the
/// same result. Currently implemented: step 1, the noise-derived region shape
/// (docs/workflow.md).
pub fn generate(config: &LevelConfig, seed: u64) -> GeneratedLevel {
    let built = shape::build(config, seed);
    let mut notes = Vec::new();
    if built.shape.mask.count_true() == 0 {
        notes.push(format!(
            "{} shape came out empty — lower the coverage target or the min blob size",
            config.kind.label()
        ));
    }
    GeneratedLevel {
        config: config.clone(),
        seed,
        objects: Vec::new(),
        shape: Some(built.shape),
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
