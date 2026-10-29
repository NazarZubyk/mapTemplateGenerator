//! The houses node (town maps): rotated rectangular houses, first lining the
//! streets, then filling the yards behind them.
//!
//! 1. **Buildable ground** — town cells that are not outer wall, region wall
//!    or road; houses keep `spacing_m` (≥ 1 cell) from walls and the town
//!    edge, and street houses `setback_m` from the road edge.
//! 2. **Street fronts** — every road is walked on both sides; a slot every
//!    house width + spacing gets a house rotated to the road's direction,
//!    set back from its edge (pushed a little deeper, then made smaller, if
//!    the first spot does not fit, e.g. on a bend or against a wall). Slots are shuffled, so a low density gives
//!    scattered street houses, a high one gives continuous frontages.
//! 3. **Yards** — random free spots; with `face_road_pct` chance a house
//!    turns to the nearest road's direction (± a few degrees), otherwise it
//!    takes a random angle. A house that does not fit shrinks toward
//!    `size_min_m` and tries turned 90° before the spot is given up — tight
//!    spots get small houses.
//! 4. **Stop** — when houses cover `density_pct` of the buildable ground, or
//!    no more house fits.
//!
//! Every house keeps at least one free cell (Chebyshev) to every other house
//! and wall, and houses are convex — so the walkable ground can never be
//! sealed into a pocket. Width and depth are rolled independently in
//! `[size_min_m, size_max_m]`.

use std::collections::VecDeque;

use rand::seq::SliceRandom;
use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::Stage;
use crate::level::{Category, CellMask, HouseLayout, PlacedObject, RegionMap, RoadNetwork};
use crate::model::LevelConfig;

/// A spot along a road where a street-front house may stand.
struct StreetSlot {
    /// Road centerline point, meters.
    point: (f32, f32),
    /// Unit normal pointing to this side of the road.
    normal: (f32, f32),
    /// Road direction, degrees.
    angle_deg: f32,
}

/// Display name of a house (map legend, spec.json).
pub const HOUSE_NAME: &str = "house";
/// Path cells on each side used to estimate a road's direction (long enough
/// to smooth the 8-direction staircase of a diagonal road).
const TANGENT_SPAN: usize = 8;
/// Random yard spots tried per house before counting a failure.
const YARD_TRIES: usize = 40;
/// Consecutive yard failures before the yards count as full.
const YARD_GIVE_UP: usize = 80;
/// Jitter of a road-facing yard house, degrees (±).
const FACE_JITTER_DEG: f32 = 6.0;

pub struct HouseBuild {
    pub layout: HouseLayout,
    pub objects: Vec<PlacedObject>,
    pub notes: Vec<String>,
}

/// What the houses stand on: the town after walls and roads.
pub struct Ground<'a> {
    pub regions: &'a RegionMap,
    pub outer_wall: Option<&'a CellMask>,
    pub region_walls: Option<&'a CellMask>,
    pub roads: Option<&'a RoadNetwork>,
}

/// `build_with_progress` without progress reporting.
pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, ground: &Ground) -> Option<HouseBuild> {
    build_with_progress(config, rng, ground, &Stage { progress: &|_, _| {}, from: 0.0, to: 1.0 })
}

