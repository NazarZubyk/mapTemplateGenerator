use image::{ImageBuffer, Rgb, Rgba};

use crate::generator::geometry;
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
        let rotated = o.angle_deg.abs() > 1e-3;
        let corners = rotated.then(|| {
            geometry::rot_rect_corners(
                (o.x_m + o.w_m / 2.0, o.y_m + o.h_m / 2.0),
                o.w_m,
                o.h_m,
                o.angle_deg,
            )
        });
        // Pixel loop over the AABB of the (possibly rotated) corners.
        let (x0, y0, x1, y1) = match &corners {
            Some(cs) => {
                let (min_x, min_y, max_x, max_y) = cs.iter().fold(
                    (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
                    |(a, b, c, d), p| (a.min(p.0), b.min(p.1), c.max(p.0), d.max(p.1)),
                );
                (
                    (min_x * px_per_m).floor() as i64,
                    (min_y * px_per_m).floor() as i64,
                    (max_x * px_per_m).ceil() as i64,
                    (max_y * px_per_m).ceil() as i64,
                )
            }
            None => (
                (o.x_m * px_per_m).round() as i64,
                (o.y_m * px_per_m).round() as i64,
                ((o.x_m + o.w_m) * px_per_m).round() as i64,
                ((o.y_m + o.h_m) * px_per_m).round() as i64,
            ),
        };
        for y in y0.max(0)..y1.min(h as i64) {
            for x in x0.max(0)..x1.min(w as i64) {
                let (px, py) = (x as u32, y as u32);
                if let Some(corners) = corners {
                    let p = ((x as f32 + 0.5) / px_per_m, (y as f32 + 0.5) / px_per_m);
                    if !geometry::point_in_polygon(p, &corners) {
                        continue;
                    }
                }
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
        Category::Wall => "wall",
        Category::Road => "road",
        Category::MobSpawn => "mob_spawn",
        Category::Gate => "gate",
    }
}

/// Category names indexed by layer id (index 0 = empty ground), so layer.png
/// consumers can map R-channel values without knowing this crate.
pub fn layer_categories() -> Vec<&'static str> {
    let mut names = vec!["ground"];
    for id in 1..=9u8 {
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

        // Every placed object keeps at least one pixel of its own color + layer
        // id, even though later objects may overpaint parts of it (e.g. a gate
        // sitting in the opening carved through the perimeter wall). Scan the
        // whole footprint; a full overpaint is impossible since placements are
        // overlap-free — EXCEPT roads, which deliberately overlap each other at
        // crossings: for them only the color must stay visible.
        for (idx, o) in level.objects.iter().enumerate() {
            let corners = geometry::rot_rect_corners(
                (o.x_m + o.w_m / 2.0, o.y_m + o.h_m / 2.0),
                o.w_m,
                o.h_m,
                o.angle_deg,
            );
            let (min_x, min_y, max_x, max_y) = corners.iter().fold(
                (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
                |(a, b, c, d), p| (a.min(p.0), b.min(p.1), c.max(p.0), d.max(p.1)),
            );
            let x0 = (min_x * px_per_m).floor().max(0.0) as u32;
            let y0 = (min_y * px_per_m).floor().max(0.0) as u32;
            let x1 = (max_x * px_per_m).ceil().min(r.map.width() as f32) as u32;
            let y1 = (max_y * px_per_m).ceil().min(r.map.height() as f32) as u32;
            let is_road = o.category == Category::Road;
            let mut found = false;
            'scan: for y in y0..y1.max(y0 + 1) {
                for x in x0..x1.max(x0 + 1) {
                    if r.map.get_pixel(x, y).0 != o.color {
                        continue;
                    }
                    let layer_px = r.layer.get_pixel(x, y);
                    let instance = u16::from(layer_px.0[1]) | (u16::from(layer_px.0[2]) << 8);
                    if is_road || instance == (idx + 1) as u16 {
                        assert_eq!(layer_px.0[0], o.category.layer_id(), "layer id: {o:?}");
                        assert_eq!(layer_px.0[3], 255);
                        found = true;
                        break 'scan;
                    }
                }
            }
            assert!(found, "no pixel of {o:?} kept its color");
        }
    }
}
