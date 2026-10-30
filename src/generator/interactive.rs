//! The interactive objects node (every map kind): things the player can use —
//! herbs to gather, chests to open — placed per type in small groups.
//!
//! 1. **Free ground** — town cells that are not wall, road, house, prop or an
//!    earlier interactive. Like props, every object keeps one free cell
//!    (Chebyshev) from every obstacle and from each other, so the walkable
//!    ground is never sealed into a pocket.
//! 2. **Types in config order** — each type places `quantity` objects in
//!    groups of `group_min..=group_max` (the last group takes what is left).
//! 3. **A group** — a center is picked by the type's spot (anywhere / along
//!    walls / open ground; an empty spot falls back to anywhere), the first
//!    member stands on it and the rest within `spread_m` around it. A group
//!    that cannot fit its minimum size is dropped and another center tried;
//!    too many misses end the type with a note.
//!
//! Objects smaller than a cell still block the cell under their center.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::placement::{dilate, footprint};
use crate::generator::Stage;
use crate::level::{
    Category, CellMask, InteractiveGroup, InteractiveLayout, InteractiveTypeStats, PlacedObject, RegionMap,
    RoadNetwork,
};
use crate::model::{InteractiveSpot, InteractiveType, LevelConfig};

/// Group centers tried per group before the type gives up.
const CENTER_TRIES: usize = 40;
/// Spots tried per member around its group center.
const MEMBER_TRIES: usize = 25;
/// Distance, cells, from an obstacle that still counts as "along walls".
const ALONG_WALLS_CELLS: i64 = 2;
/// Clearance, meters, from obstacles and roads that makes "open ground".
const OPEN_GROUND_M: f32 = 3.0;

pub struct InteractiveBuild {
    pub layout: InteractiveLayout,
    /// The placed objects; `layout.groups` index into this vec.
    pub objects: Vec<PlacedObject>,
    pub notes: Vec<String>,
}

/// What the interactive objects stand on: the town after walls, roads,
/// houses and props.
pub struct Ground<'a> {
    pub regions: &'a RegionMap,
    pub outer_wall: Option<&'a CellMask>,
    pub region_walls: Option<&'a CellMask>,
    pub roads: Option<&'a RoadNetwork>,
    pub house_mask: Option<&'a CellMask>,
    pub prop_mask: Option<&'a CellMask>,
}

/// `build_with_progress` without progress reporting.
pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, ground: &Ground) -> Option<InteractiveBuild> {
    build_with_progress(config, rng, ground, &Stage { progress: &|_, _| {}, from: 0.0, to: 1.0 })
}

