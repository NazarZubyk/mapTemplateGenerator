//! The mixed map chain (docs/mixed-plan.md): the map is split into map
//! regions, each of one kind (town, swamp, forest, cave) with its own
//! settings. The shared things are decided first, for the whole map; then
//! every region runs its own kind's chain; then the seams.
//!
//! Built so far:
//! 2. **Map regions** (M1) — who is where: the town's region growth on the
//!    whole map, with a share and a placement per region.

use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};

use crate::generator::regions::{self, Start};
use crate::generator::Progress;
use crate::level::{CellMask, GeneratedLevel, MixedLayout, MixedRegionStat};
use crate::model::{LevelConfig, MapKind, RegionPlacement};

/// Most map regions a mixed map can have.
pub const MAX_REGIONS: usize = 16;

/// The main map color of a kind's ground, for the region map.
pub fn kind_color(kind: MapKind) -> [u8; 3] {
    match kind {
        MapKind::Town => [206, 182, 140],
        MapKind::Swamp => [96, 124, 104],
        MapKind::Forest => [88, 132, 70],
        MapKind::Cave => [104, 92, 86],
        MapKind::Mixed => [180, 180, 180],
    }
}

/// Region `id`'s color: its kind's color, a little lighter or darker per
/// region so two neighbours of one kind stay apart.
pub fn region_color(kind: MapKind, id: u8) -> [u8; 3] {
    const SHIFT: [f32; 5] = [1.0, 0.86, 1.12, 0.93, 1.06];
    let f = SHIFT[(id as usize).saturating_sub(1) % SHIFT.len()];
    kind_color(kind).map(|v| (v as f32 * f).round().clamp(0.0, 255.0) as u8)
}

/// Generate a mixed level: one RNG stream, nodes in fixed order.
pub fn generate_with_progress(config: &LevelConfig, seed: u64, progress: Progress) -> GeneratedLevel {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut notes = Vec::new();

    progress("Splitting the map into regions", 0.0);
    let mixed = split(config, &mut rng, &mut notes);

    progress("Done", 1.0);
    GeneratedLevel {
        config: config.clone(),
        seed,
        objects: Vec::new(),
        shape: None,
        walls: None,
        regions: None,
        roads: None,
        region_walls: None,
        houses: None,
        props: None,
        interactive: None,
        hidden_rooms: None,
        mobs: None,
        gates: None,
        swamp: None,
        cave: None,
        forest: None,
        mixed,
        noise_preview: None,
        notes,
    }
}

/// Node 2 · Map regions: the whole map split by the regions' shares and
/// placements.
fn split(config: &LevelConfig, rng: &mut ChaCha8Rng, notes: &mut Vec<String>) -> Option<MixedLayout> {
    let rows = &config.mixed.regions;
    if rows.is_empty() {
        notes.push("Mixed map: no map regions — add one in the Map regions node".to_owned());
        return None;
    }
    if rows.len() > MAX_REGIONS {
        notes.push(format!("Mixed map: only the first {MAX_REGIONS} map regions are used"));
    }
    let rows = &rows[..rows.len().min(MAX_REGIONS)];
    let cell = config.cell_size_m.max(0.01);
    let w = ((config.map.width_m / cell).round() as usize).max(1);
    let h = ((config.map.height_m / cell).round() as usize).max(1);
    let area = CellMask { width: w, height: h, cell_size_m: cell, cells: vec![true; w * h] };
    let shares: Vec<f32> = rows.iter().map(|r| r.share_pct.max(0.1)).collect();
    let starts: Vec<Start> = rows
        .iter()
        .map(|r| match r.placement {
            RegionPlacement::Anywhere => Start::Anywhere,
            RegionPlacement::Edge => Start::Edge,
            RegionPlacement::Inside => Start::Nested,
        })
        .collect();
    let split = regions::build_placed(rng, &area, shares, &starts, config.mixed.irregularity_pct)?;
    let regions = rows
        .iter()
        .zip(&split.stats)
        .map(|(r, st)| {
            let kind = if r.kind == MapKind::Mixed { MapKind::Forest } else { r.kind };
            if st.cells == 0 {
                notes.push(format!("Map region {} ({}) got no ground — raise its share", st.id, r.name));
            }
            MixedRegionStat {
                id: st.id,
                name: r.name.clone(),
                kind,
                placement: r.placement,
                target_pct: st.target_pct,
                actual_pct: st.actual_pct,
                cells: st.cells,
                color: region_color(kind, st.id),
            }
        })
        .collect();
    Some(MixedLayout { map: split.map, regions })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MixedRegion, MixedSettings};

    fn mixed(regions: Vec<MixedRegion>) -> LevelConfig {
        let mut c = LevelConfig { kind: MapKind::Mixed, ..LevelConfig::test_64() };
        c.mixed.regions = regions;
        c
    }

    #[test]
    fn every_cell_belongs_to_a_region_near_its_share() {
        let c = mixed(MixedSettings::starter());
        for seed in [1, 2, 3] {
            let lvl = generate_with_progress(&c, seed, &|_, _| {});
            let m = lvl.mixed.as_ref().expect("split");
            assert!(m.map.ids.iter().all(|&id| id >= 1 && id as usize <= 4), "seed {seed}");
            for r in &m.regions {
                assert!(r.cells > 0, "seed {seed}: region {} empty", r.id);
                assert!((r.actual_pct - r.target_pct).abs() < 10.0, "seed {seed}: {r:?}");
            }
            assert_eq!(lvl.mixed, generate_with_progress(&c, seed, &|_, _| {}).mixed, "deterministic");
        }
    }

    #[test]
    fn edge_region_touches_the_map_edge_and_inside_one_does_not() {
        let mut rows = vec![MixedRegion::of(MapKind::Forest, 70.0), MixedRegion::of(MapKind::Town, 12.0), MixedRegion::of(MapKind::Cave, 18.0)];
        rows[1].placement = RegionPlacement::Inside;
        rows[2].placement = RegionPlacement::Edge;
        let c = mixed(rows);
        for seed in [4, 5, 6] {
            let m = generate_with_progress(&c, seed, &|_, _| {}).mixed.unwrap();
            let (w, h) = (m.map.width, m.map.height);
            let on_edge = |id: u8| (0..w * h).any(|i| m.map.ids[i] == id && (i % w == 0 || i % w == w - 1 || i / w == 0 || i / w == h - 1));
            assert!(on_edge(3), "seed {seed}: the edge cave misses the map edge");
            assert!(!on_edge(2), "seed {seed}: the inside town touches the map edge");
        }
    }

    #[test]
    fn no_regions_is_a_note_not_a_panic() {
        let lvl = generate_with_progress(&mixed(Vec::new()), 1, &|_, _| {});
        assert!(lvl.mixed.is_none());
        assert!(lvl.notes.iter().any(|n| n.contains("no map regions")));
    }
}
