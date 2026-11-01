//! Mixed node 6 · Region chains (docs/mixed-plan.md, M4–M7): every map
//! region runs its own kind's chain inside its own ground. What the chain
//! gets from the mixed map (`RegionCtx`):
//!
//! - its ground (domain) and the distance to the domain's edge — the
//!   region's "map edge", where its edge band (forest thicket belt, ...)
//!   grows;
//! - its fixed gates: the portals to its neighbours (inward = into this
//!   region) and the map gates that landed in it;
//! - the shared land and water, with this region's river crossings made:
//!   fords shallow, bridges walkable;
//! - its own config ([`MixedRegion::for_share`]: whole-map counts scaled
//!   to its size) and its own random stream (the seed mixed with its id, so
//!   changing one region never reshuffles another).
//!
//! Built so far: Forest (M4). Other kinds keep their region-map color
//! until their step.

use crate::generator::mixed::connections::capsule_cells;
use crate::generator::region::{edge_distance, FixedGate, FixedGateKind, RegionCtx};
use crate::generator::Progress;
use crate::level::{CellMask, GeneratedLevel, MixedCrossingLook, MixedLayout, TerrainLayout, WaterLayout};
use crate::model::{LevelConfig, MapKind};

/// How far the edge distance is computed, meters (edge bands are narrower).
const EDGE_LIMIT_M: f32 = 80.0;
/// Width of a map gate's way in, meters.
const MAP_GATE_WIDTH_M: f32 = 3.0;

/// The kinds whose chain runs inside a map region so far.
pub fn builds(kind: MapKind) -> bool {
    kind == MapKind::Forest
}

