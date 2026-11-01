//! The mixed map chain (docs/mixed-plan.md): the map is split into map
//! regions, each of one kind (town, swamp, forest, cave) with its own
//! settings. The shared things are decided first, for the whole map; then
//! every region runs its own kind's chain; then the seams.
//!
//! Built so far:
//! 2. **Map regions** (M1) — who is where: the town's region growth on the
//!    whole map, with a share and a placement per region.
//! 3. **Land** (M2) — one height field, shaped by the kinds: a cave region
//!    is a high massif, a swamp a low basin, a town a flat terrace, a forest
//!    rolling ground; slopes between them, hills and a tilt on top.
//! 4. **Water** (M2) — the forest's drainage over the whole map: rivers
//!    rise at springs (at the foot of a massif, never in it), gather
//!    brooks and leave at the map edge or end in a swamp; ponds in hollows.
//! 5. **Connections** (M3) — which regions link (a tree + loops), one
//!    portal on each linked border, the map gates' regions, and bridges /
//!    fords where a river cuts a region apart (`connections.rs`).
//! 6. **Region chains** (M4–M7) — every region runs its own kind's chain
//!    in its own ground (`chains.rs`).
//! 7. **Seams** (M8) — the two sides of every opening joined, the whole map
//!    checked to be one walkable piece (`seams.rs`).

pub mod chains;
pub mod connections;
pub mod seams;

use rand::Rng;
use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};

