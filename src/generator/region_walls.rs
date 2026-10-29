//! The region walls node: walls along the borders between regions, built the
//! same way as the outer wall, then opened wherever a road passes.
//!
//! 1. **Border lines** — every unit edge of the corner lattice that separates
//!    two different regions, grouped by region pair and chained into lines.
//!    A line ends where the border meets the outer wall or where three
//!    regions meet (so the walls of neighboring pairs join there); a border
//!    around a nested region is a closed loop.
//! 2. **Point walk** — like the outer wall: a point after every segment of
//!    random length in `[segment_min_m, segment_max_m]`. An open line always
//!    ends exactly on its last vertex (no closing chord), so walls meet the
//!    outer wall and each other with no leaks; a loop closes with a chord.
//! 3. **Rasterize** — `thickness_m` wide, town cells only.
//! 4. **Fit the regions to the walls** — the straight segments cut across
//!    the curvy region borders, so every area the walls enclose takes the
//!    region id that owns most of it (wall cells take the nearest one): each
//!    region now ends exactly at its walls, and the region stats are
//!    recounted.
//! 5. **Gaps** — wall cells on a road, or within `gap_margin_m` of one, are
//!    removed: every road passes through its own opening, so the regions
//!    stay connected exactly where the roads cross.
//! 6. **Seal dead pockets** — two walls can close off a sliver no road
//!    reaches; such pockets become wall, so the walkable ground stays one
//!    piece.

use std::collections::{HashMap, VecDeque};

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::walls::rasterize_segment;
use crate::generator::Stage;
use crate::level::{CellMask, RegionMap, RegionSplit, RegionWalls, RoadNetwork, WallGap};
use crate::model::LevelConfig;

/// `build_with_progress` without progress reporting.
pub fn build(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    split: &mut RegionSplit,
    roads: Option<&RoadNetwork>,
) -> Option<RegionWalls> {
    build_with_progress(config, rng, split, roads, &Stage { progress: &|_, _| {}, from: 0.0, to: 1.0 })
}

/// Build the walls between the regions of `split` and refit the regions to
/// them (`split` is updated in place); `None` when there is only one region
/// (nothing to separate).
pub(crate) fn build_with_progress(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    split: &mut RegionSplit,
    roads: Option<&RoadNetwork>,
    stage: &Stage,
) -> Option<RegionWalls> {
    let map = &split.map;
    let (w, h) = (map.width, map.height);
    let cell = map.cell_size_m.max(0.05);
    let settings = &config.region_walls;

    // 1. Border lines.
    stage.report("Tracing region borders", 0.0);
    let lines = border_lines(map);
    if lines.is_empty() {
        return None;
    }

    // 2.–3. Walk every line and rasterize it.
    let lo = settings.segment_min_m.min(settings.segment_max_m).max(0.2);
    let hi = settings.segment_min_m.max(settings.segment_max_m).max(0.2);
    let thickness = ((settings.thickness_m.max(0.05) / cell).round() as i64).max(1);
    let cw = w + 1;
    let vertex_pos = |vi: usize| ((vi % cw) as f32 * cell, (vi / cw) as f32 * cell);
    let mut raw = CellMask::new(w, h, cell);
    let mut points_total = 0u32;
    for (k, line) in lines.iter().enumerate() {
        if k % 8 == 0 {
            stage.report("Building region walls", 0.1 + 0.6 * k as f32 / lines.len() as f32);
        }
        let n = line.vertices.len();
        let mut points = vec![vertex_pos(line.vertices[0])];
        let mut current = 0usize;
        loop {
            let seg_m = rng.random_range(lo..=hi);
            let next = current + ((seg_m / cell).round() as usize).max(1);
            if next >= n - 1 {
                break;
            }
            points.push(vertex_pos(line.vertices[next]));
            current = next;
        }
        if line.closed {
            if points.len() >= 3 {
                points.push(points[0]);
            } else {
                // Too short to walk as a polygon: follow the loop exactly.
                points = line.vertices.iter().map(|&v| vertex_pos(v)).collect();
                points.push(points[0]);
            }
        } else {
            points.push(vertex_pos(line.vertices[n - 1]));
        }
        points_total += points.len() as u32;
        for pair in points.windows(2) {
            rasterize_segment(&mut raw, pair[0], pair[1], cell, thickness);
        }
    }
    let mut wall = CellMask::new(w, h, cell);
    for ((out, &r), &id) in wall.cells.iter_mut().zip(&raw.cells).zip(&map.ids) {
        *out = r && id != 0;
    }

    // 4. Fit the regions to the closed walls (before any gap opens them).
    stage.report("Fitting regions to walls", 0.75);
    let recolored = fit_regions_to_walls(&mut split.map, &wall);
    recount_stats(split);
    let map = &split.map;

    // 5. Gaps where roads pass.
    stage.report("Opening gaps for roads", 0.85);
    let mut gaps = Vec::new();
    if let Some(net) = roads {
        let margin = (settings.gap_margin_m.max(0.0) / cell).round() as u32;
        let clearance = near_road(&net.mask, margin);
        let mut opened = CellMask::new(w, h, cell);
        for ((cell_wall, opened), &clear) in wall.cells.iter_mut().zip(&mut opened.cells).zip(&clearance) {
            if *cell_wall && clear {
                *cell_wall = false;
                *opened = true;
            }
        }
        gaps = describe_gaps(map, &opened);

        // 6. Seal pockets no road reaches.
        stage.report("Sealing dead pockets", 0.95);
        let mut open = CellMask::new(w, h, cell);
        for ((o, &id), &wl) in open.cells.iter_mut().zip(&map.ids).zip(&wall.cells) {
            *o = id != 0 && !wl;
        }
        let (labels, count) = crate::generator::grid::components(&open);
        let mut reached = vec![false; count as usize + 1];
        for (&l, &r) in labels.iter().zip(&net.mask.cells) {
            if r {
                reached[l as usize] = true;
            }
        }
        for (cell_wall, &l) in wall.cells.iter_mut().zip(&labels) {
            if l != 0 && !reached[l as usize] {
                *cell_wall = true;
            }
        }
    }

    Some(RegionWalls {
        mask: wall,
        gaps,
        lines: lines.len() as u32,
        points: points_total,
        recolored_cells: recolored,
    })
}

