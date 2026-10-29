//! Small 2D geometry helpers for the organic town layout: polygons, half-plane
//! clips, rotated rectangles. All points are `(x, y)` in meters with +y pointing
//! down the map (screen convention).

pub type Vec2 = (f32, f32);

pub fn add(a: Vec2, b: Vec2) -> Vec2 {
    (a.0 + b.0, a.1 + b.1)
}

pub fn sub(a: Vec2, b: Vec2) -> Vec2 {
    (a.0 - b.0, a.1 - b.1)
}

pub fn mul(a: Vec2, s: f32) -> Vec2 {
    (a.0 * s, a.1 * s)
}

pub fn dot(a: Vec2, b: Vec2) -> f32 {
    a.0 * b.0 + a.1 * b.1
}

pub fn cross(a: Vec2, b: Vec2) -> f32 {
    a.0 * b.1 - a.1 * b.0
}

pub fn length(a: Vec2) -> f32 {
    a.0.hypot(a.1)
}

pub fn dist(a: Vec2, b: Vec2) -> f32 {
    length(sub(a, b))
}

pub fn norm(a: Vec2) -> Vec2 {
    mul(a, 1.0 / length(a).max(1e-6))
}

pub fn perp(a: Vec2) -> Vec2 {
    (-a.1, a.0)
}

/// Shoelace area (absolute).
pub fn polygon_area(poly: &[Vec2]) -> f32 {
    let mut s = 0.0;
    for i in 0..poly.len() {
        s += cross(poly[i], poly[(i + 1) % poly.len()]);
    }
    (s * 0.5).abs()
}

/// Area-weighted centroid (falls back to the vertex mean for degenerate polys).
pub fn polygon_centroid(poly: &[Vec2]) -> Vec2 {
    let mut a = 0.0;
    let mut c = (0.0, 0.0);
    for i in 0..poly.len() {
        let p = poly[i];
        let q = poly[(i + 1) % poly.len()];
        let cr = cross(p, q);
        a += cr;
        c = add(c, mul(add(p, q), cr));
    }
    if a.abs() < 1e-9 {
        let n = poly.len().max(1) as f32;
        return mul(poly.iter().fold((0.0, 0.0), |s, p| add(s, *p)), 1.0 / n);
    }
    mul(c, 1.0 / (3.0 * a))
}

/// Ray-casting point-in-polygon.
pub fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
    let mut inside = false;
    let mut j = poly.len().saturating_sub(1);
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > p.1) != (yj > p.1)
            && p.0 < (xj - xi) * (p.1 - yi) / (yj - yi + 1e-12) + xi
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Split `poly` by the line `{ p : dot(p, n) = c }`. Returns `(dot <= c, dot > c)`
/// (Sutherland–Hodgman; exact for the near-star-shaped polygons used here).
pub fn clip_half_plane(poly: &[Vec2], n: Vec2, c: f32) -> (Vec<Vec2>, Vec<Vec2>) {
    let mut in_side = Vec::new();
    let mut out_side = Vec::new();
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let da = dot(a, n) - c;
        let db = dot(b, n) - c;
        if da <= 0.0 {
            in_side.push(a);
        } else {
            out_side.push(a);
        }
        if (da <= 0.0) != (db <= 0.0) {
            let t = da / (da - db);
            let p = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            in_side.push(p);
            out_side.push(p);
        }
    }
    (in_side, out_side)
}

/// Points where the line `{ dot(p, n) = c }` crosses the polygon edges, sorted
/// along the line direction. Consecutive pairs form the chords inside the polygon.
pub fn line_polygon_intersections(n: Vec2, c: f32, poly: &[Vec2]) -> Vec<Vec2> {
    let dir = perp(n);
    let mut pts = Vec::new();
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let da = dot(a, n) - c;
        let db = dot(b, n) - c;
        if (da < 0.0) != (db < 0.0) {
            let t = da / (da - db);
            pts.push((dot(dir, a) + t * dot(dir, sub(b, a)), add(a, mul(sub(b, a), t))));
        }
    }
    pts.sort_by(|x, y| x.0.total_cmp(&y.0));
    pts.into_iter().map(|(_, p)| p).collect()
}

/// Nearest intersection of the ray `origin + t * dir` (t > 0) with the polygon
/// boundary. The origin is expected to lie inside.
pub fn ray_polygon_hit(origin: Vec2, dir: Vec2, poly: &[Vec2]) -> Option<Vec2> {
    let mut best: Option<(f32, Vec2)> = None;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let e = sub(b, a);
        let denom = cross(dir, e);
        if denom.abs() < 1e-9 {
            continue;
        }
        let ao = sub(a, origin);
        let t = cross(ao, e) / denom;
        let u = cross(ao, dir) / denom;
        if t > 1e-6 && (-1e-6..=1.0 + 1e-6).contains(&u) && best.is_none_or(|(bt, _)| t < bt) {
            best = Some((t, add(origin, mul(dir, t))));
        }
    }
    best.map(|(_, p)| p)
}

/// Distance from `p` to segment `a-b`.
pub fn dist_point_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    if l2 < 1e-12 {
        return dist(p, a);
    }
    let t = (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0);
    dist(p, add(a, mul(ab, t)))
}