use crate::generator::forest::water::{self, Drainage};
use crate::generator::noise::NoiseField;
use crate::level::TerrainLayout;

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
        // Mountains: bare stone (the caves are inside).
        MapKind::Cave => [128, 122, 116],
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
    let mut mixed = split(config, &mut rng, &mut notes);
    let mut objects = Vec::new();
    if let Some(m) = mixed.as_mut() {
        progress("Shaping the land", 0.1);
        let land_layout = land(config, &mut rng, m);
        progress("Letting the water flow", 0.3);
        let built = rivers(config, &mut rng, m, &land_layout);
        notes.extend(built.notes);
        progress("Linking the regions", 0.5);
        let linked = connections::build(config, &mut rng, m, &land_layout, &built.layout);
        notes.extend(linked.notes);
        let gate_markers = linked.objects;
        m.connections = Some(linked.layout);
        let (levels, region_notes) = chains::build(config, seed, m, &land_layout, &built.layout, progress, (0.55, 0.99));
        notes.extend(region_notes);
        // The regions' objects, then the gate markers (drawn on top).
        for level in &levels {
            let start = objects.len();
            if let Some(l) = level {
                objects.extend(l.objects.iter().cloned());
            }
            m.object_ranges.push([start, objects.len() - start]);
        }
        if let Some(cn) = m.connections.as_mut() {
            let base = objects.len();
            for g in &mut cn.gates {
                g.object += base;
            }
        }
        objects.extend(gate_markers);
        m.levels = levels;
        progress("Joining the regions", 0.995);
        let joined = seams::build(m);
        notes.extend(joined.notes);
        m.walkable = Some(joined.walkable);
        m.corridors = Some(joined.corridors);
        m.seams = joined.seams;
        m.water = Some(built.layout);
        m.land = Some(land_layout);
    }

    progress("Done", 1.0);
    GeneratedLevel {
        config: config.clone(),
        seed,
        objects,
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

/// The mobs again on a finished mixed level ("Re-roll mobs" / "Apply mobs
/// only"): every built region places its mobs with its current settings
/// from `config` (scaled to its size like a full generation), on its own
/// random stream; then the mixed level's objects are put together again.
pub fn redo_mobs(level: &mut GeneratedLevel, config: &LevelConfig) {
    let Some(m) = level.mixed.as_mut() else { return };
    let total = m.map.ids.len().max(1) as f32;
    for (i, slot) in m.levels.iter_mut().enumerate() {
        let (Some(sub), Some(row), Some(r)) = (slot.as_mut(), config.mixed.regions.get(i), m.regions.get(i)) else { continue };
        let cfg = row.for_share(config, r.cells as f32 / total);
        sub.config.mobs = cfg.mobs;
        crate::generator::place_mobs(sub, &|_, _| {});
    }
    // The regions' objects, then the gate markers (as a generation does).
    let markers: Vec<_> = m.connections.as_ref().map(|cn| cn.gates.iter().map(|g| level.objects[g.object].clone()).collect()).unwrap_or_default();
    let mut objects = Vec::new();
    m.object_ranges.clear();
    for sub in &m.levels {
        let start = objects.len();
        if let Some(l) = sub {
            objects.extend(l.objects.iter().cloned());
        }
        m.object_ranges.push([start, objects.len() - start]);
    }
    if let Some(cn) = m.connections.as_mut() {
        for (k, g) in cn.gates.iter_mut().enumerate() {
            g.object = objects.len() + k;
        }
    }
    objects.extend(markers);
    level.notes.retain(|n| !(n.starts_with("Region ") && n.contains(": Mobs:")));
    for (r, sub) in m.regions.iter().zip(&m.levels) {
        if let Some(l) = sub {
            level.notes.extend(l.notes.iter().filter(|n| n.starts_with("Mobs:")).map(|n| format!("Region {} ({}): {n}", r.id, r.name)));
        }
    }
    level.objects = objects;
    level.config.mixed.regions.clone_from(&config.mixed.regions);
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
    Some(MixedLayout {
        map: split.map,
        regions,
        land: None,
        water: None,
        connections: None,
        levels: Vec::new(),
        object_ranges: Vec::new(),
        walkable: None,
        corridors: None,
        seams: Vec::new(),
    })
}

/// How high a kind's ground lies (0 low … 1 high) and how hilly it is.
fn kind_land(kind: MapKind) -> (f32, f32) {
    match kind {
        // Mountains: a rock massif, high and rugged (caves inside).
        MapKind::Cave => (1.0, 1.0),
        // Rolling ground.
        MapKind::Forest => (0.55, 0.8),
        // A flat terrace.
        MapKind::Town => (0.45, 0.25),
        // A badly drained basin.
        MapKind::Swamp => (0.0, 0.2),
        MapKind::Mixed => (0.5, 0.8),
    }
}

/// Node 3 · Land: each cell's height from its region's kind, blurred into
/// slopes across the borders, plus warped hills and a tilt.
fn land(config: &LevelConfig, rng: &mut ChaCha8Rng, m: &MixedLayout) -> TerrainLayout {
    let s = &config.mixed.land;
    let (w, h, cell) = (m.map.width, m.map.height, m.map.cell_size_m);
    let n = w * h;
    let (wm, hm) = (w as f32 * cell, h as f32 * cell);
    let hills = NoiseField::new(rng.random::<u64>());
    let warp_x = NoiseField::new(rng.random::<u64>());
    let warp_y = NoiseField::new(rng.random::<u64>());
    let dir = rng.random_range(0.0..360.0f32).to_radians();

    let mut target = vec![0.0f32; n];
    let mut amp = vec![0.0f32; n];
    for c in 0..n {
        let (t, a) = m.region_at(c).map_or((0.5, 0.8), |r| kind_land(r.kind));
        target[c] = t;
        amp[c] = a;
    }
    let r = (s.slope_m.max(0.0) / cell / 2.0).round() as usize;
    for v in [&mut target, &mut amp] {
        for _ in 0..3 {
            box_blur(v, w, h, r);
        }
    }

    let k = (s.kinds_pct / 100.0).clamp(0.0, 1.0);
    let scale = s.hill_scale_m.max(5.0);
    let tilt = (s.tilt_pct / 100.0).clamp(0.0, 1.0) * 0.6;
    let diag = (wm * wm + hm * hm).sqrt();
    let mut height = vec![0.0f32; n];
    for c in 0..n {
        let (x, y) = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
        let warp = scale * 0.5;
        let wx = x + warp * (warp_x.fbm(x / scale, y / scale, 3) - 0.5) * 2.0;
        let wy = y + warp * (warp_y.fbm(x / scale, y / scale, 3) - 0.5) * 2.0;
        let base = hills.fbm(wx / scale, wy / scale, 4);
        let side = ((x - wm / 2.0) * dir.cos() + (hm / 2.0 - y) * dir.sin()) / diag + 0.5;
        // The kinds set the level and how hilly the ground on it is (a
        // town terrace stays flat); with kinds at 0% only the hills count.
        let hilly = 1.0 + (amp[c] - 1.0) * k;
        height[c] = k * target[c] + (1.0 - k) * 0.5 + (0.35 + 0.65 * (1.0 - k)) * (base - 0.5) * hilly + tilt * (side - 0.5);
    }
    let (lo, hi) = height.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    let span = (hi - lo).max(1e-6);
    for v in &mut height {
        *v = (*v - lo) / span;
    }
    TerrainLayout { width: w, height: h, cell_size_m: cell, height_m: height, relief_m: s.relief_m.max(0.0), rock: CellMask::new(w, h, cell), rock_pct: 0.0 }
}

/// A box blur of radius `r` cells (rows, then columns), edges clamped.
fn box_blur(v: &mut [f32], w: usize, h: usize, r: usize) {
    if r == 0 {
        return;
    }
    let mut line = Vec::new();
    let mut sums = Vec::new();
    let mut pass = |v: &mut [f32], len: usize, count: usize, at: &dyn Fn(usize, usize) -> usize| {
        for k in 0..count {
            line.clear();
            line.extend((0..len).map(|i| v[at(k, i)]));
            sums.clear();
            sums.push(0.0f64);
            for &x in &line {
                let last = *sums.last().unwrap();
                sums.push(last + x as f64);
            }
            for i in 0..len {
                let (a, b) = (i.saturating_sub(r), (i + r + 1).min(len));
                v[at(k, i)] = ((sums[b] - sums[a]) / (b - a) as f64) as f32;
            }
        }
    };
    pass(v, w, h, &|y, x| y * w + x);
    pass(v, h, w, &|x, y| y * w + x);
}

/// Node 4 · Water: the forest drainage over the whole land; swamp regions
/// drink the rivers, cave regions stay dry.
fn rivers(config: &LevelConfig, rng: &mut ChaCha8Rng, m: &MixedLayout, land: &TerrainLayout) -> water::WaterBuild {
    let (w, h, cell) = (m.map.width, m.map.height, m.map.cell_size_m);
    let of_kind = |kind: MapKind| CellMask {
        width: w,
        height: h,
        cell_size_m: cell,
        cells: (0..w * h).map(|c| m.region_at(c).is_some_and(|r| r.kind == kind)).collect(),
    };
    let (sinks, dry) = (of_kind(MapKind::Swamp), of_kind(MapKind::Cave));
    // Distance to the nearest other region: a river crosses a border or
    // keeps off it (never runs along it, touching).
    let ids = &m.map.ids;
    let clear = config.mixed.river_clearance_m.max(0.0);
    let on_border = |c: usize| crate::generator::forest::neighbors4(c, w, h).any(|q| ids[q] != ids[c]);
    let (dist, _) = crate::generator::forest::distance_field(w, h, cell, on_border, clear * 4.0 + 40.0);
    let borders = (clear > 0.0 && m.regions.len() > 1).then_some(water::Borders { dist: &dist, clear_m: clear });
    let extra = Drainage { sinks: Some(&sinks).filter(|s| s.count_true() > 0), dry: Some(&dry).filter(|d| d.count_true() > 0), borders };
    water::build_drainage(&config.mixed.water, rng, land, &extra)
}

#[doc(hidden)]
/// Water cells touching a region border (within 1.5 m) with no
/// crossing of that border (water on both sides, or a river mouth into
/// a swamp) within `reach` meters. A spring at the foot of a massif (the
/// water comes out of the mountain) is no touch. For the tests and the
/// tuning examples.
pub fn touching_without_crossing(lvl: &GeneratedLevel, reach: f32) -> Vec<usize> {
    let m = lvl.mixed.as_ref().unwrap();
    let wl = m.water.as_ref().unwrap();
    let (w, h, cell) = (m.map.width, m.map.height, m.map.cell_size_m);
    let ids = &m.map.ids;
    let wet = |c: usize| wl.deep.cells[c] || wl.shallow.cells[c];
    let swamp = |c: usize| m.region_at(c).is_some_and(|r| r.kind == MapKind::Swamp);
    let mut crossings = Vec::new();
    for c in 0..w * h {
        for q in crate::generator::forest::neighbors4(c, w, h) {
            if ids[q] != ids[c] && wet(c) && (wet(q) || swamp(q)) {
                crossings.push(c);
            }
        }
    }
    let on_border = |c: usize| crate::generator::forest::neighbors4(c, w, h).any(|q| ids[q] != ids[c]);
    let (dist, _) = crate::generator::forest::distance_field(w, h, cell, on_border, 3.0);
    let at = |c: usize| (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
    (0..w * h)
        .filter(|&c| wet(c) && dist[c] <= 1.5)
        .filter(|&c| {
            let p = at(c);
            let r = (2.0 / cell).ceil() as i64;
            let (x, y) = ((c % w) as i64, (c / w) as i64);
            let cave_near = (-r..=r).any(|dy| {
                (-r..=r).any(|dx| {
                    let (nx, ny) = (x + dx, y + dy);
                    nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 && m.region_at(ny as usize * w + nx as usize).is_some_and(|r| r.kind == MapKind::Cave)
                })
            });
            !(cave_near && wl.streams.iter().any(|st| {
                let s0 = st.path_m[0];
                (s0[0] - p.0).hypot(s0[1] - p.1) <= st.width_m[0] + 8.0
            }))
        })
        .filter(|&c| {
            let p = at(c);
            !crossings.iter().any(|&x| {
                let q = at(x);
                (q.0 - p.0).hypot(q.1 - p.1) <= reach
            })
        })
        .collect()
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
    fn land_follows_the_kinds_and_water_respects_them() {
        let mut c = LevelConfig::of_kind(MapKind::Mixed);
        c.map.width_m = 120.0;
        c.map.height_m = 120.0;
        c.cell_size_m = 1.0;
        c.mixed.water.streams = 3;
        let mut sinks = 0;
        for seed in 1..=6 {
            let lvl = generate_with_progress(&c, seed, &|_, _| {});
            let m = lvl.mixed.as_ref().unwrap();
            let (land, wl) = (m.land.as_ref().unwrap(), m.water.as_ref().unwrap());
            let mean = |kind: MapKind| {
                let v: Vec<f32> = (0..land.height_m.len()).filter(|&c| m.region_at(c).unwrap().kind == kind).map(|c| land.height_m[c]).collect();
                v.iter().sum::<f32>() / v.len().max(1) as f32
            };
            assert!(mean(MapKind::Cave) > mean(MapKind::Forest) && mean(MapKind::Forest) > mean(MapKind::Swamp), "seed {seed}");
            let w = m.map.width;
            for cell in 0..m.map.ids.len() {
                let kind = m.region_at(cell).unwrap().kind;
                if matches!(kind, MapKind::Cave | MapKind::Swamp) {
                    assert!(!wl.deep.cells[cell] && !wl.shallow.cells[cell], "seed {seed}: water in a {kind:?} region");
                }
            }
            for st in wl.streams.iter().filter(|s| s.mouth == crate::level::StreamMouth::Sink) {
                sinks += 1;
                let [x, y] = *st.path_m.last().unwrap();
                let near = (0..m.map.ids.len()).any(|c| {
                    m.region_at(c).unwrap().kind == MapKind::Swamp && (((c % w) as f32 + 0.5) - x).hypot(((c / w) as f32 + 0.5) - y) < 4.0
                });
                assert!(near, "seed {seed}: stream {} ends at ({x},{y}), away from the swamp", st.id);
            }
        }
        assert!(sinks > 0, "no river ever reached a swamp");
    }

    #[test]
    fn rivers_cross_borders_or_keep_off_them() {
        let (mut before, mut after) = (0, 0);
        for seed in 1..=8 {
            let mut c = LevelConfig::of_kind(MapKind::Mixed);
            c.map.width_m = 160.0;
            c.map.height_m = 160.0;
            c.cell_size_m = 1.0;
            c.mixed.water.streams = 3;
            let on = generate_with_progress(&c, seed, &|_, _| {});
            c.mixed.river_clearance_m = 0.0;
            let off = generate_with_progress(&c, seed, &|_, _| {});
            after += touching_without_crossing(&on, 25.0).len();
            before += touching_without_crossing(&off, 25.0).len();
        }
        eprintln!("touching cells: clearance off {before}, on {after}");
        assert!(before > 0, "the test maps never had a river along a border");
        assert_eq!(after, 0, "rivers still run along borders (off: {before})");
    }

    #[test]
    fn no_regions_is_a_note_not_a_panic() {
        let lvl = generate_with_progress(&mixed(Vec::new()), 1, &|_, _| {});
        assert!(lvl.mixed.is_none());
        assert!(lvl.notes.iter().any(|n| n.contains("no map regions")));
    }
}
