use crate::model::{ColorRgb, FieldId, LevelConfig, Warning};

/// Validate a config and collect every warning. Blocking warnings (currently
/// duplicate colors) prevent generation until resolved.
pub fn validate(config: &LevelConfig) -> Vec<Warning> {
    let mut warnings = Vec::new();
    check_mix(config, &mut warnings);
    check_map(config, &mut warnings);
    check_structures(config, &mut warnings);
    check_props(config, &mut warnings);
    check_interactive(config, &mut warnings);
    check_hidden_rooms(config, &mut warnings);
    check_walls(config, &mut warnings);
    check_mobs(config, &mut warnings);
    check_gates(config, &mut warnings);
    check_colors(config, &mut warnings);
    check_feasibility(config, &mut warnings);
    warnings
}

fn check_mix(config: &LevelConfig, warnings: &mut Vec<Warning>) {
    if let [x, y] = config.template_mix.kinds().as_slice()
        && x == y
    {
        warnings.push(Warning::new(
            FieldId::TemplateMix,
            format!("Template mix uses the same template ({x}) twice"),
        ));
    }
}

fn check_map(config: &LevelConfig, warnings: &mut Vec<Warning>) {
    if config.map.width_m <= 0.0 {
        warnings.push(Warning::new(FieldId::MapWidth, "Map width must be positive"));
    }
    if config.map.height_m <= 0.0 {
        warnings.push(Warning::new(
            FieldId::MapHeight,
            "Map height must be positive",
        ));
    }
    if !(1.0..=99.0).contains(&config.map.walkable_pct) {
        warnings.push(Warning::new(
            FieldId::WalkablePct,
            format!(
                "Walkable % must be in 1..=99 (is {})",
                config.map.walkable_pct
            ),
        ));
    }
}

fn check_structures(config: &LevelConfig, warnings: &mut Vec<Warning>) {
    for kind in config.template_mix.kinds() {
        let s = config.structures.get(&kind).copied().unwrap_or_default();
        if !(0.0..=100.0).contains(&s.density_pct) {
            warnings.push(Warning::new(
                FieldId::StructureDensity(kind),
                format!(
                    "{kind} structure density must be in 0..=100 (is {})",
                    s.density_pct
                ),
            ));
        }
        if !s.size.is_valid() {
            warnings.push(Warning::new(
                FieldId::StructureSize(kind),
                format!(
                    "{kind} structure size range is invalid ({}-{} m)",
                    s.size.min_m, s.size.max_m
                ),
            ));
        }
    }
}

fn check_props(config: &LevelConfig, warnings: &mut Vec<Warning>) {
    let p = &config.props;
    if !(0.0..=100.0).contains(&p.density_pct) {
        warnings.push(Warning::new(
            FieldId::PropsDensity,
            format!("Props density must be in 0..=100 (is {})", p.density_pct),
        ));
    }
    if !p.size.is_valid() {
        warnings.push(Warning::new(
            FieldId::PropsSize,
            format!("Prop size range is invalid ({}-{} m)", p.size.min_m, p.size.max_m),
        ));
    }
    if p.density_pct > 0.0 && p.quantity == 0 {
        warnings.push(Warning::new(
            FieldId::PropsQuantity,
            "Props density is positive but quantity is 0: no props will be placed",
        ));
    }
}

fn check_interactive(config: &LevelConfig, warnings: &mut Vec<Warning>) {
    for (i, it) in config.interactive.iter().enumerate() {
        if it.size_m <= 0.0 {
            warnings.push(Warning::new(
                FieldId::InteractiveSize(i),
                format!("Interactive '{}' size must be positive", it.name),
            ));
        }
        if it.quantity == 0 {
            warnings.push(Warning::new(
                FieldId::InteractiveQuantity(i),
                format!(
                    "Interactive '{}' quantity is 0: this type will never be placed",
                    it.name
                ),
            ));
        }
        if !it.group.is_valid() || it.group.max_m < 1.0 {
            warnings.push(Warning::new(
                FieldId::InteractiveGroup(i),
                format!(
                    "Interactive '{}' group size is invalid ({}-{}, must be >= 1)",
                    it.name, it.group.min_m, it.group.max_m
                ),
            ));
        }
    }
}

fn check_hidden_rooms(config: &LevelConfig, warnings: &mut Vec<Warning>) {
    let h = &config.hidden_rooms;
    if !h.size.is_valid() {
        warnings.push(Warning::new(
            FieldId::HiddenRoomSize,
            format!(
                "Hidden room size range is invalid ({}-{} m)",
                h.size.min_m, h.size.max_m
            ),
        ));
    }
}