/// Give every area the walls enclose (a 4-connected piece of town minus
/// wall) the region id that owns most of it (ties: lower id), then every wall
/// cell the id of the nearest open cell. Returns the cells whose id changed.
fn fit_regions_to_walls(map: &mut RegionMap, wall: &CellMask) -> u32 {
    let (w, h) = (map.width, map.height);
    let mut open = CellMask::new(w, h, map.cell_size_m);
    for ((o, &id), &wl) in open.cells.iter_mut().zip(&map.ids).zip(&wall.cells) {
        *o = id != 0 && !wl;
    }
    let (labels, count) = crate::generator::grid::components(&open);
    let mut votes = vec![[0u32; 256]; count as usize + 1];
    for (i, &l) in labels.iter().enumerate() {
        if l != 0 {
            votes[l as usize][map.ids[i] as usize] += 1;
        }
    }
    let winner: Vec<u8> = votes
        .iter()
        .map(|v| (1..=255u8).max_by_key(|&id| (v[id as usize], std::cmp::Reverse(id))).unwrap_or(0))
        .collect();

    let before = map.ids.clone();
    let mut owner = vec![0u8; w * h];
    let mut queue = VecDeque::new();
    for (i, &l) in labels.iter().enumerate() {
        if l != 0 {
            owner[i] = winner[l as usize];
            queue.push_back(i);
        }
    }
    // Wall cells: nearest open cell's id (through the town).
    while let Some(c) = queue.pop_front() {
        let (x, y) = (c % w, c / w);
        let neighbors = [
            (x > 0).then(|| c - 1),
            (x + 1 < w).then(|| c + 1),
            (y > 0).then(|| c - w),
            (y + 1 < h).then(|| c + w),
        ];
        for n in neighbors.into_iter().flatten() {
            if before[n] != 0 && owner[n] == 0 {
                owner[n] = owner[c];
                queue.push_back(n);
            }
        }
    }
    let mut changed = 0u32;
    for ((id, &new), &old) in map.ids.iter_mut().zip(&owner).zip(&before) {
        if old != 0 && new != 0 && new != old {
            *id = new;
            changed += 1;
        }
    }
    changed
}

