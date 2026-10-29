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
    Wall,
    Road,
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
            Category::Wall => 6,
            Category::Road => 7,
            Category::MobSpawn => 8,
            Category::Gate => 9,
        }
    }

    pub const ALL: [Category; 9] = [
        Category::Structure,
        Category::Prop,
        Category::Interactive,
        Category::HiddenRoom,
        Category::HiddenRoomPath,
        Category::Wall,
        Category::Road,
        Category::MobSpawn,
        Category::Gate,
    ];
}

/// Type-specific details carried into the exported specification (Phase 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ObjectDetails {
    Structure { template: TemplateKind },
    Prop,
    Interactive { group_id: u32, group_size: u32 },
    HiddenRoom { room_id: u32 },
    HiddenRoomPath { room_id: u32 },
    /// Part of a walled template's enclosure: perimeter walls bound the playable
    /// area, section walls divide it into districts (gaps between them are ways).
    Wall { enclosed: TemplateKind, perimeter: bool },
    /// One segment of a curvy road. Roads are walkable open ground: props and
    /// plants may stand on them, buildings may not.
    Road,
    /// One spawn point; members are (mob type name, count).
    MobSpawn { members: Vec<(String, u32)> },
    Gate { direction_deg: f32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacedObject {
    pub category: Category,
    pub type_name: String,
    pub color: ColorRgb,
    /// Axis-aligned footprint in meters when `angle_deg` is 0; otherwise the
    /// unrotated rect that is rotated around its center by `angle_deg`.
    /// (x, y) is the top-left corner, origin at the map's top-left, +y down.
    pub x_m: f32,
    pub y_m: f32,
    pub w_m: f32,
    pub h_m: f32,
    /// Clockwise rotation in degrees around the rect center (screen +y down).
    #[serde(default)]
    pub angle_deg: f32,
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