fn check_walls(config: &LevelConfig, warnings: &mut Vec<Warning>) {
    let w = &config.walls;
    if w.thickness_m <= 0.0 {
        warnings.push(Warning::new(
            FieldId::WallThickness,
            "Wall thickness must be positive",
        ));
    }
    if w.gate_width_m < w.thickness_m {
        warnings.push(Warning::new(
            FieldId::WallGateWidth,
            format!(
                "Gate openings ({:.1} m) should be at least as wide as the wall ({:.1} m) to pierce it",
                w.gate_width_m, w.thickness_m
            ),
        ));
    }
    if w.street_width_m <= 0.0 {
        warnings.push(Warning::new(
            FieldId::WallStreetWidth,
            "Street width must be positive",
        ));
    }
    if w.min_block_m <= 0.0 {
        warnings.push(Warning::new(
            FieldId::WallMinBlock,
            "Min block size must be positive",
        ));
    }
}

fn check_mobs(config: &LevelConfig, warnings: &mut Vec<Warning>) {
    let mobs = &config.mobs;
    if mobs.types.is_empty() {
        if mobs.budget > 0.0 {
            warnings.push(Warning::new(
                FieldId::MobBudget,
                "Mob budget is set but no mob types are defined",
            ));
        }
        return;
    }
    if mobs.budget <= 0.0 {
        warnings.push(Warning::new(
            FieldId::MobBudget,
            "Mob budget must be positive when mob types are defined",
        ));
    }
    if mobs.max_entities == 0 {
        warnings.push(Warning::new(
            FieldId::MobMaxEntities,
            "Max entities is 0: no mobs will be placed",
        ));
    }
    for (i, mob) in mobs.types.iter().enumerate() {
        if mob.price <= 0.0 {
            warnings.push(Warning::new(
                FieldId::MobTypePrice(i),
                format!("Mob '{}' price must be positive", mob.name),
            ));
        }
        if mob.size_m <= 0.0 {
            warnings.push(Warning::new(
                FieldId::MobTypeSize(i),
                format!("Mob '{}' size must be positive", mob.name),
            ));
        }
    }
    if mobs.balance.len() != mobs.types.len() {
        warnings.push(Warning::new(
            FieldId::MobBalance,
            format!(
                "Balance has {} entries for {} mob types",
                mobs.balance.len(),
                mobs.types.len()
            ),
        ));
    } else {
        let sum: f32 = mobs.balance.iter().sum();
        if mobs.balance.iter().any(|&w| w < 0.0) || (sum - 100.0).abs() > 0.5 {
            warnings.push(Warning::new(
                FieldId::MobBalance,
                format!("Mob balance should sum to 100 (is {:.1})", sum),
            ));
        }
    }
}

fn check_gates(config: &LevelConfig, warnings: &mut Vec<Warning>) {
    let gates = &config.gates;
    for (i, gate) in gates.iter().enumerate() {
        if !(0.0..=360.0).contains(&gate.direction_deg) {
            warnings.push(Warning::new(
                FieldId::GateDirection(i),
                format!(
                    "Gate #{} direction must be in 0..=360 degrees",
                    i + 1
                ),
            ));
        }
    }
    for i in 0..gates.len() {
        for j in (i + 1)..gates.len() {
            let diff = (gates[i].direction_deg - gates[j].direction_deg).rem_euclid(360.0);
            let diff = diff.min(360.0 - diff);
            if diff < 5.0 {
                warnings.push(Warning::new(
                    FieldId::GateDirection(j),
                    format!(
                        "Gate #{} direction is only {:.1} degrees from gate #{}",
                        j + 1,
                        diff,
                        i + 1
                    ),
                ));
            }
        }
    }
}

