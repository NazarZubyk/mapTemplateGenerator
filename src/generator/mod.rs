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
pub mod region;
pub mod regions;
pub mod roads;
pub mod shape;
pub mod swamp;
pub mod blend;
pub mod cave;
pub mod forest;
pub mod mixed;
pub mod mountains;
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
    // Caves too (docs/cave-plan.md).
    if config.kind == crate::model::MapKind::Cave {
        return cave::generate_with_progress(config, seed, progress);
    }
    // Forests too (docs/forest-plan.md).
    if config.kind == crate::model::MapKind::Forest {
        return forest::generate_with_progress(config, seed, progress);
    }
    // Mixed maps: map regions of several kinds (docs/mixed-plan.md).
    if config.kind == crate::model::MapKind::Mixed {
        return mixed::generate_with_progress(config, seed, progress);
    }
    town_chain(config, seed, progress)
}

/// The town node chain (docs/workflow.md). Only towns use it from
/// [`generate_with_progress`]; the tests also run it on other kinds to
/// check it copes with island shapes and no houses.
pub(crate) fn town_chain(config: &LevelConfig, seed: u64, progress: Progress) -> GeneratedLevel {
    town_in(config, seed, None, progress)
}

/// The town chain — a whole map, or with `region` one map region of a
/// mixed map (docs/mixed-plan.md, M5): the town fills the region, its wall
/// along the region's edge; the portals and map gates landing in it are its
/// gates; the mixed map's rivers run through it, crossed on bridges (kept
/// clear like roads); no hidden room outside the wall (that is another
/// region).
pub fn town_in(config: &LevelConfig, seed: u64, region: Option<&region::RegionCtx>, progress: Progress) -> GeneratedLevel {
    let stage = |from, to| Stage { progress, from, to };
    let built = match region {
        Some(r) => shape::ShapeBuild {
            shape: LevelShape {
                // Two cells off the map edge: the wall's inside is found by
                // flooding from the map border, which needs room outside it.
                mask: {
                    let mut m = r.domain.clone();
                    let (w, h) = (m.width, m.height);
                    for (c, v) in m.cells.iter_mut().enumerate() {
                        let (x, y) = (c % w, c / w);
                        *v &= x >= 2 && y >= 2 && x + 2 < w && y + 2 < h;
                    }
                    m
                },
                stats: crate::level::ShapeStats {
                    raw_blobs: 1,
                    speckle_cells_removed: 0,
                    bridges: 0,
                    hole_cells_filled: 0,
                    sections: 1,
                    section_bridges: 0,
                    coverage_pct: 0.0,
                },
            },
            noise_preview: crate::level::NoiseTexture { width: 0, height: 0, cell_size_m: r.domain.cell_size_m, values: Vec::new() },
        },
        None => shape::build_with_progress(config, seed, &stage(0.0, 0.55)),
    };
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
                if let Some(r) = region {
                    region_mask = town_ground_in(r, region_mask);
                    // The wall is closed all along the region's edge: town
                    // floor that touches it (a gap where a thin wall steps
                    // diagonally) turns wall; the gates open it later.
                    if let Some(wall) = walls.as_mut() {
                        let (mw, mh) = (region_mask.width, region_mask.height);
                        for c in 0..region_mask.cells.len() {
                            if !region_mask.cells[c] {
                                continue;
                            }
                            // Side by side only: a corner touch is no way through.
                            let (x, y) = ((c % mw) as i64, (c / mw) as i64);
                            let at_edge = [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)].iter().any(|&(dx, dy)| {
                                let (nx, ny) = (x + dx, y + dy);
                                nx < 0 || ny < 0 || nx >= mw as i64 || ny >= mh as i64 || !r.inside(ny as usize * mw + nx as usize)
                            });
                            if at_edge {
                                region_mask.cells[c] = false;
                                wall.mask.cells[c] = true;
                            }
                        }
                        // That can cut a narrow neck: the town keeps its main piece.
                        let before = region_mask.count_true();
                        region_mask = town_ground_in(r, region_mask);
                        let lost = (before - region_mask.count_true()) as f32 * cell_area(&region_mask);
                        if lost >= 40.0 {
                            notes.push(format!("Town: {lost:.0} m² beyond a neck the closed wall cut off was left out"));
                        }
                    }
                }
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
    let mut region_walls = region_walls;
    let gates = regions.as_mut().map(|split| {
        gates::build(config, split, walls.as_mut(), region_walls.as_mut().map(|w| &mut w.mask), roads.as_mut(), region.map(|r| r.gates.as_slice()))
    });
    // A map region: its town ground and roads end at its edge (a gate
    // opening in the wall can reach past it); its bridges stay clear like
    // roads (nothing is built on them).
    if let (Some(r), Some(split)) = (region, regions.as_mut()) {
        for (c, id) in split.map.ids.iter_mut().enumerate() {
            if !r.inside(c) {
                *id = 0;
            }
        }
        if let Some(net) = roads.as_mut() {
            for c in 0..net.mask.cells.len() {
                net.mask.cells[c] = (net.mask.cells[c] && r.inside(c)) || (r.bridges.cells[c] && split.map.ids[c] != 0);
            }
        }
        // Specks of town floor the walls box in (a cell or two between a
        // wall and the edge) are no town: one walkable piece.
        let rw = region_walls.as_ref().map(|x| &x.mask);
        let floor = CellMask {
            width: split.map.width,
            height: split.map.height,
            cell_size_m: split.map.cell_size_m,
            cells: (0..split.map.ids.len()).map(|c| split.map.ids[c] != 0 && !rw.is_some_and(|m| m.cells[c])).collect(),
        };
        let (labels, count) = grid::components(&floor);
        if count > 1 {
            let mut size = vec![0usize; count as usize + 1];
            for &l in &labels {
                size[l as usize] += 1;
            }
            let main = (1..=count as usize).max_by_key(|&l| (size[l], std::cmp::Reverse(l))).unwrap_or(1);
            let speck = (4.0 / (split.map.cell_size_m * split.map.cell_size_m)).ceil() as usize;
            for (c, &l) in labels.iter().enumerate() {
                if l > 0 && l as usize != main && size[l as usize] <= speck {
                    split.map.ids[c] = 0;
                }
            }
        }
    }
    // Blending: embankments (quay walls) along the rivers inside the town,
    // open at the bridges (docs/blending-plan.md, B4) — before the houses,
    // so everything after keeps off them like off the wall.
    if let (Some(r), Some(split), Some(wall)) = (region, regions.as_ref(), walls.as_mut())
        && r.embankment_m > 0.0
    {
        let n = split.map.ids.len();
        let water: Vec<bool> = (0..n).map(|c| r.inside(c) && !r.bridges.cells[c] && (r.water.deep.cells[c] || r.water.shallow.cells[c])).collect();
        let gates: Vec<[f32; 2]> = r.gates.iter().map(|g| g.point_m).collect();
        let ground = blend::EmbankmentGround {
            ids: &split.map.ids,
            water: &water,
            roads: roads.as_ref().map(|n| &n.mask),
            region_walls: region_walls.as_ref().map(|w| &w.mask),
            bridges: &r.bridges,
            gates: &gates,
        };
        wall.stats.embankment_cells = blend::embankments(&ground, &mut wall.mask, r.embankment_m);
    }

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

    // Blending: trees in the yards near a forest region, on the yard ground
    // the houses, props, interactive objects and hidden rooms left (they
    // never take a place something asked for); the mobs keep off them (the
    // trunks join the house mask). No draws from the chain's stream:
    // hashes of the blending seed.
    let mut houses = houses;
    let mut props = props;
    // The yard ground nothing took, for the blending's trees and boulders.
    // (Called only with the regions built; houses and props passed in, as
    // the blending adds to them.)
    let yard_ground = |houses: Option<&CellMask>, props: Option<&CellMask>| {
        let split = regions.as_ref().expect("regions built");
        let n = split.map.ids.len();
        CellMask {
            width: split.map.width,
            height: split.map.height,
            cell_size_m: split.map.cell_size_m,
            cells: (0..n)
                .map(|c| {
                    split.map.ids[c] != 0
                        && !walls.as_ref().is_some_and(|w| w.mask.cells[c])
                        && !region_walls.as_ref().is_some_and(|w| w.mask.cells[c])
                        && !roads.as_ref().is_some_and(|w| w.mask.cells[c])
                        && !houses.is_some_and(|m| m.cells[c])
                        && !props.is_some_and(|m| m.cells[c])
                        && !interactive.as_ref().is_some_and(|w| w.mask.cells[c])
                        && !hidden_rooms.as_ref().is_some_and(|w| w.floor_mask.cells[c] || w.wall_mask.cells[c])
                })
                .collect(),
        }
    };
    if let (Some(r), Some(_)) = (region, regions.as_ref())
        && let (Some(t), Some(trees)) = (r.blend_toward(crate::model::MapKind::Forest), r.neighbour_trees.as_ref())
    {
        let free = yard_ground(houses.as_ref().map(|l| &l.mask), props.as_ref().map(|l| &l.mask));
        let built = blend::yard_trees(t, trees, &free, &r.gates, r.blend_seed ^ r.id as u64);
        if !built.objects.is_empty() {
            let layout = houses.get_or_insert_with(|| crate::level::HouseLayout {
                mask: CellMask::new(free.width, free.height, free.cell_size_m),
                street_houses: 0,
                yard_houses: 0,
                complex_houses: 0,
                coverage_pct: 0.0,
                target_pct: 0.0,
                yard_trees: 0,
            });
            for (m, &tr) in layout.mask.cells.iter_mut().zip(&built.mask.cells) {
                *m |= tr;
            }
            layout.yard_trees = built.objects.len() as u32;
            objects.extend(built.objects);
        }
    }
    // Blending: boulders fallen from a neighbouring massif (scree, B3) on
    // the yard ground left after the trees (the town wall stands; the
    // rocks lie inside it). Never on a road; a boulder's ring may touch
    // one. They join the props mask, so the mobs keep off them.
    if let (Some(r), Some(_)) = (region, regions.as_ref())
        && let Some(t) = r.blend_toward(crate::model::MapKind::Cave)
    {
        let free = yard_ground(houses.as_ref().map(|l| &l.mask), props.as_ref().map(|l| &l.mask));
        let n = free.cells.len();
        let road = |c: usize| roads.as_ref().is_some_and(|w| w.mask.cells[c]);
        let place: Vec<bool> = free.cells.clone();
        let walkable: Vec<bool> = (0..n).map(|c| free.cells[c] || road(c)).collect();
        let taken: Vec<bool> = (0..n).map(|c| !walkable[c] || houses.as_ref().is_some_and(|h| h.mask.cells[c])).collect();
        let gates: Vec<[f32; 2]> = r.gates.iter().map(|g| g.point_m).collect();
        let sg = blend::ScreeGround { w: free.width, h: free.height, cell: free.cell_size_m, place: &place, walkable: &walkable, taken: &taken, gates: &gates };
        let built = blend::scree(t, &sg, config.props.color, r.blend_seed ^ r.id as u64 ^ 0x5C);
        if !built.objects.is_empty() {
            let layout = props.get_or_insert_with(|| crate::level::PropLayout {
                mask: CellMask::new(free.width, free.height, free.cell_size_m),
                near_houses: 0,
                roadside: 0,
                scattered: 0,
                near_trees: 0,
                shore: 0,
                in_water: 0,
                path_edge: 0,
                near_walls: 0,
                in_tunnels: 0,
                pool_edge: 0,
                scree: 0,
                requested: 0,
                density_cap: 0,
            });
            for (m, &b) in layout.mask.cells.iter_mut().zip(&built.mask.cells) {
                *m |= b;
            }
            layout.scree = built.objects.len() as u32;
            objects.extend(built.objects);
        }
    }

    let mut gates = gates;
    if let Some(layout) = gates.as_mut() {
        let offset = objects.len();
        for g in layout.gates.iter_mut().filter(|g| g.object != usize::MAX) {
            g.object += offset;
        }
        objects.extend(gate_objects);
    }

    let total = region_mask.width * region_mask.height;
    let mut stats = built.shape.stats;
    stats.coverage_pct = region_mask.count_true() as f32 / total.max(1) as f32 * 100.0;

    let mut level = GeneratedLevel {
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
        mobs: None,
        gates,
        swamp: None,
        cave: None,
        forest: None,
        mixed: None,
        noise_preview: region.is_none().then_some(built.noise_preview),
        notes,
    };
    // Step 11: mobs (every map kind), last.
    place_mobs(&mut level, progress);
    progress("Done", 1.0);
    level
}

