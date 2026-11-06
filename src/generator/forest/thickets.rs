//! Forest node 5, thickets (step F4 in docs/forest-plan.md): every cell is
//! open ground, forest floor (both walkable; the trees node puts trunks on
//! them) or thicket (brush and young trees too dense to pass), from one
//! density value per cell, kept for the trees node:
//!
//! - the area's **character** (meadow thin, dense woods thick, ...) plus
//!   `density_pct`;
//! - **patches** — thickets and glades inside the areas
//!   (`patchiness_pct`); managed areas get half, and their thickets are
//!   cut back (only the border belts stay dense);
//! - **border belts** — thicket grows in from the borders between
//!   areas (`border_pct`, `border_width_m`), its edge pushed in and out by
//!   noise, so the belts are wide ragged masses with lobes and bays, not
//!   bands;
//! - the **map edge** — a thicket belt `edge_width_m` deep;
//! - **moisture** — denser along water; **height** — thinner up high.
//!
//! Water and rock keep their own ground. Thicket specks become forest floor.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::generator::forest::distance_field;
use crate::generator::grid;
use crate::generator::noise::NoiseField;
use crate::level::{AreaLayout, CellMask, ForestCharacter, ForestGround, ForestStyle, TerrainLayout, WaterLayout, ThicketLayout};
use crate::generator::region::RegionCtx;
use crate::model::LevelConfig;

/// Density cut: thicket at and above, open ground below `OPEN_BELOW`.
pub const DENSE_FROM: f32 = 0.62;
const OPEN_BELOW: f32 = 0.36;
/// Thicket patches smaller than this, m², become forest floor.
const MIN_DENSE_M2: f32 = 10.0;
const PATCH_SCALE_M: f32 = 22.0;
const FINE_SCALE_M: f32 = 6.0;
const EDGE_WARP_SCALE_M: f32 = 26.0;
const FRINGE_SCALE_M: f32 = 5.0;
const MOIST_M: f32 = 6.0;

/// Blending toward a town: share of the edge belt cut away at the border.
const CUT_BELT: f32 = 0.7;
/// Blending toward a town: thicket density taken off at the border.
const CUT_DENSITY: f32 = 0.22;
/// Blending toward a swamp: thicket density added at the border (damp
/// undergrowth).
const WET_DENSITY: f32 = 0.08;
/// Blending toward a swamp: share of the ground under puddles at the border.
const PUDDLE_SHARE: f32 = 0.4;
/// Size of the puddle blobs, meters.
const PUDDLE_SCALE_M: f32 = 9.0;
/// Blending toward a massif (scree): thicket density taken off at the
/// foot — stony open ground, where the boulders and rock patches lie.
const SCREE_DENSITY: f32 = 0.3;

