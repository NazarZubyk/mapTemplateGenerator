//! Generation shell. The generator is being rebuilt section by section under the
//! "Current workflow" panel (see docs/workflow.md). The previous monolithic
//! implementation is preserved under `reference/` and will be deleted once the
//! new build replaces it.

use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};

use crate::level::{CellMask, GeneratedLevel, LevelShape};
use crate::model::LevelConfig;

pub mod gates;
pub mod grid;
pub mod hidden_rooms;
pub mod houses;
pub mod interactive;
pub mod mobs;
pub mod noise;
pub mod placement;
pub mod props;
pub mod region_walls;
pub mod regions;
pub mod roads;
pub mod shape;
pub mod swamp;
pub mod walls;

/// Progress callback: (stage label, overall fraction 0..1). Called from the
/// generating thread; must be cheap.
pub type Progress<'a> = &'a dyn Fn(&'static str, f32);

/// A sub-range of the overall progress: phase-local fractions 0..1 are
/// mapped into `from..to` of the whole generation.
pub(crate) struct Stage<'a> {
    pub progress: Progress<'a>,
    pub from: f32,
    pub to: f32,
}

impl Stage<'_> {
    pub fn report(&self, label: &'static str, local: f32) {
        (self.progress)(label, self.from + (self.to - self.from) * local.clamp(0.0, 1.0));
    }
}

/// `generate_with_progress` without progress reporting.
pub fn generate(config: &LevelConfig, seed: u64) -> GeneratedLevel {
    generate_with_progress(config, seed, &|_, _| {})
}

