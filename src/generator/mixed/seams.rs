//! Mixed node 7 · Seams (step M8 in docs/mixed-plan.md): the regions are
//! built, each one walkable piece reaching its own openings; here the two
//! sides of every opening are joined for real, and the whole map is
//! checked to be one walkable piece.
//!
//! 1. **Walkable ground** of each region, from its own result: forest
//!    ground, town floor (minus walls), swamp water within wading depth and
//!    land, cave floor (minus chasms and deep pools) and the mountain
//!    passes.
//! 2. **Corridors** — for every portal, from the nearest walkable cell of
//!    each side (within `REACH_M`) to the portal point, `width_m` wide,
//!    through whatever edge band is left (thicket belt, swamp border, rock,
//!    a stretch of town wall); for every map gate the same on its one side.
//!    A corridor cell stays in its region and takes that region's path look
//!    on the map (trail, road, mud path, pass).
//! 3. **Check** — one flood fill over the whole map; pieces cut off from
//!    the biggest one are reported.
//!
//! Objects are not walls here: each region keeps its own objects clear of
//! its paths and gates.

use crate::generator::mixed::connections::capsule_cells;
use crate::generator::grid;
use crate::level::{CellMask, ForestGround, GeneratedLevel, MixedLayout, MixedSeam};
use crate::model::MapKind;

/// How far from an opening a region's walkable ground is looked for, m.
const REACH_M: f32 = 25.0;
/// Cut-off pieces at least this big are reported, m².
const REPORT_M2: f32 = 40.0;
/// Around an opening, two regions may meet this far beyond its width, m.
const SEAL_CLEAR_M: f32 = 8.0;

pub struct SeamBuild {
    /// Walkable ground of the whole map, corridors included.
    pub walkable: CellMask,
    /// The corridor cells.
    pub corridors: CellMask,
    pub seams: Vec<MixedSeam>,
    pub notes: Vec<String>,
}

/// A region level's walkable ground (objects not counted), on the whole
/// grid; only its own cells are ever true.
pub fn region_walkable(level: &GeneratedLevel, kind: MapKind, n: usize) -> Vec<bool> {
    match kind {
        MapKind::Forest => level
            .forest
            .as_ref()
            .and_then(|f| f.ground())
            .map(|g| g.iter().map(|&x| ForestGround::from_code(x).walkable()).collect())
            .unwrap_or_else(|| vec![false; n]),
        MapKind::Town => match &level.regions {
            Some(split) => (0..n)
                .map(|c| {
                    split.map.ids[c] != 0
                        && !level.region_walls.as_ref().is_some_and(|w| w.mask.cells[c])
                        && !level.walls.as_ref().is_some_and(|w| w.mask.cells[c])
                })
                .collect(),
            None => vec![false; n],
        },
        MapKind::Swamp => match level.swamp.as_ref().and_then(|s| s.depth.as_ref()) {
            Some(d) => (0..n).map(|c| !d.deep.cells[c] && !d.thicket.cells[c]).collect(),
            None => vec![false; n],
        },
        MapKind::Cave | MapKind::Mixed => match level.cave.as_ref() {
            Some(cv) => {
                let pass = cv.passes.as_ref();
                (0..n)
                    .map(|c| {
                        pass.is_some_and(|p| p.mask.cells[c])
                            || match (cv.carve.as_ref(), cv.hazards.as_ref()) {
                                (Some(carve), Some(hz)) => hz.walkable(&carve.floor, c),
                                (Some(carve), None) => carve.floor.cells[c],
                                _ => false,
                            }
                    })
                    .collect()
            }
            None => vec![false; n],
        },
    }
}

