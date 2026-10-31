//! The forest node chain (docs/forest-plan.md). Forests do not use the
//! town chain. The land comes first and everything follows from it, the
//! way a real forest does:
//!
//! 2. **Areas** — the gameplay skeleton: a few large areas, each wild or
//!    managed and with its own character (meadow, dense woods, logged, ...).
//! 3. **Terrain** — a hidden height field (hills, a tilt) and rock ridges.
//! 4. **Water** — streams flow downhill from springs off the map, brooks
//!    join them; ponds fill hollows.
//! 5. **Thickets** — open ground / forest floor / thicket from the area
//!    character, belts along the area borders and the map edge, moisture.
//! 6. **Passages** — which areas link (labyrinth); unlinked borders are
//!    sealed, linked ones open where the barrier is weakest (trail, pass,
//!    ford, bridge); gate trails.
//! 7. **Clearings & trails** — glades in the areas (organic in wild, cut in
//!    managed areas) and trails linking the gates, crossings and glades
//!    (footpaths in wild, cart tracks in managed areas).
//! 8. **Trees** — tree trunks (round solid objects) on the walkable ground,
//!    by density, groves and the area character.
//! 9. **Props** — uninteractive props, shape and place only (groups along a
//!    direction, singles, near the rock, in managed areas); all blocked.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};

use crate::generator::Progress;
use crate::level::{ForestLayout, GeneratedLevel};
use crate::model::LevelConfig;

pub mod areas;
pub mod clearings;
pub mod passages;
pub mod props;
pub mod terrain;
pub mod water;
pub mod thickets;
pub mod trees;

/// Generate a forest level: one RNG stream, nodes in fixed order.
pub fn generate_with_progress(config: &LevelConfig, seed: u64, progress: Progress) -> GeneratedLevel {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut notes = Vec::new();

    progress("Splitting the forest into areas", 0.0);
    let areas = areas::build(config, &mut rng);
    notes.extend(areas.notes);

    progress("Shaping the land", 0.2);
    let terrain = terrain::build(config, &mut rng, &areas.layout);

    progress("Letting the water flow", 0.35);
    let water = water::build(config, &mut rng, &terrain);
    notes.extend(water.notes);

    progress("Growing the thickets", 0.5);
    let thickets = thickets::build(config, &mut rng, &areas.layout, &terrain, &water.layout);

    progress("Opening the passages", 0.65);
    let passages = passages::build(config, &mut rng, &areas.layout, &thickets);
    notes.extend(passages.notes);

    progress("Clearing glades and trails", 0.8);
    let clearings = clearings::build(config, &mut rng, &areas.layout, &passages.layout);

    progress("Planting the trees", 0.9);
    let mut trees = trees::build(config, &mut rng, &areas.layout, &thickets, &passages.layout, &clearings);

    progress("Placing the props", 0.95);
    let mut props = props::build(config, &mut rng, &areas.layout, &terrain, &passages.layout, &clearings, &mut trees.objects);
    // Trunks cleared for the prop groups: count them out.
    for (n, f) in trees.layout.per_area.iter_mut().zip(&props.cleared) {
        *n -= f;
    }
    trees.layout.total = trees.objects.len() as u32;
    let mut objects = passages.objects;
    // Gate markers keep their indexes; trunks and props come after them.
    objects.extend(trees.objects);
    let base = objects.len();
    for g in &mut props.layout.groups {
        for i in &mut g.objects {
            *i += base;
        }
    }
    objects.extend(props.objects);

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
        swamp: None,
        cave: None,
        forest: Some(ForestLayout {
            areas: Some(areas.layout),
            terrain: Some(terrain),
            water: Some(water.layout),
            thickets: Some(thickets),
            passages: Some(passages.layout),
            clearings: Some(clearings),
            trees: Some(trees.layout),
            props: Some(props.layout),
        }),
        noise_preview: None,
        notes,
    }
}

pub(crate) fn neighbors4(c: usize, w: usize, h: usize) -> impl Iterator<Item = usize> {
    let (x, y) = (c % w, c / w);
    [
        (x > 0).then(|| c - 1),
        (x + 1 < w).then(|| c + 1),
        (y > 0).then(|| c - w),
        (y + 1 < h).then(|| c + w),
    ]
    .into_iter()
    .flatten()
}

/// Euclidean distance, meters, from every cell to its nearest source cell
/// (8-neighbor propagation carrying the source), and that source; up to
/// `limit_m` (farther: infinity and `usize::MAX`).
pub(crate) fn distance_field(w: usize, h: usize, cell: f32, is_source: impl Fn(usize) -> bool, limit_m: f32) -> (Vec<f32>, Vec<usize>) {
    let n = w * h;
    let mut dist = vec![f32::INFINITY; n];
    let mut src = vec![usize::MAX; n];
    let mut heap = BinaryHeap::new();
    for c in 0..n {
        if is_source(c) {
            dist[c] = 0.0;
            src[c] = c;
            heap.push(Reverse((0u32, c)));
        }
    }
    let limit = limit_m / cell;
    while let Some(Reverse((d_bits, c))) = heap.pop() {
        if f32::from_bits(d_bits) > dist[c] {
            continue;
        }
        let s = src[c];
        let (sx, sy) = ((s % w) as f32, (s / w) as f32);
        for nb in water::neighbors8(c, w, h) {
            let nd = ((nb % w) as f32 - sx).hypot((nb / w) as f32 - sy);
            if nd < dist[nb] && nd <= limit {
                dist[nb] = nd;
                src[nb] = s;
                heap.push(Reverse((nd.to_bits(), nb)));
            }
        }
    }
    (dist.into_iter().map(|d| d * cell).collect(), src)
}