fn cell_area(m: &CellMask) -> f32 {
    m.cell_size_m * m.cell_size_m
}

/// A town region's ground inside its wall: the mixed map's water is no
/// town ground, except on the bridges; pieces the water cuts off (too small
/// for a crossing) are left out, so the town is one piece.
fn town_ground_in(r: &region::RegionCtx, inside_wall: CellMask) -> CellMask {
    let mut m = inside_wall;
    for c in 0..m.cells.len() {
        let wet = r.water.deep.cells[c] || r.water.shallow.cells[c];
        m.cells[c] &= !wet || r.bridges.cells[c];
    }
    let (labels, count) = grid::components(&m);
    if count > 1 {
        let mut size = vec![0usize; count as usize + 1];
        for &l in &labels {
            size[l as usize] += 1;
        }
        let main = (1..=count as usize).max_by_key(|&l| (size[l], std::cmp::Reverse(l))).unwrap_or(1);
        for (c, v) in m.cells.iter_mut().enumerate() {
            *v &= labels[c] as usize == main;
        }
    }
    m
}

/// Mobs, the last node of every chain: (re)places the level's mobs from its
/// config. Old mob markers and "Mobs:" notes are dropped; new markers go
/// before the gate markers (which draw on top). The mobs have their own
/// random stream (level seed + `mobs.reroll`), so this can run again on a
/// finished level — "Re-roll mobs" — and a full generation with the same
/// config gives the same mobs.
pub fn place_mobs(level: &mut GeneratedLevel, progress: Progress) {
    let stage = Stage { progress, from: 0.99, to: 0.995 };
    if let Some(old) = level.mobs.take() {
        let gone: std::collections::HashSet<usize> = old.spawns.iter().flat_map(|sp| sp.objects.iter().copied()).collect();
        let mut map = vec![usize::MAX; level.objects.len()];
        let mut kept = Vec::with_capacity(level.objects.len());
        for (i, o) in std::mem::take(&mut level.objects).into_iter().enumerate() {
            if !gone.contains(&i) {
                map[i] = kept.len();
                kept.push(o);
            }
        }
        level.objects = kept;
        remap_objects(level, |i| map[i]);
        level.notes.retain(|n| !n.starts_with("Mobs:"));
    }
    let mut rng = ChaCha8Rng::seed_from_u64(
        level.seed ^ 0x6D6F_6273_0000_0000 ^ level.config.mobs.reroll.wrapping_mul(0x9E37_79B9_7F4A_7C15),
    );
    let config = &level.config;
    let built = if config.kind == crate::model::MapKind::Cave {
        level.cave.as_ref().and_then(|cv| {
            let ground = cave::spawns::CaveGround::new(
                cv.style.as_ref()?,
                cv.chambers.as_ref()?,
                cv.tunnels.as_ref()?,
                cv.carve.as_ref()?,
                cv.hazards.as_ref()?,
            );
            let placed: Vec<&CellMask> =
                [level.props.as_ref().map(|l| &l.mask), level.interactive.as_ref().map(|l| &l.mask)].into_iter().flatten().collect();
            Some(cave::spawns::mobs(config, &mut rng, &ground, &placed, level.hidden_rooms.as_ref(), &stage))
        })
    } else if config.kind == crate::model::MapKind::Forest && level.forest.is_some() {
        level.forest.as_ref().and_then(|f| {
            let ground = forest::spawns::ForestSpawnGround::new(f)?;
            let placed: Vec<&CellMask> = level.interactive.as_ref().map(|l| &l.mask).into_iter().collect();
            Some(forest::spawns::mobs(config, &mut rng, &ground, &placed, level.hidden_rooms.as_ref(), &stage))
        })
    } else if config.kind == crate::model::MapKind::Swamp {
        level.swamp.as_ref().and_then(|s| {
            let ground = swamp::spawns::SwampGround::new(
                config,
                &s.gate_anchors,
                s.islands.as_ref()?,
                s.paths.as_ref()?,
                s.depth.as_ref()?,
                s.trees.as_ref()?,
            );
            let placed: Vec<&CellMask> =
                [level.props.as_ref().map(|l| &l.mask), level.interactive.as_ref().map(|l| &l.mask)].into_iter().flatten().collect();
            Some(swamp::spawns::mobs(config, &mut rng, &ground, &placed, level.hidden_rooms.as_ref(), &stage))
        })
    } else {
        level.regions.as_ref().and_then(|split| {
            let object_masks: Vec<&CellMask> = [
                level.houses.as_ref().map(|l| &l.mask),
                level.props.as_ref().map(|l| &l.mask),
                level.interactive.as_ref().map(|l| &l.mask),
            ]
            .into_iter()
            .flatten()
            .collect();
            let ground = mobs::Ground {
                regions: &split.map,
                outer_wall: level.walls.as_ref().map(|w| &w.mask),
                region_walls: level.region_walls.as_ref().map(|w| &w.mask),
                roads: level.roads.as_ref(),
                objects: &object_masks,
                hidden_rooms: level.hidden_rooms.as_ref(),
                gates: level.gates.as_ref().map(|g| g.gates.as_slice()).unwrap_or(&[]),
            };
            mobs::build_with_progress(config, &mut rng, &ground, &stage)
        })
    };
    let Some(mut built) = built else { return };
    // Insert before the gate markers (always the last objects).
    let at = level.objects.iter().position(|o| o.category == crate::level::Category::Gate).unwrap_or(level.objects.len());
    let n = built.objects.len();
    remap_objects(level, |i| if i >= at { i + n } else { i });
    for sp in &mut built.layout.spawns {
        for o in &mut sp.objects {
            *o += at;
        }
    }
    level.objects.splice(at..at, built.objects);
    level.notes.extend(built.notes);
    level.mobs = Some(built.layout);
}

