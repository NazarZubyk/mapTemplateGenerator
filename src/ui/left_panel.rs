use eframe::egui;
use rand::Rng;

use crate::model::{LevelConfig, MapKind, NodeAmount, ShapeSettings};
use crate::ui::app::{ConfigStatus, MapGenApp};
use crate::ui::widgets::param_row;

/// The settings panel: the generation pipeline as a vertical chain of nodes.
/// Node 1 is the empty template (the map); every next node's input is the
/// output of the node above it. A section under development is tested in the
/// "Current workflow" bench first, then becomes a node here (docs/workflow.md).
/// Config save/load and export are app features and stay permanently.
pub fn show(ui: &mut egui::Ui, app: &mut MapGenApp) {
    generation_node_chain(ui, app);
    export_section(ui, app);
    config_file_section(
        ui,
        &mut app.config,
        &mut app.config_path,
        &mut app.config_status,
    );
}

/// The pipeline: one card per generation node, top to bottom.
fn generation_node_chain(ui: &mut egui::Ui, app: &mut MapGenApp) {
    node_template(ui, app);
    flow_arrow(ui, "empty field (the map)");
    node_shape(ui, app);
    flow_arrow(ui, "region mask");
    node_outer_walls(ui, app);
    flow_arrow(ui, "region inside the wall");
    node_regions(ui, app);
    flow_arrow(ui, "region id map");
    node_roads(ui, app);
    flow_arrow(ui, "regions + roads");
    node_region_walls(ui, app);
    if app.config.kind == MapKind::Town {
        flow_arrow(ui, "free ground between roads and walls");
        node_houses(ui, app);
    }
    ui.weak("Next nodes appear here as sections finish; which nodes exist depends on the map kind (docs/workflow.md).");
    ui.add_space(4.0);
    ui.separator();
}

fn node<R>(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui) -> R) {
    egui::CollapsingHeader::new(egui::RichText::new(title).strong())
        .default_open(true)
        .show(ui, body);
}

/// Small "data flows down" marker between two nodes, naming what flows.
fn flow_arrow(ui: &mut egui::Ui, what: &str) {
    ui.horizontal(|ui| {
        ui.label("↓");
        ui.weak(what);
    });
}

/// Node 1: the empty template — a sized field plus WHAT the map is. The kind
/// decides the shape character and, later, which nodes the pipeline gets.
fn node_template(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_SIZE: &str = "Map size in meters — the empty field the Template node \
outputs and every other node works on. The preview keeps the old picture until you \
press ⚙ Generate.";
    const TIP_CELL: &str = "Grid resolution in meters per cell; everything the generator \
builds is quantized to this grid. 0.5 m: fine, crisp edges, but 4× more cells than 1 m \
(and slower). 1–2 m: chunkier edges, faster. Changing it re-samples the noise, so the \
region shape can shift.";
    const TIP_KIND: &str = "What this map is. It picks the shape defaults (a TOWN keeps \
most of the map and cuts the rest away; swamp / forest / cave scatter islands and \
bridge them) and decides which nodes the pipeline gets — a forest has no buildings \
node, for example. Switching the kind loads that kind's shape defaults.";
    node(ui, "1 · Template", |ui| {
        ui.weak("input: nothing · output: empty field + map kind");
        egui::Grid::new("template_node_grid")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Kind").underline())
                    .on_hover_text(TIP_KIND);
                let before = app.config.kind;
                egui::ComboBox::from_id_salt("map_kind_combo")
                    .selected_text(app.config.kind.label())
                    .show_ui(ui, |ui| {
                        for kind in MapKind::ALL {
                            ui.selectable_value(&mut app.config.kind, kind, kind.label());
                        }
                    })
                    .response
                    .on_hover_text(TIP_KIND);
                if app.config.kind != before {
                    app.config.shape = ShapeSettings::for_kind(app.config.kind);
                }
                ui.end_row();
                ui.label(egui::RichText::new("Width").underline())
                    .on_hover_text(TIP_SIZE);
                ui.add(
                    egui::DragValue::new(&mut app.config.map.width_m)
                        .range(4.0..=512.0)
                        .speed(1.0)
                        .suffix(" m"),
                )
                .on_hover_text(TIP_SIZE);
                ui.end_row();
                ui.label(egui::RichText::new("Height").underline())
                    .on_hover_text(TIP_SIZE);
                ui.add(
                    egui::DragValue::new(&mut app.config.map.height_m)
                        .range(4.0..=512.0)
                        .speed(1.0)
                        .suffix(" m"),
                )
                .on_hover_text(TIP_SIZE);
                ui.end_row();
                ui.label(egui::RichText::new("Cell size").underline())
                    .on_hover_text(TIP_CELL);
                ui.add(
                    egui::DragValue::new(&mut app.config.cell_size_m)
                        .range(0.1..=4.0)
                        .speed(0.05)
                        .suffix(" m"),
                )
                .on_hover_text(TIP_CELL);
                ui.end_row();
            });
        ui.weak("Changes apply on ⚙ Generate");
    });
}

