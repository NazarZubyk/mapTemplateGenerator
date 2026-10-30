use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::level::GeneratedLevel;
use crate::render;

#[derive(Debug, Clone)]
pub struct ExportResult {
    pub map_path: PathBuf,
    pub layer_path: PathBuf,
    pub spec_path: PathBuf,
}

/// Write the full export bundle into `dir`: map.png, layer.png, spec.json.
pub fn export_bundle(
    level: &GeneratedLevel,
    dir: &Path,
    px_per_m: f32,
) -> Result<ExportResult, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;

    let rasterized = render::rasterize(level, px_per_m);
    let map_path = dir.join("map.png");
    rasterized
        .map
        .save(&map_path)
        .map_err(|e| format!("saving map.png failed: {e}"))?;
    let layer_path = dir.join("layer.png");
    rasterized
        .layer
        .save(&layer_path)
        .map_err(|e| format!("saving layer.png failed: {e}"))?;

    let spec = ExportSpec::new(level, px_per_m);
    let spec_path = dir.join("spec.json");
    let json = serde_json::to_string_pretty(&spec)
        .map_err(|e| format!("serializing spec failed: {e}"))?;
    std::fs::write(&spec_path, json).map_err(|e| format!("writing spec.json failed: {e}"))?;

    Ok(ExportResult {
        map_path,
        layer_path,
        spec_path,
    })
}

/// Machine-readable legend + every placed object. Designed so an AI agent can
/// reconstruct the level from spec.json + layer.png alone.
#[derive(Debug, Serialize)]
pub struct ExportSpec {
    pub format_version: u32,
    pub generator: String,
    pub seed: u64,
    /// Map size in meters, [width, height]; origin is the map's top-left.
    pub map_size_m: [f32; 2],
    pub cell_size_m: f32,
    pub px_per_m: f32,
    /// One entry per unique (color, object type) — the meaning of every color
    /// on map.png.
    pub legend: Vec<LegendEntry>,
    /// Category names indexed by the layer.png R-channel value (index 0 = ground).
    pub layer_categories: Vec<&'static str>,
    /// What kind of map this is ("town", "swamp", ...).
    pub map_kind: &'static str,
    /// The region split, once the regions node is built.
    pub regions: Option<Vec<ExportRegion>>,
    /// The road network, once the roads node is built.
    pub roads: Option<ExportRoads>,
    /// The walls between regions and their road openings.
    pub region_walls: Option<ExportRegionWalls>,
    /// Groups of interactive objects: type, center and member indices into
    /// `objects`.
    pub interactive_groups: Option<Vec<crate::level::InteractiveGroup>>,
    /// Hidden rooms: floor rect, access path centerline + width, whether
    /// mobs may use the path, and their object indices in `objects`.
    pub hidden_rooms: Option<Vec<crate::level::HiddenRoom>>,
    /// Mob spawn points (center, radius, pack members, cost, on road, hidden
    /// room) and the budget spent.
    pub mobs: Option<crate::level::MobLayout>,
    /// Gates: direction asked / used (0° = east, 90° = north), point,
    /// region, wall opening width, gate road link index.
    pub gates: Option<Vec<crate::level::GateInfo>>,
    /// Swamp gates: direction asked / used, style (shore / bridge), corner,
    /// gate point on the map edge, where the gate island goes and its
    /// diameter, bridge length.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_anchors: Option<Vec<crate::level::GateAnchor>>,
    /// Every placed object with position and size in meters.
    pub objects: Vec<crate::level::PlacedObject>,
}

/// One region of the split, with its color on map.png.
#[derive(Debug, Serialize)]
pub struct ExportRegion {
    pub id: u8,
    pub color: String,
    pub target_pct: f32,
    pub actual_pct: f32,
    pub cells: u32,
}

/// The roads: each one as a centerline polyline with its width, plus the points where they
/// cross between regions — all in meters from the map's top-left corner.
#[derive(Debug, Serialize)]
pub struct ExportRoads {
    pub width_m: f32,
    pub roads: Vec<ExportRoad>,
    /// Where a road crosses between two regions (future wall gaps).
    pub passages: Vec<ExportPassage>,
}

