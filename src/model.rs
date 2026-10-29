use serde::{Deserialize, Serialize};

/// The app config. Deliberately minimal while the generator is rebuilt section
/// by section (docs/workflow.md): each finished section adds its fields back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelConfig {
    pub map: MapSettings,
    pub cell_size_m: f32,
    pub seed: u64,
}

impl Default for LevelConfig {
    fn default() -> Self {
        Self {
            map: MapSettings::default(),
            cell_size_m: 0.5,
            seed: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapSettings {
    pub width_m: f32,
    pub height_m: f32,
    /// Share of the map that must remain walkable; governs the occupancy
    /// budget of everything the generator places.
    pub walkable_pct: f32,
}

impl Default for MapSettings {
    fn default() -> Self {
        Self {
            width_m: 64.0,
            height_m: 64.0,
            walkable_pct: 60.0,
        }
    }
}

/// A validation warning; `field` names the offending input when there is one.
#[derive(Debug, Clone, PartialEq)]
pub struct Warning {
    pub field: Option<String>,
    pub message: String,
    /// Blocking warnings prevent generation until resolved.
    pub blocking: bool,
}

impl Warning {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            field: None,
            message: message.into(),
            blocking: false,
        }
    }

    pub fn blocking(message: impl Into<String>) -> Self {
        Self {
            field: None,
            message: message.into(),
            blocking: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_json_round_trip() {
        let config = LevelConfig::default();
        let json = serde_json::to_string_pretty(&config).unwrap();
        let back: LevelConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, back);
    }

    #[test]
    fn old_config_files_load_with_defaults() {
        // A config saved by an older build (with removed fields) must still load.
        let old = r#"{
            "map": {"width_m": 128.0, "height_m": 96.0, "walkable_pct": 60.0},
            "cell_size_m": 0.5,
            "seed": 7,
            "structures": {},
            "mobs": {"budget": 100.0}
        }"#;
        let config: LevelConfig = serde_json::from_str(old).unwrap();
        assert_eq!(config.map.width_m, 128.0);
        assert_eq!(config.seed, 7);
    }
}
