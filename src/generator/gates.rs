use crate::generator::grid::{Grid, RectM};
use crate::level::{Category, ObjectDetails, PlacedObject};
use crate::model::LevelConfig;

/// Border point at `deg` degrees from the map center. Angle system: 0° = east
/// (right edge), 90° = north (top edge), increasing counterclockwise on screen.
pub fn border_point(map_w: f32, map_h: f32, deg: f32) -> (f32, f32) {
    let (cx, cy) = (map_w / 2.0, map_h / 2.0);
    let rad = deg.to_radians();
    let (dx, dy) = (rad.cos(), -rad.sin()); // screen y points down
    let mut t = f32::INFINITY;
    if dx.abs() > 1e-6 {
        t = t.min((if dx > 0.0 { map_w - cx } else { -cx }) / dx);
    }
    if dy.abs() > 1e-6 {
        t = t.min((if dy > 0.0 { map_h - cy } else { -cy }) / dy);
    }
    if !t.is_finite() {
        return (cx, cy);
    }
    (cx + t * dx, cy + t * dy)
}

/// Place gate markers: 1 m squares centered on the border point. Gates are pure
/// points on the layout (per goals.md), so they don't occupy grid cells.
pub fn place(
    grid: &Grid,
    config: &LevelConfig,
    objects: &mut Vec<PlacedObject>,
    notes: &mut Vec<String>,
) {
    let map_w = config.map.width_m;
    let map_h = config.map.height_m;
    let size = 1.0_f32.min(map_w).min(map_h);
    for (i, gate) in config.gates.iter().enumerate() {
        let (px, py) = border_point(map_w, map_h, gate.direction_deg);
        let rect = RectM::new(
            (px - size / 2.0).clamp(0.0, map_w - size),
            (py - size / 2.0).clamp(0.0, map_h - size),
            size,
            size,
        );
        if !grid.area_free(rect) {
            notes.push(format!(
                "Gate #{} overlaps a placed object at the map border",
                i + 1
            ));
        }
        objects.push(PlacedObject {
            category: Category::Gate,
            type_name: "Gate".to_owned(),
            color: gate.color,
            x_m: rect.x,
            y_m: rect.y,
            w_m: rect.w,
            h_m: rect.h,
            details: ObjectDetails::Gate {
                direction_deg: gate.direction_deg,
            },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_points_hit_edges_at_cardinal_directions() {
        let (w, h) = (64.0_f32, 32.0_f32);
        let (x, y) = border_point(w, h, 0.0);
        assert!((x - w).abs() < 1e-4 && (y - h / 2.0).abs() < 1e-4, "{x} {y}");
        let (x, y) = border_point(w, h, 90.0);
        assert!((x - w / 2.0).abs() < 1e-4 && y.abs() < 1e-4, "{x} {y}");
        let (x, y) = border_point(w, h, 180.0);
        assert!(x.abs() < 1e-4 && (y - h / 2.0).abs() < 1e-4, "{x} {y}");
        let (x, y) = border_point(w, h, 270.0);
        assert!((x - w / 2.0).abs() < 1e-4 && (y - h).abs() < 1e-4, "{x} {y}");
    }
}
