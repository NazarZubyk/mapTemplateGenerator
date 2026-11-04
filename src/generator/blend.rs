//! Blending between the regions of a mixed map (docs/blending-plan.md).
//!
//! Every region builds its border as if it were the map edge; blending
//! gives each region, in its `RegionCtx`, how deep every cell lies in the
//! transition zone toward each neighbouring kind of land, and the chains
//! use it in the node where the transition belongs (yard trees in the
//! town's houses step, the forest's cut edge in its thickets step, …).
//!
//! The zones come from the region map alone, never from a neighbour's
//! result, so the chains stay independent. Their randomness is noise and
//! hashes of the blending seed: no chain's random stream gets extra draws.

use crate::generator::forest::distance_field;
use crate::generator::noise::NoiseField;
use crate::level::{Category, CellMask, MixedLayout, PlacedObject};
use crate::model::{BlendSettings, ForestTreeSettings, LevelConfig, MapKind};

/// The zone's width wavers by this share along the border (no stripes).
const WIDTH_WAVER: f32 = 0.4;
/// Scale of that wavering, meters.
const WAVER_SCALE_M: f32 = 40.0;

/// The blending seed of a mixed map: its own, so no chain stream moves.
pub fn seed(map_seed: u64) -> u64 {
    mix(map_seed ^ 0xB1E4_D000_0000_0001)
}

/// A well-mixed 64-bit hash (splitmix64's finalizer).
pub fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A hash of (`seed`, `c`, `salt`) as a number in [0, 1).
pub fn unit(seed: u64, c: usize, salt: u64) -> f32 {
    (mix(seed ^ mix(c as u64 ^ salt.wrapping_mul(0xA24B_AED4_963E_E407))) >> 40) as f32 / (1u64 << 24) as f32
}

fn kind_salt(k: MapKind) -> u64 {
    match k {
        MapKind::Town => 1,
        MapKind::Swamp => 2,
        MapKind::Forest => 3,
        MapKind::Cave => 4,
        MapKind::Mixed => 5,
    }
}

/// The transition zones of region `id` toward each neighbouring kind with
/// a width: per cell 1 at the border falling to 0 at the (wavering)
/// width, 0 outside the region. Kinds whose zone is empty are left out.
pub fn zones(m: &MixedLayout, id: u8, s: &BlendSettings, seed: u64) -> Vec<(MapKind, Vec<f32>)> {
    let (w, h, cell) = (m.map.width, m.map.height, m.map.cell_size_m);
    let kind_of = |rid: u8| m.regions.get(rid as usize - 1).map(|r| r.kind);
    let Some(own) = kind_of(id) else { return Vec::new() };
    let mut kinds: Vec<MapKind> = m.regions.iter().filter(|r| r.id != id && r.cells > 0).map(|r| r.kind).collect();
    kinds.sort_by_key(|&k| kind_salt(k));
    kinds.dedup();
    let mut out = Vec::new();
    for k in kinds {
        let width = s.width_m(own, k);
        if width <= 0.0 {
            continue;
        }
        let (d, _) = distance_field(w, h, cell, |c| m.map.ids[c] != 0 && m.map.ids[c] != id && kind_of(m.map.ids[c]) == Some(k), width * (1.0 + WIDTH_WAVER) + cell);
        let waver = NoiseField::new(seed ^ (kind_salt(own) << 8) ^ kind_salt(k));
        let t: Vec<f32> = (0..w * h)
            .map(|c| {
                if m.map.ids[c] != id || !d[c].is_finite() {
                    return 0.0;
                }
                let (x, y) = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
                let wv = width * (1.0 + WIDTH_WAVER * (waver.fbm(x / WAVER_SCALE_M, y / WAVER_SCALE_M, 3) * 2.0 - 1.0));
                // The first cell inside counts as on the border.
                (1.0 - (d[c] - cell).max(0.0) / wv.max(cell)).clamp(0.0, 1.0)
            })
            .collect();
        if t.iter().any(|&v| v > 0.0) {
            out.push((k, t));
        }
    }
    out
}

