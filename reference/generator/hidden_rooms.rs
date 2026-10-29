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
    let regions_before = grid.connected_walkable_components(1);
    let mut room_rects = Vec::new();

    for room_id in 1..=settings.count {
        let max = settings.size.max_m.max(settings.size.min_m);
        let w = rng.random_range(settings.size.min_m..=max);
        let h = rng.random_range(settings.size.min_m..=max);
        // Try a few spots: rooms must avoid roads and not seal off pockets.
        let mut placed = None;
        for _ in 0..5 {
            match grid.random_free_building_rect(rng, w, h, map, 40) {
                Some(r) if grid.placement_keeps_regions(r, regions_before) => {
                    placed = Some(r);
                    break;
                }
                Some(_) => {}
                None => break,
            }
        }
        let Some(rect) = placed else {
            notes.push(format!("Hidden room #{room_id}: not enough free space to place it"));
            continue;
        };
        objects.push(PlacedObject {
            category: Category::HiddenRoom,
            type_name: "Hidden room".to_owned(),
            color: settings.color_room,
            x_m: rect.x,
            y_m: rect.y,
            w_m: rect.w,
            h_m: rect.h,
            angle_deg: 0.0,
            details: ObjectDetails::HiddenRoom { room_id },
        });
        room_rects.push(rect);
        connect_room(grid, rect, room_id, config, objects, notes);
    }
    room_rects
}

/// Carve an access path from the room to the main walkable area: a BFS from the
/// room's free neighbors to the largest walkable component. The corridor routes
/// through open ground and may tunnel through walls (secret passage), but never
/// through houses, rocks or roads.
fn connect_room(
    grid: &mut Grid,
    room: RectM,
    room_id: u32,
    config: &LevelConfig,
    objects: &mut Vec<PlacedObject>,
    notes: &mut Vec<String>,
) {
    let total = grid.total_cells();
    // Label the current walkable components; the largest one is the main area.
    let mut label = vec![0u32; total];
    let mut sizes: Vec<u32> = vec![0];
    for start in 0..total {
        if label[start] != 0 || !bfs_passable(grid, start) {
            continue;
        }
        let id = sizes.len() as u32;
        let mut stack = vec![start];
        label[start] = id;
        let mut count = 0u32;
        while let Some(u) = stack.pop() {
            count += 1;
            for v in neighbors(grid, u) {
                if label[v] == 0 && bfs_passable(grid, v) {
                    label[v] = id;
                    stack.push(v);
                }
            }
        }
        sizes.push(count);
    }
    let Some(main) = (1..sizes.len()).max_by_key(|&i| sizes[i]) else {
        notes.push(format!("Hidden room #{room_id}: no walkable area to connect to"));
        return;
    };
    let main = main as u32;

    // BFS from the free cells around the room toward the main area.
    let (c0, r0, c1, r1) = grid.cell_range(room);
    let mut parent: Vec<Option<usize>> = vec![None; total];
    let mut queue = std::collections::VecDeque::new();
    for r in r0..r1 {
        for c in c0..c1 {
            for v in neighbors(grid, r * grid.cols + c) {
                if bfs_passable(grid, v) && parent[v].is_none() {
                    parent[v] = Some(v); // sources are their own parents
                    queue.push_back(v);
                }
            }
        }
    }
    let mut goal: Option<usize> = None;
    while let Some(u) = queue.pop_front() {
        if label[u] == main {
            goal = Some(u);
            break;
        }
        for v in neighbors(grid, u) {
            if bfs_passable(grid, v) && parent[v].is_none() {
                parent[v] = Some(u);
                queue.push_back(v);
            }
        }
    }
    let Some(goal) = goal else {
        notes.push(format!(
            "Hidden room #{room_id}: no access to the main area could be carved"
        ));
        return;
    };

    // Trace goal back to its source; carve everything except the main-area cell.
    let mut carved: Vec<usize> = Vec::new();
    let mut v = goal;
    while parent[v] != Some(v) {
        carved.push(v);
        v = parent[v].expect("visited cell has a parent");
    }
    if carved.is_empty() {
        carved.push(goal); // room directly touches the main area: keep the doorway
    } else {
        carved.reverse(); // from the room outward
        carved.pop(); // the last cell is main ground, leave it unreserved
        if carved.is_empty() {
            carved.push(goal);
        }
    }
    for &cell in &carved {
        let (c, r) = (cell % grid.cols, cell / grid.cols);
        grid.mark_path_cell(c, r);
    }

    // Merge consecutive same-row cells into horizontal segment rects.
    let mut segments: Vec<RectM> = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    for &cell in &carved {
        if let Some(&last) = run.last() {
            let (lc, lr) = (last % grid.cols, last / grid.cols);
            let (cc, cr) = (cell % grid.cols, cell / grid.cols);
            if lr == cr && lc.abs_diff(cc) == 1 {
                run.push(cell);
                continue;
            }
            flush_run(grid, &run, &mut segments);
            run.clear();
        }
        run.push(cell);
    }
    flush_run(grid, &run, &mut segments);
    for seg in segments {
        objects.push(PlacedObject {
            category: Category::HiddenRoomPath,
            type_name: "Hidden room path".to_owned(),
            color: config.hidden_rooms.color_path,
            x_m: seg.x,
            y_m: seg.y,
            w_m: seg.w,
            h_m: seg.h,
            angle_deg: 0.0,
            details: ObjectDetails::HiddenRoomPath { room_id },
        });
    }
}

