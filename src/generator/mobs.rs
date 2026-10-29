//! The mobs node (every map kind): spawn points bought from a budget.
//!
//! 1. **Where mobs may stand** — walkable ground: the town minus walls,
//!    houses, props and interactives; roads too when `on_roads` is set; the
//!    floor of a hidden room only if its path is mob-passable (never the
//!    narrow paths themselves). Spawn points are markers, not obstacles —
//!    they never block a road or seal a pocket.
//! 2. **Spending** — packs are bought one by one until the budget cannot
//!    pay for any type, `max_entities` is reached or no spot is left. A pack
//!    rolls `pack_min..=pack_max` mobs; each member is the affordable type
//!    furthest behind its balance share (so the counts follow the sliders)
//!    that may combine with every type already in the pack. Cheaper types
//!    fill the end of the budget, so as much of it as possible is spent.
//! 3. **The minimum** — `min_entities` (or `min_per_100m2` × the walkable
//!    area), capped by `max_entities`. While below it, buying goes on after
//!    the budget is spent (members picked by balance alone; a note says how
//!    far over budget), and when no spot is left the spacing is halved
//!    (down to circles merely not overlapping) instead of stopping.
//! 4. **The spot** — a random free cell at least `spacing_m` from every
//!    other spawn (and never so close the circles overlap); the pack's
//!    circle (members on a sunflower spiral, see `member_offset`) must lie wholly on
//!    spawnable ground, in one place (not half in a hidden room). No spot
//!    for the pack → it shrinks to one mob; no spot even then → spending
//!    stops with a note (or the spacing relaxes, below the minimum).

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::Stage;
use crate::level::{
    Category, CellMask, HiddenRoomLayout, MobLayout, MobPackMember, MobSpawn, PlacedObject, RegionMap, RoadNetwork,
};
use crate::model::{LevelConfig, NodeAmount};

/// Cells drawn per pack while looking for a spot.
const TRIES: usize = 60;

pub struct MobBuild {
    pub layout: MobLayout,
    /// One marker per spawn; `layout.spawns[..].object` indexes here.
    pub objects: Vec<PlacedObject>,
    pub notes: Vec<String>,
}

/// What the mobs stand on: the map after every earlier node.
pub struct Ground<'a> {
    pub regions: &'a RegionMap,
    pub outer_wall: Option<&'a CellMask>,
    pub region_walls: Option<&'a CellMask>,
    pub roads: Option<&'a RoadNetwork>,
    /// Footprints of solid objects (houses, props, interactives).
    pub objects: &'a [&'a CellMask],
    pub hidden_rooms: Option<&'a HiddenRoomLayout>,
}

/// `build_with_progress` without progress reporting.
pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, ground: &Ground) -> Option<MobBuild> {
    build_with_progress(config, rng, ground, &Stage { progress: &|_, _| {}, from: 0.0, to: 1.0 })
}