/// Node 2: the noise-derived region shape (finished — promoted from the
/// bench; see step 1 notes in docs/workflow.md).
fn node_shape(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_SCALE: &str = "Scale of the noise blobs in meters — how far apart the \
field's highs and lows sit. 8–15 m: many scattered islands. ~24 m: a few medium blobs. \
60+ m: one landmass with a smooth edge. Always changes the shape on Generate — the \
strongest lever together with Coverage.";
    const TIP_OCTAVES: &str = "How many noise layers are stacked: each layer adds detail \
at half the size of the previous one. 1–2: smooth, wavy coasts. 4–5: natural, slightly \
ragged edges. 7–8: crumpled, restless edges. Subtle when the feature size is much \
larger than the map.";
    const TIP_COVERAGE: &str = "How much of the BUILDABLE area the region should cover \
(the buildable area is the map minus the border margin; with margin 0 it is the whole \
map). The cut level is picked automatically to hit this share on every seed. Low \
(20–40%): scattered islands — swamp / cave style. High (80%+): mostly full with eroded \
edges — town style. Always visible on Generate.";
    const TIP_MIN_BLOB: &str = "Pieces smaller than this many cells are deleted before \
bridging. 1: keep specks. 8+: small islands disappear. NO EFFECT when the cut already \
produced a single piece or only large pieces — check 'pieces' in the stats line.";
    const TIP_BRIDGE: &str = "Thickness of the corridors that weld separate pieces into \
one connected shape. 1 cell: thin isthmuses. 3–4: proper connective strips. NO EFFECT \
when the cut produced a single piece.";
    const TIP_BORDER: &str = "Distance the region keeps from the map border — with \
randomness. The margin line is not straight: it meanders with the noise, cutting deep \
in some places and barely in others. 0 m: the region may touch the border.";
    const TIP_WARP: &str = "Bends the noise: it is sampled at coordinates pushed around \
by a second noise field, so blobs flow and swirl (river bends, curled bays, tendrils) \
instead of looking like plain noise. 0%: plain noise. 40–70%: natural. 100%: strongly \
swirled. Always changes the shape on Generate.";
    const TIP_SILHOUETTE: &str = "Pulls the region toward a randomly placed, stretched \
and rotated center (a new one every seed): the shape gathers into one tilted organic \
mass instead of filling the map rectangle, and islands cluster instead of spreading \
evenly. 0%: no pull — the noise alone decides (town ≈ the map rectangle). 100%: a \
compact tilted mass. Weaker with a very high coverage (little is cut away).";
    const TIP_WANDER: &str = "How much the bridges between pieces wind: they follow \
the valleys of a noise field instead of the straight shortest line. 0%: straight \
L-shaped corridors. 60–80%: natural winding passages. NO EFFECT when the cut produced a \
single piece.";
    const TIP_SCALE_MAP: &str = "When on, Noise feature size, Border margin, Min blob and \
Bridge width are values for a 64 m map and grow with the map: a 200 m map gets the same \
shape character as a 64 m one, just bigger (×3.1 sizes, ×9.8 min blob). Off: the values \
are absolute — a big map then gets a much busier, fringier coastline, thin bridges and \
many small specks. No effect on a 64 m map.";
    const TIP_SECTIONS: &str = "How many sections (islands) the shape is built from. \
Each section is its own organic mass with its own tilt, separated from the others by an \
open channel (Section gap) and joined to them by wide bridges (Section bridge) — a town \
on several islands, a cave of several chambers. 1: one mass (default) — Section gap and \
Section bridge then have NO EFFECT. A section can come out empty with a very low coverage.";
    const TIP_GAP: &str = "Width of the open channel between sections, in meters (×map \
size / 64 with Scale with map). Small: sections almost touch. Large: clearly separate \
islands with long bridges. NO EFFECT with 1 section.";
    const TIP_SECTION_BRIDGE: &str = "Width of the bridges joining sections, in meters \
(×map size / 64 with Scale with map). Sections are joined closest-first, so N sections \
get N−1 bridges; they wind with Bridge wander. NO EFFECT with 1 section.";
    node(
        ui,
        &format!("2 · {} shape", app.config.kind.label()),
        |ui| {
            ui.weak("input: template field · output: region mask (one connected blob)");
            egui::Grid::new("shape_node_grid")
                .num_columns(2)
                .spacing([10.0, 3.0])
                .show(ui, |ui| {
                    let shape = &mut app.config.shape;
                    param_row(ui, "Noise feature size", TIP_SCALE, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut shape.noise_scale_m)
                                .range(4.0..=120.0)
                                .speed(0.4)
                                .suffix(" m"),
                        )
                    });
                    param_row(ui, "Octaves (detail)", TIP_OCTAVES, |ui| {
                        ui.add(egui::DragValue::new(&mut shape.octaves).range(1..=8).speed(0.2))
                    });
                    param_row(ui, "Coverage (buildable)", TIP_COVERAGE, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut shape.coverage_target_pct)
                                .range(1.0..=99.0)
                                .speed(0.5)
                                .suffix(" %"),
                        )
                    });
                    param_row(ui, "Min blob (cells)", TIP_MIN_BLOB, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut shape.min_blob_cells)
                                .range(1..=64)
                                .speed(0.3),
                        )
                    });
                    param_row(ui, "Bridge width", TIP_BRIDGE, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut shape.bridge_width_cells)
                                .range(1..=8)
                                .speed(0.2)
                                .suffix(" cells"),
                        )
                    });
                    param_row(ui, "Border margin", TIP_BORDER, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut shape.border_margin_m)
                                .range(0.0..=64.0)
                                .speed(0.2)
                                .suffix(" m"),
                        )
                    });
                    param_row(ui, "Sections", TIP_SECTIONS, |ui| {
                        ui.add(egui::DragValue::new(&mut shape.sections).range(1..=8).speed(0.1))
                    });
                    param_row(ui, "Section gap", TIP_GAP, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut shape.section_gap_m)
                                .range(0.0..=40.0)
                                .speed(0.2)
                                .suffix(" m"),
                        )
                    });
                    param_row(ui, "Section bridge", TIP_SECTION_BRIDGE, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut shape.section_bridge_m)
                                .range(0.5..=20.0)
                                .speed(0.1)
                                .suffix(" m"),
                        )
                    });
                    param_row(ui, "Scale with map", TIP_SCALE_MAP, |ui| {
                        ui.checkbox(&mut shape.scale_with_map, "")
                    });
                    param_row(ui, "Warp (flow)", TIP_WARP, |ui| {
                        ui.add(egui::Slider::new(&mut shape.warp_pct, 0.0..=100.0).suffix(" %"))
                    });
                    param_row(ui, "Silhouette pull", TIP_SILHOUETTE, |ui| {
                        ui.add(
                            egui::Slider::new(&mut shape.silhouette_pct, 0.0..=100.0).suffix(" %"),
                        )
                    });
                    param_row(ui, "Bridge wander", TIP_WANDER, |ui| {
                        ui.add(
                            egui::Slider::new(&mut shape.bridge_wander_pct, 0.0..=100.0)
                                .suffix(" %"),
                        )
                    });
                });
            ui.horizontal(|ui| {
                if ui
                    .button("🎲 Random params")
                    .on_hover_text(
                        "Every value is picked inside its reasonable range, but the \
                         combination is deliberately unconstrained — parameters may \
                         contradict each other. Set values only; press Generate.",
                    )
                    .clicked()
                {
                    let mut rng = rand::rng();
                    let shape = &mut app.config.shape;
                    shape.noise_scale_m = rng.random_range(4.0..=120.0);
                    shape.octaves = rng.random_range(1..=8);
                    shape.coverage_target_pct = rng.random_range(1.0..=99.0);
                    shape.min_blob_cells = rng.random_range(1..=64);
                    shape.bridge_width_cells = rng.random_range(1..=8);
                    shape.border_margin_m = rng.random_range(0.0..=32.0);
                    shape.warp_pct = rng.random_range(0.0..=100.0);
                    shape.silhouette_pct = rng.random_range(0.0..=100.0);
                    shape.bridge_wander_pct = rng.random_range(0.0..=100.0);
                    shape.sections = rng.random_range(1..=5);
                    shape.section_gap_m = rng.random_range(2.0..=20.0);
                    shape.section_bridge_m = rng.random_range(1.0..=10.0);
                }
                ui.weak("Changes apply on ⚙ Generate");
            });
            if let Some(shape) = app.generated.as_ref().and_then(|l| l.shape.as_ref()) {
                ui.weak(format!(
                    "generated: {:.1}% of the map, {} cells, {} section(s), {} wide bridge(s)",
                    shape.stats.coverage_pct,
                    shape.mask.count_true(),
                    shape.stats.sections,
                    shape.stats.section_bridges
                ));
            }
        },
    );
}