/// Feasibility pre-check (goals.md): estimate whether everything the user requested
/// can fit into the walkable area BEFORE generating. Placement achieves roughly 70 %
/// coverage of the area it claims, which the PACKING factor accounts for. The
/// walkable area is shared sequentially: structures first, then props, interactives
/// and hidden rooms see what is left.
fn check_feasibility(config: &LevelConfig, warnings: &mut Vec<Warning>) {
    const PACKING: f32 = 0.7;
    let map_area = config.map.width_m * config.map.height_m;
    if map_area <= 0.0 {
        return;
    }
    let mut free = map_area * config.map.walkable_pct.clamp(0.0, 100.0) / 100.0;

    let kinds = config.template_mix.kinds();
    let zone_area = map_area / kinds.len().max(1) as f32;
    for kind in config.template_mix.kinds() {
        let s = config.structure(kind);
        let need = zone_area * s.density_pct.clamp(0.0, 100.0) / 100.0 / PACKING;
        if need > free {
            warnings.push(Warning::new(
                FieldId::StructureDensity(kind),
                format!(
                    "Feasibility: {kind} structures need ~{need:.0} m² but only ~{free:.0} m² \
                     of walkable area remains — lower the density or raise the walkable %"
                ),
            ));
        }
        free = (free - need).max(0.0);
    }

    let avg_prop = ((config.props.size.min_m + config.props.size.max_m) * 0.5).max(0.01);
    let need = config.props.quantity as f32 * avg_prop * avg_prop / PACKING;
    if need > free {
        warnings.push(Warning::new(
            FieldId::PropsQuantity,
            format!(
                "Feasibility: {} props need ~{need:.0} m² but only ~{free:.0} m² walkable \
                 area remains — lower the quantity or the density",
                config.props.quantity
            ),
        ));
    }
    free = (free - need).max(0.0);

    for (i, it) in config.interactive.iter().enumerate() {
        let need = it.quantity as f32 * it.size_m.max(0.0) * it.size_m.max(0.0) / PACKING;
        if need > free {
            warnings.push(Warning::new(
                FieldId::InteractiveQuantity(i),
                format!(
                    "Feasibility: interactive '{}' needs ~{need:.0} m² but only ~{free:.0} m² \
                     walkable area remains",
                    it.name
                ),
            ));
        }
        free = (free - need).max(0.0);
    }

    let avg_room = ((config.hidden_rooms.size.min_m + config.hidden_rooms.size.max_m) * 0.5).max(0.01);
    let need = config.hidden_rooms.count as f32 * avg_room * avg_room / PACKING;
    if need > free {
        warnings.push(Warning::new(
            FieldId::HiddenRoomCount,
            format!(
                "Feasibility: {} hidden rooms need ~{need:.0} m² but only ~{free:.0} m² \
                 walkable area remains",
                config.hidden_rooms.count
            ),
        ));
    }
}

