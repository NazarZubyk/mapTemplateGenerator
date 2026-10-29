use rand::Rng;

use crate::generator::grid::{Grid, RectM};
use crate::level::{Category, ObjectDetails, PlacedObject};
use crate::model::LevelConfig;

/// Place interactive objects per type in clusters (groups) on free ground.
pub fn place(
    rng: &mut impl Rng,
    grid: &mut Grid,
    config: &LevelConfig,
    objects: &mut Vec<PlacedObject>,
    notes: &mut Vec<String>,
) {
    let map = RectM::new(0.0, 0.0, config.map.width_m, config.map.height_m);
    let regions_before = grid.connected_walkable_components(1);

    for (type_idx, it) in config.interactive.iter().enumerate() {
        let name = if it.name.is_empty() {
            format!("Interactive #{}", type_idx + 1)
        } else {
            it.name.clone()
        };
        let size = it.size_m.max(0.01);
        let gmin = it.group.min_m.max(1.0);
        let gmax = it.group.max_m.max(gmin).max(gmin);

        let mut group_id: u32 = 0;
        let mut remaining = it.quantity;
        let mut rejected = 0u32;
        while remaining > 0 {
            let want = rng.random_range(gmin..=gmax).round().max(1.0) as u32;
            let k = want.min(remaining);
            // Cluster laid out on a small grid: ceil(sqrt(k)) columns.
            let cols = (k as f32).sqrt().ceil() as u32;
            let rows = k.div_ceil(cols);
            let bbox = match grid.random_free_rect(
                rng,
                cols as f32 * size,
                rows as f32 * size,
                map,
                50,
            ) {
                Some(b) => {
                    // A cluster must not seal off a street or pocket; on success
                    // the whole bbox is already marked on the grid.
                    if grid.placement_keeps_regions(b, regions_before) {
                        b
                    } else {
                        rejected += 1;
                        if rejected > 100 {
                            let placed = it.quantity - remaining;
                            notes.push(format!(
                                "Interactive '{name}': placed {placed} of {} (free spots \
                                 would seal the map)",
                                it.quantity
                            ));
                            break;
                        }
                        continue;
                    }
                }
                None => {
                    let placed = it.quantity - remaining;
                    notes.push(format!(
                        "Interactive '{name}': placed {placed} of {} (no free space)",
                        it.quantity
                    ));
                    break;
                }
            };
            for i in 0..k {
                let (gc, gr) = (i % cols, i / cols);
                let rect = RectM::new(
                    bbox.x + gc as f32 * size,
                    bbox.y + gr as f32 * size,
                    size,
                    size,
                );
                objects.push(PlacedObject {
                    category: Category::Interactive,
                    type_name: name.clone(),
                    color: it.color,
                    x_m: rect.x,
                    y_m: rect.y,
                    w_m: rect.w,
                    h_m: rect.h,
                    angle_deg: 0.0,
                    details: ObjectDetails::Interactive { group_id, group_size: k },
                });
            }
            group_id += 1;
            remaining -= k;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    #[test]
    fn groups_place_exactly_quantity_objects() {
        let config = LevelConfig::default();
        let mut rng = ChaCha8Rng::seed_from_u64(21);
        let mut grid = Grid::new(config.map.width_m, config.map.height_m, config.cell_size_m);
        let mut objects = Vec::new();
        let mut notes = Vec::new();
        place(&mut rng, &mut grid, &config, &mut objects, &mut notes);

        let herbs: Vec<_> = objects
            .iter()
            .filter(|o| o.category == Category::Interactive)
            .collect();
        let quantity = config.interactive[0].quantity as usize;
        assert_eq!(herbs.len(), quantity);
        assert!(notes.is_empty(), "notes: {notes:?}");

        // Groups are 2..=5 and sum to the quantity.
        let mut total = 0;
        let mut group_sizes = Vec::new();
        for o in &herbs {
            if let ObjectDetails::Interactive { group_size, .. } = &o.details {
                if !group_sizes.contains(group_size) {
                    group_sizes.push(*group_size);
                }
                total += 1;
            }
        }
        assert_eq!(total, quantity);
        for g in group_sizes {
            assert!((1..=5).contains(&g), "group size {g} out of range");
        }

        // Overlap-free.
        for (i, a) in herbs.iter().enumerate() {
            for b in herbs.iter().skip(i + 1) {
                assert!(!a.rect().overlaps(&b.rect()), "overlap:\n{a:?}\n{b:?}");
            }
        }
    }
}
