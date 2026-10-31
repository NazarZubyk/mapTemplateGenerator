use image::{ImageBuffer, Rgb, Rgba};

use crate::level::{CaveStyle, Category, GeneratedLevel};

/// A level rasterized to the two exported images.
pub struct Rasterized {
    /// Human-facing colored map: every object type in its own color.
    pub map: ImageBuffer<Rgb<u8>, Vec<u8>>,
    /// Machine layer data. Object pixels: R = category layer id (1–7),
    /// instance id (1-based) = G + 256·B + 65536·(255 − A) — A = 255 up to
    /// 65535 objects. Swamp ground pixels
    /// (under no object): R = ground id (`GROUND_IDS`, 8–14), G = depth (0
    /// safe ground … 254, 255 deep water / thicket), B = island id low byte
    /// (0 off islands), A = 255. Cave ground pixels: R = ground id
    /// (`CAVE_GROUND_IDS`, 15–23), G = style zone id low byte, B = chamber
    /// id low byte (0 off the chambers), A = 255. Forest ground pixels: R =
    /// ground id (`ForestGround::layer_id`, 24–31), G = area id low byte,
    /// B = 1 walkable / 0 blocked, A = 255. Elsewhere all 0.
    pub layer: ImageBuffer<Rgba<u8>, Vec<u8>>,
    pub px_per_m: f32,
}

pub const GROUND: [u8; 3] = [225, 220, 205];
/// Object instance ids layer.png can hold (A = 255 − high byte, down to 1).
pub const MAX_INSTANCES: usize = 255 * 65536 - 1;
/// The region footprint from the shape node (town, swamp, forest, cave),
/// painted under every object.
pub const REGION: [u8; 3] = [204, 186, 158];
/// The outer wall ring, painted over the region.
pub const WALL: [u8; 3] = [97, 82, 70];

/// Walls between regions, painted over regions and roads.
pub const REGION_WALL: [u8; 3] = [128, 108, 90];
/// Roads, painted over the regions and under the wall.
pub const ROAD: [u8; 3] = [238, 228, 202];
/// Swamp ground ids on layer.png (R channel), after the object categories
/// (1–7). Stable: tools read them from spec.json `layer_categories`.
pub const GROUND_IDS: [(u8, &str); 7] = [
    (8, "island"),
    (9, "mud_path"),
    (10, "boardwalk"),
    (11, "shallow_water"),
    (12, "deep_mud"),
    (13, "deep_water"),
    (14, "thicket"),
];

/// Cave ground ids on layer.png (R channel), after the swamp ones.
pub const CAVE_GROUND_IDS: [(u8, &str); 9] = [
    (15, "cave_floor_natural"),
    (16, "cave_floor_hand_made"),
    (17, "tunnel"),
    (18, "rock"),
    (19, "cave_wall"),
    (20, "chasm"),
    (21, "rope_bridge"),
    (22, "shallow_pool"),
    (23, "deep_pool"),
];

/// Colors for the region split (cycled when there are more regions than
/// colors) — earthy tones distinct from the wall and ground.
const REGION_PALETTE: [[u8; 3]; 8] = [
    [196, 130, 115], // terracotta
    [150, 170, 120], // moss
    [165, 145, 185], // lilac
    [195, 170, 115], // sand
    [125, 160, 170], // teal
    [200, 150, 95],  // ochre
    [145, 125, 110], // umber
    [175, 175, 175], // stone gray
];

/// The color of region `id` (1-based).
pub fn region_color(id: u8) -> [u8; 3] {
    REGION_PALETTE[(id.max(1) as usize - 1) % REGION_PALETTE.len()]
}

/// Paint a cell mask onto the map image, one color per cell.
fn paint_mask(map: &mut image::ImageBuffer<image::Rgb<u8>, Vec<u8>>, m: &crate::level::CellMask, px_per_m: f32, w: u32, h: u32, color: [u8; 3]) {
    let cs = m.cell_size_m * px_per_m;
    if cs <= 0.0 {
        return;
    }
    for cy in 0..m.height {
        let y0 = (cy as f32 * cs).round() as i64;
        let y1 = ((cy + 1) as f32 * cs).round() as i64;
        for cx in 0..m.width {
            if !m.get(cx, cy) {
                continue;
            }
            let x0 = (cx as f32 * cs).round() as i64;
            let x1 = ((cx + 1) as f32 * cs).round() as i64;
            for py in y0.max(0)..y1.min(h as i64) {
                for px in x0.max(0)..x1.min(w as i64) {
                    map.put_pixel(px as u32, py as u32, image::Rgb(color));
                }
            }
        }
    }
}

