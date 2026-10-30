//! The regions node: split the town area into regions. The user sets a count
//! and each region's share of the town area (the UI auto-balances the rest so
//! the total stays 100%). Regions are placed SMALLEST first: the smallest
//! seeds first and grows to its target area, every next one seeds on the
//! frontier of the already-placed ones ("beside"), and the largest region is
//! placed last, absorbing everything left — so the town always ends fully
//! split. Each region rolls `nesting_pct` for where it starts: against the
//! wall (regions slice the town from the wall inward) or deep inside the town
//! (the regions placed after it grow around it — nested regions).
//!
//! Growth is a shortest-path flood from the seed whose step cost is stretched
//! along a random per-region direction and modulated by a strong noise field
//! (`irregularity_pct`): elongated, lobed, bay-cut shapes without holes. Two clean-up
//! passes then guarantee every region is ONE 4-connected piece: cells no
//! growth could reach go to the nearest neighboring region, and stray
//! fragments merge into the region they share the most border with.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::noise::NoiseField;
use crate::generator::Stage;
use crate::level::{CellMask, RegionMap, RegionSplit, RegionStat};
use crate::model::LevelConfig;

/// Noise contrast at 100% irregularity: the per-step cost is multiplied by
/// exp(±CONTRAST·…), so growth races along cheap noise valleys (lobes,
/// fingers) and stalls on ridges (bays).
const CONTRAST: f32 = 3.0;
/// Largest length-to-width stretch of a region at 100% irregularity.
const MAX_STRETCH: f32 = 3.0;

/// `build_with_progress` without progress reporting.
pub fn build(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    town: &CellMask,
) -> Option<RegionSplit> {
    build_with_progress(config, rng, town, &Stage { progress: &|_, _| {}, from: 0.0, to: 1.0 })
}

pub(crate) fn build_with_progress(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    town: &CellMask,
    stage: &Stage,
) -> Option<RegionSplit> {
    let total = town.count_true() as u32;
    if total == 0 {
        return None;
    }
    let (w, h) = (town.width, town.height);
    let count = (config.regions.count as usize).clamp(1, 16);

    // Normalized shares (the UI keeps them near 100; hand-edited config files
    // may not — the generator self-heals either way).
    let mut shares = if config.regions.shares.len() == count {
        config.regions.shares.clone()
    } else {
        vec![100.0 / count as f32; count]
    };
    let sum: f32 = shares.iter().sum();
    if sum <= 0.0 {
        for s in &mut shares {
            *s = 100.0 / count as f32;
        }
    } else {
        for s in &mut shares {
            *s = *s / sum * 100.0;
        }
    }

    // Targets in cells; placement order is smallest first, ties by id.
    let targets: Vec<u32> = shares
        .iter()
        .map(|s| ((s / 100.0) * total as f32).round() as u32)
        .collect();
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by_key(|&i| (targets[i], i));

    // Shape character: one noise field for all regions (each reads its own
    // offset patch of it), contrast and stretch scaled by irregularity.
    let noise = NoiseField::new(rng.random());
    let irregularity = config.regions.irregularity_pct.clamp(0.0, 100.0) / 100.0;

    let depth = wall_depth(town);
    let nesting = config.regions.nesting_pct.clamp(0.0, 100.0) / 100.0;

    let mut ids = vec![0u8; w * h];
    let mut claimed: u32 = 0;
    for (place_no, &i) in order.iter().enumerate() {
        stage.report("Growing regions", claimed as f32 / total as f32 * 0.95);
        let last = place_no + 1 == order.len();
        // The largest (last) region takes everything still free.
        let target = if last {
            total - claimed
        } else {
            targets[i].min(total - claimed)
        };
        if target == 0 {
            continue;
        }
        let id = (i + 1) as u8;
        // Nested: start deep enough that a round blob of this size fits
        // without touching the wall, so later regions can close around it.
        let nested = rng.random::<f32>() < nesting;
        let radius = (target as f32 / std::f32::consts::PI).sqrt() as u32 + 1;
        let start = SeedPlan {
            first: place_no == 0,
            nested,
            depth: &depth,
            radius,
        };
        let seed = pick_seed(rng, town, &ids, &start);
        // Every region gets its own direction, stretch and noise patch; the
        // noise features are sized to the region so it gets a few big lobes.
        let angle = rng.random::<f32>() * std::f32::consts::PI;
        let stretch = 1.0 + irregularity * (MAX_STRETCH - 1.0) * rng.random::<f32>();
        let offset = (rng.random::<f32>() * 1000.0, rng.random::<f32>() * 1000.0);
        let growth = Growth {
            noise: &noise,
            scale: ((target as f32).sqrt() * 0.35).max(3.0),
            offset,
            contrast: irregularity * CONTRAST,
            stretch,
            axis: (angle.cos(), angle.sin()),
            keep_free_connected: !last,
            // A nested region stays off the wall so later regions can close
            // around it (falls back to the wall only if it runs out of room).
            avoid_wall: nested.then_some(depth.as_slice()),
        };
        claimed += grow(town, &mut ids, &growth, seed, id, target);
    }

    // Pockets the growth could not reach (cut off by other regions) join the
    // nearest region, then every region is made one connected piece.
    stage.report("Cleaning up regions", 0.95);
    assign_leftovers(town, &mut ids);
    merge_fragments(town, &mut ids);

    let stats = (0..count)
        .map(|i| {
            let cells = ids.iter().filter(|&&id| id == i as u8 + 1).count() as u32;
            RegionStat {
                id: i as u8 + 1,
                target_pct: shares[i],
                actual_pct: cells as f32 / total as f32 * 100.0,
                cells,
            }
        })
        .collect();

    Some(RegionSplit {
        map: RegionMap {
            width: w,
            height: h,
            cell_size_m: town.cell_size_m,
            ids,
        },
        stats,
    })
}

