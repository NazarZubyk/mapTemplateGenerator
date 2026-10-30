//! Swamp node 2, gate anchors (step S1 in docs/swamp-plan.md): where each
//! gate meets the map border — the swamp's way out to "the big land". The
//! later swamp nodes build the islands and bridges around these anchors.
//!
//! 1. **Edge point** — a ray from the map center at the gate's direction
//!    (0° = east, 90° = north, counterclockwise on screen) to the map edge.
//! 2. **Corner snap** — a direction within `corner_snap_deg` of a map
//!    corner (as seen from the center) moves the gate into that corner.
//! 3. **Land** — every gate gets an island, diameter rolled in
//!    `island_min_m..island_max_m`:
//!    - *shore*: its center half a radius in from the edge (from both edges
//!      in a corner), so the island reaches over the border;
//!    - *bridge*: a bridge length rolled in `bridge_min_m..bridge_max_m`;
//!      the island sits that far in from the edge, the bridge (built by the
//!      paths node) runs from its shore out to the gate point.
//!
//! "In" is along the edge's inward normal, or toward the map center for a
//! corner. The gate marker is the only thing drawn; the land point stays in
//! the data until the islands node builds the island.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::gates::{GATE_NAME, MARKER_M};
use crate::level::{Category, GateAnchor, PlacedObject};
use crate::model::{GateStyle, LevelConfig};

pub struct AnchorBuild {
    pub anchors: Vec<GateAnchor>,
    /// One marker per gate; `anchors[..].object` indexes here.
    pub objects: Vec<PlacedObject>,
    pub notes: Vec<String>,
}

/// Largest gate island and longest bridge, as a share of the shorter map side.
const MAX_ISLAND_SHARE: f32 = 0.4;
const MAX_REACH_SHARE: f32 = 0.4;

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng) -> AnchorBuild {
    let s = &config.swamp.gate_anchors;
    let (w, h) = (config.map.width_m.max(1.0), config.map.height_m.max(1.0));
    // The marker stays wholly on the map.
    let inset = (config.cell_size_m.max(0.05) / 2.0).max(MARKER_M / 2.0);
    let center = (w / 2.0, h / 2.0);
    let side = w.min(h);
    let mut anchors = Vec::new();
    let mut objects = Vec::new();
    let mut notes = Vec::new();

    for (i, spec) in config.gates.gates.iter().enumerate() {
        let id = i as u32 + 1;
        let deg = spec.direction_deg.rem_euclid(360.0);
        let corner = (s.corner_snap_deg > 0.0)
            .then(|| nearest_corner(center, deg, w, h))
            .flatten()
            .filter(|c| angle_diff(c.1, deg) <= s.corner_snap_deg);
        let (point, inward, actual) = match corner {
            Some((p, a)) => {
                let p = (p.0.clamp(inset, w - inset), p.1.clamp(inset, h - inset));
                let (dx, dy) = (center.0 - p.0, center.1 - p.1);
                let len = dx.hypot(dy).max(1e-6);
                (p, (dx / len, dy / len), a)
            }
            None => {
                let (p, n) = edge_hit(center, deg, w, h);
                ((p.0.clamp(inset, w - inset), p.1.clamp(inset, h - inset)), n, deg)
            }
        };

        let mut island = roll(rng, s.island_min_m, s.island_max_m).max(1.0);
        if island > side * MAX_ISLAND_SHARE {
            island = side * MAX_ISLAND_SHARE;
            notes.push(format!("Gate #{id}: gate island shrunk to {island:.1} m to fit the map"));
        }
        let r = island / 2.0;
        let bridge = match spec.style {
            GateStyle::Shore => 0.0,
            GateStyle::Bridge => {
                let rolled = roll(rng, s.bridge_min_m, s.bridge_max_m).max(0.0);
                let max = (side * MAX_REACH_SHARE - r).max(0.0);
                if rolled > max {
                    notes.push(format!("Gate #{id}: bridge shortened to {max:.1} m to fit the map"));
                }
                rolled.min(max)
            }
        };
        let land = match (spec.style, corner.is_some()) {
            // Half a radius in from both edges: the island covers the corner.
            (GateStyle::Shore, true) => (point.0 + inward.0.signum() * r / 2.0, point.1 + inward.1.signum() * r / 2.0),
            (GateStyle::Shore, false) => (point.0 + inward.0 * r / 2.0, point.1 + inward.1 * r / 2.0),
            (GateStyle::Bridge, _) => (point.0 + inward.0 * (bridge + r), point.1 + inward.1 * (bridge + r)),
        };

        objects.push(PlacedObject {
            category: Category::Gate,
            type_name: GATE_NAME.to_owned(),
            color: config.gates.color,
            x_m: point.0 - MARKER_M / 2.0,
            y_m: point.1 - MARKER_M / 2.0,
            w_m: MARKER_M,
            h_m: MARKER_M,
            angle_deg: 0.0,
            parts: Vec::new(),
            round: false,
        });
        anchors.push(GateAnchor {
            id,
            direction_deg: spec.direction_deg,
            actual_deg: actual,
            style: spec.style,
            corner: corner.is_some(),
            point_m: [point.0, point.1],
            land_m: [land.0, land.1],
            island_m: island,
            bridge_m: bridge,
            object: objects.len() - 1,
        });
    }

    for (a, x) in anchors.iter().enumerate() {
        for y in &anchors[a + 1..] {
            let d = (x.land_m[0] - y.land_m[0]).hypot(x.land_m[1] - y.land_m[1]);
            if d < (x.island_m + y.island_m) / 2.0 {
                notes.push(format!(
                    "Gates #{} and #{}: their islands overlap and squeeze each other — spread the directions",
                    x.id, y.id
                ));
            }
        }
    }
    AnchorBuild { anchors, objects, notes }
}

