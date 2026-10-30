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
//! **Complex houses** — `complex_pct` of the houses get 1 or 2 wings
//! (`max_parts` rectangles in all): smaller rectangles (`wing_min_pct`..
//! `wing_max_pct` of the main block's sides) overlapping a side of the main
//! block, flush with one of its ends or anywhere along it → L, T, U, Z and
//! cross shapes. A street house never gets a wing on its road side, so its
//! front stays on the street. A shape whose wings would close a courtyard
//! is rejected, and a complex house that does not fit falls back to the
//! plain block before it shrinks.
//!
//! Every house keeps at least one free cell (Chebyshev) to every other house
//! and wall, and a house is one solid piece without holes — so the walkable
//! ground can never be sealed into a pocket. Width and depth of the main
//! block are rolled independently in `[size_min_m, size_max_m]`.

use rand::seq::SliceRandom;
use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::placement::{dilate, footprint, nearest_road_angle, path_tangents, within};
use crate::generator::Stage;
use crate::level::{Category, CellMask, HouseLayout, ObjectPart, PlacedObject, RegionMap, RoadNetwork};
use crate::model::LevelConfig;

/// A spot along a road where a street-front house may stand.
struct StreetSlot {
    /// Road centerline point, meters.
    point: (f32, f32),
    /// Unit normal pointing to this side of the road.
    normal: (f32, f32),
    /// Road direction, degrees.
    angle_deg: f32,
    /// Half the width of this road, meters.
    half_road: f32,
    /// Which side of the road (±1): the road lies toward the house's local
    /// −side·v axis, so wings never go there.
    side: f32,
}

/// Display name of a house (map legend, spec.json).
pub const HOUSE_NAME: &str = "house";
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
                 parts: &[ObjectPart],
                 angle_deg: f32|
     -> bool {
        let Some(cells) = footprint(center, size, parts, angle_deg, w, h, cell) else {
            return false;
        };
        if cells.is_empty() || !cells.iter().all(|&c| free[c]) {
            return false;
        }
        // A compound house must not close its own courtyard (a U whose wings
        // almost touch): every gap between its parts stays open.
        if !parts.is_empty() && !courtyards_open(&cells, w) {
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
            parts: parts.to_vec(),
        });
        true
    };
    let complex = s.complex_pct.clamp(0.0, 100.0) / 100.0;
    let max_extra = s.max_parts.clamp(1, 3) - 1;
    let wing_lo = (s.wing_min_pct.min(s.wing_max_pct) / 100.0).clamp(0.1, 1.0);
    let wing_hi = (s.wing_min_pct.max(s.wing_max_pct) / 100.0).clamp(0.1, 1.0);
    let mut complex_houses = 0u32;
    // Roll a house's wings (empty for a plain house). `forbid` = the side
    // that must stay flat (a street house's road side).
    let roll_wings = |rng: &mut ChaCha8Rng, forbid: Option<Side>| -> Vec<WingSpec> {
        if max_extra == 0 || rng.random::<f32>() >= complex {
            return Vec::new();
        }
        let n = rng.random_range(1..=max_extra);
        wing_specs(rng, n as usize, (wing_lo, wing_hi), forbid)
    };

    // 2. Street fronts.
    stage.report("Lining the streets", 0.0);
    let mut street_houses = 0u32;
    if let Some(net) = ground.roads {

        let mut slots: Vec<StreetSlot> = Vec::new();
        for link in &net.links {
            let half_road = link.width_cells.max(1) as f32 * cell / 2.0;
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
                        half_road,
                        side,
                    });
                    next_at = walked + rng.random_range(lo..=hi) + s.spacing_m.max(cell);
                }
            }
        }
        slots.shuffle(rng);
        for (k, StreetSlot { point: p, normal: n, angle_deg: angle, half_road, side }) in
            slots.into_iter().enumerate()
        {
            if (covered as f32) >= target {
                break;
            }
            if k % 32 == 0 {
                stage.report("Lining the streets", 0.5 * (covered as f32 / target.max(1.0)).min(1.0));
            }
            let size = (rng.random_range(lo..=hi), rng.random_range(lo..=hi));
            // The road lies toward the house's local −side·v: keep it flat.
            let wings = roll_wings(rng, Some(if side > 0.0 { Side::MinusV } else { Side::PlusV }));
            // Width runs along the road. Try the setback, then a little
            // deeper; a complex house that still does not fit drops its
            // wings, then the house gets shallower and narrower toward the
            // minimum size.
            let shrink = |f: f32| (lo + (size.0 - lo) * f, lo + (size.1 - lo) * f);
            'fit: for fit in [shrink(1.0), shrink(0.5), shrink(0.0)] {
                for with_wings in [true, false] {
                    if with_wings && wings.is_empty() {
                        continue;
                    }
                    let parts = if with_wings { wing_parts(&wings, fit, cell) } else { Vec::new() };
                    for extra in [0.0f32, 0.5, 1.0] {
                        let depth = half_road + s.setback_m.max(0.0) + extra + fit.1 / 2.0 + cell * 0.5;
                        let center = (p.0 + n.0 * depth, p.1 + n.1 * depth);
                        if place(&mut objects, &mut free, &mut house_mask, &mut covered, center, fit, &parts, angle) {
                            street_houses += 1;
                            complex_houses += u32::from(!parts.is_empty());
                            break 'fit;
                        }
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
        let wings = roll_wings(rng, None);
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
            // Tight spot: drop the wings, shrink toward the minimum size, and
            // try the house turned 90°, before giving up here.
            let shrink = |f: f32| (lo + (size.0 - lo) * f, lo + (size.1 - lo) * f);
            'fit: for fit in [shrink(1.0), shrink(0.5), shrink(0.0)] {
                for with_wings in [true, false] {
                    if with_wings && wings.is_empty() {
                        continue;
                    }
                    let parts = if with_wings { wing_parts(&wings, fit, cell) } else { Vec::new() };
                    for turn in [0.0f32, 90.0] {
                        if place(&mut objects, &mut free, &mut house_mask, &mut covered, center, fit, &parts, angle + turn) {
                            placed = true;
                            complex_houses += u32::from(!parts.is_empty());
                            break 'fit;
                        }
                    }
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
            complex_houses,
            coverage_pct,
            target_pct,
        },
        objects,
        notes,
    })
}

