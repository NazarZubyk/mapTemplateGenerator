//! The cave node chain (docs/cave-plan.md). Caves do not use the town
//! chain: rock is the default ground and the walkable space is carved out
//! of it. Nodes so far:
//!
//! 2. **Style** — which parts of the map are natural caverns and which are
//!    hand-made passages, as zones.
//! 3. **Chambers** — halls and grottos: blobs in natural zones,
//!    rectangular rooms in hand-made ones. Not linked yet.

use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};

use crate::generator::Progress;
use crate::level::{CaveLayout, GeneratedLevel};
use crate::model::LevelConfig;

pub mod chambers;
pub mod style;

/// Generate a cave level: one RNG stream, nodes in fixed order.
pub fn generate_with_progress(config: &LevelConfig, seed: u64, progress: Progress) -> GeneratedLevel {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut notes = Vec::new();

    progress("Laying out cave styles", 0.0);
    let style = style::build(config, &mut rng);
    notes.extend(style.notes);

    progress("Carving chambers", 0.3);
    let chambers = chambers::build(config, &mut rng, &style.layout);
    notes.extend(chambers.notes);

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
        cave: Some(CaveLayout { style: Some(style.layout), chambers: Some(chambers.layout) }),
        noise_preview: None,
        notes,
    }
}