/// A region's own random stream: the map seed mixed with its id.
pub fn region_seed(seed: u64, id: u8) -> u64 {
    seed ^ (id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// The context for region `id`.
pub fn context(m: &MixedLayout, id: u8, land: &TerrainLayout, water: &WaterLayout) -> RegionCtx {
    let (w, h, cell) = (m.map.width, m.map.height, m.map.cell_size_m);
    let domain = CellMask { width: w, height: h, cell_size_m: cell, cells: m.map.ids.iter().map(|&i| i == id).collect() };
    let edge_dist = edge_distance(&domain, EDGE_LIMIT_M);
    let mut gates = Vec::new();
    let mut water = water.clone();
    let mut bridges = CellMask::new(w, h, cell);
    if let Some(cn) = &m.connections {
        for p in cn.portals_of(id) {
            let to = if p.regions[0] == id { p.regions[1] } else { p.regions[0] };
            // The normal points from regions[0] into regions[1].
            let s = if p.regions[1] == id { 1.0 } else { -1.0 };
            gates.push(FixedGate { point_m: p.point_m, inward: [p.normal[0] * s, p.normal[1] * s], width_m: p.width_m, kind: FixedGateKind::Portal { portal: p.id, to } });
        }
        for g in cn.gates.iter().filter(|g| g.region == id) {
            gates.push(FixedGate { point_m: g.point_m, inward: g.inward, width_m: MAP_GATE_WIDTH_M, kind: FixedGateKind::MapGate { gate: g.id } });
        }
        for x in cn.crossings.iter().filter(|x| x.region == id) {
            for c in capsule_cells(w, h, cell, x.from_m, x.to_m, x.width_m / 2.0) {
                if m.map.ids[c] != id || !water.deep.cells[c] {
                    continue;
                }
                match x.look {
                    MixedCrossingLook::Ford => {
                        water.deep.cells[c] = false;
                        water.shallow.cells[c] = true;
                    }
                    MixedCrossingLook::Bridge => bridges.cells[c] = true,
                }
            }
        }
    }
    RegionCtx { id, domain, edge_dist, gates, land: land.clone(), water, bridges }
}

/// Node 6: every region whose kind is built runs its chain. Returns the
/// levels (id order) and the notes, each prefixed with its region.
pub fn build(config: &LevelConfig, seed: u64, m: &MixedLayout, land: &TerrainLayout, water: &WaterLayout, progress: Progress, span: (f32, f32)) -> (Vec<Option<GeneratedLevel>>, Vec<String>) {
    let mut levels = Vec::with_capacity(m.regions.len());
    let mut notes = Vec::new();
    let todo = m.regions.iter().filter(|r| builds(r.kind) && r.cells > 0).count().max(1) as f32;
    let mut done = 0.0;
    for (i, r) in m.regions.iter().enumerate() {
        let row = config.mixed.regions.get(i);
        let (true, Some(row)) = (builds(r.kind) && r.cells > 0, row) else {
            levels.push(None);
            continue;
        };
        let ctx = context(m, r.id, land, water);
        let cfg = row.for_share(config, ctx.share());
        let (a, b) = (span.0 + (span.1 - span.0) * done / todo, span.0 + (span.1 - span.0) * (done + 1.0) / todo);
        let local = |label: &'static str, f: f32| progress(label, a + (b - a) * f.clamp(0.0, 1.0));
        let level = match r.kind {
            MapKind::Forest => crate::generator::forest::generate_in(&cfg, region_seed(seed, r.id), Some(&ctx), &local),
            _ => unreachable!("only built kinds get here"),
        };
        notes.extend(level.notes.iter().map(|n| format!("Region {} ({}): {n}", r.id, r.name)));
        levels.push(Some(level));
        done += 1.0;
    }
    (levels, notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::ForestGround;

    fn config() -> LevelConfig {
        let mut c = LevelConfig::of_kind(MapKind::Mixed);
        c.map.width_m = 160.0;
        c.map.height_m = 160.0;
        c.cell_size_m = 1.0;
        c
    }

    #[test]
    fn a_forest_region_is_one_walkable_piece_inside_its_ground() {
        for seed in 1..=5 {
            let lvl = crate::generator::generate(&config(), seed);
            let m = lvl.mixed.as_ref().unwrap();
            let (w, h, cell) = (m.map.width, m.map.height, m.map.cell_size_m);
            let mut forests = 0;
            for (r, level) in m.regions.iter().zip(&m.levels) {
                let Some(level) = level else { continue };
                forests += 1;
                let p = level.forest.as_ref().unwrap().passages.as_ref().unwrap();
                let walk: Vec<bool> = p.ground.iter().map(|&g| ForestGround::from_code(g).walkable()).collect();
                assert!((0..w * h).all(|c| !walk[c] || m.map.ids[c] == r.id), "seed {seed}: walkable ground outside region {}", r.id);
                let (labels, pieces) = crate::generator::grid::components(&CellMask { width: w, height: h, cell_size_m: cell, cells: walk.clone() });
                assert_eq!(pieces, 1, "seed {seed}: region {} in {pieces} pieces; notes {:?}", r.id, lvl.notes);
                // Every opening and map gate is reached.
                let cn = m.connections.as_ref().unwrap();
                let want = cn.portals_of(r.id).count() + cn.gates.iter().filter(|g| g.region == r.id).count();
                assert_eq!(p.gates.len(), want, "seed {seed}: region {} gates; notes {:?}", r.id, lvl.notes);
                for g in &p.gates {
                    let c = (g.point_m[1] / cell) as usize * w + (g.point_m[0] / cell) as usize;
                    assert!(labels[c] == 1, "seed {seed}: gate {} not on the walkable ground", g.id);
                }
                for o in &level.objects {
                    let c = ((o.y_m + o.h_m / 2.0) / cell) as usize * w + ((o.x_m + o.w_m / 2.0) / cell) as usize;
                    assert_eq!(m.map.ids[c], r.id, "seed {seed}: a {} outside region {}", o.type_name, r.id);
                }
            }
            assert!(forests > 0);
        }
    }

    #[test]
    fn another_region_s_settings_never_change_a_forest() {
        let a = config();
        let mut b = config();
        let swamp = b.mixed.regions.iter_mut().find(|r| r.kind == MapKind::Swamp).unwrap();
        swamp.config.swamp.islands.hub_count += 3;
        swamp.config.mobs.budget *= 3.0;
        let (la, lb) = (crate::generator::generate(&a, 9), crate::generator::generate(&b, 9));
        assert_eq!(la.mixed.as_ref().unwrap().levels[0], lb.mixed.as_ref().unwrap().levels[0]);
    }

    #[test]
    fn whole_map_counts_shrink_with_the_region() {
        let c = config();
        let row = &c.mixed.regions[0];
        let half = row.for_share(&c, 0.5);
        assert_eq!(half.hidden_rooms.count, (row.config.hidden_rooms.count as f32 * 0.5).round().max(1.0) as u32);
        assert!((half.mobs.budget - row.config.mobs.budget * 0.5).abs() < 1e-3);
        let mut off = row.clone();
        off.scale_amounts = false;
        assert_eq!(off.for_share(&c, 0.5).hidden_rooms.count, row.config.hidden_rooms.count);
    }
}
