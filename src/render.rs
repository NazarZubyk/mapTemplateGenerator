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

pub fn rasterize(level: &GeneratedLevel, px_per_m: f32) -> Rasterized {
    let px_per_m = px_per_m.max(0.1);
    let w = ((level.config.map.width_m * px_per_m).round() as u32).max(1);
    let h = ((level.config.map.height_m * px_per_m).round() as u32).max(1);
    let mut map = ImageBuffer::from_pixel(w, h, Rgb(GROUND));
    let mut layer = ImageBuffer::from_pixel(w, h, Rgba([0, 0, 0, 0]));

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
            notes: Vec::new(),
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
