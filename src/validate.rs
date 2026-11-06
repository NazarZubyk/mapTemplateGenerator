use crate::model::{LevelConfig, MapKind, Warning};

/// Grid cells (map / cell size, both ways) above which generation is
/// blocked: 8192 × 8192 cells, about 5 GB of memory.
pub const MAX_CELLS: f64 = 8192.0 * 8192.0;
/// Above this a "big map" warning (minutes, gigabytes): 4096 × 4096 cells.
pub const BIG_CELLS: f64 = 4096.0 * 4096.0;
/// Peak memory per cell measured on every map kind (about 75 B), rounded up.
const BYTES_PER_CELL: f64 = 80.0;

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
    if config.cell_size_m <= 0.0 {
        warnings.push(Warning::new("Cell size must be positive"));
    }
    // Size: memory and time grow with the number of grid cells.
    if config.cell_size_m > 0.0 && config.map.width_m > 0.0 && config.map.height_m > 0.0 {
        let cells = (config.map.width_m / config.cell_size_m).ceil() as f64 * (config.map.height_m / config.cell_size_m).ceil() as f64;
        let gb = cells * BYTES_PER_CELL / 1e9;
        if cells > MAX_CELLS {
            warnings.push(Warning {
                field: Some("map.width_m".to_owned()),
                message: format!(
                    "Map too big: {:.0} million grid cells would need about {gb:.0} GB of memory (limit {:.0} million) — use a bigger cell size or a smaller map",
                    cells / 1e6,
                    MAX_CELLS / 1e6
                ),
                blocking: true,
            });
        } else if cells > BIG_CELLS {
            warnings.push(Warning {
                field: Some("map.width_m".to_owned()),
                message: format!(
                    "Big map: {:.0} million grid cells — generation can take minutes and about {gb:.1} GB of memory; a bigger cell size makes it faster",
                    cells / 1e6
                ),
                blocking: false,
            });
        }
    }
    // A mixed map has no nodes of its own past the map regions: each map
    // region is checked as the map of its kind it is.
    if config.kind == MapKind::Mixed {
        mixed(config, &mut warnings);
        return warnings;
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
    colors(config, &mut warnings);
    nodes(config, &mut warnings);
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

/// Everything drawn in its own color on map.png for a map of this kind:
/// (what it is, the setting, the color). Fixed colors have no setting.
fn color_uses(config: &LevelConfig) -> Vec<(String, Option<String>, [u8; 3])> {
    let mut uses: Vec<(String, Option<String>, [u8; 3])> = Vec::new();
    let mut set = |what: &str, field: &str, color: [u8; 3]| uses.push((what.to_owned(), Some(field.to_owned()), color));
    match config.kind {
        MapKind::Town => set("houses", "houses.color", config.houses.color),
        MapKind::Swamp => {
            let d = &config.swamp.depth;
            set("shallow water", "swamp.depth.shallow_color", d.shallow_color);
            set("deep mud", "swamp.depth.mud_color", d.mud_color);
            set("deep water", "swamp.depth.deep_color", d.deep_color);
            set("thicket", "swamp.depth.thicket_color", d.thicket_color);
            set("islands", "swamp.islands.color", config.swamp.islands.color);
            set("mud paths", "swamp.paths.mud_color", config.swamp.paths.mud_color);
            set("boardwalks", "swamp.paths.boardwalk_color", config.swamp.paths.boardwalk_color);
            set("trees", "swamp.trees.color", config.swamp.trees.color);
            set("trees in water", "swamp.trees.water_color", config.swamp.trees.water_color);
        }
        MapKind::Cave => {
            let c = &config.cave;
            set("rock", "cave.chambers.rock_color", c.chambers.rock_color);
            set("natural floor", "cave.style.natural_color", c.style.natural_color);
            set("hand-made floor", "cave.style.handmade_color", c.style.handmade_color);
            set("cave wall", "cave.carve.wall_color", c.carve.wall_color);
            set("chasms", "cave.hazards.chasm_color", c.hazards.chasm_color);
            set("rope bridges", "cave.hazards.bridge_color", c.hazards.bridge_color);
            set("shallow pools", "cave.hazards.shallow_color", c.hazards.shallow_color);
            set("deep pools", "cave.hazards.deep_color", c.hazards.deep_color);
        }
        MapKind::Forest => set("tree trunks", "forest.trees.color", config.forest.trees.color),
        MapKind::Mixed => {}
    }
    set("props", "props.color", config.props.color);
    for (i, it) in config.interactive.iter().enumerate() {
        set(&format!("interactive type \"{}\"", it.name), &format!("interactive[{i}].color"), it.color);
    }
    if config.hidden_rooms.count > 0 {
        set("hidden rooms", "hidden_rooms.color_room", config.hidden_rooms.color_room);
        set("hidden room paths", "hidden_rooms.color_path", config.hidden_rooms.color_path);
    }
    for (i, m) in config.mobs.types.iter().enumerate() {
        set(&format!("mob type \"{}\"", m.name), &format!("mobs.types[{i}].color"), m.color);
    }
    if !config.gates.gates.is_empty() {
        set("gates", "gates.color", config.gates.color);
    }
    let mut fixed = |what: &str, color: [u8; 3]| uses.push((what.to_owned(), None, color));
    match config.kind {
        MapKind::Town => {
            fixed("roads", crate::render::ROAD);
            fixed("walls", crate::render::WALL);
            fixed("district walls", crate::render::REGION_WALL);
        }
        MapKind::Forest => {
            for g in crate::level::ForestGround::ALL {
                fixed(g.label(), g.color());
            }
            if config.hidden_rooms.count > 0 {
                fixed("hidden passages", crate::generator::forest::spawns::HIDDEN_PASSAGE_COLOR);
            }
        }
        MapKind::Cave if config.hidden_rooms.count > 0 => fixed("cracked walls", crate::generator::cave::spawns::CRACKED_WALL_COLOR),
        _ => {}
    }
    uses
}

/// Every color already in use for a map of this kind (its settings and
/// its fixed colors, plus the plain ground), for picking a new one.
pub fn used_colors(config: &LevelConfig) -> Vec<[u8; 3]> {
    let mut v: Vec<[u8; 3]> = color_uses(config).into_iter().map(|u| u.2).collect();
    v.push(crate::render::GROUND);
    v
}

/// A saturated color far from every used one (golden-angle hues, the
/// candidate with the largest distance to its nearest used color wins).
pub fn distinct_color(used: &[[u8; 3]]) -> [u8; 3] {
    let dist = |a: [u8; 3], b: [u8; 3]| (0..3).map(|k| (a[k] as i32 - b[k] as i32).pow(2)).sum::<i32>();
    (0..36)
        .map(|k| hsv((k as f32 * 137.508) % 360.0, if k % 2 == 0 { 0.75 } else { 0.55 }, if k % 3 == 0 { 0.6 } else { 0.85 }))
        .max_by_key(|&c| used.iter().map(|&u| dist(c, u)).min().unwrap_or(i32::MAX))
        .unwrap_or([200, 60, 50])
}

fn hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8]
}