/// Share of the style color mixed into the rock over a zone, so the zones
/// stay readable after the chambers are carved.
const ROCK_TINT: f32 = 0.22;

/// Per-cell colors of a cave map (width, height, cell size, colors): rock
/// tinted by the style zone under it, chamber floor in the chamber's style
/// color. Before the chambers node, the zones alone. None off caves.
pub fn cave_colors(level: &GeneratedLevel) -> Option<(usize, usize, f32, Vec<[u8; 3]>)> {
    let cave = level.cave.as_ref()?;
    let style = cave.style.as_ref()?;
    let st = &level.config.cave.style;
    let of = |s: CaveStyle| if s == CaveStyle::Natural { st.natural_color } else { st.handmade_color };
    let colors = match &cave.chambers {
        None => (0..style.ids.len()).map(|c| of(style.style_at(c))).collect(),
        Some(ch) => {
            let rock = level.config.cave.chambers.rock_color;
            let mix = |t: [u8; 3]| {
                let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * ROCK_TINT).round() as u8;
                [m(rock[0], t[0]), m(rock[1], t[1]), m(rock[2], t[2])]
            };
            let (rock_n, rock_h) = (mix(st.natural_color), mix(st.handmade_color));
            let tunnel = |c: usize| cave.tunnels.as_ref().is_some_and(|t| t.mask.cells[c]);
            if let Some(carve) = &cave.carve {
                // The final floor: chamber floor in its chamber's style, the
                // rest in its zone's; the wall band over the rock.
                let wall = level.config.cave.carve.wall_color;
                let hz = &level.config.cave.hazards;
                let hazard = |c: usize| {
                    let l = cave.hazards.as_ref()?;
                    if l.bridge.cells[c] {
                        Some(hz.bridge_color)
                    } else if l.chasm.cells[c] {
                        Some(hz.chasm_color)
                    } else if l.deep.cells[c] {
                        Some(hz.deep_color)
                    } else if l.shallow.cells[c] {
                        Some(hz.shallow_color)
                    } else {
                        None
                    }
                };
                return Some((
                    style.width,
                    style.height,
                    style.cell_size_m,
                    (0..style.ids.len())
                        .map(|c| {
                            if let Some(color) = hazard(c) {
                                color
                            } else if carve.floor.cells[c] {
                                match ch.ids[c] {
                                    0 => of(style.style_at(c)),
                                    id => of(ch.chambers[id as usize - 1].style),
                                }
                            } else if carve.wall.cells[c] {
                                wall
                            } else if style.style_at(c) == CaveStyle::Natural {
                                rock_n
                            } else {
                                rock_h
                            }
                        })
                        .collect(),
                ));
            }
            (0..style.ids.len())
                .map(|c| match ch.ids[c] {
                    // Tunnels take the zone's style: the carving node shapes
                    // them by it.
                    0 if tunnel(c) => of(style.style_at(c)),
                    0 if style.style_at(c) == CaveStyle::Natural => rock_n,
                    0 => rock_h,
                    id => of(ch.chambers[id as usize - 1].style),
                })
                .collect()
        }
    };
    Some((style.width, style.height, style.cell_size_m, colors))
}

