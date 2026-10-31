//! Forest nodes 10–12, spawns (step F9 in docs/forest-plan.md): hidden
//! glades, interactive objects and mobs on forest ground. The shared cores
//! do the placing (`hidden_rooms::place_rooms`, `interactive::place_types`,
//! `mobs::spend`); this module works out where things may go.
//!
//! - Spawn terrains (`SpawnOn`): **land** = open ground and forest floor
//!   (glades included), **path** = trails and bridges, **shallow** =
//!   shallow water, **hidden room** = hidden glade floors; mud is unused.
//!   The style filter reads **wild** for natural and **managed** for
//!   hand-made (by the area under the spot). Thicket, rock and deep water
//!   never get spawns.
//! - Solid objects keep a free cell from every blocker (thicket, rock, deep
//!   water, trunks, props) and from each other, and stay clear of the
//!   gates — the walkable ground stays one piece.
//! - Hidden glades (before the interactive objects): organic pockets
//!   inside a thicket (the thicket is the wall), one narrow overgrown
//!   path to the walkable ground, a **hidden passage** marker where it
//!   breaks through. Never through a sealed border strip, deep water or
//!   rock.
//! - Mobs: the glades are the hubs (`hub_pct`); dead-end areas (one link)
//!   may get a guard pack first.

use rand_chacha::ChaCha8Rng;

use crate::generator::cave::spawns::shape_pockets;
use crate::generator::forest::distance_field;
use crate::generator::hidden_rooms::{self, HiddenRoomBuild, RoomArea};
use crate::generator::interactive::{self, Area, InteractiveBuild, SwampTerrain};
use crate::generator::mobs::{self, MobArea, MobBuild};
use crate::generator::placement::{dilate, TerrainSets};
use crate::generator::Stage;
use crate::level::{
    AreaLayout, Category, CellMask, ClearingLayout, ForestGround, ForestLayout, ForestStyle, HiddenRoomLayout, PassageLayout, PlacedObject,
};
use crate::model::LevelConfig;

/// Spawns keep this far from a gate point, meters.
pub const GATE_CLEAR_M: f32 = 6.0;
pub const HIDDEN_PASSAGE_NAME: &str = "hidden passage";
const HIDDEN_PASSAGE_COLOR: [u8; 3] = [230, 150, 60];

/// The forest after the layout nodes: what every spawn node starts from.
pub struct ForestSpawnGround<'a> {
    pub w: usize,
    pub h: usize,
    pub cell: f32,
    /// Final ground kind per cell.
    pub ground: Vec<ForestGround>,
    /// Walkable ground, minus trunks and props.
    pub walk: Vec<bool>,
    /// Trunks and props (solid, blocked).
    pub solid: Vec<bool>,
    /// Wild area per cell (the style filter's "natural").
    pub wild: Vec<bool>,
    pub sealed: &'a [bool],
    pub areas: &'a AreaLayout,
    pub passages: &'a PassageLayout,
    pub clearings: &'a ClearingLayout,
}

impl<'a> ForestSpawnGround<'a> {
    pub fn new(forest: &'a ForestLayout) -> Option<Self> {
        let areas = forest.areas.as_ref()?;
        let passages = forest.passages.as_ref()?;
        let clearings = forest.clearings.as_ref()?;
        let (w, h, cell) = (passages.width, passages.height, passages.cell_size_m);
        let n = w * h;
        let ground: Vec<ForestGround> = clearings.ground.iter().map(|&g| ForestGround::from_code(g)).collect();
        let solid = forest.objects_mask.as_ref().map(|m| m.cells.clone()).unwrap_or_else(|| vec![false; n]);
        Some(Self {
            w,
            h,
            cell,
            walk: (0..n).map(|c| ground[c].walkable() && !solid[c]).collect(),
            wild: (0..n).map(|c| areas.area_at(c).style == ForestStyle::Wild).collect(),
            ground,
            solid,
            sealed: &passages.sealed.cells,
            areas,
            passages,
            clearings,
        })
    }