/// All object-type colors compared pairwise; any duplicate is blocking.
fn check_colors(config: &LevelConfig, warnings: &mut Vec<Warning>) {
    struct Use {
        color: ColorRgb,
        field: FieldId,
        label: String,
    }

    let mut uses: Vec<Use> = Vec::new();
    for kind in config.template_mix.kinds() {
        let s = config.structure(kind);
        uses.push(Use {
            color: s.color,
            field: FieldId::StructureColor(kind),
            label: format!("{kind} structure"),
        });
    }
    uses.push(Use {
        color: config.props.color,
        field: FieldId::PropsColor,
        label: "Props".to_owned(),
    });
    for (i, it) in config.interactive.iter().enumerate() {
        let label = if it.name.is_empty() {
            format!("interactive #{}", i + 1)
        } else {
            it.name.clone()
        };
        uses.push(Use {
            color: it.color,
            field: FieldId::InteractiveColor(i),
            label,
        });
    }
    if config.hidden_rooms.count > 0 {
        uses.push(Use {
            color: config.hidden_rooms.color_room,
            field: FieldId::HiddenRoomColor,
            label: "Hidden room".to_owned(),
        });
        uses.push(Use {
            color: config.hidden_rooms.color_path,
            field: FieldId::HiddenRoomPathColor,
            label: "Hidden room path".to_owned(),
        });
    }
    for (i, mob) in config.mobs.types.iter().enumerate() {
        let label = if mob.name.is_empty() {
            format!("mob #{}", i + 1)
        } else {
            mob.name.clone()
        };
        uses.push(Use {
            color: mob.color,
            field: FieldId::MobTypeColor(i),
            label,
        });
    }
    for (i, gate) in config.gates.iter().enumerate() {
        uses.push(Use {
            color: gate.color,
            field: FieldId::GateColor(i),
            label: format!("gate #{}", i + 1),
        });
    }
    // Wall objects exist only when a walled template kind is actually in the mix.
    if config.template_mix.kinds().iter().any(|&k| config.walls.includes(k)) {
        uses.push(Use {
            color: config.walls.color,
            field: FieldId::WallColor,
            label: "Walls".to_owned(),
        });
        uses.push(Use {
            color: config.walls.road_color,
            field: FieldId::WallRoadColor,
            label: "Roads".to_owned(),
        });
    }

    for i in 0..uses.len() {
        for j in (i + 1)..uses.len() {
            if uses[i].color == uses[j].color {
                let msg = format!(
                    "Duplicate color #{:02X}{:02X}{:02X} used by both {} and {}",
                    uses[i].color[0],
                    uses[i].color[1],
                    uses[i].color[2],
                    uses[i].label,
                    uses[j].label
                );
                warnings.push(Warning::blocking(uses[i].field, msg.clone()));
                warnings.push(Warning::blocking(uses[j].field, msg));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Gate;

    #[test]
    fn default_config_has_no_warnings() {
        assert!(validate(&LevelConfig::default()).is_empty());
    }

    #[test]
    fn detects_duplicate_colors_as_blocking() {
        let mut config = LevelConfig::default();
        config.props.color = config.interactive[0].color;
        let warnings = validate(&config);
        assert!(warnings.iter().any(|w| w.blocking && w.field == Some(FieldId::PropsColor)));
        assert!(
            warnings
                .iter()
                .any(|w| w.blocking && w.field == Some(FieldId::InteractiveColor(0)))
        );

        // Room and path colors also participate.
        config.props.color = [1, 2, 3];
        config.hidden_rooms.color_room = [1, 2, 3];
        config.hidden_rooms.count = 0;
        assert!(validate(&config).iter().all(|w| !w.blocking));
        config.hidden_rooms.count = 2;
        let warnings = validate(&config);
        assert!(warnings.iter().any(|w| w.blocking && w.field == Some(FieldId::HiddenRoomColor)));
    }

    #[test]
    fn gate_angle_warning() {
        let mut config = LevelConfig::default();
        config.gates.push(Gate {
            direction_deg: 92.0,
            ..Gate::default()
        });
        let warnings = validate(&config);
        assert!(warnings.iter().any(|w| w.field == Some(FieldId::GateDirection(1))));

        // Far apart and wrapped angles are fine.
        config.gates[1].direction_deg = 270.0;
        config.gates[0].direction_deg = 359.0;
        config.gates.push(Gate {
            direction_deg: 184.0,
            color: [10, 10, 240],
        });
        let warnings = validate(&config);
        assert!(!warnings.iter().any(|w| matches!(w.field, Some(FieldId::GateDirection(_)))));
    }

    #[test]
    fn feasibility_precheck_fires_on_overstuffed_config() {
        // Default config fits comfortably.
        assert!(validate(&LevelConfig::default()).is_empty());

        let mut config = LevelConfig {
            template_mix: crate::model::TemplateMix::Mix(
                crate::model::TemplateKind::Town,
                crate::model::TemplateKind::Forest,
            ),
            ..LevelConfig::default()
        };
        // Two structures at full density cannot fit the walkable area.
        for kind in config.template_mix.kinds() {
            config.structure_mut(kind).density_pct = 100.0;
        }
        let warnings = validate(&config);
        assert!(warnings
            .iter()
            .any(|w| matches!(w.field, Some(FieldId::StructureDensity(_)))));

        // A ridiculous prop quantity is caught too.
        let mut config = LevelConfig::default();
        config.props.quantity = 1_000_000;
        config.props.size = crate::model::SizeRange::new(2.0, 3.0);
        assert!(validate(&config)
            .iter()
            .any(|w| w.field == Some(FieldId::PropsQuantity)));
    }

    #[test]
    fn field_warnings_fire() {
        let mut config = LevelConfig::default();
        config.map.walkable_pct = 150.0;
        config.props.size = crate::model::SizeRange::new(5.0, 1.0);
        config.mobs.budget = 100.0;
        config.mobs.types.clear();
        config.template_mix = crate::model::TemplateMix::Mix(
            crate::model::TemplateKind::Cave,
            crate::model::TemplateKind::Cave,
        );
        let warnings = validate(&config);
        assert!(warnings.iter().any(|w| w.field == Some(FieldId::WalkablePct)));
        assert!(warnings.iter().any(|w| w.field == Some(FieldId::PropsSize)));
        assert!(warnings.iter().any(|w| w.field == Some(FieldId::MobBudget)));
        assert!(warnings.iter().any(|w| w.field == Some(FieldId::TemplateMix)));
    }
}
