//! Forest node 9, props (step F8 in docs/forest-plan.md): uninteractive
//! props on the forest floor. The generator only decides **shape and
//! place** — what a prop is (a log, a rock, a stump, a crate…) is up to the
//! level designer. Every prop is a plain `prop` (`Category::Prop`, the props
//! color) and counts as **blocked** on the map.
//!
//! Placements (each with its own shape: long — a rotated bar, round, box):
//!
//! - **Groups** — `groups_per_ha` per hectare of forest floor (by the area
//!   character: old growth / dense woods most, camp grounds none): a patch
//!   of `group_min..group_max` props within `group_radius_m`; long props
//!   in a group lie along one direction (shared by the whole map with
//!   `same_direction`, ± `align_spread_deg`). The standing trunks under a
//!   group are cleared (`clear_trees_pct`) so it has room. spec.json lists
//!   every group with its props, so the designer can turn a group into one
//!   thing.
//! - **Singles** — `singles_per_ha` per hectare of forest floor (old growth
//!   ×2), any direction.
//! - **Near rock** — `near_rock_per_ha` per hectare of the map, within
//!   `rock_reach_m` of rock (closer = likelier and bigger), 15% anywhere.
//! - **Managed** — `managed_per_ha` in logged areas, a tenth in other
//!   managed areas.
//!
//! Props lie on open ground / forest floor only, off trails
//! (`trail_clear_m`) and gates, never on a trunk or another prop, and keep
//! a one-cell ring of walkable ground — the walkable ground stays one
//! piece.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::forest::distance_field;
use crate::generator::props::PROP_NAME;
use crate::level::{
    AreaLayout, Category, ClearingLayout, ForestCharacter, ForestGround, ForestPropGroup, ForestPropLayout, ForestStyle, PassageLayout,
    PlacedObject, TerrainLayout,
};
use crate::model::{LevelConfig, PropShape};

pub struct PropBuild {
    pub layout: ForestPropLayout,
    /// The props; `layout.groups[..].objects` index here (the chain shifts
    /// them to `GeneratedLevel::objects` indexes).
    pub objects: Vec<PlacedObject>,
    /// Standing trunks cleared under the groups, per area (index = area id − 1).
    pub cleared: Vec<u32>,
}

/// No prop this close to a gate point, meters.
const GATE_CLEAR_M: f32 = 6.0;
/// Tries per prop.
const TRIES: usize = 12;
/// Share of the near-rock props that go anywhere instead.
const ANYWHERE_SHARE: f32 = 0.15;
/// Managed areas that are not logged get this × the logged amount.
const OTHER_MANAGED: f32 = 0.1;
/// Trunks within this × the group radius are cleared.
const CLEAR_REACH: f32 = 1.1;

