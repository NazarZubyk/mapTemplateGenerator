use std::collections::HashMap;

use rand::Rng;
use serde::{Deserialize, Serialize};

/// RGB color, one per object type (used for map rendering and the legend).
pub type ColorRgb = [u8; 3];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TemplateKind {
    Town,
    Forest,
    Cave,
    Swamp,
}

impl TemplateKind {
    pub const ALL: [TemplateKind; 4] = [
        TemplateKind::Town,
        TemplateKind::Forest,
        TemplateKind::Cave,
        TemplateKind::Swamp,
    ];
}

impl std::fmt::Display for TemplateKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            TemplateKind::Town => "Town",
            TemplateKind::Forest => "Forest",
            TemplateKind::Cave => "Cave",
            TemplateKind::Swamp => "Swamp",
        };
        f.write_str(name)
    }
}

/// A level uses one template or a mix of exactly two.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TemplateMix {
    Single(TemplateKind),
    Mix(TemplateKind, TemplateKind),
}

impl TemplateMix {
    pub fn is_mix(&self) -> bool {
        matches!(self, TemplateMix::Mix(..))
    }

    pub fn kinds(&self) -> Vec<TemplateKind> {
        match self {
            TemplateMix::Single(k) => vec![*k],
            TemplateMix::Mix(a, b) => vec![*a, *b],
        }
    }