/// The 4-neighbors of `cell` inside the grid.
fn neighbors4(w: usize, h: usize, cell: usize) -> impl Iterator<Item = usize> {
    let (x, y) = (cell % w, cell / w);
    [
        (x > 0).then(|| cell - 1),
        (x + 1 < w).then(|| cell + 1),
        (y > 0).then(|| cell - w),
        (y + 1 < h).then(|| cell + w),
    ]
    .into_iter()
    .flatten()
}

/// The 8-neighbors of `cell` inside the grid.
fn neighbors8(w: usize, h: usize, cell: usize) -> impl Iterator<Item = usize> {
    let (x, y) = ((cell % w) as i64, (cell / w) as i64);
    [(-1i64, -1i64), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]
        .into_iter()
        .filter_map(move |(dx, dy)| {
            let (nx, ny) = (x + dx, y + dy);
            (nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64)
                .then(|| ny as usize * w + nx as usize)
        })
}

/// Distance of every town cell from the outside, in 4-steps (1 = touches
/// the wall or the map border, 0 = not town).
fn wall_depth(town: &CellMask) -> Vec<u32> {
    let (w, h) = (town.width, town.height);
    let mut depth = vec![0u32; w * h];
    let mut queue = VecDeque::new();
    for (i, d) in depth.iter_mut().enumerate() {
        let on_edge = town.cells[i] && neighbors4(w, h, i).filter(|&n| town.cells[n]).count() < 4;
        if on_edge {
            *d = 1;
            queue.push_back(i);
        }
    }
    while let Some(c) = queue.pop_front() {
        for n in neighbors4(w, h, c) {
            if town.cells[n] && depth[n] == 0 {
                depth[n] = depth[c] + 1;
                queue.push_back(n);
            }
        }
    }
    depth
}

/// Where the next region should start.
struct SeedPlan<'a> {
    /// The first placement (anywhere in town); later ones start beside the
    /// already-claimed cells.
    first: bool,
    /// Start deep inside the town instead of against the wall.
    nested: bool,
    depth: &'a [u32],
    /// Rough radius of the region, in cells.
    radius: u32,
}

