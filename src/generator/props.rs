use rand::Rng;

use crate::generator::grid::{Grid, RectM};
use crate::level::{Category, ObjectDetails, PlacedObject};
use crate::model::LevelConfig;

/// Scatter uninteractive props on free ground. Quantity is the requested count;
/// density acts as a coverage cap: props never fill more than density % of the map
/// (in expectation), excess quantity is skipped with a note.
pub fn place(
    rng: &mut impl Rng,
    grid: &mut Grid,
    config: &LevelConfig,
    objects: &mut Vec<PlacedObject>,
    notes: &mut Vec<String>,
) {
    let p = &config.props;
    if p.quantity == 0 {
        return;
    }
    let map = RectM::new(0.0, 0.0, config.map.width_m, config.map.height_m);
    let avg = ((p.size.min_m + p.size.max_m) * 0.5).max(0.01);
    let avg_area = avg * avg;
    let max_by_density = ((map.area() * p.density_pct.clamp(0.0, 100.0) / 100.0) / avg_area) as u32;

    let effective = p.quantity.min(max_by_density.max(1));
    if effective < p.quantity {
        notes.push(format!(
            "Props: density {:.0}% caps the count at ~{effective} (requested {})",
            p.density_pct, p.quantity
        ));
    }

    let mut skipped = 0u32;
    for _ in 0..effective {
        let w = rng.random_range(p.size.min_m..=p.size.max_m.max(p.size.min_m));
        let h = rng.random_range(p.size.min_m..=p.size.max_m.max(p.size.min_m));
        match grid.random_free_rect(rng, w, h, map, 50) {
            Some(rect) => {
                grid.mark(rect);
                objects.push(PlacedObject {
                    category: Category::Prop,
                    type_name: "Prop".to_owned(),
                    color: p.color,
                    x_m: rect.x,
                    y_m: rect.y,
                    w_m: rect.w,
                    h_m: rect.h,
                    details: ObjectDetails::Prop,
                });
            }
            None => skipped += 1,
        }
    }
    if skipped > 0 {
        notes.push(format!(
            "Props: {skipped} could not be placed (no free space)"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    #[test]
    fn props_place_overlap_free() {
        let config = LevelConfig::default();
        let mut rng = ChaCha8Rng::seed_from_u64(31);
        let mut grid = Grid::new(config.map.width_m, config.map.height_m, config.cell_size_m);
        let mut objects = Vec::new();
        let mut notes = Vec::new();
        place(&mut rng, &mut grid, &config, &mut objects, &mut notes);

        let props: Vec<_> = objects
            .iter()
            .filter(|o| o.category == Category::Prop)
            .collect();
        assert_eq!(props.len() as u32, config.props.quantity);
        assert!(notes.is_empty(), "notes: {notes:?}");
        for (i, a) in props.iter().enumerate() {
            assert!(a.x_m + a.w_m <= config.map.width_m + 0.01);
            for b in props.iter().skip(i + 1) {
                assert!(!a.rect().overlaps(&b.rect()));
            }
        }
    }

    #[test]
    fn density_caps_quantity() {
        let mut config = LevelConfig::default();
        config.props.density_pct = 1.0; // tiny coverage cap
        let mut rng = ChaCha8Rng::seed_from_u64(32);
        let mut grid = Grid::new(config.map.width_m, config.map.height_m, config.cell_size_m);
        let mut objects = Vec::new();
        let mut notes = Vec::new();
        place(&mut rng, &mut grid, &config, &mut objects, &mut notes);

        let props = objects
            .iter()
            .filter(|o| o.category == Category::Prop)
            .count();
        assert!(props < config.props.quantity as usize, "expected density cap to bite");
        assert!(notes.iter().any(|n| n.contains("caps the count")));
    }
}