pub(crate) fn build_with_progress(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    ground: &Ground,
    stage: &Stage,
) -> Option<MobBuild> {
    let map = ground.regions;
    let (w, h) = (map.width, map.height);
    let cell = map.cell_size_m.max(0.05);
    let s = &config.mobs;
    let n_types = s.types.len();

    // 1. Spawnable ground, and which hidden room (if any) each cell is in.
    let in_mask = |m: Option<&CellMask>, i: usize| m.is_some_and(|m| m.cells[i]);
    let road = |i: usize| ground.roads.is_some_and(|r| r.mask.cells[i]);
    let mut room_of: Vec<u32> = vec![0; w * h];
    let mut room_ok: Vec<bool> = vec![false; w * h];
    if let Some(rooms) = ground.hidden_rooms {
        for r in &rooms.rooms {
            let [x, y, rw, rh] = r.rect_m;
            let (x0, y0) = ((x / cell).round() as usize, (y / cell).round() as usize);
            let (x1, y1) = (((x + rw) / cell).round() as usize, ((y + rh) / cell).round() as usize);
            for cy in y0..y1.min(h) {
                for cx in x0..x1.min(w) {
                    room_of[cy * w + cx] = r.id;
                    room_ok[cy * w + cx] = r.mob_passable;
                }
            }
        }
    }
    let hidden = |m: fn(&HiddenRoomLayout) -> &CellMask, i: usize| ground.hidden_rooms.is_some_and(|l| m(l).cells[i]);
    let spawnable: Vec<bool> = (0..w * h)
        .map(|i| {
            if room_of[i] != 0 {
                return room_ok[i];
            }
            map.ids[i] != 0
                && !in_mask(ground.outer_wall, i)
                && !in_mask(ground.region_walls, i)
                && !ground.objects.iter().any(|m| m.cells[i])
                && !hidden(|l| &l.wall_mask, i)
                && !hidden(|l| &l.floor_mask, i)
                && (s.on_roads || !road(i))
        })
        .collect();

    let mut layout = MobLayout {
        spawns: Vec::new(),
        per_type: vec![0; n_types],
        entities: 0,
        min_target: 0,
        spent: 0.0,
        budget: s.budget,
        spacing_used_m: s.spacing_m.max(0.0),
        stop_reason: String::new(),
    };
    let mut objects: Vec<PlacedObject> = Vec::new();
    let mut notes = Vec::new();
    let walkable_m2 = spawnable.iter().filter(|&&v| v).count() as f32 * cell * cell;
    let min_wanted = match s.min_amount {
        NodeAmount::Count => s.min_entities,
        NodeAmount::Density => (s.min_per_100m2.max(0.0) * walkable_m2 / 100.0).round() as u32,
    };
    let min_target = min_wanted.min(s.max_entities);
    layout.min_target = min_target;
    if n_types == 0 || s.max_entities == 0 || (s.budget <= 0.0 && min_target == 0) {
        layout.stop_reason = "nothing to place".to_owned();
        return Some(MobBuild { layout, objects, notes });
    }

    // Balance targets; zero-share types are never bought (all zero → equal).
    let weights: Vec<f32> = s.types.iter().map(|t| t.balance_pct.max(0.0)).collect();
    let weight_sum: f32 = weights.iter().sum();
    let target: Vec<f32> =
        if weight_sum > 0.0 { weights.iter().map(|w| w / weight_sum).collect() } else { vec![1.0 / n_types as f32; n_types] };
    let price = |t: usize| s.types[t].price.max(0.01);
    let pack_lo = s.pack_min.min(s.pack_max).max(1);
    let pack_hi = s.pack_min.max(s.pack_max).max(1);
    let mut spacing = s.spacing_m.max(0.0);
    let cheapest = (0..n_types).filter(|&t| target[t] > 0.0).map(price).fold(f32::MAX, f32::min);

    let mut candidates: Vec<usize> = (0..w * h).filter(|&i| spawnable[i]).collect();
    let mut remaining = s.budget;
    let stop_reason = loop {
        let goal = s.budget.max(1.0).max(min_target as f32 * cheapest.min(1e6));
        stage.report("Placing mobs", (layout.spent / goal).max(layout.entities as f32 / min_target.max(1) as f32));
        if layout.entities >= s.max_entities {
            break "entity cap reached";
        }
        // 2. Buy a pack — past the budget only to reach the minimum.
        let below_min = layout.entities < min_target;
        let over_budget = below_min && remaining < cheapest;
        let mut want = rng.random_range(pack_lo..=pack_hi).min(s.max_entities - layout.entities);
        if over_budget {
            want = want.min(min_target - layout.entities);
        }
        let mut pack: Vec<usize> = Vec::new();
        let mut cost = 0.0;
        while (pack.len() as u32) < want {
            let total_after = (layout.entities + pack.len() as u32 + 1) as f32;
            let count = |t: usize| layout.per_type[t] + pack.iter().filter(|&&p| p == t).count() as u32;
            let pick = (0..n_types)
                .filter(|&t| target[t] > 0.0 && (over_budget || price(t) <= remaining - cost))
                .filter(|&t| pack.iter().all(|&p| s.combines(t, p)))
                .max_by(|&a, &b| {
                    let deficit = |t: usize| target[t] * total_after - count(t) as f32;
                    // Ties go to the lower index (max_by keeps the last max).
                    deficit(a).total_cmp(&deficit(b)).then(b.cmp(&a))
                });
            let Some(t) = pick else {
                break;
            };
            pack.push(t);
            cost += price(t);
        }
        if pack.is_empty() {
            break "budget spent";
        }

        // 4. Find a spot; a pack that fits nowhere shrinks to one mob.
        let mut spot = find_spot(rng, &mut candidates, &spawnable, &room_of, &layout.spawns, spacing, &pack, s, w, h, cell);
        if spot.is_none() && pack.len() > 1 {
            pack.truncate(1);
            cost = price(pack[0]);
            spot = find_spot(rng, &mut candidates, &spawnable, &room_of, &layout.spawns, spacing, &pack, s, w, h, cell);
        }
        let Some((c, radius)) = spot else {
            if below_min && spacing > 0.0 {
                // Relax the spacing and give the dropped cells another chance.
                spacing = if spacing > 0.5 { spacing / 2.0 } else { 0.0 };
                candidates = (0..w * h).filter(|&i| spawnable[i]).collect();
                continue;
            }
            break "no free spot left";
        };

        let center = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
        let mut members: Vec<MobPackMember> = Vec::new();
        for &t in &pack {
            match members.iter_mut().find(|m| m.type_index == t) {
                Some(m) => m.count += 1,
                None => members.push(MobPackMember { type_index: t, name: s.types[t].name.clone(), count: 1 }),
            }
            layout.per_type[t] += 1;
        }
        // One marker per mob, at its own body size, spread over the pack's
        // circle (sunflower spiral, turned at random).
        let first = objects.len();
        let turn = rng.random_range(0.0..std::f32::consts::TAU);
        for (k, &t) in pack.iter().enumerate() {
            let (dx, dy) = member_offset(k, pack.len(), biggest_size(&pack, s, cell), turn);
            let r = s.types[t].size_m.max(0.05) / 2.0;
            objects.push(PlacedObject {
                category: Category::MobSpawn,
                type_name: s.types[t].name.clone(),
                color: s.types[t].color,
                x_m: center.0 + dx - r,
                y_m: center.1 + dy - r,
                w_m: r * 2.0,
                h_m: r * 2.0,
                angle_deg: 0.0,
                parts: Vec::new(),
            });
        }
        layout.entities += pack.len() as u32;
        layout.spent += cost;
        remaining -= cost;
        layout.spawns.push(MobSpawn {
            center_m: [center.0, center.1],
            radius_m: radius,
            members,
            cost,
            on_road: road(c),
            hidden_room: (room_of[c] != 0).then_some(room_of[c]),
            objects: (first..objects.len()).collect(),
        });
    };
    layout.stop_reason = stop_reason.to_owned();
    layout.spacing_used_m = spacing;
    if min_wanted > s.max_entities {
        notes.push(format!("Mobs: the minimum of {min_wanted} is above the cap — capped at {}", s.max_entities));
    }
    if layout.spent > s.budget + 1e-3 {
        notes.push(format!(
            "Mobs: went {:.0} over the {:.0} budget to reach the minimum of {min_target} mobs",
            layout.spent - s.budget,
            s.budget
        ));
    }
    if spacing < s.spacing_m {
        notes.push(format!(
            "Mobs: spawn spacing relaxed from {:.1} m to {spacing:.1} m to reach the minimum",
            s.spacing_m
        ));
    }
    if layout.entities < min_target {
        notes.push(format!(
            "Mobs: only {} of the minimum {min_target} fit — no walkable spot left",
            layout.entities
        ));
    } else if stop_reason == "no free spot left" {
        notes.push(format!(
            "Mobs: no free spot left — spent {:.0} of {:.0} on {} mobs; try a smaller spacing or pack",
            layout.spent, s.budget, layout.entities
        ));
    } else if stop_reason == "entity cap reached" && remaining >= s.types.iter().map(|t| t.price).fold(f32::MAX, f32::min) {
        notes.push(format!(
            "Mobs: the cap of {} mobs was reached with {:.0} of the budget left",
            s.max_entities, remaining
        ));
    }
    Some(MobBuild { layout, objects, notes })
}

