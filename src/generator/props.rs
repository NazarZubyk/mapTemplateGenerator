//! The props node (every map kind): decorative, non-interactive objects —
//! crates and barrels in a town, rocks in a cave, logs in a forest.
//!
//! 1. **Free ground** — town cells that are not wall, road or house. Props
//!    never stand on a road, and keep one free cell (Chebyshev) from every
//!    wall, house, the town edge and each other: obstacles never touch, so
//!    the walkable ground can never be sealed into a pocket.
//! 2. **How many** — `quantity`, capped by `density_pct`: props may cover at
//!    most that share of the free ground (at the average prop size). The
//!    lower of the two wins; a note says which one stopped placement.
//! 3. **Where** — split by `near_houses_pct` / `roadside_pct` / the rest:
//!    - *near houses*: against a random side of a house (or one of its
//!      wings), one cell off the wall, turned like the house;
//!    - *roadside*: at a road's edge, turned along the road;
//!    - *scattered*: random free spots, random angles.
//!
//!    Maps without houses scatter the near-house share; a prop that finds no
//!    spot in its mode is retried as scattered.
//!
//! Width and depth are rolled independently in `[size_min_m, size_max_m]`.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::placement::{dilate, footprint, path_tangents};
use crate::generator::Stage;
use crate::level::{Category, CellMask, PlacedObject, PropLayout, RegionMap, RoadNetwork};
use crate::model::LevelConfig;

/// Display name of a prop (map legend, spec.json).
pub const PROP_NAME: &str = "prop";
/// Spots tried per prop in its mode before it falls back to scattering.
pub(crate) const TRIES: usize = 30;

pub struct PropBuild {
    pub layout: PropLayout,
    pub objects: Vec<PlacedObject>,
    pub notes: Vec<String>,
}

/// What the props stand on: the town after walls, roads and houses.
pub struct Ground<'a> {
    pub regions: &'a RegionMap,
    pub outer_wall: Option<&'a CellMask>,
    pub region_walls: Option<&'a CellMask>,
    pub roads: Option<&'a RoadNetwork>,
    /// Houses already placed (their footprint and their shapes).
    pub house_mask: Option<&'a CellMask>,
    pub houses: &'a [PlacedObject],
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    NearHouse,
    Roadside,
    Scattered,
}

/// `build_with_progress` without progress reporting.
pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, ground: &Ground) -> Option<PropBuild> {
    build_with_progress(config, rng, ground, &Stage { progress: &|_, _| {}, from: 0.0, to: 1.0 })
}

