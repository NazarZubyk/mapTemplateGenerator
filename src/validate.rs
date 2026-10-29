use crate::model::{LevelConfig, Warning};

/// Validate a config and collect every warning. Blocking warnings prevent
/// generation until resolved. Grows back per section (docs/workflow.md).
pub fn validate(config: &LevelConfig) -> Vec<Warning> {
    let mut warnings = Vec::new();
    if config.map.width_m <= 0.0 {
        warnings.push(Warning::new("Map width must be positive"));
    }
    if config.map.height_m <= 0.0 {
        warnings.push(Warning::new("Map height must be positive"));
    }
    if !(1.0..=99.0).contains(&config.map.walkable_pct) {
        warnings.push(Warning::new(format!(
            "Walkable % must be in 1..=99 (is {})",
            config.map.walkable_pct
        )));
    }
    if config.cell_size_m <= 0.0 {
        warnings.push(Warning::new("Cell size must be positive"));
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_no_warnings() {
        assert!(validate(&LevelConfig::default()).is_empty());
    }

    #[test]
    fn field_warnings_fire() {
        let mut config = LevelConfig::default();
        config.map.walkable_pct = 150.0;
        config.map.width_m = -1.0;
        config.cell_size_m = 0.0;
        let warnings = validate(&config);
        assert_eq!(warnings.len(), 3);
    }
}
