use rand::Rng;

use crate::generator::grid::{Grid, RectM, WalkBudget};
use crate::level::{Category, ObjectDetails, PlacedObject};
use crate::model::{LevelConfig, StructureSettings, TemplateKind, TemplateMix};

/// A region of the map governed by one template kind.
#[derive(Debug, Clone, Copy)]
pub struct Zone {
    pub kind: TemplateKind,
    pub rect: RectM,
}

impl Zone {
    pub fn structure(&self, config: &LevelConfig) -> StructureSettings {
        config.structure(self.kind)
    }
}

/// Split the map into zones. A single template covers the whole map; a mix of two
/// splits it vertically or horizontally (seed-random) at a 35-65 % line.
pub fn build_zones(rng: &mut impl Rng, config: &LevelConfig) -> Vec<Zone> {
    let map = RectM::new(0.0, 0.0, config.map.width_m, config.map.height_m);
    match config.template_mix {
        TemplateMix::Single(kind) => vec![Zone { kind, rect: map }],
        TemplateMix::Mix(a, b) => {
            let vertical = rng.random::<bool>();
            let frac = 0.35 + rng.random::<f32>() * 0.30;
            let (ra, rb) = if vertical {
                let split = map.w * frac;
                (
                    RectM::new(0.0, 0.0, split, map.h),
                    RectM::new(split, 0.0, map.w - split, map.h),
                )
            } else {
                let split = map.h * frac;
                (
                    RectM::new(0.0, 0.0, map.w, split),
                    RectM::new(0.0, split, map.w, map.h - split),
                )
            };
            vec![
                Zone { kind: a, rect: ra },
                Zone { kind: b, rect: rb },
            ]
        }
    }
}

/// Display name of one template structure (used on the map and in the legend).
pub fn structure_name(kind: TemplateKind) -> &'static str {
    match kind {
        TemplateKind::Town => "House",
        TemplateKind::Forest => "Tree",
        TemplateKind::Cave => "Rock formation",
        TemplateKind::Swamp => "Swamp tree",
    }
}

/// Estimate of how many structures fill `density_pct` of the buildable ground:
/// coverage divided by the average structure footprint.
fn target_count(buildable_area: f32, settings: &StructureSettings) -> usize {
    let avg = ((settings.size.min_m + settings.size.max_m) * 0.5).max(0.01);
    let avg_area = avg * avg;
    let coverage = buildable_area * settings.density_pct.clamp(0.0, 100.0) / 100.0;
    (coverage / avg_area).ceil() as usize
}