/// Choose where a region starts. Candidates: anywhere in town for the first
/// placement, the frontier of the already-claimed cells ("beside") for later
/// ones — never where the claim alone would split the free area. A wall
/// region takes a candidate on the town edge; a nested one takes a candidate
/// at least `radius` cells from the wall (or the deepest available), so the
/// regions placed after it can grow all the way around it.
fn pick_seed(rng: &mut ChaCha8Rng, town: &CellMask, ids: &[u8], plan: &SeedPlan) -> usize {
    let (w, h) = (town.width, town.height);
    let free: Vec<usize> = (0..ids.len())
        .filter(|&i| town.cells[i] && ids[i] == 0)
        .collect();
    let beside: Vec<usize> = if plan.first {
        free.clone()
    } else {
        free.iter()
            .copied()
            .filter(|&i| neighbors4(w, h, i).any(|n| ids[n] != 0))
            .collect()
    };
    let safe: Vec<usize> = beside
        .iter()
        .copied()
        .filter(|&i| !splits_free(town, ids, i))
        .collect();
    let preferred: Vec<usize> = if plan.nested {
        let deepest = safe.iter().map(|&i| plan.depth[i]).max().unwrap_or(0);
        let need = (plan.radius + 1).min(deepest);
        safe.iter()
            .copied()
            .filter(|&i| need > 1 && plan.depth[i] >= need)
            .collect()
    } else {
        safe.iter()
            .copied()
            .filter(|&i| plan.depth[i] == 1)
            .collect()
    };
    let candidates = [preferred, safe, beside, free]
        .into_iter()
        .find(|c| !c.is_empty())
        .expect("a seed is picked only while free town cells remain");
    candidates[rng.random_range(0..candidates.len())]
}

/// Grow a region from its seed to `target` cells: a shortest-path flood over
/// 8 directions (diagonals cost √2, so no diamond bias). A step's cost is its
/// length measured in the region's stretched frame (cheap along `axis`,
/// expensive across it → elongated regions) times a noise factor (cheap
/// valleys → lobes and fingers, ridges → bays). Cells are claimed in cost
/// order, so the region stays hole-free; a cell is claimed only when it
/// touches the region orthogonally, keeping it one 4-connected piece. With `keep_free_connected`, the still-free town area
/// is never split: a claim that would cut off a pocket takes the pocket along
/// when it fits the remaining budget, otherwise the cell is skipped (and
/// retried when a later claim reaches it again). With `avoid_wall`, cells on
/// the town edge are held back until nothing else is reachable.
/// Deterministic (no RNG; ties by cell index). Returns the cells claimed —
/// short of target if the reachable area ran out.
fn grow(
    town: &CellMask,
    ids: &mut [u8],
    growth: &Growth,
    seed: usize,
    id: u8,
    target: u32,
) -> u32 {
    let (w, h) = (town.width, town.height);
    let mut avoid_wall = growth.avoid_wall;
    let step_cost = |from: usize, to: usize| {
        let (dx, dy) = (
            (to % w) as f32 - (from % w) as f32,
            (to / w) as f32 - (from / w) as f32,
        );
        let (c, s) = growth.axis;
        let (along, across) = (dx * c + dy * s, dy * c - dx * s);
        let length = (along * along / growth.stretch + across * across * growth.stretch).sqrt();
        let (x, y) = ((to % w) as f32, (to / w) as f32);
        let n = growth.noise.fbm(
            x / growth.scale + growth.offset.0,
            y / growth.scale + growth.offset.1,
            3,
        );
        let factor = (growth.contrast * (n - 0.5) * 4.0).exp();
        // Fixed point keeps the heap ordering exact and platform-independent.
        ((length * factor * 1024.0) as u32).max(1)
    };
    let mut held_back: Vec<Reverse<(u32, usize)>> = Vec::new();
    let mut search = PocketSearch::new(w * h);
    let mut best = vec![u32::MAX; w * h];
    let mut heap = BinaryHeap::new();
    best[seed] = 0;
    heap.push(Reverse((0u32, seed)));
    let mut grown = 0u32;
    loop {
        let Some(Reverse((cost, cell))) = heap.pop() else {
            // Out of room away from the wall: release the held-back cells.
            if held_back.is_empty() {
                break;
            }
            avoid_wall = None;
            heap.extend(held_back.drain(..));
            continue;
        };
        if ids[cell] != 0 || cost > best[cell] {
            continue;
        }
        if cell != seed && !neighbors4(w, h, cell).any(|n| ids[n] == id) {
            // Reached only diagonally so far: wait for an orthogonal claim.
            best[cell] = u32::MAX;
            continue;
        }
        if growth.keep_free_connected && cell != seed {
            match search.claim(town, ids, cell, id, target - grown) {
                Some(n) => grown += n,
                None => {
                    // Allow a later neighbor claim to push it again.
                    best[cell] = u32::MAX;
                    continue;
                }
            }
        } else {
            ids[cell] = id;
            grown += 1;
        }
        if grown >= target {
            break;
        }
        for n in neighbors8(w, h, cell) {
            if !town.cells[n] || ids[n] != 0 {
                continue;
            }
            let c = cost + step_cost(cell, n);
            if c < best[n] {
                best[n] = c;
                if avoid_wall.is_some_and(|depth| depth[n] == 1) {
                    held_back.push(Reverse((c, n)));
                } else {
                    heap.push(Reverse((c, n)));
                }
            }
        }
    }
    grown
}

