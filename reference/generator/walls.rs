use rand::Rng;

use crate::generator::geometry::{self, Vec2};
use crate::generator::grid::{Grid, WalkBudget};
use crate::generator::roads;
use crate::generator::sections;
use crate::generator::structures::Zone;
use crate::level::{Category, ObjectDetails, PlacedObject};
use crate::model::{LevelConfig, TemplateKind};

/// Boundary samples per blob polygon (also the perimeter wall segment count).
const BOUNDARY_STEPS: usize = 48;

/// The organic walled shape of one template zone: a smooth blob boundary (with a
/// few deep notches on big maps) that the perimeter wall follows.
#[derive(Debug, Clone)]
pub struct TownBlob {
    pub kind: TemplateKind,
    pub center: Vec2,
    pub boundary: Vec<Vec2>,
}

impl TownBlob {
    /// Boundary point where the ray from the center at `deg` (0° = east, 90° =
    /// north, counterclockwise) leaves the blob.
    pub fn boundary_point(&self, deg: f32) -> Vec2 {
        let rad = deg.to_radians();
        let dir = (rad.cos(), -rad.sin()); // screen y points down
        geometry::ray_polygon_hit(self.center, dir, &self.boundary)
            .unwrap_or(self.center)
    }
}

/// Build the organic town layout for every walled zone and return the blobs (so
/// the gate phase can place gates on the wall instead of the map border).
///
/// Phases per blob: block the ground outside -> perimeter wall along the boundary
/// -> gate plazas punched through -> sections with wall chords -> road web.
pub fn place(
    rng: &mut impl Rng,
    grid: &mut Grid,
    budget: &mut WalkBudget,
    zones: &[Zone],
    config: &LevelConfig,
    objects: &mut Vec<PlacedObject>,
    notes: &mut Vec<String>,
) -> Vec<TownBlob> {
    let mut blobs = Vec::new();
    for zone in zones.iter().filter(|z| config.walls.includes(z.kind)) {
        let Some(blob) = build_blob(rng, zone, config) else {
            notes.push(format!(
                "{}: zone is too small for a walled town shape, skipped",
                zone.kind
            ));
            continue;
        };
        block_outside(grid, zone, &blob);
        let t = snap_up(config.walls.thickness_m, grid.cell_m);
        build_perimeter(rng, grid, &blob, t, config, objects);
        carve_gate_plazas(grid, &blob, config, t);
        let built = sections::carve(rng, grid, &blob, t, config, objects);
        roads::build(
            rng,
            grid,
            zone,
            &blob,
            &built.sections,
            &built.gap_points,
            config,
            t,
            objects,
        );
        blobs.push(blob);
    }
    let _ = budget; // walls and roads are infrastructure: the budget only governs content
    blobs
}

/// Smooth polar-noise blob covering most of the zone, clamped to stay inside it.
/// Big maps get a few deep notches; the boundary is a closed polygon.
fn build_blob(rng: &mut impl Rng, zone: &Zone, config: &LevelConfig) -> Option<TownBlob> {
    let zr = zone.rect;
    if zr.w < 3.0 * config.walls.thickness_m || zr.h < 3.0 * config.walls.thickness_m {
        return None;
    }
    let center = (
        zr.x + zr.w * rng.random_range(0.42..0.58),
        zr.y + zr.h * rng.random_range(0.42..0.58),
    );
    // Base radius keeps the worst-case radius inside the zone.
    let r0 = 0.5 * zr.w.min(zr.h);
    let (a1, a2, a3) = (
        rng.random_range(0.05..0.11),
        rng.random_range(0.03..0.08),
        rng.random_range(0.02..0.06),
    );
    let (p1, p2, p3) = (
        rng.random_range(0.0..std::f32::consts::TAU),
        rng.random_range(0.0..std::f32::consts::TAU),
        rng.random_range(0.0..std::f32::consts::TAU),
    );

    // Deep gaps (notches) only when there is room for them.
    let min_dim = zr.w.min(zr.h);
    let notch_count = if min_dim >= 140.0 {
        rng.random_range(2..=3)
    } else if min_dim >= 80.0 {
        rng.random_range(1..=2)
    } else {
        0
    };
    let notches: Vec<(f32, f32, f32)> = (0..notch_count)
        .map(|_| {
            (
                rng.random_range(0.0..360.0),                      // center angle
                rng.random_range(4.0..9.0),                        // angular width (deg)
                rng.random_range(0.28..0.45),                      // depth
            )
        })
        .collect();

    let mut boundary = Vec::with_capacity(BOUNDARY_STEPS);
    for i in 0..BOUNDARY_STEPS {
        let deg = i as f32 * 360.0 / BOUNDARY_STEPS as f32;
        let rad = deg.to_radians();
        let mut r = 1.0
            + a1 * (rad + p1).sin()
            + a2 * (2.0 * rad + p2).sin()
            + a3 * (3.0 * rad + p3).sin();
        for &(nc, nw, nd) in &notches {
            let d = (deg - nc).abs().min(360.0 - (deg - nc).abs());
            r *= 1.0 - nd * (-0.5 * (d / nw).powi(2)).exp();
        }
        r = r.max(0.15);
        // Clamp the point so the blob never leaves the zone rect.
        let dir = (rad.cos(), -rad.sin());
        let mut t_max = f32::INFINITY;
        for (dc, span) in [
            (dir.0, (if dir.0 > 0.0 { zr.x + zr.w - center.0 } else { center.0 - zr.x }).abs()),
            (dir.1, (if dir.1 > 0.0 { zr.y + zr.h - center.1 } else { center.1 - zr.y }).abs()),
        ] {
            if dc.abs() > 1e-6 {
                t_max = t_max.min(span / dc.abs());
            }
        }
        let r_m = (r * r0).min(t_max * 0.985).max(0.5);
        boundary.push((center.0 + dir.0 * r_m, center.1 + dir.1 * r_m));
    }
    Some(TownBlob {
        kind: zone.kind,
        center,
        boundary,
    })
}

