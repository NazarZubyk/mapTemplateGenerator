//! Swamp node 6, trees (step S5 in docs/swamp-plan.md): the swamp's
//! structure (a town has houses). Round, solid objects.
//!
//! 1. **Grove zones** — the island ground (not hummocks, not within
//!    `gate_clear_m` of a gate point) with the highest values of a 14 m
//!    noise field, `grove_area_pct` of it; the rest stays clearing.
//! 2. **Groves** — trees at random spots inside the zones, diameter rolled
//!    in `size_min_m..size_max_m`, until they cover `density_pct` of the
//!    zones or no spot is left.
//! 3. **Water trees** — with `water_trees`, `water_per_1000m2` single trees
//!    standing in shallow water and deep mud (never on a mud path or a
//!    boardwalk, not near a gate).
//!
//! Every tree keeps a one-cell ring of free walkable ground (no other tree,
//! no deep water or thicket): solid objects never touch anything that
//! blocks, so the walkable ground stays one piece.

use rand::seq::SliceRandom;
use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::noise::NoiseField;
use crate::level::{Category, CellMask, DepthLayout, GateAnchor, IslandKind, IslandLayout, PlacedObject, TreeLayout};
use crate::model::LevelConfig;

pub const TREE_NAME: &str = "tree";
pub const WATER_TREE_NAME: &str = "water tree";
/// Grove noise feature size, meters.
const GROVE_SCALE_M: f32 = 14.0;

pub struct TreeBuild {
    pub layout: TreeLayout,
    pub objects: Vec<PlacedObject>,
    pub notes: Vec<String>,
}

