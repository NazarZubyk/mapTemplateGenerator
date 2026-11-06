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

/// Scree: boulders per m² of free ground right at the massif's foot.
const SCREE_PER_M2: f32 = 1.0 / 35.0;
/// Boulder sizes, meters: the smallest far out, the biggest at the face.
const SCREE_MIN_M: f32 = 0.6;
const SCREE_MAX_M: f32 = 2.6;
/// Share of the boulders that are round (the rest rotated blocks).
const SCREE_ROUND: f32 = 0.6;
/// Forest: share of the ground under rock patches at the massif's foot.
const SCREE_ROCK_SHARE: f32 = 0.3;
/// Size of the rock patches, meters.
const SCREE_ROCK_SCALE_M: f32 = 7.0;

/// Where a region's scree may go: every slice is one value per cell of
/// the map grid.
pub struct ScreeGround<'a> {
    pub w: usize,
    pub h: usize,
    pub cell: f32,
    /// Cells a boulder may cover.
    pub place: &'a [bool],
    /// Walkable ground (what must stay one piece), objects not counted.
    pub walkable: &'a [bool],
    /// Cells a boulder and the ring around it keep off: objects, and
    /// whatever must stay clear (paths, boardwalks).
    pub taken: &'a [bool],
    /// Openings (portals, map gates): nothing within the gate clearance.
    pub gates: &'a [[f32; 2]],
}

/// The scree: the boulders and the cells they cover.
pub struct Scree {
    pub objects: Vec<PlacedObject>,
    pub mask: CellMask,
}

/// Boulders fallen from a neighbouring massif (`t`: the zone toward the
/// mountains, 1 at the foot of the face): plain blocked props in `color`,
/// denser and bigger nearer the face, none past the zone or near an
/// opening. A boulder covers only `place` cells and keeps a one-cell ring
/// off `taken` and off other boulders; one that covers walkable ground has
/// walkable ground all around it, so the ground stays one piece. A
/// boulder may stand in deep water (a rock in the water) — it takes no
/// walkable ground there. Hashes of `seed` only: no chain stream moves.
pub fn scree(t: &[f32], g: &ScreeGround, color: [u8; 3], seed: u64) -> Scree {
    let (w, h, cell) = (g.w, g.h, g.cell);
    let mut mask = CellMask::new(w, h, cell);
    let mut objects = Vec::new();
    let open = |c: usize| t[c] > 0.0 && g.place[c] && !g.taken[c];
    let want = (SCREE_PER_M2 * (0..w * h).filter(|&c| open(c)).map(|c| t[c] * t[c]).sum::<f32>() * cell * cell).round() as usize;
    if want == 0 {
        return Scree { objects, mask };
    }
    // A weighted random order (key = u^(1/weight)): the foot comes first.
    let mut picks: Vec<(f32, usize)> = (0..w * h).filter(|&c| open(c)).map(|c| (unit(seed, c, 21).max(1e-6).powf(1.0 / (t[c] * t[c]).max(1e-3)), c)).collect();
    picks.sort_by(|a, b| b.0.total_cmp(&a.0));
    let tries = want * 40;
    for (k, (_, c)) in picks.into_iter().enumerate() {
        if objects.len() >= want || k >= tries {
            break;
        }
        let jitter = |salt| (unit(seed, c, salt) - 0.5) * cell;
        let p = [((c % w) as f32 + 0.5) * cell + jitter(22), ((c / w) as f32 + 0.5) * cell + jitter(23)];
        let d = (SCREE_MIN_M + (SCREE_MAX_M - SCREE_MIN_M) * (0.55 * t[c] + 0.45 * unit(seed, c, 24)).powf(1.3)).max(cell);
        if g.gates.iter().any(|q| (q[0] - p[0]).hypot(q[1] - p[1]) < GATE_CLEAR_M + d / 2.0) {
            continue;
        }
        let round = unit(seed, c, 25) < SCREE_ROUND;
        let (ow, oh, angle) = if round { (d, d, 0.0) } else { (d, d * (0.6 + 0.4 * unit(seed, c, 26)), 180.0 * unit(seed, c, 27)) };
        let o = PlacedObject {
            category: Category::Prop,
            type_name: crate::generator::props::PROP_NAME.to_owned(),
            color,
            x_m: p[0] - ow / 2.0,
            y_m: p[1] - oh / 2.0,
            w_m: ow,
            h_m: oh,
            angle_deg: angle,
            parts: Vec::new(),
            round,
        };
        // The footprint as the objects masks count it: covered cells plus
        // the center cell.
        let r = ow.max(oh);
        let (x0, x1) = (((p[0] - r) / cell).floor() as i64, ((p[0] + r) / cell).ceil() as i64);
        let (y0, y1) = (((p[1] - r) / cell).floor() as i64, ((p[1] + r) / cell).ceil() as i64);
        if x0 < 1 || y0 < 1 || x1 >= w as i64 || y1 >= h as i64 {
            continue;
        }
        let center = ((p[1] / cell) as usize) * w + (p[0] / cell) as usize;
        let foot: Vec<usize> = (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| y as usize * w + x as usize))
            .filter(|&q| q == center || o.contains(((q % w) as f32 + 0.5) * cell, ((q / w) as f32 + 0.5) * cell))
            .collect();
        if !foot.iter().all(|&q| g.place[q] && !g.taken[q] && !mask.cells[q]) {
            continue;
        }
        let walks = foot.iter().any(|&q| g.walkable[q]);
        let ring_ok = foot.iter().all(|&q| {
            let (x, y) = (q % w, q / w);
            (y - 1..=y + 1).all(|ny| {
                (x - 1..=x + 1).all(|nx| {
                    let n = ny * w + nx;
                    foot.contains(&n) || (!g.taken[n] && !mask.cells[n] && (!walks || g.walkable[n]))
                })
            })
        });
        if !ring_ok {
            continue;
        }
        for &q in &foot {
            mask.cells[q] = true;
        }
        objects.push(o);
    }
    Scree { objects, mask }
}