    fn center(&self, c: usize) -> (f32, f32) {
        (((c % self.w) as f32 + 0.5) * self.cell, ((c / self.w) as f32 + 0.5) * self.cell)
    }

    fn near_gates(&self, r: f32) -> Vec<bool> {
        (0..self.w * self.h)
            .map(|c| {
                let (x, y) = self.center(c);
                self.passages.gates.iter().any(|g| (g.point_m[0] - x).hypot(g.point_m[1] - y) < r)
            })
            .collect()
    }

    fn land(&self, c: usize) -> bool {
        matches!(self.ground[c], ForestGround::Open | ForestGround::Floor)
    }

    fn trail(&self, c: usize) -> bool {
        matches!(self.ground[c], ForestGround::Trail | ForestGround::Bridge)
    }

    /// The spawn terrains (+ wild / managed) restricted to `ok`.
    fn sets(&self, ok: &[bool], room_of: &[u32]) -> TerrainSets {
        let n = self.w * self.h;
        TerrainSets {
            land: (0..n).map(|c| ok[c] && self.land(c) && room_of[c] == 0).collect(),
            path: (0..n).map(|c| ok[c] && self.trail(c) && room_of[c] == 0).collect(),
            shallow: (0..n).map(|c| ok[c] && self.ground[c] == ForestGround::Shallow && room_of[c] == 0).collect(),
            mud: vec![false; n],
            room: (0..n).map(|c| ok[c] && room_of[c] != 0).collect(),
            natural: Some(self.wild.clone()),
        }
    }
}

/// Hidden glades: pockets inside a thicket, one overgrown path out.
pub(crate) fn hidden_rooms(config: &LevelConfig, rng: &mut ChaCha8Rng, g: &ForestSpawnGround, stage: &Stage) -> HiddenRoomBuild {
    let (w, h, cell) = (g.w, g.h, g.cell);
    let n = w * h;
    let near_gate = g.near_gates(GATE_CLEAR_M.max(hidden_rooms::GATE_CLEARANCE_M));
    let edge = |c: usize| {
        let (x, y) = (c % w, c / w);
        x < 2 || y < 2 || x + 2 >= w || y + 2 >= h
    };
    // A glade sits in thicket a cell clear of any walkable ground or object.
    let open: Vec<bool> = (0..n).map(|c| g.ground[c].walkable() || g.solid[c]).collect();
    let near_open = dilate(&open, w, h, 1);
    let ok: Vec<bool> = (0..n).map(|c| g.ground[c] == ForestGround::Thicket && !g.sealed[c] && !near_open[c] && !near_gate[c] && !edge(c)).collect();
    let s = &config.hidden_rooms;
    let area = RoomArea {
        w,
        h,
        cell,
        walkable: g.walk.clone(),
        outside_ok: ok.clone(),
        inside_ok: ok,
        // The path cuts thicket only: never sealed strips, rock, water,
        // trunks, props or the map edge.
        blocked: (0..n)
            .map(|c| g.sealed[c] || g.solid[c] || matches!(g.ground[c], ForestGround::Rock | ForestGround::Deep | ForestGround::Shallow) || edge(c))
            .collect(),
        wall_cells: 0,
        max_len: (s.path_max_m / cell).round().max(1.0) as u32,
    };
    let mut built = hidden_rooms::place_rooms(config, rng, area, stage);
    let organic = vec![true; n];
    shape_pockets(&mut built, w, h, cell, &organic, rng, s.path_width_m);

    // Hidden passage marker: the path cell next to the walkable ground.
    let side = (s.path_width_m.max(cell) * 1.4).max(1.0);
    for room in &mut built.layout.rooms {
        let at = room.path_m.iter().rev().find_map(|p| {
            let c = ((p[1] / cell) as usize).min(h - 1) * w + ((p[0] / cell) as usize).min(w - 1);
            if g.walk[c] {
                return None;
            }
            let (x, y) = (c % w, c / w);
            let touches = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)]
                .into_iter()
                .any(|(nx, ny)| nx < w && ny < h && g.walk[ny * w + nx]);
            touches.then_some(*p)
        });
        if let Some(p) = at {
            built.objects.push(PlacedObject {
                category: Category::HiddenRoomPath,
                type_name: HIDDEN_PASSAGE_NAME.to_owned(),
                color: HIDDEN_PASSAGE_COLOR,
                x_m: p[0] - side / 2.0,
                y_m: p[1] - side / 2.0,
                w_m: side,
                h_m: side,
                angle_deg: 0.0,
                parts: Vec::new(),
                round: false,
            });
            room.objects.push(built.objects.len() - 1);
        }
    }
    built
}

