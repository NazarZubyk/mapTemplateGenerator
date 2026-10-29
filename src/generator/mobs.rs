use rand::Rng;

use crate::generator::grid::{Grid, RectM};
use crate::level::{Category, ObjectDetails, PlacedObject};
use crate::model::LevelConfig;

const MAX_PACK: u32 = 3;
const CELL_ATTEMPTS: usize = 100;

/// Spend the mob budget greedily: spawn points on free ground, packs (1..=3 mobs)
/// chosen by balance weights among types allowed by the combination rules. Stops on
/// budget exhaustion, the entity cap, or no free space — the reason goes into notes.
///
/// Interpretation of `mob_passable = false`: mobs do not spawn inside hidden rooms,
/// since their only access is the narrow player path.
pub fn place(
    rng: &mut impl Rng,
    grid: &mut Grid,
    config: &LevelConfig,
    room_rects: &[RectM],
    objects: &mut Vec<PlacedObject>,
    notes: &mut Vec<String>,
) {
    let mobs = &config.mobs;
    if mobs.types.is_empty() || mobs.budget <= 0.0 || mobs.max_entities == 0 {
        return;
    }
    let n = mobs.types.len();
    let weights: Vec<f32> = if mobs.balance.len() == n {
        mobs.balance.clone()
    } else {
        vec![1.0; n]
    };
    let comb = |a: usize, b: usize| {
        mobs.combinations
            .get(a)
            .and_then(|r| r.get(b))
            .copied()
            .unwrap_or(true)
    };

    let mut remaining_budget = mobs.budget;
    let mut entities: u32 = 0;
    let mut spawn_points: u32 = 0;
    let stop_reason;

    'outer: loop {
        if entities >= mobs.max_entities {
            stop_reason = "max entities reached";
            break;
        }
        let min_price = mobs.types.iter().map(|t| t.price).fold(f32::INFINITY, f32::min);
        if min_price.is_finite() && remaining_budget < min_price {
            stop_reason = "budget exhausted";
            break;
        }

        // Find a spawn cell; when paths are mob-proof, skip cells inside hidden rooms.
        let cell = loop {
            match grid.random_free_cell(rng, CELL_ATTEMPTS) {
                None => break None,
                Some(c) => {
                    let inside_room = !config.hidden_rooms.mob_passable
                        && room_rects
                            .iter()
                            .any(|r| r.contains_point(c.x + c.w / 2.0, c.y + c.h / 2.0));
                    if !inside_room {
                        break Some(c);
                    }
                }
            }
        };
        let Some(cell) = cell else {
            stop_reason = "no free space";
            break 'outer;
        };

        // Build a pack: weighted pick among types compatible with the pack so far.
        let mut members: Vec<(usize, u32)> = Vec::new();
        let mut pack_budget = 0.0;
        loop {
            let allowed: Vec<usize> = (0..n)
                .filter(|&i| {
                    mobs.types[i].price <= remaining_budget - pack_budget
                        && members
                            .iter()
                            .all(|&(j, _)| i == j || comb(i, j))
                })
                .collect();
            if allowed.is_empty() || entities >= mobs.max_entities {
                break;
            }
            let Some(&pick) = weighted_pick(rng, &weights, &allowed).as_ref() else {
                break;
            };
            match members.iter_mut().find(|(j, _)| *j == pick) {
                Some((_, count)) => *count += 1,
                None => members.push((pick, 1)),
            }
            pack_budget += mobs.types[pick].price;
            entities += 1;
            // Continue the pack probabilistically, capped at MAX_PACK mobs.
            if members.iter().map(|(_, c)| c).sum::<u32>() >= MAX_PACK || !rng.random_bool(0.5) {
                break;
            }
        }
        if members.is_empty() {
            stop_reason = "budget exhausted";
            break 'outer;
        }
        remaining_budget -= pack_budget;

        // Footprint sized by the biggest mob in the pack; falls back to one cell.
        let size = members
            .iter()
            .map(|&(i, _)| mobs.types[i].size_m)
            .fold(0.0_f32, f32::max);
        let mut rect = RectM::new(cell.x, cell.y, size.max(grid.cell_m), size.max(grid.cell_m));
        if !grid.area_free(rect) {
            rect = cell;
        }
        grid.mark(rect);
        spawn_points += 1;

        let (first, _) = members[0];
        objects.push(PlacedObject {
            category: Category::MobSpawn,
            type_name: mobs.types[first].name.clone(),
            color: mobs.types[first].color,
            x_m: rect.x,
            y_m: rect.y,
            w_m: rect.w,
            h_m: rect.h,
            details: ObjectDetails::MobSpawn {
                members: members
                    .into_iter()
                    .map(|(i, count)| (mobs.types[i].name.clone(), count))
                    .collect(),
            },
        });
    }

    let spent = mobs.budget - remaining_budget;
    notes.push(format!(
        "Mobs: {spawn_points} spawn points, {entities} entities, spent {:.0} of {:.0} budget ({stop_reason})",
        spent, mobs.budget
    ));
}

