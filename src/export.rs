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
    /// Layer names indexed by the layer.png R-channel value (index 0 =
    /// empty ground; 1–7 object categories; 8–14 swamp ground; 15–23 cave
    /// ground; 24–31 forest ground).
    pub layer_categories: Vec<&'static str>,
    /// How to read layer.png, channel by channel.
    pub layer_encoding: LayerEncoding,
    /// What kind of map this is ("town", "swamp", ...).
    pub map_kind: &'static str,
    /// The region split, once the regions node is built.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regions: Option<Vec<ExportRegion>>,
    /// The road network, once the roads node is built.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roads: Option<ExportRoads>,
    /// The walls between regions and their road openings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region_walls: Option<ExportRegionWalls>,
    /// Groups of interactive objects: type, center and member indices into
    /// `objects`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interactive_groups: Option<Vec<crate::level::InteractiveGroup>>,
    /// Hidden rooms: floor rect, access path centerline + width, whether
    /// mobs may use the path, and their object indices in `objects`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden_rooms: Option<Vec<crate::level::HiddenRoom>>,
    /// Mob spawn points (center, radius, pack members, cost, on road, hidden
    /// room) and the budget spent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mobs: Option<crate::level::MobLayout>,
    /// Town gates: direction asked / used (0° = east, 90° = north), point,
    /// region, wall opening width, gate road link index. (Swamp, cave and
    /// forest gates are in their own sections.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gates: Option<Vec<crate::level::GateInfo>>,
    /// Swamp gates: direction asked / used, style (shore / bridge), corner,
    /// gate point on the map edge, where the gate island goes and its
    /// diameter, bridge length.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_anchors: Option<Vec<crate::level::GateAnchor>>,
    /// Swamp islands: id, kind (gate / hub / small), gate id, centroid,
    /// asked diameter, cells, area. Their cells are the "island" color on
    /// map.png.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub islands: Option<Vec<crate::level::Island>>,
    /// Swamp links between islands: island ids, kind (mud / boardwalk),
    /// loop / dead-end flags, width, length over water, centerline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<crate::level::SwampPath>>,
    /// Swamp bridge gates: the boardwalk from the gate island's shore to the
    /// gate point.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_bridges: Option<Vec<crate::level::GateBridge>>,
    /// Swamp water: the wading rules the layout was built for and the
    /// shares of each terrain. Shallow water and deep mud are walkable,
    /// deep water and thicket are not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub water: Option<ExportWater>,
    /// Cave style zones: connected patches of natural caverns or hand-made
    /// passages (id, style, area, center).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cave_zones: Option<Vec<crate::level::CaveZone>>,
    /// Forest areas: id, style (wild / managed), character, area, center.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forest_areas: Option<Vec<crate::level::ForestArea>>,
    /// Forest ground kinds: layer id, name, map color and whether the
    /// player can walk on it — the key to both map.png and layer.png.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forest_ground: Option<Vec<ExportGroundKind>>,
    /// Forest glades (open spots: fights, camps, landmarks) and trails
    /// (footpaths in wild areas, cart tracks in managed ones) with their
    /// centerlines.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forest_clearings: Option<ExportForestClearings>,
    /// Forest trees: trunk counts (the trunks themselves are `objects` of
    /// type "tree trunk": round, solid).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forest_trees: Option<crate::level::ForestTreeLayout>,
    /// Forest props: the groups (center, radius, direction, their props'
    /// indexes in `objects`) and counts. The props themselves are `objects`
    /// of type "prop" — shape and place only, all blocked; what they are is
    /// up to the level designer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forest_props: Option<crate::level::ForestPropLayout>,
    /// Forest streams (spring → mouth centerlines, widths, where they end)
    /// and ponds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forest_water: Option<ExportForestWater>,
    /// Forest passages: per touching area pair whether it is linked and
    /// how (open, trail, pass, ford, bridge) and where; the gates; the area
    /// graph stats; the ground shares.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forest_passages: Option<ExportForestPassages>,
    /// Cave chambers: id, kind (hall / grotto), style, zone, center, size,
    /// long-axis angle, area.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chambers: Option<Vec<crate::level::Chamber>>,
    /// Cave tunnels: ends (chamber / junction / gate / blind), style, loop
    /// and zone-crossing flags, width, squeeze width, length, centerline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnels: Option<Vec<crate::level::Tunnel>>,
    /// Cave tunnel crossings with no chamber.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub junctions: Option<Vec<crate::level::Junction>>,
    /// Cave gates: edge point, direction, the chamber their tunnel leads to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cave_gates: Option<Vec<crate::level::CaveGate>>,
    /// Cave floor shares after carving (the floor itself is in map.png).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cave_floor: Option<ExportCaveFloor>,
    /// Cave hazards: chasms (impassable, maybe a rope bridge across) and
    /// pools (walkable shallow edge, impassable deep core), per chamber.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hazards: Option<Vec<crate::level::Hazard>>,
    /// Every placed object with position and size in meters.
    pub objects: Vec<crate::level::PlacedObject>,
}

