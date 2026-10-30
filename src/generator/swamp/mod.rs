//! The swamp node chain (docs/swamp-plan.md). Swamps do not use the town
//! chain (shape → walls → regions → roads → ...): water is the default
//! ground and firm land the exception, so the chain is built around the
//! islands. Nodes so far:
//!
//! 2. **Gate anchors** — where each gate meets the map border.
//! 3. **Islands** — the firm ground: gate islands, hubs, small islands.
//! 4. **Paths** — mud paths and boardwalks linking the islands, bridges out
//!    to the bridge gates.

use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};

use crate::generator::Progress;
use crate::level::{GeneratedLevel, SwampLayout};
use crate::model::LevelConfig;

pub mod gate_anchors;
pub mod islands;
pub mod paths;

/// Generate a swamp level: one RNG stream, nodes in fixed order.
pub fn generate_with_progress(config: &LevelConfig, seed: u64, progress: Progress) -> GeneratedLevel {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut notes = Vec::new();

    progress("Placing gate anchors", 0.0);
    let anchors = gate_anchors::build(config, &mut rng);
    notes.extend(anchors.notes);
    let objects = anchors.objects;

    progress("Raising islands", 0.05);
    let islands = islands::build(config, &mut rng, &anchors.anchors);
    notes.extend(islands.notes);

    progress("Laying paths", 0.3);
    let paths = paths::build(config, &mut rng, &anchors.anchors, &islands.layout);
    notes.extend(paths.notes);

    progress("Done", 1.0);
    GeneratedLevel {
        config: config.clone(),
        seed,
        objects,
        shape: None,
        walls: None,
        regions: None,
        roads: None,
        region_walls: None,
        houses: None,
        props: None,
        interactive: None,
        hidden_rooms: None,
        mobs: None,
        gates: None,
        swamp: Some(SwampLayout { gate_anchors: anchors.anchors, islands: Some(islands.layout), paths: Some(paths.layout) }),
        noise_preview: None,
        notes,
    }
}