/// Node 3: the outer wall ring (finished — promoted from the bench; see
/// step 2 notes in docs/workflow.md).
fn node_outer_walls(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_SEG_MIN: &str = "Shortest wall segment in meters. The wall is built point \
by point: it starts on the region border, walks the border and places the next point \
after a random length between min and max. Small values (2–5 m) hug the border closely; \
large values cut corners and can bridge over small bays.";
    const TIP_SEG_MAX: &str = "Longest wall segment in meters. With min == max every \
segment has the same length and the loop closes with a straight chord that can pass \
through the interior — that is expected. If max is smaller than min the two are swapped \
at generation time (a warning tells you).";
    const TIP_THICKNESS: &str = "Wall thickness in meters, rounded to whole grid cells. \
The wall grows on both sides of the walked line; bigger values swallow more of the \
region behind the wall into the ring itself.";
    node(ui, "3 · Outer walls", |ui| {
        ui.weak("input: region mask · output: region mask + wall ring");
        egui::Grid::new("wall_node_grid")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                let wall = &mut app.config.outer_wall;
                param_row(ui, "Segment min", TIP_SEG_MIN, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut wall.segment_min_m)
                            .range(0.5..=64.0)
                            .speed(0.2)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Segment max", TIP_SEG_MAX, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut wall.segment_max_m)
                            .range(0.5..=64.0)
                            .speed(0.2)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Wall width", TIP_THICKNESS, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut wall.thickness_m)
                            .range(0.1..=16.0)
                            .speed(0.1)
                            .suffix(" m"),
                    )
                });
            });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls the segment lengths and thickness independently inside their \
                     reasonable ranges — they may contradict each other. Set values \
                     only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                let wall = &mut app.config.outer_wall;
                wall.segment_min_m = rng.random_range(0.5..=24.0);
                wall.segment_max_m = rng.random_range(0.5..=24.0);
                wall.thickness_m = rng.random_range(0.1..=8.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(wall) = app.generated.as_ref().and_then(|l| l.walls.as_ref()) {
            ui.weak(format!(
                "generated: {} points, {} wall cells",
                wall.stats.points, wall.stats.wall_cells
            ));
        }
    });
}

