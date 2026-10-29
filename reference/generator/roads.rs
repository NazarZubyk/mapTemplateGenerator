use rand::Rng;

use crate::generator::geometry::{self, Vec2};
use crate::generator::grid::Grid;
use crate::generator::structures::Zone;
use crate::generator::walls::{stamp_disc, snap_up, TownBlob};
use crate::level::{Category, ObjectDetails, PlacedObject};
use crate::model::LevelConfig;

/// Distance between road samples (curves are stamped as short thick strips).
const SAMPLE_STEP_M: f32 = 1.5;
/// Extra crossroad links beyond the spanning tree, for a webbed look.
const EXTRA_EDGES: usize = 3;

/// Build the road web: nodes at the gates, at every section-wall gap and at one
/// random crossroad inside each section, connected by a minimum spanning tree of
/// gently curved roads (plus a few extra links). Roads are stamped into the grid
/// as open walkable cells that buildings must avoid; props may stand on them.
/// In a mix, one road continues from inside the town through the wall to the
/// zone edge so the walled town connects to the neighboring template.
#[allow(clippy::too_many_arguments)]
pub fn build(
    rng: &mut impl Rng,
    grid: &mut Grid,
    zone: &Zone,
    blob: &TownBlob,
    sections: &[Vec<Vec2>],
    gap_points: &[Vec2],
    config: &LevelConfig,
    t: f32,
    objects: &mut Vec<PlacedObject>,
) {
    let road_w = snap_up(config.walls.street_width_m, grid.cell_m);
    let mut nodes: Vec<Vec2> = Vec::new();

    // Gate nodes on the perimeter wall.
    for gate in &config.gates {
        push_node(&mut nodes, blob.boundary_point(gate.direction_deg));
    }
    // Section-wall gap midpoints.
    for p in gap_points {
        push_node(&mut nodes, *p);
    }
    // One crossroad per section, on free ground inside it.
    for section in sections {
        if let Some(p) = interior_crossroad(rng, grid, section) {
            push_node(&mut nodes, p);
        }
    }

    // Road(s) leading out of town: from inside the wall to the zone edge, when
    // that edge borders another zone (a mix) instead of the map.
    for exit in outward_exits(rng, zone, blob, config, t) {
        let samples = geometry::quad_bezier(exit.0, exit.1, exit.2, SAMPLE_STEP_M);
        for w in samples.windows(2) {
            stamp_road(grid, Some(w[0]), Some(w[1]), road_w, config, objects);
        }
        nodes.push(exit.2);
    }

    if nodes.len() < 2 {
        stamp_road(grid, nodes.first().copied(), nodes.first().copied(), road_w, config, objects);
        return;
    }

    let mut edges = spanning_tree(&nodes);
    add_extra_links(rng, &nodes, &mut edges);

    for (a, b) in edges {
        let p0 = nodes[a];
        let p1 = nodes[b];
        // Gentle curve: control point pushed sideways by a modest fraction.
        let mid = geometry::mul(geometry::add(p0, p1), 0.5);
        let dir = geometry::norm(geometry::sub(p1, p0));
        let bend = geometry::mul(
            geometry::perp(dir),
            geometry::dist(p0, p1) * rng.random_range(-0.18..0.18),
        );
        let ctrl = geometry::add(mid, bend);
        let samples = geometry::quad_bezier(p0, ctrl, p1, SAMPLE_STEP_M);
        for w in samples.windows(2) {
            stamp_road(grid, Some(w[0]), Some(w[1]), road_w, config, objects);
        }
    }
    let _ = t; // plazas and gap reserves were carved by the earlier phases
}

/// (boundary point, bezier control, zone-edge exit) for one outward road, when
/// the zone borders another zone (a mix) instead of covering the whole map. The
/// road runs from the blob boundary to a random point on the interior zone edge,
/// carving the perimeter wall where it passes.
fn outward_exits(
    rng: &mut impl Rng,
    zone: &Zone,
    blob: &TownBlob,
    config: &LevelConfig,
    t: f32,
) -> Vec<(Vec2, Vec2, Vec2)> {
    let zr = zone.rect;
    let eps = 0.5;
    let whole_map = zr.x <= eps
        && zr.y <= eps
        && zr.x + zr.w >= config.map.width_m - eps
        && zr.y + zr.h >= config.map.height_m - eps;
    if whole_map {
        return Vec::new(); // a single-template town needs no road to a neighbor
    }
    // Interior edges of the zone (those not on the map border), as
    // (fixed axis value, jitter range over the other axis, horizontal?).
    let margin = 3.0;
    let mut edges: Vec<(f32, f32, f32, bool)> = Vec::new(); // (fixed, lo, hi, horizontal_edge)
    if zr.x > eps {
        edges.push((zr.x, zr.y + margin, zr.y + zr.h - margin, false));
    }
    if zr.x + zr.w < config.map.width_m - eps {
        edges.push((zr.x + zr.w, zr.y + margin, zr.y + zr.h - margin, false));
    }
    if zr.y > eps {
        edges.push((zr.y, zr.x + margin, zr.x + zr.w - margin, true));
    }
    if zr.y + zr.h < config.map.height_m - eps {
        edges.push((zr.y + zr.h, zr.x + margin, zr.x + zr.w - margin, true));
    }
    let Some(&(fixed, lo, hi, horizontal)) = edges.first() else {
        return Vec::new();
    };
    let along = rng.random_range(lo..hi.max(lo));
    let exit = if horizontal {
        (along, fixed)
    } else {
        (fixed, along)
    };
    // Start INSIDE the wall band (offset inward from the boundary) so the road
    // pierces the perimeter wall, and run slightly past the zone edge so the
    // cells there touch the neighbor zone's open ground.
    let start = *blob
        .boundary
        .iter()
        .min_by(|a, b| {
            geometry::dist(**a, exit)
                .total_cmp(&geometry::dist(**b, exit))
        })
        .expect("blob has a boundary");
    let inward = geometry::norm(geometry::sub(blob.center, start));
    let start = geometry::add(start, geometry::mul(inward, t * 2.5));
    let dir = geometry::norm(geometry::sub(exit, start));
    let end = geometry::add(exit, geometry::mul(dir, 0.5));
    let mid = geometry::mul(geometry::add(start, end), 0.5);
    let ctrl = geometry::add(
        mid,
        geometry::mul(
            geometry::perp(dir),
            geometry::dist(start, end) * rng.random_range(-0.15..0.15),
        ),
    );
    vec![(start, ctrl, end)]
}

