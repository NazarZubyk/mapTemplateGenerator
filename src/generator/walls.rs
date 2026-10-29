//! The outer wall node: walk the region's border point by point — every
//! segment takes a random length between `segment_min_m` and `segment_max_m`
//! — until the wall closes on itself. The wall is built from the INSIDE of
//! the region: points sit on the border and the straight segments between
//! them cut corners slightly inward. The walk follows the border exactly, so
//! lagoons and deep bays are wrapped, never skipped by a shortcut across
//! non-town area. When the loop closes, the final chord may pass through the
//! interior — expected whenever the remaining border is shorter than a
//! segment (e.g. min == max). Afterwards the region is redefined as the
//! wall's interior: town outside the wall is cut away, empty space inside is
//! filled.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::level::{CellMask, OuterWall, OuterWallStats};
use crate::model::LevelConfig;

pub struct OuterWallBuild {
    pub wall: OuterWall,
    /// The region mask redefined as the wall's interior (wall cells excluded).
    pub region: CellMask,
}

/// Build the outer wall around `region`. Returns `None` when the region is
/// too small for a closed wall with these segment lengths.
pub fn build(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    region: &CellMask,
) -> Option<OuterWallBuild> {
    if region.count_true() == 0 {
        return None;
    }
    let (w, h) = (region.width, region.height);
    let cw = w + 1; // corner lattice width: vertices per row

    // 1. Collect the region's boundary as directed unit edges on the corner
    //    lattice, oriented so the region stays on the LEFT of every edge.
    //    Chaining edge-to-edge therefore walks the border hugging the region.
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); cw * (h + 1)];
    let v = |cx: usize, cy: usize| cy * cw + cx;
    for y in 0..h {
        for x in 0..w {
            if !region.get(x, y) {
                continue;
            }
            let above = y == 0 || !region.get(x, y - 1);
            let below = y == h - 1 || !region.get(x, y + 1);
            let left = x == 0 || !region.get(x - 1, y);
            let right = x == w - 1 || !region.get(x + 1, y);
            // Top side, walking west (region below = left).
            if above {
                let e = edges.len();
                edges.push((v(x + 1, y), v(x, y)));
                outgoing[v(x + 1, y)].push(e);
            }
            // Bottom side, walking east (region above = left).
            if below {
                let e = edges.len();
                edges.push((v(x, y + 1), v(x + 1, y + 1)));
                outgoing[v(x, y + 1)].push(e);
            }
            // Left side, walking south (region to the east = left).
            if left {
                let e = edges.len();
                edges.push((v(x, y), v(x, y + 1)));
                outgoing[v(x, y)].push(e);
            }
            // Right side, walking north (region to the west = left).
            if right {
                let e = edges.len();
                edges.push((v(x + 1, y + 1), v(x + 1, y)));
                outgoing[v(x + 1, y + 1)].push(e);
            }
        }
    }
    if edges.is_empty() {
        return None;
    }

    // 2. Chain the edges into closed loops (deterministic: lowest edge index
    //    first). The region is one connected, hole-free blob, so the longest
    //    loop is the outer boundary; anything else is a pinch artifact.
    let mut used = vec![false; edges.len()];
    let mut loops: Vec<(Vec<usize>, bool)> = Vec::new(); // (vertices, clean close)
    for start in 0..edges.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let start_vertex = edges[start].0;
        let mut loop_v = vec![start_vertex];
        let mut clean = true;
        let mut cur = start;
        loop {
            let next_vertex = edges[cur].1;
            if next_vertex == start_vertex {
                break;
            }
            loop_v.push(next_vertex);
            match outgoing[next_vertex].iter().copied().find(|&e| !used[e]) {
                Some(e) => {
                    used[e] = true;
                    cur = e;
                }
                None => {
                    clean = false;
                    break;
                }
            }
        }
        loops.push((loop_v, clean));
    }
    let (contour, clean) = loops.iter().max_by_key(|(l, _)| l.len())?;
    let extra_loops = (loops.len() - 1) as u32;

    // 3. Walk the contour placing a point after every random-length segment.
    //    One unit edge = one cell of along-border distance. When the next
    //    segment would run past the start, the loop closes with a straight
    //    chord back to the first point (it may cut through the interior).
    let settings = &config.outer_wall;
    let cell = region.cell_size_m.max(0.05);
    // Normalize so random_range can never panic on min > max (validated as a
    // warning; the generator just swaps the two).
    let lo = settings.segment_min_m.min(settings.segment_max_m).max(0.2);
    let hi = settings.segment_min_m.max(settings.segment_max_m).max(0.2);
    let n = contour.len();
    let vertex_pos = |vi: usize| {
        ((vi % cw) as f32 * cell, (vi / cw) as f32 * cell)
    };
    let mut points: Vec<(f32, f32)> = vec![vertex_pos(contour[0])];
    let mut current = 0usize;
    loop {
        let seg_m = rng.random_range(lo..=hi);
        let step = ((seg_m / cell).round() as usize).max(1);
        let next = current + step;
        if next >= n {
            break;
        }
        points.push(vertex_pos(contour[next]));
        current = next;
    }
    if points.len() < 3 {
        return None; // degenerate polygon — the region is too small
    }

    // 4. Rasterize the closed polygon (including the closing chord) into
    //    wall cells, stamping the configured thickness around every sample.
    //    Half-cell sampling keeps the marked cells 8-connected, and a
    //    diagonal chain already blocks a 4-connected flood.
    let thickness = ((settings.thickness_m.max(0.05) / cell).round() as i64).max(1);
    let mut wall = CellMask::new(w, h, cell);
    for i in 0..points.len() {
        rasterize_segment(
            &mut wall,
            points[i],
            points[(i + 1) % points.len()],
            cell,
            thickness,
        );
    }

    // 5. Flood the outside from the map border (through non-wall cells). What
    //    the flood cannot reach is inside the wall: that becomes the new
    //    region — town outside is cut, empty space inside is filled.
    let mut outside = vec![false; w * h];
    let mut stack: Vec<(i64, i64)> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let on_ring = x == 0 || y == 0 || x == w - 1 || y == h - 1;
            if on_ring && !wall.cells[i] {
                outside[i] = true;
                stack.push((x as i64, y as i64));
            }
        }
    }
    while let Some((cx, cy)) = stack.pop() {
        for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
            let (nx, ny) = (cx + dx, cy + dy);
            if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            let i = ny * w + nx;
            if !wall.cells[i] && !outside[i] {
                outside[i] = true;
                stack.push((nx as i64, ny as i64));
            }
        }
    }

    let mut new_region = CellMask::new(w, h, cell);
    let (mut cut, mut filled, mut wall_cells) = (0u32, 0u32, 0u32);
    for (((is_wall, is_outside), was_region), new_cell) in wall
        .cells
        .iter()
        .zip(outside.iter())
        .zip(region.cells.iter())
        .zip(&mut new_region.cells)
    {
        if *is_wall {
            wall_cells += 1;
        } else if *is_outside {
            if *was_region {
                cut += 1;
            }
        } else {
            *new_cell = true;
            if !*was_region {
                filled += 1;
            }
        }
    }

    Some(OuterWallBuild {
        wall: OuterWall {
            mask: wall,
            stats: OuterWallStats {
                points: points.len() as u32,
                closed: *clean,
                extra_loops,
                wall_cells,
                area_cut_cells: cut,
                area_filled_cells: filled,
            },
        },
        region: new_region,
    })
}