/// How one region grows (see `grow`).
#[derive(Clone, Copy)]
struct Growth<'a> {
    noise: &'a NoiseField,
    /// Noise feature size in cells, and this region's patch of the field.
    scale: f32,
    offset: (f32, f32),
    /// Noise influence on the step cost (0 = none).
    contrast: f32,
    /// Length-to-width stretch along `axis` (cos, sin); 1 = round.
    stretch: f32,
    axis: (f32, f32),
    keep_free_connected: bool,
    /// Wall depth map; when set, town-edge cells are held back.
    avoid_wall: Option<&'a [u32]>,
}

/// Connectivity-preserving claims: decides whether claiming a cell would cut
/// the free town area apart and, if so, how big the cut-off pocket is. Keeps
/// its visit stamps between calls so each check costs only what it explores.
struct PocketSearch {
    stamp: Vec<u32>,
    generation: u32,
}

impl PocketSearch {
    fn new(cells: usize) -> Self {
        Self {
            stamp: vec![0; cells],
            generation: 0,
        }
    }

    /// Claim `cell` for region `id` without splitting the free area. Every
    /// pocket the claim would cut off is claimed too, if cell + pockets fit
    /// `budget`; otherwise nothing changes and `None` is returned. Returns
    /// the number of cells claimed.
    fn claim(
        &mut self,
        town: &CellMask,
        ids: &mut [u8],
        cell: usize,
        id: u8,
        budget: u32,
    ) -> Option<u32> {
        if !splits_free(town, ids, cell) {
            ids[cell] = id;
            return Some(1);
        }
        let (w, h) = (town.width, town.height);
        ids[cell] = id;
        let mut taken: Vec<usize> = Vec::new();
        loop {
            let free: Vec<usize> = neighbors4(w, h, cell)
                .filter(|&n| town.cells[n] && ids[n] == 0)
                .collect();
            let pocket = free[1..]
                .iter()
                .find_map(|&b| self.pocket_between(town, ids, free[0], b));
            let Some(pocket) = pocket else { break };
            if 1 + taken.len() + pocket.len() > budget as usize {
                ids[cell] = 0;
                for c in taken {
                    ids[c] = 0;
                }
                return None;
            }
            for &c in &pocket {
                ids[c] = id;
            }
            taken.extend(pocket);
        }
        Some(1 + taken.len() as u32)
    }

