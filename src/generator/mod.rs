pub mod gates;
pub mod grid;
pub mod hidden_rooms;
pub mod interactives;
pub mod mobs;
pub mod props;
pub mod structures;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::level::GeneratedLevel;
use crate::model::LevelConfig;

/// Generate a level deterministically: same (config, seed) always yields the same
/// result. Phases run in a fixed order on one RNG stream:
/// zones -> structures -> hidden rooms + paths -> interactives -> props -> mobs -> gates.
pub fn generate(config: &LevelConfig, seed: u64) -> GeneratedLevel {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut grid = grid::Grid::new(config.map.width_m, config.map.height_m, config.cell_size_m);
    let mut budget = grid::WalkBudget::new(grid.total_cells(), config.map.walkable_pct);
    let mut objects = Vec::new();
    let mut notes = Vec::new();

    let zones = structures::build_zones(&mut rng, config);
    structures::place(&mut rng, &mut grid, &mut budget, &zones, config, &mut objects, &mut notes);
    let rooms = hidden_rooms::place(&mut rng, &mut grid, config, &mut objects, &mut notes);
    interactives::place(&mut rng, &mut grid, config, &mut objects, &mut notes);
    props::place(&mut rng, &mut grid, config, &mut objects, &mut notes);
    mobs::place(&mut rng, &mut grid, config, &rooms, &mut objects, &mut notes);
    gates::place(&grid, config, &mut objects, &mut notes);

    let fragments = grid.connected_walkable_components(4);
    if fragments > 1 {
        notes.push(format!(
            "Walkable area is fragmented into {fragments} disconnected regions"
        ));
    }

    GeneratedLevel {
        config: config.clone(),
        seed,
        objects,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{Category, ObjectDetails};
    use crate::model::{TemplateKind, TemplateMix};

    #[test]
    fn same_seed_same_result_different_seed_differs() {
        let config = LevelConfig::default();
        let a = generate(&config, 42);
        let b = generate(&config, 42);
        let c = generate(&config, 43);
        assert_eq!(a.objects, b.objects);
        assert_ne!(a.objects, c.objects);
    }

    #[test]
    fn solid_objects_never_overlap() {
        let mut config = LevelConfig {
            template_mix: TemplateMix::Mix(TemplateKind::Town, TemplateKind::Cave),
            ..LevelConfig::default()
        };
        config.hidden_rooms.count = 3;
        let level = generate(&config, 7);
        assert!(!level.objects.is_empty());
        for (i, a) in level.objects.iter().enumerate() {
            for b in level.objects.iter().skip(i + 1) {
                if a.category == Category::Gate || b.category == Category::Gate {
                    continue; // gates are pure points, may sit near objects
                }
                assert!(!a.rect().overlaps(&b.rect()), "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn mix_places_both_structure_kinds() {
        let config = LevelConfig {
            template_mix: TemplateMix::Mix(TemplateKind::Town, TemplateKind::Swamp),
            ..LevelConfig::default()
        };
        let level = generate(&config, 9);
        let kinds: Vec<_> = level
            .objects
            .iter()
            .filter(|o| o.category == Category::Structure)
            .map(|o| o.type_name.as_str())
            .collect();
        assert!(kinds.contains(&"House"), "kinds: {kinds:?}");
        assert!(kinds.contains(&"Swamp tree"), "kinds: {kinds:?}");
    }

    #[test]
    fn gates_sit_on_the_border() {
        let mut config = LevelConfig {
            gates: vec![
                crate::model::Gate { direction_deg: 0.0, color: [200, 0, 0] },
                crate::model::Gate { direction_deg: 45.0, color: [0, 200, 0] },
                crate::model::Gate { direction_deg: 200.0, color: [0, 0, 200] },
            ],
            ..LevelConfig::default()
        };
        // Keep gate colors unique vs the defaults.
        config.props.color = [10, 10, 10];
        let level = generate(&config, 13);
        let w = config.map.width_m;
        let h = config.map.height_m;
        for g in level.objects.iter().filter(|o| o.category == Category::Gate) {
            let on_border = g.x_m.abs() < 1e-3
                || g.y_m.abs() < 1e-3
                || (g.x_m + g.w_m - w).abs() < 1e-3
                || (g.y_m + g.h_m - h).abs() < 1e-3;
            assert!(on_border, "gate not on border: {g:?}");
        }
    }

    #[test]
    fn mob_spawn_details_carry_composition() {
        let mut config = LevelConfig::default();
        config.mobs.add_type(crate::model::MobType {
            name: "Orc".to_owned(),
            price: 25.0,
            size_m: 1.5,
            color: [10, 120, 200],
        });
        let level = generate(&config, 17);
        let with_members = level
            .objects
            .iter()
            .filter_map(|o| match &o.details {
                ObjectDetails::MobSpawn { members } => Some(members),
                _ => None,
            })
            .count();
        assert!(with_members > 0, "no mob spawns generated");
    }
}
