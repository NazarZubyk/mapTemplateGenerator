//! Cave node 3, chambers (step C2 in docs/cave-plan.md): the halls and
//! grottos carved out of the rock. The tunnels node links them later.
//!
//! 1. **Centers** — `hall_count` halls first, then the grottos (a count or
//!    a density). Each takes the best of 32 random spots: the one farthest
//!    from the chambers placed so far (edge to edge, plus `gap_m`), inside
//!    the `rock_margin_m`. No spot clear → it shrinks (×0.8, up to 3
//!    times), then it is skipped with a note.
//! 2. **Style** — a chamber takes the style of the zone at its center
//!    (style node).
//! 3. **Shape** — natural: an ellipse of the chamber's area, stretched by
//!    `elongation_pct`, domain-warped and roughened by `irregularity_pct`
//!    (like the swamp islands). Hand-made: a rectangle of the same area,
//!    aspect up to `room_aspect_max`, axis-aligned or rotated up to
//!    `room_rotation_deg`, its walls wandering by `room_roughness_pct`
//!    (dug by hand, not cut by tools). Natural keeps the best-scoring cells
//!    of its window up to the area; hand-made keeps the cells inside its
//!    (wobbly) rectangle.
//! 4. **Gap** — two chambers never come closer than `gap_m` (the weaker
//!    cell goes), so there is always rock between them.
//! 5. **Tidy** — each chamber keeps its largest piece, holes filled; one
//!    left with under a fifth of its area is dropped (note).

use std::f32::consts::PI;

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::noise::NoiseField;
use crate::generator::swamp::islands::tidy;
use crate::level::{CaveStyle, Chamber, ChamberKind, ChamberLayout, StyleLayout};
use crate::model::{LevelConfig, NodeAmount};

pub struct ChamberBuild {
    pub layout: ChamberLayout,
    pub notes: Vec<String>,
}

/// Most grottos, whatever the density says.
const MAX_GROTTOS: u32 = 400;
const CANDIDATES: usize = 32;
/// Hand-made room walls wander by up to this at 100% roughness, meters
/// (the noise rarely reaches its ends, so the walls stay within ~1 m).
const ROOM_WOBBLE_M: f32 = 3.0;
/// Feature size of that wander, meters.
const ROOM_WOBBLE_SCALE_M: f32 = 3.5;