pub(crate) fn build_with_progress(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    ground: &Ground,
    stage: &Stage,
) -> Option<InteractiveBuild> {
    let map = ground.regions;
    let (w, h) = (map.width, map.height);
    let cell = map.cell_size_m.max(0.05);

    let in_mask = |m: Option<&CellMask>, i: usize| m.is_some_and(|m| m.cells[i]);
    let is_road = |i: usize| ground.roads.is_some_and(|r| r.mask.cells[i]);
    let obstacles: Vec<bool> = (0..w * h)
        .map(|i| {
            map.ids[i] == 0
                || in_mask(ground.outer_wall, i)
                || in_mask(ground.region_walls, i)
                || in_mask(ground.house_mask, i)
                || in_mask(ground.prop_mask, i)
        })
        .collect();
    if obstacles.iter().all(|&o| o) {
        return None;
    }
    let near_obstacle = dilate(&obstacles, w, h, 1);
    let mut free: Vec<bool> = (0..w * h).map(|i| !obstacles[i] && !near_obstacle[i] && !is_road(i)).collect();
    // Spot filters, fixed before any interactive is placed.
    let along_walls = dilate(&obstacles, w, h, ALONG_WALLS_CELLS);
    let open_reach = (OPEN_GROUND_M / cell).ceil().max(1.0) as i64;
    let blocked_or_road: Vec<bool> = (0..w * h).map(|i| obstacles[i] || is_road(i)).collect();
    let crowded = dilate(&blocked_or_road, w, h, open_reach);
    let fits_spot = |spot: InteractiveSpot, i: usize| match spot {
        InteractiveSpot::Anywhere => true,
        InteractiveSpot::AlongWalls => along_walls[i],
        InteractiveSpot::OpenGround => !crowded[i],
    };

    let mut mask = CellMask::new(w, h, cell);
    let mut objects: Vec<PlacedObject> = Vec::new();
    let mut groups: Vec<InteractiveGroup> = Vec::new();
    let mut per_type: Vec<InteractiveTypeStats> = Vec::new();
    let mut notes = Vec::new();
    let total: u32 = config.interactive.iter().map(|t| t.quantity).sum();
    let mut done = 0u32;

    for (type_index, it) in config.interactive.iter().enumerate() {
        let name = type_name(it, type_index);
        let lo = it.size_min_m.min(it.size_max_m).max(0.05);
        let hi = it.size_min_m.max(it.size_max_m).max(0.05);
        let gmin = it.group_min.min(it.group_max).max(1);
        let gmax = it.group_min.max(it.group_max).max(1);
        let spread = it.spread_m.max(0.0);

        let mut stats = InteractiveTypeStats { name: name.clone(), requested: it.quantity, placed: 0, groups: 0 };
        let mut fell_back = false;
        let mut remaining = it.quantity;
        // Candidate centers, built once per type; cells taken since are
        // dropped lazily when drawn.
        let mut centers: Vec<usize> = (0..w * h).filter(|&i| free[i] && fits_spot(it.spot, i)).collect();
        while remaining > 0 {
            stage.report("Placing interactive objects", done as f32 / total.max(1) as f32);
            let want = rng.random_range(gmin..=gmax).min(remaining);
            let need = gmin.min(remaining);
            let mut group: Option<Vec<Placement>> = None;
            let mut tries = 0;
            while tries < CENTER_TRIES {
                if centers.is_empty() {
                    if it.spot == InteractiveSpot::Anywhere || fell_back {
                        break;
                    }
                    fell_back = true;
                    centers = (0..w * h).filter(|&i| free[i]).collect();
                    continue;
                }
                let k = rng.random_range(0..centers.len());
                let c = centers[k];
                if !free[c] {
                    centers.swap_remove(k);
                    continue;
                }
                tries += 1;
                let center = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
                let members = try_group(rng, &free, center, want, spread, (lo, hi), w, h, cell);
                if members.len() as u32 >= need {
                    group = Some(members);
                    break;
                }
            }
            let Some(members) = group else {
                break;
            };

            let first = objects.len();
            for m in &members {
                for &c in &m.cells {
                    mask.cells[c] = true;
                }
                block_ring(&mut free, &m.cells, w, h);
                objects.push(PlacedObject {
                    category: Category::Interactive,
                    type_name: name.clone(),
                    color: it.color,
                    x_m: m.center.0 - m.size / 2.0,
                    y_m: m.center.1 - m.size / 2.0,
                    w_m: m.size,
                    h_m: m.size,
                    angle_deg: m.angle_deg,
                    parts: Vec::new(),
                    round: false,
                });
            }
            let n = members.len() as u32;
            groups.push(InteractiveGroup {
                type_index,
                type_name: name.clone(),
                center_m: [members[0].center.0, members[0].center.1],
                members: (first..objects.len()).collect(),
            });
            stats.placed += n;
            stats.groups += 1;
            remaining -= n;
            done += n;
        }

        if fell_back {
            notes.push(format!(
                "Interactive '{name}': no {} spot left, some groups placed anywhere",
                it.spot.label().to_lowercase()
            ));
        }
        if stats.placed < it.quantity {
            notes.push(format!(
                "Interactive '{name}': placed {} of {} — no free spot for another group of {gmin}+",
                stats.placed, it.quantity
            ));
        }
        per_type.push(stats);
    }

    Some(InteractiveBuild { layout: InteractiveLayout { mask, groups, types: per_type }, objects, notes })
}

/// The type's display name, or "Interactive #n" when it has none.
pub fn type_name(it: &InteractiveType, index: usize) -> String {
    let name = it.name.trim();
    if name.is_empty() { format!("Interactive #{}", index + 1) } else { name.to_owned() }
}

/// One member of a group being laid out.
struct Placement {
    center: (f32, f32),
    size: f32,
    angle_deg: f32,
    cells: Vec<usize>,
}