#[derive(Debug, Serialize)]
pub struct ExportRoad {
    /// This road's width (main roads widest, alleys narrowest).
    pub width_m: f32,
    pub length_m: f32,
    /// Centerline points every ~1 m, end to end.
    pub path_m: Vec<[f32; 2]>,
}

#[derive(Debug, Serialize)]
pub struct ExportPassage {
    pub x_m: f32,
    pub y_m: f32,
    pub from_region: u8,
    pub to_region: u8,
}

impl ExportRoads {
    fn new(net: &crate::level::RoadNetwork) -> Self {
        let w = net.mask.width;
        let cs = net.mask.cell_size_m;
        let at = |x: usize, y: usize| [(x as f32 + 0.5) * cs, (y as f32 + 0.5) * cs];
        let every = ((1.0 / cs).round() as usize).max(1);
        Self {
            width_m: net.width_cells as f32 * cs,
            roads: net
                .links
                .iter()
                .map(|l| {
                    let pts: Vec<[f32; 2]> = l.path.iter().map(|&c| at(c % w, c / w)).collect();
                    let length_m = pts
                        .windows(2)
                        .map(|p| ((p[1][0] - p[0][0]).powi(2) + (p[1][1] - p[0][1]).powi(2)).sqrt())
                        .sum();
                    let mut path_m: Vec<[f32; 2]> = pts.iter().step_by(every).copied().collect();
                    if pts.len() > 1 && !(pts.len() - 1).is_multiple_of(every) {
                        path_m.push(pts[pts.len() - 1]);
                    }
                    ExportRoad { width_m: l.width_cells.max(1) as f32 * cs, length_m, path_m }
                })
                .collect(),
            passages: net
                .passages
                .iter()
                .map(|p| {
                    let [x_m, y_m] = at(p.x, p.y);
                    ExportPassage { x_m, y_m, from_region: p.from_region, to_region: p.to_region }
                })
                .collect(),
        }
    }
}

/// Walls between regions: drawn on map.png; their openings listed here.
#[derive(Debug, Serialize)]
pub struct ExportRegionWalls {
    pub thickness_m: f32,
    /// Openings where a road passes, in meters from the top-left corner.
    pub gaps: Vec<ExportWallGap>,
}

#[derive(Debug, Serialize)]
pub struct ExportWallGap {
    pub x_m: f32,
    pub y_m: f32,
    /// The two regions the opening connects.
    pub regions: [u8; 2],
}

#[derive(Debug, Serialize)]
pub struct LegendEntry {
    pub color: String,
    pub name: String,
    pub category: &'static str,
}