/// Close cell `c` of a region level, in its kind's look: forest thicket,
/// town wall, swamp deep water, mountain rock.
fn close(level: &mut GeneratedLevel, kind: MapKind, c: usize) {
    match kind {
        MapKind::Forest => {
            if let Some(f) = level.forest.as_mut() {
                let code = ForestGround::Thicket.code();
                if let Some(cl) = f.clearings.as_mut() {
                    cl.ground[c] = code;
                } else if let Some(p) = f.passages.as_mut() {
                    p.ground[c] = code;
                }
            }
        }
        MapKind::Town => {
            if let Some(wl) = level.walls.as_mut() {
                wl.mask.cells[c] = true;
            }
        }
        MapKind::Swamp => {
            if let Some(sw) = level.swamp.as_mut() {
                if let Some(d) = sw.depth.as_mut() {
                    d.deep.cells[c] = true;
                    d.shallow.cells[c] = false;
                    d.mud.cells[c] = false;
                    d.thicket.cells[c] = false;
                }
                if let Some(i) = sw.islands.as_mut() {
                    i.ids[c] = 0;
                }
                if let Some(p) = sw.paths.as_mut() {
                    p.mud.cells[c] = false;
                    p.boardwalk.cells[c] = false;
                }
            }
        }
        MapKind::Cave | MapKind::Mixed => {
            if let Some(cv) = level.cave.as_mut() {
                if let Some(carve) = cv.carve.as_mut() {
                    carve.floor.cells[c] = false;
                }
                if let Some(p) = cv.passes.as_mut() {
                    p.mask.cells[c] = false;
                }
            }
        }
    }
}

