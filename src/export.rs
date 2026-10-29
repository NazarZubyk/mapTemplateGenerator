use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::level::{Category, GeneratedLevel, ObjectDetails, PlacedObject};
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
/// reconstruct the level from spec.json + layer.png alone (goals.md "Export").
#[derive(Debug, Serialize)]
pub struct ExportSpec {
    pub format_version: u32,
    pub generator: String,
    pub seed: u64,
    /// Map size in meters, [width, height]; origin is the map's top-left.
    pub map_size_m: [f32; 2],
    pub cell_size_m: f32,
    pub px_per_m: f32,
    /// One entry per unique (color, object type) — the human- and machine-readable
    /// meaning of every color on map.png.
    pub legend: Vec<LegendEntry>,
    /// Every placed object with position and size in meters.
    pub objects: Vec<PlacedObject>,
}

#[derive(Debug, Serialize)]
pub struct LegendEntry {
    pub color: String,
    pub name: String,
    pub category: &'static str,
    pub details: String,
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
                details: details_text(level, o),
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
            objects: level.objects.clone(),
        }
    }
}

fn color_hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// Short human-readable legend line per object type, sourced from the config snapshot.
fn details_text(level: &GeneratedLevel, o: &PlacedObject) -> String {
    let config = &level.config;
    match &o.details {
        ObjectDetails::Structure { template } => {
            let s = config.structure(*template);
            format!(
                "{template} structure, density {:.0}%, size {}–{} m",
                s.density_pct, s.size.min_m, s.size.max_m
            )
        }
        ObjectDetails::Prop => format!(
            "Uninteractive prop, size {}–{} m",
            config.props.size.min_m, config.props.size.max_m
        ),
        ObjectDetails::Interactive { group_size, .. } => {
            let it = config
                .interactive
                .iter()
                .find(|i| i.name == o.type_name || i.color == o.color);
            match it {
                Some(it) => format!(
                    "Interactive object, size {} m, groups of up to {group_size}",
                    it.size_m
                ),
                None => format!("Interactive object, groups of up to {group_size}"),
            }
        }
        ObjectDetails::HiddenRoom { .. } => {
            "Hidden room, connected to the main area by one access path".to_owned()
        }
        ObjectDetails::HiddenRoomPath { .. } => {
            format!(
                "Access path (player-passable; mob-passable: {})",
                if config.hidden_rooms.mob_passable { "yes" } else { "no" }
            )
        }
        ObjectDetails::MobSpawn { members } => {
            let list: Vec<String> = members
                .iter()
                .map(|(name, count)| format!("{count}x {name}"))
                .collect();
            format!("Mob spawn point: {}", list.join(", "))
        }
        ObjectDetails::Gate { direction_deg } => {
            format!("Level gate at {direction_deg:.0}° from the map center")
        }
    }
}

/// Category name for legend/serialization helpers.
#[allow(dead_code)]
pub fn category_label(c: Category) -> &'static str {
    render::category_name(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::generate;
    use crate::model::LevelConfig;

    #[test]
    fn bundle_writes_three_valid_files() {
        let config = LevelConfig::default();
        let level = generate(&config, 42);
        let dir = std::env::temp_dir().join(format!("mtg_export_test_{}", std::process::id()));

        let result = export_bundle(&level, &dir, 8.0).unwrap();
        assert!(result.map_path.is_file());
        assert!(result.layer_path.is_file());
        assert!(result.spec_path.is_file());

        // PNGs decode and have the right dimensions.
        let map = image::open(&result.map_path).unwrap();
        assert_eq!(map.width(), (config.map.width_m * 8.0) as u32);

        // spec.json parses, carries all objects and a non-empty legend.
        let text = std::fs::read_to_string(&result.spec_path).unwrap();
        let spec: serde_json::Value = serde_json::from_str(&text).unwrap();
        let objects = spec["objects"].as_array().unwrap();
        assert_eq!(objects.len(), level.objects.len());
        let legend = spec["legend"].as_array().unwrap();
        assert!(legend.len() >= 4, "legend too small: {legend:?}");
        assert_eq!(spec["seed"].as_u64(), Some(42));
        for l in legend {
            let color = l["color"].as_str().unwrap();
            assert!(color.starts_with('#') && color.len() == 7);
            assert!(l["name"].as_str().is_some());
            assert!(l["category"].as_str().is_some());
        }

        std::fs::remove_dir_all(&dir).ok();
    }
}