const TIP_COUNT: &str = "How many regions the town area is split into. Changing the \
count resets the shares to equal parts.";

const TIP_SHARE: &str = "This region's share of the town area, in %. Raising it shrinks \
all the others automatically so the total stays 100. Placement runs SMALLEST region \
first: it starts on the wall or deep inside (see Nested chance) and grows to its target; \
every next one starts beside the already-placed ones and grows the same way; the largest is \
placed last and fills everything left, so the town always ends fully split. Every \
region comes out as one connected piece with smooth, wavy borders.";

const TIP_NESTING: &str = "Chance, per region, that it starts deep inside the town \
instead of against the wall. A nested region gets surrounded by the regions placed after \
it (the larger ones), so you get districts inside districts. 0% = every region sits on \
the wall, 100% = every region nests inside the next. No visible effect with 1 region; \
with a tiny town or very large shares there may be no spot deep enough, and the region \
starts as deep as it can.";

const TIP_IRREGULARITY: &str = "How irregular region shapes are. 0% = compact round \
blobs. Higher values stretch each region along its own random direction (up to 3× longer \
than wide) and let a strong noise field carve lobes, bays and fingers into its border. No \
visible effect with 1 region (it takes the whole town).";

/// Rebalance the shares after region `i` was changed to a new value: the
/// others are rescaled to fill the remainder, keeping their proportions.
fn rebalance(shares: &mut [f32], i: usize) {
    let n = shares.len();
    if n <= 1 {
        if n == 1 {
            shares[0] = 100.0;
        }
        return;
    }
    let rest_target = (100.0 - shares[i]).max(0.0);
    let rest_sum: f32 = shares
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != i)
        .map(|(_, v)| v)
        .sum();
    if rest_sum > 0.0 {
        for (j, s) in shares.iter_mut().enumerate() {
            if j != i {
                *s = *s / rest_sum * rest_target;
            }
        }
    } else {
        let each = rest_target / (n - 1) as f32;
        for (j, s) in shares.iter_mut().enumerate() {
            if j != i {
                *s = each;
            }
        }
    }
}

