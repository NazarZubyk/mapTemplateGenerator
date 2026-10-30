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
    if !(1.0..=99.0).contains(&config.shape.coverage_target_pct) {
        warnings.push(Warning {
            field: Some("shape.coverage_target_pct".to_owned()),
            message: format!(
                "Region coverage must be in 1..=99 % (is {})",
                config.shape.coverage_target_pct
            ),
            blocking: false,
        });
    }
    if config.shape.border_margin_m < 0.0 {
        warnings.push(Warning {
            field: Some("shape.border_margin_m".to_owned()),
            message: "Border margin cannot be negative".to_owned(),
            blocking: false,
        });
    }
    let effective_margin = config.shape.border_margin_m
        * if config.shape.scale_with_map {
            config.map.width_m.min(config.map.height_m) / crate::generator::shape::REFERENCE_MAP_M
        } else {
            1.0
        };
    if config.map.width_m > 0.0
        && config.map.height_m > 0.0
        && 2.0 * effective_margin >= config.map.width_m.min(config.map.height_m)
    {
        warnings.push(Warning {
            field: Some("shape.border_margin_m".to_owned()),
            message: format!(
                "Border margin {:.1} m leaves no interior on a {}×{} m map — the region is \
                 squeezed into a thin strip",
                effective_margin, config.map.width_m, config.map.height_m
            ),
            blocking: false,
        });
    }
    if config.outer_wall.segment_min_m <= 0.0 {
        warnings.push(Warning {
            field: Some("outer_wall.segment_min_m".to_owned()),
            message: "Wall segment min must be positive".to_owned(),
            blocking: false,
        });
    }
    if config.outer_wall.thickness_m <= 0.0 {
        warnings.push(Warning {
            field: Some("outer_wall.thickness_m".to_owned()),
            message: "Wall thickness must be positive".to_owned(),
            blocking: false,
        });
    }
    if config.outer_wall.segment_max_m < config.outer_wall.segment_min_m {
        warnings.push(Warning {
            field: Some("outer_wall.segment_max_m".to_owned()),
            message: format!(
                "Wall segment max ({}) is smaller than min ({}) — the two are swapped \
                 at generation time",
                config.outer_wall.segment_max_m, config.outer_wall.segment_min_m
            ),
            blocking: false,
        });
    }
    if config.regions.count == 0 {
        warnings.push(Warning {
            field: Some("regions.count".to_owned()),
            message: "Region count must be at least 1".to_owned(),
            blocking: false,
        });
    }
    if config.regions.count as usize != config.regions.shares.len() {
        warnings.push(Warning {
            field: Some("regions.shares".to_owned()),
            message: format!(
                "Region share count ({}) does not match the region count ({}) — equal \
                 shares are used instead",
                config.regions.shares.len(),
                config.regions.count
            ),
            blocking: false,
        });
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_no_warnings() {
        assert!(validate(&LevelConfig::test_64()).is_empty());
    }

    #[test]
    fn field_warnings_fire() {
        let mut config = LevelConfig::test_64();
        config.map.walkable_pct = 150.0;
        config.map.width_m = -1.0;
        config.cell_size_m = 0.0;
        let warnings = validate(&config);
        assert_eq!(warnings.len(), 3);
    }

    #[test]
    fn bad_town_coverage_warns_non_blocking() {
        let mut config = LevelConfig::test_64();
        config.shape.coverage_target_pct = 150.0;
        let warnings = validate(&config);
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0].field.as_deref(),
            Some("shape.coverage_target_pct")
        );
        assert!(!warnings[0].blocking);
    }

    #[test]
    fn wall_segment_max_below_min_warns() {
        let mut config = LevelConfig::test_64();
        config.outer_wall.segment_min_m = 8.0;
        config.outer_wall.segment_max_m = 3.0;
        let warnings = validate(&config);
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0].field.as_deref(),
            Some("outer_wall.segment_max_m")
        );
        assert!(!warnings[0].blocking);
    }
}