/// Weighted random pick among `allowed` indices; falls back to a uniform pick when
/// all weights are zero.
fn weighted_pick(rng: &mut impl Rng, weights: &[f32], allowed: &[usize]) -> Option<usize> {
    let total: f32 = allowed.iter().map(|&i| weights.get(i).copied().unwrap_or(1.0).max(0.0)).sum();
    if total <= 0.0 {
        return allowed.get(rng.random_range(0..allowed.len())).copied();
    }
    let mut roll = rng.random::<f32>() * total;
    for &i in allowed {
        roll -= weights.get(i).copied().unwrap_or(1.0).max(0.0);
        if roll <= 0.0 {
            return Some(i);
        }
    }
    allowed.last().copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::structures::{build_zones, place as place_structures};
    use crate::generator::grid::WalkBudget;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    fn small_level(seed: u64) -> Vec<PlacedObject> {
        let config = LevelConfig::default();
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut grid = Grid::new(config.map.width_m, config.map.height_m, config.cell_size_m);
        let mut budget = WalkBudget::new(grid.total_cells(), config.map.walkable_pct);
        let mut objects = Vec::new();
        let mut notes = Vec::new();
        let zones = build_zones(&mut rng, &config);
        place_structures(&mut rng, &mut grid, &mut budget, &zones, &config, &mut objects, &mut notes);
        let rooms = crate::generator::hidden_rooms::place(&mut rng, &mut grid, &config, &mut objects, &mut notes);
        crate::generator::interactives::place(&mut rng, &mut grid, &config, &mut objects, &mut notes);
        crate::generator::props::place(&mut rng, &mut grid, &config, &mut objects, &mut notes);
        place(&mut rng, &mut grid, &config, &rooms, &mut objects, &mut notes);
        objects
    }

    #[test]
    fn respects_budget_and_entity_cap() {
        let config = LevelConfig::default();
        let objects = small_level(41);
        let spawns: Vec<_> = objects
            .iter()
            .filter(|o| o.category == Category::MobSpawn)
            .collect();
        assert!(!spawns.is_empty());

        let mut entities = 0u32;
        let mut spent = 0.0f32;
        for o in &spawns {
            if let ObjectDetails::MobSpawn { members } = &o.details {
                for (name, count) in members {
                    let price = config
                        .mobs
                        .types
                        .iter()
                        .find(|t| &t.name == name)
                        .unwrap()
                        .price;
                    spent += price * *count as f32;
                    entities += count;
                }
            }
        }
        assert!(spent <= config.mobs.budget + 1e-3, "spent {spent}");
        assert!(entities <= config.mobs.max_entities);

        // Mobs never sit inside hidden rooms (default: paths not mob-passable).
        for o in &spawns {
            for c in 0..4 {
                let _ = c;
            }
            assert!(!objects
                .iter()
                .any(|r| r.category == Category::HiddenRoom && r.rect().overlaps(&o.rect())));
        }
    }
}
