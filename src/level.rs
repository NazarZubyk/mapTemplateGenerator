use serde::{Deserialize, Serialize};

use crate::model::{ColorRgb, LevelConfig, TemplateKind};

/// Object category — doubles as the layer id in the exported layer PNG (Phase 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    Structure,
    Prop,
    Interactive,
    HiddenRoom,
    HiddenRoomPath,
    MobSpawn,
    Gate,
}

impl Category {
    /// Stable id used in the machine-readable layer PNG (R channel).
    pub fn layer_id(self) -> u8 {
        match self {
            Category::Structure => 1,
            Category::Prop => 2,
            Category::Interactive => 3,
            Category::HiddenRoom => 4,
            Category::HiddenRoomPath => 5,
            Category::MobSpawn => 6,
            Category::Gate => 7,
        }
    }
}

/// Type-specific details carried into the exported specification (Phase 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ObjectDetails {
    Structure { template: TemplateKind },
    Prop,
    Interactive { group_id: u32, group_size: u32 },
    HiddenRoom { room_id: u32 },
    HiddenRoomPath { room_id: u32 },
    /// One spawn point; members are (mob type name, count).
    MobSpawn { members: Vec<(String, u32)> },
    Gate { direction_deg: f32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacedObject {
    pub category: Category,
    pub type_name: String,
    pub color: ColorRgb,
    /// Axis-aligned footprint in meters; (x, y) is the top-left corner,
    /// origin at the map's top-left, +y pointing down the map.
    pub x_m: f32,
    pub y_m: f32,
    pub w_m: f32,
    pub h_m: f32,
    pub details: ObjectDetails,
}

impl PlacedObject {
    pub fn rect(&self) -> crate::generator::grid::RectM {
        crate::generator::grid::RectM {
            x: self.x_m,
            y: self.y_m,
            w: self.w_m,
            h: self.h_m,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedLevel {
    /// Snapshot of the config used, so exports (Phase 3) are self-contained.
    pub config: LevelConfig,
    pub seed: u64,
    pub objects: Vec<PlacedObject>,
    /// Non-blocking notes from generation (early stops, budget exhaustion, ...).
    /// Shown in the warnings panel with no field marker.
    pub notes: Vec<String>,
}