/// Mark ground outside the blob — but only within this zone — as blocked: the
/// rest of the map belongs to the other template in a mix and stays untouched.
fn block_outside(grid: &mut Grid, zone: &Zone, blob: &TownBlob) {
    let (c0, r0, c1, r1) = grid.cell_range(zone.rect);
    for r in r0..r1 {
        for c in c0..c1 {
            let p = ((c as f32 + 0.5) * grid.cell_m, (r as f32 + 0.5) * grid.cell_m);
            if !geometry::point_in_polygon(p, &blob.boundary) {
                grid.mark_cell(c, r);
            }
        }
    }
}

/// Solid wall strip (rotated rect) along the segment `a-b`, pushed `t` deep
/// inside the blob and extended by `t` at both ends so joints seal.
#[allow(clippy::too_many_arguments)]
pub(crate) fn wall_strip(
    grid: &mut Grid,
    a: Vec2,
    b: Vec2,
    inward: Vec2,
    t: f32,
    kind: TemplateKind,
    perimeter: bool,
    color: [u8; 3],
    objects: &mut Vec<PlacedObject>,
) {
    let dir = geometry::norm(geometry::sub(b, a));
    let n = {
        let p = geometry::perp(dir);
        if geometry::dot(p, inward) >= 0.0 {
            p
        } else {
            geometry::mul(p, -1.0)
        }
    };
    let len = geometry::dist(a, b) + t;
    let mid = geometry::add(
        geometry::mul(geometry::add(a, b), 0.5),
        geometry::mul(n, t / 2.0),
    );
    let angle = dir.1.atan2(dir.0).to_degrees();
    grid.mark_rotated(mid, len, t, angle);
    objects.push(PlacedObject {
        category: Category::Wall,
        type_name: if perimeter {
            "Perimeter wall".to_owned()
        } else {
            "Section wall".to_owned()
        },
        color,
        x_m: mid.0 - len / 2.0,
        y_m: mid.1 - t / 2.0,
        w_m: len,
        h_m: t,
        angle_deg: angle,
        details: ObjectDetails::Wall {
            enclosed: kind,
            perimeter,
        },
    });
}

/// Perimeter wall: one strip per boundary edge, hugging the blob boundary.
#[allow(clippy::too_many_arguments)]
fn build_perimeter(
    rng: &mut impl Rng,
    grid: &mut Grid,
    blob: &TownBlob,
    t: f32,
    config: &LevelConfig,
    objects: &mut Vec<PlacedObject>,
) {
    let _ = rng;
    let n = blob.boundary.len();
    for i in 0..n {
        let a = blob.boundary[i];
        let b = blob.boundary[(i + 1) % n];
        wall_strip(
            grid,
            a,
            b,
            geometry::sub(blob.center, geometry::mul(geometry::add(a, b), 0.5)),
            t,
            blob.kind,
            true,
            config.walls.color,
            objects,
        );
    }
}

/// Punch a walkable plaza through the wall at every gate point.
fn carve_gate_plazas(grid: &mut Grid, blob: &TownBlob, config: &LevelConfig, t: f32) {
    let radius = (config.walls.gate_width_m.max(2.0 * t) / 2.0 + t * 0.5).max(1.5);
    for gate in &config.gates {
        let p = blob.boundary_point(gate.direction_deg);
        stamp_disc(grid, p, radius);
    }
}

/// Mark every cell within `radius` of `p` as road (open, walkable, no building).
pub(crate) fn stamp_disc(grid: &mut Grid, p: Vec2, radius: f32) {
    let (c0, r0, c1, r1) = (
        ((p.0 - radius) / grid.cell_m).floor().max(0.0) as usize,
        ((p.1 - radius) / grid.cell_m).floor().max(0.0) as usize,
        (((p.0 + radius) / grid.cell_m).ceil() as usize).min(grid.cols),
        (((p.1 + radius) / grid.cell_m).ceil() as usize).min(grid.rows),
    );
    for r in r0..r1 {
        for c in c0..c1 {
            let q = ((c as f32 + 0.5) * grid.cell_m, (r as f32 + 0.5) * grid.cell_m);
            if geometry::dist(q, p) <= radius {
                grid.mark_road_cell(c, r);
            }
        }
    }
}

