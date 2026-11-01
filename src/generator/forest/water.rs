//! Forest node 4, water (step F3 in docs/forest-plan.md): streams that flow
//! downhill over the terrain and leave the map, and ponds in the hollows.
//!
//! 1. **Drainage** — a priority flood from the map edge over the height
//!    (rock counts as high ground): every cell gets the neighbor it drains
//!    to, so every drop of water reaches the map edge (through hollows,
//!    which fill up). The flow per cell is the number of cells upstream.
//! 2. **Ponds** — hollows the flood had to fill, at least `pond_min_m2`.
//! 3. **Streams** — the `streams` map-edge outlets with the most flow
//!    (spaced apart); each is traced upstream along the biggest flow until
//!    the catchment is too small for a stream (the spring). Side brooks
//!    branch off where an inflow is big enough (`tributaries_pct`). So a
//!    stream never starts or ends in the open: it rises at a spring,
//!    gathers brooks, grows wider downstream and flows off the map.
//! 4. **Shape** — the cell path is smoothed, wound a little more
//!    (`meander_pct`), and drawn with its width: `width_min_m` at a spring,
//!    up to `width_max_m` at the biggest mouth (by the square root of the
//!    flow). From `deep_from_m` wide on, the middle is deep water; brooks
//!    are wadeable.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::noise::NoiseField;
use crate::level::{CellMask, ForestPond, ForestStream, StreamMouth, TerrainLayout, WaterLayout};
use crate::model::LevelConfig;

pub struct WaterBuild {
    pub layout: WaterLayout,
    pub notes: Vec<String>,
}

/// Rise per cell in a filled hollow (keeps the flow moving).
const EPS: f32 = 1e-5;
/// Fill depth (share of the relief) that counts as a pond.
const POND_DEPTH: f32 = 0.03;
/// Most ponds (the biggest hollows win).
const MAX_PONDS: usize = 3;
/// Least gap between two outlets, meters.
const OUTLET_GAP_M: f32 = 30.0;
/// Deep middle share of a deep stream's width.
const DEEP_SHARE: f32 = 0.6;
/// Shallow rim around a pond, meters.
const POND_RIM_M: f32 = 1.5;
/// Meander wave length, meters, and amplitude at 100% (× width, capped).
/// The bend size the amplitude cap is tuned for (the default `bend_m`).
const MEANDER_WAVE_M: f32 = 14.0;
const MEANDER_AMP: f32 = 0.9;
const MEANDER_MAX_M: f32 = 4.0;

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, terrain: &TerrainLayout) -> WaterBuild {
    build_drainage(&config.forest.water, rng, terrain, &Drainage::default())
}

/// What a mixed map adds to the drainage (docs/mixed-plan.md, M2).
#[derive(Default)]
pub struct Drainage<'a> {
    /// Cells that take the water like the map edge: a stream reaching one
    /// ends there (a swamp drinks it).
    pub sinks: Option<&'a CellMask>,
    /// Cells no stream runs in (a cave massif): the water still drains off
    /// them, but a stream traced uphill stops at their foot — its spring.
    pub dry: Option<&'a CellMask>,
    /// Region borders a stream must cross clearly or keep away from.
    pub borders: Option<Borders<'a>>,
}

/// The map regions' borders for the streams (mixed maps). A low hump runs
/// along every border, so water gathers at its foot, `clear_m` off the
/// border, and crosses only at the hump's lowest spot — a stream crosses a
/// border clearly or keeps away from it, never running along it touching.
#[derive(Clone, Copy)]
pub struct Borders<'a> {
    /// Distance from each cell to the nearest cell of another region,
    /// meters.
    pub dist: &'a [f32],
    /// Least gap between a stream and a border it does not cross, meters.
    pub clear_m: f32,
}

/// Height of the border hump (share of the relief): below the depth that
/// counts as a pond, so it makes none.
const BORDER_HUMP: f32 = 0.025;

