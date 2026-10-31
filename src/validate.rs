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
    if config.kind == MapKind::Cave {
        cave(config, &mut warnings);
    }
    if config.kind == MapKind::Forest {
        forest(config, &mut warnings);
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

/// Cave settings that cannot work as asked (the generator still runs and
/// clamps / skips, with notes).
fn cave(config: &LevelConfig, out: &mut Vec<Warning>) {
    let cv = &config.cave;
    let side = config.map.width_m.min(config.map.height_m);
    let ranges = [
        ("cave.chambers.hall_max_m", "Hall size", cv.chambers.hall_min_m, cv.chambers.hall_max_m),
        ("cave.chambers.grotto_max_m", "Grotto size", cv.chambers.grotto_min_m, cv.chambers.grotto_max_m),
        ("cave.tunnels.width_max_m", "Tunnel width", cv.tunnels.width_min_m, cv.tunnels.width_max_m),
        ("cave.tunnels.stub_max_m", "Stub length", cv.tunnels.stub_min_m, cv.tunnels.stub_max_m),
        ("hidden_rooms.size_max_m", "Hidden room size", config.hidden_rooms.size_min_m, config.hidden_rooms.size_max_m),
    ];
    for (f, label, lo, hi) in ranges {
        if hi < lo {
            out.push(field(f, format!("{label}: max ({hi}) is smaller than min ({lo}) — the two are swapped at generation time")));
        }
    }
    let inner = side - 2.0 * cv.chambers.rock_margin_m.max(0.0);
    let hall = cv.chambers.hall_min_m.max(cv.chambers.hall_max_m);
    if cv.chambers.hall_count > 0 && hall > inner {
        out.push(field(
            "cave.chambers.hall_max_m",
            format!("Halls up to {hall:.0} m do not fit inside the {inner:.0} m between the rock margins — they get shrunk or skipped"),
        ));
    }
    if cv.tunnels.squeeze_pct > 0.0 && cv.tunnels.squeeze_width_m >= cv.tunnels.width_max_m.max(cv.tunnels.width_min_m) {
        out.push(field(
            "cave.tunnels.squeeze_width_m",
            format!(
                "Squeezes of {:.1} m are not narrower than the widest tunnel ({:.1} m) — no tunnel gets one",
                cv.tunnels.squeeze_width_m,
                cv.tunnels.width_max_m.max(cv.tunnels.width_min_m)
            ),
        ));
    }
    let pct = cv.style.natural_pct;
    if pct <= 0.0 || pct >= 100.0 {
        let only = if pct <= 0.0 { "hand-made" } else { "natural" };
        out.push(field("cave.style.mixing_pct", format!("Natural {pct:.0}%: the whole cave is {only} — Mixing has no effect")));
    }
    if config.gates.gates.is_empty() {
        out.push(field("gates.gates", "No gates: the cave has no way in from the map edge".to_owned()));
    }
    // Spawn terrains that exist in a cave (mud does not).
    let cave_any = |f: crate::model::SpawnOn| (f.land || f.path || f.shallow || f.hidden_room) && (f.natural || f.hand_made);
    for t in &config.mobs.types {
        if !cave_any(t.spawn_on) {
            out.push(field(
                "mobs.types.spawn_on",
                format!("Mob type '{}' has no cave spawn terrain or style ticked — it is never placed", t.name),
            ));
        }
    }
    for t in &config.interactive {
        if !cave_any(t.spawn_on) {
            out.push(field(
                "interactive.spawn_on",
                format!("Interactive type '{}' has no cave spawn terrain or style ticked — it is never placed", t.name),
            ));
        }
        let f = t.spawn_on;
        if f.hidden_room && !f.land && !f.path && !f.shallow && config.hidden_rooms.count == 0 {
            out.push(field(
                "interactive.spawn_on",
                format!("Interactive type '{}' spawns only in hidden rooms, but there are none", t.name),
            ));
        }
    }
}

/// Forest settings that cannot work as asked (the generator still runs and
/// clamps / skips, with notes).
fn forest(config: &LevelConfig, out: &mut Vec<Warning>) {
    let f = &config.forest;
    let ranges = [
        ("forest.water.width_max_m", "Stream width", f.water.width_min_m, f.water.width_max_m),
        ("forest.clearings.glade_max_m", "Glade size", f.clearings.glade_min_m, f.clearings.glade_max_m),
        ("forest.clearings.glades_max", "Glades per area", f.clearings.glades_min as f32, f.clearings.glades_max as f32),
        ("forest.trees.trunk_max_m", "Trunk size", f.trees.trunk_min_m, f.trees.trunk_max_m),
        ("forest.props.group_max", "Props per group", f.props.group_min as f32, f.props.group_max as f32),
        ("forest.props.group_size_max_m", "Group prop size", f.props.group_size_min_m, f.props.group_size_max_m),
        ("forest.props.single_size_max_m", "Single prop size", f.props.single_size_min_m, f.props.single_size_max_m),
        ("forest.props.rock_size_max_m", "Near-rock prop size", f.props.rock_size_min_m, f.props.rock_size_max_m),
        ("forest.props.managed_size_max_m", "Managed prop size", f.props.managed_size_min_m, f.props.managed_size_max_m),
        ("forest.props.long_width_max_m", "Long prop width", f.props.long_width_min_m, f.props.long_width_max_m),
        ("hidden_rooms.size_max_m", "Hidden glade size", config.hidden_rooms.size_min_m, config.hidden_rooms.size_max_m),
    ];
    for (fd, label, lo, hi) in ranges {
        if hi < lo {
            out.push(field(fd, format!("{label}: max ({hi}) is smaller than min ({lo}) — the two are swapped at generation time")));
        }
    }
    // Areas so small the border belts fill them.
    let (wm, hm) = (config.map.width_m, config.map.height_m);
    if wm > 0.0 && hm > 0.0 && f.areas.count > 0 && f.thickets.border_pct > 0.0 {
        let side = (wm * hm / f.areas.count as f32).sqrt();
        let belts = 2.0 * f.thickets.border_width_m + f.thickets.edge_width_m;
        if side < belts * 1.5 {
            out.push(field(
                "forest.areas.count",
                format!(
                    "{} areas on a {wm}×{hm} m map are about {side:.0} m across — the thicket belts ({:.0} m from each border) leave little ground; fewer areas or narrower belts",
                    f.areas.count, f.thickets.border_width_m
                ),
            ));
        }
    }
    // A style in use whose characters all weigh 0.
    let c = &f.areas.characters;
    let wild = c.meadow + c.open_woods + c.dense_woods + c.old_growth;
    let managed = c.meadow + c.logged + c.plantation + c.camp_grounds;
    if f.areas.wild_pct > 0.0 && wild <= 0.0 {
        out.push(field("forest.areas.characters", "Every wild character weighs 0 — wild areas fall back to open woods".to_owned()));
    }
    if f.areas.wild_pct < 100.0 && managed <= 0.0 {
        out.push(field("forest.areas.characters", "Every managed character weighs 0 — managed areas fall back to logged".to_owned()));
    }
    let pct = f.areas.wild_pct;
    if pct <= 0.0 || pct >= 100.0 {
        let only = if pct <= 0.0 { "managed" } else { "wild" };
        out.push(field("forest.areas.mixing_pct", format!("Wild {pct:.0}%: the whole forest is {only} — Mixing has no effect")));
    }
    if config.gates.gates.is_empty() {
        out.push(field("gates.gates", "No gates: the forest has no way in from the map edge".to_owned()));
    }
    if f.water.streams > 0 && f.water.width_max_m.max(f.water.width_min_m) < f.water.deep_from_m {
        out.push(field(
            "forest.water.deep_from_m",
            format!(
                "No stream reaches Deep from ({:.1} m; widest {:.1} m): every stream is wadeable and separates nothing",
                f.water.deep_from_m,
                f.water.width_max_m.max(f.water.width_min_m)
            ),
        ));
    }
    if f.trees.density_pct > 0.0 && f.trees.trunk_max_m.max(f.trees.trunk_min_m) >= f.trees.spacing_m {
        out.push(field(
            "forest.trees.spacing_m",
            format!(
                "Trunks up to {:.1} m are as wide as the {:.1} m spacing — few trees fit",
                f.trees.trunk_max_m.max(f.trees.trunk_min_m),
                f.trees.spacing_m
            ),
        ));
    }
    if f.terrain.rock_pct <= 0.0 && f.props.near_rock_per_ha > 0.0 {
        out.push(field("forest.props.near_rock_per_ha", "Rock 0%: the near-rock props have no rock and go anywhere".to_owned()));
    }
    // Spawn terrains that exist in a forest (mud does not).
    let forest_any = |s: crate::model::SpawnOn| (s.land || s.path || s.shallow || s.hidden_room) && (s.natural || s.hand_made);
    for t in &config.mobs.types {
        if !forest_any(t.spawn_on) {
            out.push(field(
                "mobs.types.spawn_on",
                format!("Mob type '{}' has no forest spawn terrain (ground, trail, shallow water, hidden glade) or wild / managed ticked — it is never placed", t.name),
            ));
        }
    }
    for t in &config.interactive {
        if !forest_any(t.spawn_on) {
            out.push(field(
                "interactive.spawn_on",
                format!("Interactive type '{}' has no forest spawn terrain or wild / managed ticked — it is never placed", t.name),
            ));
        }
        let s = t.spawn_on;
        if s.hidden_room && !s.land && !s.path && !s.shallow && config.hidden_rooms.count == 0 {
            out.push(field("interactive.spawn_on", format!("Interactive type '{}' spawns only in hidden glades, but there are none", t.name)));
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
    fn default_forest_is_clean() {
        let forest = LevelConfig::starting();
        assert_eq!(forest.kind, MapKind::Forest);
        assert!(validate(&forest).is_empty(), "{:?}", validate(&forest));
    }

    #[test]
    fn forest_warnings_fire() {
        let mut c = LevelConfig::starting();
        c.forest.trees.trunk_min_m = 2.0;
        c.forest.trees.trunk_max_m = 1.0;
        c.forest.areas.count = 40;
        c.forest.areas.wild_pct = 100.0;
        c.forest.water.deep_from_m = 20.0;
        c.forest.terrain.rock_pct = 0.0;
        c.gates.gates.clear();
        c.mobs.types[0].spawn_on = crate::model::SpawnOn { mud: true, natural: true, hand_made: true, ..crate::model::SpawnOn::NONE };
        let fields: Vec<String> = validate(&c).into_iter().filter_map(|w| w.field).collect();
        for want in [
            "forest.trees.trunk_max_m",
            "forest.areas.count",
            "forest.areas.mixing_pct",
            "forest.water.deep_from_m",
            "forest.props.near_rock_per_ha",
            "gates.gates",
            "mobs.types.spawn_on",
        ] {
            assert!(fields.iter().any(|f| f == want), "missing {want}: {fields:?}");
        }
        c.kind = MapKind::Town;
        assert!(validate(&c).iter().all(|w| w.field.as_deref().is_none_or(|f| !f.starts_with("forest."))));
    }

    #[test]
    fn cave_warnings_fire_and_default_cave_is_clean() {
        let cave = LevelConfig { kind: MapKind::Cave, shape: crate::model::ShapeSettings::for_kind(MapKind::Cave), ..LevelConfig::default() };
        assert!(validate(&cave).is_empty(), "{:?}", validate(&cave));
        let mut c = cave;
        c.cave.chambers.hall_min_m = 50.0;
        c.cave.chambers.hall_max_m = 30.0;
        c.cave.tunnels.squeeze_width_m = 9.0;
        c.cave.style.natural_pct = 100.0;
        c.gates.gates.clear();
        c.mobs.types[0].spawn_on = crate::model::SpawnOn { mud: true, natural: true, hand_made: true, ..crate::model::SpawnOn::NONE };
        let fields: Vec<String> = validate(&c).into_iter().filter_map(|w| w.field).collect();
        for want in ["cave.chambers.hall_max_m", "cave.tunnels.squeeze_width_m", "cave.style.mixing_pct", "gates.gates", "mobs.types.spawn_on"] {
            assert!(fields.iter().any(|f| f == want), "missing {want}: {fields:?}");
        }
        c.kind = MapKind::Town;
        assert!(validate(&c).iter().all(|w| w.field.as_deref().is_none_or(|f| !f.starts_with("cave."))));
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