pub(crate) fn build_with_progress(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    ground: &Ground,
    stage: &Stage,
) -> Option<PropBuild> {
    let map = ground.regions;
    let (w, h) = (map.width, map.height);
    let cell = map.cell_size_m.max(0.05);
    let s = &config.props;
    let lo = s.size_min_m.min(s.size_max_m).max(cell);
    let hi = s.size_min_m.max(s.size_max_m).max(cell);

    // 1. Free ground and the keep-clear ring around obstacles.
    let in_mask = |m: Option<&CellMask>, i: usize| m.is_some_and(|m| m.cells[i]);
    let is_wall = |i: usize| in_mask(ground.outer_wall, i) || in_mask(ground.region_walls, i);
    let is_road = |i: usize| ground.roads.is_some_and(|r| r.mask.cells[i]);
    let is_house = |i: usize| in_mask(ground.house_mask, i);
    let free_ground: Vec<bool> =
        (0..w * h).map(|i| map.ids[i] != 0 && !is_wall(i) && !is_road(i) && !is_house(i)).collect();
    let free_cells = free_ground.iter().filter(|&&f| f).count();
    if free_cells == 0 {
        return None;
    }
    let obstacles: Vec<bool> = (0..w * h).map(|i| map.ids[i] == 0 || is_wall(i) || is_house(i)).collect();
    let near_obstacle = dilate(&obstacles, w, h, 1);
    let mut free: Vec<bool> = (0..w * h).map(|i| free_ground[i] && !near_obstacle[i]).collect();

    // 2. How many: quantity, capped by density at the average prop size.
    let avg_cells = ((lo + hi) / 2.0 / cell).powi(2).max(1.0);
    let density_cap = (s.density_pct.clamp(0.0, 100.0) / 100.0 * free_cells as f32 / avg_cells).floor() as u32;
    let wanted = s.quantity.min(density_cap);

    // 3. Where: shares per mode (near-house only with houses on the map).
    let has_houses = !ground.houses.is_empty();
    let has_roads = ground.roads.is_some_and(|r| !r.links.is_empty());
    let near_share = if has_houses { s.near_houses_pct.clamp(0.0, 100.0) } else { 0.0 };
    let road_share = if has_roads { s.roadside_pct.clamp(0.0, 100.0) } else { 0.0 };
    let scale = if near_share + road_share > 100.0 { 100.0 / (near_share + road_share) } else { 1.0 };
    let n_near = (wanted as f32 * near_share * scale / 100.0).round() as u32;
    let n_road = ((wanted as f32 * road_share * scale / 100.0).round() as u32).min(wanted - n_near);
    let mut plan: Vec<Mode> = Vec::with_capacity(wanted as usize);
    plan.extend(std::iter::repeat_n(Mode::NearHouse, n_near as usize));
    plan.extend(std::iter::repeat_n(Mode::Roadside, n_road as usize));
    plan.extend(std::iter::repeat_n(Mode::Scattered, (wanted - n_near - n_road) as usize));

    let mut mask = CellMask::new(w, h, cell);
    let mut objects: Vec<PlacedObject> = Vec::new();
    let mut place = |free: &mut Vec<bool>, center: (f32, f32), size: (f32, f32), angle_deg: f32| -> bool {
        let Some(cells) = footprint(center, size, &[], angle_deg, w, h, cell) else {
            return false;
        };
        if cells.is_empty() || !cells.iter().all(|&c| free[c]) {
            return false;
        }
        for &c in &cells {
            mask.cells[c] = true;
            let (x, y) = ((c % w) as i64, (c / w) as i64);
            for dy in -1..=1i64 {
                for dx in -1..=1i64 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 {
                        free[ny as usize * w + nx as usize] = false;
                    }
                }
            }
        }
        objects.push(PlacedObject {
            category: Category::Prop,
            type_name: PROP_NAME.to_owned(),
            color: s.color,
            x_m: center.0 - size.0 / 2.0,
            y_m: center.1 - size.1 / 2.0,
            w_m: size.0,
            h_m: size.1,
            angle_deg,
            parts: Vec::new(),
            round: false,
        });
        true
    };

    let tangents: Vec<Vec<(f32, f32)>> = ground
        .roads
        .map(|net| net.links.iter().map(|l| path_tangents(&l.path, w)).collect())
        .unwrap_or_default();
    let (mut near, mut roadside, mut scattered) = (0u32, 0u32, 0u32);
    let mut scatter_pool: Vec<usize> = Vec::new();
    let mut pool_stale = true;
    for (k, mode) in plan.iter().copied().enumerate() {
        if k % 16 == 0 {
            stage.report("Placing props", k as f32 / plan.len().max(1) as f32);
        }
        let size = (rng.random_range(lo..=hi), rng.random_range(lo..=hi));
        // Its own mode first; a near-house or roadside prop that finds no
        // spot is scattered instead.
        let modes: &[Mode] = if mode == Mode::Scattered { &[Mode::Scattered] } else { &[mode, Mode::Scattered] };
        let mut placed = None;
        'modes: for &attempt in modes {
            for _ in 0..TRIES {
                let spot = match attempt {
                    Mode::NearHouse => near_house_spot(rng, ground.houses, size, cell),
                    Mode::Roadside => ground.roads.and_then(|net| roadside_spot(rng, net, &tangents, size, w, cell)),
                    Mode::Scattered => {
                        if pool_stale {
                            scatter_pool = (0..w * h).filter(|&i| free[i]).collect();
                            pool_stale = false;
                        }
                        if scatter_pool.is_empty() {
                            break 'modes;
                        }
                        let c = scatter_pool[rng.random_range(0..scatter_pool.len())];
                        let center = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
                        Some((center, rng.random_range(0.0..180.0)))
                    }
                };
                if let Some((center, angle)) = spot
                    && place(&mut free, center, size, angle)
                {
                    placed = Some(attempt);
                    pool_stale = true;
                    break 'modes;
                }
            }
        }
        match placed {
            Some(Mode::NearHouse) => near += 1,
            Some(Mode::Roadside) => roadside += 1,
            Some(Mode::Scattered) => scattered += 1,
            None => {}
        }
    }

    let total = near + roadside + scattered;
    let mut notes = Vec::new();
    if s.quantity > density_cap {
        notes.push(format!(
            "Props: the {:.0}% density cap allows ~{density_cap} of the {} asked",
            s.density_pct, s.quantity
        ));
    }
    if total < wanted {
        notes.push(format!("Props: only {total} of {wanted} fit — no free spot left for the rest"));
    }
    Some(PropBuild {
        layout: PropLayout {
            mask,
            near_houses: near,
            roadside,
            scattered,
            requested: s.quantity,
            density_cap,
            near_trees: 0,
            shore: 0,
            in_water: 0,
            path_edge: 0,
        },
        objects,
        notes,
    })
}