/// The tree settings of the forest region that shares the longest border
/// with region `id` (what its yard trees look like), if any.
pub fn neighbour_trees(config: &LevelConfig, m: &MixedLayout, id: u8) -> Option<ForestTreeSettings> {
    let (w, h) = (m.map.width, m.map.height);
    let mut contact = vec![0u32; m.regions.len() + 1];
    for y in 0..h {
        for x in 0..w {
            let c = y * w + x;
            if m.map.ids[c] != id {
                continue;
            }
            for nb in [(x + 1 < w).then(|| c + 1), (x > 0).then(|| c - 1), (y + 1 < h).then(|| c + w), (y > 0).then(|| c - w)].into_iter().flatten() {
                let o = m.map.ids[nb] as usize;
                if o != 0 && o != id as usize {
                    contact[o] += 1;
                }
            }
        }
    }
    (1..contact.len())
        .filter(|&o| contact[o] > 0 && m.regions[o - 1].kind == MapKind::Forest)
        .max_by_key(|&o| (contact[o], std::cmp::Reverse(o)))
        .and_then(|o| config.mixed.regions.get(o - 1))
        .map(|row| row.config.forest.trees)
}

/// The yard trees: the trunks and the cells they cover.
pub struct YardTrees {
    pub objects: Vec<PlacedObject>,
    pub mask: CellMask,
}

/// Trees per m² of free yard ground right at the border.
const YARD_TREES_PER_M2: f32 = 1.0 / 45.0;
/// Free ground kept all around a yard tree's trunk, meters.
const YARD_TREE_CLEAR_M: f32 = 1.5;
/// No yard tree this close to an opening (portal or map gate), meters.
const GATE_CLEAR_M: f32 = 8.0;