/// (groups ×, singles ×) per area character.
fn character_mix(c: ForestCharacter) -> (f32, f32) {
    match c {
        ForestCharacter::Meadow => (0.1, 0.2),
        ForestCharacter::OpenWoods => (0.8, 1.0),
        ForestCharacter::DenseWoods => (1.5, 1.2),
        ForestCharacter::OldGrowth => (1.6, 2.0),
        ForestCharacter::Logged => (0.2, 0.5),
        ForestCharacter::Plantation => (0.5, 0.4),
        ForestCharacter::CampGrounds => (0.0, 0.2),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    areas: &AreaLayout,
    terrain: &TerrainLayout,
    passages: &PassageLayout,
    clearings: &ClearingLayout,
    trunks: &mut Vec<PlacedObject>,
) -> PropBuild {
    let s = &config.forest.props;
    let color = config.props.color;
    let (w, h, cell) = (passages.width, passages.height, passages.cell_size_m);
    let n = w * h;
    let ids = &areas.ids;
    let ground: Vec<ForestGround> = clearings.ground.iter().map(|&g| ForestGround::from_code(g)).collect();
    let xy_m = |c: usize| [((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell];
    let floor_of = |id: u16| -> Vec<usize> { (0..n).filter(|&c| ids[c] == id && ground[c] == ForestGround::Floor).collect() };
    let ha = |cells: usize| cells as f32 * cell * cell / 10_000.0;
    let direction_deg = rng.random_range(0.0..180.0f32);
    let mut layout = ForestPropLayout { direction_deg, groups: Vec::new(), singles: 0, near_rock: 0, managed: 0 };
    let mut objects: Vec<PlacedObject> = Vec::new();

    // Group spots first, then clear the trunks under them.
    let mut spots: Vec<([f32; 2], u32, f32)> = Vec::new();
    for area in &areas.areas {
        let floor = floor_of(area.id);
        if floor.is_empty() {
            continue;
        }
        let want = poisson_round(rng, ha(floor.len()) * s.groups_per_ha.max(0.0) * character_mix(area.character).0);
        for _ in 0..want {
            let center = xy_m(floor[rng.random_range(0..floor.len())]);
            let count = rng.random_range(s.group_min.min(s.group_max)..=s.group_max.max(s.group_min));
            let dir = if s.same_direction { direction_deg } else { rng.random_range(0.0..180.0f32) };
            spots.push((center, count, dir));
        }
    }
    let radius = s.group_radius_m.max(1.0);
    let clear = (s.clear_trees_pct / 100.0).clamp(0.0, 1.0);
    let mut cleared = vec![0u32; areas.areas.len()];
    let rolls: Vec<f32> = trunks.iter().map(|_| rng.random()).collect();
    let mut k = 0;
    trunks.retain(|o| {
        let (cx, cy) = o.center();
        let roll = rolls[k];
        k += 1;
        let under = spots.iter().any(|&(p, _, _)| (p[0] - cx).hypot(p[1] - cy) <= radius * CLEAR_REACH);
        if under && roll < clear {
            let c = ((cy / cell) as usize).min(h - 1) * w + ((cx / cell) as usize).min(w - 1);
            cleared[ids[c] as usize - 1] += 1;
            false
        } else {
            true
        }
    });

    // What the standing trunks take.
    let mut taken = vec![false; n];
    for o in trunks.iter().filter(|o| o.category == Category::Structure) {
        let r = o.w_m.max(o.h_m);
        let (cx, cy) = o.center();
        let (x0, x1) = (((cx - r) / cell).floor().max(0.0) as usize, (((cx + r) / cell).ceil() as usize).min(w));
        let (y0, y1) = (((cy - r) / cell).floor().max(0.0) as usize, (((cy + r) / cell).ceil() as usize).min(h));
        for y in y0..y1 {
            for x in x0..x1 {
                let c = y * w + x;
                let p = xy_m(c);
                if o.contains(p[0], p[1]) || (x, y) == ((cx / cell) as usize, (cy / cell) as usize) {
                    taken[c] = true;
                }
            }
        }
    }
    let (to_trail, _) = distance_field(w, h, cell, |c| matches!(ground[c], ForestGround::Trail | ForestGround::Bridge), s.trail_clear_m.max(0.0) + cell * 2.0);
    let free = |c: usize| matches!(ground[c], ForestGround::Open | ForestGround::Floor) && to_trail[c] > s.trail_clear_m.max(0.0);
    let near_gate = |p: [f32; 2]| passages.gates.iter().any(|g| (g.point_m[0] - p[0]).hypot(g.point_m[1] - p[1]) < GATE_CLEAR_M);

    // Puts a prop down if it fits: free ground, a one-cell gap to every other
    // object, walkable ground all around. Returns its index.
    let mut put = |rng: &mut ChaCha8Rng, p: [f32; 2], shape: PropShape, size: f32, angle_deg: f32, taken: &mut Vec<bool>| -> Option<usize> {
        if near_gate(p) {
            return None;
        }
        let (ow, oh) = match shape {
            PropShape::Long => (size, roll(rng, s.long_width_min_m, s.long_width_max_m).max(cell * 0.5)),
            PropShape::Round => (size, size),
            PropShape::Box => (size, size * rng.random_range(0.6..=1.0f32)),
        };
        let angle = if shape == PropShape::Round { 0.0 } else { angle_deg };
        let reach = ow.max(oh) / 2.0 + cell;
        let (cx, cy) = ((p[0] / cell) as i64, (p[1] / cell) as i64);
        let rr = (reach / cell).ceil() as i64;
        let (sa, ca) = angle.to_radians().sin_cos();
        let mut foot = Vec::new();
        for y in cy - rr..=cy + rr {
            for x in cx - rr..=cx + rr {
                if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                    return None;
                }
                let c = y as usize * w + x as usize;
                let q = xy_m(c);
                let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
                let inside = match shape {
                    PropShape::Round => dx.hypot(dy) <= (ow / 2.0).max(cell * 0.5),
                    _ => {
                        let (u, v) = (dx * ca + dy * sa, -dx * sa + dy * ca);
                        u.abs() <= (ow / 2.0).max(cell * 0.5) && v.abs() <= (oh / 2.0).max(cell * 0.5)
                    }
                };
                if inside {
                    foot.push(c);
                }
            }
        }
        if foot.is_empty() {
            return None;
        }
        for &c in &foot {
            if !free(c) || taken[c] {
                return None;
            }
            let (x, y) = ((c % w) as i64, (c / w) as i64);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        return None;
                    }
                    let q = ny as usize * w + nx as usize;
                    if !foot.contains(&q) && (taken[q] || !ground[q].walkable()) {
                        return None;
                    }
                }
            }
        }
        for &c in &foot {
            taken[c] = true;
        }
        objects.push(PlacedObject {
            category: Category::Prop,
            type_name: PROP_NAME.to_owned(),
            color,
            x_m: p[0] - ow / 2.0,
            y_m: p[1] - oh / 2.0,
            w_m: ow,
            h_m: oh,
            angle_deg: angle,
            parts: Vec::new(),
            round: shape == PropShape::Round,
        });
        Some(objects.len() - 1)
    };
    let in_map = |p: [f32; 2]| p[0] >= 0.0 && p[1] >= 0.0 && p[0] < w as f32 * cell && p[1] < h as f32 * cell;

    // 1. Groups.
    for &(center, count, dir) in &spots {
        let mut members = Vec::new();
        for _ in 0..count {
            for _ in 0..TRIES {
                let (r, a) = (radius * rng.random::<f32>().sqrt(), rng.random_range(0.0..std::f32::consts::TAU));
                let p = [center[0] + r * a.cos(), center[1] + r * a.sin()];
                let angle = dir + rng.random_range(-1.0..=1.0f32) * s.align_spread_deg.max(0.0);
                let size = roll(rng, s.group_size_min_m, s.group_size_max_m);
                if in_map(p)
                    && let Some(i) = put(rng, p, s.group_shape, size, angle, &mut taken)
                {
                    members.push(i);
                    break;
                }
            }
        }
        if !members.is_empty() {
            layout.groups.push(ForestPropGroup { id: layout.groups.len() as u32 + 1, center_m: center, radius_m: radius, direction_deg: dir, objects: members });
        }
    }

    // 2. Singles.
    for area in &areas.areas {
        let floor = floor_of(area.id);
        if floor.is_empty() {
            continue;
        }
        let want = poisson_round(rng, ha(floor.len()) * s.singles_per_ha.max(0.0) * character_mix(area.character).1);
        for _ in 0..want {
            for _ in 0..TRIES * 3 {
                let p = xy_m(floor[rng.random_range(0..floor.len())]);
                let angle = rng.random_range(0.0..180.0f32);
                let size = roll(rng, s.single_size_min_m, s.single_size_max_m);
                if put(rng, p, s.single_shape, size, angle, &mut taken).is_some() {
                    layout.singles += 1;
                    break;
                }
            }
        }
    }

    // 3. Near rock (closer = likelier and bigger), a few anywhere.
    let reach = s.rock_reach_m.max(cell);
    let (to_rock, _) = distance_field(w, h, cell, |c| terrain.rock.cells[c], reach);
    let near_rock: Vec<usize> = (0..n).filter(|&c| free(c) && to_rock[c] <= reach).collect();
    let anywhere: Vec<usize> = (0..n).filter(|&c| free(c)).collect();
    let want = poisson_round(rng, ha(n) * s.near_rock_per_ha.max(0.0));
    let (rmin, rmax) = (s.rock_size_min_m.min(s.rock_size_max_m).max(cell), s.rock_size_max_m.max(s.rock_size_min_m).max(cell));
    for _ in 0..want {
        let far = near_rock.is_empty() || rng.random::<f32>() < ANYWHERE_SHARE;
        let pool = if far { &anywhere } else { &near_rock };
        if pool.is_empty() {
            break;
        }
        for _ in 0..TRIES {
            let c = pool[rng.random_range(0..pool.len())];
            let close = if far { 0.3 } else { 1.0 - (to_rock[c] / reach).clamp(0.0, 1.0) };
            let keep: f32 = rng.random();
            let size = rmin + (rmax - rmin) * (close * 0.7 + 0.3 * rng.random::<f32>());
            let angle = rng.random_range(0.0..180.0f32);
            if keep > close.max(0.15) {
                continue;
            }
            let p0 = xy_m(c);
            let p = [p0[0] + rng.random_range(-0.5..0.5f32) * cell, p0[1] + rng.random_range(-0.5..0.5f32) * cell];
            if put(rng, p, s.rock_shape, size, angle, &mut taken).is_some() {
                layout.near_rock += 1;
                break;
            }
        }
    }

    // 4. Managed areas: logged ones most.
    for area in areas.areas.iter().filter(|a| a.style == ForestStyle::Managed) {
        let mult = if area.character == ForestCharacter::Logged { 1.0 } else { OTHER_MANAGED };
        let cells: Vec<usize> = (0..n).filter(|&c| ids[c] == area.id && free(c)).collect();
        if cells.is_empty() {
            continue;
        }
        let want = poisson_round(rng, ha(cells.len()) * s.managed_per_ha.max(0.0) * mult);
        for _ in 0..want {
            for _ in 0..TRIES {
                let p0 = xy_m(cells[rng.random_range(0..cells.len())]);
                let p = [p0[0] + rng.random_range(-0.5..0.5f32) * cell, p0[1] + rng.random_range(-0.5..0.5f32) * cell];
                let size = roll(rng, s.managed_size_min_m, s.managed_size_max_m);
                let angle = rng.random_range(0.0..180.0f32);
                if put(rng, p, s.managed_shape, size, angle, &mut taken).is_some() {
                    layout.managed += 1;
                    break;
                }
            }
        }
    }
    PropBuild { layout, objects, cleared }
}