/// Lay out up to `want` members around `center`, the first on the center
/// itself. Nothing is committed: members only check `free` and each other.
#[allow(clippy::too_many_arguments)]
fn try_group(
    rng: &mut ChaCha8Rng,
    free: &[bool],
    center: (f32, f32),
    want: u32,
    spread: f32,
    (lo, hi): (f32, f32),
    w: usize,
    h: usize,
    cell: f32,
) -> Vec<Placement> {
    let mut members: Vec<Placement> = Vec::new();
    // Cells blocked by members already laid out (their footprint + ring).
    let mut taken: Vec<usize> = Vec::new();
    for k in 0..want {
        let tries = if k == 0 { 1 } else { MEMBER_TRIES };
        for _ in 0..tries {
            let at = if k == 0 {
                center
            } else {
                let r = spread * rng.random_range(0.0f32..=1.0).sqrt();
                let a = rng.random_range(0.0..std::f32::consts::TAU);
                (center.0 + r * a.cos(), center.1 + r * a.sin())
            };
            let size = rng.random_range(lo..=hi);
            let angle_deg = rng.random_range(0.0..90.0);
            let Some(cells) = cells_of(at, size, angle_deg, w, h, cell) else {
                continue;
            };
            if cells.iter().all(|&c| free[c] && !taken.contains(&c)) {
                taken.extend(ring(&cells, w, h));
                members.push(Placement { center: at, size, angle_deg, cells });
                break;
            }
        }
        if k == 0 && members.is_empty() {
            break;
        }
    }
    members
}

/// The cells an object blocks; a sub-cell object blocks the cell under its
/// center. `None` if it leaves the map.
fn cells_of(center: (f32, f32), size: f32, angle_deg: f32, w: usize, h: usize, cell: f32) -> Option<Vec<usize>> {
    let cells = footprint(center, (size, size), &[], angle_deg, w, h, cell)?;
    if !cells.is_empty() {
        return Some(cells);
    }
    let (cx, cy) = ((center.0 / cell).floor() as usize, (center.1 / cell).floor() as usize);
    (cx < w && cy < h).then(|| vec![cy * w + cx])
}

/// The cells plus their 8 neighbours.
fn ring(cells: &[usize], w: usize, h: usize) -> Vec<usize> {
    let mut out = Vec::with_capacity(cells.len() * 9);
    for &c in cells {
        let (x, y) = ((c % w) as i64, (c / w) as i64);
        for dy in -1..=1i64 {
            for dx in -1..=1i64 {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 {
                    out.push(ny as usize * w + nx as usize);
                }
            }
        }
    }
    out
}

