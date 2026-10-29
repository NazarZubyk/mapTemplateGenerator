use rand::Rng;

use crate::generator::geometry::{self, Vec2};
use crate::generator::grid::Grid;
use crate::generator::walls::{snap_up, wall_strip, TownBlob};
use crate::level::{PlacedObject};
use crate::model::LevelConfig;

/// Result of the section partition: the final section polygons and the midpoint
/// of every gap left in a section wall (road-web nodes).
pub struct Built {
    pub sections: Vec<Vec<Vec2>>,
    pub gap_points: Vec<Vec2>,
}

/// How many sections a blob of `area` m² gets: more area, more sections.
fn section_count(area: f32) -> usize {
    let k = 2.0 + (area / 700.0).log2();
    (k.round() as usize).clamp(1, 8)
}

/// Partition the blob interior into sections with strictly decreasing areas
/// (random geometric ratio), then wall every cut chord — each chord gets one
/// street-wide gap for the road web. Sections never overlap: every split takes
/// its area out of the piece it cuts.
pub fn carve(
    rng: &mut impl Rng,
    grid: &mut Grid,
    blob: &TownBlob,
    t: f32,
    config: &LevelConfig,
    objects: &mut Vec<PlacedObject>,
) -> Built {
    let mut built = Built {
        sections: Vec::new(),
        gap_points: Vec::new(),
    };
    // Interior polygon: boundary pushed inward so walls don't double up.
    let centroid = geometry::polygon_centroid(&blob.boundary);
    let interior: Vec<Vec2> = blob
        .boundary
        .iter()
        .map(|p| {
            let toward = geometry::norm(geometry::sub(centroid, *p));
            geometry::add(*p, geometry::mul(toward, t * 1.5))
        })
        .collect();
    if geometry::polygon_area(&interior) < config.walls.min_block_m * config.walls.min_block_m {
        built.sections.push(interior);
        return built;
    }

    let k = section_count(geometry::polygon_area(&interior));
    let ratio: f32 = rng.random_range(0.6..0.85);
    let weights: Vec<f32> = (0..k).map(|i| ratio.powi(i as i32)).collect();

    // Recursive split: a piece owning weight indices [s..e) is cut so the first
    // index gets its share; both halves recurse. Fully random orientation and
    // offset => sections end up beside the outer wall or enclosed by others.
    let mut stack = vec![(interior, 0usize, k)];
    while let Some((poly, s, e)) = stack.pop() {
        if e - s == 1 || geometry::polygon_area(&poly) < config.walls.min_block_m * 3.0 {
            built.sections.push(poly);
            continue;
        }
        let total: f32 = weights[s..e].iter().sum();
        let fraction = (weights[s] / total).clamp(0.15, 0.85);
        let (piece, rest, chord) = split_polygon(rng, &poly, fraction);
        if let Some((chord_a, chord_b)) = chord {
            wall_chord(rng, grid, blob, chord_a, chord_b, t, config, objects, &mut built.gap_points);
        }
        stack.push((rest, s + 1, e));
        stack.push((piece, s, s + 1));
    }
    built
}

/// Cut `poly` into (piece, rest) where `piece` holds `fraction` of the area.
/// Returns the longest chord of the cut line for the section wall.
fn split_polygon(
    rng: &mut impl Rng,
    poly: &[Vec2],
    fraction: f32,
) -> (Vec<Vec2>, Vec<Vec2>, Option<(Vec2, Vec2)>) {
    let area = geometry::polygon_area(poly).max(1e-6);
    let angle = rng.random_range(0.0..std::f32::consts::TAU);
    let n = (angle.cos(), angle.sin());
    let mut projections: Vec<f32> = poly.iter().map(|p| geometry::dot(*p, n)).collect();
    projections.sort_by(f32::total_cmp);
    let (mut lo, mut hi) = (projections[0], projections[projections.len() - 1]);
    let target = area * fraction;
    // Binary search the cut offset so the inside piece hits the target area.
    for _ in 0..34 {
        let c = (lo + hi) / 2.0;
        let (in_side, _) = geometry::clip_half_plane(poly, n, c);
        if geometry::polygon_area(&in_side) < target {
            lo = c;
        } else {
            hi = c;
        }
    }
    let (piece, rest) = geometry::clip_half_plane(poly, n, (lo + hi) / 2.0);
    let chord = geometry::line_polygon_intersections(n, (lo + hi) / 2.0, poly);
    let longest = chord
        .chunks(2)
        .filter(|p| p.len() == 2)
        .max_by(|a, b| geometry::dist(a[0], a[1]).total_cmp(&geometry::dist(b[0], b[1])))
        .map(|p| (p[0], p[1]));
    (piece, rest, longest)
}

/// Wall along a cut chord with one street-wide gap; the gap midpoint becomes a
/// road-web node.
#[allow(clippy::too_many_arguments)]
fn wall_chord(
    rng: &mut impl Rng,
    grid: &mut Grid,
    blob: &TownBlob,
    a: Vec2,
    b: Vec2,
    t: f32,
    config: &LevelConfig,
    objects: &mut Vec<PlacedObject>,
    gap_points: &mut Vec<Vec2>,
) {
    let len = geometry::dist(a, b);
    let gap = snap_up(config.walls.street_width_m, grid.cell_m);
    let inward = geometry::sub(blob.center, geometry::mul(geometry::add(a, b), 0.5));
    if len <= gap + 2.0 * t {
        // Too short for a gap: leave the chord open (acts as a full passage).
        return;
    }
    let gap_start = rng.random_range(0.2..0.8) * (len - gap);
    let (a1, b1) = (a, point_at(a, b, gap_start));
    let (a2, b2) = (point_at(a, b, gap_start + gap), b);
    for (ca, cb) in [(a1, b1), (a2, b2)] {
        if geometry::dist(ca, cb) > 1e-3 {
            wall_strip(grid, ca, cb, inward, t, blob.kind, false, config.walls.color, objects);
        }
    }
    let mid = point_at(a, b, gap_start + gap / 2.0);
    crate::generator::walls::stamp_disc(grid, mid, gap / 2.0 + t * 0.5);
    gap_points.push(mid);
}

/// Point `d` meters from `a` toward `b`.
fn point_at(a: Vec2, b: Vec2, d: f32) -> Vec2 {
    let dir = geometry::norm(geometry::sub(b, a));
    geometry::add(a, geometry::mul(dir, d))
}