/// The meaning of layer.png's channels (written into spec.json so a reader
/// needs nothing else).
#[derive(Debug, Serialize)]
pub struct LayerEncoding {
    pub object: &'static str,
    pub ground: &'static str,
    pub empty: &'static str,
}

impl Default for LayerEncoding {
    fn default() -> Self {
        Self {
            object: "R = category id (1-7, see layer_categories); G + 256*B = instance id = index in `objects` + 1; A = 255",
            ground: "swamp maps, pixels under no object: R = ground id (8-14, see layer_categories); \
G = depth: 0 = safe ground (island, hummock, boardwalk), 1-254 = breath used to wade there from safe \
ground (254 = the wading limit, see `water`), 255 = deep water / thicket (not walkable); \
B = island id (low byte, matches `islands[].id`) on islands, else 0; A = 255. \
Cave maps, pixels under no object: R = ground id (15-23, see layer_categories: natural / hand-made \
chamber floor, tunnel, rock, cave_wall = rock along the floor, chasm, rope_bridge, shallow_pool, \
deep_pool; walkable = floors, tunnel, rope_bridge, shallow_pool); G = style zone id (low byte, \
matches `cave_zones[].id`); B = chamber id (low byte, matches `chambers[].id`) on chamber cells, \
else 0; A = 255. Forest maps, pixels under no object: R = ground id (24-31, see layer_categories \
and `forest_ground`); G = area id (low byte, matches `forest_areas[].id`); B = 1 walkable, 0 \
blocked; A = 255",
            empty: "R = G = B = A = 0: no object and no ground data (town maps outside the region)",
        }
    }
}

/// One ground kind of a map kind that paints its own ground.
#[derive(Debug, Clone, Serialize)]
pub struct ExportGroundKind {
    /// The layer.png R value.
    pub layer_id: u8,
    pub name: &'static str,
    pub label: &'static str,
    /// The map.png color.
    pub color: String,
    pub walkable: bool,
}

/// Forest glades and trails.
#[derive(Debug, Clone, Serialize)]
pub struct ExportForestClearings {
    pub glades: Vec<crate::level::ForestGlade>,
    pub trails: Vec<crate::level::ForestTrail>,
    /// Walkable share of the map after the clearings, %.
    pub walkable_pct: f32,
}

/// Forest water.
#[derive(Debug, Clone, Serialize)]
pub struct ExportForestWater {
    pub streams: Vec<crate::level::ForestStream>,
    pub ponds: Vec<crate::level::ForestPond>,
}

/// Forest passages (the area graph) and gates.
#[derive(Debug, Clone, Serialize)]
pub struct ExportForestPassages {
    pub links: Vec<crate::level::AreaLink>,
    pub gates: Vec<crate::level::ForestGate>,
    /// Areas on the longest walk from a gate over the links.
    pub main_route_areas: u32,
    /// Links beyond the spanning tree.
    pub loops: u32,
    /// Areas with one link only.
    pub dead_end_areas: u32,
    /// Walkable share of the map, %.
    pub walkable_pct: f32,
    /// Height between the lowest and the highest ground, meters.
    pub relief_m: f32,
}

/// The cave floor after carving.
#[derive(Debug, Clone, Serialize)]
pub struct ExportCaveFloor {
    /// Share of the map that is walkable floor, %.
    pub floor_pct: f32,
    /// Share of the floor in natural zones, %.
    pub natural_floor_pct: f32,
    /// Width of the wall band (rock touching the floor) on the map, meters.
    pub wall_band_m: f32,
}