fn block_ring(free: &mut [bool], cells: &[usize], w: usize, h: usize) {
    for c in ring(cells, w, h) {
        free[c] = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::grid;
    use crate::level::GeneratedLevel;
    use crate::model::{MapKind, ShapeSettings};

    fn level(config: &LevelConfig, seed: u64) -> GeneratedLevel {
        crate::generator::generate(config, seed)
    }

    fn interactives(lvl: &GeneratedLevel) -> Vec<&PlacedObject> {
        lvl.objects.iter().filter(|o| o.category == Category::Interactive).collect()
    }

    fn two_types() -> LevelConfig {
        let mut config = LevelConfig::test_64();
        config.interactive.push(InteractiveType {
            name: "Chest".to_owned(),
            size_min_m: 0.8,
            size_max_m: 1.0,
            quantity: 6,
            group_min: 1,
            group_max: 1,
            spread_m: 0.0,
            spot: InteractiveSpot::AlongWalls,
            color: [200, 160, 40],
        });
        config
    }

    #[test]
    fn quantity_is_placed_in_groups_of_the_allowed_size() {
        for seed in 1..=4u64 {
            let lvl = level(&two_types(), seed);
            let layout = lvl.interactive.as_ref().unwrap();
            assert_eq!(interactives(&lvl).len(), 36, "seed {seed}: {:?}", lvl.notes);
            assert_eq!(layout.types[0].placed, 30);
            assert_eq!(layout.types[1].placed, 6);
            let mut total = 0;
            for g in &layout.groups {
                let n = g.members.len();
                total += n;
                let (lo, hi) = if g.type_index == 0 { (2, 5) } else { (1, 1) };
                // The last group of a type may be smaller (what was left).
                assert!(n <= hi && (n >= lo || g.type_index == 0), "seed {seed}: group of {n}");
                for &m in &g.members {
                    let o = &lvl.objects[m];
                    assert_eq!(o.category, Category::Interactive);
                    assert_eq!(o.type_name, g.type_name);
                }
            }
            assert_eq!(total, 36);
        }
    }

    #[test]
    fn group_members_stay_within_spread() {
        let lvl = level(&LevelConfig::test_64(), 3);
        let layout = lvl.interactive.as_ref().unwrap();
        for g in &layout.groups {
            for &m in &g.members {
                let (x, y) = lvl.objects[m].center();
                let d = ((x - g.center_m[0]).powi(2) + (y - g.center_m[1]).powi(2)).sqrt();
                assert!(d <= 2.0 + 1e-3, "member {d} m from its group center");
            }
        }
    }

    #[test]
    fn interactives_never_overlap_anything() {
        for seed in 1..=4u64 {
            let mut config = two_types();
            config.interactive[0].quantity = 200;
            let lvl = level(&config, seed);
            let road = &lvl.roads.as_ref().unwrap().mask;
            let houses = &lvl.houses.as_ref().unwrap().mask;
            let props = &lvl.props.as_ref().unwrap().mask;
            let (w, h, cell) = (road.width, road.height, road.cell_size_m);
            let mut taken = vec![false; w * h];
            for o in interactives(&lvl) {
                for c in cells_of(o.center(), o.w_m, o.angle_deg, w, h, cell).unwrap() {
                    let wall = lvl.walls.as_ref().is_some_and(|m| m.mask.cells[c])
                        || lvl.region_walls.as_ref().is_some_and(|m| m.mask.cells[c]);
                    assert!(!road.cells[c] && !houses.cells[c] && !props.cells[c] && !wall, "seed {seed}");
                    assert!(!taken[c], "seed {seed}: interactives overlap");
                    taken[c] = true;
                }
            }
        }
    }

    #[test]
    fn walkable_ground_stays_one_piece() {
        for seed in 1..=4u64 {
            let mut config = LevelConfig::test_64();
            config.interactive[0].quantity = 400;
            config.interactive[0].group_max = 8;
            let lvl = level(&config, seed);
            let map = &lvl.regions.as_ref().unwrap().map;
            let blocked = [
                lvl.houses.as_ref().map(|l| &l.mask),
                lvl.props.as_ref().map(|l| &l.mask),
                lvl.interactive.as_ref().map(|l| &l.mask),
                lvl.walls.as_ref().map(|l| &l.mask),
                lvl.region_walls.as_ref().map(|l| &l.mask),
            ];
            let mut open = CellMask::new(map.width, map.height, map.cell_size_m);
            for i in 0..open.cells.len() {
                open.cells[i] = map.ids[i] != 0 && !blocked.iter().flatten().any(|m| m.cells[i]);
            }
            assert!(grid::is_connected(&open), "seed {seed}: interactives sealed off a pocket");
        }
    }

    #[test]
    fn along_walls_groups_start_next_to_an_obstacle() {
        let lvl = level(&two_types(), 6);
        let layout = lvl.interactive.as_ref().unwrap();
        let map = &lvl.regions.as_ref().unwrap().map;
        let (w, h, cell) = (map.width, map.height, map.cell_size_m);
        let obstacle = |i: usize| {
            map.ids[i] == 0
                || lvl.walls.as_ref().is_some_and(|m| m.mask.cells[i])
                || lvl.region_walls.as_ref().is_some_and(|m| m.mask.cells[i])
                || lvl.houses.as_ref().is_some_and(|m| m.mask.cells[i])
                || lvl.props.as_ref().is_some_and(|m| m.mask.cells[i])
        };
        for g in layout.groups.iter().filter(|g| g.type_index == 1) {
            let (cx, cy) = ((g.center_m[0] / cell) as i64, (g.center_m[1] / cell) as i64);
            let near = (-ALONG_WALLS_CELLS..=ALONG_WALLS_CELLS).any(|dy| {
                (-ALONG_WALLS_CELLS..=ALONG_WALLS_CELLS).any(|dx| {
                    let (x, y) = (cx + dx, cy + dy);
                    x >= 0 && y >= 0 && x < w as i64 && y < h as i64 && obstacle(y as usize * w + x as usize)
                })
            });
            assert!(near, "chest group at {:?} is not along a wall", g.center_m);
        }
    }

    #[test]
    fn too_many_ends_with_a_note() {
        let mut config = LevelConfig::test_64();
        config.interactive[0].quantity = 100_000;
        let lvl = level(&config, 2);
        let layout = lvl.interactive.as_ref().unwrap();
        assert!(layout.types[0].placed < 100_000);
        assert!(lvl.notes.iter().any(|n| n.contains("Interactive 'Herb': placed")), "{:?}", lvl.notes);
    }

    #[test]
    fn non_town_maps_get_interactives() {
        let mut config = LevelConfig::test_64();
        config.kind = MapKind::Forest;
        config.shape = ShapeSettings::for_kind(config.kind);
        let lvl = level(&config, 2);
        assert_eq!(interactives(&lvl).len(), 30, "{:?}", lvl.notes);
    }

    #[test]
    fn same_seed_same_interactives() {
        let config = two_types();
        assert_eq!(level(&config, 8).objects, level(&config, 8).objects);
    }
}
