use crate::model::{LevelConfig, MapKind, Warning};

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
    if config.kind == MapKind::Swamp {
        swamp(config, &mut warnings);
    }
    warnings
}

fn field(field: &str, message: String) -> Warning {
    Warning { field: Some(field.to_owned()), message, blocking: false }
}

/// Swamp settings that cannot work as asked (the generator still runs and
/// clamps / skips, with notes).
fn swamp(config: &LevelConfig, out: &mut Vec<Warning>) {
    let sw = &config.swamp;
    let side = config.map.width_m.min(config.map.height_m);
    let ranges = [
        ("swamp.gate_anchors.island_max_m", "Gate island size", sw.gate_anchors.island_min_m, sw.gate_anchors.island_max_m),
        ("swamp.gate_anchors.bridge_max_m", "Bridge length", sw.gate_anchors.bridge_min_m, sw.gate_anchors.bridge_max_m),
        ("swamp.islands.hub_max_m", "Hub size", sw.islands.hub_min_m, sw.islands.hub_max_m),
        ("swamp.islands.small_max_m", "Small island size", sw.islands.small_min_m, sw.islands.small_max_m),
        ("swamp.paths.width_max_m", "Path width", sw.paths.width_min_m, sw.paths.width_max_m),
        ("swamp.depth.segment_max_m", "Border segment length", sw.depth.segment_min_m, sw.depth.segment_max_m),
        ("swamp.trees.size_max_m", "Tree size", sw.trees.size_min_m, sw.trees.size_max_m),
    ];
    for (f, label, lo, hi) in ranges {
        if hi < lo {
            out.push(field(f, format!("{label}: max ({hi}) is smaller than min ({lo}) — the two are swapped at generation time")));
        }
    }
    let gate_island = sw.gate_anchors.island_min_m.max(sw.gate_anchors.island_max_m);
    if side > 0.0 && gate_island > side * 0.4 {
        out.push(field(
            "swamp.gate_anchors.island_max_m",
            format!("Gate islands up to {gate_island:.0} m are shrunk to {:.0} m (40% of the shorter map side)", side * 0.4),
        ));
    }
    let bridge = sw.gate_anchors.bridge_min_m.max(sw.gate_anchors.bridge_max_m);
    let bridged = config.gates.gates.iter().any(|g| g.style == crate::model::GateStyle::Bridge);
    if bridged && side > 0.0 && bridge + gate_island / 2.0 > side * 0.4 {
        out.push(field(
            "swamp.gate_anchors.bridge_max_m",
            format!("Bridges up to {bridge:.0} m plus the gate island reach past 40% of the map side — bridges get shortened"),
        ));
    }
    let gates = &config.gates.gates;
    for (a, x) in gates.iter().enumerate() {
        for (b, y) in gates.iter().enumerate().skip(a + 1) {
            let d = (x.direction_deg - y.direction_deg).rem_euclid(360.0);
            if d.min(360.0 - d) < 15.0 {
                out.push(field(
                    "gates.gates",
                    format!("Gates #{} and #{} point within 15° of each other — their gate islands squeeze each other", a + 1, b + 1),
                ));
            }
        }
    }
    let inner = side - 2.0 * sw.islands.border_m.max(0.0);
    let hub = sw.islands.hub_min_m.max(sw.islands.hub_max_m);
    if sw.islands.hub_count > 0 && hub > inner {
        out.push(field(
            "swamp.islands.hub_max_m",
            format!("Hubs up to {hub:.0} m do not fit inside the {inner:.0} m between the border margins — they get shrunk or skipped"),
        ));
    }
    if sw.depth.wade_limit_m < sw.depth.hummock_m * 2.0 {
        out.push(field(
            "swamp.depth.wade_limit_m",
            format!(
                "A wading limit of {:.0} m is shorter than two hummocks ({:.0} m) — mud paths fill up with hummocks",
                sw.depth.wade_limit_m, sw.depth.hummock_m
            ),
        ));
    }
    for t in &config.mobs.types {
        if !t.spawn_on.any() {
            out.push(field("mobs.types.spawn_on", format!("Mob type '{}' has no spawn terrain ticked — it is never placed", t.name)));
        }
    }
    for t in &config.interactive {
        if !t.spawn_on.any() {
            out.push(field(
                "interactive.spawn_on",
                format!("Interactive type '{}' has no spawn terrain ticked — it is never placed", t.name),
            ));
        }
        let wet = t.spawn_on.shallow || t.spawn_on.mud;
        if t.spawn_on.hidden_room && !wet && !t.spawn_on.land && !t.spawn_on.path && config.hidden_rooms.count == 0 {
            out.push(field(
                "interactive.spawn_on",
                format!("Interactive type '{}' spawns only in hidden rooms, but there are none", t.name),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_no_warnings() {
        assert!(validate(&LevelConfig::test_64()).is_empty());
        let swamp = LevelConfig { kind: MapKind::Swamp, ..LevelConfig::default() };
        assert!(validate(&swamp).is_empty(), "{:?}", validate(&swamp));
    }

    #[test]
    fn swamp_warnings_fire() {
        let mut c = LevelConfig { kind: MapKind::Swamp, ..LevelConfig::default() };
        c.swamp.islands.hub_min_m = 50.0;
        c.swamp.islands.hub_max_m = 30.0;
        c.swamp.gate_anchors.island_max_m = 120.0;
        c.gates.gates = vec![crate::model::GateSpec::at(90.0), crate::model::GateSpec::at(100.0)];
        c.mobs.types[0].spawn_on = crate::model::SpawnOn::NONE;
        c.swamp.depth.wade_limit_m = 4.0;
        let fields: Vec<String> = validate(&c).into_iter().filter_map(|w| w.field).collect();
        for want in ["swamp.islands.hub_max_m", "swamp.gate_anchors.island_max_m", "gates.gates", "mobs.types.spawn_on", "swamp.depth.wade_limit_m"] {
            assert!(fields.iter().any(|f| f == want), "missing {want}: {fields:?}");
        }
        // Town maps ignore the swamp settings.
        c.kind = MapKind::Town;
        assert!(validate(&c).iter().all(|w| w.field.as_deref().is_none_or(|f| !f.starts_with("swamp."))));
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
