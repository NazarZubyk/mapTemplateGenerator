//! Forest node 2, areas (step F1 in docs/forest-plan.md): the map split
//! into a few large areas, each wild or managed and with its own
//! character.
//!
//! 1. **Seeds** — one per area, spread out (best of `SEED_TRIES` random
//!    spots, the one farthest from the seeds already there), then evened
//!    out by a few Lloyd passes (each seed moves to the middle of its
//!    cell), so the sizes differ only as much as the weights say.
//! 2. **Split** — a weighted Voronoi on noise-warped coordinates: every
//!    cell goes to the seed with the smallest distance / weight. The warp
//!    (`irregularity_pct`) bends the borders, the weights
//!    (`size_variation_pct`) make some areas bigger.
//! 3. **Cleanup** — every area is made one 4-connected piece: stray
//!    fragments join the area they share the most border with; areas
//!    left with no cells are dropped and the ids renumbered.
//! 4. **Style** — per area a value mixed from a gradient (distance from
//!    the first gate with `managed_near_gate`, else a random direction)
//!    and a random roll (`mixing_pct`). The lowest values are managed,
//!    up to the managed share of the map area.
//! 5. **Character** — rolled by weight from the characters of the area's
//!    style, avoiding the characters of the neighbors already rolled.

use std::collections::{HashMap, VecDeque};

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::noise::NoiseField;
use crate::generator::swamp::gate_anchors::edge_hit;
use crate::level::{AreaBorder, AreaLayout, ForestArea, ForestCharacter, ForestStyle};
use crate::model::LevelConfig;

/// Random spots tried per seed.
const SEED_TRIES: usize = 24;
/// Lloyd passes evening out the seeds.
const LLOYD_PASSES: usize = 3;
/// Cell stride of the Lloyd passes (they only need a rough centroid).
const LLOYD_STRIDE: usize = 4;
/// Warp amplitude at 100% irregularity, as a share of the mean area size.
const WARP: f32 = 0.6;
/// Seed weight swing at 100% size variation (weight in 1 ± this).
const WEIGHT_SWING: f32 = 0.45;
/// Cleanup passes (each moves every stray fragment once).
const CLEANUP_PASSES: usize = 6;