/// Recount every region's cells and share after the ids changed.
pub(crate) fn recount_stats(split: &mut RegionSplit) {
    let total = split.map.ids.iter().filter(|&&id| id != 0).count().max(1) as f32;
    for stat in &mut split.stats {
        stat.cells = split.map.ids.iter().filter(|&&id| id == stat.id).count() as u32;
        stat.actual_pct = stat.cells as f32 / total * 100.0;
    }
}

/// One stretch of border between two regions, as corner-lattice vertices.
struct BorderLine {
    vertices: Vec<usize>,
    closed: bool,
}

/// Collect the borders between regions and chain them into lines: per region
/// pair, the unit edges form a graph; lines run between vertices whose degree
/// is not 2 (border meets the outer wall, three regions meet, or a pinch),
/// and whatever is left are closed loops. Deterministic: pairs in id order,
/// edges in scan order.
fn border_lines(map: &RegionMap) -> Vec<BorderLine> {
    let (w, h) = (map.width, map.height);
    let cw = w + 1;
    let v = |x: usize, y: usize| y * cw + x;
    let ids = &map.ids;

    // Edges per region pair; a vertex's degree counts ALL inter-region and
    // town-edge edges, so a line stops wherever another wall meets it.
    let mut by_pair: HashMap<(u8, u8), Vec<(usize, usize)>> = HashMap::new();
    let mut degree_all: HashMap<usize, u32> = HashMap::new();
    let mut add_boundary = |a: u8, b: u8, e: (usize, usize)| {
        if a == b {
            return;
        }
        *degree_all.entry(e.0).or_default() += 1;
        *degree_all.entry(e.1).or_default() += 1;
        if a != 0 && b != 0 {
            by_pair.entry((a.min(b), a.max(b))).or_default().push(e);
        }
    };
    let id_at = |x: i64, y: i64| {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            0
        } else {
            ids[y as usize * w + x as usize]
        }
    };
    for y in 0..=h as i64 {
        for x in 0..=w as i64 {
            // Vertical edge on the left side of cell (x, y).
            if y < h as i64 {
                add_boundary(id_at(x - 1, y), id_at(x, y), (v(x as usize, y as usize), v(x as usize, y as usize + 1)));
            }
            // Horizontal edge on the top side of cell (x, y).
            if x < w as i64 {
                add_boundary(id_at(x, y - 1), id_at(x, y), (v(x as usize, y as usize), v(x as usize + 1, y as usize)));
            }
        }
    }

    let mut pairs: Vec<(u8, u8)> = by_pair.keys().copied().collect();
    pairs.sort();
    let mut lines = Vec::new();
    for pair in pairs {
        let edges = &by_pair[&pair];
        let mut at: HashMap<usize, Vec<usize>> = HashMap::new();
        for (i, &(a, b)) in edges.iter().enumerate() {
            at.entry(a).or_default().push(i);
            at.entry(b).or_default().push(i);
        }
        let stops = |vertex: usize| degree_all.get(&vertex).copied().unwrap_or(0) != 2;
        let mut used = vec![false; edges.len()];
        let walk = |start_vertex: usize, first: usize, used: &mut Vec<bool>| {
            let mut vertices = vec![start_vertex];
            let mut edge = first;
            let mut vertex = start_vertex;
            loop {
                used[edge] = true;
                let (a, b) = edges[edge];
                vertex = if a == vertex { b } else { a };
                vertices.push(vertex);
                if vertex == start_vertex || stops(vertex) {
                    break;
                }
                match at[&vertex].iter().copied().find(|&e| !used[e]) {
                    Some(e) => edge = e,
                    None => break,
                }
            }
            vertices
        };
        // Open lines first: start at every stop vertex, once per unused edge.
        for (i, &(a, b)) in edges.iter().enumerate() {
            for start in [a, b] {
                if !used[i] && stops(start) {
                    let vertices = walk(start, i, &mut used);
                    lines.push(BorderLine { vertices, closed: false });
                }
            }
        }
        // What remains are closed loops (borders around nested regions).
        for i in 0..edges.len() {
            if !used[i] {
                let start = edges[i].0;
                let mut vertices = walk(start, i, &mut used);
                let closed = vertices.last() == Some(&start);
                if closed {
                    vertices.pop();
                }
                lines.push(BorderLine { vertices, closed });
            }
        }
    }
    lines
}