/// The water with the extra outlets and dry ground of `extra` (none: the
/// forest's own water).
pub fn build_drainage(s: &crate::model::ForestWaterSettings, rng: &mut ChaCha8Rng, terrain: &TerrainLayout, extra: &Drainage) -> WaterBuild {
    let (w, h, cell) = (terrain.width, terrain.height, terrain.cell_size_m);
    let is_sink = |c: usize| extra.sinks.is_some_and(|m| m.cells[c]);
    let is_dry = |c: usize| extra.dry.is_some_and(|m| m.cells[c]);
    let n = w * h;
    let mut notes = Vec::new();
    let meander_noise = NoiseField::new(rng.random::<u64>());

    // 1. Drainage.
    let hump = |c: usize| {
        extra.borders.map_or(0.0, |b| {
            let reach = b.clear_m * 1.6 + 2.0;
            let t = (b.dist[c] / reach).clamp(0.0, 1.0);
            BORDER_HUMP * (1.0 - t * t * (3.0 - 2.0 * t))
        })
    };
    let ground: Vec<f32> = (0..n).map(|c| terrain.height_m[c] + if terrain.rock.cells[c] { 1.0 } else { 0.0 } + hump(c)).collect();
    let mut filled = vec![f32::INFINITY; n];
    let mut down = vec![usize::MAX; n];
    let mut order = Vec::with_capacity(n);
    let mut heap = BinaryHeap::new();
    let mut seen = vec![false; n];
    for c in 0..n {
        let (x, y) = (c % w, c / w);
        if x == 0 || y == 0 || x + 1 == w || y + 1 == h || is_sink(c) {
            seen[c] = true;
            filled[c] = ground[c];
            heap.push(Reverse((ground[c].to_bits(), c)));
        }
    }
    while let Some(Reverse((_, c))) = heap.pop() {
        order.push(c);
        for nb in neighbors8(c, w, h) {
            if !seen[nb] {
                seen[nb] = true;
                filled[nb] = ground[nb].max(filled[c] + EPS);
                down[nb] = c;
                heap.push(Reverse((filled[nb].to_bits(), nb)));
            }
        }
    }
    let mut flow = vec![1u32; n];
    for &c in order.iter().rev() {
        if down[c] != usize::MAX {
            flow[down[c]] += flow[c];
        }
    }

    // 2. Ponds.
    let mut pond_id = vec![0u32; n];
    let mut ponds = Vec::new();
    let mut shallow = CellMask::new(w, h, cell);
    let mut deep = CellMask::new(w, h, cell);
    if s.pond_min_m2 > 0.0 {
        let hollow = CellMask { width: w, height: h, cell_size_m: cell, cells: (0..n).map(|c| filled[c] - ground[c] > POND_DEPTH && !is_dry(c)).collect() };
        let (labels, count) = crate::generator::grid::components(&hollow);
        let mut cells_of: Vec<Vec<usize>> = vec![Vec::new(); count as usize + 1];
        for (c, &l) in labels.iter().enumerate() {
            if l > 0 {
                cells_of[l as usize].push(c);
            }
        }
        // Roundish hollows only: a long one is a valley, not a pond.
        let compact = |v: &Vec<usize>| {
            let (x0, x1) = v.iter().fold((usize::MAX, 0), |(a, b), &c| (a.min(c % w), b.max(c % w)));
            let (y0, y1) = v.iter().fold((usize::MAX, 0), |(a, b), &c| (a.min(c / w), b.max(c / w)));
            let (bw, bh) = ((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
            v.len() as f32 >= 0.4 * bw * bh && bw.max(bh) <= 3.0 * bw.min(bh)
        };
        let mut hollows: Vec<&Vec<usize>> = cells_of.iter().skip(1).filter(|v| v.len() as f32 * cell * cell >= s.pond_min_m2 && compact(v)).collect();
        hollows.sort_by_key(|v| (Reverse(v.len()), v[0]));
        hollows.truncate(MAX_PONDS);
        hollows.sort_by_key(|v| v[0]);
        for cells in hollows {
            let area = cells.len() as f32 * cell * cell;
            let id = ponds.len() as u32 + 1;
            let (mut sx, mut sy) = (0.0, 0.0);
            let max_depth = cells.iter().map(|&c| filled[c] - ground[c]).fold(0.0, f32::max);
            for &c in cells {
                pond_id[c] = id;
                sx += ((c % w) as f32 + 0.5) * cell;
                sy += ((c / w) as f32 + 0.5) * cell;
                // Deep in the middle of a big enough pond.
                if area >= 2.0 * s.pond_min_m2 && filled[c] - ground[c] > max_depth * 0.4 {
                    deep.cells[c] = true;
                } else {
                    shallow.cells[c] = true;
                }
            }
            let k = cells.len() as f32;
            ponds.push(ForestPond { id, center_m: [sx / k, sy / k], area_m2: area });
        }
        // Shallow rim.
        let r = (POND_RIM_M / cell).ceil() as i64;
        let wet: Vec<usize> = (0..n).filter(|&c| pond_id[c] > 0).collect();
        for c in wet {
            stamp(&mut shallow, &deep, w, h, c, r, |_| true);
        }
    }

    // 3. Streams. Children in flow order (CSR), the biggest first.
    let mut start = vec![0usize; n + 1];
    for &d in &down {
        if d != usize::MAX {
            start[d + 1] += 1;
        }
    }
    for i in 0..n {
        start[i + 1] += start[i];
    }
    let mut fill_at = start.clone();
    let mut kids = vec![0usize; start[n]];
    for (c, &d) in down.iter().enumerate() {
        if d != usize::MAX {
            kids[fill_at[d]] = c;
            fill_at[d] += 1;
        }
    }
    for c in 0..n {
        kids[start[c]..start[c + 1]].sort_by_key(|&k| (Reverse(flow[k]), k));
    }
    let spring = (s.spring_m2.max(cell * cell) / (cell * cell)).ceil() as u32;
    let trib = (s.tributaries_pct / 100.0).clamp(0.0, 1.0);
    // (cells spring → mouth, mouth)
    let mut raw: Vec<(Vec<usize>, StreamMouth)> = Vec::new();
    let trace_up = |from: usize| {
        let mut path = vec![from];
        let mut c = from;
        loop {
            let ks = &kids[start[c]..start[c + 1]];
            match ks.first() {
                Some(&k) if flow[k] >= spring && !is_dry(k) => {
                    path.push(k);
                    c = k;
                }
                _ => break,
            }
        }
        path.reverse();
        path
    };
    // The outlets: by the biggest catchment, or with Prefer long also by
    // the length of the main stream; too short ones skipped.
    let mut outlets: Vec<usize> = (0..n).filter(|&c| down[c] == usize::MAX && flow[c] >= spring * 3 && !is_dry(c)).collect();
    let prefer = (s.prefer_long_pct / 100.0).clamp(0.0, 1.0);
    // Outlets from this index on are shorter than Min length.
    let mut short_from = usize::MAX;
    let min_cells = (s.min_length_m.max(0.0) / cell).ceil() as usize;
    if prefer > 0.0 || min_cells > 0 {
        let len: Vec<usize> = outlets.iter().map(|&o| trace_up(o).len()).collect();
        let (fmax, lmax) = (outlets.iter().map(|&o| flow[o]).max().unwrap_or(1).max(1) as f32, len.iter().copied().max().unwrap_or(1).max(1) as f32);
        // Long enough first (by score), then the too short ones, longest
        // first — they only fill slots nothing long enough could.
        let mut scored: Vec<(bool, f32, usize)> = outlets
            .iter()
            .zip(&len)
            .map(|(&o, &l)| {
                let score = (flow[o] as f32 / fmax).powf(1.0 - prefer) * (l as f32 / lmax).powf(prefer);
                (l < min_cells, if l < min_cells { l as f32 } else { score }, o)
            })
            .collect();
        scored.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)).then(a.2.cmp(&b.2)));
        short_from = scored.iter().position(|x| x.0).unwrap_or(scored.len());
        outlets = scored.into_iter().map(|(_, _, o)| o).collect();
    } else {
        outlets.sort_by_key(|&c| (Reverse(flow[c]), c));
    }
    let mut chosen: Vec<usize> = Vec::new();
    let mut short = 0;
    for (i, &o) in outlets.iter().enumerate() {
        if chosen.len() >= s.streams as usize {
            break;
        }
        let far = chosen.iter().all(|&q| {
            let (dx, dy) = ((o % w) as f32 - (q % w) as f32, (o / w) as f32 - (q / w) as f32);
            dx.hypot(dy) * cell >= OUTLET_GAP_M
        });
        if far {
            chosen.push(o);
            short += usize::from(i >= short_from);
        }
    }
    if (chosen.len() as u32) < s.streams {
        notes.push(format!("Water: {} of {} streams found a catchment big enough", chosen.len(), s.streams));
    }
    if short > 0 {
        notes.push(format!("Water: {short} stream(s) shorter than the {:.0} m Min length — no longer one fits this land", s.min_length_m));
    }
    let max_flow = chosen.iter().map(|&o| flow[o]).max().unwrap_or(1).max(1) as f32;
    for &o in &chosen {
        let main = trace_up(o);
        let main_id = raw.len() as u32 + 1;
        let outlet_mouth = if is_sink(o) { StreamMouth::Sink } else { StreamMouth::Edge };
        let trib_min = if trib > 0.0 { ((flow[o] as f32) * (0.5 - 0.46 * trib)).max(spring as f32 * 2.0) as u32 } else { u32::MAX };
        // Side brooks off the main stream, then off those (two levels).
        let mut queue = vec![(main.clone(), main_id)];
        raw.push((main, outlet_mouth));
        let mut level = 0;
        while !queue.is_empty() && level < 2 {
            let mut next = Vec::new();
            for (path, id) in queue {
                for &c in &path {
                    for &k in kids[start[c]..start[c + 1]].iter().skip(1) {
                        if flow[k] >= trib_min && !is_dry(k) && !path.contains(&k) {
                            let sub = trace_up(k);
                            let mouth = if pond_id[c] > 0 { StreamMouth::Pond(pond_id[c]) } else { StreamMouth::Stream(id) };
                            let sid = raw.len() as u32 + 1;
                            let mut full = sub.clone();
                            full.push(c);
                            raw.push((full.clone(), mouth));
                            next.push((full, sid));
                        }
                    }
                }
            }
            queue = next;
            level += 1;
        }
    }

    // 4. Shape and draw.
    let wlo = s.width_min_m.min(s.width_max_m).max(cell * 0.5);
    let whi = s.width_max_m.max(s.width_min_m).max(cell * 0.5);
    let width_of = |f: u32| wlo + (whi - wlo) * (f as f32 / max_flow).sqrt().min(1.0);
    let meander = (s.meander_pct / 100.0).clamp(0.0, 1.0);
    let bend = s.bend_m.max(2.0);
    let amp_cap = MEANDER_MAX_M * (bend / MEANDER_WAVE_M).max(1.0);
    let width_var = (s.width_var_pct / 100.0).clamp(0.0, 1.0);
    let braid = (s.braid_pct / 100.0).clamp(0.0, 1.0);
    let oxbow = (s.oxbow_pct / 100.0).clamp(0.0, 1.0);
    let mut streams = Vec::new();
    // Mixed maps: room to a region border at a point, meters (the swing of
    // a meander, a braid or an oxbow never reaches over it).
    let room = |p: [f32; 2]| {
        extra.borders.map_or(f32::INFINITY, |b| {
            let c = ((p[1] / cell).floor().clamp(0.0, h as f32 - 1.0) as usize) * w + (p[0] / cell).floor().clamp(0.0, w as f32 - 1.0) as usize;
            b.dist[c]
        })
    };
    for (i, (cells, mouth)) in raw.iter().enumerate() {
        let id = i as u32 + 1;
        // Mixed maps: a spring on a region border (other than a massif's
        // foot, where the water comes out of the mountain) moves down the
        // stream until it is clear of the border.
        let cells: &[usize] = match extra.borders {
            Some(b) => {
                let near_dry = |c: usize| {
                    let r = (b.clear_m / cell).ceil() as i64;
                    let (x, y) = ((c % w) as i64, (c / w) as i64);
                    (-r..=r).any(|dy| (-r..=r).any(|dx| {
                        let (nx, ny) = (x + dx, y + dy);
                        nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 && is_dry(ny as usize * w + nx as usize)
                    }))
                };
                let skip = cells.iter().take_while(|&&c| b.dist[c] < b.clear_m && !near_dry(c)).count();
                &cells[skip.min(cells.len())..]
            }
            None => cells,
        };
        if cells.len() < 2 {
            continue;
        }
        // Every 2nd cell, then smoothed.
        let mut pts: Vec<([f32; 2], f32)> = cells
            .iter()
            .enumerate()
            .filter(|(k, _)| k % 2 == 0 || *k == cells.len() - 1)
            .map(|(_, &c)| ([((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell], width_of(flow[c])))
            .collect();
        for _ in 0..2 {
            pts = chaikin(&pts);
        }
        // Meander: a sideways wave along the length, none at the ends.
        let total: f32 = pts.windows(2).map(|p| dist(p[0].0, p[1].0)).sum();
        let mut along = 0.0;
        let mut wound = Vec::with_capacity(pts.len());
        // Per point: distance along, meander offset and its cap, the fade.
        let mut wave: Vec<(f32, f32, f32, f32)> = Vec::with_capacity(pts.len());
        for k in 0..pts.len() {
            if k > 0 {
                along += dist(pts[k - 1].0, pts[k].0);
            }
            let (prev, next) = (pts[k.saturating_sub(1)].0, pts[(k + 1).min(pts.len() - 1)].0);
            let (tx, ty) = (next[0] - prev[0], next[1] - prev[1]);
            let tl = tx.hypot(ty).max(1e-6);
            let fade = (along.min(total - along) / 10.0).clamp(0.0, 1.0);
            // Near a border the swing shrinks to the room left.
            let amp = (meander * MEANDER_AMP * pts[k].1 * 2.0).min(amp_cap).min((room(pts[k].0) - pts[k].1 * 0.5 - 1.0).max(0.0) / 1.2) * fade;
            let off = (meander_noise.fbm(along / bend + id as f32 * 7.3, id as f32 * 3.1, 2) - 0.5) * 4.0 * amp;
            // Pools and narrows (none at the ends).
            let mut wd = pts[k].1;
            if width_var > 0.0 {
                let v = meander_noise.fbm(along / (bend * 1.3) + id as f32 * 5.1, 41.7, 2) - 0.5;
                wd = (wd * (1.0 + width_var * v * 1.6 * fade)).max(cell * 0.5);
            }
            wound.push(([pts[k].0[0] - ty / tl * off, pts[k].0[1] + tx / tl * off], wd));
            wave.push((along, off, amp, fade));
        }
        let is_deep = wound.iter().any(|p| p.1 >= s.deep_from_m);
        // Braids: on wide stretches the stream splits into two channels
        // around an island and joins again.
        let split: Vec<f32> = if braid > 0.0 {
            let t = 0.68 - 0.3 * braid;
            wound
                .iter()
                .zip(&wave)
                .map(|(p, &(al, _, _, fade))| {
                    let v = meander_noise.fbm(al / (bend * 2.5) + id as f32 * 9.7, 13.3, 2);
                    smoothstep(t, t + 0.08, v) * smoothstep(2.0, 3.5, p.1) * fade * smoothstep(p.1 * 1.5, p.1 * 2.5 + 2.0, room(p.0))
                })
                .collect()
        } else {
            Vec::new()
        };
        // Braids and oxbows follow the smooth line under the meander (the
        // wound line turns at every bend; offsets along it would cross).
        let base_n = normals(&pts);
        if split.iter().any(|&b| b > 0.0) {
            let normals = &base_n;
            for side in [1.0f32, -1.0] {
                let chan: Vec<([f32; 2], f32)> = wound
                    .iter()
                    .zip(normals)
                    .zip(&split)
                    .map(|((p, nrm), &b)| {
                        let off = side * b * p.1 * 0.9;
                        ([p.0[0] + nrm[0] * off, p.0[1] + nrm[1] * off], p.1 * (1.0 - 0.45 * b))
                    })
                    .collect();
                for seg in chan.windows(2) {
                    draw_segment(&mut shallow, &mut deep, w, h, cell, seg[0], seg[1], s.deep_from_m);
                }
            }
        } else {
            for seg in wound.windows(2) {
                draw_segment(&mut shallow, &mut deep, w, h, cell, seg[0], seg[1], s.deep_from_m);
            }
        }
        // Oxbow lakes: beside the strongest bends, a cut-off crescent of the
        // old loop, outside the bend.
        if oxbow > 0.0 && wound.len() > 4 {
            let mut last_at = f32::NEG_INFINITY;
            for k in 1..wound.len() - 1 {
                let (al, off, amp, _) = wave[k];
                // The apex of a strong bend, one per bend at most.
                let strong = amp > 0.8 && off.abs() >= 0.7 * amp && off.abs() >= wave[k - 1].1.abs() && off.abs() > wave[k + 1].1.abs();
                if !strong || al - last_at < bend * 1.5 || unit_hash(id, k as u32) >= oxbow {
                    continue;
                }
                last_at = al;
                let wd = wound[k].1;
                let side = off.signum();
                if room(wound[k].0) < wd * 2.0 + amp * 1.5 + 6.0 {
                    continue;
                }
                // In the smooth frame: the river's apex offset, then a gap,
                // then a crescent bowing out (the cut-off old loop).
                let gap = wd * 0.5 + 2.5;
                let bow = amp * 0.6 + wd * 0.5;
                let half = bend * 0.35;
                let arc: Vec<([f32; 2], f32)> = (0..pts.len())
                    .filter(|&j| (wave[j].0 - al).abs() <= half)
                    .map(|j| {
                        let u = (wave[j].0 - al) / half;
                        let o = off + side * (gap + wd * 0.35 + bow * (1.0 - u * u));
                        ([pts[j].0[0] + base_n[j][0] * o, pts[j].0[1] + base_n[j][1] * o], (wd * 0.7 * (1.0 - 0.6 * u * u)).max(cell))
                    })
                    .collect();
                if arc.len() < 3 {
                    continue;
                }
                for seg in arc.windows(2) {
                    draw_segment(&mut shallow, &mut deep, w, h, cell, seg[0], seg[1], s.deep_from_m);
                }
                let mid = arc[arc.len() / 2].0;
                let length: f32 = arc.windows(2).map(|p| dist(p[0].0, p[1].0)).sum();
                ponds.push(ForestPond { id: ponds.len() as u32 + 1, center_m: mid, area_m2: length * wd * 0.6 });
            }
        }
        let path_m: Vec<[f32; 2]> = wound.iter().map(|p| p.0).collect();
        streams.push(ForestStream {
            id,
            length_m: path_m.windows(2).map(|p| dist(p[0], p[1])).sum(),
            width_m: [wound[0].1, wound[wound.len() - 1].1],
            path_m,
            mouth: *mouth,
            deep: is_deep,
        });
    }
    for c in 0..n {
        if deep.cells[c] || is_sink(c) || is_dry(c) {
            shallow.cells[c] = false;
        }
        if is_sink(c) || is_dry(c) {
            deep.cells[c] = false;
        }
    }
    WaterBuild { layout: WaterLayout { shallow, deep, streams, ponds }, notes }
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Unit normals (left of the direction of travel) of a polyline.
fn normals(pts: &[([f32; 2], f32)]) -> Vec<[f32; 2]> {
    (0..pts.len())
        .map(|k| {
            let (prev, next) = (pts[k.saturating_sub(1)].0, pts[(k + 1).min(pts.len() - 1)].0);
            let (tx, ty) = (next[0] - prev[0], next[1] - prev[1]);
            let tl = tx.hypot(ty).max(1e-6);
            [-ty / tl, tx / tl]
        })
        .collect()
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A stable value in 0..1 from two numbers (no random stream: the water's
/// draws stay the same whatever is switched on).
fn unit_hash(a: u32, b: u32) -> f32 {
    let mut x = (a as u64) << 32 | b as u64;
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// One Chaikin pass (keeps the end points).
fn chaikin(pts: &[([f32; 2], f32)]) -> Vec<([f32; 2], f32)> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut out = vec![pts[0]];
    for p in pts.windows(2) {
        let (a, b) = (p[0], p[1]);
        let lerp = |t: f32| ([a.0[0] + (b.0[0] - a.0[0]) * t, a.0[1] + (b.0[1] - a.0[1]) * t], a.1 + (b.1 - a.1) * t);
        out.push(lerp(0.25));
        out.push(lerp(0.75));
    }
    out.push(pts[pts.len() - 1]);
    out
}

/// Paints the water of one stream segment (width interpolated).
#[allow(clippy::too_many_arguments)]
fn draw_segment(shallow: &mut CellMask, deep: &mut CellMask, w: usize, h: usize, cell: f32, a: ([f32; 2], f32), b: ([f32; 2], f32), deep_from: f32) {
    let r = a.1.max(b.1) / 2.0 + cell;
    let (x0, x1) = (((a.0[0].min(b.0[0]) - r) / cell).floor().max(0.0) as usize, (((a.0[0].max(b.0[0]) + r) / cell).ceil() as usize).min(w));
    let (y0, y1) = (((a.0[1].min(b.0[1]) - r) / cell).floor().max(0.0) as usize, (((a.0[1].max(b.0[1]) + r) / cell).ceil() as usize).min(h));
    let (dx, dy) = (b.0[0] - a.0[0], b.0[1] - a.0[1]);
    let len2 = (dx * dx + dy * dy).max(1e-9);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = ((x as f32 + 0.5) * cell, (y as f32 + 0.5) * cell);
            let t = (((p.0 - a.0[0]) * dx + (p.1 - a.0[1]) * dy) / len2).clamp(0.0, 1.0);
            let q = (a.0[0] + dx * t, a.0[1] + dy * t);
            let d = (p.0 - q.0).hypot(p.1 - q.1);
            let wd = a.1 + (b.1 - a.1) * t;
            let c = y * w + x;
            if d <= wd / 2.0 {
                if wd >= deep_from && d <= wd / 2.0 * DEEP_SHARE {
                    deep.cells[c] = true;
                } else {
                    shallow.cells[c] = true;
                }
            }
        }
    }
}

/// Marks shallow every cell within `r` cells of `c` that is not deep.
fn stamp(shallow: &mut CellMask, deep: &CellMask, w: usize, h: usize, c: usize, r: i64, keep: impl Fn(usize) -> bool) {
    let (x, y) = ((c % w) as i64, (c / w) as i64);
    for dy in -r..=r {
        for dx in -r..=r {
            let (nx, ny) = (x + dx, y + dy);
            if dx * dx + dy * dy > r * r || nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                continue;
            }
            let q = ny as usize * w + nx as usize;
            if !deep.cells[q] && keep(q) {
                shallow.cells[q] = true;
            }
        }
    }
}

pub(crate) fn neighbors8(c: usize, w: usize, h: usize) -> impl Iterator<Item = usize> {
    let (x, y) = ((c % w) as i64, (c / w) as i64);
    [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)].into_iter().filter_map(move |(dx, dy)| {
        let (nx, ny) = (x + dx, y + dy);
        (nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64).then(|| ny as usize * w + nx as usize)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::generator::forest::{areas, terrain};
    use crate::model::MapKind;

    fn run(seed: u64, edit: impl Fn(&mut LevelConfig)) -> WaterLayout {
        let mut c = LevelConfig { kind: MapKind::Forest, ..LevelConfig::default() };
        c.map.width_m = 160.0;
        c.map.height_m = 120.0;
        c.cell_size_m = 1.0;
        edit(&mut c);
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let a = areas::build(&c, &mut rng, None).layout;
        let t = terrain::build(&c, &mut rng, &a);
        build(&c, &mut rng, &t).layout
    }

    fn main_lengths(l: &WaterLayout) -> Vec<f32> {
        l.streams.iter().filter(|s| s.mouth == StreamMouth::Edge).map(|s| s.length_m).collect()
    }

    #[test]
    fn length_settings_give_longer_streams() {
        let (mut plain, mut long) = (0.0, 0.0);
        for seed in 0..6 {
            plain += main_lengths(&run(seed, |c| c.forest.water.streams = 2)).iter().sum::<f32>();
            let l = run(seed, |c| {
                c.forest.water.streams = 2;
                c.forest.water.spring_m2 = 150.0;
                c.forest.water.prefer_long_pct = 100.0;
                c.forest.water.min_length_m = 60.0;
            });
            long += main_lengths(&l).iter().sum::<f32>();
        }
        assert!(long > plain * 1.1, "long {long} vs plain {plain}");
    }

    #[test]
    fn oxbows_are_separate_lakes_and_braids_leave_islands() {
        let (mut oxbows, mut islands) = (0, 0);
        for seed in 0..6 {
            let plain = run(seed, |c| c.forest.water.meander_pct = 100.0);
            let l = run(seed, |c| {
                c.forest.water.meander_pct = 100.0;
                c.forest.water.oxbow_pct = 100.0;
                c.forest.water.braid_pct = 100.0;
                c.forest.water.width_max_m = 10.0;
            });
            oxbows += l.ponds.len().saturating_sub(plain.ponds.len());
            // Islands: dry pieces with no way to the map edge.
            let dry = CellMask { width: l.deep.width, height: l.deep.height, cell_size_m: 1.0, cells: l.deep.cells.iter().map(|&d| !d).collect() };
            let (labels, count) = crate::generator::grid::components(&dry);
            let (w, h) = (dry.width, dry.height);
            let mut on_edge = vec![false; count as usize + 1];
            for (c, &lb) in labels.iter().enumerate() {
                if c % w == 0 || c / w == 0 || c % w == w - 1 || c / w == h - 1 {
                    on_edge[lb as usize] = true;
                }
            }
            islands += (1..=count as usize).filter(|&i| !on_edge[i]).count();
        }
        assert!(oxbows > 0, "no oxbow lake");
        assert!(islands > 0, "no braid island");
    }

    #[test]
    fn main_streams_leave_the_map_and_brooks_join_them() {
        for seed in 0..5 {
            let l = run(seed, |c| c.forest.water.streams = 2);
            assert!(!l.streams.is_empty(), "seed {seed}");
            for st in &l.streams {
                match st.mouth {
                    StreamMouth::Edge => {
                        let m = st.path_m[st.path_m.len() - 1];
                        let edge = m[0].min(m[1]).min(160.0 - m[0]).min(120.0 - m[1]);
                        assert!(edge < 2.0, "seed {seed}: stream {} ends at {m:?}", st.id);
                    }
                    StreamMouth::Stream(p) => assert!(l.streams.iter().any(|x| x.id == p)),
                    StreamMouth::Pond(p) => assert!(l.ponds.iter().any(|x| x.id == p)),
                    StreamMouth::Sink => panic!("seed {seed}: a forest has no sinks"),
                }
                assert!(st.width_m[0] <= st.width_m[1] + 1e-3, "seed {seed}: narrows downstream");
            }
        }
    }

    #[test]
    fn no_tributaries_at_zero() {
        let l = run(1, |c| c.forest.water.tributaries_pct = 0.0);
        assert!(l.streams.iter().all(|s| s.mouth == StreamMouth::Edge));
        let l = run(1, |c| c.forest.water.streams = 0);
        assert!(l.streams.is_empty());
    }
}