/// One chamber before rasterizing.
struct Seed {
    kind: ChamberKind,
    style: CaveStyle,
    zone: u16,
    center: (f32, f32),
    /// Radius of the target area, meters.
    r: f32,
    /// Long axis, radians, in map coordinates (y down).
    angle: f32,
    /// Natural: stretch of the ellipse. Hand-made: length / width.
    stretch: f32,
}

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, style: &StyleLayout) -> ChamberBuild {
    let s = &config.cave.chambers;
    let (w, h, cell) = (style.width, style.height, style.cell_size_m);
    let (wm, hm) = (w as f32 * cell, h as f32 * cell);
    let irr = (s.irregularity_pct / 100.0).clamp(0.0, 1.0);
    let elong = (s.elongation_pct / 100.0).clamp(0.0, 1.0);
    let room_rough = (s.room_roughness_pct / 100.0).clamp(0.0, 1.0);
    let margin_m = s.rock_margin_m.max(0.0);
    let mut notes = Vec::new();
    let noise = NoiseField::new(rng.random::<u64>());

    // 1 + 2. Centers and styles.
    let grottos = match s.grotto_amount {
        NodeAmount::Count => s.grotto_count,
        NodeAmount::Density => (s.grotto_per_1000m2.max(0.0) * wm * hm / 1000.0).round() as u32,
    }
    .min(MAX_GROTTOS);
    let wanted = (0..s.hall_count)
        .map(|_| (ChamberKind::Hall, s.hall_min_m, s.hall_max_m))
        .chain((0..grottos).map(|_| (ChamberKind::Grotto, s.grotto_min_m, s.grotto_max_m)));
    let mut seeds: Vec<Seed> = Vec::new();
    let mut skipped = 0;
    for (kind, lo, hi) in wanted {
        let (lo, hi) = (lo.min(hi).max(cell), lo.max(hi).max(cell));
        let mut r = if hi - lo < 1e-6 { lo } else { rng.random_range(lo..=hi) } / 2.0;
        let turn: f32 = rng.random_range(0.0..1.0);
        let stretch_roll: f32 = rng.random_range(0.0..=1.0);
        let tilt_roll: f32 = rng.random_range(-1.0..=1.0);
        let mut placed = None;
        for _ in 0..4 {
            let margin = margin_m + r;
            let best = (0..CANDIDATES)
                .map(|_| {
                    let mut span = |size: f32| {
                        if size - 2.0 * margin > 0.0 { rng.random_range(margin..size - margin) } else { size / 2.0 }
                    };
                    let p = (span(wm), span(hm));
                    let clear = seeds
                        .iter()
                        .map(|o| (p.0 - o.center.0).hypot(p.1 - o.center.1) - r - o.r - s.gap_m)
                        .fold(f32::INFINITY, f32::min);
                    (p, clear)
                })
                .max_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((p, clear)) = best
                && clear >= 0.0
                && wm >= 2.0 * margin
                && hm >= 2.0 * margin
            {
                placed = Some(p);
                break;
            }
            r *= 0.8;
        }
        let Some(center) = placed else {
            skipped += 1;
            continue;
        };
        let c = ((center.1 / cell) as usize).min(h - 1) * w + ((center.0 / cell) as usize).min(w - 1);
        let (zone, style) = (style.ids[c], style.style_at(c));
        let (angle, stretch) = match style {
            CaveStyle::Natural => (turn * PI, 1.0 + elong * 1.5 * stretch_roll),
            CaveStyle::HandMade => {
                // Axis-aligned (long side east or south), turned at most
                // room_rotation_deg.
                let axis = if turn < 0.5 { 0.0 } else { PI / 2.0 };
                let tilt = s.room_rotation_deg.clamp(0.0, 45.0).to_radians() * tilt_roll;
                (axis + tilt, 1.0 + (s.room_aspect_max.max(1.0) - 1.0) * stretch_roll)
            }
        };
        seeds.push(Seed { kind, style, zone, center, r, angle, stretch });
    }
    if skipped > 0 {
        notes.push(format!("Chambers: {skipped} did not fit — fewer or smaller chambers, a smaller gap or rock margin"));
    }

    // 3. Shape: per chamber, its best cells by score; a contested cell goes
    // to the higher score.
    let margin = (margin_m / cell).round() as usize;
    let mut owner = vec![0usize; w * h]; // seed index + 1
    let mut score = vec![f32::NEG_INFINITY; w * h];
    for (i, seed) in seeds.iter().enumerate() {
        let (a, b) = (seed.r * seed.stretch.sqrt(), seed.r / seed.stretch.sqrt());
        let reach = a * 1.6 + cell;
        let (sin, cos) = seed.angle.sin_cos();
        let feature = (seed.r * 0.7).max(2.0);
        let salt = i as f32 * 37.13;
        let x0 = ((seed.center.0 - reach) / cell).floor().max(0.0) as usize;
        let x1 = (((seed.center.0 + reach) / cell).ceil() as usize).min(w);
        let y0 = ((seed.center.1 - reach) / cell).floor().max(0.0) as usize;
        let y1 = (((seed.center.1 + reach) / cell).ceil() as usize).min(h);
        let mut cells: Vec<(f32, usize)> = Vec::new();
        for y in y0..y1 {
            for x in x0..x1 {
                if x < margin || y < margin || x + margin >= w || y + margin >= h {
                    continue;
                }
                let (px, py) = ((x as f32 + 0.5) * cell, (y as f32 + 0.5) * cell);
                let sc = match seed.style {
                    CaveStyle::Natural => {
                        let (nx, ny) = (px / feature + salt, py / feature - salt);
                        let warp = irr * 0.5 * seed.r;
                        let wx = px + warp * (noise.fbm(nx + 5.2, ny + 1.3, 3) - 0.5) * 2.0;
                        let wy = py + warp * (noise.fbm(nx - 3.7, ny + 8.1, 3) - 0.5) * 2.0;
                        let (dx, dy) = (wx - seed.center.0, wy - seed.center.1);
                        let (u, v) = (dx * cos + dy * sin, -dx * sin + dy * cos);
                        let rough = irr * 0.8 * (noise.fbm(nx * 2.0, ny * 2.0, 4) - 0.5) * 2.0;
                        1.0 - ((u / a).powi(2) + (v / b).powi(2)) + rough
                    }
                    CaveStyle::HandMade => {
                        let (dx, dy) = (px - seed.center.0, py - seed.center.1);
                        let (u, v) = (dx * cos + dy * sin, -dx * sin + dy * cos);
                        // Meters inside the nearest wall; half sides
                        // (a, b)·√π/2 give area π·a·b = π·r². Hand-dug
                        // walls wander by up to ROOM_WOBBLE_M.
                        let k = PI.sqrt() / 2.0;
                        let wobble = room_rough * ROOM_WOBBLE_M * (noise.fbm(px / ROOM_WOBBLE_SCALE_M + salt, py / ROOM_WOBBLE_SCALE_M - salt, 2) - 0.5) * 2.0;
                        (a * k - u.abs()).min(b * k - v.abs()) + wobble
                    }
                };
                cells.push((sc, y * w + x));
            }
        }
        cells.sort_by(|p, q| q.0.total_cmp(&p.0).then(p.1.cmp(&q.1)));
        // Natural: the best cells up to the area. Hand-made: exactly the
        // cells inside the rectangle (a cut by count would leave a ragged
        // last row).
        let target = match seed.style {
            CaveStyle::Natural => (PI * seed.r * seed.r / (cell * cell)).round().max(1.0) as usize,
            CaveStyle::HandMade => cells.iter().filter(|p| p.0 >= 0.0).count().max(1),
        };
        for &(sc, c) in cells.iter().take(target) {
            if sc > score[c] {
                score[c] = sc;
                owner[c] = i + 1;
            }
        }
    }

    // 4. Gap: the weaker cell of every too-close pair goes.
    let g = (s.gap_m.max(0.0) / cell).ceil() as i64;
    let gm = s.gap_m.max(0.0) / cell;
    let mut drop = vec![false; w * h];
    if g > 0 {
        for c in 0..w * h {
            let i = owner[c];
            if i == 0 {
                continue;
            }
            let (cx, cy) = ((c % w) as i64, (c / w) as i64);
            'scan: for dy in -g..=g {
                for dx in -g..=g {
                    let (nx, ny) = (cx + dx, cy + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 || ((dx * dx + dy * dy) as f32) > gm * gm {
                        continue;
                    }
                    let n = ny as usize * w + nx as usize;
                    let j = owner[n];
                    if j != 0 && j != i && (score[n] > score[c] || (score[n] == score[c] && j < i)) {
                        drop[c] = true;
                        break 'scan;
                    }
                }
            }
        }
    }
    for c in 0..w * h {
        if drop[c] {
            owner[c] = 0;
        }
    }

    // 5. Tidy.
    let mut ids = vec![0u16; w * h];
    let mut chambers = Vec::new();
    let mut dropped = 0;
    for (i, seed) in seeds.iter().enumerate() {
        let mine: Vec<usize> = (0..w * h).filter(|&c| owner[c] == i + 1).collect();
        let target = PI * seed.r * seed.r / (cell * cell);
        let Some(cells) = tidy(&mine, &owner, i + 1, w, h, cell) else {
            dropped += 1;
            continue;
        };
        if (cells.len() as f32) < (target * 0.2).max(4.0) {
            dropped += 1;
            continue;
        }
        let id = chambers.len() as u16 + 1;
        let (mut sx, mut sy) = (0.0f64, 0.0f64);
        for &c in &cells {
            ids[c] = id;
            sx += (c % w) as f64 + 0.5;
            sy += (c / w) as f64 + 0.5;
        }
        let n = cells.len() as f64;
        chambers.push(Chamber {
            id,
            kind: seed.kind,
            style: seed.style,
            zone: seed.zone,
            center_m: [(sx / n) as f32 * cell, (sy / n) as f32 * cell],
            size_m: seed.r * 2.0,
            angle_deg: (-seed.angle.to_degrees()).rem_euclid(180.0),
            cells: cells.len() as u32,
            area_m2: cells.len() as f32 * cell * cell,
        });
    }
    if dropped > 0 {
        notes.push(format!("Chambers: {dropped} squeezed to almost nothing by their neighbors and dropped"));
    }
    let floor = ids.iter().filter(|&&i| i > 0).count();
    ChamberBuild {
        layout: ChamberLayout {
            width: w,
            height: h,
            cell_size_m: cell,
            ids,
            chambers,
            requested: s.hall_count + grottos,
            floor_pct: floor as f32 / (w * h) as f32 * 100.0,
        },
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    use crate::generator::cave::style;
    use crate::model::MapKind;

    fn config() -> LevelConfig {
        let mut c = LevelConfig { kind: MapKind::Cave, ..LevelConfig::default() };
        c.map.width_m = 160.0;
        c.map.height_m = 140.0;
        c.cell_size_m = 1.0;
        c
    }

    fn run(c: &LevelConfig, seed: u64) -> (StyleLayout, ChamberBuild) {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let st = style::build(c, &mut rng).layout;
        let ch = build(c, &mut rng, &st);
        (st, ch)
    }

    #[test]
    fn chambers_fit_keep_apart_and_off_the_edge() {
        let c = config();
        for seed in 0..3 {
            let (_, b) = run(&c, seed);
            let l = &b.layout;
            assert_eq!(l.chambers.len() as u32, l.requested, "seed {seed}: {:?}", b.notes);
            let m = c.cave.chambers.rock_margin_m as usize;
            for y in 0..l.height {
                for x in 0..l.width {
                    let id = l.ids[y * l.width + x];
                    if id == 0 {
                        continue;
                    }
                    assert!(x >= m && y >= m && x + m < l.width && y + m < l.height, "chamber in the rock margin");
                    // No other chamber within the gap.
                    let g = c.cave.chambers.gap_m as i64 - 1;
                    for dy in -g..=g {
                        for dx in -g..=g {
                            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                            if (dx * dx + dy * dy) as f32 > (g * g) as f32 || nx < 0 || ny < 0 {
                                continue;
                            }
                            let (nx, ny) = (nx as usize, ny as usize);
                            if nx < l.width && ny < l.height {
                                let o = l.ids[ny * l.width + nx];
                                assert!(o == 0 || o == id, "chambers {id} and {o} touch");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn chambers_take_their_zone_style_and_size() {
        let c = config();
        let (st, b) = run(&c, 4);
        for ch in &b.layout.chambers {
            let (x, y) = (ch.center_m[0] as usize, ch.center_m[1] as usize);
            let k = y * st.width + x;
            if b.layout.ids[k] == ch.id {
                assert_eq!(st.ids[k], ch.zone);
            }
            assert_eq!(st.zones[ch.zone as usize - 1].style, ch.style);
            let (lo, hi) = match ch.kind {
                ChamberKind::Hall => (c.cave.chambers.hall_min_m, c.cave.chambers.hall_max_m),
                ChamberKind::Grotto => (c.cave.chambers.grotto_min_m, c.cave.chambers.grotto_max_m),
            };
            assert!(ch.size_m >= lo * 0.5 - 1e-3 && ch.size_m <= hi + 1e-3, "{ch:?}");
            assert!(ch.area_m2 > PI * (ch.size_m / 2.0).powi(2) * 0.5, "{ch:?} lost most of its area");
        }
    }

    #[test]
    fn hand_made_rooms_are_rectangles() {
        let mut c = config();
        c.cave.style.natural_pct = 0.0;
        c.cave.chambers.room_roughness_pct = 0.0;
        let (_, b) = run(&c, 5);
        let l = &b.layout;
        assert!(l.chambers.iter().all(|ch| ch.style == CaveStyle::HandMade));
        for ch in &l.chambers {
            // Axis-aligned rectangle: the bounding box is (almost) full.
            let cells: Vec<usize> = (0..l.ids.len()).filter(|&k| l.ids[k] == ch.id).collect();
            let xs = cells.iter().map(|&k| k % l.width);
            let ys = cells.iter().map(|&k| k / l.width);
            let (x0, x1) = (xs.clone().min().unwrap(), xs.max().unwrap());
            let (y0, y1) = (ys.clone().min().unwrap(), ys.max().unwrap());
            let bbox = (x1 - x0 + 1) * (y1 - y0 + 1);
            assert!(cells.len() as f32 >= bbox as f32 * 0.9, "room {} fills {} of {bbox}", ch.id, cells.len());
        }
    }

    #[test]
    fn rough_rooms_stay_roomlike() {
        let mut c = config();
        c.cave.style.natural_pct = 0.0;
        let (_, b) = run(&c, 5);
        let l = &b.layout;
        let mut uneven = 0;
        for ch in &l.chambers {
            let cells: Vec<usize> = (0..l.ids.len()).filter(|&k| l.ids[k] == ch.id).collect();
            let xs = cells.iter().map(|&k| k % l.width);
            let ys = cells.iter().map(|&k| k / l.width);
            let bbox = (xs.clone().max().unwrap() - xs.min().unwrap() + 1) * (ys.clone().max().unwrap() - ys.min().unwrap() + 1);
            let fill = cells.len() as f32 / bbox as f32;
            assert!(fill > 0.7, "room {} fills only {fill}", ch.id);
            uneven += (fill < 0.97) as usize;
        }
        assert!(uneven * 2 > l.chambers.len(), "hand-dug rooms should not be exact rectangles");
    }

    #[test]
    fn same_seed_same_chambers() {
        let c = config();
        assert_eq!(run(&c, 6).1.layout, run(&c, 6).1.layout);
    }
}