/// Forest rock patches at the massif's foot (`t`: the zone toward the
/// mountains): blobs of a noise of the blending seed turn dry ground into
/// rock, more of it nearer the face, none near an opening. The forest's
/// passages node joins the ground up again afterwards. Returns the cells
/// turned.
pub fn scree_rock(t: &[f32], rock: &mut CellMask, dry: impl Fn(usize) -> bool, gates: &[[f32; 2]], seed: u64) -> usize {
    let (w, cell) = (rock.width, rock.cell_size_m);
    let noise = NoiseField::new(seed ^ 0x5C2E_E000);
    let mut turned = 0;
    for (c, &tc) in t.iter().enumerate().take(rock.cells.len()) {
        if tc <= 0.0 || rock.cells[c] || !dry(c) {
            continue;
        }
        let (x, y) = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
        // fBm sits mostly in 0.3..0.7: stretch it to 0..1.
        let v = ((noise.fbm(x / SCREE_ROCK_SCALE_M, y / SCREE_ROCK_SCALE_M, 3) - 0.3) / 0.4).clamp(0.0, 1.0);
        if v > 1.0 - SCREE_ROCK_SHARE * tc && !gates.iter().any(|g| (g[0] - x).hypot(g[1] - y) < GATE_CLEAR_M) {
            rock.cells[c] = true;
            turned += 1;
        }
    }
    turned
}

/// Embankments stay open this far around a bridge, meters.
const BRIDGE_OPEN_M: f32 = 3.0;

/// The ground a town's embankments are built on.
pub struct EmbankmentGround<'a> {
    /// Town floor per cell (district ids; 0 = no town ground: water,
    /// outside, the wall).
    pub ids: &'a [u8],
    /// Open water next to the town (a river, a brook; not a bridge).
    pub water: &'a [bool],
    pub roads: Option<&'a CellMask>,
    pub region_walls: Option<&'a CellMask>,
    pub bridges: &'a CellMask,
    pub gates: &'a [[f32; 2]],
}