/// Is every non-house cell around the house reachable (4-steps) from outside
/// its bounding box? False when the parts enclose a courtyard.
fn courtyards_open(cells: &[usize], w: usize) -> bool {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for &c in cells {
        let (x, y) = (c % w, c / w);
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    // Local box with a one-cell margin all round (the margin is outside).
    let (bw, bh) = (x1 - x0 + 3, y1 - y0 + 3);
    let mut house = vec![false; bw * bh];
    for &c in cells {
        house[(c / w - y0 + 1) * bw + (c % w - x0 + 1)] = true;
    }
    let mut reached = vec![false; bw * bh];
    let mut stack = vec![0usize];
    reached[0] = true;
    while let Some(i) = stack.pop() {
        let (x, y) = (i % bw, i / bw);
        let neighbors = [
            (x > 0).then(|| i - 1),
            (x + 1 < bw).then(|| i + 1),
            (y > 0).then(|| i - bw),
            (y + 1 < bh).then(|| i + bw),
        ];
        for n in neighbors.into_iter().flatten() {
            if !house[n] && !reached[n] {
                reached[n] = true;
                stack.push(n);
            }
        }
    }
    house.iter().zip(&reached).all(|(&hs, &r)| hs || r)
}

/// A side of a house's main block, in its own frame (u along the width,
/// v along the depth).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    PlusU,
    MinusU,
    PlusV,
    MinusV,
}

/// One wing as fractions of the main block, so it can be rebuilt for any
/// (shrunk) main size.
#[derive(Debug, Clone, Copy)]
struct WingSpec {
    side: Side,
    /// Length along the side, as a share of that side.
    along: f32,
    /// How far it sticks out, as a share of the block's other dimension.
    out: f32,
    /// Where along the side: 0 = flush with its start, 1 = flush with its end.
    at: f32,
}