pub(crate) fn build_with_progress(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    ground: &Ground,
    stage: &Stage,
) -> Option<HouseBuild> {
    let map = ground.regions;
    let (w, h) = (map.width, map.height);
    let cell = map.cell_size_m.max(0.05);
    let s = &config.houses;
    let lo = s.size_min_m.min(s.size_max_m).max(cell);
    let hi = s.size_min_m.max(s.size_max_m).max(cell);
    let clearance = ((s.spacing_m / cell).round() as i64).max(1);
    let setback = (s.setback_m.max(0.0) / cell).round() as u32;

    // 1. Buildable ground and what houses must keep away from.
    let is_wall = |i: usize| {
        ground.outer_wall.is_some_and(|m| m.cells[i]) || ground.region_walls.is_some_and(|m| m.cells[i])
    };
    let is_road = |i: usize| ground.roads.is_some_and(|r| r.mask.cells[i]);
    let buildable: Vec<bool> = (0..w * h).map(|i| map.ids[i] != 0 && !is_wall(i) && !is_road(i)).collect();
    let buildable_cells = buildable.iter().filter(|&&b| b).count();
    if buildable_cells == 0 {
        return None;
    }
    // Obstacles a house keeps `clearance` from: everything not town, walls.
    let solid: Vec<bool> = (0..w * h).map(|i| map.ids[i] == 0 || is_wall(i)).collect();
    let near_solid = dilate(&solid, w, h, clearance);
    let near_road = match ground.roads {
        Some(r) => within(&r.mask.cells, w, h, setback),
        None => vec![false; w * h],
    };
    let mut free: Vec<bool> = (0..w * h).map(|i| buildable[i] && !near_solid[i] && !near_road[i]).collect();

    let target = s.density_pct.clamp(0.0, 100.0) / 100.0 * buildable_cells as f32;
    let mut house_mask = CellMask::new(w, h, cell);
    let mut covered = 0usize;
    let mut objects: Vec<PlacedObject> = Vec::new();
    let place = |objects: &mut Vec<PlacedObject>,
                     free: &mut Vec<bool>,
                     house_mask: &mut CellMask,
                     covered: &mut usize,
                     center: (f32, f32),
                     size: (f32, f32),
                     angle_deg: f32|
     -> bool {
        let Some(cells) = footprint(center, size, angle_deg, w, h, cell) else {
            return false;
        };
        if cells.is_empty() || !cells.iter().all(|&c| free[c]) {
            return false;
        }
        for &c in &cells {
            house_mask.cells[c] = true;
        }
        // Reserve the clearance ring around the house.
        for &c in &cells {
            let (x, y) = ((c % w) as i64, (c / w) as i64);
            for dy in -clearance..=clearance {
                for dx in -clearance..=clearance {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 {
                        free[ny as usize * w + nx as usize] = false;
                    }
                }
            }
        }
        *covered += cells.len();
        objects.push(PlacedObject {
            category: Category::Structure,
            type_name: HOUSE_NAME.to_owned(),
            color: s.color,
            x_m: center.0 - size.0 / 2.0,
            y_m: center.1 - size.1 / 2.0,
            w_m: size.0,
            h_m: size.1,
            angle_deg,
        });
        true
    };

    // 2. Street fronts.
    stage.report("Lining the streets", 0.0);
    let mut street_houses = 0u32;
    if let Some(net) = ground.roads {
        let half_road = net.width_cells as f32 * cell / 2.0;
        let mut slots: Vec<StreetSlot> = Vec::new();
        for link in &net.links {
            let tangents = path_tangents(&link.path, w);
            for side in [-1.0f32, 1.0] {
                let mut next_at = rng.random_range(0.0..hi);
                let mut walked = 0.0f32;
                for (k, &(tx, ty)) in tangents.iter().enumerate().skip(1) {
                    let (a, b) = (link.path[k - 1], link.path[k]);
                    let step = if a % w != b % w && a / w != b / w { std::f32::consts::SQRT_2 } else { 1.0 } * cell;
                    walked += step;
                    if walked < next_at {
                        continue;
                    }
                    let p = (((b % w) as f32 + 0.5) * cell, ((b / w) as f32 + 0.5) * cell);
                    slots.push(StreetSlot {
                        point: p,
                        normal: (-ty * side, tx * side),
                        angle_deg: ty.atan2(tx).to_degrees(),
                    });
                    next_at = walked + rng.random_range(lo..=hi) + s.spacing_m.max(cell);
                }
            }
        }
        slots.shuffle(rng);
        for (k, StreetSlot { point: p, normal: n, angle_deg: angle }) in slots.into_iter().enumerate() {
            if (covered as f32) >= target {
                break;
            }
            if k % 32 == 0 {
                stage.report("Lining the streets", 0.5 * (covered as f32 / target.max(1.0)).min(1.0));
            }
            let size = (rng.random_range(lo..=hi), rng.random_range(lo..=hi));
            // Width runs along the road. Try the setback, then a little
            // deeper; a house that still does not fit gets shallower and
            // narrower toward the minimum size.
            let shrink = |f: f32| (lo + (size.0 - lo) * f, lo + (size.1 - lo) * f);
            'fit: for fit in [shrink(1.0), shrink(0.5), shrink(0.0)] {
                for extra in [0.0f32, 0.5, 1.0] {
                    let depth = half_road + s.setback_m.max(0.0) + extra + fit.1 / 2.0 + cell * 0.5;
                    let center = (p.0 + n.0 * depth, p.1 + n.1 * depth);
                    if place(&mut objects, &mut free, &mut house_mask, &mut covered, center, fit, angle) {
                        street_houses += 1;
                        break 'fit;
                    }
                }
            }
        }
    }

    // 3. Yards.
    let road_angle = ground.roads.map(|net| nearest_road_angle(net, &map.ids, w, h));
    let face = s.face_road_pct.clamp(0.0, 100.0) / 100.0;
    let mut yard_houses = 0u32;
    let mut failures = 0usize;
    // Spots are drawn from the cells still free; the list is refreshed every
    // few houses so tries are not wasted on taken ground.
    let mut candidates: Vec<usize> = Vec::new();
    let mut stale = true;
    while (covered as f32) < target && failures < YARD_GIVE_UP {
        if stale {
            candidates = (0..w * h).filter(|&i| free[i]).collect();
            if candidates.is_empty() {
                break;
            }
        }
        if yard_houses.is_multiple_of(16) {
            stage.report("Filling the yards", 0.5 + 0.5 * (covered as f32 / target.max(1.0)).min(1.0));
        }
        let size = (rng.random_range(lo..=hi), rng.random_range(lo..=hi));
        let mut placed = false;
        for _ in 0..YARD_TRIES {
            let c = candidates[rng.random_range(0..candidates.len())];
            if !free[c] {
                continue;
            }
            let center = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
            let faces = rng.random::<f32>() < face;
            let angle = match (&road_angle, faces) {
                (Some(field), true) => field[c] + rng.random_range(-FACE_JITTER_DEG..=FACE_JITTER_DEG),
                _ => rng.random_range(0.0..180.0),
            };
            // Tight spot: shrink toward the minimum size, and try the house
            // turned 90° (width and depth swapped), before giving up here.
            let shrink = |f: f32| (lo + (size.0 - lo) * f, lo + (size.1 - lo) * f);
            for fit in [shrink(1.0), shrink(0.5), shrink(0.0)] {
                if place(&mut objects, &mut free, &mut house_mask, &mut covered, center, fit, angle)
                    || place(&mut objects, &mut free, &mut house_mask, &mut covered, center, (fit.1, fit.0), angle)
                {
                    placed = true;
                    break;
                }
            }
            if placed {
                break;
            }
        }
        if placed {
            yard_houses += 1;
            failures = 0;
            stale = yard_houses.is_multiple_of(8);
        } else {
            failures += 1;
            stale = failures.is_multiple_of(8);
        }
    }

    let coverage_pct = covered as f32 / buildable_cells as f32 * 100.0;
    let target_pct = s.density_pct.clamp(0.0, 100.0);
    let mut notes = Vec::new();
    if target_pct > 0.0 && coverage_pct < target_pct * 0.8 {
        notes.push(format!(
            "Houses cover {coverage_pct:.0}% of the buildable ground (asked {target_pct:.0}%) — no \
             more house fits; try smaller houses, less spacing or less setback"
        ));
    }
    Some(HouseBuild {
        layout: HouseLayout {
            mask: house_mask,
            street_houses,
            yard_houses,
            coverage_pct,
            target_pct,
        },
        objects,
        notes,
    })
}