/// Two different things in one color make map.png ambiguous: blocking
/// until one of them changes (docs/goals.md "Color conflicts").
fn colors(config: &LevelConfig, out: &mut Vec<Warning>) {
    let uses = color_uses(config);
    for (i, a) in uses.iter().enumerate() {
        for b in &uses[i + 1..] {
            if a.2 != b.2 || (a.1.is_none() && b.1.is_none()) {
                continue;
            }
            let [r, g, bl] = a.2;
            out.push(Warning {
                // Point at a setting the user can change.
                field: b.1.clone().or_else(|| a.1.clone()),
                message: format!("Duplicate color #{r:02X}{g:02X}{bl:02X}: {} and {} — pick another color for one of them", a.0, b.0),
                blocking: true,
            });
        }
    }
}

/// A min / max pair the generator swaps when min is the bigger one.
fn range(out: &mut Vec<Warning>, f: &str, what: &str, min: f32, max: f32) {
    if min > max {
        out.push(field(f, format!("{what}: max ({max}) is smaller than min ({min}) — the two are swapped at generation time")));
    }
}

/// The town's own nodes after the regions (roads, district walls, houses)
/// and the nodes every kind shares (props, interactive objects, hidden
/// rooms, mobs, gates): settings that cannot work as asked.
fn nodes(config: &LevelConfig, out: &mut Vec<Warning>) {
    if config.kind == MapKind::Town {
        let r = &config.roads;
        if r.width_m <= 0.0 {
            out.push(field("roads.width_m", "Road width must be positive".to_owned()));
        }
        if r.min_width_m > r.width_m {
            out.push(field("roads.min_width_m", format!("Quietest road width ({} m) is wider than the main road width ({} m) — every road gets the main width", r.min_width_m, r.width_m)));
        }
        let rw = &config.region_walls;
        if config.regions.count > 1 {
            if rw.thickness_m <= 0.0 {
                out.push(field("region_walls.thickness_m", "District wall thickness must be positive".to_owned()));
            }
            range(out, "region_walls.segment_max_m", "District wall segments", rw.segment_min_m, rw.segment_max_m);
        }
        let h = &config.houses;
        if h.density_pct > 0.0 {
            if h.size_min_m <= 0.0 {
                out.push(field("houses.size_min_m", "House size min must be positive".to_owned()));
            }
            range(out, "houses.size_max_m", "House size", h.size_min_m, h.size_max_m);
            range(out, "houses.wing_max_pct", "House wing size", h.wing_min_pct, h.wing_max_pct);
        }
    }
    let p = &config.props;
    if p.quantity > 0 {
        if p.size_min_m <= 0.0 {
            out.push(field("props.size_min_m", "Prop size min must be positive".to_owned()));
        }
        range(out, "props.size_max_m", "Prop size", p.size_min_m, p.size_max_m);
    }
    for (i, it) in config.interactive.iter().enumerate() {
        if it.quantity == 0 {
            continue;
        }
        let what = format!("Interactive type \"{}\"", it.name);
        if it.size_min_m <= 0.0 {
            out.push(field(&format!("interactive[{i}].size_min_m"), format!("{what}: size min must be positive")));
        }
        range(out, &format!("interactive[{i}].size_max_m"), &format!("{what} size"), it.size_min_m, it.size_max_m);
        range(out, &format!("interactive[{i}].group_max"), &format!("{what} group size"), it.group_min as f32, it.group_max as f32);
        if it.group_min.min(it.group_max) > it.quantity {
            out.push(field(
                &format!("interactive[{i}].group_min"),
                format!("{what}: the smallest group ({}) is bigger than the quantity ({}) — one smaller group is placed", it.group_min.min(it.group_max), it.quantity),
            ));
        }
    }
    let hr = &config.hidden_rooms;
    if hr.count > 0 {
        if hr.size_min_m <= 0.0 {
            out.push(field("hidden_rooms.size_min_m", "Hidden room size min must be positive".to_owned()));
        }
        range(out, "hidden_rooms.size_max_m", "Hidden room size", hr.size_min_m, hr.size_max_m);
        if hr.path_width_m <= 0.0 {
            out.push(field("hidden_rooms.path_width_m", "Hidden room path width must be positive".to_owned()));
        }
    }
    let m = &config.mobs;
    let wants_mobs = m.budget > 0.0 || m.min_entities > 0 || m.min_per_100m2 > 0.0;
    if wants_mobs && m.types.is_empty() {
        out.push(field("mobs.types", "Mobs have a budget but no mob types — add a mob type or set the budget to 0".to_owned()));
    }
    if wants_mobs && !m.types.is_empty() {
        if m.types.iter().all(|t| t.balance_pct <= 0.0) {
            out.push(field("mobs.types", "Every mob type has a 0 % balance share — no mobs are placed".to_owned()));
        }
        if m.max_entities == 0 {
            out.push(field("mobs.max_entities", "Mob entity cap is 0 — no mobs are placed".to_owned()));
        }
        if m.min_amount == crate::model::NodeAmount::Count && m.min_entities > m.max_entities {
            out.push(field("mobs.min_entities", format!("Mob minimum ({}) is above the entity cap ({}) — the cap wins", m.min_entities, m.max_entities)));
        }
        range(out, "mobs.pack_max", "Mob pack size", m.pack_min as f32, m.pack_max as f32);
        for (i, t) in m.types.iter().enumerate() {
            if t.price <= 0.0 {
                out.push(field(&format!("mobs.types[{i}].price"), format!("Mob type \"{}\": price must be positive", t.name)));
            }
            if t.size_m <= 0.0 {
                out.push(field(&format!("mobs.types[{i}].size_m"), format!("Mob type \"{}\": size must be positive", t.name)));
            }
        }
    }
    gates(config, out);
    feasibility(config, out);
}