/// Cave legend entries (color, name): the style zones before the chambers
/// node; then the chamber floors of each style that has one, and rock.
pub fn cave_legend(level: &GeneratedLevel) -> Vec<([u8; 3], String)> {
    let Some(cave) = &level.cave else { return Vec::new() };
    let st = &level.config.cave.style;
    let kinds = [(CaveStyle::Natural, st.natural_color), (CaveStyle::HandMade, st.handmade_color)];
    let mut out = Vec::new();
    match (&cave.chambers, &cave.style) {
        (Some(ch), _) => {
            for (kind, color) in kinds {
                let in_tunnels = cave.tunnels.as_ref().zip(cave.style.as_ref()).is_some_and(|(t, st)| {
                    t.mask.cells.iter().enumerate().any(|(c, &on)| on && st.style_at(c) == kind)
                });
                if ch.chambers.iter().any(|c| c.style == kind) || in_tunnels {
                    out.push((color, format!("{} floor", kind.label())));
                }
            }
            if let Some(l) = &cave.hazards {
                let hz = &level.config.cave.hazards;
                for (mask, color, name) in [
                    (&l.chasm, hz.chasm_color, "chasm"),
                    (&l.bridge, hz.bridge_color, "rope bridge"),
                    (&l.shallow, hz.shallow_color, "shallow pool"),
                    (&l.deep, hz.deep_color, "deep pool"),
                ] {
                    if mask.count_true() > 0 {
                        out.push((color, name.to_owned()));
                    }
                }
            }
            if cave.carve.as_ref().is_some_and(|c| c.wall.count_true() > 0) {
                out.push((level.config.cave.carve.wall_color, "cave wall".to_owned()));
            }
            out.push((level.config.cave.chambers.rock_color, "rock".to_owned()));
        }
        (None, Some(style)) => {
            for (kind, color) in kinds {
                if style.zones.iter().any(|z| z.style == kind) {
                    out.push((color, kind.label().to_owned()));
                }
            }
        }
        (None, None) => {}
    }
    out
}

/// Per-cell colors of a forest map (width, height, cell size, colors):
/// one flat color per ground kind ([`ForestGround::color`]) — walkable kinds
/// light, blocked kinds dark — with no tint, texture or shading, so every
/// color on the map means exactly one thing. None off forests.
///
/// [`ForestGround::color`]: crate::level::ForestGround::color
pub fn forest_colors(level: &GeneratedLevel) -> Option<(usize, usize, f32, Vec<[u8; 3]>)> {
    use crate::level::ForestGround as G;
    let forest = level.forest.as_ref()?;
    let l = forest.areas.as_ref()?;
    let ground = forest.ground().filter(|g| g.len() == l.ids.len());
    let colors = (0..l.ids.len())
        .map(|c| match ground {
            Some(g) => G::from_code(g[c]).color(),
            None => l.area_at(c).character.color(),
        })
        .collect();
    Some((l.width, l.height, l.cell_size_m, colors))
}

/// Forest legend entries (color, name): every ground kind on the map,
/// each marked walkable or blocked.
pub fn forest_legend(level: &GeneratedLevel) -> Vec<([u8; 3], String)> {
    let Some(f) = level.forest.as_ref() else { return Vec::new() };
    let Some(g) = f.ground() else { return Vec::new() };
    crate::level::ForestGround::ALL
        .into_iter()
        .filter(|k| g.contains(&k.code()))
        .map(|k| (k.color(), format!("{} ({})", k.label(), if k.walkable() { "walkable" } else { "blocked" })))
        .collect()
}

/// Per-cell ground colors of the kinds that paint their own ground (cave,
/// forest).
pub fn ground_colors(level: &GeneratedLevel) -> Option<(usize, usize, f32, Vec<[u8; 3]>)> {
    cave_colors(level).or_else(|| forest_colors(level))
}

/// Legend entries of [`ground_colors`].
pub fn ground_legend(level: &GeneratedLevel) -> Vec<([u8; 3], String)> {
    let mut out = cave_legend(level);
    out.extend(forest_legend(level));
    out
}