/// Roll `n` wings on distinct sides (never `forbid`). A second wing may go on
/// the same side as the first, at the opposite end — a U shape. Wings are
/// flush with an end of their side 60% of the time (crisp L / U corners).
fn wing_specs(rng: &mut ChaCha8Rng, n: usize, size: (f32, f32), forbid: Option<Side>) -> Vec<WingSpec> {
    let mut sides: Vec<Side> = [Side::PlusU, Side::MinusU, Side::PlusV, Side::MinusV]
        .into_iter()
        .filter(|&s| Some(s) != forbid)
        .collect();
    let mut specs: Vec<WingSpec> = Vec::with_capacity(n);
    for i in 0..n {
        let (lo, hi) = size;
        let mut spec = WingSpec {
            side: sides[rng.random_range(0..sides.len())],
            along: rng.random_range(lo..=hi),
            out: rng.random_range(lo..=hi),
            at: if rng.random_bool(0.6) { f32::from(u8::from(rng.random_bool(0.5))) } else { rng.random() },
        };
        if i == 1 && rng.random_bool(0.4) {
            // U shape: the first wing's side, flush with the other end.
            let first = specs[0];
            spec.side = first.side;
            spec.at = if first.at < 0.5 { 1.0 } else { 0.0 };
            spec.along = spec.along.min(0.45);
            specs[0].at = 1.0 - spec.at;
            specs[0].along = specs[0].along.min(0.45);
        }
        sides.retain(|&s| s != spec.side);
        if sides.is_empty() {
            sides.push(spec.side);
        }
        specs.push(spec);
    }
    specs
}

