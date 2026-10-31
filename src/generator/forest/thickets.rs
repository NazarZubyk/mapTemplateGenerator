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

pub fn build(config: &LevelConfig, rng: &mut ChaCha8Rng, areas: &AreaLayout, terrain: &TerrainLayout, water: &WaterLayout) -> ThicketLayout {
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

    let mut class = vec![ForestGround::Floor; n];
    let mut density = vec![0.0f32; n];
    for c in 0..n {
        let (x, y) = (((c % w) as f32 + 0.5) * cell, ((c / w) as f32 + 0.5) * cell);
        let area = areas.area_at(c);
        let managed = area.style == ForestStyle::Managed;
        // Two scales: big lobes and bays, and a ragged fringe.
        let wobble = (edge_warp.fbm(x / EDGE_WARP_SCALE_M, y / EDGE_WARP_SCALE_M, 3) - 0.5) * 2.0 * 0.75
            + (edge_warp.fbm(x / FRINGE_SCALE_M + 31.7, y / FRINGE_SCALE_M, 2) - 0.5) * 2.0 * 0.35;
        let belt = border * fade((to_border[c] + wobble * bw * 1.3) / bw);
        let edge_d = (x.min(y).min(w as f32 * cell - x).min(h as f32 * cell - y)).max(0.0);
        let edge = if ew > 0.0 { 0.9 * fade((edge_d + wobble * ew * 0.5) / ew) } else { 0.0 };
        let amp = if managed { 0.5 } else { 1.0 } * patchy * 0.5;
        let inner = amp * (patch.fbm(x / PATCH_SCALE_M, y / PATCH_SCALE_M, 4) - 0.5) * 2.0 + 0.08 * (fine.fbm(x / FINE_SCALE_M, y / FINE_SCALE_M, 2) - 0.5) * 2.0;
        let moist = 0.12 * fade(to_water[c] / MOIST_M);
        let mut d = base_density(area.character) + shift + inner + moist - 0.1 * terrain.height_m[c];
        // Managed: thickets inside are cut back, the belts stay.
        if managed {
            d = d.min(DENSE_FROM - 0.02);
        }
        d += belt + edge;
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
    let ground: Vec<u8> = (0..n)
        .map(|c| {
            let g = if water.deep.cells[c] {
                ForestGround::Deep
            } else if water.shallow.cells[c] {
                ForestGround::Shallow
            } else if terrain.rock.cells[c] {
                ForestGround::Rock
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
    ThicketLayout { ground, density, open_pct: pct(counts[0]), floor_pct: pct(counts[1]), thicket_pct: pct(counts[2]) }
}

/// 1 at 0 and below, falling smoothly to 0 at 1.
fn fade(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}
