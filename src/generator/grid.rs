//! Algorithms over [`CellMask`]: component labeling, speckle removal,
//! bridging separate pieces, filling enclosed holes. Pure deterministic
//! functions — no RNG, everything is derived from the mask itself.

use std::collections::VecDeque;

use crate::level::CellMask;

/// Fixed 4-direction neighbor order, so every walk over the grid is
/// deterministic.
const NEIGHBORS4: [(i64, i64); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

/// 4-connected component labeling. Returns per-cell labels (0 = unmasked,
/// components numbered 1..=count) and the component count. Scan and neighbor
/// order are fixed, so the labels are deterministic.
pub fn components(mask: &CellMask) -> (Vec<u32>, u32) {
    let mut labels = vec![0u32; mask.cells.len()];
    let mut count = 0u32;
    for y in 0..mask.height {
        for x in 0..mask.width {
            if !mask.get(x, y) || labels[y * mask.width + x] != 0 {
                continue;
            }
            count += 1;
            let mut stack = vec![(x, y)];
            labels[y * mask.width + x] = count;
            while let Some((cx, cy)) = stack.pop() {
                for (dx, dy) in NEIGHBORS4 {
                    let (nx, ny) = (cx as i64 + dx, cy as i64 + dy);
                    if nx < 0 || ny < 0 {
                        continue;
                    }
                    let (nx, ny) = (nx as usize, ny as usize);
                    if nx < mask.width
                        && ny < mask.height
                        && mask.get(nx, ny)
                        && labels[ny * mask.width + nx] == 0
                    {
                        labels[ny * mask.width + nx] = count;
                        stack.push((nx, ny));
                    }
                }
            }
        }
    }
    (labels, count)
}

/// True when the mask is empty or forms one single 4-connected piece.
pub fn is_connected(mask: &CellMask) -> bool {
    let (_, count) = components(mask);
    count <= 1
}

/// Drop every component smaller than `min_cells`; returns the number of
/// removed cells. Used to shed single-cell noise specks before bridging.
pub fn remove_small_components(mask: &mut CellMask, min_cells: u32) -> u32 {
    let (labels, count) = components(mask);
    if count == 0 {
        return 0;
    }
    let mut sizes = vec![0u32; count as usize + 1];
    for label in &labels {
        if *label > 0 {
            sizes[*label as usize] += 1;
        }
    }
    let mut removed = 0u32;
    for (i, label) in labels.iter().enumerate() {
        if *label > 0 && sizes[*label as usize] < min_cells {
            mask.cells[i] = false;
            removed += 1;
        }
    }
    removed
}

/// Fill unmasked cells that are fully enclosed by the shape (not reachable
/// from the map border). Returns the number of cells filled.
pub fn fill_holes(mask: &mut CellMask) -> u32 {
    if mask.width == 0 || mask.height == 0 {
        return 0;
    }
    let (w, h) = (mask.width, mask.height);
    let mut outside = vec![false; mask.cells.len()];
    let mut stack = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let on_border = x == 0 || y == 0 || x == w - 1 || y == h - 1;
            let i = y * w + x;
            if on_border && !mask.cells[i] && !outside[i] {
                outside[i] = true;
                stack.push((x, y));
            }
        }
    }
    while let Some((cx, cy)) = stack.pop() {
        for (dx, dy) in NEIGHBORS4 {
            let (nx, ny) = (cx as i64 + dx, cy as i64 + dy);
            if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            let i = ny * w + nx;
            if !mask.cells[i] && !outside[i] {
                outside[i] = true;
                stack.push((nx, ny));
            }
        }
    }
    let mut filled = 0u32;
    for (i, cell) in mask.cells.iter_mut().enumerate() {
        if !*cell && !outside[i] {
            *cell = true;
            filled += 1;
        }
    }
    filled
}

