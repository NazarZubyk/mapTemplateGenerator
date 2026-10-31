//! Forest node 8, trees (step F7 in docs/forest-plan.md): tree trunks as
//! round solid objects — on a playground map a tree is what the player
//! bumps into, so only the trunk is placed (the crown is up to the art).
//!
//! Trunks grow on the walkable ground (open ground, forest floor), never on
//! trails, water, rock or in thickets (those are already blocked):
//!
//! - **How many** — about one tree per (`spacing_m` / √(`density_pct` /
//!   100))² of forest floor, scaled by the area character (dense woods
//!   closer, old growth and logged farther) and by the thickets node's
//!   density (denser ground, more trees).
//! - **Where** — a grove noise (`grove_size_m`) gathers the trees into
//!   groves with open gaps between (`groves_pct`); open ground and glades
//!   only get lone trees (`lone_trees_pct`).
//! - **How big** — `trunk_min_m..trunk_max_m`, most of them thin; old
//!   growth ×1.8, dense woods ×0.7. Plantations: about one size (± 10%,
//!   planted at one time), placed at random like the rest — or, opt-in,
//!   in rows (`plantation_rows`) on a grid turned to a random angle.
//! - **Forest floor: plain random placement, two passes.** First about
//!   40% of the trees at uniformly random spots; then the rest, again at
//!   random spots (more likely in groves and on denser ground), until the
//!   target count. The only rule: no other trunk inside a tree's own
//!   radius (two trunks stand at least their two radii apart). No tree is
//!   placed relative to another, and the ground is never packed full (a
//!   target count, not "as many as fit"), so no lines or grids appear.
//! - **Irregularity** (`irregularity_pct`) — how much the trees' own radii
//!   differ (± 80% at 100%); 0% = all the same. Plantation rows wobble and
//!   miss a few trees.
//! - **Narrow strips** — a tree's chance falls where little plantable
//!   ground lies around it, so a thin strip gets a few scattered trees.
//! - **Room** — no trunk within `trail_clear_m` of a trail, 6 m of a gate;
//!   every trunk keeps a one-cell ring of free walkable ground, so the
//!   walkable ground stays one piece and no narrow passage is plugged.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::forest::distance_field;
use crate::generator::noise::NoiseField;
use crate::level::{
    AreaLayout, Category, ClearingLayout, ForestCharacter, ForestGround, ForestTreeLayout, PassageLayout, PlacedObject, ThicketLayout,
};
use crate::model::LevelConfig;

pub struct TreeBuild {
    pub layout: ForestTreeLayout,
    pub objects: Vec<PlacedObject>,
}

pub const TRUNK_NAME: &str = "tree trunk";
/// No trunk this close to a gate point, meters.
const GATE_CLEAR_M: f32 = 6.0;
/// Darts thrown per possible tree.
const DARTS: f32 = 14.0;
/// Lone trees: chance per dart at 100%, and their spacing × the usual.
const LONE_CHANCE: f32 = 0.06;
const LONE_SPACING: f32 = 2.5;
/// Forest floor: the share of trees in the first (uniform) pass, the
/// tries per tree, and a tree's own radius (× the spacing) with its random
/// swing at 100% irregularity (± share).
const FIRST_SHARE: f32 = 0.4;
const TRIES_PER_TREE: usize = 30;
const RADIUS_SHARE: f32 = 0.3;
const RADIUS_SWING: f32 = 0.8;
/// Plantable share around a spot (within one spacing) below which no tree
/// grows, and above which it grows freely: narrow strips stay sparse.
const STRIP_LO: f32 = 0.3;
const STRIP_HI: f32 = 0.75;
/// Plantation rows at 100% irregularity: wobble (× the row step) and the
/// share of missing trees.
const ROW_WOBBLE: f32 = 0.2;
const ROW_MISSING: f32 = 0.15;
/// Thickets-node density where ordinary woods start and thicket begins.
const D_OPEN: f32 = 0.36;
const D_THICKET: f32 = 0.62;

