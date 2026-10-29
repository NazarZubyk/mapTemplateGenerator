//! Generation shell. The generator is being rebuilt section by section under the
//! "Current workflow" panel (see docs/workflow.md). The previous monolithic
//! implementation is preserved under `reference/` and will be deleted once the
//! new build replaces it.

use crate::level::GeneratedLevel;
use crate::model::LevelConfig;

/// Generate a level deterministically: same (config, seed) always yields the
/// same result. Stub until the first workflow section lands.
pub fn generate(config: &LevelConfig, seed: u64) -> GeneratedLevel {
    GeneratedLevel {
        config: config.clone(),
        seed,
        objects: Vec::new(),
        notes: vec![
            "Generator is being rebuilt — no sections implemented yet (docs/workflow.md)"
                .to_owned(),
        ],
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