/// Node 4: the region split of the town area (finished — promoted from the
/// bench; see step 3 notes in docs/workflow.md).
fn node_regions(ui: &mut egui::Ui, app: &mut MapGenApp) {
    node(ui, "4 · Regions", |ui| {
        ui.weak("input: region inside the wall · output: region id map");
        egui::Grid::new("regions_node_grid")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                let regions = &mut app.config.regions;
                let before = regions.count;
                param_row(ui, "Regions count", TIP_COUNT, |ui| {
                    ui.add(egui::DragValue::new(&mut regions.count).range(1..=8).speed(0.2))
                });
                if regions.count != before {
                    let count = regions.count as usize;
                    regions.shares = vec![100.0 / count as f32; count];
                }
                param_row(ui, "Nested chance", TIP_NESTING, |ui| {
                    ui.add(egui::Slider::new(&mut regions.nesting_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Irregularity", TIP_IRREGULARITY, |ui| {
                    ui.add(egui::Slider::new(&mut regions.irregularity_pct, 0.0..=100.0).suffix(" %"))
                });
                for i in 0..regions.shares.len() {
                    let before = regions.shares[i];
                    param_row(ui, &format!("Region {}", i + 1), TIP_SHARE, |ui| {
                        ui.add(egui::Slider::new(&mut regions.shares[i], 1.0..=98.0).suffix(" %"))
                    });
                    if regions.shares[i] != before {
                        rebalance(&mut regions.shares, i);
                    }
                }
            });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls the count, shares, nested chance and irregularity independently \
                     inside their reasonable ranges. Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                let regions = &mut app.config.regions;
                regions.count = rng.random_range(1..=8);
                let raw: Vec<f32> = (0..regions.count).map(|_| rng.random_range(1.0..=10.0)).collect();
                let sum: f32 = raw.iter().sum();
                regions.shares = raw.iter().map(|v| v / sum * 100.0).collect();
                regions.nesting_pct = rng.random_range(0.0..=100.0);
                regions.irregularity_pct = rng.random_range(0.0..=100.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(regions) = app.generated.as_ref().and_then(|l| l.regions.as_ref()) {
            let line = regions
                .stats
                .iter()
                .map(|s| format!("#{} {:.1}%", s.id, s.actual_pct))
                .collect::<Vec<_>>()
                .join(" · ");
            ui.weak(format!("generated: {line}"));
        }
    });
}

const TIP_AMOUNT: &str = "How the number of road junctions (the points roads run between) is set. count: a fixed total. \
density: nodes per 1000 m² of town area, so a bigger town gets more crossroads. Either \
way every region gets at least one node, so the minimum is the regions count (node 4).";

const TIP_NODES: &str = "Total road junctions — the points the roads run between (not drawn on the map). Never fewer than the regions count — \
every region gets one; the rest go to the regions with the most area per node, so big \
regions get more. More junctions: a denser street network with shorter roads.";

const TIP_DENSITY: &str = "Road junctions per 1000 m² of town area (0.5 m cells: 4000 \
cells = 1000 m²). Never fewer than one per region, at most 64 in total.";

const TIP_WIDTH: &str = "Width of the main roads in meters — the widest a road gets \
(rounded to whole cells, at least one). Wall gaps follow each road's own width.";

const TIP_MIN_WIDTH: &str = "Width of the quietest roads (dead-end alleys) in meters. \
Roads get wider with the traffic they carry: every pair of junctions is joined by its \
shortest route, and each road counts the routes that use it. Set equal to Max width for \
roads all the same width. If it is bigger than Max width the two are swapped.";

const TIP_MAIN: &str = "Share of the roads that count as main roads — the busiest ones — \
and get the full Max width. The rest narrow with their traffic toward Min width. 100%: \
every road at Max width. 0%: only the single busiest road is full width. NO EFFECT when Min \
width equals Max width.";

const TIP_WIGGLE: &str = "How much each road bends away from the straight line, following \
a smooth noise field. 0%: straight (only bent around obstacles like bays). 30–50%: gentle \
curves. 100%: a lazy meander — still never a maze.";

const TIP_EXTRA: &str = "The minimum network connects all crossroads with no loops. Each \
extra candidate link (between near neighbors) is added with this chance — loops and \
alternative routes. Candidates that would cross another road or meet one at a sharp angle \
are skipped. 0%: a tree (exactly one route between any two crossroads). NO EFFECT with 2 \
or fewer crossroads.";

/// Node 5: the road network, every map kind (finished; see step 4 notes in
/// docs/workflow.md). Crossroad nodes only steer where roads go — the map
/// shows the roads alone.
fn node_roads(ui: &mut egui::Ui, app: &mut MapGenApp) {
    node(ui, "5 · Roads", |ui| {
        ui.weak("input: region id map · output: road mask (+ passages for region walls)");
        let min_nodes = app.config.regions.count.max(1);
        let roads = &mut app.config.roads;
        egui::Grid::new("roads_node_grid")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                param_row(ui, "Max width", TIP_WIDTH, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut roads.width_m)
                            .range(0.5..=12.0)
                            .speed(0.1)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Min width", TIP_MIN_WIDTH, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut roads.min_width_m)
                            .range(0.5..=12.0)
                            .speed(0.1)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Main roads", TIP_MAIN, |ui| {
                    ui.add(egui::Slider::new(&mut roads.main_roads_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Wiggle", TIP_WIGGLE, |ui| {
                    ui.add(egui::Slider::new(&mut roads.wiggle_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Extra links", TIP_EXTRA, |ui| {
                    ui.add(egui::Slider::new(&mut roads.extra_links_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Junctions by", TIP_AMOUNT, |ui| {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut roads.amount, NodeAmount::Count, "count");
                        ui.radio_value(&mut roads.amount, NodeAmount::Density, "density")
                    })
                    .inner
                });
                match roads.amount {
                    NodeAmount::Count => {
                        // The minimum follows the region count (edited in node 4).
                        roads.node_count = roads.node_count.max(min_nodes);
                        param_row(ui, "Junctions", TIP_NODES, |ui| {
                            ui.add(
                                egui::DragValue::new(&mut roads.node_count)
                                    .range(min_nodes..=crate::generator::roads::MAX_NODES.max(min_nodes))
                                    .speed(0.1),
                            )
                        });
                    }
                    NodeAmount::Density => {
                        param_row(ui, "Junctions / 1000 m²", TIP_DENSITY, |ui| {
                            ui.add(
                                egui::DragValue::new(&mut roads.nodes_per_1000m2)
                                    .range(0.0..=50.0)
                                    .speed(0.05),
                            )
                        });
                    }
                }
            });
        ui.weak(format!("Minimum junctions = regions count ({min_nodes}) — set in node 4."));
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Every value is picked inside its reasonable range, independently — the \
                     combination may contradict itself. Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                let roads = &mut app.config.roads;
                roads.amount = if rng.random_bool(0.5) { NodeAmount::Count } else { NodeAmount::Density };
                roads.node_count = rng.random_range(min_nodes..=24.max(min_nodes));
                roads.nodes_per_1000m2 = rng.random_range(0.5..=10.0);
                roads.width_m = rng.random_range(0.5..=6.0);
                roads.min_width_m = rng.random_range(0.5..=6.0);
                roads.main_roads_pct = rng.random_range(0.0..=100.0);
                roads.wiggle_pct = rng.random_range(0.0..=100.0);
                roads.extra_links_pct = rng.random_range(0.0..=100.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(net) = app.generated.as_ref().and_then(|l| l.roads.as_ref()) {
            let cells = net.mask.count_true();
            let main = net.links.iter().filter(|l| l.width_cells == net.width_cells).count();
            ui.weak(format!(
                "generated: {} roads ({} at max width), {:.0} m² of road, {} region crossings",
                net.links.len(),
                main,
                cells as f32 * net.mask.cell_size_m * net.mask.cell_size_m,
                net.passages.len()
            ));
        }
    });
}

/// Node 6: walls between regions, built like the outer wall and opened where
/// roads cross (see step 5 notes in docs/workflow.md).
fn node_region_walls(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_SEG_MIN: &str = "Shortest wall segment in meters. Each border between two \
regions is walked point by point, like the outer wall: the next point lands after a random \
length between min and max. Small values (1–3 m) hug the border closely; large values cut \
across its bends. NO EFFECT with 1 region (there are no borders).";
    const TIP_SEG_MAX: &str = "Longest wall segment in meters. Every wall still ends \
exactly where its border meets the outer wall or another region wall, so walls always \
join up with no leaks. If max is smaller than min the two are swapped.";
    const TIP_THICKNESS: &str = "Wall thickness in meters, rounded to whole grid cells. \
The wall grows on both sides of the walked line.";
    const TIP_GAP: &str = "Extra clearance on each side of a road where it passes through \
a wall, in meters: each opening is the road width plus twice this. 0: the opening is \
exactly the road. Openings exist only where roads cross — set roads in node 5.";
    node(ui, "6 · Region walls", |ui| {
        ui.weak("input: regions + roads · output: walls between regions with road gaps");
        egui::Grid::new("region_walls_node_grid")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                let walls = &mut app.config.region_walls;
                param_row(ui, "Segment min", TIP_SEG_MIN, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut walls.segment_min_m)
                            .range(0.5..=64.0)
                            .speed(0.2)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Segment max", TIP_SEG_MAX, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut walls.segment_max_m)
                            .range(0.5..=64.0)
                            .speed(0.2)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Wall width", TIP_THICKNESS, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut walls.thickness_m)
                            .range(0.1..=8.0)
                            .speed(0.1)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Gap margin", TIP_GAP, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut walls.gap_margin_m)
                            .range(0.0..=8.0)
                            .speed(0.1)
                            .suffix(" m"),
                    )
                });
            });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls every value independently inside its reasonable range — they may \
                     contradict each other. Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                let walls = &mut app.config.region_walls;
                walls.segment_min_m = rng.random_range(0.5..=16.0);
                walls.segment_max_m = rng.random_range(0.5..=16.0);
                walls.thickness_m = rng.random_range(0.1..=4.0);
                walls.gap_margin_m = rng.random_range(0.0..=3.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        match app.generated.as_ref().map(|l| l.region_walls.as_ref()) {
            Some(Some(walls)) => {
                ui.weak(format!(
                    "generated: {} wall lines, {} points, {} road gaps, {} cells refit to walls",
                    walls.lines,
                    walls.points,
                    walls.gaps.len(),
                    walls.recolored_cells
                ));
            }
            Some(None) => {
                ui.weak("generated: no region walls (only one region)");
            }
            None => {}
        }
    });
}