    /// Two interleaved floods through the free cells, from `a` and from `b`.
    /// If they meet, `a` and `b` are connected → `None`. If one flood runs
    /// out first, the cells it visited are a pocket cut off from the other
    /// side → returned. Cost is about twice the smaller side.
    fn pocket_between(
        &mut self,
        town: &CellMask,
        ids: &[u8],
        a: usize,
        b: usize,
    ) -> Option<Vec<usize>> {
        let (w, h) = (town.width, town.height);
        self.generation += 2;
        let mark = [self.generation, self.generation + 1];
        let mut seen: [Vec<usize>; 2] = [vec![a], vec![b]];
        let mut head = [0usize; 2];
        self.stamp[a] = mark[0];
        self.stamp[b] = mark[1];
        loop {
            for side in 0..2 {
                if head[side] == seen[side].len() {
                    return Some(std::mem::take(&mut seen[side]));
                }
                let c = seen[side][head[side]];
                head[side] += 1;
                for n in neighbors4(w, h, c) {
                    if !town.cells[n] || ids[n] != 0 {
                        continue;
                    }
                    if self.stamp[n] == mark[1 - side] {
                        return None;
                    }
                    if self.stamp[n] != mark[side] {
                        self.stamp[n] = mark[side];
                        seen[side].push(n);
                    }
                }
            }
        }
    }
}

/// Fast local test: could claiming `cell` split the free (unclaimed town)
/// cells around it? False when its free 4-neighbors stay linked through the
/// free 3×3 ring (N–E via NE, and so on) — then the claim provably keeps the
/// free area connected. True only means "maybe": `PocketSearch` decides.
fn splits_free(town: &CellMask, ids: &[u8], cell: usize) -> bool {
    let (w, h) = (town.width as i64, town.height as i64);
    let (x, y) = ((cell as i64) % w, (cell as i64) / w);
    let free = |dx: i64, dy: i64| {
        let (nx, ny) = (x + dx, y + dy);
        if nx < 0 || ny < 0 || nx >= w || ny >= h {
            return false;
        }
        let n = (ny * w + nx) as usize;
        town.cells[n] && ids[n] == 0
    };
    // N, E, S, W and the corners between consecutive ones: NE, SE, SW, NW.
    let orth = [free(0, -1), free(1, 0), free(0, 1), free(-1, 0)];
    let corner = [free(1, -1), free(1, 1), free(-1, 1), free(-1, -1)];
    let n_free = orth.iter().filter(|&&f| f).count();
    if n_free <= 1 {
        return false;
    }
    let links = (0..4)
        .filter(|&i| orth[i] && orth[(i + 1) % 4] && corner[i])
        .count();
    // A full ring of 4 links is a cycle: still one group.
    n_free.saturating_sub(links).max(1) > 1
}

/// Give every still-unclaimed town cell to the nearest claimed region
/// (multi-source BFS in index order — deterministic). First through the town
/// itself, so each pocket joins a region it touches; then across the whole
/// grid, for town slivers the outer wall's chords cut off from the main area.
fn assign_leftovers(town: &CellMask, ids: &mut [u8]) {
    let (w, h) = (town.width, town.height);
    for through_town_only in [true, false] {
        let mut owner = ids.to_vec();
        let mut queue: VecDeque<usize> = (0..ids.len()).filter(|&i| ids[i] != 0).collect();
        while let Some(cell) = queue.pop_front() {
            for n in neighbors4(w, h, cell) {
                if owner[n] != 0 || (through_town_only && !town.cells[n]) {
                    continue;
                }
                owner[n] = owner[cell];
                queue.push_back(n);
            }
        }
        for i in 0..ids.len() {
            if town.cells[i] && ids[i] == 0 {
                ids[i] = owner[i];
            }
        }
    }
}