/// Generate a level deterministically: same (config, seed) always yields the
/// same result. `progress` is told the current stage as generation runs. Currently implemented: step 1 (the noise-derived region
/// shape), step 2 (the outer wall ring) and step 3 (the region split);
/// phases run in fixed order and share the one RNG stream (docs/workflow.md).
pub fn generate_with_progress(
    config: &LevelConfig,
    seed: u64,
    progress: Progress,
) -> GeneratedLevel {
    // Swamps have their own node chain (docs/swamp-plan.md).
    if config.kind == crate::model::MapKind::Swamp {
        return swamp::generate_with_progress(config, seed, progress);
    }
    let stage = |from, to| Stage { progress, from, to };
    let built = shape::build_with_progress(config, seed, &stage(0.0, 0.55));
    let mut notes = Vec::new();
    if built.shape.mask.count_true() == 0 {
        notes.push(format!(
            "{} shape came out empty — lower the coverage target or the min blob size",
            config.kind.label()
        ));
    }

    // One RNG stream for every placement phase, in fixed order.
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    // Step 2: outer wall. Redefines the region as the wall's interior.
    let mut region_mask = built.shape.mask;
    let mut walls = None;
    if region_mask.count_true() > 0 {
        stage(0.55, 0.65).report("Building outer wall", 0.0);
        match walls::build(config, &mut rng, &region_mask) {
            Some(w) => {
                if w.wall.stats.extra_loops > 0 {
                    notes.push(format!(
                        "Outer wall: {} extra boundary loop(s) discarded (border pinch \
                         points)",
                        w.wall.stats.extra_loops
                    ));
                }
                region_mask = w.region;
                walls = Some(w.wall);
            }
            None => notes.push(
                "Outer wall skipped: the region is too small for these segment lengths"
                    .to_owned(),
            ),
        }
    }

    // Step 3: split the town area into regions (smallest placed first).
    let mut regions = if region_mask.count_true() > 0 {
        regions::build_with_progress(config, &mut rng, &region_mask, &stage(0.65, 0.85))
    } else {
        None
    };

    // Step 4: the road network — crossroad nodes (≥ 1 per region) and the
    // winding links between them; region crossings become passages.
    let mut roads = regions.as_ref().and_then(|split| {
        let built = roads::build_with_progress(config, &mut rng, split, &stage(0.85, 0.9))?;
        notes.extend(built.notes);
        Some(built.network)
    });

    // Step 5: walls along the borders between regions, opened where roads
    // cross them.
    // The regions are refit so each one ends exactly at its walls.
    let region_walls = regions.as_mut().and_then(|split| {
        region_walls::build_with_progress(config, &mut rng, split, roads.as_ref(), &stage(0.9, 0.93))
    });

    // Step 12 (runs here, before the objects): gates on the outer wall —
    // openings plus a road into the network, so houses, props and the rest
    // keep off the gate roads and mobs keep away from the gates. Markers
    // are appended last so they draw on top.
    let gates = regions.as_mut().map(|split| {
        gates::build(config, split, walls.as_mut(), region_walls.as_ref().map(|w| &w.mask), roads.as_mut())
    });
    let (gates, gate_objects) = match gates {
        Some(built) => {
            notes.extend(built.notes);
            (Some(built.layout), built.objects)
        }
        None => (None, Vec::new()),
    };

    // Step 6: houses (town maps) — street fronts first, then the yards.
    let mut objects = Vec::new();
    let houses = match (config.kind, regions.as_ref()) {
        (crate::model::MapKind::Town, Some(split)) => {
            let ground = houses::Ground {
                regions: &split.map,
                outer_wall: walls.as_ref().map(|w| &w.mask),
                region_walls: region_walls.as_ref().map(|w| &w.mask),
                roads: roads.as_ref(),
            };
            houses::build_with_progress(config, &mut rng, &ground, &stage(0.93, 0.96)).map(|built| {
                notes.extend(built.notes);
                objects.extend(built.objects);
                built.layout
            })
        }
        _ => None,
    };

    // Step 8: props (every map kind) — against houses, at road edges,
    // scattered.
    let props = regions.as_ref().and_then(|split| {
        let ground = props::Ground {
            regions: &split.map,
            outer_wall: walls.as_ref().map(|w| &w.mask),
            region_walls: region_walls.as_ref().map(|w| &w.mask),
            roads: roads.as_ref(),
            house_mask: houses.as_ref().map(|h| &h.mask),
            houses: &objects,
        };
        let built = props::build_with_progress(config, &mut rng, &ground, &stage(0.96, 0.98))?;
        notes.extend(built.notes);
        Some((built.layout, built.objects))
    });
    let props = props.map(|(layout, placed)| {
        objects.extend(placed);
        layout
    });

    // Step 9: interactive objects (every map kind) — groups per type.
    let interactive = regions.as_ref().and_then(|split| {
        let ground = interactive::Ground {
            regions: &split.map,
            outer_wall: walls.as_ref().map(|w| &w.mask),
            region_walls: region_walls.as_ref().map(|w| &w.mask),
            roads: roads.as_ref(),
            house_mask: houses.as_ref().map(|h| &h.mask),
            prop_mask: props.as_ref().map(|p| &p.mask),
        };
        interactive::build_with_progress(config, &mut rng, &ground, &stage(0.98, 0.985))
    });
    let interactive = interactive.map(|mut built| {
        notes.extend(built.notes);
        // Group members index into the level's object list.
        let offset = objects.len();
        for g in &mut built.layout.groups {
            for m in &mut g.members {
                *m += offset;
            }
        }
        objects.extend(built.objects);
        built.layout
    });

    // Step 10: hidden rooms (every map kind) — walled rooms outside the wall
    // or inside, each with one narrow access path.
    let hidden_rooms = regions.as_ref().and_then(|split| {
        let object_masks: Vec<&CellMask> = [
            houses.as_ref().map(|l| &l.mask),
            props.as_ref().map(|l| &l.mask),
            interactive.as_ref().map(|l| &l.mask),
        ]
        .into_iter()
        .flatten()
        .collect();
        let gate_points: Vec<[f32; 2]> =
            gates.as_ref().map(|g| g.gates.iter().map(|g| g.point_m).collect()).unwrap_or_default();
        let ground = hidden_rooms::Ground {
            regions: &split.map,
            outer_wall: walls.as_ref().map(|w| &w.mask),
            region_walls: region_walls.as_ref().map(|w| &w.mask),
            roads: roads.as_ref(),
            objects: &object_masks,
            gates: &gate_points,
        };
        hidden_rooms::build_with_progress(config, &mut rng, &ground, &stage(0.985, 0.99))
    });
    let hidden_rooms = hidden_rooms.map(|mut built| {
        notes.extend(built.notes);
        let offset = objects.len();
        for r in &mut built.layout.rooms {
            for m in &mut r.objects {
                *m += offset;
            }
        }
        objects.extend(built.objects);
        // Paths tunnel through the outer wall: open it there.
        if let Some(w) = walls.as_mut() {
            for (cell, &dug) in w.mask.cells.iter_mut().zip(&built.layout.floor_mask.cells) {
                *cell &= !dug;
            }
        }
        built.layout
    });

    // Step 11: mobs (every map kind) — packs bought from the budget, on
    // walkable ground (roads too, if allowed).
    let mobs = regions.as_ref().and_then(|split| {
        let object_masks: Vec<&CellMask> = [
            houses.as_ref().map(|l| &l.mask),
            props.as_ref().map(|l| &l.mask),
            interactive.as_ref().map(|l| &l.mask),
        ]
        .into_iter()
        .flatten()
        .collect();
        let ground = mobs::Ground {
            regions: &split.map,
            outer_wall: walls.as_ref().map(|w| &w.mask),
            region_walls: region_walls.as_ref().map(|w| &w.mask),
            roads: roads.as_ref(),
            objects: &object_masks,
            hidden_rooms: hidden_rooms.as_ref(),
            gates: gates.as_ref().map(|g| g.gates.as_slice()).unwrap_or(&[]),
        };
        mobs::build_with_progress(config, &mut rng, &ground, &stage(0.99, 0.995))
    });
    let mobs = mobs.map(|mut built| {
        notes.extend(built.notes);
        let offset = objects.len();
        for sp in &mut built.layout.spawns {
            for o in &mut sp.objects {
                *o += offset;
            }
        }
        objects.extend(built.objects);
        built.layout
    });

    let mut gates = gates;
    if let Some(layout) = gates.as_mut() {
        let offset = objects.len();
        for g in &mut layout.gates {
            g.object += offset;
        }
        objects.extend(gate_objects);
    }

    let total = region_mask.width * region_mask.height;
    let mut stats = built.shape.stats;
    stats.coverage_pct = region_mask.count_true() as f32 / total.max(1) as f32 * 100.0;

    progress("Done", 1.0);
    GeneratedLevel {
        config: config.clone(),
        seed,
        objects,
        shape: Some(LevelShape {
            mask: region_mask,
            stats,
        }),
        walls,
        regions,
        roads,
        region_walls,
        houses,
        props,
        interactive,
        hidden_rooms,
        mobs,
        gates,
        swamp: None,
        noise_preview: Some(built.noise_preview),
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_only_moves_forward_and_ends_at_one() {
        let seen = std::cell::RefCell::new(Vec::new());
        let mut config = LevelConfig::test_64();
        config.shape.sections = 3;
        let level = generate_with_progress(&config, 7, &|label, f| seen.borrow_mut().push((label, f)));
        let seen = seen.into_inner();
        assert!(seen.len() > 5, "too few progress reports: {}", seen.len());
        for pair in seen.windows(2) {
            assert!(pair[1].1 >= pair[0].1, "progress went back: {pair:?}");
        }
        assert_eq!(seen.last().copied(), Some(("Done", 1.0)));
        // Reporting must not change the result.
        assert_eq!(level, generate(&config, 7));
    }

    #[test]
    fn same_seed_same_result() {
        let config = LevelConfig::test_64();
        assert_eq!(generate(&config, 42), generate(&config, 42));
        let other = LevelConfig {
            map: crate::model::MapSettings {
                width_m: 32.0,
                ..crate::model::MapSettings::default()
            },
            ..LevelConfig::test_64()
        };
        assert_ne!(generate(&config, 42), generate(&other, 42));
    }
}