pub fn base_density(c: ForestCharacter) -> f32 {
    match c {
        ForestCharacter::Meadow => 0.12,
        ForestCharacter::OpenWoods => 0.42,
        ForestCharacter::DenseWoods => 0.56,
        ForestCharacter::OldGrowth => 0.48,
        ForestCharacter::Logged => 0.24,
        ForestCharacter::Plantation => 0.46,
        ForestCharacter::CampGrounds => 0.16,
    }
}

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, areas: &AreaLayout, terrain: &TerrainLayout, water: &WaterLayout, region: Option<&RegionCtx>) -> ThicketLayout {
    let s = &config.forest.thickets;
    let (w, h, cell) = (areas.width, areas.height, areas.cell_size_m);
    let n = w * h;
    let patch = NoiseField::new(rng.random::<u64>());
    let fine = NoiseField::new(rng.random::<u64>());
    let edge_warp = NoiseField::new(rng.random::<u64>());

    let bw = s.border_width_m.max(cell);
    let ew = s.edge_width_m.max(0.0);
    let (to_border, _) = distance_field(w, h, cell, |c| crate::generator::forest::neighbors4(c, w, h).any(|nb| areas.ids[nb] != areas.ids[c]), bw * 3.5);
    let (to_water, _) = distance_field(w, h, cell, |c| water.shallow.cells[c] || water.deep.cells[c], MOIST_M * 1.5);
    let patchy = (s.patchiness_pct / 100.0).clamp(0.0, 1.0);
    let border = (s.border_pct / 100.0).clamp(0.0, 1.5);
    let shift = s.density_pct / 100.0 * 0.3;

    let town_cut = region.and_then(|r| r.blend_toward(crate::model::MapKind::Town));
    let swamp_wet = region.and_then(|r| r.blend_toward(crate::model::MapKind::Swamp));
    let scree = region.and_then(|r| r.blend_toward(crate::model::MapKind::Cave));
    // Puddles in the wet belt: blobs of a noise of the blending seed (no
    // draws from the chain's stream), more of them nearer the swamp.
    let puddle_noise = region.map(|r| NoiseField::new(r.blend_seed ^ 0x9D1E ^ r.id as u64));
    let puddle = |c: usize| {
        let (Some(t), Some(nf)) = (swamp_wet, puddle_noise.as_ref()) else { return false };
        if t[c] <= 0.0 {
            return false;
        }
        let (x, y) = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
        let v = ((nf.fbm(x / PUDDLE_SCALE_M, y / PUDDLE_SCALE_M, 3) - 0.3) / 0.4).clamp(0.0, 1.0);
        v > 1.0 - PUDDLE_SHARE * t[c]
    };
    let mut class = vec![ForestGround::Floor; n];
    let mut density = vec![0.0f32; n];
    for c in 0..n {
        let (x, y) = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
        let area = areas.area_at(c);
        let managed = area.style == ForestStyle::Managed;
        // Two scales: big lobes and bays, and a ragged fringe.
        let wobble = (edge_warp.fbm(x / EDGE_WARP_SCALE_M, y / EDGE_WARP_SCALE_M, 3) - 0.5) * 2.0 * 0.75
            + (edge_warp.fbm(x / FRINGE_SCALE_M + 31.7, y / FRINGE_SCALE_M, 2) - 0.5) * 2.0 * 0.35;
        let mut belt = border * fade((to_border[c] + wobble * bw * 1.3) / bw);
        // The map edge — or, in a map region, its own edge (the border to
        // the neighbouring regions counts as the map edge).
        let edge_d = match region {
            Some(r) => r.edge_dist[c].min(1e4),
            None => (x.min(y).min(w as f32 * cell - x).min(h as f32 * cell - y)).max(0.0),
        };
        // Blending: toward a town the forest is cut — the belt is a narrow
        // hedge outside the wall and the woods behind it are thinned,
        // most at the border (docs/blending-plan.md, B1).
        let cut = town_cut.map_or(0.0, |t| t[c]);
        // Blending: at the foot of a massif the belt gives way to the
        // stony scree slope the same way (B3).
        let stony = scree.map_or(0.0, |t| t[c]);
        let ew = ew * (1.0 - CUT_BELT * (cut.max(stony) * 3.0).min(1.0));
        // (The areas' own belt also runs along the region's edge: thinned
        // there too, at the foot only — a town keeps it as B1 built it.)
        belt *= 1.0 - CUT_BELT * (stony * 3.0).min(1.0);
        let edge = if ew > 0.0 { 0.9 * fade((edge_d + wobble * ew * 0.5) / ew) } else { 0.0 };
        let amp = if managed { 0.5 } else { 1.0 } * patchy * 0.5;
        let inner = amp * (patch.fbm(x / PATCH_SCALE_M, y / PATCH_SCALE_M, 4) - 0.5) * 2.0 + 0.08 * (fine.fbm(x / FINE_SCALE_M, y / FINE_SCALE_M, 2) - 0.5) * 2.0;
        // Blending: toward a swamp the ground gets wetter (B2).
        let moist = 0.12 * fade(to_water[c] / MOIST_M) + WET_DENSITY * swamp_wet.map_or(0.0, |t| t[c]);
        let mut d = base_density(area.character) + shift + inner + moist - 0.1 * terrain.height_m[c];
        // Managed: thickets inside are cut back, the belts stay.
        if managed {
            d = d.min(DENSE_FROM - 0.02);
        }
        d += belt + edge - CUT_DENSITY * cut - SCREE_DENSITY * stony;
        density[c] = d;
        class[c] = if d >= DENSE_FROM {
            ForestGround::Thicket
        } else if d < OPEN_BELOW {
            ForestGround::Open
        } else {
            ForestGround::Floor
        };
    }
    // Thicket specks → forest floor.
    let mut dense = CellMask { width: w, height: h, cell_size_m: cell, cells: class.iter().map(|&g| g == ForestGround::Thicket).collect() };
    grid::remove_small_components(&mut dense, (MIN_DENSE_M2 / (cell * cell)).ceil() as u32);
    let mut counts = [0usize; 3];
    let mut puddles = 0u32;
    let ground: Vec<u8> = (0..n)
        .map(|c| {
            let g = if region.is_some_and(|r| !r.inside(c)) {
                // Outside the region: closed (another region's ground).
                ForestGround::Thicket
            } else if region.is_some_and(|r| r.bridges.cells[c]) && water.deep.cells[c] {
                ForestGround::Bridge
            } else if water.deep.cells[c] {
                ForestGround::Deep
            } else if water.shallow.cells[c] {
                ForestGround::Shallow
            } else if terrain.rock.cells[c] {
                ForestGround::Rock
            } else if !(class[c] == ForestGround::Thicket && dense.cells[c]) && puddle(c) {
                puddles += 1;
                ForestGround::Shallow
            } else if class[c] == ForestGround::Thicket && !dense.cells[c] {
                ForestGround::Floor
            } else {
                class[c]
            };
            match g {
                ForestGround::Open => counts[0] += 1,
                ForestGround::Floor => counts[1] += 1,
                ForestGround::Thicket => counts[2] += 1,
                _ => {}
            }
            g.code()
        })
        .collect();
    let pct = |k: usize| k as f32 / n.max(1) as f32 * 100.0;
    ThicketLayout { ground, density, open_pct: pct(counts[0]), floor_pct: pct(counts[1]), thicket_pct: pct(counts[2]), puddle_cells: puddles }
}

/// 1 at 0 and below, falling smoothly to 0 at 1.
fn fade(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}