/// Interactive objects: the shared core on forest ground. Runs after the
/// hidden glades, so types may spawn inside them (never on their path).
pub(crate) fn interactive(config: &LevelConfig, rng: &mut ChaCha8Rng, g: &ForestSpawnGround, rooms: &HiddenRoomLayout, stage: &Stage) -> InteractiveBuild {
    let (w, h, cell) = (g.w, g.h, g.cell);
    let n = w * h;
    let (room_of, _) = mobs::room_cells(Some(rooms), w, h, cell);
    let access: Vec<bool> = (0..n).map(|c| rooms.floor_mask.cells[c] && room_of[c] == 0).collect();
    let near_gate = g.near_gates(GATE_CLEAR_M);
    // Blockers: not walkable (unless a hidden floor), trunks, props; bridges
    // are kept clear like blockers.
    let blockers: Vec<bool> = (0..n).map(|c| ((!g.ground[c].walkable() || g.solid[c]) && !rooms.floor_mask.cells[c]) || g.ground[c] == ForestGround::Bridge).collect();
    let near = dilate(&blockers, w, h, 1);
    let ground = |c: usize| g.land(c) || g.trail(c) || g.ground[c] == ForestGround::Shallow || room_of[c] != 0;
    let free: Vec<bool> = (0..n).map(|c| ground(c) && !access[c] && !near[c] && !near_gate[c]).collect();
    let along_walls = dilate(&blockers, w, h, interactive::ALONG_WALLS_CELLS);
    let busy: Vec<bool> = (0..n).map(|c| blockers[c] || g.trail(c) || access[c]).collect();
    let crowded = dilate(&busy, w, h, (interactive::OPEN_GROUND_M / cell).ceil().max(1.0) as i64);
    let all = vec![true; n];
    let sets = g.sets(&all, &room_of);
    // "Near land": distance from open ground / forest floor.
    let (to_land, _) = distance_field(w, h, cell, |c| g.land(c), 60.0);
    let terrain = SwampTerrain { sets, to_land_m: to_land, room_of };
    let area = Area { w, h, cell, free, along_walls, crowded, terrain: Some(terrain) };
    interactive::place_types(config, rng, area, stage)
}