/// Share of the free ground the props, interactive objects and hidden
/// rooms may ask for before a warning: they need room around them too.
const FEASIBLE_SHARE: f32 = 0.4;

/// A rough pre-check before generating: the ground the props, interactive
/// objects and hidden rooms ask for, against the free ground of the map
/// (a town: its area minus the houses' share; other kinds: half the map).
fn feasibility(config: &LevelConfig, out: &mut Vec<Warning>) {
    let area = config.map.width_m.max(0.0) * config.map.height_m.max(0.0);
    if area <= 0.0 {
        return;
    }
    let free = if config.kind == MapKind::Town {
        area * (config.shape.coverage_target_pct / 100.0).clamp(0.0, 1.0) * (1.0 - config.houses.density_pct / 100.0).clamp(0.0, 1.0)
    } else {
        area * 0.5
    };
    let mean_sq = |a: f32, b: f32| {
        let m = (a.max(0.0) + b.max(0.0)) / 2.0;
        m * m
    };
    let p = &config.props;
    let props = p.quantity as f32 * mean_sq(p.size_min_m, p.size_max_m);
    let interactive: f32 = config.interactive.iter().map(|t| t.quantity as f32 * mean_sq(t.size_min_m, t.size_max_m)).sum();
    let hr = &config.hidden_rooms;
    let rooms = hr.count as f32 * mean_sq(hr.size_min_m, hr.size_max_m);
    let asked = props + interactive + rooms;
    if asked > free * FEASIBLE_SHARE {
        let biggest = [(props, "props.quantity"), (interactive, "interactive"), (rooms, "hidden_rooms.count")]
            .into_iter()
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map_or("props.quantity", |b| b.1);
        out.push(field(
            biggest,
            format!(
                "Props, interactive objects and hidden rooms ask for about {asked:.0} m², {:.0} % of the free ground (about {free:.0} m²) — expect fewer than asked; lower the amounts or sizes, or use a bigger map",
                asked / free.max(1.0) * 100.0
            ),
        ));
    }
}

