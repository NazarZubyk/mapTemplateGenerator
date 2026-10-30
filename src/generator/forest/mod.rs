//! The forest node chain (docs/forest-plan.md). Forests do not use the
//! town chain: the map is split into a few large areas joined at a few
//! crossings, so it is roomy inside an area but not flat. Nodes so far:
//!
//! 2. **Areas** — the map split into large areas, each wild or managed and
//!    with its own character (meadow, dense woods, logged, ...).

use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};

use crate::generator::Progress;
use crate::level::{ForestLayout, GeneratedLevel};
use crate::model::LevelConfig;

pub mod areas;

/// Generate a forest level: one RNG stream, nodes in fixed order.
pub fn generate_with_progress(config: &LevelConfig, seed: u64, progress: Progress) -> GeneratedLevel {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut notes = Vec::new();

    progress("Splitting the forest into areas", 0.0);
    let areas = areas::build(config, &mut rng);
    notes.extend(areas.notes);

    progress("Done", 1.0);
    GeneratedLevel {
        config: config.clone(),
        seed,
        objects: Vec::new(),
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
        swamp: None,
        cave: None,
        forest: Some(ForestLayout { areas: Some(areas.layout) }),
        noise_preview: None,
        notes,
    }
}