/// The wings as parts for a main block of `size` (width, depth). Each wing
/// overlaps the block by a third of its reach (at least one cell), so the
/// house is always one solid piece.
fn wing_parts(specs: &[WingSpec], size: (f32, f32), cell: f32) -> Vec<ObjectPart> {
    let (width, depth) = size;
    specs
        .iter()
        .map(|spec| {
            // ±v sides run along the width; ±u sides along the depth.
            let (side_len, other) = match spec.side {
                Side::PlusV | Side::MinusV => (width, depth),
                Side::PlusU | Side::MinusU => (depth, width),
            };
            let len = (side_len * spec.along).clamp(cell, side_len);
            let out = (other * spec.out).max(cell);
            let overlap = (out / 3.0).max(cell);
            let along_center = -side_len / 2.0 + len / 2.0 + (side_len - len) * spec.at;
            let out_center = other / 2.0 + (out - overlap) / 2.0;
            let thick = out + overlap;
            match spec.side {
                Side::PlusU => ObjectPart { u_m: out_center, v_m: along_center, w_m: thick, h_m: len },
                Side::MinusU => ObjectPart { u_m: -out_center, v_m: along_center, w_m: thick, h_m: len },
                Side::PlusV => ObjectPart { u_m: along_center, v_m: out_center, w_m: len, h_m: thick },
                Side::MinusV => ObjectPart { u_m: along_center, v_m: -out_center, w_m: len, h_m: thick },
            }
        })
        .collect()
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
            let lvl = level(&LevelConfig::test_64(), seed);
            let layout = lvl.houses.as_ref().unwrap();
            assert!(!lvl.objects.is_empty(), "seed {seed}: no houses");
            let road = &lvl.roads.as_ref().unwrap().mask;
            let mut owner = vec![usize::MAX; layout.mask.cells.len()];
            let (w, h, cell) = (layout.mask.width, layout.mask.height, layout.mask.cell_size_m);
            for (k, o) in lvl.objects.iter().filter(|o| o.category == Category::Structure).enumerate() {
                let cells = footprint(o.center(), (o.w_m, o.h_m), &o.parts, o.angle_deg, w, h, cell).unwrap();
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
            let mut config = LevelConfig::test_64();
            config.houses.density_pct = 70.0;
            let lvl = level(&config, seed);
            assert!(grid::is_connected(&walkable(&lvl)), "seed {seed}: houses sealed off a pocket");
        }
    }

    #[test]
    fn density_controls_coverage_and_streets_fill_first() {
        let mut config = LevelConfig::test_64();
        config.houses.density_pct = 10.0;
        let sparse = level(&config, 3).houses.unwrap();
        config.houses.density_pct = 40.0;
        let dense = level(&config, 3).houses.unwrap();
        assert!(sparse.coverage_pct >= 9.0 && sparse.coverage_pct < 20.0, "{}", sparse.coverage_pct);
        assert!(dense.coverage_pct > sparse.coverage_pct * 1.8, "{} vs {}", dense.coverage_pct, sparse.coverage_pct);
        // A low density is (almost) only street fronts.
        assert!(sparse.street_houses > 0 && sparse.yard_houses <= sparse.street_houses / 4);
    }

    #[test]
    fn complex_houses_have_overlapping_wings() {
        let mut config = LevelConfig::test_64();
        config.map.width_m = 128.0;
        config.map.height_m = 128.0;
        config.houses.complex_pct = 50.0;
        let lvl = level(&config, 3);
        let layout = lvl.houses.as_ref().unwrap();
        let complex: Vec<&PlacedObject> = lvl.objects.iter().filter(|o| !o.parts.is_empty()).collect();
        assert_eq!(complex.len() as u32, layout.complex_houses);
        let total = lvl.objects.iter().filter(|o| o.category == Category::Structure).count() as f32;
        assert!(complex.len() as f32 > total * 0.2, "{} of {total} complex", complex.len());
        assert!(complex.iter().any(|o| o.parts.len() == 2), "some houses have 3 rectangles");
        for o in &complex {
            assert!(o.parts.len() <= 2);
            for p in &o.parts {
                // Each wing overlaps the main block (shares its center region).
                let overlap_u = (o.w_m / 2.0 + p.w_m / 2.0) - p.u_m.abs();
                let overlap_v = (o.h_m / 2.0 + p.h_m / 2.0) - p.v_m.abs();
                assert!(overlap_u > 0.0 && overlap_v > 0.0, "{o:?}");
            }
        }
        // Off: no wings at all; max 2 parts: at most one wing.
        config.houses.complex_pct = 0.0;
        assert!(level(&config, 3).objects.iter().all(|o| o.parts.is_empty()));
        config.houses.complex_pct = 100.0;
        config.houses.max_parts = 2;
        assert!(level(&config, 3).objects.iter().all(|o| o.parts.len() <= 1));
    }

    #[test]
    fn a_closed_courtyard_is_detected() {
        // 5×4 ring of house cells around one empty cell (w = 10).
        let w = 10;
        let ring: Vec<usize> = (0..4)
            .flat_map(|y| (0..5).map(move |x| (x, y)))
            .filter(|&(x, y)| !(x == 2 && (y == 1 || y == 2)))
            .map(|(x, y)| y * w + x)
            .collect();
        assert!(!courtyards_open(&ring, w));
        // Open the ring's bottom: the courtyard reaches outside.
        let open: Vec<usize> = ring.into_iter().filter(|&c| c != 3 * w + 2).collect();
        assert!(courtyards_open(&open, w));
    }

    #[test]
    fn street_houses_keep_a_flat_front() {
        let mut rng = <ChaCha8Rng as rand_chacha::rand_core::SeedableRng>::seed_from_u64(1);
        for _ in 0..200 {
            for forbid in [Side::PlusV, Side::MinusV] {
                let specs = wing_specs(&mut rng, 2, (0.4, 0.75), Some(forbid));
                assert!(specs.iter().all(|s| s.side != forbid));
            }
        }
    }

    #[test]
    fn house_sizes_stay_in_range() {
        let mut config = LevelConfig::test_64();
        config.houses.complex_pct = 0.0;
        config.houses.size_min_m = 3.0;
        config.houses.size_max_m = 5.0;
        let lvl = level(&config, 6);
        for o in lvl.objects.iter().filter(|o| o.category == Category::Structure) {
            assert!((3.0..=5.0).contains(&o.w_m) && (3.0..=5.0).contains(&o.h_m), "{o:?}");
        }
    }

    #[test]
    fn only_towns_get_houses() {
        let mut config = LevelConfig::test_64();
        config.kind = crate::model::MapKind::Forest;
        config.shape = crate::model::ShapeSettings::for_kind(config.kind);
        let lvl = level(&config, 2);
        assert!(lvl.houses.is_none());
        assert!(lvl.objects.iter().all(|o| o.category != Category::Structure));
    }

    #[test]
    fn same_seed_same_houses() {
        let config = LevelConfig::test_64();
        assert_eq!(level(&config, 5).objects, level(&config, 5).objects);
    }

    /// Eyeball helper: `cargo test ascii_houses_preview -- --nocapture`
    /// ('H' house, '=' road, '#' wall, '.' outside, ' ' open ground).
    #[test]
    fn ascii_houses_preview() {
        let lvl = level(&LevelConfig::test_64(), 42);
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