/// Uniform in `a..=b` (either order); `a` when they are equal.
fn roll(rng: &mut ChaCha8Rng, a: f32, b: f32) -> f32 {
    let (lo, hi) = (a.min(b), a.max(b));
    if hi - lo < 1e-6 { lo } else { rng.random_range(lo..=hi) }
}

/// Where the ray from `center` at `deg` leaves the `w`×`h` map, and that
/// edge's inward normal.
pub(crate) fn edge_hit(center: (f32, f32), deg: f32, w: f32, h: f32) -> ((f32, f32), (f32, f32)) {
    let (dx, dy) = (deg.to_radians().cos(), -deg.to_radians().sin());
    let t_for = |d: f32, c: f32, size: f32| {
        if d > 1e-6 {
            (size - c) / d
        } else if d < -1e-6 {
            -c / d
        } else {
            f32::INFINITY
        }
    };
    let (tx, ty) = (t_for(dx, center.0, w), t_for(dy, center.1, h));
    let t = tx.min(ty);
    let p = (center.0 + dx * t, center.1 + dy * t);
    let n = if tx <= ty { (-dx.signum(), 0.0) } else { (0.0, -dy.signum()) };
    (p, n)
}

/// The map corner whose direction from the center is closest to `deg`:
/// (corner point, its direction in degrees).
fn nearest_corner(center: (f32, f32), deg: f32, w: f32, h: f32) -> Option<((f32, f32), f32)> {
    [(w, 0.0), (0.0, 0.0), (0.0, h), (w, h)]
        .into_iter()
        .map(|p| (p, (center.1 - p.1).atan2(p.0 - center.0).to_degrees().rem_euclid(360.0)))
        .min_by(|a, b| angle_diff(a.1, deg).total_cmp(&angle_diff(b.1, deg)))
}