impl ExportSpec {
    pub fn new(level: &GeneratedLevel, px_per_m: f32) -> Self {
        let mut legend: Vec<LegendEntry> = Vec::new();
        if let Some(regions) = &level.regions {
            // Each region paints in its own color; the plain region entry is
            // replaced by per-region entries.
            for s in &regions.stats {
                if s.cells > 0 {
                    legend.push(LegendEntry {
                        color: color_hex(render::region_color(s.id)),
                        name: format!("region {}", s.id),
                        category: "ground",
                    });
                }
            }
        } else if let Some(shape) = &level.shape
            && shape.stats.coverage_pct > 0.0
        {
            legend.push(LegendEntry {
                color: color_hex(render::REGION),
                name: level.config.kind.slug().to_owned(),
                category: "ground",
            });
        }
        if let Some(roads) = &level.roads
            && !roads.nodes.is_empty()
        {
            legend.push(LegendEntry {
                color: color_hex(render::ROAD),
                name: "road".to_owned(),
                category: "ground",
            });
        }
        if let Some(region_walls) = &level.region_walls
            && region_walls.mask.count_true() > 0
        {
            legend.push(LegendEntry {
                color: color_hex(render::REGION_WALL),
                name: "region wall".to_owned(),
                category: "ground",
            });
        }
        if let Some(walls) = &level.walls
            && walls.stats.wall_cells > 0
        {
            legend.push(LegendEntry {
                color: color_hex(render::WALL),
                name: "outer wall".to_owned(),
                category: "ground",
            });
        }
        for o in &level.objects {
            if legend
                .iter()
                .any(|l| l.color == color_hex(o.color) && l.name == o.type_name)
            {
                continue;
            }
            legend.push(LegendEntry {
                color: color_hex(o.color),
                name: o.type_name.clone(),
                category: render::category_name(o.category),
            });
        }
        let regions = level.regions.as_ref().map(|r| {
            r.stats
                .iter()
                .map(|s| ExportRegion {
                    id: s.id,
                    color: color_hex(render::region_color(s.id)),
                    target_pct: s.target_pct,
                    actual_pct: s.actual_pct,
                    cells: s.cells,
                })
                .collect()
        });
        Self {
            format_version: 1,
            generator: format!("MapTemplateGenerator {}", env!("CARGO_PKG_VERSION")),
            seed: level.seed,
            map_size_m: [level.config.map.width_m, level.config.map.height_m],
            map_kind: level.config.kind.slug(),
            cell_size_m: level.config.cell_size_m,
            px_per_m,
            legend,
            layer_categories: render::layer_categories(),
            regions,
            roads: level.roads.as_ref().map(ExportRoads::new),
            region_walls: level.region_walls.as_ref().map(|rw| {
                let cs = rw.mask.cell_size_m;
                ExportRegionWalls {
                    thickness_m: (level.config.region_walls.thickness_m / cs).round().max(1.0) * cs,
                    gaps: rw
                        .gaps
                        .iter()
                        .map(|g| ExportWallGap {
                            x_m: (g.x as f32 + 0.5) * cs,
                            y_m: (g.y as f32 + 0.5) * cs,
                            regions: [g.regions.0, g.regions.1],
                        })
                        .collect(),
                }
            }),
            interactive_groups: level.interactive.as_ref().map(|l| l.groups.clone()),
            hidden_rooms: level.hidden_rooms.as_ref().map(|l| l.rooms.clone()),
            mobs: level.mobs.clone(),
            gates: level.gates.as_ref().map(|g| g.gates.clone()),
            gate_anchors: level.swamp.as_ref().map(|s| s.gate_anchors.clone()),
            objects: level.objects.clone(),
        }
    }
}

fn color_hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{Category, PlacedObject};

    #[test]
    fn bundle_writes_three_valid_files() {
        let level = GeneratedLevel {
            config: crate::model::LevelConfig::default(),
            seed: 42,
            objects: vec![
                PlacedObject {
                    category: Category::Structure,
                    type_name: "House".to_owned(),
                    color: [10, 20, 30],
                    x_m: 1.0,
                    y_m: 1.0,
                    w_m: 3.0,
                    h_m: 3.0,
                    angle_deg: 0.0,
                    parts: Vec::new(),
                },
                PlacedObject {
                    category: Category::Structure,
                    type_name: "House".to_owned(),
                    color: [10, 20, 30],
                    x_m: 10.0,
                    y_m: 10.0,
                    w_m: 3.0,
                    h_m: 3.0,
                    angle_deg: 0.0,
                    parts: Vec::new(),
                },
            ],
            shape: None,
            walls: None,
            roads: None,
            region_walls: None,
            houses: None,
            props: None,
            interactive: None,
            hidden_rooms: None,
            mobs: None,
            gates: None,
            swamp: None,
            regions: None,
            noise_preview: None,
            notes: Vec::new(),
        };
        let dir = std::env::temp_dir().join(format!("mtg_export_test_{}", std::process::id()));

        let result = export_bundle(&level, &dir, 8.0).unwrap();
        assert!(result.map_path.is_file());
        assert!(result.layer_path.is_file());
        assert!(result.spec_path.is_file());

        let map = image::open(&result.map_path).unwrap();
        assert_eq!(map.width(), (level.config.map.width_m * 8.0) as u32);

        let text = std::fs::read_to_string(&result.spec_path).unwrap();
        let spec: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(spec["objects"].as_array().unwrap().len(), 2);
        let legend = spec["legend"].as_array().unwrap();
        assert_eq!(legend.len(), 1, "same type+color merges into one legend entry");
        assert_eq!(spec["seed"].as_u64(), Some(42));
        assert_eq!(spec["layer_categories"].as_array().unwrap().len(), 8);

        std::fs::remove_dir_all(&dir).ok();
    }
}
