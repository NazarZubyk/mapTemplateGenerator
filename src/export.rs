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
    /// Every placed object with position and size in meters.
    pub objects: Vec<crate::level::PlacedObject>,
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
        Self {
            format_version: 1,
            generator: format!("MapTemplateGenerator {}", env!("CARGO_PKG_VERSION")),
            seed: level.seed,
            map_size_m: [level.config.map.width_m, level.config.map.height_m],
            cell_size_m: level.config.cell_size_m,
            px_per_m,
            legend,
            layer_categories: render::layer_categories(),
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
                },
                PlacedObject {
                    category: Category::Structure,
                    type_name: "House".to_owned(),
                    color: [10, 20, 30],
                    x_m: 10.0,
                    y_m: 10.0,
                    w_m: 3.0,
                    h_m: 3.0,
                },
            ],
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