/// Smallest difference between two directions, degrees (0..=180).
fn angle_diff(a: f32, b: f32) -> f32 {
    (((a - b).rem_euclid(360.0) + 180.0).rem_euclid(360.0) - 180.0).abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::model::{GateAnchorSettings, GateSpec, MapKind};

    fn swamp(dirs: &[(f32, GateStyle)]) -> LevelConfig {
        let mut config = LevelConfig { kind: MapKind::Swamp, ..LevelConfig::default() };
        config.map.width_m = 100.0;
        config.map.height_m = 60.0;
        config.gates.gates = dirs.iter().map(|&(d, style)| GateSpec { direction_deg: d, style }).collect();
        config.swamp.gate_anchors =
            GateAnchorSettings { corner_snap_deg: 20.0, island_min_m: 10.0, island_max_m: 16.0, bridge_min_m: 8.0, bridge_max_m: 14.0 };
        config
    }

    fn anchors(config: &LevelConfig) -> AnchorBuild {
        build(config, &mut ChaCha8Rng::seed_from_u64(5))
    }

    #[test]
    fn angle_diff_wraps() {
        assert!((angle_diff(350.0, 10.0) - 20.0).abs() < 1e-4);
        assert!((angle_diff(10.0, 350.0) - 20.0).abs() < 1e-4);
        assert!((angle_diff(90.0, 270.0) - 180.0).abs() < 1e-4);
    }

    #[test]
    fn gates_sit_on_the_edge_they_point_at() {
        let mut config = swamp(&[(0.0, GateStyle::Shore), (90.0, GateStyle::Shore), (180.0, GateStyle::Shore), (270.0, GateStyle::Shore)]);
        config.swamp.gate_anchors.corner_snap_deg = 0.0;
        let b = anchors(&config);
        let p: Vec<[f32; 2]> = b.anchors.iter().map(|a| a.point_m).collect();
        let near = |a: f32, b: f32| (a - b).abs() <= MARKER_M / 2.0 + 1e-3;
        assert!(near(p[0][0], 100.0) && near(p[0][1], 30.0), "east {:?}", p[0]);
        assert!(near(p[1][0], 50.0) && near(p[1][1], 0.0), "north {:?}", p[1]);
        assert!(near(p[2][0], 0.0) && near(p[2][1], 30.0), "west {:?}", p[2]);
        assert!(near(p[3][0], 50.0) && near(p[3][1], 60.0), "south {:?}", p[3]);
        assert_eq!(b.objects.len(), 4);
        assert!(b.objects.iter().all(|o| o.category == Category::Gate));
    }

    #[test]
    fn near_corner_directions_snap_into_the_corner() {
        // 100×60: the north-east corner is at atan(30/50) ≈ 31°.
        let config = swamp(&[(40.0, GateStyle::Shore), (90.0, GateStyle::Shore)]);
        let b = anchors(&config);
        let ne = &b.anchors[0];
        assert!(ne.corner && (ne.actual_deg - 30.96).abs() < 0.1, "{ne:?}");
        assert!(ne.point_m[0] > 99.0 && ne.point_m[1] < 1.0, "{ne:?}");
        assert!(!b.anchors[1].corner);
        let mut off = config.clone();
        off.swamp.gate_anchors.corner_snap_deg = 5.0;
        assert!(!anchors(&off).anchors[0].corner, "40° is 9° from the corner");
    }

    #[test]
    fn shore_islands_reach_over_the_border() {
        let config = swamp(&[(0.0, GateStyle::Shore), (35.0, GateStyle::Shore), (200.0, GateStyle::Shore)]);
        for a in anchors(&config).anchors {
            let r = a.island_m / 2.0;
            let [x, y] = a.land_m;
            let to_edge = x.min(100.0 - x).min(y).min(60.0 - y);
            assert!(to_edge < r, "gate #{} island does not touch the border: {a:?}", a.id);
            assert!((10.0..=16.0).contains(&a.island_m) && a.bridge_m == 0.0);
        }
    }

    #[test]
    fn bridge_islands_sit_inland_at_bridge_length() {
        let config = swamp(&[(0.0, GateStyle::Bridge), (90.0, GateStyle::Bridge), (225.0, GateStyle::Bridge)]);
        for a in anchors(&config).anchors {
            let r = a.island_m / 2.0;
            let [x, y] = a.land_m;
            let to_edge = x.min(100.0 - x).min(y).min(60.0 - y);
            assert!(to_edge >= r, "gate #{} bridge island touches the border: {a:?}", a.id);
            assert!((8.0..=14.0).contains(&a.bridge_m), "{a:?}");
            let d = (x - a.point_m[0]).hypot(y - a.point_m[1]);
            assert!((d - (a.bridge_m + r)).abs() < 0.01, "{a:?}");
        }
    }

    #[test]
    fn oversized_settings_are_clamped_with_a_note() {
        let mut config = swamp(&[(90.0, GateStyle::Bridge)]);
        config.swamp.gate_anchors.island_min_m = 50.0;
        config.swamp.gate_anchors.island_max_m = 50.0;
        config.swamp.gate_anchors.bridge_min_m = 80.0;
        config.swamp.gate_anchors.bridge_max_m = 80.0;
        let b = anchors(&config);
        let a = &b.anchors[0];
        assert!(a.island_m <= 24.0 + 1e-3 && a.bridge_m + a.island_m / 2.0 <= 24.0 + 1e-3, "{a:?}");
        assert_eq!(b.notes.len(), 2, "{:?}", b.notes);
    }

    #[test]
    fn close_gates_get_an_overlap_note() {
        let config = swamp(&[(85.0, GateStyle::Shore), (95.0, GateStyle::Shore)]);
        assert_eq!(anchors(&config).notes.len(), 1);
        let apart = swamp(&[(90.0, GateStyle::Shore), (270.0, GateStyle::Shore)]);
        assert!(anchors(&apart).notes.is_empty());
    }

    #[test]
    fn swamp_levels_use_the_swamp_chain() {
        let config = swamp(&[(0.0, GateStyle::Shore), (180.0, GateStyle::Bridge)]);
        let level = crate::generator::generate(&config, 3);
        let s = level.swamp.as_ref().expect("swamp layout");
        assert_eq!(s.gate_anchors.len(), 2);
        assert!(level.regions.is_none() && level.shape.is_none());
        assert_eq!(level.objects.iter().filter(|o| o.category == Category::Gate).count(), 2);
        for a in &s.gate_anchors {
            assert_eq!(level.objects[a.object].category, Category::Gate, "anchor indexes its marker");
        }
        assert_eq!(level, crate::generator::generate(&config, 3));
        assert_ne!(level.swamp, crate::generator::generate(&config, 4).swamp, "seed rolls the sizes");
    }
}
