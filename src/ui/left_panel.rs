use eframe::egui;
use rand::Rng;

use crate::model::{LevelConfig, MapKind, ShapeSettings};
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
                }
                ui.weak("Changes apply on ⚙ Generate");
            });
            if let Some(shape) = app.generated.as_ref().and_then(|l| l.shape.as_ref()) {
                ui.weak(format!(
                    "generated: {:.1}% of the map, {} cells",
                    shape.stats.coverage_pct,
                    shape.mask.count_true()
                ));
            }
        },
    );
}

/// Node 3: the outer wall ring around the region. In development: its test
/// controls live in the bench; once accepted they move into this node body.
fn node_outer_walls(ui: &mut egui::Ui, app: &mut MapGenApp) {
    node(ui, "3 · Outer walls", |ui| {
        ui.weak("input: region mask · output: region mask + wall ring");
        ui.weak("⚙ in development — test controls in the Current workflow bench");
        if let Some(wall) = app.generated.as_ref().and_then(|l| l.walls.as_ref()) {
            ui.weak(format!(
                "generated: {} points, {} wall cells",
                wall.stats.points, wall.stats.wall_cells
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
