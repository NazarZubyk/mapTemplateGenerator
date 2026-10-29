use rand::Rng;

use crate::generator::grid::{Grid, RectM};
use crate::level::{Category, ObjectDetails, PlacedObject};
use crate::model::LevelConfig;

/// Place hidden rooms (overlap-free) and connect each to the nearest walkable cell
/// with one L-shaped, one-cell-wide access path. Returns the room rects so the mob
/// phase can honor the mob-passable rule.
pub fn place(
    rng: &mut impl Rng,
    grid: &mut Grid,
    config: &LevelConfig,
    objects: &mut Vec<PlacedObject>,
    notes: &mut Vec<String>,
) -> Vec<RectM> {
    let settings = &config.hidden_rooms;
    let map = RectM::new(0.0, 0.0, config.map.width_m, config.map.height_m);
    let mut room_rects = Vec::new();

    for room_id in 1..=settings.count {
        let max = settings.size.max_m.max(settings.size.min_m);
        let w = rng.random_range(settings.size.min_m..=max);
        let h = rng.random_range(settings.size.min_m..=max);
        let Some(rect) = grid.random_free_rect(rng, w, h, map, 200) else {
            notes.push(format!("Hidden room #{room_id}: not enough free space to place it"));
            continue;
        };
        grid.mark(rect);
        objects.push(PlacedObject {
            category: Category::HiddenRoom,
            type_name: "Hidden room".to_owned(),
            color: settings.color_room,
            x_m: rect.x,
            y_m: rect.y,
            w_m: rect.w,
            h_m: rect.h,
            details: ObjectDetails::HiddenRoom { room_id },
        });
        room_rects.push(rect);
        connect_room(grid, rect, room_id, config, objects, notes);
    }
    room_rects
}

/// Carve an L-shaped path (horizontal leg at the room's middle row, then a vertical
/// leg at the target column) from the room to the nearest free cell.
fn connect_room(
    grid: &mut Grid,
    room: RectM,
    room_id: u32,
    config: &LevelConfig,
    objects: &mut Vec<PlacedObject>,
    notes: &mut Vec<String>,
) {
    let (c0, r0, c1, r1) = grid.cell_range(room);
    let mc = (c0 + c1) / 2;
    let mr = (r0 + r1) / 2;
    let Some((tc, tr)) = grid.nearest_free_cell((mc, mr), grid.cols.max(grid.rows)) else {
        notes.push(format!("Hidden room #{room_id}: no free cell to connect to"));
        return;
    };
    let in_room = |c: usize, r: usize| c >= c0 && c < c1 && r >= r0 && r < r1;

    let mut segments: Vec<RectM> = Vec::new();

    // Horizontal leg along row mr toward column tc.
    let step_x: isize = if tc >= mc { 1 } else { -1 };
    let (mut c, target_x) = (mc as isize, tc as isize);
    let mut min_c: Option<isize> = None;
    let mut max_c: Option<isize> = None;
    while c != target_x {
        c += step_x;
        if in_room(c as usize, mr) {
            continue;
        }
        grid.mark_path_cell(c as usize, mr);
        min_c = Some(min_c.map_or(c, |m| m.min(c)));
        max_c = Some(max_c.map_or(c, |m| m.max(c)));
    }
    if let (Some(mn), Some(mx)) = (min_c, max_c) {
        segments.push(RectM::new(
            mn as f32 * grid.cell_m,
            mr as f32 * grid.cell_m,
            (mx - mn + 1) as f32 * grid.cell_m,
            grid.cell_m,
        ));
    }

    // Vertical leg along column tc toward row tr.
    let step_y: isize = if tr >= mr { 1 } else { -1 };
    let (mut r, target_y) = (mr as isize, tr as isize);
    let mut min_r: Option<isize> = None;
    let mut max_r: Option<isize> = None;
    while r != target_y {
        r += step_y;
        if in_room(tc, r as usize) {
            continue;
        }
        grid.mark_path_cell(tc, r as usize);
        min_r = Some(min_r.map_or(r, |m| m.min(r)));
        max_r = Some(max_r.map_or(r, |m| m.max(r)));
    }
    if let (Some(mn), Some(mx)) = (min_r, max_r) {
        segments.push(RectM::new(
            tc as f32 * grid.cell_m,
            mn as f32 * grid.cell_m,
            grid.cell_m,
            (mx - mn + 1) as f32 * grid.cell_m,
        ));
    }

    if segments.is_empty() {
        notes.push(format!("Hidden room #{room_id}: access path is empty"));
        return;
    }
    for seg in segments {
        objects.push(PlacedObject {
            category: Category::HiddenRoomPath,
            type_name: "Hidden room path".to_owned(),
            color: config.hidden_rooms.color_path,
            x_m: seg.x,
            y_m: seg.y,
            w_m: seg.w,
            h_m: seg.h,
            details: ObjectDetails::HiddenRoomPath { room_id },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::structures::{build_zones, place as place_structures};
    use crate::generator::grid::WalkBudget;
    use crate::model::{TemplateKind, TemplateMix};
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    fn base_config() -> LevelConfig {
        let mut config = LevelConfig::default();
        config.hidden_rooms.count = 3;
        config.structures.values_mut().for_each(|s| s.density_pct = 0.0);
        config
    }

    #[test]
    fn rooms_and_paths_keep_the_map_connected() {
        let config = base_config();
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let mut grid = Grid::new(config.map.width_m, config.map.height_m, config.cell_size_m);
        let mut objects = Vec::new();
        let mut notes = Vec::new();
        let rooms = place(&mut rng, &mut grid, &config, &mut objects, &mut notes);

        assert_eq!(rooms.len(), 3);
        assert!(notes.is_empty(), "notes: {notes:?}");
        // One path with at least one segment per room.
        let paths = objects
            .iter()
            .filter(|o| o.category == Category::HiddenRoomPath)
            .count();
        assert!(paths >= 3, "expected >= 3 path segments, got {paths}");
        // Rooms + paths + open ground form ONE walkable component.
        assert_eq!(grid.connected_walkable_components(4), 1);
    }

    #[test]
    fn paths_do_not_cross_structures_and_connect_to_free_ground() {
        let mut config = LevelConfig::default();
        config.hidden_rooms.count = 5;
        config.template_mix = TemplateMix::Mix(TemplateKind::Town, TemplateKind::Forest);
        let mut rng = ChaCha8Rng::seed_from_u64(11);
        let mut grid = Grid::new(config.map.width_m, config.map.height_m, config.cell_size_m);
        let mut budget = WalkBudget::new(grid.total_cells(), config.map.walkable_pct);
        let mut objects = Vec::new();
        let mut notes = Vec::new();

        let zones = build_zones(&mut rng, &config);
        place_structures(&mut rng, &mut grid, &mut budget, &zones, &config, &mut objects, &mut notes);
        let solid_before: Vec<_> = objects.iter().map(|o| o.rect()).collect();
        let rooms = place(&mut rng, &mut grid, &config, &mut objects, &mut notes);

        // Path cells are reserved but walkable; the level must still be one region.
        assert_eq!(rooms.len(), 5);
        assert_eq!(grid.connected_walkable_components(4), 1);
        // Paths never overlap previously placed structures.
        for path in objects.iter().filter(|o| o.category == Category::HiddenRoomPath) {
            for solid in &solid_before {
                assert!(!path.rect().overlaps(solid), "path {path:?} overlaps {solid:?}");
            }
        }
    }
}