/// Corners of a rect `w x h` rotated by `angle_deg` around its center.
pub fn rot_rect_corners(center: Vec2, w: f32, h: f32, angle_deg: f32) -> [Vec2; 4] {
    let (s, c) = angle_deg.to_radians().sin_cos();
    let (hw, hh) = (w / 2.0, h / 2.0);
    [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)].map(|(lx, ly)| {
        (center.0 + lx * c - ly * s, center.1 + lx * s + ly * c)
    })
}

/// Separating-axis test: do two rotated rects (given by corners) overlap?
/// Touching edges count as separated (epsilon slack).
pub fn rotated_rects_overlap(a: &[Vec2; 4], b: &[Vec2; 4]) -> bool {
    const EPS: f32 = 1e-4;
    let axes = |r: &[Vec2; 4]| [norm(sub(r[1], r[0])), norm(sub(r[3], r[0]))];
    for axis in axes(a).into_iter().chain(axes(b)) {
        let proj = |r: &[Vec2; 4]| {
            let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
            for p in r {
                let d = dot(*p, axis);
                lo = lo.min(d);
                hi = hi.max(d);
            }
            (lo, hi)
        };
        let (alo, ahi) = proj(a);
        let (blo, bhi) = proj(b);
        if ahi < blo + EPS || bhi < alo + EPS {
            return false;
        }
    }
    true
}

/// Points along the quadratic bezier `p0 - ctrl - p1`, roughly `step_m` apart
/// (endpoints included).
pub fn quad_bezier(p0: Vec2, ctrl: Vec2, p1: Vec2, step_m: f32) -> Vec<Vec2> {
    let approx_len = dist(p0, ctrl) + dist(ctrl, p1);
    let steps = ((approx_len / step_m.max(0.1)).ceil() as usize).clamp(2, 200);
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let a = add(p0, mul(sub(ctrl, p0), t));
            let b = add(ctrl, mul(sub(p1, ctrl), t));
            add(a, mul(sub(b, a), t))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f32, y: f32, s: f32) -> Vec<Vec2> {
        vec![(x, y), (x + s, y), (x + s, y + s), (x, y + s)]
    }

    #[test]
    fn areas_and_containment() {
        let sq = square(0.0, 0.0, 10.0);
        assert!((polygon_area(&sq) - 100.0).abs() < 1e-4);
        assert!(point_in_polygon((5.0, 5.0), &sq));
        assert!(!point_in_polygon((15.0, 5.0), &sq));
        let c = polygon_centroid(&sq);
        assert!((c.0 - 5.0).abs() < 1e-3 && (c.1 - 5.0).abs() < 1e-3);
    }

    #[test]
    fn half_plane_clip_splits_area() {
        let sq = square(0.0, 0.0, 10.0);
        let (left, right) = clip_half_plane(&sq, (1.0, 0.0), 2.5);
        assert!((polygon_area(&left) - 25.0).abs() < 1e-3);
        assert!((polygon_area(&right) - 75.0).abs() < 1e-3);
    }

    #[test]
    fn chords_and_ray_hit() {
        let sq = square(0.0, 0.0, 10.0);
        let pts = line_polygon_intersections((1.0, 0.0), 5.0, &sq);
        assert_eq!(pts.len(), 2);
        assert!((pts[0].0 - 5.0).abs() < 1e-4 && pts[0].1.abs() < 1e-4);
        assert!((pts[1].0 - 5.0).abs() < 1e-4 && (pts[1].1 - 10.0).abs() < 1e-4);

        let hit = ray_polygon_hit((5.0, 5.0), (1.0, 0.0), &sq);
        assert!(hit.map(|p| (p.0 - 10.0).abs() < 1e-4 && (p.1 - 5.0).abs() < 1e-4).unwrap_or(false));
    }

    #[test]
    fn sat_detects_rotated_overlaps() {
        let a = rot_rect_corners((5.0, 5.0), 4.0, 2.0, 0.0);
        let b = rot_rect_corners((6.0, 5.0), 4.0, 2.0, 0.0);
        assert!(rotated_rects_overlap(&a, &b));
        // Parallel bars offset sideways: overlapping AABBs but no real overlap.
        let c = rot_rect_corners((5.0, 5.0), 4.0, 1.0, 45.0);
        let d = rot_rect_corners((5.0, 6.6), 4.0, 1.0, 45.0);
        assert!(!rotated_rects_overlap(&c, &d));
        // Touching edges do not count.
        let e = rot_rect_corners((5.0, 5.0), 2.0, 2.0, 0.0);
        let f = rot_rect_corners((7.0, 5.0), 2.0, 2.0, 0.0);
        assert!(!rotated_rects_overlap(&e, &f));
    }

    #[test]
    fn bezier_stays_between_endpoints() {
        let pts = quad_bezier((0.0, 0.0), (5.0, 3.0), (10.0, 0.0), 1.0);
        assert_eq!(pts.first().copied(), Some((0.0, 0.0)));
        assert_eq!(pts.last().copied(), Some((10.0, 0.0)));
        assert!(pts.iter().all(|p| p.1.abs() <= 1.6));
    }
}