/// Node 7: houses (town maps only) — street fronts first, then the yards
/// (see step 6 notes in docs/workflow.md).
fn node_houses(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_DENSITY: &str = "Share of the buildable ground (town minus walls and roads) \
covered by houses. Street fronts fill first: a low density gives houses along the roads \
only, a high one fills the yards behind them too. The real ceiling is about 25–35% (roads, \
walls and the spacing around every house take the rest) — when the ask cannot fit, a note \
tells you how much was reached.";
    const TIP_SIZE_MIN: &str = "Smallest house side in meters. Width and depth are rolled \
independently between min and max, so houses come out square and oblong. A house that does \
not fit a tight spot shrinks toward this size.";
    const TIP_SIZE_MAX: &str = "Largest house side in meters. If max is smaller than min \
the two are swapped.";
    const TIP_SPACING: &str = "Free ground kept around every house — to other houses, walls \
and the town edge — in meters (at least one cell). It guarantees that houses never seal off \
a pocket of ground. Smaller: tighter, denser blocks.";
    const TIP_SETBACK: &str = "Gap between a street-front house and the road edge, in meters. \
0: houses stand right at the road. NO EFFECT on yard houses.";
    const TIP_FACE: &str = "Chance that a yard house (one not on a street front) turns to \
face the nearest road, with a few degrees of jitter. The rest get a random angle — a messy, \
organic old town. 100%: everything lines up with the streets. Street-front houses always \
face their road.";
    const TIP_COLOR: &str = "House color on the map image, the preview and the legend.";
    const TIP_COMPLEX: &str = "Share of houses built from more than one rectangle: a main \
block plus 1–2 overlapping wings, giving L, T, U, Z and cross shapes. Street houses never \
get a wing on their road side, so their front stays on the street. A complex house that \
does not fit a spot falls back to a plain block. 0%: every house is one rectangle.";
    const TIP_PARTS: &str = "Most rectangles in one complex house. 2: main block + one wing \
(L and T shapes). 3: up to two wings (also U, Z and cross shapes). NO EFFECT at 0% complex.";
    const TIP_WING_MIN: &str = "Smallest wing, as a share of the main block's side: how far \
along the block a wing runs, and how far it sticks out. Wings make a house bigger than Size \
max (which is the main block). NO EFFECT at 0% complex.";
    const TIP_WING_MAX: &str = "Largest wing, as a share of the main block's side. If it is \
smaller than the minimum the two are swapped. NO EFFECT at 0% complex.";
    node(ui, "7 · Houses", |ui| {
        ui.weak("input: town minus walls and roads · output: houses (town only)");
        egui::Grid::new("houses_node_grid")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                let houses = &mut app.config.houses;
                param_row(ui, "Density", TIP_DENSITY, |ui| {
                    ui.add(egui::Slider::new(&mut houses.density_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Size min", TIP_SIZE_MIN, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut houses.size_min_m)
                            .range(1.0..=40.0)
                            .speed(0.1)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Size max", TIP_SIZE_MAX, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut houses.size_max_m)
                            .range(1.0..=40.0)
                            .speed(0.1)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Spacing", TIP_SPACING, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut houses.spacing_m)
                            .range(0.0..=10.0)
                            .speed(0.05)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Road setback", TIP_SETBACK, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut houses.setback_m)
                            .range(0.0..=10.0)
                            .speed(0.05)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Face the road", TIP_FACE, |ui| {
                    ui.add(egui::Slider::new(&mut houses.face_road_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Complex houses", TIP_COMPLEX, |ui| {
                    ui.add(egui::Slider::new(&mut houses.complex_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Rectangles max", TIP_PARTS, |ui| {
                    ui.add(egui::DragValue::new(&mut houses.max_parts).range(2..=3).speed(0.05))
                });
                param_row(ui, "Wing size min", TIP_WING_MIN, |ui| {
                    ui.add(egui::Slider::new(&mut houses.wing_min_pct, 10.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Wing size max", TIP_WING_MAX, |ui| {
                    ui.add(egui::Slider::new(&mut houses.wing_max_pct, 10.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Color", TIP_COLOR, |ui| ui.color_edit_button_srgb(&mut houses.color));
            });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls every value independently inside its reasonable range — they may \
                     contradict each other. Keeps the color. Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                let houses = &mut app.config.houses;
                houses.density_pct = rng.random_range(5.0..=60.0);
                houses.size_min_m = rng.random_range(2.0..=8.0);
                houses.size_max_m = rng.random_range(3.0..=14.0);
                houses.spacing_m = rng.random_range(0.0..=3.0);
                houses.setback_m = rng.random_range(0.0..=3.0);
                houses.face_road_pct = rng.random_range(0.0..=100.0);
                houses.complex_pct = rng.random_range(0.0..=100.0);
                houses.max_parts = rng.random_range(2..=3);
                houses.wing_min_pct = rng.random_range(10.0..=100.0);
                houses.wing_max_pct = rng.random_range(10.0..=100.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(h) = app.generated.as_ref().and_then(|l| l.houses.as_ref()) {
            ui.weak(format!(
                "generated: {} houses ({} on streets, {} in yards, {} complex), {:.0}% of the buildable ground (asked {:.0}%)",
                h.street_houses + h.yard_houses,
                h.street_houses,
                h.yard_houses,
                h.complex_houses,
                h.coverage_pct,
                h.target_pct
            ));
        }
    });
}

fn section<R>(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui) -> R) {
    egui::CollapsingHeader::new(egui::RichText::new(title).strong())
        .default_open(true)
        .show(ui, body);
}

fn export_section(ui: &mut egui::Ui, app: &mut MapGenApp) {
    section(ui, "Export", |ui| {
        ui.horizontal(|ui| {
            ui.label("Folder:");
            ui.add(egui::TextEdit::singleline(&mut app.export_dir).desired_width(160.0));
            ui.label("Scale:");
            ui.add(
                egui::DragValue::new(&mut app.export_px_per_m)
                    .speed(1.0)
                    .range(1.0..=200.0)
                    .suffix(" px/m"),
            );
        });
        ui.weak("Writes map.png + layer.png (layer data) + spec.json (legend and objects)");

        let response = ui.add_enabled(
            app.generated.is_some(),
            egui::Button::new(egui::RichText::new("Export bundle").strong()),
        );
        if let Some(level) = app.generated.as_ref() {
            if response.clicked() {
                let dir = std::path::PathBuf::from(app.export_dir.trim());
                let status = match crate::export::export_bundle(level, &dir, app.export_px_per_m) {
                    Ok(r) => ConfigStatus {
                        ok: true,
                        text: format!(
                            "Exported: {}, {}, {}",
                            r.map_path.display(),
                            r.layer_path.display(),
                            r.spec_path.display()
                        ),
                    },
                    Err(e) => ConfigStatus { ok: false, text: e },
                };
                app.export_status = Some(status);
            }
        } else {
            response.on_disabled_hover_text("Generate a level first");
        }

        if let Some(status) = &app.export_status {
            let color = if status.ok {
                egui::Color32::from_rgb(140, 210, 140)
            } else {
                egui::Color32::from_rgb(255, 135, 135)
            };
            ui.colored_label(color, &status.text);
        }
    });
}

fn config_file_section(
    ui: &mut egui::Ui,
    config: &mut LevelConfig,
    config_path: &mut String,
    config_status: &mut Option<ConfigStatus>,
) {
    section(ui, "Config file", |ui| {
        ui.horizontal(|ui| {
            ui.label("Path:");
            ui.add(egui::TextEdit::singleline(config_path).desired_width(220.0));
        });
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                *config_status = Some(save_config(config, config_path));
            }
            if ui.button("Load").clicked() {
                match load_config(config_path) {
                    Ok(loaded) => {
                        *config = loaded;
                        *config_status = Some(ConfigStatus {
                            ok: true,
                            text: format!("Loaded from {config_path}"),
                        });
                    }
                    Err(e) => *config_status = Some(ConfigStatus {
                        ok: false,
                        text: format!("Load failed: {e}"),
                    }),
                }
            }
        });
        if let Some(status) = config_status {
            let color = if status.ok {
                egui::Color32::from_rgb(140, 210, 140)
            } else {
                egui::Color32::from_rgb(255, 135, 135)
            };
            ui.colored_label(color, &status.text);
        }
    });
}

fn save_config(config: &LevelConfig, path: &str) -> ConfigStatus {
    let result = (|| -> std::io::Result<()> {
        if let Some(parent) = std::path::Path::new(path).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        std::fs::write(path, json)
    })();
    match result {
        Ok(()) => ConfigStatus {
            ok: true,
            text: format!("Saved to {path}"),
        },
        Err(e) => ConfigStatus {
            ok: false,
            text: format!("Save failed: {e}"),
        },
    }
}

fn load_config(path: &str) -> Result<LevelConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}