/// Embankments along the rivers inside a town (towns wall their banks): a
/// quay wall `thickness_m` deep on the town floor along open water, never
/// on a road, open around the bridges and the openings. Written into
/// `wall` (the town's outer wall mask: it is drawn as wall). Where a quay
/// would cut town floor off, it opens next to the cut-off piece until the
/// floor is in as many pieces as before. Returns the cells built.
pub fn embankments(g: &EmbankmentGround, wall: &mut CellMask, thickness_m: f32) -> u32 {
    let (w, h, cell) = (wall.width, wall.height, wall.cell_size_m);
    let n = w * h;
    let floor = |c: usize, wall: &CellMask| g.ids[c] != 0 && !wall.cells[c] && !g.region_walls.is_some_and(|m| m.cells[c]);
    let reach = thickness_m.max(cell);
    let (to_water, _) = distance_field(w, h, cell, |c| g.water[c] && g.ids[c] == 0, reach + cell);
    let (to_bridge, _) = distance_field(w, h, cell, |c| g.bridges.cells[c], BRIDGE_OPEN_M + cell);
    let mut quay: Vec<bool> = (0..n)
        .map(|c| {
            let (x, y) = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
            floor(c, wall)
                && !g.roads.is_some_and(|m| m.cells[c])
                && to_water[c] > 0.0
                && to_water[c] <= reach + 1e-3
                && to_bridge[c] > BRIDGE_OPEN_M
                && !g.gates.iter().any(|q| (q[0] - x).hypot(q[1] - y) < GATE_CLEAR_M)
        })
        .collect();
    let pieces = |quay: &[bool]| {
        let m = CellMask { width: w, height: h, cell_size_m: cell, cells: (0..n).map(|c| floor(c, wall) && !quay[c]).collect() };
        crate::generator::grid::components(&m)
    };
    let (_, before) = pieces(&vec![false; n]);
    for _ in 0..32 {
        let (labels, count) = pieces(&quay);
        if count <= before {
            break;
        }
        let mut size = vec![0usize; count as usize + 1];
        for &l in &labels {
            size[l as usize] += 1;
        }
        let main = (1..=count as usize).max_by_key(|&l| (size[l], std::cmp::Reverse(l))).unwrap_or(1) as u32;
        // Open the quay beside every piece but the main one.
        let mut opened = false;
        for (c, q) in quay.iter_mut().enumerate() {
            if !*q {
                continue;
            }
            let (x, y) = (c % w, c / w);
            let cut_off = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)]
                .into_iter()
                .any(|(nx, ny)| nx < w && ny < h && labels[ny * w + nx] != 0 && labels[ny * w + nx] != main);
            if cut_off {
                *q = false;
                opened = true;
            }
        }
        if !opened {
            break;
        }
    }
    let (_, after) = pieces(&quay);
    if after > before {
        return 0;
    }
    let mut built = 0;
    for (m, &q) in wall.cells.iter_mut().zip(&quay) {
        if q {
            *m = true;
            built += 1;
        }
    }
    built
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

    /// Boulders a region's level counts (forest props / town and swamp props).
    fn scree_of(l: &crate::level::GeneratedLevel) -> u32 {
        l.forest.as_ref().and_then(|f| f.props.as_ref()).map(|p| p.scree).or(l.props.as_ref().map(|p| p.scree)).unwrap_or(0)
    }

    #[test]
    fn scree_lies_at_the_foot_of_the_massif_on_free_ground() {
        let width = 14.0;
        // Boulders per receiving kind: forest, town, swamp.
        const BELOW: [MapKind; 3] = [MapKind::Forest, MapKind::Town, MapKind::Swamp];
        let mut per_kind = [0u32; 3];
        for seed in 1..=4 {
            let mut c = LevelConfig::of_kind(MapKind::Mixed);
            c.mixed.blending.scree_m = width;
            let lvl = crate::generator::generate(&c, seed);
            let m = lvl.mixed.as_ref().unwrap();
            let (w, h, cell) = (m.map.width, m.map.height, m.map.cell_size_m);
            let (to_massif, _) = distance_field(w, h, cell, |c| m.map.ids[c] != 0 && m.regions[m.map.ids[c] as usize - 1].kind == MapKind::Cave, width * 2.0);
            for (r, l) in m.regions.iter().zip(&m.levels) {
                let Some(l) = l.as_ref() else { continue };
                let n = scree_of(l);
                if r.kind == MapKind::Cave {
                    assert_eq!(n, 0, "seed {seed}: scree inside the massif");
                    continue;
                }
                per_kind[BELOW.iter().position(|&k| k == r.kind).expect("a kind below a massif")] += n;
                // The boulders: the last props before the gate markers and
                // the mobs, in this region, near the massif, on nothing else.
                let last = l.objects.iter().rposition(|o| o.category == Category::Prop).map_or(0, |i| i + 1);
                for o in &l.objects[last - n as usize..last] {
                    assert_eq!(o.category, Category::Prop);
                    let p = [o.x_m + o.w_m / 2.0, o.y_m + o.h_m / 2.0];
                    let c = (p[1] / cell) as usize * w + (p[0] / cell) as usize;
                    assert_eq!(m.map.ids[c], r.id, "seed {seed}: a boulder outside its {:?}", r.kind);
                    assert!(to_massif[c] <= width * (1.0 + WIDTH_WAVER) + 2.0 * cell, "seed {seed}: a boulder {:.1} m from the massif", to_massif[c]);
                    for q in &l.objects {
                        if !std::ptr::eq(q, o) && q.category != Category::MobSpawn {
                            assert!(!q.contains(p[0], p[1]), "seed {seed}: a boulder under a {}", q.type_name);
                        }
                    }
                    if let Some(roads) = l.roads.as_ref() {
                        assert!(!roads.mask.cells[c], "seed {seed}: a boulder on a road");
                    }
                    if let Some(f) = l.forest.as_ref() {
                        let g = crate::level::ForestGround::from_code(f.ground().unwrap()[c]);
                        assert!(matches!(g, crate::level::ForestGround::Open | crate::level::ForestGround::Floor), "seed {seed}: a forest boulder on {g:?}");
                    }
                    if let Some(s) = l.swamp.as_ref() {
                        let paths = s.paths.as_ref().unwrap();
                        assert!(!paths.mud.cells[c] && !paths.boardwalk.cells[c], "seed {seed}: a boulder on a swamp path");
                    }
                }
            }
        }
        for (kind, n) in BELOW.iter().zip(per_kind) {
            assert!(n > 0, "no scree in any {kind:?} on four maps: {per_kind:?}");
        }
    }

    #[test]
    fn scree_off_adds_no_boulders_and_no_rock() {
        for seed in 1..=2 {
            let mut c = LevelConfig::of_kind(MapKind::Mixed);
            c.mixed.blending.scree_m = 0.0;
            let off = crate::generator::generate(&c, seed);
            c.mixed.blending.scree_m = 14.0;
            let on = crate::generator::generate(&c, seed);
            let (mo, mn) = (off.mixed.as_ref().unwrap(), on.mixed.as_ref().unwrap());
            assert!(mo.levels.iter().flatten().all(|l| scree_of(l) == 0), "seed {seed}");
            // The forest's rock never shrinks with scree on.
            for (a, b) in mo.levels.iter().zip(&mn.levels) {
                if let (Some(a), Some(b)) = (a.as_ref().and_then(|l| l.forest.as_ref()), b.as_ref().and_then(|l| l.forest.as_ref())) {
                    let (ra, rb) = (a.terrain.as_ref().unwrap().rock.count_true(), b.terrain.as_ref().unwrap().rock.count_true());
                    assert!(rb >= ra, "seed {seed}: forest rock {ra} → {rb}");
                }
            }
        }
    }

    #[test]
    fn embankments_line_the_water_open_at_the_bridge_and_never_cut_the_town() {
        // A 40 × 30 m town, a river in x 18..=21 crossed by a bridge in rows
        // 14..=16 that carries the road (row 15). A district wall in row 10
        // (x 0..=16) leaves the top-left block one way out: the cell x 17
        // beside the river.
        let (w, h) = (40usize, 30usize);
        let at = |x: usize, y: usize| y * w + x;
        let mut ids = vec![1u8; w * h];
        let mut water = vec![false; w * h];
        let mut bridges = CellMask::new(w, h, 1.0);
        let mut roads = CellMask::new(w, h, 1.0);
        let mut rw = CellMask::new(w, h, 1.0);
        for y in 0..h {
            for x in 18..=21 {
                if (14..=16).contains(&y) {
                    bridges.cells[at(x, y)] = true;
                } else {
                    ids[at(x, y)] = 0;
                    water[at(x, y)] = true;
                }
            }
        }
        for x in 0..w {
            roads.cells[at(x, 15)] = true;
        }
        for x in 0..=16 {
            rw.cells[at(x, 10)] = true;
        }
        let g = EmbankmentGround { ids: &ids, water: &water, roads: Some(&roads), region_walls: Some(&rw), bridges: &bridges, gates: &[] };
        let mut wall = CellMask::new(w, h, 1.0);
        let built = embankments(&g, &mut wall, 1.0);
        assert!(built > 20, "{built} quay cells");
        for c in (0..w * h).filter(|&c| wall.cells[c]) {
            let (x, y) = (c % w, c / w);
            assert!(x == 17 || x == 22, "quay at ({x}, {y}) is not on the bank");
            assert!(!roads.cells[c], "quay on the road at ({x}, {y})");
            assert!(!(13..=17).contains(&y), "quay at ({x}, {y}) closes the bridge head");
        }
        // The neck beside the river stays open: the town floor is one piece.
        assert!(!wall.cells[at(17, 10)], "the only way out of the top-left block was walled");
        let floor = CellMask { width: w, height: h, cell_size_m: 1.0, cells: (0..w * h).map(|c| ids[c] != 0 && !rw.cells[c] && !wall.cells[c]).collect() };
        assert_eq!(crate::generator::grid::components(&floor).1, 1);
    }

    #[test]
    fn town_drains_the_swamp_and_walls_its_rivers() {
        for seed in 1..=2 {
            let mut c = LevelConfig::of_kind(MapKind::Mixed);
            c.mixed.blending.town_swamp_m = 0.0;
            c.mixed.blending.embankment_m = 0.0;
            let off = crate::generator::generate(&c, seed);
            let on = crate::generator::generate(&LevelConfig::of_kind(MapKind::Mixed), seed);
            let (mo, mn) = (off.mixed.as_ref().unwrap(), on.mixed.as_ref().unwrap());
            let depth = |l: &crate::level::GeneratedLevel| l.swamp.as_ref().unwrap().depth.as_ref().unwrap().clone();
            let deep = |l: &crate::level::GeneratedLevel, c: usize| depth(l).deep.cells[c];
            let wade = |l: &crate::level::GeneratedLevel, c: usize| {
                let d = depth(l);
                d.shallow.cells[c] || d.mud.cells[c]
            };
            let (a, b) = (count_by(mo, MapKind::Swamp, MapKind::Town, 6.0, deep), count_by(mn, MapKind::Swamp, MapKind::Town, 6.0, deep));
            assert!(b * 2 < a, "seed {seed}: deep water by the town {a} → {b}");
            let (a, b) = (count_by(mo, MapKind::Swamp, MapKind::Town, 6.0, wade), count_by(mn, MapKind::Swamp, MapKind::Town, 6.0, wade));
            assert!(b > a, "seed {seed}: shallow water and mud by the town {a} → {b}");
            let quay = |m: &MixedLayout| m.levels.iter().flatten().filter_map(|l| l.walls.as_ref()).map(|w| w.stats.embankment_cells).sum::<u32>();
            assert_eq!(quay(mo), 0, "seed {seed}");
            assert!(quay(mn) > 0, "seed {seed}: no embankments along the town's river");
        }
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
