//! Placement helpers shared by the object nodes (houses, props): rotated
//! footprints on the cell grid, keep-clear rings (dilation), distances to
//! roads and road directions.

use std::collections::VecDeque;

use crate::level::{Category, ObjectPart, PlacedObject, RoadNetwork};

/// Path cells on each side used to estimate a road's direction (long enough
/// to smooth the 8-direction staircase of a diagonal road).
const TANGENT_SPAN: usize = 8;

/// The cells whose centers fall inside a rotated object — main rect (`size` =
/// width along the angle, depth across it) plus its parts. `None` if any
/// corner leaves the map.
pub(crate) fn footprint(
    center: (f32, f32),
    size: (f32, f32),
    parts: &[ObjectPart],
    angle_deg: f32,
    w: usize,
    h: usize,
    cell: f32,
) -> Option<Vec<usize>> {
    let probe = PlacedObject {
        category: Category::Structure,
        type_name: String::new(),
        color: [0; 3],
        x_m: center.0 - size.0 / 2.0,
        y_m: center.1 - size.1 / 2.0,
        w_m: size.0,
        h_m: size.1,
        angle_deg,
        parts: parts.to_vec(),
        round: false,
    };
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (x, y) in probe.all_corners().into_iter().flatten() {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    if x0 < 0.0 || y0 < 0.0 || x1 > w as f32 * cell || y1 > h as f32 * cell {
        return None;
    }
    let mut cells = Vec::new();
    for cy in (y0 / cell).floor() as usize..((y1 / cell).ceil() as usize).min(h) {
        for cx in (x0 / cell).floor() as usize..((x1 / cell).ceil() as usize).min(w) {
            if probe.contains((cx as f32 + 0.5) * cell, (cy as f32 + 0.5) * cell) {
                cells.push(cy * w + cx);
            }
        }
    }
    Some(cells)
}

/// Unit direction of the road at every path cell (from a few cells behind
/// to a few cells ahead).
pub(crate) fn path_tangents(path: &[usize], w: usize) -> Vec<(f32, f32)> {
    let at = |c: usize| ((c % w) as f32, (c / w) as f32);
    (0..path.len())
        .map(|k| {
            let a = at(path[k.saturating_sub(TANGENT_SPAN)]);
            let b = at(path[(k + TANGENT_SPAN).min(path.len() - 1)]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = (dx * dx + dy * dy).sqrt();
            if len > 0.0 { (dx / len, dy / len) } else { (1.0, 0.0) }
        })
        .collect()
}

/// For every town cell, the direction (degrees) of the nearest road
/// centerline — multi-source BFS through the town carrying the angle.
pub(crate) fn nearest_road_angle(net: &RoadNetwork, ids: &[u8], w: usize, h: usize) -> Vec<f32> {
    let mut angle = vec![f32::NAN; w * h];
    let mut queue = VecDeque::new();
    for link in &net.links {
        for (&c, &(tx, ty)) in link.path.iter().zip(&path_tangents(&link.path, w)) {
            if angle[c].is_nan() {
                angle[c] = ty.atan2(tx).to_degrees();
                queue.push_back(c);
            }
        }
    }
    while let Some(c) = queue.pop_front() {
        let (x, y) = (c % w, c / w);
        let neighbors = [
            (x > 0).then(|| c - 1),
            (x + 1 < w).then(|| c + 1),
            (y > 0).then(|| c - w),
            (y + 1 < h).then(|| c + w),
        ];
        for n in neighbors.into_iter().flatten() {
            if ids[n] != 0 && angle[n].is_nan() {
                angle[n] = angle[c];
                queue.push_back(n);
            }
        }
    }
    for a in &mut angle {
        if a.is_nan() {
            *a = 0.0;
        }
    }
    angle
}

/// Chebyshev dilation: cells within `r` (8-neighborhood steps) of a set cell.
pub(crate) fn dilate(mask: &[bool], w: usize, h: usize, r: i64) -> Vec<bool> {
    // Separable max filter: rows, then columns.
    let mut rows = vec![false; w * h];
    for y in 0..h {
        let mut last: i64 = i64::MIN / 2;
        let mut next = vec![i64::MAX / 2; w];
        let mut upcoming: i64 = i64::MAX / 2;
        for x in (0..w).rev() {
            if mask[y * w + x] {
                upcoming = x as i64;
            }
            next[x] = upcoming;
        }
        for x in 0..w {
            if mask[y * w + x] {
                last = x as i64;
            }
            rows[y * w + x] = x as i64 - last <= r || next[x] - x as i64 <= r;
        }
    }
    let mut out = vec![false; w * h];
    for x in 0..w {
        let mut last: i64 = i64::MIN / 2;
        let mut next = vec![i64::MAX / 2; h];
        let mut upcoming: i64 = i64::MAX / 2;
        for y in (0..h).rev() {
            if rows[y * w + x] {
                upcoming = y as i64;
            }
            next[y] = upcoming;
        }
        for y in 0..h {
            if rows[y * w + x] {
                last = y as i64;
            }
            out[y * w + x] = y as i64 - last <= r || next[y] - y as i64 <= r;
        }
    }
    out
}

/// Cells within `r` 4-steps of a set cell (r = 0: the set cells themselves).
pub(crate) fn within(mask: &[bool], w: usize, h: usize, r: u32) -> Vec<bool> {
    let mut dist = vec![u32::MAX; w * h];
    let mut queue = VecDeque::new();
    for (i, &m) in mask.iter().enumerate() {
        if m {
            dist[i] = 0;
            queue.push_back(i);
        }
    }
    while let Some(c) = queue.pop_front() {
        if dist[c] >= r {
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

