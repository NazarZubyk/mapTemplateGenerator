pub mod gates;
pub mod geometry;
pub mod grid;
pub mod hidden_rooms;
pub mod interactives;
pub mod mobs;
pub mod props;
pub mod roads;
pub mod sections;
pub mod structures;
pub mod walls;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::level::GeneratedLevel;
use crate::model::LevelConfig;

/// Generate a level deterministically: same (config, seed) always yields the same
/// result. Phases run in a fixed order on one RNG stream:
/// zones -> town shape (blob, perimeter wall, gate plazas, sections, road web)
/// -> structures -> hidden rooms + paths -> interactives -> props -> mobs -> gates.
pub fn generate(config: &LevelConfig, seed: u64) -> GeneratedLevel {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut grid = grid::Grid::new(config.map.width_m, config.map.height_m, config.cell_size_m);
    let mut budget = grid::WalkBudget::new(grid.total_cells(), config.map.walkable_pct);
    let mut objects = Vec::new();
    let mut notes = Vec::new();

    let zones = structures::build_zones(&mut rng, config);
    let blobs = walls::place(&mut rng, &mut grid, &mut budget, &zones, config, &mut objects, &mut notes);
    structures::place(&mut rng, &mut grid, &mut budget, &zones, config, &mut objects, &mut notes);
    let rooms = hidden_rooms::place(&mut rng, &mut grid, config, &mut objects, &mut notes);
    interactives::place(&mut rng, &mut grid, config, &mut objects, &mut notes);
    props::place(&mut rng, &mut grid, config, &mut objects, &mut notes);
    mobs::place(&mut rng, &mut grid, config, &rooms, &mut objects, &mut notes);
    gates::place(&grid, config, &blobs, &mut objects, &mut notes);

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
    use crate::generator::geometry;
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

    /// SAT overlap check over two placed objects (handles rotations).
    fn overlaps(a: &crate::level::PlacedObject, b: &crate::level::PlacedObject) -> bool {
        let corners = |o: &crate::level::PlacedObject| {
            geometry::rot_rect_corners(
                (o.x_m + o.w_m / 2.0, o.y_m + o.h_m / 2.0),
                o.w_m,
                o.h_m,
                o.angle_deg,
            )
        };
        geometry::rotated_rects_overlap(&corners(a), &corners(b))
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
                // Gates are pure points; roads may cross anything (open ground);
                // wall strips overlap each other at the joints by construction.
                let skippable = |o: &crate::level::PlacedObject| {
                    o.category == Category::Gate
                        || o.category == Category::Road
                        || o.category == Category::Wall
                };
                if skippable(a) || skippable(b) {
                    continue;
                }
                assert!(!overlaps(a, b), "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn town_is_walled_by_default_and_organic() {
        let level = generate(&LevelConfig::default(), 42);
        let (w, h) = (level.config.map.width_m, level.config.map.height_m);

        let perimeter: Vec<_> = level
            .objects
            .iter()
            .filter(|o| matches!(&o.details, ObjectDetails::Wall { perimeter: true, .. }))
            .collect();
        assert!(!perimeter.is_empty(), "no perimeter wall generated");
        // The wall follows an organic blob: strips sit in a broad ring around the
        // map center, NOT in straight lines along the map border.
        let center = (w / 2.0, h / 2.0);
        let mut radii = Vec::new();
        for p in &perimeter {
            let c = (p.x_m + p.w_m / 2.0, p.y_m + p.h_m / 2.0);
            radii.push(geometry::dist(c, center));
        }
        let r_min = radii.iter().cloned().fold(f32::INFINITY, f32::min);
        let r_max = radii.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(r_min > 0.25 * w.min(h), "wall too close to center: {r_min}");
        assert!(r_max < 0.75 * w.max(h), "wall outside the map: {r_max}");
        assert!(r_max - r_min > 1.0, "boundary looks like a perfect circle");

        assert!(
            level
                .objects
                .iter()
                .any(|o| matches!(&o.details, ObjectDetails::Wall { perimeter: false, .. })),
            "no section walls generated"
        );
        assert!(
            level.objects.iter().any(|o| o.category == Category::Road),
            "no road web generated"
        );
        // Buildings are rotated (not axis-aligned).
        assert!(level.objects.iter().any(|o| o.category == Category::Structure
            && o.angle_deg.abs() > 1.0));
    }

    #[test]
    fn structures_keep_off_roads_but_props_may_stand_on_them() {
        let level = generate(&LevelConfig::default(), 11);
        let road_rects: Vec<_> = level
            .objects
            .iter()
            .filter(|o| o.category == Category::Road)
            .collect();
        assert!(!road_rects.is_empty());
        let corners = |o: &crate::level::PlacedObject| {
            geometry::rot_rect_corners(
                (o.x_m + o.w_m / 2.0, o.y_m + o.h_m / 2.0),
                o.w_m,
                o.h_m,
                o.angle_deg,
            )
        };
        for solid in level.objects.iter().filter(|o| {
            o.category == Category::Structure || o.category == Category::HiddenRoom
        }) {
            for road in &road_rects {
                assert!(
                    !geometry::rotated_rects_overlap(&corners(solid), &corners(road)),
                    "solid on road: {solid:?} vs {road:?}"
                );
            }
        }
    }

    #[test]
    fn walled_maps_stay_connected_across_seeds_and_mixes() {
        let mut mix = LevelConfig {
            template_mix: TemplateMix::Mix(TemplateKind::Town, TemplateKind::Forest),
            ..LevelConfig::default()
        };
        mix.gates.clear();
        for (name, config) in [("town", LevelConfig::default()), ("mix", mix)] {
            for seed in [1u64, 7, 42, 123, 999] {
                let level = generate(&config, seed);
                assert!(
                    level.notes.iter().all(|n| !n.contains("fragmented")),
                    "{name} seed {seed}: {:?}",
                    level.notes
                );
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
    fn gates_follow_the_walled_town_or_the_map_border() {
        // Walled town: gates sit on the blob wall, well inside the map.
        let level = generate(&LevelConfig::default(), 13);
        let (w, h) = (level.config.map.width_m, level.config.map.height_m);
        let center = (w / 2.0, h / 2.0);
        for g in level.objects.iter().filter(|o| o.category == Category::Gate) {
            let c = (g.x_m + g.w_m / 2.0, g.y_m + g.h_m / 2.0);
            let r = geometry::dist(c, center);
            assert!(
                r > 0.2 * w.min(h) && r < 0.75 * w.max(h),
                "gate not on the town wall: {g:?} (r = {r})"
            );
        }

        // Open map: gates stay on the map border as before.
        let mut open = LevelConfig {
            template_mix: TemplateMix::Single(TemplateKind::Forest),
            ..LevelConfig::default()
        };
        open.gates = vec![
            crate::model::Gate { direction_deg: 0.0, color: [200, 0, 0] },
            crate::model::Gate { direction_deg: 200.0, color: [0, 0, 200] },
        ];
        let level = generate(&open, 13);
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