/// Node 7: seal stray contacts, join every opening, check the whole map.
pub fn build(m: &mut MixedLayout) -> SeamBuild {
    let (w, h, cell) = (m.map.width, m.map.height, m.map.cell_size_m);
    let n = w * h;
    let mut notes = Vec::new();
    let mut walk = vec![false; n];
    for (r, level) in m.regions.iter().zip(&m.levels) {
        let Some(level) = level else { continue };
        let own = region_walkable(level, r.kind, n);
        for c in 0..n {
            walk[c] |= own[c] && m.map.ids[c] == r.id;
        }
    }

    // Seal: walkable ground of two regions that meets away from their
    // openings (a trail along a border, a gate island against it) closes
    // on both sides, in each region's own look — the regions are joined
    // only where the labyrinth linked them.
    let near_opening = |a: u8, b: u8, p: [f32; 2]| {
        m.connections.as_ref().is_some_and(|cn| {
            cn.portals.iter().any(|o| o.regions.contains(&a) && o.regions.contains(&b) && (o.point_m[0] - p[0]).hypot(o.point_m[1] - p[1]) <= o.width_m + SEAL_CLEAR_M)
        })
    };
    let mut contacts: Vec<(usize, usize)> = Vec::new();
    for c in 0..n {
        for q in [(c % w + 1 < w).then(|| c + 1), (c / w + 1 < h).then(|| c + w)].into_iter().flatten() {
            let (a, b) = (m.map.ids[c], m.map.ids[q]);
            if a != b && walk[c] && walk[q] {
                let p = [((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell];
                if !near_opening(a, b, p) {
                    contacts.push((c, q));
                }
            }
        }
    }
    // Clusters of contact cells (8-neighbours); per cluster close ONE side —
    // the first whose region stays one walkable piece without those cells
    // (a trail along the border may be that region's only way through).
    let mut cells: Vec<usize> = contacts.iter().flat_map(|&(c, q)| [c, q]).collect();
    cells.sort_unstable();
    cells.dedup();
    let mut cluster = vec![usize::MAX; n];
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    for &start in &cells {
        if cluster[start] != usize::MAX {
            continue;
        }
        let k = clusters.len();
        let mut stack = vec![start];
        cluster[start] = k;
        let mut members = Vec::new();
        while let Some(c) = stack.pop() {
            members.push(c);
            let (x, y) = ((c % w) as i64, (c / w) as i64);
            for dy in -1..=1i64 {
                for dx in -1..=1i64 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let q = ny as usize * w + nx as usize;
                    if cluster[q] == usize::MAX && cells.binary_search(&q).is_ok() {
                        cluster[q] = k;
                        stack.push(q);
                    }
                }
            }
        }
        clusters.push(members);
    }
    let pieces_of = |walk: &[bool], id: u8| {
        let mask = CellMask { width: w, height: h, cell_size_m: cell, cells: (0..n).map(|c| walk[c] && m.map.ids[c] == id).collect() };
        grid::components(&mask).1
    };
    let mut closed_n = 0usize;
    let mut kept = 0usize;
    for members in &clusters {
        let mut sides: Vec<u8> = members.iter().map(|&c| m.map.ids[c]).collect();
        sides.sort_unstable();
        sides.dedup();
        let mut done = false;
        for &id in &sides {
            let before = pieces_of(&walk, id);
            let mut trial = walk.clone();
            for &c in members.iter().filter(|&&c| m.map.ids[c] == id) {
                trial[c] = false;
            }
            if pieces_of(&trial, id) <= before {
                let kind = m.regions[id as usize - 1].kind;
                for &c in members.iter().filter(|&&c| m.map.ids[c] == id) {
                    walk[c] = false;
                    closed_n += 1;
                    if let Some(Some(level)) = m.levels.get_mut(id as usize - 1) {
                        close(level, kind, c);
                    }
                }
                done = true;
                break;
            }
        }
        kept += usize::from(!done);
    }
    if closed_n > 0 {
        notes.push(format!("Seams: {} m² closed where two regions met away from an opening", (closed_n as f32 * cell * cell).round()));
    }
    if kept > 0 {
        notes.push(format!("Seams: {kept} place(s) where two regions meet away from an opening stay open (closing either side would cut it apart)"));
    }
    let ids = &m.map.ids;
    let center = |c: usize| [((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell];
    // The walkable cell of region `id` nearest to `p`, within REACH_M.
    let nearest = |walk: &[bool], id: u8, p: [f32; 2]| {
        let r = (REACH_M / cell).ceil() as i64;
        let (px, py) = ((p[0] / cell) as i64, (p[1] / cell) as i64);
        let mut best: Option<(f32, usize)> = None;
        for y in (py - r).max(0)..=(py + r).min(h as i64 - 1) {
            for x in (px - r).max(0)..=(px + r).min(w as i64 - 1) {
                let c = y as usize * w + x as usize;
                if !walk[c] || ids[c] != id {
                    continue;
                }
                let q = center(c);
                let d = (q[0] - p[0]).hypot(q[1] - p[1]);
                if d <= REACH_M && best.is_none_or(|b| d < b.0) {
                    best = Some((d, c));
                }
            }
        }
        best.map(|b| b.1)
    };
    let mut corridors = CellMask::new(w, h, cell);
    let mut seams = Vec::new();
    let cut = |corridors: &mut CellMask, walk: &mut [bool], from: [f32; 2], to: [f32; 2], width: f32, sides: &[u8]| {
        let mut k = 0;
        for c in capsule_cells(w, h, cell, from, to, width / 2.0) {
            if sides.contains(&ids[c]) {
                k += usize::from(!walk[c]);
                corridors.cells[c] = true;
                walk[c] = true;
            }
        }
        k
    };
    if let Some(cn) = &m.connections {
        for p in &cn.portals {
            let [a, b] = p.regions;
            let built = |id: u8| m.levels.get(id as usize - 1).is_some_and(|l| l.is_some());
            if !built(a) || !built(b) {
                continue;
            }
            let (na, nb) = (nearest(&walk, a, p.point_m), nearest(&walk, b, p.point_m));
            match (na, nb) {
                (Some(x), Some(y)) => {
                    let mut opened = cut(&mut corridors, &mut walk, center(x), p.point_m, p.width_m, &[a, b]);
                    opened += cut(&mut corridors, &mut walk, p.point_m, center(y), p.width_m, &[a, b]);
                    let length_m = {
                        let (q1, q2) = (center(x), center(y));
                        (q1[0] - p.point_m[0]).hypot(q1[1] - p.point_m[1]) + (q2[0] - p.point_m[0]).hypot(q2[1] - p.point_m[1])
                    };
                    seams.push(MixedSeam { portal: Some(p.id), gate: None, regions: p.regions, length_m, opened_m2: opened as f32 * cell * cell });
                }
                _ => notes.push(format!("Seams: the opening between regions {a} and {b} found no walkable ground within {REACH_M:.0} m on one side")),
            }
        }
        for g in &cn.gates {
            if m.levels.get(g.region as usize - 1).is_none_or(|l| l.is_none()) {
                continue;
            }
            match nearest(&walk, g.region, g.point_m) {
                Some(x) => {
                    let opened = cut(&mut corridors, &mut walk, center(x), g.point_m, 3.0, &[g.region]);
                    let q = center(x);
                    let length_m = (q[0] - g.point_m[0]).hypot(q[1] - g.point_m[1]);
                    seams.push(MixedSeam { portal: None, gate: Some(g.id), regions: [g.region, g.region], length_m, opened_m2: opened as f32 * cell * cell });
                }
                None => notes.push(format!("Seams: gate #{} found no walkable ground within {REACH_M:.0} m", g.id)),
            }
        }
    }

    // One piece?
    let walkable = CellMask { width: w, height: h, cell_size_m: cell, cells: walk };
    let (labels, count) = grid::components(&walkable);
    if count > 1 {
        let mut size = vec![0usize; count as usize + 1];
        let mut region = vec![0u8; count as usize + 1];
        for (c, &l) in labels.iter().enumerate() {
            size[l as usize] += 1;
            region[l as usize] = ids[c];
        }
        let main = (1..=count as usize).max_by_key(|&l| (size[l], std::cmp::Reverse(l))).unwrap_or(1);
        for l in (1..=count as usize).filter(|&l| l != main) {
            let m2 = size[l] as f32 * cell * cell;
            if m2 >= REPORT_M2 {
                notes.push(format!("Seams: {m2:.0} m² of walkable ground in region {} is cut off from the rest of the map", region[l]));
            }
        }
    }
    SeamBuild { walkable, corridors, seams, notes }
}

#[cfg(test)]
mod tests {
    use crate::model::{LevelConfig, MapKind};

    #[test]
    fn mobs_redo_matches_a_generation_and_reroll_moves_one_region() {
        let mut c = LevelConfig::of_kind(MapKind::Mixed);
        c.map.width_m = 140.0;
        c.map.height_m = 140.0;
        c.cell_size_m = 1.0;
        let lvl = crate::generator::generate(&c, 4);
        let mut same = lvl.clone();
        crate::generator::mixed::redo_mobs(&mut same, &c);
        assert_eq!(same.objects, lvl.objects, "redoing the mobs with the same settings changes nothing");
        c.mixed.regions[0].config.mobs.reroll = 12345;
        let mut moved = lvl.clone();
        crate::generator::mixed::redo_mobs(&mut moved, &c);
        let (a, b) = (lvl.mixed.as_ref().unwrap(), moved.mixed.as_ref().unwrap());
        assert_ne!(a.levels[0].as_ref().unwrap().mobs, b.levels[0].as_ref().unwrap().mobs, "region 1's mobs move");
        for i in 1..a.levels.len() {
            assert_eq!(a.levels[i].as_ref().map(|l| &l.mobs), b.levels[i].as_ref().map(|l| &l.mobs), "region {} keeps its mobs", i + 1);
        }
        // A full generation with the new value gives the same level.
        let again = crate::generator::generate(&c, 4);
        assert_eq!(again.objects, moved.objects);
    }

    #[test]
    fn the_whole_map_is_one_piece_crossing_borders_only_at_openings() {
        for seed in 1..=6 {
            let mut c = LevelConfig::of_kind(MapKind::Mixed);
            c.map.width_m = 160.0;
            c.map.height_m = 160.0;
            c.cell_size_m = 1.0;
            let lvl = crate::generator::generate(&c, seed);
            let m = lvl.mixed.as_ref().unwrap();
            let walk = m.walkable.as_ref().unwrap();
            let (_, pieces) = crate::generator::grid::components(walk);
            assert_eq!(pieces, 1, "seed {seed}: the map is in {pieces} walkable pieces; notes {:?}", lvl.notes);
            // Walkable ground meets across a border only at an opening.
            let (w, cell) = (m.map.width, m.map.cell_size_m);
            let cn = m.connections.as_ref().unwrap();
            for cc in 0..walk.cells.len() {
                for q in [cc + 1, cc + w] {
                    if q >= walk.cells.len() || (q == cc + 1 && q % w == 0) {
                        continue;
                    }
                    let (a, b) = (m.map.ids[cc], m.map.ids[q]);
                    if a == b || !walk.cells[cc] || !walk.cells[q] {
                        continue;
                    }
                    let p = [((cc % w) as f32 + 0.5) * cell, ((cc / w) as f32 + 0.5) * cell];
                    let at_opening = cn.portals.iter().any(|o| {
                        o.regions.contains(&a) && o.regions.contains(&b) && (o.point_m[0] - p[0]).hypot(o.point_m[1] - p[1]) <= o.width_m + 8.0
                    });
                    assert!(at_opening, "seed {seed}: regions {a} and {b} meet walkably at {p:?}, away from any opening");
                }
            }
        }
    }
}