/// Mark the cells a straight segment passes through, sampling every half cell
/// and stamping a `thickness`-cell square around each sample.
fn rasterize_segment(
    wall: &mut CellMask,
    a: (f32, f32),
    b: (f32, f32),
    cell: f32,
    thickness: i64,
) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let dist = (dx * dx + dy * dy).sqrt();
    let steps = (dist / (cell * 0.5)).ceil() as usize;
    let steps = steps.max(1);
    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let px = a.0 + dx * t;
        let py = a.1 + dy * t;
        stamp(
            wall,
            (px / cell).floor() as i64,
            (py / cell).floor() as i64,
            thickness,
        );
    }
}

/// Set a `thickness`-square of cells, centered as closely as possible on the
/// given cell (so the wall grows on both sides of the walked line).
fn stamp(wall: &mut CellMask, cx: i64, cy: i64, thickness: i64) {
    let t = thickness.max(1);
    let (x0, y0) = (cx - (t - 1) / 2, cy - (t - 1) / 2);
    for dy in 0..t {
        for dx in 0..t {
            wall.set_if_in_bounds(x0 + dx, y0 + dy, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    /// A rectangular region: cells x in 2..=27, y in 2..=17 (cell size 1 m).
    fn rect_region(w: usize, h: usize) -> CellMask {
        let mut m = CellMask::new(w, h, 1.0);
        for y in 2..h - 2 {
            for x in 2..w - 2 {
                m.set(x, y, true);
            }
        }
        m
    }

    fn config_with(min: f32, max: f32) -> LevelConfig {
        let mut config = LevelConfig::default();
        config.outer_wall.segment_min_m = min;
        config.outer_wall.segment_max_m = max;
        config
    }

    #[test]
    fn wall_closes_and_keeps_the_interior() {
        let region = rect_region(30, 20);
        let mut rng = ChaCha8Rng::seed_from_u64(42);
        let built = build(&config_with(2.0, 5.0), &mut rng, &region).unwrap();

        assert!(built.wall.stats.closed);
        assert!(built.wall.stats.points >= 3);
        assert!(built.wall.stats.wall_cells > 20);
        // The center is inside the wall; the map corner is outside.
        assert!(built.region.get(14, 9));
        assert!(!built.region.get(0, 0));
        // Nothing was cut on a plain rectangle; the rasterized wall can dip a
        // cell or two outside at contour corners, which the interior fill
        // then absorbs — that is the designed corner behavior, so only a
        // small filled bound is asserted.
        // Chords cut the region's corners: slivers of region outside the wall
        // are cut, and rasterization dips can claim a cell or two inside —
        // both are the designed corner behavior, so only small bounds hold.
        assert!(built.wall.stats.area_cut_cells <= 8);
        assert!(built.wall.stats.area_filled_cells <= 8);
    }

    #[test]
    fn lagoon_is_wrapped_not_skipped() {
        // A U-shaped region: a wide deep bay open at the top. The wall must
        // follow the border into the bay instead of bridging its mouth.
        let mut region = rect_region(30, 20);
        for y in 2..12 {
            for x in 12..18 {
                region.set(x, y, false); // the lagoon
            }
        }
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let built = build(&config_with(2.0, 5.0), &mut rng, &region).unwrap();

        // The lagoon water stays OUTSIDE the wall.
        assert!(!built.region.get(14, 6), "lagoon was skipped and filled");
        // The wall wraps deep into the bay: wall cells line its lower part.
        let deep_wall = (0..built.wall.mask.cells.len()).any(|i| {
            let x = i % built.wall.mask.width;
            let y = i / built.wall.mask.width;
            built.wall.mask.cells[i] && (12..18).contains(&x) && y >= 9
        });
        assert!(deep_wall, "no wall cells deep inside the lagoon");
    }

    #[test]
    fn fixed_segment_length_still_closes() {
        // min == max: the loop must close with a chord through the interior.
        let region = rect_region(30, 20);
        let mut rng = ChaCha8Rng::seed_from_u64(3);
        let built = build(&config_with(10.0, 10.0), &mut rng, &region).unwrap();
        assert!(built.wall.stats.closed);
        assert!(built.region.get(14, 9));
    }

    #[test]
    fn same_seed_same_wall() {
        let region = rect_region(30, 20);
        let mut a = ChaCha8Rng::seed_from_u64(99);
        let mut b = ChaCha8Rng::seed_from_u64(99);
        let wa = build(&config_with(3.0, 9.0), &mut a, &region).unwrap();
        let wb = build(&config_with(3.0, 9.0), &mut b, &region).unwrap();
        assert_eq!(wa.wall.mask, wb.wall.mask);
        assert_eq!(wa.region, wb.region);
    }

    #[test]
    fn thicker_wall_widens_the_ring() {
        let region = rect_region(30, 20);
        let mut thin_config = config_with(3.0, 6.0);
        thin_config.outer_wall.thickness_m = 0.5; // 1 cell
        let mut thick_config = config_with(3.0, 6.0);
        thick_config.outer_wall.thickness_m = 2.0; // 2 cells
        let (mut a, mut b) = (ChaCha8Rng::seed_from_u64(11), ChaCha8Rng::seed_from_u64(11));
        let thin = build(&thin_config, &mut a, &region).unwrap();
        let thick = build(&thick_config, &mut b, &region).unwrap();
        assert!(thick.wall.stats.wall_cells > thin.wall.stats.wall_cells * 2);
        // Still a closed ring: the center stays inside.
        assert!(thick.region.get(14, 9));
    }

    #[test]
    fn region_too_small_yields_no_wall() {
        let mut tiny = CellMask::new(8, 8, 1.0);
        tiny.set(3, 3, true);
        tiny.set(4, 3, true);
        tiny.set(3, 4, true);
        tiny.set(4, 4, true);
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        assert!(build(&config_with(10.0, 20.0), &mut rng, &tiny).is_none());
    }

    /// Eyeball helper (not an assertion): run with
    /// `cargo test ascii_wall_preview -- --nocapture` to see the wall ring
    /// (W) around the region (#).
    #[test]
    fn ascii_wall_preview() {
        let built = crate::generator::shape::build(&LevelConfig::default(), 42);
        let region = built.shape.mask;
        let mut rng = ChaCha8Rng::seed_from_u64(42);
        if let Some(w) = build(&LevelConfig::default(), &mut rng, &region) {
            for y in 0..region.height {
                let row: String = (0..region.width)
                    .map(|x| {
                        if w.wall.mask.get(x, y) {
                            'W'
                        } else if w.region.get(x, y) {
                            '#'
                        } else {
                            '.'
                        }
                    })
                    .collect();
                println!("{row}");
            }
            println!("{:?}", w.wall.stats);
        }
    }
}