/// Place template structures for all zones, round-robin so both templates in a mix
/// share the global walkable budget fairly. Each structure samples random anchor
/// cells AND angles: the first rotation that fits the tight free spot wins, which
/// makes houses lean every which way like an organically grown town. Stops early
/// when the walkable % target or the free space is exhausted.
pub fn place(
    rng: &mut impl Rng,
    grid: &mut Grid,
    budget: &mut WalkBudget,
    zones: &[Zone],
    config: &LevelConfig,
    objects: &mut Vec<PlacedObject>,
    notes: &mut Vec<String>,
) {
    let regions_before = grid.connected_walkable_components(1);
    let mut work: Vec<(usize, usize)> = zones // (zone index, remaining)
        .iter()
        .enumerate()
        .map(|(i, z)| {
            (
                i,
                target_count(grid.buildable_area_in(z.rect), &z.structure(config)),
            )
        })
        .filter(|&(_, n)| n > 0)
        .collect();
    let initial_targets: Vec<usize> = work.iter().map(|&(_, n)| n).collect();

    let mut budget_exhausted = false;
    while !work.is_empty() {
        let mut i = 0;
        while i < work.len() {
            let (zi, remaining) = work[i];
            let zone = &zones[zi];
            let settings = zone.structure(config);
            let w = rng.random_range(settings.size.min_m..=settings.size.max_m.max(settings.size.min_m));
            let h = rng.random_range(settings.size.min_m..=settings.size.max_m.max(settings.size.min_m));

            // Try anchor cells near open ground + random angles; first fit wins
            // (organic, tight packing).
            let mut fit: Option<(f32, f32, f32, usize)> = None;
            for _ in 0..400 {
                let Some((ax, ay)) = grid.random_free_anchor(rng, zone.rect, 1) else {
                    continue;
                };
                let cx = ax + rng.random_range(-w / 2.0..w / 2.0);
                let cy = ay + rng.random_range(-h / 2.0..h / 2.0);
                let angle = rng.random_range(0.0..360.0);
                // Charge the EXACT area to the budget: the conservative cell
                // marking over-covers by up to half a cell for non-overlap
                // safety, which would otherwise starve the budget.
                let exact_cells = ((w * h) / (grid.cell_m * grid.cell_m)).ceil() as usize;
                if !budget.can_fit(exact_cells) {
                    budget_exhausted = true;
                    break;
                }
                if grid
                    .place_rotated_checked((cx, cy), w, h, angle, regions_before)
                    .is_some()
                {
                    fit = Some((cx, cy, angle, exact_cells));
                    break;
                }
            }

            match fit {
                None => {
                    // Organic packing rarely hits the exact target; only report
                    // when less than half of it was achieved.
                    if !budget_exhausted && remaining * 2 > initial_targets[zi] {
                        notes.push(format!(
                            "{}: could only place some structures (no fitting spot left in its zone)",
                            zone.kind
                        ));
                    }
                    work.swap_remove(i);
                }
                Some((cx, cy, angle, cells)) => {
                    budget.consume(cells);
                    objects.push(PlacedObject {
                        category: Category::Structure,
                        type_name: structure_name(zone.kind).to_owned(),
                        color: settings.color,
                        x_m: cx - w / 2.0,
                        y_m: cy - h / 2.0,
                        w_m: w,
                        h_m: h,
                        angle_deg: angle,
                        details: ObjectDetails::Structure { template: zone.kind },
                    });
                    work[i] = (zi, remaining - 1);
                    if remaining == 1 {
                        work.swap_remove(i);
                    } else {
                        i += 1;
                    }
                }
            }
        }
    }

    if budget_exhausted {
        notes.push(format!(
            "Structure placement stopped early to keep the walkable target of {:.0}%",
            config.map.walkable_pct
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::geometry;
    use rand_chacha::ChaCha8Rng;
    use rand::SeedableRng;

    #[test]
    fn single_template_covers_whole_map() {
        let config = LevelConfig::default();
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let zones = build_zones(&mut rng, &config);
        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0].kind, TemplateKind::Town);
        assert_eq!(zones[0].rect.w, config.map.width_m);
        assert_eq!(zones[0].rect.h, config.map.height_m);
    }

    #[test]
    fn mix_makes_two_disjoint_zones() {
        let config = LevelConfig {
            template_mix: TemplateMix::Mix(TemplateKind::Town, TemplateKind::Forest),
            ..LevelConfig::default()
        };
        let mut rng = ChaCha8Rng::seed_from_u64(2);
        let zones = build_zones(&mut rng, &config);
        assert_eq!(zones.len(), 2);
        assert_eq!(zones[0].kind, TemplateKind::Town);
        assert_eq!(zones[1].kind, TemplateKind::Forest);
        assert!(!zones[0].rect.overlaps(&zones[1].rect));
        let total: f32 = zones.iter().map(|z| z.rect.area()).sum();
        assert!((total - config.map.width_m * config.map.height_m).abs() < 1e-3);
    }

    #[test]
    fn structures_place_overlap_free_in_both_mix_zones() {
        let config = LevelConfig {
            template_mix: TemplateMix::Mix(TemplateKind::Town, TemplateKind::Forest),
            ..LevelConfig::default()
        };
        let mut rng = ChaCha8Rng::seed_from_u64(3);
        let zones = build_zones(&mut rng, &config);
        let mut grid = Grid::new(config.map.width_m, config.map.height_m, config.cell_size_m);
        let mut budget = WalkBudget::new(grid.total_cells(), config.map.walkable_pct);
        let mut objects = Vec::new();
        let mut notes = Vec::new();
        place(&mut rng, &mut grid, &mut budget, &zones, &config, &mut objects, &mut notes);

        assert!(!objects.is_empty());
        assert!(objects.iter().any(|o| o.type_name == "House"));
        assert!(objects.iter().any(|o| o.type_name == "Tree"));
        // Houses come out rotated (organic town), trees may too.
        assert!(objects.iter().any(|o| o.angle_deg.abs() > 1.0));
        let corners = |o: &PlacedObject| {
            geometry::rot_rect_corners(
                (o.x_m + o.w_m / 2.0, o.y_m + o.h_m / 2.0),
                o.w_m,
                o.h_m,
                o.angle_deg,
            )
        };
        for (i, a) in objects.iter().enumerate() {
            assert!(a.x_m + a.w_m >= 0.0 && a.y_m + a.h_m >= 0.0);
            assert!(a.x_m - a.w_m <= config.map.width_m + 0.01);
            assert!(a.y_m - a.h_m <= config.map.height_m + 0.01);
            for b in objects.iter().skip(i + 1) {
                assert!(
                    !geometry::rotated_rects_overlap(&corners(a), &corners(b)),
                    "{a:?} overlaps {b:?}"
                );
            }
        }
    }
}