/// The biggest body in the pack (at least a cell): the spiral's step.
fn biggest_size(pack: &[usize], s: &crate::model::MobSettings, cell: f32) -> f32 {
    pack.iter().map(|&t| s.types[t].size_m).fold(0.0f32, f32::max).max(cell)
}

/// Spiral step as a share of the biggest body: neighbours stay at least one
/// body apart.
const SPIRAL_STEP: f32 = 0.65;

/// Where member `k` of an `n`-mob pack stands, relative to the pack center:
/// alone in the middle, else on a sunflower spiral (golden angle, radius ∝
/// √(k + ½)) so members never overlap.
fn member_offset(k: usize, n: usize, body: f32, turn: f32) -> (f32, f32) {
    if n <= 1 {
        return (0.0, 0.0);
    }
    const GOLDEN_ANGLE: f32 = 2.399_963;
    let r = SPIRAL_STEP * body * (k as f32 + 0.5).sqrt();
    let a = turn + k as f32 * GOLDEN_ANGLE;
    (r * a.cos(), r * a.sin())
}

/// Radius of the circle holding every member of an `n`-mob pack.
fn pack_radius(n: usize, body: f32) -> f32 {
    if n <= 1 { body / 2.0 } else { SPIRAL_STEP * body * (n as f32 - 0.5).sqrt() + body / 2.0 }
}

