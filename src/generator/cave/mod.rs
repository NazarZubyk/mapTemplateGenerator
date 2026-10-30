//! The cave node chain (docs/cave-plan.md). Caves do not use the town
//! chain: rock is the default ground and the walkable space is carved out
//! of it. Nodes so far:
//!
//! 2. **Style** — which parts of the map are natural caverns and which are
//!    hand-made passages, as zones.
//! 3. **Chambers** — halls and grottos: blobs in natural zones,
//!    rectangular rooms in hand-made ones.
//! 4. **Tunnels** — the chambers linked from a chain to a maze (labyrinth
//!    slider), natural winding or hand-made straight, gate tunnels in from
//!    the edge.
//! 5. **Carve & smooth** — the final floor: natural walls roughened and
//!    smoothed (cellular automaton), hand-made ones as dug; the wall band.
//! 6. **Hazards** — chasms in halls (some with rope bridges), pools with a
//!    shallow edge and a deep core; the walkable floor stays one piece.
//! 7. **Spawns** (nodes 7–10) — props, hidden rooms (pockets in the rock
//!    behind a cracked wall), interactive objects and mobs on cave ground
//!    (the shared cores, cave rules).

use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};

use crate::generator::Progress;
use crate::level::{CaveLayout, GeneratedLevel};
use crate::model::LevelConfig;

pub mod carve;
pub mod chambers;
pub mod hazards;
pub mod spawns;
pub mod tunnels;
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

    progress("Digging tunnels", 0.5);
    let tunnels = tunnels::build(config, &mut rng, &style.layout, &chambers.layout);
    notes.extend(tunnels.notes);

    progress("Smoothing the walls", 0.7);
    let carve = carve::build(config, &mut rng, &style.layout, &chambers.layout, &tunnels.layout);
    notes.extend(carve.notes);

    progress("Opening chasms and pools", 0.85);
    let hazards = hazards::build(config, &mut rng, &chambers.layout, &tunnels.layout, &carve.layout);
    notes.extend(hazards.notes);

    let stage = |from, to| crate::generator::Stage { progress, from, to };
    let ground = spawns::CaveGround::new(&style.layout, &chambers.layout, &tunnels.layout, &carve.layout, &hazards.layout);

    progress("Placing props", 0.88);
    let props = spawns::props(config, &mut rng, &ground);
    notes.extend(props.notes);
    let mut objects = props.objects;

    // Hidden rooms before the interactive objects, so objects can go
    // inside them.
    let mut rooms = spawns::hidden_rooms(config, &mut rng, &ground, &[&props.layout.mask], &stage(0.9, 0.93));
    notes.extend(rooms.notes);
    let offset = objects.len();
    for r in &mut rooms.layout.rooms {
        for m in &mut r.objects {
            *m += offset;
        }
    }
    objects.extend(rooms.objects);

    let mut inter = spawns::interactive(config, &mut rng, &ground, &props.layout.mask, &rooms.layout, &stage(0.93, 0.96));
    notes.extend(inter.notes);
    let offset = objects.len();
    for g in &mut inter.layout.groups {
        for m in &mut g.members {
            *m += offset;
        }
    }
    objects.extend(inter.objects);

    // Gate markers last, so they draw on top.
    let mut tunnel_layout = tunnels.layout;
    for g in &mut tunnel_layout.gates {
        g.object += objects.len();
    }
    objects.extend(tunnels.objects);

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
        swamp: None,
        cave: Some(CaveLayout {
            style: Some(style.layout),
            chambers: Some(chambers.layout),
            tunnels: Some(tunnel_layout),
            carve: Some(carve.layout),
            hazards: Some(hazards.layout),
        }),
        forest: None,
        noise_preview: None,
        notes,
    };
    // Node 10: mobs, last (own random stream; "Re-roll mobs" reruns it).
    crate::generator::place_mobs(&mut level, progress);
    progress("Done", 1.0);
    level
}