/// The swamp's wading rules and terrain shares.
#[derive(Debug, Serialize)]
pub struct ExportWater {
    /// Meters of shallow water the player can wade away from safe ground
    /// (islands, hummocks, boardwalks) before drowning.
    pub wade_limit_m: f32,
    /// 1 m of deep mud counts as this many meters of shallow water.
    pub mud_cost: f32,
    /// Spawns in the water are reachable and leavable (`reach_m` = half the
    /// limit) or only reachable (`reach_m` = the limit).
    pub return_trip: bool,
    pub reach_m: f32,
    /// Hummocks added so every mud path crossing fits in one breath.
    pub hummocks: u32,
    /// Shares of the map, %.
    pub walkable_pct: f32,
    pub shallow_pct: f32,
    pub mud_pct: f32,
    pub deep_pct: f32,
    pub thicket_pct: f32,
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
        for (color, name) in render::ground_legend(level) {
            legend.push(LegendEntry { color: color_hex(color), name, category: "ground" });
        }
        if let Some(depth) = level.swamp.as_ref().and_then(|s| s.depth.as_ref()) {
            let d = &level.config.swamp.depth;
            for (mask, color, name) in [
                (&depth.shallow, d.shallow_color, "shallow water"),
                (&depth.mud, d.mud_color, "deep mud"),
                (&depth.deep, d.deep_color, "deep water"),
                (&depth.thicket, d.thicket_color, "thicket"),
            ] {
                if mask.count_true() > 0 {
                    legend.push(LegendEntry { color: color_hex(color), name: name.to_owned(), category: "ground" });
                }
            }
        }
        if let Some(islands) = level.swamp.as_ref().and_then(|s| s.islands.as_ref())
            && !islands.islands.is_empty()
        {
            legend.push(LegendEntry {
                color: color_hex(level.config.swamp.islands.color),
                name: "island".to_owned(),
                category: "ground",
            });
        }
        if let Some(paths) = level.swamp.as_ref().and_then(|s| s.paths.as_ref()) {
            let p = &level.config.swamp.paths;
            for (mask, color, name) in [(&paths.mud, p.mud_color, "mud path"), (&paths.boardwalk, p.boardwalk_color, "boardwalk")] {
                if mask.count_true() > 0 {
                    legend.push(LegendEntry { color: color_hex(color), name: name.to_owned(), category: "ground" });
                }
            }
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
            format_version: 2,
            generator: format!("MapTemplateGenerator {}", env!("CARGO_PKG_VERSION")),
            seed: level.seed,
            map_size_m: [level.config.map.width_m, level.config.map.height_m],
            map_kind: level.config.kind.slug(),
            cell_size_m: level.config.cell_size_m,
            px_per_m,
            legend,
            layer_categories: render::layer_categories(),
            layer_encoding: LayerEncoding::default(),
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
            islands: level.swamp.as_ref().and_then(|s| s.islands.as_ref()).map(|l| l.islands.clone()),
            paths: level.swamp.as_ref().and_then(|s| s.paths.as_ref()).map(|l| l.paths.clone()),
            gate_bridges: level.swamp.as_ref().and_then(|s| s.paths.as_ref()).map(|l| l.bridges.clone()),
            cave_zones: level.cave.as_ref().and_then(|c| c.style.as_ref()).map(|l| l.zones.clone()),
            forest_areas: level.forest.as_ref().and_then(|f| f.areas.as_ref()).map(|l| l.areas.clone()),
            forest_ground: level.forest.as_ref().map(|_| {
                crate::level::ForestGround::ALL
                    .iter()
                    .map(|g| ExportGroundKind { layer_id: g.layer_id(), name: g.layer_name(), label: g.label(), color: color_hex(g.color()), walkable: g.walkable() })
                    .collect()
            }),
            forest_clearings: level.forest.as_ref().and_then(|f| f.clearings.as_ref()).map(|l| ExportForestClearings {
                glades: l.glades.clone(),
                trails: l.trails.clone(),
                walkable_pct: l.walkable_pct,
            }),
            forest_trees: level.forest.as_ref().and_then(|f| f.trees.clone()),
            forest_props: level.forest.as_ref().and_then(|f| f.props.clone()),
            forest_water: level.forest.as_ref().and_then(|f| f.water.as_ref()).map(|l| ExportForestWater { streams: l.streams.clone(), ponds: l.ponds.clone() }),
            forest_passages: level.forest.as_ref().and_then(|f| f.passages.as_ref()).map(|l| ExportForestPassages {
                links: l.links.clone(),
                gates: l.gates.clone(),
                main_route_areas: l.main_route_areas,
                loops: l.loops,
                dead_end_areas: l.dead_end_areas,
                walkable_pct: l.walkable_pct,
                relief_m: level.forest.as_ref().and_then(|f| f.terrain.as_ref()).map_or(0.0, |t| t.relief_m),
            }),
            chambers: level.cave.as_ref().and_then(|c| c.chambers.as_ref()).map(|l| l.chambers.clone()),
            tunnels: level.cave.as_ref().and_then(|c| c.tunnels.as_ref()).map(|l| l.tunnels.clone()),
            junctions: level.cave.as_ref().and_then(|c| c.tunnels.as_ref()).map(|l| l.junctions.clone()),
            cave_gates: level.cave.as_ref().and_then(|c| c.tunnels.as_ref()).map(|l| l.gates.clone()),
            hazards: level.cave.as_ref().and_then(|c| c.hazards.as_ref()).map(|l| l.hazards.clone()),
            cave_floor: level.cave.as_ref().and_then(|c| c.carve.as_ref()).map(|l| ExportCaveFloor {
                floor_pct: l.floor_pct,
                natural_floor_pct: l.natural_floor_pct,
                wall_band_m: level.config.cave.carve.wall_band_m,
            }),
            water: level.swamp.as_ref().and_then(|s| s.depth.as_ref()).map(|d| {
                let s = &level.config.swamp.depth;
                ExportWater {
                    wade_limit_m: s.wade_limit_m,
                    mud_cost: s.mud_cost,
                    return_trip: s.return_trip,
                    reach_m: d.reach_m,
                    hummocks: d.hummocks,
                    walkable_pct: d.walkable_pct,
                    shallow_pct: d.shallow_pct,
                    mud_pct: d.mud_pct,
                    deep_pct: d.deep_pct,
                    thicket_pct: d.thicket_pct,
                }
            }),
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
            config: crate::model::LevelConfig::test_64(),
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
                    round: false,
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
                    round: false,
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
            cave: None,
            forest: None,
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
        assert_eq!(spec["layer_categories"].as_array().unwrap().len(), 32);
        assert_eq!(spec["format_version"].as_u64(), Some(2));

        std::fs::remove_dir_all(&dir).ok();
    }
}