pub struct AreaBuild {
    pub layout: AreaLayout,
    pub notes: Vec<String>,
}

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng) -> AreaBuild {
    let s = &config.forest.areas;
    let cell = config.cell_size_m;
    let w = ((config.map.width_m / cell).round() as usize).max(1);
    let h = ((config.map.height_m / cell).round() as usize).max(1);
    let (wm, hm) = (w as f32 * cell, h as f32 * cell);
    let n = w * h;
    let mut notes = Vec::new();
    let count = s.count.clamp(1, 64) as usize;

    // RNG: seeds and weights, the two warp seeds, the style direction and
    // rolls, then the characters.
    let mut seeds: Vec<(f32, f32)> = Vec::with_capacity(count);
    for _ in 0..count {
        let mut best = (0.0, 0.0);
        let mut best_d = -1.0f32;
        for _ in 0..SEED_TRIES {
            let p = (rng.random_range(0.0..wm), rng.random_range(0.0..hm));
            let d = seeds.iter().map(|q| (p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).fold(f32::INFINITY, f32::min);
            if d > best_d {
                best_d = d;
                best = p;
            }
        }
        seeds.push(best);
    }
    for _ in 0..LLOYD_PASSES {
        let mut sums = vec![(0.0f32, 0.0f32, 0u32); count];
        for y in (0..h).step_by(LLOYD_STRIDE) {
            for x in (0..w).step_by(LLOYD_STRIDE) {
                let p = ((x as f32 + 0.5) * cell, (y as f32 + 0.5) * cell);
                let near = (0..count)
                    .min_by(|&a, &b| {
                        let d = |q: (f32, f32)| (p.0 - q.0).powi(2) + (p.1 - q.1).powi(2);
                        d(seeds[a]).total_cmp(&d(seeds[b]))
                    })
                    .unwrap_or(0);
                let e = &mut sums[near];
                e.0 += p.0;
                e.1 += p.1;
                e.2 += 1;
            }
        }
        for (seed, &(x, y, k)) in seeds.iter_mut().zip(&sums) {
            if k > 0 {
                *seed = (x / k as f32, y / k as f32);
            }
        }
    }
    let var = (s.size_variation_pct / 100.0).clamp(0.0, 1.0);
    let weights: Vec<f32> = (0..count).map(|_| 1.0 + var * WEIGHT_SWING * rng.random_range(-1.0..1.0f32)).collect();
    let warp_x = NoiseField::new(rng.random::<u64>());
    let warp_y = NoiseField::new(rng.random::<u64>());

    // 2. Split.
    let mean = (wm * hm / count as f32).sqrt();
    let amp = (s.irregularity_pct / 100.0).clamp(0.0, 1.0) * WARP * mean;
    let scale = mean * 0.5;
    let mut ids = vec![0u16; n];
    for (c, id) in ids.iter_mut().enumerate() {
        let (x, y) = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
        let px = x + amp * (warp_x.fbm(x / scale, y / scale, 4) - 0.5) * 2.0;
        let py = y + amp * (warp_y.fbm(x / scale, y / scale, 4) - 0.5) * 2.0;
        let mut best = (f32::INFINITY, 0);
        for (i, (q, wt)) in seeds.iter().zip(&weights).enumerate() {
            let d = ((px - q.0).powi(2) + (py - q.1).powi(2)).sqrt() / wt;
            if d < best.0 {
                best = (d, i);
            }
        }
        *id = best.1 as u16 + 1;
    }

    // 3. Cleanup, then renumber the areas that kept cells.
    for _ in 0..CLEANUP_PASSES {
        if !merge_fragments(&mut ids, w, h) {
            break;
        }
    }
    let mut remap = vec![0u16; count + 1];
    let mut next = 0u16;
    let mut kept_seeds = Vec::new();
    for (i, seed) in seeds.iter().enumerate() {
        if ids.contains(&(i as u16 + 1)) {
            next += 1;
            remap[i + 1] = next;
            kept_seeds.push(*seed);
        }
    }
    for id in &mut ids {
        *id = remap[*id as usize];
    }
    let kept = next as usize;
    if kept < count {
        notes.push(format!("Areas: {kept} of {count} came out — the others were too small; raise the map size or lower the size variation"));
    }

    // Stats and borders.
    let mut sums = vec![(0u32, 0.0f32, 0.0f32); kept];
    let mut edges: HashMap<(u16, u16), u32> = HashMap::new();
    for c in 0..n {
        let id = ids[c];
        let e = &mut sums[id as usize - 1];
        e.0 += 1;
        e.1 += ((c % w) as f32 + 0.5) * cell;
        e.2 += ((c / w) as f32 + 0.5) * cell;
        for nb in [(c % w + 1 < w).then(|| c + 1), (c / w + 1 < h).then(|| c + w)].into_iter().flatten() {
            if ids[nb] != id {
                *edges.entry((id.min(ids[nb]), id.max(ids[nb]))).or_default() += 1;
            }
        }
    }
    let mut borders: Vec<AreaBorder> = edges.into_iter().map(|((a, b), e)| AreaBorder { a, b, length_m: e as f32 * cell }).collect();
    borders.sort_by_key(|b| (b.a, b.b));
    let center: Vec<[f32; 2]> = sums.iter().map(|&(k, x, y)| [x / k.max(1) as f32, y / k.max(1) as f32]).collect();
    let area_m2: Vec<f32> = sums.iter().map(|&(k, _, _)| k as f32 * cell * cell).collect();

    // 4. Style.
    let m = (s.mixing_pct / 100.0).clamp(0.0, 1.0);
    let dir: f32 = rng.random_range(0.0..360.0f32).to_radians();
    let rolls: Vec<f32> = (0..kept).map(|_| rng.random_range(-1.0..1.0f32)).collect();
    let gate = config.gates.gates.first().filter(|_| s.managed_near_gate).map(|g| edge_hit((wm / 2.0, hm / 2.0), g.direction_deg.rem_euclid(360.0), wm, hm).0);
    let diag = (wm * wm + hm * hm).sqrt();
    let gradient = |p: [f32; 2]| match gate {
        // 0 at the gate, about 1 at the far corner.
        Some(g) => ((p[0] - g.0).powi(2) + (p[1] - g.1).powi(2)).sqrt() / diag * 2.0 - 1.0,
        None => ((p[0] - wm / 2.0) * dir.cos() + (hm / 2.0 - p[1]) * dir.sin()) / (diag / 2.0),
    };
    let value: Vec<f32> = (0..kept).map(|i| (1.0 - m) * gradient(center[i]) + m * rolls[i]).collect();
    let mut order: Vec<usize> = (0..kept).collect();
    order.sort_by(|&a, &b| value[a].total_cmp(&value[b]).then(a.cmp(&b)));
    let total: f32 = area_m2.iter().sum();
    let managed_want = (1.0 - (s.wild_pct / 100.0).clamp(0.0, 1.0)) * total;
    // Take managed areas in order while that brings the share closer.
    let mut style = vec![ForestStyle::Wild; kept];
    let mut managed = 0.0;
    for &i in &order {
        if (managed + area_m2[i] - managed_want).abs() < (managed - managed_want).abs() {
            managed += area_m2[i];
            style[i] = ForestStyle::Managed;
        } else {
            break;
        }
    }
    let wild_pct = if total > 0.0 { (total - managed) / total * 100.0 } else { 100.0 };

    // 5. Character.
    let neighbors = |id: u16| borders.iter().filter_map(move |b| if b.a == id { Some(b.b) } else if b.b == id { Some(b.a) } else { None });
    let mut character: Vec<Option<ForestCharacter>> = vec![None; kept];
    for i in 0..kept {
        let taken: Vec<ForestCharacter> = neighbors(i as u16 + 1).filter_map(|nb| character[nb as usize - 1]).collect();
        let fits: Vec<ForestCharacter> = ForestCharacter::ALL.into_iter().filter(|c| c.fits(style[i])).collect();
        let free: Vec<ForestCharacter> = fits.iter().copied().filter(|c| !taken.contains(c)).collect();
        let pick = roll(rng, &free, &s.characters).or_else(|| roll(rng, &fits, &s.characters)).unwrap_or(match style[i] {
            ForestStyle::Wild => ForestCharacter::OpenWoods,
            ForestStyle::Managed => ForestCharacter::Logged,
        });
        character[i] = Some(pick);
    }

    let areas = (0..kept)
        .map(|i| ForestArea {
            id: i as u16 + 1,
            style: style[i],
            character: character[i].unwrap_or(ForestCharacter::OpenWoods),
            area_m2: area_m2[i],
            center_m: center[i],
        })
        .collect();
    AreaBuild {
        layout: AreaLayout { width: w, height: h, cell_size_m: cell, ids, areas, borders, wild_pct, requested: count as u32 },
        notes,
    }
}

/// A weighted pick; `None` when the list is empty or all weights are 0.
fn roll(rng: &mut ChaCha8Rng, from: &[ForestCharacter], weights: &crate::model::CharacterWeights) -> Option<ForestCharacter> {
    let total: f32 = from.iter().map(|&c| weights.of(c).max(0.0)).sum();
    if total <= 0.0 {
        return None;
    }
    let mut r = rng.random_range(0.0..total);
    for &c in from {
        r -= weights.of(c).max(0.0);
        if r < 0.0 {
            return Some(c);
        }
    }
    from.iter().copied().rfind(|&c| weights.of(c) > 0.0)
}

/// Gives every 4-connected fragment of an area except its largest to the
/// area it shares the most border with. Returns whether anything moved.
fn merge_fragments(ids: &mut [u16], w: usize, h: usize) -> bool {
    let n = ids.len();
    let mut label = vec![u32::MAX; n];
    let mut pieces: Vec<(u16, Vec<usize>)> = Vec::new();
    let mut queue = VecDeque::new();
    for start in 0..n {
        if label[start] != u32::MAX {
            continue;
        }
        let id = ids[start];
        let no = pieces.len() as u32;
        let mut cells = vec![start];
        label[start] = no;
        queue.push_back(start);
        while let Some(c) = queue.pop_front() {
            for nb in neighbors4(c, w, h) {
                if label[nb] == u32::MAX && ids[nb] == id {
                    label[nb] = no;
                    cells.push(nb);
                    queue.push_back(nb);
                }
            }
        }
        pieces.push((id, cells));
    }
    let mut largest: HashMap<u16, usize> = HashMap::new();
    for (i, (id, cells)) in pieces.iter().enumerate() {
        let e = largest.entry(*id).or_insert(i);
        if pieces[*e].1.len() < cells.len() {
            *e = i;
        }
    }
    let mut moved = false;
    for (i, (id, cells)) in pieces.iter().enumerate() {
        if largest[id] == i {
            continue;
        }
        let mut touch: HashMap<u16, u32> = HashMap::new();
        for &c in cells {
            for nb in neighbors4(c, w, h) {
                if ids[nb] != *id {
                    *touch.entry(ids[nb]).or_default() += 1;
                }
            }
        }
        if let Some((&to, _)) = touch.iter().max_by_key(|&(&k, &v)| (v, std::cmp::Reverse(k))) {
            for &c in cells {
                ids[c] = to;
            }
            moved = true;
        }
    }
    moved
}

fn neighbors4(c: usize, w: usize, h: usize) -> impl Iterator<Item = usize> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::generator::grid;
    use crate::level::CellMask;
    use crate::model::MapKind;

    fn config() -> LevelConfig {
        let mut c = LevelConfig { kind: MapKind::Forest, ..LevelConfig::default() };
        c.map.width_m = 160.0;
        c.map.height_m = 120.0;
        c.cell_size_m = 1.0;
        c
    }

    fn run(c: &LevelConfig, seed: u64) -> AreaLayout {
        build(c, &mut ChaCha8Rng::seed_from_u64(seed)).layout
    }

    #[test]
    fn every_cell_in_one_connected_area() {
        for (irr, seed) in [(0.0, 1), (50.0, 2), (100.0, 3)] {
            let mut c = config();
            c.forest.areas.irregularity_pct = irr;
            let l = run(&c, seed);
            assert_eq!(l.areas.len(), 6, "seed {seed}");
            assert!(l.ids.iter().all(|&id| id >= 1 && id as usize <= l.areas.len()));
            for a in &l.areas {
                let mask = CellMask { width: l.width, height: l.height, cell_size_m: 1.0, cells: l.ids.iter().map(|&id| id == a.id).collect() };
                assert!(grid::is_connected(&mask), "seed {seed}: area {} split", a.id);
            }
            assert!(l.borders.len() >= 5, "seed {seed}: {:?}", l.borders);
        }
    }

    #[test]
    fn wild_share_is_close() {
        for (wild, seed) in [(60.0, 4), (30.0, 5)] {
            let mut c = config();
            c.forest.areas.wild_pct = wild;
            let l = run(&c, seed);
            // Whole areas only: within one mean area (100/6 %).
            assert!((l.wild_pct - wild).abs() <= 100.0 / 6.0 + 1.0, "asked {wild}, got {}", l.wild_pct);
            assert!(l.areas.iter().any(|a| a.style == ForestStyle::Managed));
            assert!(l.areas.iter().any(|a| a.style == ForestStyle::Wild));
        }
        let mut c = config();
        c.forest.areas.wild_pct = 100.0;
        assert!(run(&c, 6).areas.iter().all(|a| a.style == ForestStyle::Wild));
    }

    #[test]
    fn managed_starts_at_the_gate() {
        for seed in 0..4 {
            let mut c = config();
            c.forest.areas.mixing_pct = 0.0;
            c.gates.gates = vec![crate::model::GateSpec::at(180.0)];
            let l = run(&c, seed);
            // West edge, middle.
            let at = (l.height / 2) * l.width;
            assert_eq!(l.area_at(at).style, ForestStyle::Managed, "seed {seed}");
        }
    }

    #[test]
    fn characters_fit_the_style_and_neighbors_differ() {
        for seed in 0..4 {
            let l = run(&config(), seed);
            for a in &l.areas {
                assert!(a.character.fits(a.style), "{a:?}");
            }
            let same = l.borders.iter().filter(|b| l.areas[b.a as usize - 1].character == l.areas[b.b as usize - 1].character).count();
            assert!(same <= 1, "seed {seed}: {same} neighbor pairs share a character");
        }
    }

    #[test]
    fn same_seed_same_areas() {
        let c = config();
        assert_eq!(run(&c, 7), run(&c, 7));
    }
}