/// Apply `f` to every stored index into `level.objects` (mobs excluded).
fn remap_objects(level: &mut GeneratedLevel, f: impl Fn(usize) -> usize) {
    if let Some(l) = level.interactive.as_mut() {
        for g in &mut l.groups {
            for m in &mut g.members {
                *m = f(*m);
            }
        }
    }
    if let Some(l) = level.hidden_rooms.as_mut() {
        for r in &mut l.rooms {
            for m in &mut r.objects {
                *m = f(*m);
            }
        }
    }
    if let Some(l) = level.gates.as_mut() {
        for g in l.gates.iter_mut().filter(|g| g.object != usize::MAX) {
            g.object = f(g.object);
        }
    }
    if let Some(s) = level.swamp.as_mut() {
        for a in s.gate_anchors.iter_mut().filter(|a| a.object != usize::MAX) {
            a.object = f(a.object);
        }
    }
    if let Some(t) = level.cave.as_mut().and_then(|c| c.tunnels.as_mut()) {
        for g in t.gates.iter_mut().filter(|g| g.object != usize::MAX) {
            g.object = f(g.object);
        }
    }
    if let Some(c) = level.mixed.as_mut().and_then(|m| m.connections.as_mut()) {
        for g in &mut c.gates {
            g.object = f(g.object);
        }
    }
    if let Some(forest) = level.forest.as_mut() {
        if let Some(p) = forest.passages.as_mut() {
            for g in p.gates.iter_mut().filter(|g| g.object != usize::MAX) {
                g.object = f(g.object);
            }
        }
        if let Some(p) = forest.props.as_mut() {
            for g in &mut p.groups {
                for i in &mut g.objects {
                    *i = f(*i);
                }
            }
        }
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