/// Push a road node unless an existing one is within 2.5 m: near-duplicate nodes
/// produce overlapping nearly-coincident road edges.
fn push_node(nodes: &mut Vec<Vec2>, p: Vec2) {
    if nodes.iter().all(|n| geometry::dist(*n, p) > 2.5) {
        nodes.push(p);
    }
}

/// A free cell inside `section` for the crossroad node.
fn interior_crossroad(rng: &mut impl Rng, grid: &Grid, section: &[Vec2]) -> Option<Vec2> {
    for _ in 0..80 {
        let c = rng.random_range(0..grid.cols);
        let r = rng.random_range(0..grid.rows);
        let p = ((c as f32 + 0.5) * grid.cell_m, (r as f32 + 0.5) * grid.cell_m);
        if grid.is_free(c, r) && !grid.is_road(c, r) && geometry::point_in_polygon(p, section) {
            return Some(p);
        }
    }
    None
}

/// Prim's minimum spanning tree over the node positions; returns index pairs.
fn spanning_tree(nodes: &[Vec2]) -> Vec<(usize, usize)> {
    let n = nodes.len();
    let mut in_tree = vec![false; n];
    let mut best_dist = vec![f32::INFINITY; n];
    let mut parent = vec![usize::MAX; n];
    let mut edges = Vec::new();
    best_dist[0] = 0.0;
    for _ in 0..n {
        let next = (0..n)
            .filter(|&i| !in_tree[i])
            .min_by(|&a, &b| best_dist[a].total_cmp(&best_dist[b]));
        let Some(u) = next else { break };
        in_tree[u] = true;
        if parent[u] != usize::MAX {
            edges.push((parent[u], u));
        }
        for v in 0..n {
            let d = geometry::dist(nodes[u], nodes[v]);
            if !in_tree[v] && d < best_dist[v] {
                best_dist[v] = d;
                parent[v] = u;
            }
        }
    }
    edges
}

/// A few short extra links so the web has loops, not just a tree.
fn add_extra_links(rng: &mut impl Rng, nodes: &[Vec2], edges: &mut Vec<(usize, usize)>) {
    if nodes.len() < 3 {
        return;
    }
    let mut added = 0;
    for _ in 0..40 {
        if added >= EXTRA_EDGES {
            break;
        }
        let a = rng.random_range(0..nodes.len());
        let b = rng.random_range(0..nodes.len());
        if a == b || edges.contains(&(a, b)) || edges.contains(&(b, a)) {
            continue;
        }
        if geometry::dist(nodes[a], nodes[b]) < 30.0 {
            edges.push((a, b));
            added += 1;
        }
    }
}

/// Stamp one road strip into the grid (open walkable ground; carves walls it
/// crosses) and push its object.
fn stamp_road(
    grid: &mut Grid,
    p0: Option<Vec2>,
    p1: Option<Vec2>,
    road_w: f32,
    config: &LevelConfig,
    objects: &mut Vec<PlacedObject>,
) {
    let (Some(p0), Some(p1)) = (p0, p1) else {
        return;
    };
    if geometry::dist(p0, p1) < 1e-4 {
        // Degenerate (single-node road): still stamp a plaza so the node exists.
        stamp_disc(grid, p0, road_w / 2.0);
        return;
    }
    let dir = geometry::norm(geometry::sub(p1, p0));
    let mid = geometry::mul(geometry::add(p0, p1), 0.5);
    let len = geometry::dist(p0, p1);
    let angle = dir.1.atan2(dir.0).to_degrees();

    // Stamp cells near the segment as road; this carves any wall it crosses.
    let half = road_w / 2.0;
    let (c0, r0) = (
        ((mid.0 - len / 2.0 - half) / grid.cell_m).floor().max(0.0) as usize,
        ((mid.1 - len / 2.0 - half) / grid.cell_m).floor().max(0.0) as usize,
    );
    let (c1, r1) = (
        (((mid.0 + len / 2.0 + half) / grid.cell_m).ceil() as usize).min(grid.cols),
        (((mid.1 + len / 2.0 + half) / grid.cell_m).ceil() as usize).min(grid.rows),
    );
    for r in r0..r1 {
        for c in c0..c1 {
            let q = ((c as f32 + 0.5) * grid.cell_m, (r as f32 + 0.5) * grid.cell_m);
            if geometry::dist_point_segment(q, p0, p1) <= half + grid.cell_m * 0.35 {
                grid.mark_road_cell(c, r);
            }
        }
    }

    objects.push(PlacedObject {
        category: Category::Road,
        type_name: "Road".to_owned(),
        color: config.walls.road_color,
        x_m: mid.0 - len / 2.0,
        y_m: mid.1 - road_w / 2.0,
        w_m: len,
        h_m: road_w,
        angle_deg: angle,
        details: ObjectDetails::Road,
    });
}
