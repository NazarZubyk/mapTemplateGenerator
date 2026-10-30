//! The swamp node chain (docs/swamp-plan.md). Swamps do not use the town
//! chain (shape → walls → regions → roads → ...): water is the default
//! ground and firm land the exception, so the chain is built around the
//! islands. Nodes so far:
//!
//! 2. **Gate anchors** — where each gate meets the map border.
//! 3. **Islands** — the firm ground: gate islands, hubs, small islands.
//! 4. **Paths** — mud paths and boardwalks linking the islands, bridges out
//!    to the bridge gates.
//! 5. **Depth & border** — shallow water, deep mud and deep water by
//!    distance, hummocks so every crossing fits in one breath, the deep
//!    water / thicket border.
//! 6. **Trees** — groves on the islands, single trees in the water.
//! 7. **Spawns** (nodes 7–10) — props, interactive objects, hidden islets
//!    and mobs on swamp ground (the town nodes' cores, swamp rules).

use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};

use crate::generator::Progress;
use crate::level::{GeneratedLevel, SwampLayout};
use crate::model::LevelConfig;

pub mod depth;
pub mod gate_anchors;
pub mod islands;
pub mod paths;
pub mod spawns;
pub mod trees;

/// Generate a swamp level: one RNG stream, nodes in fixed order.
pub fn generate_with_progress(config: &LevelConfig, seed: u64, progress: Progress) -> GeneratedLevel {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut notes = Vec::new();

    progress("Placing gate anchors", 0.0);
    let anchors = gate_anchors::build(config, &mut rng);
    notes.extend(anchors.notes);

    progress("Raising islands", 0.05);
    let mut islands = islands::build(config, &mut rng, &anchors.anchors);
    notes.extend(islands.notes);

    progress("Laying paths", 0.3);
    let mut paths = paths::build(config, &mut rng, &anchors.anchors, &islands.layout);
    notes.extend(paths.notes);

    progress("Filling the water", 0.7);
    let depth = depth::build(config, &mut rng, &mut islands.layout, &mut paths.layout);
    notes.extend(depth.notes);

    progress("Planting trees", 0.85);
    let trees = trees::build(config, &mut rng, &anchors.anchors, &islands.layout, &depth.layout);
    notes.extend(trees.notes);

    let mut objects = trees.objects;
    let stage = |from, to| crate::generator::Stage { progress, from, to };
    let ground = spawns::SwampGround::new(config, &anchors.anchors, &islands.layout, &paths.layout, &depth.layout, &trees.layout);

    progress("Placing props", 0.9);
    let props = spawns::props(config, &mut rng, &ground);
    notes.extend(props.notes);
    objects.extend(props.objects);

    let mut inter = spawns::interactive(config, &mut rng, &ground, &props.layout.mask, &stage(0.92, 0.94));
    notes.extend(inter.notes);
    let offset = objects.len();
    for g in &mut inter.layout.groups {
        for m in &mut g.members {
            *m += offset;
        }
    }
    objects.extend(inter.objects);

    let placed = [&props.layout.mask, &inter.layout.mask];
    let mut rooms = spawns::hidden_rooms(config, &mut rng, &ground, &placed, &stage(0.94, 0.96));
    notes.extend(rooms.notes);
    let offset = objects.len();
    for r in &mut rooms.layout.rooms {
        for m in &mut r.objects {
            *m += offset;
        }
    }
    objects.extend(rooms.objects);

    // Gate markers last, so they draw on top.
    let mut gate_anchors = anchors.anchors;
    for a in &mut gate_anchors {
        a.object += objects.len();
    }
    objects.extend(anchors.objects);

    let mut level = GeneratedLevel {
        config: config.clone(),
        seed,
        objects,
        shape: None,
        walls: None,
        regions: None,
        roads: None,
        region_walls: None,
        houses: None,
        props: Some(props.layout),
        interactive: Some(inter.layout),
        hidden_rooms: Some(rooms.layout),
        mobs: None,
        gates: None,
        swamp: Some(SwampLayout { gate_anchors, islands: Some(islands.layout), paths: Some(paths.layout), depth: Some(depth.layout), trees: Some(trees.layout) }),
        noise_preview: None,
        notes,
    };
    // Node 10: mobs, last (own random stream; "Re-roll mobs" reruns it).
    crate::generator::place_mobs(&mut level, progress);
    progress("Done", 1.0);
    level
}