/// Carve the shortest corridor between the two closest components (BFS from
/// all masked cells at once; the first meeting of two different labels is the
/// globally closest pair). Returns false when there is nothing to bridge
/// (0 or 1 components). One call merges one pair — loop it until the mask is
/// one piece.
pub fn bridge_closest_pair(mask: &mut CellMask, width_cells: u32) -> bool {
    let (w, h) = (mask.width as i64, mask.height as i64);
    // Seed the BFS with one label per COMPONENT (not per cell — two adjacent
    // cells of one blob must share a label, or the meet test fires inside it).
    let (mut label, count) = components(mask);
    if count <= 1 {
        return false;
    }
    let mut parent = vec![usize::MAX; mask.cells.len()];
    let mut queue = VecDeque::new();
    for y in 0..mask.height {
        for x in 0..mask.width {
            if mask.get(x, y) {
                queue.push_back((x as i64, y as i64));
            }
        }
    }

    // BFS over ALL cells (masked or not), carrying the label of the nearest
    // component. First cell reached from two different labels = bridge spot.
    while let Some((cx, cy)) = queue.pop_front() {
        let ci = (cy as usize) * mask.width + cx as usize;
        let cl = label[ci];
        for (dx, dy) in NEIGHBORS4 {
            let (nx, ny) = (cx + dx, cy + dy);
            if nx < 0 || ny < 0 || nx >= w || ny >= h {
                continue;
            }
            let ni = (ny as usize) * mask.width + nx as usize;
            let nl = label[ni];
            if nl == 0 {
                label[ni] = cl;
                parent[ni] = ci;
                queue.push_back((nx, ny));
            } else if nl != cl {
                carve_path(mask, ni, &parent, width_cells);
                carve_path(mask, ci, &parent, width_cells);
                return true;
            }
        }
    }
    false // unreachable on a connected grid; kept as a safety net
}

/// Follow parent links back to the source cell, stamping the mask along the way.
fn carve_path(mask: &mut CellMask, mut cell: usize, parent: &[usize], width_cells: u32) {
    while cell != usize::MAX {
        let x = (cell % mask.width) as i64;
        let y = (cell / mask.width) as i64;
        stamp(mask, x, y, width_cells);
        cell = parent[cell];
    }
}

/// Set a `width_cells`-square of cells, centered as closely as possible on (x, y).
fn stamp(mask: &mut CellMask, x: i64, y: i64, width_cells: u32) {
    let w = width_cells.max(1) as i64;
    let (x0, y0) = (x - (w - 1) / 2, y - (w - 1) / 2);
    for dy in 0..w {
        for dx in 0..w {
            mask.set_if_in_bounds(x0 + dx, y0 + dy, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// '#' = masked cell. Rows must all be `width` chars wide.
    fn mask_from_rows(width: usize, rows: &[&str]) -> CellMask {
        let mut m = CellMask::new(width, rows.len(), 1.0);
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                if c == '#' {
                    m.set(x, y, true);
                }
            }
        }
        m
    }

    #[test]
    fn components_are_counted_separately() {
        let m = mask_from_rows(7, &[
            "##.....",
            "##.....",
            ".....##",
            ".....##",
        ]);
        assert_eq!(components(&m).1, 2);
        assert!(!is_connected(&m));
    }

    #[test]
    fn small_components_are_removed() {
        let mut m = mask_from_rows(5, &[
            "###..",
            "###..",
            "###..",
            "....#",
            ".....",
        ]);
        let removed = remove_small_components(&mut m, 4);
        assert_eq!(removed, 1);
        assert_eq!(components(&m).1, 1);
    }

    #[test]
    fn enclosed_holes_fill_open_bays_do_not() {
        // Ring with an enclosed hole in the middle.
        let mut ring = mask_from_rows(5, &[
            ".....",
            ".###.",
            ".#.#.",
            ".###.",
            ".....",
        ]);
        assert_eq!(fill_holes(&mut ring), 1);
        assert!(ring.get(2, 2));

        // A bay that reaches the map border stays open.
        let mut bay = mask_from_rows(6, &[
            ".##.##",
            ".##.##",
            "######",
            "######",
        ]);
        assert_eq!(fill_holes(&mut bay), 0);
        assert!(!bay.get(0, 0) && !bay.get(3, 0));
    }

    #[test]
    fn bridge_merges_two_blobs_into_one() {
        let mut m = mask_from_rows(9, &[
            "###......",
            "###......",
            "###......",
            "......###",
            "......###",
            "......###",
        ]);
        assert!(!is_connected(&m));
        assert!(bridge_closest_pair(&mut m, 2));
        assert!(is_connected(&m), "one bridge must merge the two blobs");
        // A second call finds nothing left to bridge.
        assert!(!bridge_closest_pair(&mut m, 2));
    }

    #[test]
    fn bridging_loops_many_blobs_into_one() {
        let mut m = mask_from_rows(11, &[
            "##........#",
            "##...##...#",
            "##........#",
        ]);
        let mut bridges = 0;
        while !is_connected(&m) {
            assert!(bridge_closest_pair(&mut m, 1), "must always find a pair");
            bridges += 1;
            assert!(bridges <= 10, "bridging did not converge");
        }
        assert_eq!(bridges, 2);
    }
}