fn flush_run(grid: &Grid, run: &[usize], segments: &mut Vec<RectM>) {
    if run.is_empty() {
        return;
    }
    let cs: Vec<usize> = run.iter().map(|cell| cell % grid.cols).collect();
    let r = run[0] / grid.cols;
    let min_c = cs.iter().copied().min().unwrap();
    let max_c = cs.iter().copied().max().unwrap();
    segments.push(RectM::new(
        min_c as f32 * grid.cell_m,
        r as f32 * grid.cell_m,
        (max_c - min_c + 1) as f32 * grid.cell_m,
        grid.cell_m,
    ));
}

/// Cells the access path may traverse: open ground, existing path cells and WALLS
/// (secret tunnels). Houses, rocks, roads and blocked ground are off-limits.
fn bfs_passable(grid: &Grid, i: usize) -> bool {
    let (c, r) = (i % grid.cols, i / grid.cols);
    grid.is_free(c, r) || grid.is_path(c, r) || grid.is_wall(c, r)
}

fn neighbors(grid: &Grid, i: usize) -> Vec<usize> {
    let (c, r) = (i % grid.cols, i / grid.cols);
    let mut out = Vec::with_capacity(4);
    if c > 0 {
        out.push(i - 1);
    }
    if c + 1 < grid.cols {
        out.push(i + 1);
    }
    if r > 0 {
        out.push(i - grid.cols);
    }
    if r + 1 < grid.rows {
        out.push(i + grid.cols);
    }
    out
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
        // Light densities so the rooms still fit between the structures.
        for kind in config.template_mix.kinds() {
            config.structure_mut(kind).density_pct = 15.0;
        }
        let mut rng = ChaCha8Rng::seed_from_u64(11);
        let mut grid = Grid::new(config.map.width_m, config.map.height_m, config.cell_size_m);
        let mut budget = WalkBudget::new(grid.total_cells(), config.map.walkable_pct);
        let mut objects = Vec::new();
        let mut notes = Vec::new();

        let zones = build_zones(&mut rng, &config);
        place_structures(&mut rng, &mut grid, &mut budget, &zones, &config, &mut objects, &mut notes);
        let solid_before: Vec<_> = objects.iter().map(|o| o.rect()).collect();
        let angles: Vec<_> = objects.iter().map(|o| o.angle_deg).collect();
        let rooms = place(&mut rng, &mut grid, &config, &mut objects, &mut notes);

        // Rotated structures pack tighter, so a couple of rooms may fail; the
        // engine must never seal the map and paths must avoid solids.
        assert!(rooms.len() >= 3, "rooms: {}", rooms.len());
        assert_eq!(grid.connected_walkable_components(4), 1);
        let corners = |o: &PlacedObject| {
            crate::generator::geometry::rot_rect_corners(
                (o.x_m + o.w_m / 2.0, o.y_m + o.h_m / 2.0),
                o.w_m,
                o.h_m,
                o.angle_deg,
            )
        };
        for (i, path) in objects
            .iter()
            .enumerate()
            .filter(|(_, o)| o.category == Category::HiddenRoomPath)
        {
            for (j, solid) in solid_before.iter().enumerate() {
                let solid_corners = crate::generator::geometry::rot_rect_corners(
                    (solid.x + solid.w / 2.0, solid.y + solid.h / 2.0),
                    solid.w,
                    solid.h,
                    angles[j],
                );
                assert!(
                    !crate::generator::geometry::rotated_rects_overlap(
                        &corners(path),
                        &solid_corners
                    ),
                    "path {:?} overlaps solid {j}",
                    objects[i]
                );
            }
        }
    }
}