/// Mobs: the shared spending core on forest ground, glades as hubs and
/// guard packs in dead-end areas.
pub(crate) fn mobs(
    config: &LevelConfig,
    rng: &mut ChaCha8Rng,
    g: &ForestSpawnGround,
    objects: &[&CellMask],
    rooms: Option<&HiddenRoomLayout>,
    stage: &Stage,
) -> MobBuild {
    use rand::Rng;
    let (w, h, cell) = (g.w, g.h, g.cell);
    let n = w * h;
    let s = &config.mobs;
    let near_gate = g.near_gates(s.gate_clearance_m.max(0.0));
    let (room_of, room_ok) = mobs::room_cells(rooms, w, h, cell);
    let hidden_floor = |c: usize| rooms.is_some_and(|l| l.floor_mask.cells[c]);
    let spawnable: Vec<bool> = (0..n)
        .map(|c| {
            if near_gate[c] || objects.iter().any(|m| m.cells[c]) {
                return false;
            }
            if room_of[c] != 0 {
                return room_ok[c];
            }
            g.walk[c] && !hidden_floor(c)
        })
        .collect();
    let terrain = g.sets(&spawnable, &room_of);

    // Dead-end areas: one link.
    let k = g.areas.areas.len();
    let mut degree = vec![0u32; k + 1];
    for l in g.passages.links.iter().filter(|l| l.linked) {
        degree[l.a as usize] += 1;
        degree[l.b as usize] += 1;
    }
    let chance = s.dead_end_guard_pct.clamp(0.0, 100.0) / 100.0;
    let ids = &g.areas.ids;
    let guards = (1..=k as u16)
        .filter(|&id| degree[id as usize] == 1)
        .filter(|_| rng.random::<f32>() < chance)
        .map(|id| (0..n).map(|c| spawnable[c] && ids[c] == id).collect())
        .collect();
    // Glades are the hubs.
    let hub = (s.hub_pct > 0.0 && !g.clearings.glades.is_empty()).then(|| {
        let glades: Vec<(f32, [f32; 2])> =
            g.clearings.glades.iter().map(|gl| ((gl.area_m2 / std::f32::consts::PI).sqrt(), gl.center_m)).collect();
        (0..n)
            .map(|c| {
                let (x, y) = g.center(c);
                spawnable[c] && glades.iter().any(|&(r, p)| (p[0] - x).hypot(p[1] - y) <= r)
            })
            .collect()
    });
    let area = MobArea {
        w,
        h,
        cell,
        spawnable,
        room_of,
        road: (0..n).map(|c| g.trail(c)).collect(),
        terrain: Some(terrain),
        guards,
        hub,
    };
    mobs::spend(config, rng, &area, stage)
}