pub(crate) fn snap_up(len_m: f32, cell: f32) -> f32 {
    (len_m / cell).ceil().max(1.0) * cell
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::structures::build_zones;
    use crate::model::TemplateMix;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    struct Town {
        grid: Grid,
        objects: Vec<PlacedObject>,
        blob: TownBlob,
    }

    fn run_town(config: &LevelConfig, seed: u64) -> Town {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut grid = Grid::new(config.map.width_m, config.map.height_m, config.cell_size_m);
        let mut budget = WalkBudget::new(grid.total_cells(), config.map.walkable_pct);
        let mut objects = Vec::new();
        let mut notes = Vec::new();
        let zones = build_zones(&mut rng, config);
        let blobs = place(&mut rng, &mut grid, &mut budget, &zones, config, &mut objects, &mut notes);
        Town {
            grid,
            objects,
            blob: blobs.into_iter().next().expect("town blob"),
        }
    }

    #[test]
    fn town_is_a_walled_blob_with_sections_and_roads() {
        let config = LevelConfig::default();
        let town = run_town(&config, 5);

        let perimeter = town
            .objects
            .iter()
            .filter(|o| matches!(&o.details, ObjectDetails::Wall { perimeter: true, .. }))
            .count();
        assert!(perimeter >= 20, "perimeter strips: {perimeter}");
        let section_walls = town
            .objects
            .iter()
            .filter(|o| matches!(&o.details, ObjectDetails::Wall { perimeter: false, .. }))
            .count();
        assert!(section_walls >= 1, "section walls: {section_walls}");
        let road_segments = town
            .objects
            .iter()
            .filter(|o| o.category == Category::Road)
            .count();
        assert!(road_segments >= 5, "road segments: {road_segments}");

        // Ground outside the blob is blocked, the blob interior is walkable.
        let cell = config.cell_size_m;
        assert!(!town.grid.is_free(0, 0), "map corner must be blocked");
        let mid = (
            (config.map.width_m / 2.0 / cell) as usize,
            (config.map.height_m / 2.0 / cell) as usize,
        );
        assert!(
            town.grid.is_free(mid.0, mid.1) || town.grid.is_road(mid.0, mid.1),
            "blob interior must be open ground"
        );

        // Everything is one connected walkable region after the road web.
        assert_eq!(town.grid.connected_walkable_components(1), 1);
    }

    #[test]
    fn gate_plaza_pierces_the_perimeter_wall() {
        let config = LevelConfig::default(); // one gate at 90° = north
        let town = run_town(&config, 6);
        let p = town.blob.boundary_point(90.0);
        let cell = config.cell_size_m;
        let c = (p.0 / cell) as usize;
        let r = (p.1 / cell) as usize;
        // A road cell sits within a couple of cells of the boundary point.
        let found = (r.saturating_sub(3)..=(r + 3).min(town.grid.rows - 1)).any(|rr| {
            (c.saturating_sub(3)..=(c + 3).min(town.grid.cols - 1))
                .any(|cc| town.grid.is_road(cc, rr))
        });
        assert!(found, "no road cell near the gate point {p:?}");
    }

    #[test]
    fn blob_shape_is_not_a_rectangle() {
        let config = LevelConfig::default();
        let town = run_town(&config, 7);
        // Radii from the center vary along the boundary (organic, not circular
        // either: low-frequency wobble shifts individual radii apart).
        let radii: Vec<f32> = town
            .blob
            .boundary
            .iter()
            .map(|p| geometry::dist(*p, town.blob.center))
            .collect();
        let min = radii.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = radii.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(min > 0.5, "degenerate radius {min}");
        assert!(
            (max - min) / max > 0.05,
            "boundary looks like a perfect circle: {min}..{max}"
        );
    }

    #[test]
    fn open_templates_get_no_town() {
        let config = LevelConfig {
            template_mix: TemplateMix::Single(TemplateKind::Forest),
            ..LevelConfig::default()
        };
        let mut rng = ChaCha8Rng::seed_from_u64(8);
        let mut grid = Grid::new(config.map.width_m, config.map.height_m, config.cell_size_m);
        let mut budget = WalkBudget::new(grid.total_cells(), config.map.walkable_pct);
        let mut objects = Vec::new();
        let mut notes = Vec::new();
        let zones = build_zones(&mut rng, &config);
        let blobs = place(&mut rng, &mut grid, &mut budget, &zones, &config, &mut objects, &mut notes);
        assert!(blobs.is_empty() && objects.is_empty());
        assert!((grid.free_ratio() - 1.0).abs() < 1e-6, "forest must stay fully open");
    }
}