/// A spot against a random side of a random house rect (main block or a
/// wing): one cell off the wall, turned along that side. Returns (center,
/// angle).
fn near_house_spot(
    rng: &mut ChaCha8Rng,
    houses: &[PlacedObject],
    size: (f32, f32),
    cell: f32,
) -> Option<((f32, f32), f32)> {
    let house = houses.get(rng.random_range(0..houses.len().max(1)))?;
    let rects = house.all_corners();
    let rect = rects[rng.random_range(0..rects.len())];
    let i = rng.random_range(0..4);
    let (a, b) = (rect[i], rect[(i + 1) % 4]);
    let (ex, ey) = (b.0 - a.0, b.1 - a.1);
    let len = (ex * ex + ey * ey).sqrt().max(1e-6);
    // Corners run clockwise on screen (+y down): (ey, −ex) points outward.
    let normal = (ey / len, -ex / len);
    let t = rng.random_range(0.15..=0.85);
    let on_wall = (a.0 + ex * t, a.1 + ey * t);
    let off = cell * 1.5 + size.1 / 2.0;
    Some(((on_wall.0 + normal.0 * off, on_wall.1 + normal.1 * off), ey.atan2(ex).to_degrees()))
}

/// A spot at the edge of a random road, on a random side, turned along it.
fn roadside_spot(
    rng: &mut ChaCha8Rng,
    net: &RoadNetwork,
    tangents: &[Vec<(f32, f32)>],
    size: (f32, f32),
    w: usize,
    cell: f32,
) -> Option<((f32, f32), f32)> {
    if net.links.is_empty() {
        return None;
    }
    let li = rng.random_range(0..net.links.len());
    let link = &net.links[li];
    let k = rng.random_range(0..link.path.len());
    let c = link.path[k];
    let (tx, ty) = tangents[li][k];
    let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
    let normal = (-ty * side, tx * side);
    let half_road = link.width_cells.max(1) as f32 * cell / 2.0;
    let off = half_road + cell * 0.75 + size.1 / 2.0;
    let p = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
    Some(((p.0 + normal.0 * off, p.1 + normal.1 * off), ty.atan2(tx).to_degrees()))
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

    fn props(lvl: &GeneratedLevel) -> Vec<&PlacedObject> {
        lvl.objects.iter().filter(|o| o.category == Category::Prop).collect()
    }

    #[test]
    fn quantity_is_placed_when_there_is_room() {
        for seed in 1..=4u64 {
            let lvl = level(&LevelConfig::test_64(), seed);
            let layout = lvl.props.as_ref().unwrap();
            assert_eq!(props(&lvl).len(), 40, "seed {seed}: {:?}", lvl.notes);
            assert_eq!(layout.near_houses + layout.roadside + layout.scattered, 40);
            assert!(layout.near_houses > 0 && layout.roadside > 0 && layout.scattered > 0, "seed {seed}");
        }
    }

    #[test]
    fn density_caps_the_quantity() {
        let mut config = LevelConfig::test_64();
        config.props.quantity = 500;
        config.props.density_pct = 2.0;
        let lvl = level(&config, 3);
        let layout = lvl.props.as_ref().unwrap();
        assert!(layout.density_cap < 500);
        assert!(props(&lvl).len() as u32 <= layout.density_cap);
        assert!(lvl.notes.iter().any(|n| n.contains("density cap")), "{:?}", lvl.notes);
    }

    #[test]
    fn props_never_overlap_roads_walls_houses_or_each_other() {
        for seed in 1..=4u64 {
            let mut config = LevelConfig::test_64();
            config.props.quantity = 150;
            config.props.density_pct = 30.0;
            let lvl = level(&config, seed);
            let road = &lvl.roads.as_ref().unwrap().mask;
            let houses = &lvl.houses.as_ref().unwrap().mask;
            let (w, h, cell) = (road.width, road.height, road.cell_size_m);
            let mut taken = vec![false; w * h];
            for o in props(&lvl) {
                for c in footprint(o.center(), (o.w_m, o.h_m), &[], o.angle_deg, w, h, cell).unwrap() {
                    let wall = lvl.walls.as_ref().is_some_and(|m| m.mask.cells[c])
                        || lvl.region_walls.as_ref().is_some_and(|m| m.mask.cells[c]);
                    assert!(!road.cells[c] && !houses.cells[c] && !wall, "seed {seed}: prop on road/house/wall");
                    assert!(!taken[c], "seed {seed}: props overlap");
                    taken[c] = true;
                }
            }
        }
    }

    #[test]
    fn walkable_ground_stays_one_piece() {
        for seed in 1..=4u64 {
            let mut config = LevelConfig::test_64();
            config.props.quantity = 300;
            config.props.density_pct = 40.0;
            let lvl = level(&config, seed);
            let map = &lvl.regions.as_ref().unwrap().map;
            let houses = &lvl.houses.as_ref().unwrap().mask;
            let props = &lvl.props.as_ref().unwrap().mask;
            let mut open = CellMask::new(map.width, map.height, map.cell_size_m);
            for i in 0..open.cells.len() {
                let wall = lvl.walls.as_ref().is_some_and(|m| m.mask.cells[i])
                    || lvl.region_walls.as_ref().is_some_and(|m| m.mask.cells[i]);
                open.cells[i] = map.ids[i] != 0 && !wall && !houses.cells[i] && !props.cells[i];
            }
            assert!(grid::is_connected(&open), "seed {seed}: props sealed off a pocket");
        }
    }

    #[test]
    fn near_house_props_hug_a_house() {
        let mut config = LevelConfig::test_64();
        config.props.near_houses_pct = 100.0;
        config.props.roadside_pct = 0.0;
        let lvl = level(&config, 5);
        let houses: Vec<&PlacedObject> = lvl.objects.iter().filter(|o| o.category == Category::Structure).collect();
        let layout = lvl.props.as_ref().unwrap();
        assert!(layout.near_houses as usize >= props(&lvl).len() * 3 / 4);
        // Most props have a house within 1.5 m of their center + half size.
        let close = props(&lvl)
            .iter()
            .filter(|p| {
                let (cx, cy) = p.center();
                let reach = p.w_m.max(p.h_m) / 2.0 + 1.5;
                houses.iter().any(|hs| {
                    (0..16).any(|k| {
                        let a = k as f32 / 16.0 * std::f32::consts::TAU;
                        hs.contains(cx + reach * a.cos(), cy + reach * a.sin())
                    })
                })
            })
            .count();
        assert!(close * 4 >= props(&lvl).len() * 3, "{close} of {} near a house", props(&lvl).len());
    }

    #[test]
    fn non_town_maps_get_props_without_houses() {
        let mut config = LevelConfig::test_64();
        config.kind = MapKind::Forest;
        config.shape = ShapeSettings::for_kind(config.kind);
        let lvl = level(&config, 2);
        let layout = lvl.props.as_ref().unwrap();
        assert_eq!(layout.near_houses, 0);
        assert!(!props(&lvl).is_empty());
    }

    #[test]
    fn same_seed_same_props() {
        let config = LevelConfig::test_64();
        assert_eq!(level(&config, 8).objects, level(&config, 8).objects);
    }
}
