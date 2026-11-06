//! A map region of a mixed map, as its own kind's chain sees it
//! (docs/mixed-plan.md, problems 2–4): its ground (the *domain*), how far
//! each cell is from the domain's edge (the region's "map edge"), the fixed
//! gates (the portals to its neighbours and the map gates that land in it),
//! and the shared land and water decided for the whole map.
//!
//! A chain given a context builds only inside the domain; given none, it
//! builds a whole map as before.

use crate::level::{CellMask, TerrainLayout, WaterLayout};

/// What a fixed gate leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedGateKind {
    /// The opening to another map region (`portal` id, the region id there).
    Portal { portal: u32, to: u8 },
    /// A map gate (its id in the gate list).
    MapGate { gate: u32 },
    /// A way down into a mountain's caves from one of its passes.
    PassEntrance { pass: u32 },
}

/// A gate at a fixed point on the region's edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FixedGate {
    /// On the region's edge, meters.
    pub point_m: [f32; 2],
    /// Unit direction into the region.
    pub inward: [f32; 2],
    pub width_m: f32,
    pub kind: FixedGateKind,
    /// How a swamp meets it: its gate island at the edge (shore) or a
    /// boardwalk out to it (bridge). A portal is a shore; a map gate keeps
    /// its own style.
    pub style: crate::model::GateStyle,
}

impl FixedGate {
    /// The direction the gate faces out of the region, degrees (0 east,
    /// 90 north), as the gate lists record it.
    pub fn direction_deg(&self) -> f32 {
        // Outward = −inward; screen y points down, so north is −y.
        let (ox, oy) = (-self.inward[0], -self.inward[1]);
        (-oy).atan2(ox).to_degrees().rem_euclid(360.0)
    }
}

/// Everything a chain needs to build one map region.
pub struct RegionCtx {
    /// The map region id (1-based).
    pub id: u8,
    /// The region's cells (the whole map grid).
    pub domain: CellMask,
    /// Distance from each cell to the nearest cell outside the domain or
    /// the map edge, meters (0 outside).
    pub edge_dist: Vec<f32>,
    pub gates: Vec<FixedGate>,
    /// The mixed map's land (height per cell).
    pub land: TerrainLayout,
    /// The mixed map's water, with this region's fords made shallow.
    pub water: WaterLayout,
    /// This region's bridges: walkable over the deep water.
    pub bridges: CellMask,
    /// Blending (docs/blending-plan.md): per neighbouring kind, how deep
    /// each cell lies in the transition zone toward it — 1 at the border,
    /// falling to 0 at the zone's (noisy) width, 0 outside the region.
    /// Empty with blending off. From the region map only, never from a
    /// neighbour's result.
    pub blend: Vec<(crate::model::MapKind, Vec<f32>)>,
    /// The trees a forest neighbour lends to the transition (its tree
    /// settings: trunk sizes, color).
    pub neighbour_trees: Option<crate::model::ForestTreeSettings>,
    /// Seed of the blending's own noise and hashes (never a chain's stream).
    pub blend_seed: u64,
    /// Blending: embankment (quay wall) thickness along the rivers inside
    /// a town, meters; 0 = open banks.
    pub embankment_m: f32,
}

impl RegionCtx {
    pub fn inside(&self, c: usize) -> bool {
        self.domain.cells[c]
    }

    /// The transition zone toward `kind`, if this region has one.
    pub fn blend_toward(&self, kind: crate::model::MapKind) -> Option<&[f32]> {
        self.blend.iter().find(|(k, _)| *k == kind).map(|(_, t)| t.as_slice())
    }

    /// The region's share of the whole map, 0..1.
    pub fn share(&self) -> f32 {
        self.domain.count_true() as f32 / self.domain.cells.len().max(1) as f32
    }
}

/// Distance from every cell to the nearest cell outside `domain` or the
/// map edge, meters, up to `limit_m` (infinity beyond).
pub fn edge_distance(domain: &CellMask, limit_m: f32) -> Vec<f32> {
    let (w, h, cell) = (domain.width, domain.height, domain.cell_size_m);
    let source = |c: usize| {
        let (x, y) = (c % w, c / w);
        !domain.cells[c] || x == 0 || y == 0 || x + 1 == w || y + 1 == h
    };
    let (d, _) = crate::generator::forest::distance_field(w, h, cell, source, limit_m);
    d.into_iter().enumerate().map(|(c, v)| if domain.cells[c] { v } else { 0.0 }).collect()
}
