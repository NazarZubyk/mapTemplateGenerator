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

    // Draw in stored order: structures first, gates/mob spawns last on top.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::generate;
    use crate::model::LevelConfig;

    #[test]
    fn map_carries_object_colors_layer_carries_ids() {
        let config = LevelConfig::default();
        let level = generate(&config, 42);
        let px_per_m = 4.0;
        let r = rasterize(&level, px_per_m);

        assert_eq!(r.map.width(), (config.map.width_m * px_per_m) as u32);
        assert_eq!(r.map.height(), (config.map.height_m * px_per_m) as u32);

        // Every placed object paints at least its center pixel in both images,
        // and layer ids stay consistent with the category.
        for (idx, o) in level.objects.iter().enumerate() {
            let cx = ((o.x_m + o.w_m / 2.0) * px_per_m) as u32;
            let cy = ((o.y_m + o.h_m / 2.0) * px_per_m) as u32;
            let map_px = r.map.get_pixel(cx, cy);
            assert_eq!(map_px.0, o.color, "map color mismatch for {o:?}");
            let layer_px = r.layer.get_pixel(cx, cy);
            assert_eq!(layer_px.0[0], o.category.layer_id());
            assert_eq!(
                u16::from(layer_px.0[1]) | (u16::from(layer_px.0[2]) << 8),
                (idx + 1) as u16
            );
            assert_eq!(layer_px.0[3], 255);
        }
    }
}