/// A cell for the pack's center and the pack radius, or `None` after
/// `TRIES` draws. Cells too close to another spawn are dropped for good.
#[allow(clippy::too_many_arguments)]
fn find_spot(
    rng: &mut ChaCha8Rng,
    candidates: &mut Vec<usize>,
    spawnable: &[bool],
    room_of: &[u32],
    spawns: &[MobSpawn],
    spacing: f32,
    pack: &[usize],
    s: &crate::model::MobSettings,
    w: usize,
    h: usize,
    cell: f32,
) -> Option<(usize, f32)> {
    let radius = pack_radius(pack.len(), biggest_size(pack, s, cell));
    let reach = (radius / cell).ceil() as i64;
    let mut tries = 0;
    while tries < TRIES && !candidates.is_empty() {
        let k = rng.random_range(0..candidates.len());
        let c = candidates[k];
        let (cx, cy) = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
        let mut too_close = false;
        let mut overlaps = false;
        for sp in spawns {
            let d2 = (sp.center_m[0] - cx).powi(2) + (sp.center_m[1] - cy).powi(2);
            too_close |= d2 < spacing * spacing;
            overlaps |= d2 < (radius + sp.radius_m).powi(2);
        }
        if too_close {
            // The spacing only ever shrinks with a full candidate rebuild, so
            // the cell is out for good.
            candidates.swap_remove(k);
            continue;
        }
        if overlaps {
            // Depends on this pack's size: skip the cell, keep it.
            tries += 1;
            continue;
        }
        tries += 1;
        let (x, y) = ((c % w) as i64, (c / w) as i64);
        let fits = (-reach..=reach).all(|dy| {
            (-reach..=reach).all(|dx| {
                let (nx, ny) = (x + dx, y + dy);
                let (px, py) = ((nx as f32 + 0.5) * cell - cx, (ny as f32 + 0.5) * cell - cy);
                if px * px + py * py > radius * radius {
                    return true;
                }
                if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                    return false;
                }
                let i = ny as usize * w + nx as usize;
                spawnable[i] && room_of[i] == room_of[c]
            })
        });
        if fits {
            return Some((c, radius));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::GeneratedLevel;
    use crate::model::{HiddenRoomPlace, MapKind, MobType, ShapeSettings};

    fn level(config: &LevelConfig, seed: u64) -> GeneratedLevel {
        crate::generator::generate(config, seed)
    }

    fn cost_of(config: &LevelConfig, sp: &MobSpawn) -> f32 {
        sp.members.iter().map(|m| config.mobs.types[m.type_index].price * m.count as f32).sum()
    }

    #[test]
    fn budget_is_spent_as_far_as_it_goes() {
        for seed in 1..=4u64 {
            let config = LevelConfig::default();
            let lvl = level(&config, seed);
            let mobs = lvl.mobs.as_ref().unwrap();
            let spent: f32 = mobs.spawns.iter().map(|sp| cost_of(&config, sp)).sum();
            assert!((spent - mobs.spent).abs() < 1e-3);
            assert!(spent <= config.mobs.budget + 1e-3);
            // Default: budget 400, Goblins at 10 → whatever is left buys
            // nothing more (unless the cap or space stopped it).
            if mobs.stop_reason == "budget spent" {
                assert!(config.mobs.budget - spent < 10.0, "seed {seed}: {spent}");
            }
            assert!(mobs.entities <= config.mobs.max_entities);
            for sp in &mobs.spawns {
                let n: u32 = sp.members.iter().map(|m| m.count).sum();
                assert!((1..=4).contains(&n), "pack of {n}");
            }
        }
    }

    #[test]
    fn counts_follow_the_balance() {
        let mut config = LevelConfig::default();
        config.mobs.budget = 1000.0;
        config.mobs.max_entities = 1000;
        config.mobs.spacing_m = 2.0;
        config.mobs.types[1].price = 10.0; // equal prices: only balance decides
        let lvl = level(&config, 2);
        let mobs = lvl.mobs.as_ref().unwrap();
        let (a, b) = (mobs.per_type[0] as f32, mobs.per_type[1] as f32);
        let share = a / (a + b);
        assert!((share - 0.7).abs() < 0.08, "goblin share {share} ({a}/{b}), {}", mobs.stop_reason);
    }

    #[test]
    fn entity_cap_holds() {
        let mut config = LevelConfig::default();
        config.mobs.budget = 100_000.0;
        config.mobs.max_entities = 12;
        let lvl = level(&config, 3);
        let mobs = lvl.mobs.as_ref().unwrap();
        assert_eq!(mobs.entities, 12);
        assert_eq!(mobs.stop_reason, "entity cap reached");
    }

    #[test]
    fn forbidden_combinations_never_share_a_spawn() {
        let mut config = LevelConfig::default();
        config.mobs.set_combines(0, 1, false);
        config.mobs.types[1].price = 10.0;
        config.mobs.types[0].balance_pct = 50.0;
        config.mobs.types[1].balance_pct = 50.0;
        for seed in 1..=3u64 {
            let lvl = level(&config, seed);
            for sp in &lvl.mobs.as_ref().unwrap().spawns {
                assert_eq!(sp.members.len(), 1, "seed {seed}: mixed pack {:?}", sp.members);
            }
        }
        // Allowed → mixed packs show up.
        config.mobs.set_combines(0, 1, true);
        let lvl = level(&config, 1);
        assert!(lvl.mobs.as_ref().unwrap().spawns.iter().any(|sp| sp.members.len() == 2));
    }

    #[test]
    fn spawns_keep_spacing_and_stand_on_walkable_ground() {
        for seed in 1..=4u64 {
            let config = LevelConfig::default();
            let lvl = level(&config, seed);
            let mobs = lvl.mobs.as_ref().unwrap();
            let map = &lvl.regions.as_ref().unwrap().map;
            let (w, cell) = (map.width, map.cell_size_m);
            for (i, a) in mobs.spawns.iter().enumerate() {
                for b in &mobs.spawns[i + 1..] {
                    let d = ((a.center_m[0] - b.center_m[0]).powi(2) + (a.center_m[1] - b.center_m[1]).powi(2)).sqrt();
                    assert!(d >= config.mobs.spacing_m - 1e-3, "seed {seed}: spawns {d} m apart");
                }
                let c = (a.center_m[1] / cell) as usize * w + (a.center_m[0] / cell) as usize;
                let solid = [
                    lvl.houses.as_ref().map(|l| &l.mask),
                    lvl.props.as_ref().map(|l| &l.mask),
                    lvl.interactive.as_ref().map(|l| &l.mask),
                    lvl.walls.as_ref().map(|l| &l.mask),
                    lvl.region_walls.as_ref().map(|l| &l.mask),
                ];
                assert!(!solid.iter().flatten().any(|m| m.cells[c]), "seed {seed}: spawn on a solid cell");
                assert!(map.ids[c] != 0 || a.hidden_room.is_some());
            }
        }
    }

    #[test]
    fn roads_are_used_only_when_allowed() {
        let mut config = LevelConfig::default();
        config.mobs.budget = 2000.0;
        config.mobs.max_entities = 200;
        config.mobs.spacing_m = 3.0;
        let on = level(&config, 4);
        assert!(on.mobs.as_ref().unwrap().spawns.iter().any(|sp| sp.on_road));
        config.mobs.on_roads = false;
        let off = level(&config, 4);
        let road = &off.roads.as_ref().unwrap().mask;
        for sp in &off.mobs.as_ref().unwrap().spawns {
            assert!(!sp.on_road);
            let c = (sp.center_m[1] / road.cell_size_m) as usize * road.width + (sp.center_m[0] / road.cell_size_m) as usize;
            assert!(!road.cells[c]);
        }
    }

    #[test]
    fn hidden_rooms_get_mobs_only_through_passable_paths() {
        let mut config = LevelConfig::default();
        config.hidden_rooms.count = 3;
        config.hidden_rooms.place = HiddenRoomPlace::OutsideWall;
        config.mobs.budget = 3000.0;
        config.mobs.max_entities = 300;
        config.mobs.spacing_m = 2.0;
        config.hidden_rooms.mob_passable_pct = 0.0;
        let closed = level(&config, 3);
        assert!(closed.mobs.as_ref().unwrap().spawns.iter().all(|sp| sp.hidden_room.is_none()));
        config.hidden_rooms.mob_passable_pct = 100.0;
        let open = level(&config, 3);
        assert!(!open.hidden_rooms.as_ref().unwrap().rooms.is_empty());
        assert!(open.mobs.as_ref().unwrap().spawns.iter().any(|sp| sp.hidden_room.is_some()), "{:?}", open.notes);
    }

    #[test]
    fn minimum_count_goes_over_budget_and_relaxes_spacing() {
        let mut config = LevelConfig::default();
        config.mobs.budget = 50.0;
        config.mobs.min_entities = 80;
        config.mobs.max_entities = 100;
        config.mobs.pack_max = 1; // 80 separate spawns cannot keep 5 m apart
        let lvl = level(&config, 2);
        let mobs = lvl.mobs.as_ref().unwrap();
        assert_eq!(mobs.min_target, 80);
        assert!(mobs.entities >= 80, "{} mobs, {:?}", mobs.entities, lvl.notes);
        assert!(mobs.spent > 50.0);
        assert!(lvl.notes.iter().any(|n| n.contains("over the 50 budget")), "{:?}", lvl.notes);
        assert!(mobs.spacing_used_m < config.mobs.spacing_m, "80 packs need a tighter spacing");
        // Circles never overlap, even with relaxed spacing.
        for (i, a) in mobs.spawns.iter().enumerate() {
            for b in &mobs.spawns[i + 1..] {
                let d = ((a.center_m[0] - b.center_m[0]).powi(2) + (a.center_m[1] - b.center_m[1]).powi(2)).sqrt();
                assert!(d + 1e-3 >= a.radius_m + b.radius_m, "spawns overlap");
            }
        }
    }

    #[test]
    fn minimum_density_scales_with_walkable_ground() {
        let mut config = LevelConfig::default();
        config.mobs.min_amount = NodeAmount::Density;
        config.mobs.min_per_100m2 = 2.0;
        config.mobs.max_entities = 2000;
        let lvl = level(&config, 3);
        let mobs = lvl.mobs.as_ref().unwrap();
        assert!(mobs.min_target > 20, "target {}", mobs.min_target);
        assert!(mobs.entities >= mobs.min_target, "{} of {}: {:?}", mobs.entities, mobs.min_target, lvl.notes);
    }

    #[test]
    fn the_cap_beats_the_minimum() {
        let mut config = LevelConfig::default();
        config.mobs.min_entities = 500;
        config.mobs.max_entities = 30;
        let lvl = level(&config, 1);
        let mobs = lvl.mobs.as_ref().unwrap();
        assert_eq!(mobs.entities, 30);
        assert!(lvl.notes.iter().any(|n| n.contains("above the cap")));
    }

    #[test]
    fn no_types_or_budget_places_nothing() {
        let mut config = LevelConfig::default();
        config.mobs.budget = 0.0;
        assert!(level(&config, 1).mobs.as_ref().unwrap().spawns.is_empty());
        let mut config = LevelConfig::default();
        config.mobs.types.clear();
        config.mobs.combinations.clear();
        assert!(level(&config, 1).mobs.as_ref().unwrap().spawns.is_empty());
    }

    #[test]
    fn one_marker_per_mob_at_its_type_size() {
        let config = LevelConfig::default();
        let lvl = level(&config, 5);
        let mobs = lvl.mobs.as_ref().unwrap();
        let markers = lvl.objects.iter().filter(|o| o.category == Category::MobSpawn).count();
        assert_eq!(markers as u32, mobs.entities);
        for sp in &mobs.spawns {
            let n: u32 = sp.members.iter().map(|m| m.count).sum();
            assert_eq!(sp.objects.len() as u32, n);
            for (i, &a) in sp.objects.iter().enumerate() {
                let o = &lvl.objects[a];
                assert_eq!(o.category, Category::MobSpawn);
                let t = config.mobs.types.iter().find(|t| t.name == o.type_name).unwrap();
                assert!((o.w_m - t.size_m).abs() < 1e-4, "{} marker is {} m", t.name, o.w_m);
                // Inside the pack circle, and never on another member.
                let (cx, cy) = o.center();
                let d = ((cx - sp.center_m[0]).powi(2) + (cy - sp.center_m[1]).powi(2)).sqrt();
                assert!(d + o.w_m / 2.0 <= sp.radius_m + 1e-3);
                for &b in &sp.objects[i + 1..] {
                    let p = &lvl.objects[b];
                    let (px, py) = p.center();
                    let dd = ((cx - px).powi(2) + (cy - py).powi(2)).sqrt();
                    assert!(dd + 1e-3 >= (o.w_m + p.w_m) / 2.0, "pack members overlap");
                }
            }
        }
    }

    #[test]
    fn spiral_never_overlaps_for_any_pack_size() {
        for n in 2..=20 {
            let pts: Vec<(f32, f32)> = (0..n).map(|k| member_offset(k, n, 1.0, 0.3)).collect();
            for (i, a) in pts.iter().enumerate() {
                let d = (a.0 * a.0 + a.1 * a.1).sqrt();
                assert!(d + 0.5 <= pack_radius(n, 1.0) + 1e-4);
                for b in &pts[i + 1..] {
                    let dd = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
                    assert!(dd >= 0.999, "pack of {n}: members {dd} apart");
                }
            }
        }
    }

    #[test]
    fn types_add_and_remove_keep_the_matrix_square() {
        let mut m = crate::model::MobSettings::default();
        m.add_type(MobType::default());
        assert_eq!(m.combinations.len(), 3);
        assert!(m.combinations.iter().all(|r| r.len() == 3));
        m.set_combines(0, 2, false);
        assert!(!m.combines(2, 0));
        m.remove_type(1);
        assert_eq!(m.combinations.len(), 2);
        assert!(!m.combines(0, 1));
    }

    #[test]
    fn non_town_maps_get_mobs() {
        let mut config = LevelConfig::default();
        config.kind = MapKind::Swamp;
        config.shape = ShapeSettings::for_kind(config.kind);
        assert!(!level(&config, 2).mobs.as_ref().unwrap().spawns.is_empty());
    }

    #[test]
    fn same_seed_same_mobs() {
        let config = LevelConfig::default();
        assert_eq!(level(&config, 8).mobs, level(&config, 8).mobs);
    }
}