pub fn build(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    anchors: &[GateAnchor],
    islands: &IslandLayout,
    depth: &DepthLayout,
) -> TreeBuild {
    let s = &config.swamp.trees;
    let (w, h, cell) = (islands.width, islands.height, islands.cell_size_m);
    let noise = NoiseField::new(rng.random::<u64>());
    let mut notes = Vec::new();
    let center = |c: usize| (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
    let clear_of_gates = |c: usize| {
        let (x, y) = center(c);
        anchors.iter().all(|a| (a.point_m[0] - x).hypot(a.point_m[1] - y) > s.gate_clear_m.max(0.0))
    };
    let walkable: Vec<bool> = (0..w * h).map(|c| !depth.deep.cells[c] && !depth.thicket.cells[c]).collect();
    let (lo, hi) = (s.size_min_m.min(s.size_max_m).max(cell), s.size_max_m.max(s.size_min_m).max(cell));
    let mut solid = vec![false; w * h];
    let mut objects = Vec::new();

    // Places a tree of diameter `d` on cell `at` if its footprint is all
    // `allowed` and its ring all free walkable ground; returns its cells.
    let place = |at: usize, d: f32, allowed: &dyn Fn(usize) -> bool, solid: &mut [bool]| -> Option<usize> {
        let (cx, cy) = center(at);
        let r = d / 2.0;
        let ri = (r / cell).ceil() as i64 + 1;
        let (ax, ay) = ((at % w) as i64, (at / w) as i64);
        let mut foot = Vec::new();
        for dy in -ri..=ri {
            for dx in -ri..=ri {
                let (x, y) = (ax + dx, ay + dy);
                if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                    continue;
                }
                let c = y as usize * w + x as usize;
                let (px, py) = center(c);
                if c == at || (px - cx).hypot(py - cy) <= r {
                    if !allowed(c) || solid[c] {
                        return None;
                    }
                    foot.push(c);
                }
            }
        }
        for &c in &foot {
            let (x, y) = ((c % w) as i64, (c / w) as i64);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        return None;
                    }
                    let n = ny as usize * w + nx as usize;
                    if !walkable[n] || solid[n] {
                        return None;
                    }
                }
            }
        }
        for &c in &foot {
            solid[c] = true;
        }
        Some(foot.len())
    };
    let tree = |at: usize, d: f32, name: &str, color: [u8; 3]| {
        let (x, y) = center(at);
        PlacedObject {
            category: Category::Structure,
            type_name: name.to_owned(),
            color,
            x_m: x - d / 2.0,
            y_m: y - d / 2.0,
            w_m: d,
            h_m: d,
            angle_deg: 0.0,
            parts: Vec::new(),
            round: true,
        }
    };
    let roll = |rng: &mut ChaCha8Rng| if hi - lo < 1e-6 { lo } else { rng.random_range(lo..=hi) };

    // 1. Grove zones.
    let ground: Vec<usize> = (0..w * h)
        .filter(|&c| {
            let id = islands.ids[c];
            id > 0 && islands.islands[id as usize - 1].kind != IslandKind::Hummock && clear_of_gates(c)
        })
        .collect();
    let value = |c: usize| {
        let (x, y) = center(c);
        noise.fbm(x / GROVE_SCALE_M, y / GROVE_SCALE_M, 3)
    };
    let mut ranked: Vec<(f32, usize)> = ground.iter().map(|&c| (value(c), c)).collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let zone_n = ((s.grove_area_pct.clamp(0.0, 100.0) / 100.0) * ranked.len() as f32).round() as usize;
    let mut zone = vec![false; w * h];
    let mut zone_cells: Vec<usize> = ranked.iter().take(zone_n).map(|&(_, c)| c).collect();
    for &c in &zone_cells {
        zone[c] = true;
    }

    // 2. Groves.
    let target = (s.density_pct.clamp(0.0, 100.0) / 100.0 * zone_n as f32).round() as usize;
    zone_cells.shuffle(rng);
    let mut covered = 0;
    let mut grove_trees = 0;
    for &at in &zone_cells {
        if covered >= target {
            break;
        }
        let d = roll(rng);
        if let Some(n) = place(at, d, &|c| zone[c], &mut solid) {
            covered += n;
            grove_trees += 1;
            objects.push(tree(at, d, TREE_NAME, s.color));
        }
    }
    if target > 0 && (covered as f32) < target as f32 * 0.8 {
        notes.push(format!(
            "Trees: groves cover {:.0}% of their zones (asked {:.0}%) — smaller trees or a lower density",
            covered as f32 / zone_n.max(1) as f32 * 100.0,
            s.density_pct
        ));
    }

    // 3. Water trees.
    let mut water_trees = 0;
    if s.water_trees {
        let mut water: Vec<usize> =
            (0..w * h).filter(|&c| (depth.shallow.cells[c] || depth.mud.cells[c]) && clear_of_gates(c)).collect();
        let want = (s.water_per_1000m2.max(0.0) * water.len() as f32 * cell * cell / 1000.0).round() as u32;
        let is_water = |c: usize| depth.shallow.cells[c] || depth.mud.cells[c];
        water.shuffle(rng);
        for &at in &water {
            if water_trees >= want {
                break;
            }
            let d = roll(rng);
            if place(at, d, &is_water, &mut solid).is_some() {
                water_trees += 1;
                objects.push(tree(at, d, WATER_TREE_NAME, s.water_color));
            }
        }
        if water_trees < want {
            notes.push(format!("Trees: {water_trees} of {want} water trees found room"));
        }
    }

    TreeBuild {
        layout: TreeLayout {
            mask: CellMask { width: w, height: h, cell_size_m: cell, cells: solid },
            grove_trees,
            water_trees,
            grove_cover_pct: covered as f32 / zone_n.max(1) as f32 * 100.0,
        },
        objects,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::generator::grid;
    use crate::generator::swamp::{depth, gate_anchors, islands, paths};
    use crate::level::PathLayout;
    use crate::model::{GateSpec, GateStyle, MapKind};

    fn config() -> LevelConfig {
        let mut c = LevelConfig { kind: MapKind::Swamp, ..LevelConfig::default() };
        c.map.width_m = 140.0;
        c.map.height_m = 120.0;
        c.cell_size_m = 1.0;
        c.gates.gates = vec![
            GateSpec { direction_deg: 90.0, style: GateStyle::Shore },
            GateSpec { direction_deg: 270.0, style: GateStyle::Bridge },
        ];
        c
    }

    struct All {
        anchors: Vec<GateAnchor>,
        islands: IslandLayout,
        paths: PathLayout,
        depth: DepthLayout,
        trees: TreeBuild,
    }

    fn build_all(c: &LevelConfig, seed: u64) -> All {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let anchors = gate_anchors::build(c, &mut rng).anchors;
        let mut isl = islands::build(c, &mut rng, &anchors).layout;
        let mut p = paths::build(c, &mut rng, &anchors, &isl).layout;
        let d = depth::build(c, &mut rng, &mut isl, &mut p).layout;
        let trees = build(c, &mut rng, &anchors, &isl, &d);
        All { anchors, islands: isl, paths: p, depth: d, trees }
    }

    #[test]
    fn walkable_ground_stays_one_piece_with_dense_trees() {
        let mut c = config();
        c.swamp.trees.density_pct = 90.0;
        c.swamp.trees.grove_area_pct = 100.0;
        c.swamp.trees.water_per_1000m2 = 20.0;
        for seed in 0..3 {
            let a = build_all(&c, seed);
            let walk = CellMask {
                width: a.depth.deep.width,
                height: a.depth.deep.height,
                cell_size_m: 1.0,
                cells: (0..a.depth.depth.len())
                    .map(|k| !a.depth.deep.cells[k] && !a.depth.thicket.cells[k] && !a.trees.layout.mask.cells[k])
                    .collect(),
            };
            assert!(grid::is_connected(&walk), "seed {seed}: trees split the walkable ground");
            assert!(a.trees.layout.grove_trees > 20);
        }
    }

    #[test]
    fn trees_stand_where_they_belong() {
        let c = config();
        let a = build_all(&c, 1);
        let t = &a.trees.layout;
        assert!(t.grove_trees > 0 && t.water_trees > 0, "{t:?}");
        for k in 0..t.mask.cells.len() {
            if t.mask.cells[k] {
                assert!(!a.paths.mud.cells[k] && !a.paths.boardwalk.cells[k], "tree on a path at {k}");
                let id = a.islands.ids[k];
                assert!(id == 0 || a.islands.islands[id as usize - 1].kind != IslandKind::Hummock);
            }
        }
        for o in &a.trees.objects {
            assert!(o.round && o.category == Category::Structure);
            assert!((1.5..=4.0).contains(&o.w_m));
            let (x, y) = o.center();
            for g in &a.anchors {
                assert!((g.point_m[0] - x).hypot(g.point_m[1] - y) > 6.0, "tree at a gate");
            }
        }
    }

    #[test]
    fn density_and_toggles_drive_the_count() {
        let mut c = config();
        c.swamp.trees.density_pct = 0.0;
        c.swamp.trees.water_trees = false;
        let a = build_all(&c, 2);
        assert!(a.trees.objects.is_empty());
        c.swamp.trees.density_pct = 15.0;
        let sparse = build_all(&c, 2).trees.layout.grove_trees;
        c.swamp.trees.density_pct = 45.0;
        let dense = build_all(&c, 2).trees.layout;
        assert!(dense.grove_trees > sparse * 2, "{sparse} vs {}", dense.grove_trees);
        assert_eq!(dense.water_trees, 0);
    }

    #[test]
    fn same_seed_same_trees() {
        let c = config();
        assert_eq!(build_all(&c, 4).trees.objects, build_all(&c, 4).trees.objects);
    }
}