/// The cells whose centers fall inside the rotated rect (`size` = width
/// along the angle, depth across it). `None` if any corner leaves the map.
fn footprint(
    center: (f32, f32),
    size: (f32, f32),
    angle_deg: f32,
    w: usize,
    h: usize,
    cell: f32,
) -> Option<Vec<usize>> {
    let probe = PlacedObject {
        category: Category::Structure,
        type_name: String::new(),
        color: [0; 3],
        x_m: center.0 - size.0 / 2.0,
        y_m: center.1 - size.1 / 2.0,
        w_m: size.0,
        h_m: size.1,
        angle_deg,
    };
    let corners = probe.corners();
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (x, y) in corners {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    if x0 < 0.0 || y0 < 0.0 || x1 > w as f32 * cell || y1 > h as f32 * cell {
        return None;
    }
    let mut cells = Vec::new();
    for cy in (y0 / cell).floor() as usize..((y1 / cell).ceil() as usize).min(h) {
        for cx in (x0 / cell).floor() as usize..((x1 / cell).ceil() as usize).min(w) {
            if probe.contains((cx as f32 + 0.5) * cell, (cy as f32 + 0.5) * cell) {
                cells.push(cy * w + cx);
            }
        }
    }
    Some(cells)
}

/// Unit direction of the road at every path cell (from a few cells behind
/// to a few cells ahead).
fn path_tangents(path: &[usize], w: usize) -> Vec<(f32, f32)> {
    let at = |c: usize| ((c % w) as f32, (c / w) as f32);
    (0..path.len())
        .map(|k| {
            let a = at(path[k.saturating_sub(TANGENT_SPAN)]);
            let b = at(path[(k + TANGENT_SPAN).min(path.len() - 1)]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = (dx * dx + dy * dy).sqrt();
            if len > 0.0 { (dx / len, dy / len) } else { (1.0, 0.0) }
        })
        .collect()
}

/// For every town cell, the direction (degrees) of the nearest road
/// centerline — multi-source BFS through the town carrying the angle.
fn nearest_road_angle(net: &RoadNetwork, ids: &[u8], w: usize, h: usize) -> Vec<f32> {
    let mut angle = vec![f32::NAN; w * h];
    let mut queue = VecDeque::new();
    for link in &net.links {
        for (&c, &(tx, ty)) in link.path.iter().zip(&path_tangents(&link.path, w)) {
            if angle[c].is_nan() {
                angle[c] = ty.atan2(tx).to_degrees();
                queue.push_back(c);
            }
        }
    }
    while let Some(c) = queue.pop_front() {
        let (x, y) = (c % w, c / w);
        let neighbors = [
            (x > 0).then(|| c - 1),
            (x + 1 < w).then(|| c + 1),
            (y > 0).then(|| c - w),
            (y + 1 < h).then(|| c + w),
        ];
        for n in neighbors.into_iter().flatten() {
            if ids[n] != 0 && angle[n].is_nan() {
                angle[n] = angle[c];
                queue.push_back(n);
            }
        }
    }
    for a in &mut angle {
        if a.is_nan() {
            *a = 0.0;
        }
    }
    angle
}

/// Chebyshev dilation: cells within `r` (8-neighborhood steps) of a set cell.
fn dilate(mask: &[bool], w: usize, h: usize, r: i64) -> Vec<bool> {
    // Separable max filter: rows, then columns.
    let mut rows = vec![false; w * h];
    for y in 0..h {
        let mut last: i64 = i64::MIN / 2;
        let mut next = vec![i64::MAX / 2; w];
        let mut upcoming: i64 = i64::MAX / 2;
        for x in (0..w).rev() {
            if mask[y * w + x] {
                upcoming = x as i64;
            }
            next[x] = upcoming;
        }
        for x in 0..w {
            if mask[y * w + x] {
                last = x as i64;
            }
            rows[y * w + x] = x as i64 - last <= r || next[x] - x as i64 <= r;
        }
    }
    let mut out = vec![false; w * h];
    for x in 0..w {
        let mut last: i64 = i64::MIN / 2;
        let mut next = vec![i64::MAX / 2; h];
        let mut upcoming: i64 = i64::MAX / 2;
        for y in (0..h).rev() {
            if rows[y * w + x] {
                upcoming = y as i64;
            }
            next[y] = upcoming;
        }
        for y in 0..h {
            if rows[y * w + x] {
                last = y as i64;
            }
            out[y * w + x] = y as i64 - last <= r || next[y] - y as i64 <= r;
        }
    }
    out
}

/// Cells within `r` 4-steps of a set cell (r = 0: the set cells themselves).
fn within(mask: &[bool], w: usize, h: usize, r: u32) -> Vec<bool> {
    let mut dist = vec![u32::MAX; w * h];
    let mut queue = VecDeque::new();
    for (i, &m) in mask.iter().enumerate() {
        if m {
            dist[i] = 0;
            queue.push_back(i);
        }
    }
    while let Some(c) = queue.pop_front() {
        if dist[c] >= r {
            continue;
        }
        let (x, y) = (c % w, c / w);
        let neighbors = [
            (x > 0).then(|| c - 1),
            (x + 1 < w).then(|| c + 1),
            (y > 0).then(|| c - w),
            (y + 1 < h).then(|| c + w),
        ];
        for n in neighbors.into_iter().flatten() {
            if dist[n] == u32::MAX {
                dist[n] = dist[c] + 1;
                queue.push_back(n);
            }
        }
    }
    dist.into_iter().map(|d| d != u32::MAX).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::grid;
    use crate::level::GeneratedLevel;

    fn level(config: &LevelConfig, seed: u64) -> GeneratedLevel {
        crate::generator::generate(config, seed)
    }

    /// Town minus every wall and house — what people can walk on.
    fn walkable(level: &GeneratedLevel) -> CellMask {
        let map = &level.regions.as_ref().unwrap().map;
        let mut m = CellMask::new(map.width, map.height, map.cell_size_m);
        let houses = &level.houses.as_ref().unwrap().mask;
        for i in 0..m.cells.len() {
            let wall = level.walls.as_ref().is_some_and(|w| w.mask.cells[i])
                || level.region_walls.as_ref().is_some_and(|w| w.mask.cells[i]);
            m.cells[i] = map.ids[i] != 0 && !wall && !houses.cells[i];
        }
        m
    }

    #[test]
    fn houses_never_overlap_roads_walls_or_each_other() {
        for seed in 1..=4u64 {
            let lvl = level(&LevelConfig::default(), seed);
            let layout = lvl.houses.as_ref().unwrap();
            assert!(!lvl.objects.is_empty(), "seed {seed}: no houses");
            let road = &lvl.roads.as_ref().unwrap().mask;
            let mut owner = vec![usize::MAX; layout.mask.cells.len()];
            let (w, h, cell) = (layout.mask.width, layout.mask.height, layout.mask.cell_size_m);
            for (k, o) in lvl.objects.iter().enumerate() {
                let cells = footprint(o.center(), (o.w_m, o.h_m), o.angle_deg, w, h, cell).unwrap();
                for c in cells {
                    assert!(!road.cells[c], "seed {seed}: house {k} on a road");
                    assert_eq!(owner[c], usize::MAX, "seed {seed}: houses {} and {k} overlap", owner[c]);
                    owner[c] = k;
                }
            }
        }
    }

    #[test]
    fn walkable_ground_stays_one_piece() {
        for seed in 1..=4u64 {
            let mut config = LevelConfig::default();
            config.houses.density_pct = 70.0;
            let lvl = level(&config, seed);
            assert!(grid::is_connected(&walkable(&lvl)), "seed {seed}: houses sealed off a pocket");
        }
    }

    #[test]
    fn density_controls_coverage_and_streets_fill_first() {
        let mut config = LevelConfig::default();
        config.houses.density_pct = 10.0;
        let sparse = level(&config, 3).houses.unwrap();
        config.houses.density_pct = 40.0;
        let dense = level(&config, 3).houses.unwrap();
        assert!(sparse.coverage_pct >= 9.0 && sparse.coverage_pct < 20.0, "{}", sparse.coverage_pct);
        assert!(dense.coverage_pct > sparse.coverage_pct * 2.0, "{} vs {}", dense.coverage_pct, sparse.coverage_pct);
        // A low density is (almost) only street fronts.
        assert!(sparse.street_houses > 0 && sparse.yard_houses <= sparse.street_houses / 4);
    }

    #[test]
    fn house_sizes_stay_in_range() {
        let mut config = LevelConfig::default();
        config.houses.size_min_m = 3.0;
        config.houses.size_max_m = 5.0;
        let lvl = level(&config, 6);
        for o in &lvl.objects {
            assert!((3.0..=5.0).contains(&o.w_m) && (3.0..=5.0).contains(&o.h_m), "{o:?}");
        }
    }

    #[test]
    fn only_towns_get_houses() {
        let mut config = LevelConfig::default();
        config.kind = crate::model::MapKind::Forest;
        config.shape = crate::model::ShapeSettings::for_kind(config.kind);
        let lvl = level(&config, 2);
        assert!(lvl.houses.is_none() && lvl.objects.is_empty());
    }

    #[test]
    fn same_seed_same_houses() {
        let config = LevelConfig::default();
        assert_eq!(level(&config, 5).objects, level(&config, 5).objects);
    }

    /// Eyeball helper: `cargo test ascii_houses_preview -- --nocapture`
    /// ('H' house, '=' road, '#' wall, '.' outside, ' ' open ground).
    #[test]
    fn ascii_houses_preview() {
        let lvl = level(&LevelConfig::default(), 42);
        let houses = lvl.houses.as_ref().unwrap();
        let map = &lvl.regions.as_ref().unwrap().map;
        for y in (0..map.height).step_by(2) {
            let row: String = (0..map.width)
                .map(|x| {
                    let i = y * map.width + x;
                    let wall = lvl.walls.as_ref().is_some_and(|w| w.mask.cells[i])
                        || lvl.region_walls.as_ref().is_some_and(|w| w.mask.cells[i]);
                    if houses.mask.cells[i] {
                        'H'
                    } else if wall {
                        '#'
                    } else if lvl.roads.as_ref().is_some_and(|r| r.mask.cells[i]) {
                        '='
                    } else if map.ids[i] == 0 {
                        '.'
                    } else {
                        ' '
                    }
                })
                .collect();
            println!("{row}");
        }
        println!(
            "street {} yard {} coverage {:.1}% of {:.0}%",
            houses.street_houses, houses.yard_houses, houses.coverage_pct, houses.target_pct
        );
    }
}