    pub fn contains(&self, kind: TemplateKind) -> bool {
        self.kinds().contains(&kind)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct SizeRange {
    pub min_m: f32,
    pub max_m: f32,
}

impl SizeRange {
    pub fn new(min_m: f32, max_m: f32) -> Self {
        Self { min_m, max_m }
    }

    /// Valid when non-negative and ordered.
    pub fn is_valid(&self) -> bool {
        self.min_m >= 0.0 && self.min_m <= self.max_m
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapSettings {
    pub width_m: f32,
    pub height_m: f32,
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

/// Structure rules for one template kind (houses for a town, trees for a forest, ...).
/// Each template kind carries its own settings so a mix of two templates can use
/// two different structures with two different colors (see the legend in goals.md).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StructureSettings {
    pub density_pct: f32,
    pub size: SizeRange,
    pub color: ColorRgb,
}

impl Default for StructureSettings {
    fn default() -> Self {
        Self {
            density_pct: 30.0,
            size: SizeRange::new(3.0, 10.0),
            color: [165, 120, 75],
        }
    }
}

/// Per-kind structure defaults with distinct colors.
pub fn default_structure(kind: TemplateKind) -> StructureSettings {
    match kind {
        TemplateKind::Town => StructureSettings {
            density_pct: 30.0,
            size: SizeRange::new(3.0, 10.0),
            color: [165, 120, 75],
        },
        TemplateKind::Forest => StructureSettings {
            density_pct: 45.0,
            size: SizeRange::new(2.0, 8.0),
            color: [34, 100, 45],
        },
        TemplateKind::Cave => StructureSettings {
            density_pct: 25.0,
            size: SizeRange::new(2.0, 9.0),
            color: [110, 110, 125],
        },
        TemplateKind::Swamp => StructureSettings {
            density_pct: 35.0,
            size: SizeRange::new(2.0, 7.0),
            color: [95, 115, 60],
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PropsSettings {
    pub density_pct: f32,
    pub size: SizeRange,
    pub quantity: u32,
    pub color: ColorRgb,
}

impl Default for PropsSettings {
    fn default() -> Self {
        Self {
            density_pct: 20.0,
            size: SizeRange::new(0.5, 2.0),
            quantity: 50,
            color: [130, 130, 130],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InteractiveType {
    pub name: String,
    pub size_m: f32,
    pub quantity: u32,
    pub group: SizeRange,
    pub color: ColorRgb,
}

impl Default for InteractiveType {
    fn default() -> Self {
        Self {
            name: "Herb".to_owned(),
            size_m: 0.4,
            quantity: 30,
            group: SizeRange::new(2.0, 5.0),
            color: [120, 200, 80],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HiddenRoomSettings {
    pub count: u32,
    pub size: SizeRange,
    pub color_room: ColorRgb,
    pub color_path: ColorRgb,
    /// Whether the access path is passable for mobs (always passable for the player).
    pub mob_passable: bool,
}

impl Default for HiddenRoomSettings {
    fn default() -> Self {
        Self {
            count: 1,
            size: SizeRange::new(4.0, 8.0),
            color_room: [160, 90, 200],
            color_path: [190, 140, 220],
            mob_passable: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MobType {
    pub name: String,
    pub price: f32,
    pub size_m: f32,
    pub color: ColorRgb,
}

impl Default for MobType {
    fn default() -> Self {
        Self {
            name: "Goblin".to_owned(),
            price: 10.0,
            size_m: 1.0,
            color: [200, 60, 50],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MobSettings {
    pub budget: f32,
    pub max_entities: u32,
    pub types: Vec<MobType>,
    /// `combinations[a][b]`: types a and b may co-occur at one spawn point.
    /// Symmetric, diagonal always true, dimensions follow `types`.
    pub combinations: Vec<Vec<bool>>,
    /// Desired ratio per type, percent (sums to 100).
    pub balance: Vec<f32>,
}

impl Default for MobSettings {
    fn default() -> Self {
        Self {
            budget: 500.0,
            max_entities: 100,
            types: vec![MobType::default()],
            combinations: vec![vec![true]],
            balance: vec![100.0],
        }
    }
}

impl MobSettings {
    /// Add a type and keep combinations/balance consistent with `types`.
    pub fn add_type(&mut self, mob: MobType) {
        self.types.push(mob);
        let n = self.types.len();
        for row in &mut self.combinations {
            row.push(true);
        }
        self.combinations.push(vec![true; n]);
        self.balance.push(0.0);
    }

    /// Remove a type by index and keep combinations/balance consistent.
    pub fn remove_type(&mut self, idx: usize) {
        if idx >= self.types.len() {
            return;
        }
        self.types.remove(idx);
        self.combinations.remove(idx);
        for row in &mut self.combinations {
            row.remove(idx);
        }
        self.balance.remove(idx);
        normalize_balance(&mut self.balance);
    }

    /// Repair combinations/balance shapes after a file load.
    pub fn normalize(&mut self) {
        let n = self.types.len();
        self.combinations.resize(n, vec![true; n]);
        for row in &mut self.combinations {
            row.resize(n, true);
        }
        self.combinations.truncate(n);
        for i in 0..n {
            self.combinations[i][i] = true;
            for j in 0..n {
                self.combinations[j][i] = self.combinations[i][j];
            }
        }
        self.balance.resize(n, 0.0);
        self.balance.truncate(n);
        normalize_balance(&mut self.balance);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Gate {
    /// 0-360 degrees from the map center.
    pub direction_deg: f32,
    pub color: ColorRgb,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            direction_deg: 90.0,
            color: [50, 110, 230],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelConfig {
    pub template_mix: TemplateMix,
    /// Settings per template kind; templates not in the mix are ignored.
    pub structures: HashMap<TemplateKind, StructureSettings>,
    pub map: MapSettings,
    pub props: PropsSettings,
    pub interactive: Vec<InteractiveType>,
    pub hidden_rooms: HiddenRoomSettings,
    pub mobs: MobSettings,
    pub gates: Vec<Gate>,
    pub cell_size_m: f32,
    pub seed: u64,
}

impl LevelConfig {
    pub fn structure(&self, kind: TemplateKind) -> StructureSettings {
        self.structures.get(&kind).copied().unwrap_or_default()
    }

    pub fn structure_mut(&mut self, kind: TemplateKind) -> &mut StructureSettings {
        self.structures.entry(kind).or_default()
    }

    /// Fill in per-kind structures and repair mob relations after a file load.
    pub fn normalize(&mut self) {
        for kind in TemplateKind::ALL {
            self.structures.entry(kind).or_default();
        }
        self.mobs.normalize();
    }
}

impl Default for TemplateMix {
    fn default() -> Self {
        TemplateMix::Single(TemplateKind::Town)
    }
}

impl LevelConfig {
    fn default_structures() -> HashMap<TemplateKind, StructureSettings> {
        TemplateKind::ALL
            .into_iter()
            .map(|kind| (kind, default_structure(kind)))
            .collect()
    }
}

impl Default for LevelConfig {
    fn default() -> Self {
        Self {
            template_mix: TemplateMix::Single(TemplateKind::Town),
            structures: Self::default_structures(),
            map: MapSettings::default(),
            props: PropsSettings::default(),
            interactive: vec![InteractiveType::default()],
            hidden_rooms: HiddenRoomSettings::default(),
            mobs: MobSettings::default(),
            gates: vec![Gate::default()],
            cell_size_m: 0.5,
            seed: 0,
        }
    }
}

/// One variant per input field, used to attach warnings to fields (marker icons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldId {
    TemplateMix,
    MapWidth,
    MapHeight,
    WalkablePct,
    StructureDensity(TemplateKind),
    StructureSize(TemplateKind),
    StructureColor(TemplateKind),
    PropsDensity,
    PropsSize,
    PropsQuantity,
    PropsColor,
    InteractiveSize(usize),
    InteractiveQuantity(usize),
    InteractiveGroup(usize),
    InteractiveColor(usize),
    HiddenRoomCount,
    HiddenRoomSize,
    HiddenRoomColor,
    HiddenRoomPathColor,
    MobBudget,
    MobMaxEntities,
    MobBalance,
    MobTypePrice(usize),
    MobTypeSize(usize),
    MobTypeColor(usize),
    GateDirection(usize),
    GateColor(usize),
}

impl FieldId {
    /// Human-readable field name for the warnings panel.
    pub fn label(&self) -> String {
        match self {
            FieldId::TemplateMix => "template mix".to_owned(),
            FieldId::MapWidth => "map width".to_owned(),
            FieldId::MapHeight => "map height".to_owned(),
            FieldId::WalkablePct => "walkable %".to_owned(),
            FieldId::StructureDensity(k) => format!("{k} structure density"),
            FieldId::StructureSize(k) => format!("{k} structure size"),
            FieldId::StructureColor(k) => format!("{k} structure color"),
            FieldId::PropsDensity => "props density".to_owned(),
            FieldId::PropsSize => "props size".to_owned(),
            FieldId::PropsQuantity => "props quantity".to_owned(),
            FieldId::PropsColor => "props color".to_owned(),
            FieldId::InteractiveSize(i) => format!("interactive #{} size", i + 1),
            FieldId::InteractiveQuantity(i) => format!("interactive #{} quantity", i + 1),
            FieldId::InteractiveGroup(i) => format!("interactive #{} group size", i + 1),
            FieldId::InteractiveColor(i) => format!("interactive #{} color", i + 1),
            FieldId::HiddenRoomCount => "hidden room count".to_owned(),
            FieldId::HiddenRoomSize => "hidden room size".to_owned(),
            FieldId::HiddenRoomColor => "hidden room color".to_owned(),
            FieldId::HiddenRoomPathColor => "hidden room path color".to_owned(),
            FieldId::MobBudget => "mob budget".to_owned(),
            FieldId::MobMaxEntities => "max entities".to_owned(),
            FieldId::MobBalance => "mob balance".to_owned(),
            FieldId::MobTypePrice(i) => format!("mob #{} price", i + 1),
            FieldId::MobTypeSize(i) => format!("mob #{} size", i + 1),
            FieldId::MobTypeColor(i) => format!("mob #{} color", i + 1),
            FieldId::GateDirection(i) => format!("gate #{} direction", i + 1),
            FieldId::GateColor(i) => format!("gate #{} color", i + 1),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Warning {
    pub field: Option<FieldId>,
    pub message: String,
    /// Blocking warnings prevent generation (e.g. duplicate colors).
    pub blocking: bool,
}

impl Warning {
    pub fn new(field: FieldId, message: impl Into<String>) -> Self {
        Self {
            field: Some(field),
            message: message.into(),
            blocking: false,
        }
    }

    pub fn blocking(field: FieldId, message: impl Into<String>) -> Self {
        Self {
            field: Some(field),
            message: message.into(),
            blocking: true,
        }
    }
}

/// Random pleasant default color: random hue, moderate saturation/value (not neon).
pub fn random_default_color(rng: &mut impl Rng) -> ColorRgb {
    let hue = rng.random::<f32>() * 360.0;
    let sat = 0.45 + rng.random::<f32>() * 0.25;
    let val = 0.65 + rng.random::<f32>() * 0.20;
    hsv_to_rgb(hue, sat, val)
}

pub fn hsv_to_rgb(h_deg: f32, s: f32, v: f32) -> ColorRgb {
    let h = (h_deg.rem_euclid(360.0)) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let to_u8 = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    [to_u8(r), to_u8(g), to_u8(b)]
}

/// Rescale all weights so they sum to 100 (even spread when all are zero).
pub fn normalize_balance(weights: &mut [f32]) {
    if weights.is_empty() {
        return;
    }
    let sum: f32 = weights.iter().sum();
    if sum <= 0.0 {
        let share = 100.0 / weights.len() as f32;
        weights.fill(share);
    } else {
        for w in weights.iter_mut() {
            *w *= 100.0 / sum;
        }
    }
}

/// Set `weights[changed_idx] = new_value` and rescale all OTHER weights so the
/// total stays 100. If all others are 0, the remainder is spread evenly.
pub fn renormalize_balance(weights: &[f32], changed_idx: usize, new_value: f32) -> Vec<f32> {
    let mut out = weights.to_vec();
    if changed_idx >= out.len() {
        return out;
    }
    let changed = new_value.clamp(0.0, 100.0);
    out[changed_idx] = changed;
    let target = 100.0 - changed;
    let others: Vec<usize> = (0..out.len()).filter(|&i| i != changed_idx).collect();
    let others_sum: f32 = others.iter().map(|&i| out[i]).sum();
    if target <= 0.0 || others.is_empty() {
        for &i in &others {
            out[i] = 0.0;
        }
        out[changed_idx] = 100.0;
    } else if others_sum <= 0.0 {
        let share = target / others.len() as f32;
        for &i in &others {
            out[i] = share;
        }
    } else {
        for &i in &others {
            out[i] *= target / others_sum;
        }
    }
    out
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
    fn renormalize_keeps_sum_at_100_and_honors_new_value() {
        let out = renormalize_balance(&[50.0, 30.0, 20.0], 1, 50.0);
        assert_eq!(out[1], 50.0);
        assert!((out.iter().sum::<f32>() - 100.0).abs() < 1e-4);

        let out = renormalize_balance(&[100.0, 0.0, 0.0], 0, 25.0);
        // All others were 0 -> spread the remainder evenly.
        assert!((out[1] - 37.5).abs() < 1e-4);
        assert!((out[2] - 37.5).abs() < 1e-4);
        assert!((out.iter().sum::<f32>() - 100.0).abs() < 1e-4);

        // Setting a slider to its maximum zeroes out the rest.
        let out = renormalize_balance(&[40.0, 60.0], 1, 100.0);
        assert_eq!(out[1], 100.0);
        assert_eq!(out[0], 0.0);
    }

    #[test]
    fn hsv_to_rgb_corners() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), [255, 0, 0]);
        assert_eq!(hsv_to_rgb(120.0, 1.0, 1.0), [0, 255, 0]);
        assert_eq!(hsv_to_rgb(240.0, 1.0, 1.0), [0, 0, 255]);
        assert_eq!(hsv_to_rgb(0.0, 0.0, 1.0), [255, 255, 255]);
    }

    #[test]
    fn random_default_color_is_not_extreme() {
        let mut rng = rand::rng();
        for _ in 0..100 {
            let [r, g, b] = random_default_color(&mut rng);
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            assert!(max >= 128, "color too dark: {r} {g} {b}");
            assert!(max - min <= 160, "color too saturated: {r} {g} {b}");
        }
    }
}