/// Cells on the road or within `margin` cells of it (4-step distance).
fn near_road(road: &CellMask, margin: u32) -> Vec<bool> {
    let (w, h) = (road.width, road.height);
    let mut dist = vec![u32::MAX; w * h];
    let mut queue = VecDeque::new();
    for (i, &r) in road.cells.iter().enumerate() {
        if r {
            dist[i] = 0;
            queue.push_back(i);
        }
    }
    while let Some(c) = queue.pop_front() {
        if dist[c] >= margin {
            continue;
        }
        let (x, y) = (c % w, c / w);
        let neighbors = [
            (x > 0).then(|| c - 1),
            (x + 1 < w).then(|| c + 1),
            (y > 0).then(|| c - w),
            (y + 1 < h).then(|| c + w),
        ];
        for n in neighbors.into_iter().flatten() {
            if dist[n] == u32::MAX {
                dist[n] = dist[c] + 1;
                queue.push_back(n);
            }
        }
    }
    dist.into_iter().map(|d| d != u32::MAX).collect()
}

/// Group the removed wall cells into openings (8-connected pieces) and name
/// the two regions each one connects: the two most common region ids around
/// it.
fn describe_gaps(map: &RegionMap, opened: &CellMask) -> Vec<WallGap> {
    let (w, h) = (map.width, map.height);
    let mut seen = vec![false; w * h];
    let mut gaps = Vec::new();
    for start in 0..w * h {
        if !opened.cells[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        let mut piece = vec![start];
        let mut k = 0;
        while k < piece.len() {
            let c = piece[k];
            k += 1;
            let (x, y) = ((c % w) as i64, (c / w) as i64);
            for dy in -1..=1i64 {
                for dx in -1..=1i64 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let n = ny as usize * w + nx as usize;
                    if opened.cells[n] && !seen[n] {
                        seen[n] = true;
                        piece.push(n);
                    }
                }
            }
        }
        // Region ids in and right around the opening.
        let mut count = [0u32; 256];
        for &c in &piece {
            let (x, y) = ((c % w) as i64, (c / w) as i64);
            for (dx, dy) in [(0i64, 0i64), (-2, 0), (2, 0), (0, -2), (0, 2)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 {
                    count[map.ids[ny as usize * w + nx as usize] as usize] += 1;
                }
            }
        }
        let mut ranked: Vec<u8> = (1..=255u8).filter(|&id| count[id as usize] > 0).collect();
        ranked.sort_by_key(|&id| (std::cmp::Reverse(count[id as usize]), id));
        let (a, b) = match ranked.as_slice() {
            [a, b, ..] => (*a.min(b), *a.max(b)),
            [a] => (*a, *a),
            [] => (0, 0),
        };
        let (sx, sy) = piece.iter().fold((0usize, 0usize), |(sx, sy), &c| (sx + c % w, sy + c / w));
        gaps.push(WallGap {
            x: sx / piece.len(),
            y: sy / piece.len(),
            regions: (a, b),
            cells: piece.len() as u32,
        });
    }
    gaps
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    /// Shape → outer wall → regions → roads for `config` and `seed`.
    fn pipeline(config: &LevelConfig, seed: u64) -> (RegionSplit, RoadNetwork, ChaCha8Rng) {
        let shape = crate::generator::shape::build(config, seed);
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let wall = crate::generator::walls::build(config, &mut rng, &shape.shape.mask).unwrap();
        let split = crate::generator::regions::build(config, &mut rng, &wall.region).unwrap();
        let roads = crate::generator::roads::build(config, &mut rng, &split).unwrap().network;
        (split, roads, rng)
    }

    /// 4-connected pieces of the town minus the walls.
    fn open_pieces(map: &RegionMap, wall: &CellMask) -> CellMask {
        let mut open = CellMask::new(map.width, map.height, map.cell_size_m);
        for ((o, &id), &wl) in open.cells.iter_mut().zip(&map.ids).zip(&wall.cells) {
            *o = id != 0 && !wl;
        }
        open
    }

    #[test]
    fn regions_fit_the_walls_exactly() {
        for seed in 1..=5u64 {
            let config = LevelConfig::default(); // 3 regions
            let (mut split, _, mut rng) = pipeline(&config, seed);
            let original = split.map.ids.clone();
            let walls = build(&config, &mut rng, &mut split, None).unwrap();
            let map = &split.map;
            let open = open_pieces(map, &walls.mask);
            let (labels, count) = crate::generator::grid::components(&open);
            // Every area the walls enclose is exactly one region.
            for piece in 1..=count {
                let ids: std::collections::BTreeSet<u8> =
                    (0..labels.len()).filter(|&i| labels[i] == piece).map(|i| map.ids[i]).collect();
                assert_eq!(ids.len(), 1, "seed {seed}: piece {piece} mixes regions {ids:?}");
            }
            assert!(count >= 3, "seed {seed}: walls did not split the town ({count} pieces)");
            // Only slivers moved, and the stats follow the new ids.
            let town = original.iter().filter(|&&id| id != 0).count();
            assert!((walls.recolored_cells as f32) < town as f32 * 0.05, "seed {seed}");
            let stat_cells: u32 = split.stats.iter().map(|s| s.cells).sum();
            assert_eq!(stat_cells as usize, town);
            for s in &split.stats {
                assert_eq!(s.cells as usize, map.ids.iter().filter(|&&id| id == s.id).count());
            }
        }
    }

    #[test]
    fn roads_pass_through_gaps_and_every_region_stays_reachable() {
        for seed in 1..=5u64 {
            let config = LevelConfig::default();
            let (mut split, roads, mut rng) = pipeline(&config, seed);
            let walls = build(&config, &mut rng, &mut split, Some(&roads)).unwrap();
            let map = &split.map;
            // No road cell is walled.
            assert!(
                roads.mask.cells.iter().zip(&walls.mask.cells).all(|(&r, &wl)| !(r && wl)),
                "seed {seed}: a wall blocks a road"
            );
            assert!(!walls.gaps.is_empty(), "seed {seed}: no gaps");
            // No dead pockets: the open town is one piece.
            assert!(
                crate::generator::grid::is_connected(&open_pieces(map, &walls.mask)),
                "seed {seed}: a pocket is sealed off"
            );
            // All regions are reachable from each other through the open town.
            let open = open_pieces(map, &walls.mask);
            let (labels, _) = crate::generator::grid::components(&open);
            let road_piece = labels[roads.mask.cells.iter().position(|&r| r).unwrap()];
            for id in 1..=3u8 {
                assert!(
                    (0..labels.len()).any(|i| map.ids[i] == id && labels[i] == road_piece),
                    "seed {seed}: region {id} is cut off"
                );
            }
        }
    }

    #[test]
    fn one_region_needs_no_walls() {
        let mut config = LevelConfig::default();
        config.regions.count = 1;
        config.regions.shares = vec![100.0];
        let (mut split, roads, mut rng) = pipeline(&config, 4);
        assert!(build(&config, &mut rng, &mut split, Some(&roads)).is_none());
    }

    #[test]
    fn same_seed_same_walls() {
        let config = LevelConfig::default();
        let (mut sa, roads, mut a) = pipeline(&config, 8);
        let (mut sb, _, mut b) = pipeline(&config, 8);
        assert_eq!(build(&config, &mut a, &mut sa, Some(&roads)), build(&config, &mut b, &mut sb, Some(&roads)));
        assert_eq!(sa, sb);
    }

    /// Eyeball helper: `cargo test ascii_region_walls_preview -- --nocapture`
    /// ('#' region wall, '=' road, digits = region ids).
    #[test]
    fn ascii_region_walls_preview() {
        let config = LevelConfig::default();
        let (mut split, roads, mut rng) = pipeline(&config, 42);
        let walls = build(&config, &mut rng, &mut split, Some(&roads)).unwrap();
        let map = &split.map;
        for y in (0..map.height).step_by(2) {
            let row: String = (0..map.width)
                .map(|x| {
                    let i = y * map.width + x;
                    if walls.mask.cells[i] {
                        '#'
                    } else if roads.mask.cells[i] {
                        '='
                    } else if map.ids[i] == 0 {
                        '.'
                    } else {
                        (b'0' + map.ids[i]) as char
                    }
                })
                .collect();
            println!("{row}");
        }
        println!("lines {} points {} gaps {}", walls.lines, walls.points, walls.gaps.len());
    }
}