pub fn rasterize(level: &GeneratedLevel, px_per_m: f32) -> Rasterized {
    let px_per_m = px_per_m.max(0.1);
    let w = ((level.config.map.width_m * px_per_m).round() as u32).max(1);
    let h = ((level.config.map.height_m * px_per_m).round() as u32).max(1);
    let mut map = ImageBuffer::from_pixel(w, h, Rgb(GROUND));
    let mut layer = ImageBuffer::from_pixel(w, h, Rgba([0, 0, 0, 0]));

    // The region mask is the base layer; with a region split each region
    // gets its own color. The wall ring paints over it.
    if let Some(regions) = &level.regions {
        let m = &regions.map;
        let cs = m.cell_size_m * px_per_m;
        if cs > 0.0 {
            for cy in 0..m.height {
                let y0 = (cy as f32 * cs).round() as i64;
                let y1 = ((cy + 1) as f32 * cs).round() as i64;
                for cx in 0..m.width {
                    let id = m.ids[cy * m.width + cx];
                    if id == 0 {
                        continue;
                    }
                    let x0 = (cx as f32 * cs).round() as i64;
                    let x1 = ((cx + 1) as f32 * cs).round() as i64;
                    for py in y0.max(0)..y1.min(h as i64) {
                        for px in x0.max(0)..x1.min(w as i64) {
                            map.put_pixel(px as u32, py as u32, Rgb(region_color(id)));
                        }
                    }
                }
            }
        }
    } else if let Some(shape) = &level.shape {
        paint_mask(&mut map, &shape.mask, px_per_m, w, h, REGION);
    }
    if let Some((cw, ch, cell, colors)) = ground_colors(level) {
        let cs = cell * px_per_m;
        for cy in 0..ch {
            let (y0, y1) = ((cy as f32 * cs).round() as u32, ((cy + 1) as f32 * cs).round() as u32);
            for cx in 0..cw {
                let (x0, x1) = ((cx as f32 * cs).round() as u32, ((cx + 1) as f32 * cs).round() as u32);
                for py in y0..y1.min(h) {
                    for px in x0..x1.min(w) {
                        map.put_pixel(px, py, Rgb(colors[cy * cw + cx]));
                    }
                }
            }
        }
    }
    if let Some(depth) = level.swamp.as_ref().and_then(|s| s.depth.as_ref()) {
        let d = &level.config.swamp.depth;
        for (mask, color) in [(&depth.deep, d.deep_color), (&depth.mud, d.mud_color), (&depth.shallow, d.shallow_color), (&depth.thicket, d.thicket_color)] {
            paint_mask(&mut map, mask, px_per_m, w, h, color);
        }
    }
    if let Some(islands) = level.swamp.as_ref().and_then(|s| s.islands.as_ref()) {
        paint_mask(&mut map, &islands.mask(), px_per_m, w, h, level.config.swamp.islands.color);
    }
    if let Some(paths) = level.swamp.as_ref().and_then(|s| s.paths.as_ref()) {
        paint_mask(&mut map, &paths.mud, px_per_m, w, h, level.config.swamp.paths.mud_color);
        paint_mask(&mut map, &paths.boardwalk, px_per_m, w, h, level.config.swamp.paths.boardwalk_color);
    }
    if let Some(roads) = &level.roads {
        paint_mask(&mut map, &roads.mask, px_per_m, w, h, ROAD);
    }
    if let Some(region_walls) = &level.region_walls {
        paint_mask(&mut map, &region_walls.mask, px_per_m, w, h, REGION_WALL);
    }
    if let Some(walls) = &level.walls {
        paint_mask(&mut map, &walls.mask, px_per_m, w, h, WALL);
    }
    if let Some(rooms) = &level.hidden_rooms {
        paint_mask(&mut map, &rooms.wall_mask, px_per_m, w, h, WALL);
    }

    // Swamp ground on layer.png, under the objects: terrain id + depth.
    if let Some(s) = &level.swamp
        && let (Some(isl), Some(paths), Some(depth)) = (&s.islands, &s.paths, &s.depth)
    {
        let cs = isl.cell_size_m * px_per_m;
        for cy in 0..isl.height {
            for cx in 0..isl.width {
                let c = cy * isl.width + cx;
                let id = if isl.ids[c] > 0 {
                    8
                } else if paths.boardwalk.cells[c] {
                    10
                } else if paths.mud.cells[c] {
                    9
                } else if depth.shallow.cells[c] {
                    11
                } else if depth.mud.cells[c] {
                    12
                } else if depth.thicket.cells[c] {
                    14
                } else {
                    13
                };
                let px = Rgba([id, depth.depth[c], (isl.ids[c] & 0xFF) as u8, 255]);
                let (x0, x1) = ((cx as f32 * cs).round() as u32, ((cx + 1) as f32 * cs).round() as u32);
                let (y0, y1) = ((cy as f32 * cs).round() as u32, ((cy + 1) as f32 * cs).round() as u32);
                for py in y0..y1.min(h) {
                    for qx in x0..x1.min(w) {
                        layer.put_pixel(qx, py, px);
                    }
                }
            }
        }
    }

    // Cave ground on layer.png, under the objects: ground id, zone, chamber.
    if let Some(cave) = &level.cave
        && let (Some(style), Some(ch), Some(carve)) = (&cave.style, &cave.chambers, &cave.carve)
    {
        let id_of = |name: &str| CAVE_GROUND_IDS.iter().find(|g| g.1 == name).map_or(0, |g| g.0);
        let cs = style.cell_size_m * px_per_m;
        for cy in 0..style.height {
            for cx in 0..style.width {
                let c = cy * style.width + cx;
                let hz = cave.hazards.as_ref();
                let name = if hz.is_some_and(|l| l.bridge.cells[c]) {
                    "rope_bridge"
                } else if hz.is_some_and(|l| l.chasm.cells[c]) {
                    "chasm"
                } else if hz.is_some_and(|l| l.deep.cells[c]) {
                    "deep_pool"
                } else if hz.is_some_and(|l| l.shallow.cells[c]) {
                    "shallow_pool"
                } else if carve.floor.cells[c] && ch.ids[c] == 0 {
                    "tunnel"
                } else if carve.floor.cells[c] {
                    if style.style_at(c) == CaveStyle::Natural { "cave_floor_natural" } else { "cave_floor_hand_made" }
                } else if carve.wall.cells[c] {
                    "cave_wall"
                } else {
                    "rock"
                };
                let px = Rgba([id_of(name), (style.ids[c] & 0xFF) as u8, (ch.ids[c] & 0xFF) as u8, 255]);
                let (x0, x1) = ((cx as f32 * cs).round() as u32, ((cx + 1) as f32 * cs).round() as u32);
                let (y0, y1) = ((cy as f32 * cs).round() as u32, ((cy + 1) as f32 * cs).round() as u32);
                for py in y0..y1.min(h) {
                    for qx in x0..x1.min(w) {
                        layer.put_pixel(qx, py, px);
                    }
                }
            }
        }
    }

    // Forest ground on layer.png, under the objects: ground id, area id,
    // walkable flag.
    if let Some(forest) = &level.forest
        && let (Some(areas), Some(p), Some(ground)) = (&forest.areas, &forest.passages, forest.ground())
    {
        let cs = p.cell_size_m * px_per_m;
        for cy in 0..p.height {
            for cx in 0..p.width {
                let c = cy * p.width + cx;
                let g = crate::level::ForestGround::from_code(ground[c]);
                let px = Rgba([g.layer_id(), (areas.ids[c] & 0xFF) as u8, g.walkable() as u8, 255]);
                let (x0, x1) = ((cx as f32 * cs).round() as u32, ((cx + 1) as f32 * cs).round() as u32);
                let (y0, y1) = ((cy as f32 * cs).round() as u32, ((cy + 1) as f32 * cs).round() as u32);
                for py in y0..y1.min(h) {
                    for qx in x0..x1.min(w) {
                        layer.put_pixel(qx, py, px);
                    }
                }
            }
        }
    }

    // Draw in stored order: later objects paint over earlier ones. Objects
    // may be rotated and compound: every pixel in the bounding box of all
    // corners is tested against the object's rects (pixel centers). A
    // compound house is ONE instance id on layer.png.
    for (idx, o) in level.objects.iter().enumerate() {
        // Instance id = G + 256·B + 65536·(255 − A): A is 255 for the first
        // 65535 objects (as always), lower only on huge maps.
        let instance = (idx + 1).min(MAX_INSTANCES) as u32;
        // Every rect of the object (main + wings of a compound house).
        let corners: Vec<(f32, f32)> = o.all_corners().into_iter().flatten().collect();
        let fold = |f: fn(f32, f32) -> f32, init: f32, pick: fn(&(f32, f32)) -> f32| {
            corners.iter().map(pick).fold(init, f)
        };
        let x0 = (fold(f32::min, f32::MAX, |c| c.0) * px_per_m).floor() as i64;
        let y0 = (fold(f32::min, f32::MAX, |c| c.1) * px_per_m).floor() as i64;
        let x1 = (fold(f32::max, f32::MIN, |c| c.0) * px_per_m).ceil() as i64;
        let y1 = (fold(f32::max, f32::MIN, |c| c.1) * px_per_m).ceil() as i64;
        for y in y0.max(0)..y1.min(h as i64) {
            for x in x0.max(0)..x1.min(w as i64) {
                if !o.contains((x as f32 + 0.5) / px_per_m, (y as f32 + 0.5) / px_per_m) {
                    continue;
                }
                let (px, py) = (x as u32, y as u32);
                map.put_pixel(px, py, Rgb(o.color));
                layer.put_pixel(
                    px,
                    py,
                    Rgba([
                        o.category.layer_id(),
                        (instance & 0xFF) as u8,
                        ((instance >> 8) & 0xFF) as u8,
                        255 - (instance >> 16) as u8,
                    ]),
                );
            }
        }
    }

    Rasterized { map, layer, px_per_m }
}