/// Cells under the trunks and props (solid objects) of the forest.
pub fn objects_mask(objects: &[PlacedObject], w: usize, h: usize, cell: f32) -> CellMask {
    let mut m = CellMask::new(w, h, cell);
    for o in objects.iter().filter(|o| matches!(o.category, Category::Structure | Category::Prop)) {
        let r = o.w_m.max(o.h_m);
        let (cx, cy) = o.center();
        let (x0, x1) = (((cx - r) / cell).floor().max(0.0) as usize, (((cx + r) / cell).ceil() as usize).min(w));
        let (y0, y1) = (((cy - r) / cell).floor().max(0.0) as usize, (((cy + r) / cell).ceil() as usize).min(h));
        for y in y0..y1 {
            for x in x0..x1 {
                let (px, py) = ((x as f32 + 0.5) * cell, (y as f32 + 0.5) * cell);
                if o.contains(px, py) || (x, y) == (((cx / cell) as usize).min(w - 1), ((cy / cell) as usize).min(h - 1)) {
                    m.cells[y * w + x] = true;
                }
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use crate::generator::grid;
    use crate::level::{Category, CellMask, ForestGround, GeneratedLevel};
    use crate::model::{InteractiveType, LevelConfig, MapKind, MobType, SpawnOn};

    fn config() -> LevelConfig {
        let mut c = LevelConfig { kind: MapKind::Forest, ..LevelConfig::default() };
        c.shape = crate::model::ShapeSettings::for_kind(MapKind::Forest);
        c.map.width_m = 160.0;
        c.map.height_m = 140.0;
        c.cell_size_m = 0.5;
        c.hidden_rooms.count = 2;
        c.mobs.budget = 600.0;
        c.mobs.max_entities = 120;
        c
    }

    fn level(c: &LevelConfig, seed: u64) -> GeneratedLevel {
        crate::generator::generate(c, seed)
    }

    /// Walkable ground + hidden floors, minus every solid object.
    fn open(l: &GeneratedLevel) -> CellMask {
        let f = l.forest.as_ref().unwrap();
        let p = f.passages.as_ref().unwrap();
        let ground = f.ground().unwrap();
        let rooms = l.hidden_rooms.as_ref().unwrap();
        let solid = super::objects_mask(&l.objects, p.width, p.height, p.cell_size_m);
        let inter = &l.interactive.as_ref().unwrap().mask;
        CellMask {
            width: p.width,
            height: p.height,
            cell_size_m: p.cell_size_m,
            cells: (0..ground.len())
                .map(|k| (ForestGround::from_code(ground[k]).walkable() || rooms.floor_mask.cells[k]) && !solid.cells[k] && !inter.cells[k])
                .collect(),
        }
    }

    #[test]
    fn everything_spawns_and_the_ground_stays_one_piece() {
        let c = config();
        for seed in 0..3 {
            let l = level(&c, seed);
            assert!(l.interactive.as_ref().unwrap().types[0].placed > 0, "seed {seed}: {:?}", l.notes);
            assert!(!l.hidden_rooms.as_ref().unwrap().rooms.is_empty(), "seed {seed}: {:?}", l.notes);
            assert!(l.mobs.as_ref().unwrap().spent >= 500.0, "seed {seed}: {:?}", l.notes);
            assert!(grid::is_connected(&open(&l)), "seed {seed}: spawns split the ground");
            for g in &l.forest.as_ref().unwrap().passages.as_ref().unwrap().gates {
                assert_eq!(l.objects[g.object].category, Category::Gate);
            }
        }
    }

    #[test]
    fn hidden_glades_lie_in_the_thicket_with_a_passage_marker() {
        let l = level(&config(), 2);
        let f = l.forest.as_ref().unwrap();
        let ground = f.ground().unwrap();
        let rooms = l.hidden_rooms.as_ref().unwrap();
        assert!(!rooms.rooms.is_empty());
        let p = f.passages.as_ref().unwrap();
        for r in &rooms.rooms {
            for &i in &r.objects {
                let o = &l.objects[i];
                if o.category != Category::HiddenRoom {
                    continue;
                }
                let (cx, cy) = o.center();
                let k = ((cy / p.cell_size_m) as usize) * p.width + (cx / p.cell_size_m) as usize;
                assert_eq!(ForestGround::from_code(ground[k]), ForestGround::Thicket, "glade {} off the thicket", r.id);
                assert!(!p.sealed.cells[k], "glade {} on a sealed strip", r.id);
            }
            assert!(r.objects.iter().any(|&i| l.objects[i].type_name == super::HIDDEN_PASSAGE_NAME), "glade {} has no marker", r.id);
        }
    }

    #[test]
    fn wild_and_managed_filter_spawns() {
        let mut c = config();
        c.interactive = vec![InteractiveType {
            name: "Herb".into(),
            spawn_on: SpawnOn { land: true, natural: true, ..SpawnOn::NONE },
            ..InteractiveType::default()
        }];
        c.mobs.types = vec![MobType { name: "Bandit".into(), spawn_on: SpawnOn { land: true, path: true, hand_made: true, ..SpawnOn::NONE }, ..MobType::default() }];
        c.mobs.combinations = Vec::new();
        let l = level(&c, 3);
        let a = l.forest.as_ref().unwrap().areas.as_ref().unwrap();
        for g in &l.interactive.as_ref().unwrap().groups {
            for &m in &g.members {
                let (x, y) = l.objects[m].center();
                let k = (y / 0.5) as usize * a.width + (x / 0.5) as usize;
                assert_eq!(a.area_at(k).style, crate::level::ForestStyle::Wild, "herb in a managed area");
            }
        }
        let mobs = l.mobs.as_ref().unwrap();
        assert!(!mobs.spawns.is_empty(), "{:?}", l.notes);
        for sp in &mobs.spawns {
            let k = (sp.center_m[1] / 0.5) as usize * a.width + (sp.center_m[0] / 0.5) as usize;
            assert_eq!(a.area_at(k).style, crate::level::ForestStyle::Managed, "bandit in a wild area");
        }
    }

    #[test]
    fn same_seed_same_spawns() {
        let c = config();
        assert_eq!(level(&c, 5).objects, level(&c, 5).objects);
    }
}