/// Gates whose openings would overlap: their rays from the map center
/// meet the edge (or the town wall) closer than two openings apart.
/// Swamps have their own check (gate islands, see `swamp`).
fn gates(config: &LevelConfig, out: &mut Vec<Warning>) {
    let g = &config.gates;
    if config.kind == MapKind::Swamp || config.map.width_m <= 0.0 || config.map.height_m <= 0.0 {
        return;
    }
    // About where the gates sit: most of the way out from the center.
    let r = 0.4 * config.map.width_m.min(config.map.height_m);
    let need = 2.0 * g.opening_m.max(g.road_width_m).max(1.0);
    for (a, x) in g.gates.iter().enumerate() {
        for (b, y) in g.gates.iter().enumerate().skip(a + 1) {
            let d = (x.direction_deg - y.direction_deg).rem_euclid(360.0);
            let gap = d.min(360.0 - d);
            let apart = r * gap.to_radians();
            if apart < need {
                out.push(field(
                    &format!("gates.gates[{b}].direction_deg"),
                    format!("Gates #{} and #{} are only {gap:.0}° apart — about {apart:.0} m on this map, so their openings ({} m) run into each other", a + 1, b + 1, g.opening_m),
                ));
            }
        }
    }
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

/// Blending widths (docs/blending-plan.md): not negative, and not so deep
/// the zone covers the whole of the smaller region of the pair.
fn blending(config: &LevelConfig, rows: &[crate::model::MixedRegion], total: f32, area: f32, out: &mut Vec<Warning>) {
    let b = &config.mixed.blending;
    // (field, label, depth, one side's kinds, the other side's kinds, the
    // sides whose regions the zone covers).
    const FT: &[MapKind] = &[MapKind::Forest, MapKind::Town];
    const FS: &[MapKind] = &[MapKind::Forest, MapKind::Swamp];
    const BELOW: &[MapKind] = &[MapKind::Forest, MapKind::Town, MapKind::Swamp];
    for (f, label, width, side_a, side_b, covered) in [
        ("mixed.blending.forest_town_m", "Forest ↔ Town", b.forest_town_m, &[MapKind::Forest][..], &[MapKind::Town][..], FT),
        ("mixed.blending.forest_swamp_m", "Forest ↔ Swamp", b.forest_swamp_m, &[MapKind::Forest][..], &[MapKind::Swamp][..], FS),
        ("mixed.blending.scree_m", "Scree", b.scree_m, BELOW, &[MapKind::Cave][..], BELOW),
        ("mixed.blending.town_swamp_m", "Town → Swamp", b.town_swamp_m, &[MapKind::Town][..], &[MapKind::Swamp][..], &[MapKind::Swamp][..]),
    ] {
        if width < 0.0 {
            out.push(field(f, format!("Blending {label}: the depth cannot be negative")));
            continue;
        }
        // The smallest region the zone covers, about this many meters across.
        let across = rows
            .iter()
            .filter(|r| covered.contains(&r.kind))
            .map(|r| (area * r.share_pct.max(0.1) / total).sqrt())
            .fold(f32::INFINITY, f32::min);
        let both = rows.iter().any(|r| side_a.contains(&r.kind)) && rows.iter().any(|r| side_b.contains(&r.kind));
        if both && width > across / 2.0 {
            out.push(field(f, format!("Blending {label}: {width:.0} m reaches past the middle of a {across:.0} m region — the transition covers all of it")));
        }
    }
    if b.embankment_m < 0.0 {
        out.push(field("mixed.blending.embankment_m", "Blending embankments: the thickness cannot be negative".to_owned()));
    } else if b.embankment_m > 4.0 {
        out.push(field("mixed.blending.embankment_m", format!("Blending embankments: {:.1} m is a thick quay — it eats the town floor along the river", b.embankment_m)));
    }
}

/// Mixed maps (docs/mixed-plan.md): the region list, then every region's
/// own settings as a map of its kind, each warning prefixed with the region.
fn mixed(config: &LevelConfig, warnings: &mut Vec<Warning>) {
    let rows = &config.mixed.regions;
    if rows.is_empty() {
        warnings.push(Warning { field: Some("mixed.regions".to_owned()), message: "Mixed map has no map regions — add one in the Map regions node".to_owned(), blocking: false });
        return;
    }
    let max = crate::generator::mixed::MAX_REGIONS;
    // Every region keeps a whole-grid result: memory grows with them.
    if config.cell_size_m > 0.0 {
        let cells = (config.map.width_m / config.cell_size_m).ceil().max(0.0) as f64 * (config.map.height_m / config.cell_size_m).ceil().max(0.0) as f64;
        let n = rows.len().min(max) as f64;
        let load = cells * (1.0 + n) / 2.0;
        let gb = load * BYTES_PER_CELL / 1e9;
        if load > MAX_CELLS {
            warnings.push(Warning {
                field: Some("mixed.regions".to_owned()),
                message: format!("Mixed map too big: {:.0} million cells × {} regions would need about {gb:.0} GB — use a bigger cell size, a smaller map or fewer regions", cells / 1e6, n),
                blocking: true,
            });
        } else if load > BIG_CELLS && cells <= BIG_CELLS {
            warnings.push(Warning {
                field: Some("mixed.regions".to_owned()),
                message: format!("Big mixed map: each of the {n} regions keeps a whole-map result — about {gb:.1} GB and minutes; a bigger cell size makes it faster"),
                blocking: false,
            });
        }
    }
    if rows.len() > max {
        warnings.push(Warning { field: Some("mixed.regions".to_owned()), message: format!("{} map regions — only the first {max} are used", rows.len()), blocking: false });
    }
    let total: f32 = rows.iter().map(|r| r.share_pct.max(0.1)).sum();
    let area = config.map.width_m.max(0.0) * config.map.height_m.max(0.0);
    blending(config, rows, total, area, warnings);
    for (i, r) in rows.iter().enumerate().take(max) {
        let label = format!("Map region {} ({}, {})", i + 1, r.name, r.kind.region_label());
        // A region about this many meters across (as a square).
        let across = (area * r.share_pct.max(0.1) / total).sqrt();
        if across < 24.0 {
            warnings.push(Warning {
                field: Some(format!("mixed.regions[{i}].share_pct")),
                message: format!("{label} is only about {across:.0} m across — too small for its own layout; raise its share or the map size"),
                blocking: false,
            });
        }
        for mut w in validate(&r.resolved(config)) {
            // The map size is checked once, for the whole map; a region's
            // gates are its portals and map gates, not its gate list.
            if w.field.as_deref() == Some("map.width_m") || w.field.as_deref().is_some_and(|f| f.starts_with("gates.")) {
                continue;
            }
            w.message = format!("{label}: {}", w.message);
            w.field = w.field.map(|f| format!("mixed.regions[{i}].config.{f}"));
            warnings.push(w);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn mixed_regions_are_checked_with_their_own_settings() {
        let mut c = LevelConfig { kind: MapKind::Mixed, ..LevelConfig::default() };
        assert!(validate(&c).iter().any(|w| w.message.contains("no map regions")));
        c.mixed.regions = crate::model::MixedSettings::starter();
        c.mixed.regions.push(crate::model::MixedRegion::of(MapKind::Town, 0.5));
        c.mixed.regions[1].config.outer_wall.thickness_m = 0.0;
        let w = validate(&c);
        assert!(w.iter().any(|w| w.message.starts_with("Map region 2 (Town, Town): Wall thickness")), "{w:?}");
        assert!(w.iter().any(|w| w.message.contains("Map region 5") && w.message.contains("too small")), "{w:?}");
    }

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
        let forest = LevelConfig::of_kind(crate::model::MapKind::Forest);
        assert_eq!(forest.kind, MapKind::Forest);
        assert!(validate(&forest).is_empty(), "{:?}", validate(&forest));
    }

    #[test]
    fn map_size_warns_then_blocks() {
        let mut c = LevelConfig::of_kind(crate::model::MapKind::Forest);
        c.map.width_m = 2048.0;
        c.map.height_m = 2048.0;
        c.cell_size_m = 1.0;
        assert!(validate(&c).iter().all(|w| w.field.as_deref() != Some("map.width_m")), "4M cells is fine");
        c.map.width_m = 3000.0;
        c.map.height_m = 3000.0;
        c.cell_size_m = 0.5;
        let w: Vec<Warning> = validate(&c).into_iter().filter(|w| w.field.as_deref() == Some("map.width_m")).collect();
        assert!(w.len() == 1 && !w[0].blocking, "{w:?}");
        c.map.width_m = 8192.0;
        c.map.height_m = 8192.0;
        let w: Vec<Warning> = validate(&c).into_iter().filter(|w| w.field.as_deref() == Some("map.width_m")).collect();
        assert!(w.len() == 1 && w[0].blocking, "{w:?}");
    }

    #[test]
    fn forest_warnings_fire() {
        let mut c = LevelConfig::of_kind(crate::model::MapKind::Forest);
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
        config.map.width_m = -1.0;
        config.cell_size_m = 0.0;
        let warnings = validate(&config);
        assert_eq!(warnings.len(), 2);
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

    #[test]
    fn duplicate_colors_block() {
        let mut c = LevelConfig::test_64();
        c.mobs.types[1].color = c.mobs.types[0].color;
        let w = validate(&c);
        assert!(w.iter().any(|w| w.blocking && w.field.as_deref() == Some("mobs.types[1].color")), "{w:?}");
        // A fixed color (the roads) against a setting.
        let mut c = LevelConfig::test_64();
        c.props.color = crate::render::ROAD;
        assert!(validate(&c).iter().any(|w| w.blocking && w.field.as_deref() == Some("props.color")));
        // Only what the kind draws: house color is not a forest color.
        let mut f = LevelConfig::of_kind(MapKind::Forest);
        f.houses.color = f.props.color;
        assert!(validate(&f).iter().all(|w| !w.blocking), "{:?}", validate(&f));
    }

    #[test]
    fn mixed_regions_check_their_own_colors() {
        let mut c = LevelConfig::of_kind(MapKind::Mixed);
        let r = &mut c.mixed.regions[0];
        r.config.props.color = r.config.mobs.types[0].color;
        assert!(validate(&c).iter().any(|w| w.blocking && w.field.as_deref().is_some_and(|f| f.starts_with("mixed.regions[0]"))));
    }

    #[test]
    fn node_warnings_fire() {
        let mut c = LevelConfig::test_64();
        c.houses.size_min_m = 10.0;
        c.houses.size_max_m = 5.0;
        c.interactive[0].group_min = 50;
        c.interactive[0].group_max = 60;
        c.mobs.types[0].price = 0.0;
        c.mobs.pack_min = 6;
        c.mobs.pack_max = 2;
        c.gates.gates = vec![crate::model::GateSpec::at(10.0), crate::model::GateSpec::at(358.0)];
        let w = validate(&c);
        assert!(w.iter().all(|w| !w.blocking), "{w:?}");
        let fields: Vec<String> = w.into_iter().filter_map(|w| w.field).collect();
        for want in ["houses.size_max_m", "interactive[0].group_min", "mobs.types[0].price", "mobs.pack_max", "gates.gates[1].direction_deg"] {
            assert!(fields.iter().any(|f| f == want), "missing {want}: {fields:?}");
        }
        let mut c = LevelConfig::test_64();
        c.mobs.types.clear();
        c.mobs.combinations.clear();
        assert!(validate(&c).iter().any(|w| w.field.as_deref() == Some("mobs.types")));
    }

    #[test]
    fn overstuffed_map_warns() {
        let mut c = LevelConfig::test_64();
        c.props.quantity = 2000;
        c.props.size_max_m = 3.0;
        let w = validate(&c);
        assert!(w.iter().any(|w| !w.blocking && w.field.as_deref() == Some("props.quantity")), "{w:?}");
    }
}