/// (spacing ×, trunk size ×, chance ×) per area character.
fn character_mix(c: ForestCharacter) -> (f32, f32, f32) {
    match c {
        ForestCharacter::Meadow => (1.6, 1.1, 0.25),
        ForestCharacter::OpenWoods => (1.3, 1.0, 0.85),
        ForestCharacter::DenseWoods => (0.75, 0.7, 1.0),
        ForestCharacter::OldGrowth => (1.7, 1.8, 0.95),
        ForestCharacter::Logged => (2.0, 0.9, 0.4),
        ForestCharacter::Plantation => (1.0, 1.0, 1.0),
        ForestCharacter::CampGrounds => (1.8, 1.0, 0.35),
    }
}

pub fn build(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    areas: &AreaLayout,
    thickets: &ThicketLayout,
    passages: &PassageLayout,
    clearings: &ClearingLayout,
) -> TreeBuild {
    let s = &config.forest.trees;
    let (w, h, cell) = (passages.width, passages.height, passages.cell_size_m);
    let n = w * h;
    let ids = &areas.ids;
    let ground: Vec<ForestGround> = clearings.ground.iter().map(|&g| ForestGround::from_code(g)).collect();
    let grove_noise = NoiseField::new(rng.random::<u64>());
    let mut per_area = vec![0u32; areas.areas.len()];
    let mut objects = Vec::new();
    let mut in_rows = 0u32;
    let amount = (s.density_pct / 100.0).max(0.0);
    if amount <= 0.0 || n == 0 {
        return TreeBuild { layout: ForestTreeLayout { per_area, total: 0, in_rows: 0, cover_pct: 0.0 }, objects };
    }
    let spacing = s.spacing_m.max(cell * 2.0) / amount.sqrt();
    let (tmin, tmax) = (s.trunk_min_m.min(s.trunk_max_m).max(cell * 0.5), s.trunk_max_m.max(s.trunk_min_m).max(cell * 0.5));
    let groves = (s.groves_pct / 100.0).clamp(0.0, 1.0);
    let lone = (s.lone_trees_pct / 100.0).max(0.0);
    let irr = (s.irregularity_pct / 100.0).clamp(0.0, 1.0);
    let xy_m = |c: usize| [((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell];

    let (to_trail, _) = distance_field(w, h, cell, |c| matches!(ground[c], ForestGround::Trail | ForestGround::Bridge), s.trail_clear_m.max(0.0) + cell * 2.0);
    let plantable = |c: usize| matches!(ground[c], ForestGround::Open | ForestGround::Floor) && to_trail[c] > s.trail_clear_m.max(0.0);
    let near_gate = |p: [f32; 2]| passages.gates.iter().any(|g| (g.point_m[0] - p[0]).hypot(g.point_m[1] - p[1]) < GATE_CLEAR_M);
    let glade_r: Vec<(f32, [f32; 2])> = clearings.glades.iter().map(|g| ((g.area_m2 / std::f32::consts::PI).sqrt(), g.center_m)).collect();
    let in_glade = |p: [f32; 2]| glade_r.iter().any(|&(r, c)| (c[0] - p[0]).hypot(c[1] - p[1]) < r);

    // How much plantable ground lies around each cell (integral image), so
    // narrow strips get few trees instead of a packed line.
    let mut sum = vec![0u32; (w + 1) * (h + 1)];
    for y in 0..h {
        for x in 0..w {
            let v = plantable(y * w + x) as u32;
            sum[(y + 1) * (w + 1) + x + 1] = v + sum[y * (w + 1) + x + 1] + sum[(y + 1) * (w + 1) + x] - sum[y * (w + 1) + x];
        }
    }
    let room_around = |c: usize, r_m: f32| {
        let r = (r_m / cell).ceil().max(1.0) as usize;
        let (x, y) = (c % w, c / w);
        let (x0, y0, x1, y1) = (x.saturating_sub(r), y.saturating_sub(r), (x + r + 1).min(w), (y + r + 1).min(h));
        let got = sum[y1 * (w + 1) + x1] + sum[y0 * (w + 1) + x0] - sum[y0 * (w + 1) + x1] - sum[y1 * (w + 1) + x0];
        // Off-map counts as no room.
        got as f32 / ((2 * r + 1) * (2 * r + 1)) as f32
    };

    // Trunk centers with their own room (radius), in a coarse grid of
    // buckets for the distance check.
    let bucket = (spacing * 2.0).max(cell * 4.0);
    let (bw, bh) = (((w as f32 * cell) / bucket).ceil() as usize + 1, ((h as f32 * cell) / bucket).ceil() as usize + 1);
    let mut buckets: Vec<Vec<[f32; 3]>> = vec![Vec::new(); bw * bh];
    let mut max_room = 0.0f32;
    let mut solid = vec![false; n];

    // Tries to put a trunk of diameter `d` at `p` that wants `room` (a
    // radius) around it: two trunks stand at least their two rooms apart,
    // so every tree has its own distance to its neighbors. Returns whether
    // it stood.
    let mut place = |p: [f32; 2], d: f32, room: f32, solid: &mut Vec<bool>, buckets: &mut Vec<Vec<[f32; 3]>>| -> bool {
        let (bx, by) = ((p[0] / bucket) as usize, (p[1] / bucket) as usize);
        let reach = ((room + max_room) / bucket).ceil() as usize;
        for y in by.saturating_sub(reach)..(by + reach + 1).min(bh) {
            for x in bx.saturating_sub(reach)..(bx + reach + 1).min(bw) {
                if buckets[y * bw + x].iter().any(|q| (q[0] - p[0]).hypot(q[1] - p[1]) < room + q[2]) {
                    return false;
                }
            }
        }
        // The trunk's cells, then its one-cell ring.
        let r = d / 2.0;
        let (cx, cy) = ((p[0] / cell) as i64, (p[1] / cell) as i64);
        let reach = (r / cell).ceil() as i64 + 1;
        let mut disc = Vec::new();
        for y in cy - reach..=cy + reach {
            for x in cx - reach..=cx + reach {
                if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                    return false;
                }
                let c = y as usize * w + x as usize;
                let q = xy_m(c);
                if (q[0] - p[0]).hypot(q[1] - p[1]) <= r || (x, y) == (cx, cy) {
                    disc.push(c);
                }
            }
        }
        for &c in &disc {
            if !plantable(c) || solid[c] {
                return false;
            }
            let (x, y) = ((c % w) as i64, (c / w) as i64);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        return false;
                    }
                    let q = ny as usize * w + nx as usize;
                    if !disc.contains(&q) && (solid[q] || !ground[q].walkable()) {
                        return false;
                    }
                }
            }
        }
        for &c in &disc {
            solid[c] = true;
        }
        buckets[by.min(bh - 1) * bw + bx.min(bw - 1)].push([p[0], p[1], room]);
        max_room = max_room.max(room);
        objects.push(PlacedObject {
            category: Category::Structure,
            type_name: TRUNK_NAME.to_owned(),
            color: s.color,
            x_m: p[0] - r,
            y_m: p[1] - r,
            w_m: d,
            h_m: d,
            angle_deg: 0.0,
            parts: Vec::new(),
            round: true,
        });
        true
    };
    let grove_at = |p: [f32; 2]| {
        let g = grove_noise.fbm(p[0] / s.grove_size_m.max(2.0), p[1] / s.grove_size_m.max(2.0), 3);
        (1.0 - groves) + groves * smooth(0.40, 0.58, g) * 1.6
    };
    let size = |rng: &mut ChaCha8Rng, mult: f32| (tmin + (tmax - tmin) * rng.random::<f32>().powf(1.5)) * mult;

    for (ai, area) in areas.areas.iter().enumerate() {
        let (sp_mult, size_mult, chance) = character_mix(area.character);
        let cells: Vec<usize> = (0..n).filter(|&c| ids[c] == area.id && plantable(c)).collect();
        if cells.is_empty() {
            continue;
        }
        // Plantations: rows on a turned grid.
        if area.character == ForestCharacter::Plantation && s.plantation_rows {
            let angle: f32 = rng.random_range(0.0..std::f32::consts::PI);
            let step = spacing * sp_mult;
            let d = size(rng, size_mult);
            let (ca, sa) = (angle.cos(), angle.sin());
            let ctr = area.center_m;
            for &c in &cells {
                let p = xy_m(c);
                // Grid coordinates of this cell; plant where it is the grid point's cell.
                let (u, v) = (((p[0] - ctr[0]) * ca + (p[1] - ctr[1]) * sa) / step, (-(p[0] - ctr[0]) * sa + (p[1] - ctr[1]) * ca) / step);
                let (gu, gv) = (u.round(), v.round());
                let q = [ctr[0] + (gu * ca - gv * sa) * step, ctr[1] + (gu * sa + gv * ca) * step];
                if ((q[0] / cell) as usize, (q[1] / cell) as usize) != (c % w, c / w) || near_gate(q) || in_glade(q) {
                    continue;
                }
                // Nobody plants perfectly: a wobble, and some trees missing.
                let wobble = [rng.random_range(-1.0..1.0f32) * irr * ROW_WOBBLE * step, rng.random_range(-1.0..1.0f32) * irr * ROW_WOBBLE * step];
                if rng.random::<f32>() < irr * ROW_MISSING {
                    continue;
                }
                let q = [q[0] + wobble[0], q[1] + wobble[1]];
                if ground[c] == ForestGround::Floor && place(q, d, step * (0.9 - irr * ROW_WOBBLE * 2.0) / 2.0, &mut solid, &mut buckets) {
                    per_area[ai] += 1;
                    in_rows += 1;
                }
            }
            continue;
        }
        // Lone trees on open ground and in glades: darts with a wide room.
        let area_m2 = cells.len() as f32 * cell * cell;
        let darts = (area_m2 / (spacing * sp_mult).powi(2) * DARTS) as usize;
        for _ in 0..darts {
            let c = cells[rng.random_range(0..cells.len())];
            let p0 = xy_m(c);
            let jitter = [rng.random_range(-0.5..0.5f32) * cell, rng.random_range(-0.5..0.5f32) * cell];
            let p = [p0[0] + jitter[0], p0[1] + jitter[1]];
            let roll: f32 = rng.random();
            let d = size(rng, size_mult);
            let lone_spot = ground[c] == ForestGround::Open || in_glade(p);
            if near_gate(p) || !lone_spot {
                continue;
            }
            let want = LONE_CHANCE * lone * chance.max(0.3);
            let gap = spacing * sp_mult * LONE_SPACING;
            // Its own room: random like every tree's.
            let room = (gap / 2.0 * (1.0 - 0.55 * irr * rng.random::<f32>())).max(d / 2.0 + cell);
            let thin = room_around(c, spacing * sp_mult);
            if roll < want * smooth(STRIP_LO, STRIP_HI, thin) && place(p, d, room, &mut solid, &mut buckets) {
                per_area[ai] += 1;
            }
        }
        // The forest floor: plain random placement, in two passes.
        //   1. First trees: random spots all over the floor; the only rule
        //      is that no other tree stands inside a tree's own radius.
        //   2. The rest: random spots again (more likely in groves and on
        //      denser ground), same rule, until the target count.
        // No tree is placed relative to another one, so there is no pattern.
        let floor: Vec<usize> = cells.iter().copied().filter(|&c| ground[c] == ForestGround::Floor).collect();
        if floor.is_empty() {
            continue;
        }
        let floor_m2 = floor.len() as f32 * cell * cell;
        let target = (floor_m2 / (spacing * sp_mult).powi(2) * chance) as usize;
        let first = (target as f32 * FIRST_SHARE) as usize;
        let mut planted = 0usize;
        let mut tries = 0usize;
        // A plantation is planted at one time: its trees are about the same
        // size (one size ± 10%), placed at random like the rest.
        let even_age = (area.character == ForestCharacter::Plantation).then(|| size(rng, size_mult));
        while planted < target && tries < target * TRIES_PER_TREE {
            tries += 1;
            let c = floor[rng.random_range(0..floor.len())];
            let p0 = xy_m(c);
            let p = [p0[0] + rng.random_range(-0.5..0.5f32) * cell, p0[1] + rng.random_range(-0.5..0.5f32) * cell];
            let d = size(rng, size_mult);
            let d = even_age.map_or(d, |e| e * (0.9 + 0.2 * (d - tmin * size_mult) / ((tmax - tmin) * size_mult).max(1e-6)));
            let radius_roll: f32 = rng.random();
            let keep_roll: f32 = rng.random();
            let strip_roll: f32 = rng.random();
            if near_gate(p) || in_glade(p) {
                continue;
            }
            // Pass 2 follows the groves and the denser ground; pass 1 does not.
            if planted >= first {
                let dens = ((thickets.density[c] - D_OPEN) / (D_THICKET - D_OPEN)).clamp(0.0, 1.0);
                if keep_roll > grove_at(p) / 1.6 * (0.6 + 0.4 * dens) {
                    continue;
                }
            }
            // Narrow strips stay sparse.
            if strip_roll > smooth(STRIP_LO, STRIP_HI, room_around(c, spacing * sp_mult)) {
                continue;
            }
            // Its own radius: random, wider spread at higher irregularity.
            let base = spacing * sp_mult * RADIUS_SHARE;
            let radius = (base * (1.0 + irr * (radius_roll * 2.0 - 1.0) * RADIUS_SWING)).max(d / 2.0 + cell);
            if place(p, d, radius, &mut solid, &mut buckets) {
                per_area[ai] += 1;
                planted += 1;
            }
        }
    }
    let walkable = ground.iter().filter(|g| g.walkable()).count().max(1);
    let total = objects.len() as u32;
    TreeBuild {
        layout: ForestTreeLayout { per_area, total, in_rows, cover_pct: solid.iter().filter(|&&x| x).count() as f32 / walkable as f32 * 100.0 },
        objects,
    }
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::generator::forest::{areas, clearings, passages, terrain, thickets, water};
    use crate::generator::grid;
    use crate::level::CellMask;
    use crate::model::MapKind;

    fn run(seed: u64, edit: impl Fn(&mut LevelConfig)) -> (ClearingLayout, TreeBuild, usize) {
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
        let tr = build(&c, &mut rng, &a, &th, &p, &cl);
        (cl, tr, p.width)
    }

    /// Cells under the trunks (by the same disc rule as the placement).
    fn trunk_cells(objs: &[PlacedObject], w: usize, h: usize, cell: f32) -> Vec<bool> {
        let mut out = vec![false; w * h];
        for o in objs {
            let (px, py, r) = (o.x_m + o.w_m / 2.0, o.y_m + o.h_m / 2.0, o.w_m / 2.0);
            let (cx, cy) = ((px / cell) as usize, (py / cell) as usize);
            out[cy * w + cx] = true;
            for (c, o) in out.iter_mut().enumerate() {
                let (x, y) = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
                if (x - px).hypot(y - py) <= r {
                    *o = true;
                }
            }
        }
        out
    }

    #[test]
    fn trunks_keep_the_ground_one_piece_and_off_trails() {
        for seed in 0..2 {
            let (cl, tr, w) = run(seed, |_| {});
            assert!(tr.layout.total > 50, "seed {seed}: {}", tr.layout.total);
            let h = cl.ground.len() / w;
            let solid = trunk_cells(&tr.objects, w, h, 0.5);
            let mask = CellMask {
                width: w,
                height: h,
                cell_size_m: 0.5,
                cells: (0..cl.ground.len()).map(|c| ForestGround::from_code(cl.ground[c]).walkable() && !solid[c]).collect(),
            };
            assert!(grid::is_connected(&mask), "seed {seed}");
            for (c, &s) in solid.iter().enumerate() {
                if s {
                    assert!(matches!(ForestGround::from_code(cl.ground[c]), ForestGround::Open | ForestGround::Floor), "seed {seed}: trunk on {:?}", ForestGround::from_code(cl.ground[c]));
                }
            }
            assert!(tr.objects.iter().all(|o| o.round && o.category == Category::Structure));
        }
    }

    #[test]
    fn density_sets_the_amount() {
        let (_, none, _) = run(3, |c| c.forest.trees.density_pct = 0.0);
        assert_eq!(none.layout.total, 0);
        let (_, half, _) = run(3, |c| c.forest.trees.density_pct = 50.0);
        let (_, full, _) = run(3, |_| {});
        assert!(half.layout.total < full.layout.total, "{} vs {}", half.layout.total, full.layout.total);
    }

    #[test]
    fn same_seed_same_trees() {
        assert_eq!(run(4, |_| {}).1.objects, run(4, |_| {}).1.objects);
    }
}