/// Lowercase category name used in the exported specification.
pub fn category_name(c: Category) -> &'static str {
    match c {
        Category::Structure => "structure",
        Category::Prop => "prop",
        Category::Interactive => "interactive",
        Category::HiddenRoom => "hidden_room",
        Category::HiddenRoomPath => "hidden_room_path",
        Category::MobSpawn => "mob_spawn",
        Category::Gate => "gate",
    }
}

/// Layer names indexed by the layer.png R value (index 0 = empty ground):
/// the object categories (1–7), then the swamp ground ids (8–14), so
/// consumers can map R values without knowing this crate.
pub fn layer_categories() -> Vec<&'static str> {
    let mut names = vec!["ground"];
    for id in 1..=Category::ALL.len() as u8 {
        let name = Category::ALL
            .iter()
            .find(|c| c.layer_id() == id)
            .map(|c| category_name(*c))
            .unwrap_or("unknown");
        names.push(name);
    }
    names.extend(GROUND_IDS.iter().map(|&(_, n)| n));
    names.extend(CAVE_GROUND_IDS.iter().map(|&(_, n)| n));
    names.extend(crate::level::ForestGround::ALL.iter().map(|g| g.layer_name()));
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::PlacedObject;

    fn level_with_one_object() -> GeneratedLevel {
        GeneratedLevel {
            config: crate::model::LevelConfig::test_64(),
            seed: 42,
            objects: vec![PlacedObject {
                category: Category::Structure,
                type_name: "House".to_owned(),
                color: [10, 20, 30],
                x_m: 4.0,
                y_m: 4.0,
                w_m: 3.0,
                h_m: 2.0,
                angle_deg: 0.0,
                parts: Vec::new(),
                round: false,
            }],
            shape: None,
            walls: None,
            roads: None,
            region_walls: None,
            houses: None,
            props: None,
            interactive: None,
            hidden_rooms: None,
            mobs: None,
            gates: None,
            swamp: None,
            cave: None,
            forest: None,
            regions: None,
            noise_preview: None,
            notes: Vec::new(),
        }
    }

    #[test]
    fn region_mask_paints_under_objects() {
        let mut level = level_with_one_object();
        // Two 32 m cells cover the whole 64 m map.
        let mut mask = crate::level::CellMask::new(2, 2, 32.0);
        for y in 0..2 {
            for x in 0..2 {
                mask.set(x, y, true);
            }
        }
        level.shape = Some(crate::level::LevelShape {
            mask,
            stats: crate::level::ShapeStats {
                raw_blobs: 1,
                speckle_cells_removed: 0,
                bridges: 0,
                hole_cells_filled: 0,
                sections: 1,
                section_bridges: 0,
                coverage_pct: 100.0,
            },
        });
        let r = rasterize(&level, 2.0);
        // The house occupies pixels 8..15 × 8..12; everything else is town.
        assert_eq!(r.map.get_pixel(10, 10).0, [10, 20, 30]);
        assert_eq!(r.map.get_pixel(60, 60).0, REGION);
    }

    #[test]
    fn cave_layer_carries_ground_zone_and_chamber() {
        let mut c = crate::model::LevelConfig { kind: crate::model::MapKind::Cave, ..crate::model::LevelConfig::default() };
        c.map.width_m = 120.0;
        c.map.height_m = 120.0;
        c.cell_size_m = 1.0;
        c.cave.hazards.chasm_pct = 100.0;
        c.cave.hazards.pool_pct = 100.0;
        let level = crate::generator::generate(&c, 2);
        let r = rasterize(&level, 1.0);
        let cave = level.cave.as_ref().unwrap();
        let (st, ch) = (cave.style.as_ref().unwrap(), cave.chambers.as_ref().unwrap());
        let names = layer_categories();
        let mut seen = std::collections::HashSet::new();
        for (x, y, px) in r.layer.enumerate_pixels() {
            assert_eq!(px.0[3], 255, "every cave pixel carries data");
            let k = y as usize * st.width + x as usize;
            let name = names[px.0[0] as usize];
            seen.insert(name);
            if CAVE_GROUND_IDS.iter().any(|g| g.1 == name) {
                assert_eq!(px.0[1] as u16, st.ids[k] & 0xFF, "zone id in G");
                assert_eq!(px.0[2] as u16, ch.ids[k] & 0xFF, "chamber id in B");
            }
            if name == "tunnel" {
                assert_eq!(ch.ids[k], 0);
            }
        }
        for want in ["rock", "cave_wall", "tunnel", "chasm", "shallow_pool", "deep_pool", "gate", "mob_spawn"] {
            assert!(seen.contains(want), "no {want} on the cave layer: {seen:?}");
        }
        assert!(seen.contains("cave_floor_natural") || seen.contains("cave_floor_hand_made"));
    }

    #[test]
    fn swamp_layer_carries_ground_and_depth() {
        let mut c = crate::model::LevelConfig { kind: crate::model::MapKind::Swamp, ..crate::model::LevelConfig::default() };
        c.map.width_m = 120.0;
        c.map.height_m = 120.0;
        c.cell_size_m = 1.0;
        let level = crate::generator::generate(&c, 3);
        let r = rasterize(&level, 1.0);
        let s = level.swamp.as_ref().unwrap();
        let (isl, d) = (s.islands.as_ref().unwrap(), s.depth.as_ref().unwrap());
        let names = layer_categories();
        let mut seen = std::collections::HashSet::new();
        for (x, y, px) in r.layer.enumerate_pixels() {
            assert_eq!(px.0[3], 255, "every swamp pixel carries data");
            let id = px.0[0];
            seen.insert(id);
            let c = y as usize * isl.width + x as usize;
            match names[id as usize] {
                "island" => {
                    assert_eq!(px.0[2] as u16, isl.ids[c] & 0xFF);
                    assert_eq!(px.0[1], 0, "islands are safe ground");
                }
                "deep_water" | "thicket" => assert_eq!(px.0[1], 255),
                "shallow_water" | "deep_mud" => {
                    assert!((1..=254).contains(&px.0[1]));
                    assert_eq!(px.0[1], d.depth[c]);
                }
                _ => {}
            }
        }
        for want in ["island", "mud_path", "shallow_water", "deep_mud", "deep_water", "gate", "structure"] {
            let id = names.iter().position(|n| *n == want).unwrap() as u8;
            assert!(seen.contains(&id), "no {want} pixel on layer.png");
        }
    }

    #[test]
    fn map_carries_color_layer_carries_category_and_instance() {
        let level = level_with_one_object();
        let r = rasterize(&level, 4.0);
        assert_eq!(r.map.width(), 256);
        let px = r.map.get_pixel(20, 20);
        assert_eq!(px.0, [10, 20, 30]);
        let lp = r.layer.get_pixel(20, 20);
        assert_eq!(lp.0[0], Category::Structure.layer_id());
        assert_eq!(u16::from(lp.0[1]) | (u16::from(lp.0[2]) << 8), 1);
        // Ground outside the object stays empty.
        assert_eq!(r.map.get_pixel(0, 0).0, GROUND);
        assert_eq!(r.layer.get_pixel(0, 0).0[0], 0);
    }

    #[test]
    fn instance_ids_past_65535_use_the_alpha_byte() {
        // 70 000 one-pixel objects on a 280 × 256 px map (1 px/m).
        let mut level = level_with_one_object();
        level.config.map.width_m = 280.0;
        level.config.map.height_m = 256.0;
        level.objects = (0..70_000)
            .map(|i| PlacedObject {
                category: Category::Prop,
                type_name: "p".to_owned(),
                color: [1, 2, 3],
                x_m: (i % 280) as f32,
                y_m: (i / 280) as f32,
                w_m: 1.0,
                h_m: 1.0,
                angle_deg: 0.0,
                parts: Vec::new(),
                round: false,
            })
            .collect();
        let r = rasterize(&level, 1.0);
        let id = |x: u32, y: u32| {
            let p = r.layer.get_pixel(x, y).0;
            p[1] as u32 + 256 * p[2] as u32 + 65536 * (255 - p[3] as u32)
        };
        assert_eq!(id(0, 0), 1);
        assert_eq!(r.layer.get_pixel(0, 0).0[3], 255, "small ids keep A = 255");
        let last = 69_999u32;
        assert_eq!(id(last % 280, last / 280), 70_000);
        assert_eq!(r.layer.get_pixel(last % 280, last / 280).0[3], 254);
    }

    #[test]
    fn later_objects_paint_over_earlier_ones() {
        let mut level = level_with_one_object();
        level.objects.insert(
            0,
            PlacedObject {
                category: Category::Prop,
                type_name: "Big".to_owned(),
                color: [255, 0, 0],
                x_m: 0.0,
                y_m: 0.0,
                w_m: 64.0,
                h_m: 64.0,
                angle_deg: 0.0,
                parts: Vec::new(),
                round: false,
            },
        );
        let r = rasterize(&level, 4.0);
        assert_eq!(r.map.get_pixel(20, 20).0, [10, 20, 30]); // house drawn last
        assert_eq!(r.map.get_pixel(0, 0).0, [255, 0, 0]);
    }

    #[test]
    fn forest_layer_carries_ground_area_and_walkable() {
        let mut c = crate::model::LevelConfig::starting();
        c.map.width_m = 120.0;
        c.map.height_m = 100.0;
        c.cell_size_m = 1.0;
        let level = crate::generator::generate(&c, 3);
        let r = rasterize(&level, 1.0);
        let f = level.forest.as_ref().unwrap();
        let (areas, p, ground) = (f.areas.as_ref().unwrap(), f.passages.as_ref().unwrap(), f.ground().unwrap());
        let names = layer_categories();
        let mut kinds = std::collections::HashSet::new();
        for (x, y, px) in r.layer.enumerate_pixels() {
            let k = y as usize * p.width + x as usize;
            let name = names[px.0[0] as usize];
            if name.starts_with("forest_") {
                let g = crate::level::ForestGround::from_code(ground[k]);
                assert_eq!(name, g.layer_name());
                assert_eq!(px.0[1] as u16, areas.ids[k] & 0xFF, "area id in G");
                assert_eq!(px.0[2] == 1, g.walkable(), "walkable flag in B");
                kinds.insert(name);
                // map.png: the flat color of the kind.
                assert_eq!(r.map.get_pixel(x, y).0, g.color());
            } else {
                assert!(Category::ALL.iter().any(|c| category_name(*c) == name), "a pixel off forest ground must be an object: {name}");
            }
        }
        for want in ["forest_open", "forest_floor", "forest_thicket"] {
            assert!(kinds.contains(want), "{kinds:?}");
        }
    }

    #[test]
    fn layer_categories_indexed_by_layer_id() {
        let cats = layer_categories();
        assert_eq!(cats[0], "ground");
        assert_eq!(cats[Category::Structure.layer_id() as usize], "structure");
        assert_eq!(cats.len(), Category::ALL.len() + 1 + GROUND_IDS.len() + CAVE_GROUND_IDS.len() + crate::level::ForestGround::ALL.len());
        for (id, name) in GROUND_IDS.into_iter().chain(CAVE_GROUND_IDS) {
            assert_eq!(cats[id as usize], name, "ground ids follow the categories");
        }
        for g in crate::level::ForestGround::ALL {
            assert_eq!(cats[g.layer_id() as usize], g.layer_name(), "forest ids follow the cave ones");
        }
    }
}
