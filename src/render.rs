use image::{ImageBuffer, Rgb, Rgba};

use crate::level::{Category, GeneratedLevel};

/// A level rasterized to the two exported images.
pub struct Rasterized {
    /// Human-facing colored map: every object type in its own color.
    pub map: ImageBuffer<Rgb<u8>, Vec<u8>>,
    /// Machine layer data: R = category layer id, G/B = instance id (1-based,
    /// 0 = empty ground), A = 255 where an object exists.
    pub layer: ImageBuffer<Rgba<u8>, Vec<u8>>,
    pub px_per_m: f32,
}

pub const GROUND: [u8; 3] = [225, 220, 205];
/// The region footprint from the shape node (town, swamp, forest, cave),
/// painted under every object.
pub const REGION: [u8; 3] = [204, 186, 158];
/// The outer wall ring, painted over the region.
pub const WALL: [u8; 3] = [97, 82, 70];

/// Roads, painted over the regions and under the wall.
pub const ROAD: [u8; 3] = [238, 228, 202];
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
    if let Some(roads) = &level.roads {
        paint_mask(&mut map, &roads.mask, px_per_m, w, h, ROAD);
    }
    if let Some(walls) = &level.walls {
        paint_mask(&mut map, &walls.mask, px_per_m, w, h, WALL);
    }

    // Draw in stored order: later objects paint over earlier ones.
    for (idx, o) in level.objects.iter().enumerate() {
        let instance = (idx + 1) as u16;
        let x0 = (o.x_m * px_per_m).round() as i64;
        let y0 = (o.y_m * px_per_m).round() as i64;
        let x1 = ((o.x_m + o.w_m) * px_per_m).round() as i64;
        let y1 = ((o.y_m + o.h_m) * px_per_m).round() as i64;
        for y in y0.max(0)..y1.min(h as i64) {
            for x in x0.max(0)..x1.min(w as i64) {
                let (px, py) = (x as u32, y as u32);
                map.put_pixel(px, py, Rgb(o.color));
                layer.put_pixel(
                    px,
                    py,
                    Rgba([
                        o.category.layer_id(),
                        (instance & 0xFF) as u8,
                        (instance >> 8) as u8,
                        255,
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

/// Category names indexed by layer id (index 0 = empty ground), so layer.png
/// consumers can map R-channel values without knowing this crate.
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
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::PlacedObject;

    fn level_with_one_object() -> GeneratedLevel {
        GeneratedLevel {
            config: crate::model::LevelConfig::default(),
            seed: 42,
            objects: vec![PlacedObject {
                category: Category::Structure,
                type_name: "House".to_owned(),
                color: [10, 20, 30],
                x_m: 4.0,
                y_m: 4.0,
                w_m: 3.0,
                h_m: 2.0,
            }],
            shape: None,
            walls: None,
            roads: None,
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
            },
        );
        let r = rasterize(&level, 4.0);
        assert_eq!(r.map.get_pixel(20, 20).0, [10, 20, 30]); // house drawn last
        assert_eq!(r.map.get_pixel(0, 0).0, [255, 0, 0]);
    }

    #[test]
    fn layer_categories_indexed_by_layer_id() {
        let cats = layer_categories();
        assert_eq!(cats[0], "ground");
        assert_eq!(cats[Category::Structure.layer_id() as usize], "structure");
        assert_eq!(cats.len(), Category::ALL.len() + 1);
    }
}