/// Trees in a town's yards near a forest (seeds blown over the wall):
/// on free town floor (`free`: no road, wall or house), densest at the
/// border and none past the zone (`t`), each with a ring of free ground
/// around it — so the ground stays one piece and the yards stay usable —
/// and none near an opening. Trunk sizes, spacing and color are the
/// forest neighbour's.
pub fn yard_trees(t: &[f32], trees: &ForestTreeSettings, free: &CellMask, gates: &[crate::generator::region::FixedGate], seed: u64) -> YardTrees {
    let (w, h, cell) = (free.width, free.height, free.cell_size_m);
    let mut mask = CellMask::new(w, h, cell);
    let mut objects = Vec::new();
    let (tmin, tmax) = (trees.trunk_min_m.min(trees.trunk_max_m).max(cell * 0.5), trees.trunk_min_m.max(trees.trunk_max_m).max(cell * 0.5));
    let reach = tmax / 2.0 + YARD_TREE_CLEAR_M;
    // Room around each cell: distance to the nearest cell that is not free.
    let (room, _) = distance_field(w, h, cell, |c| !free.cells[c], reach + cell * 2.0);
    // How many trees the zone's free ground asks for, densest at the border.
    let want = (YARD_TREES_PER_M2 * (0..w * h).filter(|&c| free.cells[c]).map(|c| t[c] * t[c]).sum::<f32>() * cell * cell).round() as usize;
    // Every free cell with room for the smallest tree, in a weighted
    // random order (key = u^(1/weight)): deep in the zone comes first more
    // often, and a cell past it never.
    let mut picks: Vec<(f32, usize)> = (0..w * h)
        .filter(|&c| t[c] > 0.0 && free.cells[c] && room[c] >= tmin / 2.0 + YARD_TREE_CLEAR_M)
        .map(|c| (unit(seed, c, 11).max(1e-6).powf(1.0 / (t[c] * t[c]).max(1e-3)), c))
        .collect();
    picks.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut placed: Vec<([f32; 2], f32)> = Vec::new();
    for (_, c) in picks {
        if placed.len() >= want {
            break;
        }
        let p = [((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell];
        // As big as the trunk sizes allow, as far as the room goes.
        let d = (tmin + (tmax - tmin) * unit(seed, c, 13).powf(1.5)).min(2.0 * (room[c] - YARD_TREE_CLEAR_M));
        let r = d / 2.0;
        if d < tmin {
            continue;
        }
        if gates.iter().any(|g| (g.point_m[0] - p[0]).hypot(g.point_m[1] - p[1]) < GATE_CLEAR_M) {
            continue;
        }
        let spacing = trees.spacing_m.max(2.0 * YARD_TREE_CLEAR_M);
        if placed.iter().any(|(q, qr)| (q[0] - p[0]).hypot(q[1] - p[1]) < (spacing + r + qr).max(r + qr + 2.0 * YARD_TREE_CLEAR_M)) {
            continue;
        }
        placed.push((p, r));
        let span = (r / cell).ceil() as i64 + 1;
        let (cx, cy) = ((c % w) as i64, (c / w) as i64);
        for dy in -span..=span {
            for dx in -span..=span {
                let (x, y) = (cx + dx, cy + dy);
                if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                    continue;
                }
                let q = [(x as f32 + 0.5) * cell, (y as f32 + 0.5) * cell];
                if (q[0] - p[0]).hypot(q[1] - p[1]) <= r {
                    mask.cells[y as usize * w + x as usize] = true;
                }
            }
        }
        objects.push(PlacedObject {
            category: Category::Structure,
            type_name: crate::generator::forest::trees::TRUNK_NAME.to_owned(),
            color: trees.color,
            x_m: p[0] - r,
            y_m: p[1] - r,
            w_m: d,
            h_m: d,
            angle_deg: 0.0,
            parts: Vec::new(),
            round: true,
        });
    }
    YardTrees { objects, mask }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_is_in_range_and_spread() {
        let v: Vec<f32> = (0..1000).map(|c| unit(7, c, 1)).collect();
        assert!(v.iter().all(|&x| (0.0..1.0).contains(&x)));
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        assert!((0.45..0.55).contains(&mean), "{mean}");
    }

    fn mixed(forest_town_m: f32) -> LevelConfig {
        let mut c = LevelConfig::of_kind(MapKind::Mixed);
        c.map.width_m = 160.0;
        c.map.height_m = 160.0;
        c.cell_size_m = 1.0;
        c.mixed.blending.forest_town_m = forest_town_m;
        c
    }

    /// Cells of `own` regions within `reach` m of a region of `other`
    /// kind where `pick(level, cell)` holds.
    fn count_by(m: &MixedLayout, own: MapKind, other: MapKind, reach: f32, pick: impl Fn(&crate::level::GeneratedLevel, usize) -> bool) -> usize {
        let (w, h, cell) = (m.map.width, m.map.height, m.map.cell_size_m);
        let kind = |c: usize| (m.map.ids[c] != 0).then(|| m.regions[m.map.ids[c] as usize - 1].kind);
        let (d, _) = distance_field(w, h, cell, |c| kind(c) == Some(other), reach + cell);
        m.regions
            .iter()
            .zip(&m.levels)
            .filter(|(r, _)| r.kind == own)
            .filter_map(|(r, l)| l.as_ref().map(|l| (r.id, l)))
            .map(|(id, l)| (0..w * h).filter(|&c| m.map.ids[c] == id && d[c] <= reach && pick(l, c)).count())
            .sum()
    }

    #[test]
    fn forest_and_swamp_blend_wet_and_wooded() {
        for seed in 1..=2 {
            let mut off = mixed(18.0);
            off.mixed.blending.forest_swamp_m = 0.0;
            let on = mixed(18.0);
            let (lo, ln) = (crate::generator::generate(&off, seed), crate::generator::generate(&on, seed));
            let (mo, mn) = (lo.mixed.as_ref().unwrap(), ln.mixed.as_ref().unwrap());
            if !mo.regions.iter().any(|r| r.kind == MapKind::Swamp) {
                continue;
            }
            // Forest: puddles (shallow water) near the swamp.
            let shallow = |l: &crate::level::GeneratedLevel, c: usize| {
                crate::level::ForestGround::from_code(l.forest.as_ref().unwrap().ground().unwrap()[c]) == crate::level::ForestGround::Shallow
            };
            let (a, b) = (count_by(mo, MapKind::Forest, MapKind::Swamp, 10.0, shallow), count_by(mn, MapKind::Forest, MapKind::Swamp, 10.0, shallow));
            assert!(b > a, "seed {seed}: forest puddles by the swamp {a} → {b}");
            // Swamp: thicket along the forest instead of open water.
            let thicket = |l: &crate::level::GeneratedLevel, c: usize| l.swamp.as_ref().unwrap().depth.as_ref().unwrap().thicket.cells[c];
            let (a, b) = (count_by(mo, MapKind::Swamp, MapKind::Forest, 4.0, thicket), count_by(mn, MapKind::Swamp, MapKind::Forest, 4.0, thicket));
            assert!(b > a, "seed {seed}: swamp thicket by the forest {a} → {b}");
        }
    }

    #[test]
    fn yard_trees_stand_in_the_town_near_the_forest_on_free_ground() {
        let width = 18.0;
        let mut total = 0;
        for seed in 1..=4 {
            let lvl = crate::generator::generate(&mixed(width), seed);
            let m = lvl.mixed.as_ref().unwrap();
            let (w, cell) = (m.map.width, m.map.cell_size_m);
            let forest: Vec<usize> = (0..m.map.ids.len()).filter(|&c| m.map.ids[c] != 0 && m.regions[m.map.ids[c] as usize - 1].kind == MapKind::Forest).collect();
            for (r, l) in m.regions.iter().zip(&m.levels) {
                let Some(l) = l.as_ref().filter(|_| r.kind == MapKind::Town) else { continue };
                let houses = l.houses.as_ref().unwrap();
                let trunks: Vec<&PlacedObject> = l.objects.iter().filter(|o| o.round && o.type_name == crate::generator::forest::trees::TRUNK_NAME).collect();
                assert_eq!(trunks.len() as u32, houses.yard_trees, "seed {seed}");
                total += trunks.len();
                let roads = &l.roads.as_ref().unwrap().mask;
                for o in trunks {
                    let p = [o.x_m + o.w_m / 2.0, o.y_m + o.h_m / 2.0];
                    let c = (p[1] / cell) as usize * w + (p[0] / cell) as usize;
                    assert_eq!(m.map.ids[c], r.id, "seed {seed}: a yard tree outside its town");
                    assert!(!roads.cells[c], "seed {seed}: a yard tree on a road");
                    let near = forest.iter().map(|&f| (((f % w) as f32 + 0.5) * cell - p[0]).hypot(((f / w) as f32 + 0.5) * cell - p[1])).fold(f32::INFINITY, f32::min);
                    assert!(near <= width * (1.0 + WIDTH_WAVER) + 2.0 * cell, "seed {seed}: a yard tree {near:.1} m from the forest");
                    // Not on another object.
                    for q in &l.objects {
                        if !std::ptr::eq(q, o) && q.category != Category::MobSpawn {
                            assert!(!q.contains(p[0], p[1]), "seed {seed}: a yard tree under a {}", q.type_name);
                        }
                    }
                }
            }
        }
        assert!(total > 0, "no yard trees on four maps");
    }

    #[test]
    fn blending_off_adds_nothing_and_cuts_nothing() {
        for seed in 1..=2 {
            let off = crate::generator::generate(&mixed(0.0), seed);
            let on = crate::generator::generate(&mixed(18.0), seed);
            let (mo, mn) = (off.mixed.as_ref().unwrap(), on.mixed.as_ref().unwrap());
            assert!(mo.levels.iter().flatten().all(|l| l.houses.as_ref().is_none_or(|h| h.yard_trees == 0)));
            // The forest's thicket within 6 m of a town: less with blending.
            let thicket_by_town = |m: &MixedLayout| {
                let (w, h, cell) = (m.map.width, m.map.height, m.map.cell_size_m);
                let (d, _) = distance_field(w, h, cell, |c| m.map.ids[c] != 0 && m.regions[m.map.ids[c] as usize - 1].kind == MapKind::Town, 8.0);
                m.regions.iter().zip(&m.levels).filter(|(r, _)| r.kind == MapKind::Forest).filter_map(|(_, l)| l.as_ref()).map(|l| {
                    let g = l.forest.as_ref().unwrap().ground().unwrap();
                    (0..w * h).filter(|&c| d[c] <= 6.0 && m.map.ids[c] != 0 && crate::level::ForestGround::from_code(g[c]) == crate::level::ForestGround::Thicket && m.regions[m.map.ids[c] as usize - 1].kind == MapKind::Forest).count()
                }).sum::<usize>()
            };
            let (a, b) = (thicket_by_town(mo), thicket_by_town(mn));
            assert!(b < a, "seed {seed}: thicket by the town {a} → {b}");
        }
    }
}