/// Uniform in `a..=b` (either order).
fn roll(rng: &mut ChaCha8Rng, a: f32, b: f32) -> f32 {
    let (lo, hi) = (a.min(b), a.max(b));
    if hi - lo < 1e-6 { lo } else { rng.random_range(lo..=hi) }
}

/// `x` rounded at random by its fraction (4.3 → 4 or 5, 5 with 30%).
fn poisson_round(rng: &mut ChaCha8Rng, x: f32) -> usize {
    let x = x.max(0.0);
    x.floor() as usize + (rng.random::<f32>() < x.fract()) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::generator::forest::{areas, clearings, passages, terrain, thickets, trees, water};
    use crate::generator::grid;
    use crate::level::CellMask;
    use crate::model::MapKind;

    fn run(seed: u64, edit: impl Fn(&mut LevelConfig)) -> (ClearingLayout, Vec<PlacedObject>, PropBuild, usize) {
        let mut c = LevelConfig { kind: MapKind::Forest, ..LevelConfig::default() };
        c.map.width_m = 160.0;
        c.map.height_m = 120.0;
        c.cell_size_m = 0.5;
        edit(&mut c);
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let a = areas::build(&c, &mut rng).layout;
        let t = terrain::build(&c, &mut rng, &a);
        let wa = water::build(&c, &mut rng, &t).layout;
        let th = thickets::build(&c, &mut rng, &a, &t, &wa);
        let p = passages::build(&c, &mut rng, &a, &th).layout;
        let cl = clearings::build(&c, &mut rng, &a, &p);
        let mut tr = trees::build(&c, &mut rng, &a, &th, &p, &cl);
        let pr = build(&c, &mut rng, &a, &t, &p, &cl, &mut tr.objects);
        (cl, tr.objects, pr, p.width)
    }

    #[test]
    fn props_block_and_keep_the_ground_one_piece() {
        for seed in 0..3 {
            let (cl, trunks, pr, w) = run(seed, |c| {
                c.forest.props.groups_per_ha = 4.0;
                c.forest.props.near_rock_per_ha = 15.0;
                c.forest.terrain.rock_pct = 6.0;
            });
            let h = cl.ground.len() / w;
            assert!(!pr.layout.groups.is_empty() && pr.layout.near_rock > 0, "seed {seed}: {:?}", pr.layout);
            let mut cells = vec![false; w * h];
            for (c, x) in cells.iter_mut().enumerate() {
                let p = [((c % w) as f32 + 0.5) * 0.5, ((c / w) as f32 + 0.5) * 0.5];
                let blocked = trunks.iter().chain(&pr.objects).any(|o| o.contains(p[0], p[1]));
                *x = ForestGround::from_code(cl.ground[c]).walkable() && !blocked;
            }
            let (_, pieces) = grid::components(&CellMask { width: w, height: h, cell_size_m: 0.5, cells });
            assert!(pieces <= 1, "seed {seed}: {pieces} pieces");
        }
    }

    #[test]
    fn plain_props_with_the_chosen_shapes() {
        let (_, _, pr, _) = run(4, |c| {
            c.forest.props.groups_per_ha = 4.0;
            c.forest.props.group_shape = PropShape::Box;
            c.forest.props.single_shape = PropShape::Round;
        });
        assert!(pr.objects.iter().all(|o| o.category == Category::Prop && o.type_name == PROP_NAME));
        for g in &pr.layout.groups {
            for &i in &g.objects {
                assert!(!pr.objects[i].round, "group props are boxes");
            }
        }
    }

    #[test]
    fn long_props_in_a_group_share_its_direction() {
        let (_, _, pr, _) = run(5, |c| {
            c.forest.props.groups_per_ha = 6.0;
            c.forest.props.align_spread_deg = 10.0;
        });
        assert!(!pr.layout.groups.is_empty());
        for g in &pr.layout.groups {
            for &i in &g.objects {
                assert!((pr.objects[i].angle_deg - g.direction_deg).abs() <= 10.0 + 1e-3);
            }
        }
    }

    #[test]
    fn same_seed_same_props() {
        assert_eq!(run(6, |_| {}).2.objects, run(6, |_| {}).2.objects);
    }
}