/// Make every region one 4-connected piece: each region keeps its largest
/// piece (ties: the one found first), and every other piece is merged into
/// the neighboring region it shares the most border with. Repeats until
/// stable (a merge can never split a region, so this converges quickly).
fn merge_fragments(town: &CellMask, ids: &mut [u8]) {
    let (w, h) = (town.width, town.height);
    for _ in 0..16 {
        // Label pieces: 4-connected runs of the same id.
        let mut piece = vec![u32::MAX; ids.len()];
        let mut pieces: Vec<(u8, Vec<usize>)> = Vec::new();
        for start in 0..ids.len() {
            if ids[start] == 0 || piece[start] != u32::MAX {
                continue;
            }
            let label = pieces.len() as u32;
            let id = ids[start];
            let mut cells = vec![start];
            piece[start] = label;
            let mut k = 0;
            while k < cells.len() {
                let c = cells[k];
                k += 1;
                for n in neighbors4(w, h, c) {
                    if ids[n] == id && piece[n] == u32::MAX {
                        piece[n] = label;
                        cells.push(n);
                    }
                }
            }
            pieces.push((id, cells));
        }

        // The main piece of every region.
        let mut main: Vec<Option<usize>> = vec![None; 256];
        for (p, (id, cells)) in pieces.iter().enumerate() {
            let slot = &mut main[*id as usize];
            if slot.is_none_or(|m| pieces[m].1.len() < cells.len()) {
                *slot = Some(p);
            }
        }

        let mut changed = false;
        for (p, (id, cells)) in pieces.iter().enumerate() {
            if main[*id as usize] == Some(p) {
                continue;
            }
            let mut contact = [0u32; 256];
            for &c in cells {
                for n in neighbors4(w, h, c) {
                    if ids[n] != 0 && ids[n] != *id {
                        contact[ids[n] as usize] += 1;
                    }
                }
            }
            let target = (1..256).max_by_key(|&k| (contact[k], Reverse(k)));
            if let Some(t) = target.filter(|&t| contact[t] > 0) {
                for &c in cells {
                    ids[c] = t as u8;
                }
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    fn rect_town(w: usize, h: usize) -> CellMask {
        let mut m = CellMask::new(w, h, 1.0);
        for y in 2..h - 2 {
            for x in 2..w - 2 {
                m.set(x, y, true);
            }
        }
        m
    }

    fn config_with(count: u32, shares: &[f32]) -> LevelConfig {
        let mut config = LevelConfig::test_64();
        config.regions.count = count;
        config.regions.shares = shares.to_vec();
        config
    }

    #[test]
    fn town_is_fully_split_and_targets_hold() {
        let town = rect_town(30, 20);
        let total = town.count_true() as u32; // 26 × 16 = 416
        let mut rng = ChaCha8Rng::seed_from_u64(42);
        let split = build(&config_with(3, &[20.0, 30.0, 50.0]), &mut rng, &town).unwrap();

        // Every town cell got an id; nothing outside did.
        for i in 0..town.cells.len() {
            assert_eq!(split.map.ids[i] != 0, town.cells[i]);
        }
        // The two smaller regions hit their rounded targets; the largest
        // absorbs the rounding remainder.
        let expected = |pct: f32| (pct / 100.0 * total as f32).round() as u32;
        assert_eq!(split.stats[0].cells, expected(20.0));
        assert_eq!(split.stats[1].cells, expected(30.0));
        assert_eq!(
            split.stats[2].cells,
            total - split.stats[0].cells - split.stats[1].cells
        );
    }

    #[test]
    fn same_seed_same_split() {
        let town = rect_town(30, 20);
        let mut a = ChaCha8Rng::seed_from_u64(9);
        let mut b = ChaCha8Rng::seed_from_u64(9);
        let sa = build(&config_with(4, &[25.0, 25.0, 25.0, 25.0]), &mut a, &town).unwrap();
        let sb = build(&config_with(4, &[25.0, 25.0, 25.0, 25.0]), &mut b, &town).unwrap();
        assert_eq!(sa.map.ids, sb.map.ids);
    }

    #[test]
    fn one_region_takes_everything() {
        let town = rect_town(20, 16);
        let mut rng = ChaCha8Rng::seed_from_u64(5);
        let split = build(&config_with(1, &[100.0]), &mut rng, &town).unwrap();
        // Every TOWN cell is region 1; cells outside stay 0.
        for i in 0..town.cells.len() {
            assert_eq!(split.map.ids[i] != 0, town.cells[i]);
            assert!(split.map.ids[i] == 0 || split.map.ids[i] == 1);
        }
    }

    #[test]
    fn shares_are_normalized() {
        let town = rect_town(30, 20);
        let total = town.count_true() as u32;
        let mut rng = ChaCha8Rng::seed_from_u64(13);
        // Sums to 120 — normalized to 5/12 and 7/12 of the town.
        let split = build(&config_with(2, &[50.0, 70.0]), &mut rng, &town).unwrap();
        let expected_small = total * 50 / 120;
        assert_eq!(split.stats[0].cells, expected_small);
        assert_eq!(split.stats[1].cells, total - expected_small);
    }

    /// Eyeball helper (not an assertion): run with
    /// `cargo test ascii_regions_preview -- --nocapture` to see the split
    /// (digits = region ids, '.' = outside).
    #[test]
    fn ascii_regions_preview() {
        let mut config = LevelConfig::test_64();
        if let Ok(n) = std::env::var("NESTING") {
            config.regions.nesting_pct = n.parse().unwrap();
        }
        let shape = crate::generator::shape::build(&config, 42);
        let mut rng = ChaCha8Rng::seed_from_u64(42);
        let wall = crate::generator::walls::build(
            &LevelConfig::test_64(),
            &mut rng,
            &shape.shape.mask,
        );
        if let Some(wall) = wall {
            let split = build(&config, &mut rng, &wall.region).unwrap();
            for y in 0..split.map.height {
                let row: String = (0..split.map.width)
                    .map(|x| {
                        let id = split.map.ids[y * split.map.width + x];
                        if id == 0 {
                            '.'
                        } else {
                            (b'0' + id) as char
                        }
                    })
                    .collect();
                println!("{row}");
            }
            for s in &split.stats {
                println!("region {}: target {:.1}% actual {:.1}%", s.id, s.target_pct, s.actual_pct);
            }
        }
    }


    #[test]
    fn regions_are_one_piece_and_hit_targets() {
        let town = rect_town(60, 40);
        for count in 2..=8u32 {
            let shares: Vec<f32> = (1..=count).map(|k| k as f32).collect();
            let sum: f32 = shares.iter().sum();
            for seed in 0..10u64 {
                let mut rng = ChaCha8Rng::seed_from_u64(seed);
                let split = build(&config_with(count, &shares), &mut rng, &town).unwrap();
                for (s, share) in split.stats.iter().zip(&shares) {
                    let mut m = CellMask::new(town.width, town.height, 1.0);
                    for (cell, &id) in m.cells.iter_mut().zip(&split.map.ids) {
                        *cell = id == s.id;
                    }
                    let (_, pieces) = crate::generator::grid::components(&m);
                    assert_eq!(pieces, 1, "count {count} seed {seed} region {}", s.id);
                    let want = share / sum * 100.0;
                    assert!(
                        (s.actual_pct - want).abs() < 0.5,
                        "count {count} seed {seed} region {}: {} vs {want}",
                        s.id,
                        s.actual_pct
                    );
                }
            }
        }
    }

    #[test]
    fn nesting_puts_regions_inside_each_other() {
        let town = rect_town(60, 40);
        let depth = wall_depth(&town);
        let touches_wall = |split: &RegionSplit, id: u8| {
            (0..split.map.ids.len()).any(|i| split.map.ids[i] == id && depth[i] == 1)
        };
        for seed in 0..10u64 {
            // Always nested: the smallest region sits fully inside the town.
            let mut config = config_with(3, &[10.0, 30.0, 60.0]);
            config.regions.nesting_pct = 100.0;
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let split = build(&config, &mut rng, &town).unwrap();
            assert!(!touches_wall(&split, 1), "seed {seed}: nested region touches the wall");

            // Never nested: every region reaches the wall.
            config.regions.nesting_pct = 0.0;
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let split = build(&config, &mut rng, &town).unwrap();
            for id in 1..=3 {
                assert!(touches_wall(&split, id), "seed {seed}: region {id} is enclosed");
            }
        }
    }
}
