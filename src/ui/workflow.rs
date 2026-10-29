use eframe::egui;
use rand::Rng;

use crate::ui::app::MapGenApp;

/// Hover explanations for the step-1 settings. Each one states what the value
/// is responsible for and — importantly — when changing it has no visible
/// effect, which is the most common source of confusion.
const TIP_SCALE: &str = "Scale of the noise blobs in meters — how far apart the field's \
highs and lows sit. 8–15 m: many scattered islands. ~24 m: a few medium blobs (default). \
60+ m: one landmass with a smooth edge. Always changes the shape on Generate — the \
strongest lever together with Coverage.";

const TIP_OCTAVES: &str = "How many noise layers are stacked: each layer adds detail at \
half the size of the previous one. 1–2: smooth, wavy coasts. 4–5: natural, slightly \
ragged edges (default). 7–8: crumpled, restless edges. The effect is subtle when the \
feature size is much larger than the map.";

const TIP_COVERAGE: &str = "How much of the BUILDABLE area the region should cover (the \
buildable area is the map minus the border margin; with margin 0 it is the whole map). \
The cut level is picked automatically to hit this share on every seed. Low (20–40%): \
scattered islands — swamp / cave style. High (80%+): mostly full with eroded edges — \
town style. Always visible on Generate.";

const TIP_MIN_BLOB: &str = "Pieces smaller than this many cells are deleted before \
bridging. 1: keep specks. 8+: small islands disappear. NO EFFECT when the cut already \
produced a single piece or only large pieces — check 'pieces' in the stats line below \
the preview.";

const TIP_BRIDGE: &str = "Thickness of the corridors that weld separate pieces into one \
connected shape. 1 cell: thin isthmuses. 3–4: proper connective strips. NO EFFECT when \
the cut produced a single piece ('pieces 1' in the stats line) — raise Coverage or \
lower Octaves first if you want to see bridging happen.";

const TIP_BORDER: &str = "Distance the region keeps from the map border — with randomness. \
The margin line is not straight: it meanders with the noise, cutting deep into the region \
in some places and barely in others, so the border never forms a straight or square \
edge. 0 m: the region may touch the border. Large values shrink the buildable area \
(coverage is measured against it).";

/// Which layer the preview shows while a section is under development.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BenchView {
    /// The raw normalized noise field, before the threshold cut.
    Noise,
    /// The shape after cut → speckle removal → bridges → hole fill.
    Shape,
}

impl BenchView {
    pub fn label(self) -> &'static str {
        match self {
            BenchView::Noise => "noise texture",
            BenchView::Shape => "region shape",
        }
    }
}

/// Which section is being built right now; update as steps start and finish
/// (step list in docs/workflow.md).
pub fn current_step_label() -> &'static str {
    "step 1: noise & region shape (in development)"
}

/// The test-bench area, rendered inside the "Current workflow" panel below its
/// header. Experimental controls for the section under development go here and
/// render into the preview via Generate. When the section is done, its logic
/// moves into `src/generator`, its settings UI into the left panel, and this
/// bench is cleared for the next step (docs/workflow.md).
/// One grid row: underlined setting name (hover = explanation) + value editor
/// (same explanation on hover).
fn param_row(
    ui: &mut egui::Ui,
    name: &str,
    tip: &str,
    edit: impl FnOnce(&mut egui::Ui) -> egui::Response,
) {
    ui.label(egui::RichText::new(name).underline()).on_hover_text(tip);
    edit(ui).on_hover_text(tip);
    ui.end_row();
}

pub fn show_bench(ui: &mut egui::Ui, app: &mut MapGenApp) {
    ui.label(
        "Pipeline: seeded fBm noise sampled over the map → stretch to full contrast → \
         one cut (the level is picked so the region covers the target share of the \
         buildable area) → drop tiny pieces → bridge the pieces into one shape → fill \
         enclosed holes. The map kind (Template node) sets the character: a TOWN keeps \
         most of the map and cuts the rest away; swamp / forest / cave scatter islands \
         and bridge them. Walls, sections and roads come later.",
    );
    ui.weak("Hover an underlined setting to see what it does — including when it has NO effect.");

    egui::Grid::new("step1_shape_controls")
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
            param_row(ui, "Coverage (buildable area)", TIP_COVERAGE, |ui| {
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
    ui.weak("Changes apply on ⚙ Generate");

    ui.horizontal(|ui| {
        if ui
            .button("🎲 Random params")
            .on_hover_text(
                "Every value is picked inside its reasonable range, but the combination \
                 is deliberately unconstrained — parameters may contradict each other \
                 (e.g. huge coverage with a huge min-blob). Set values only; press \
                 Generate to see what the generator does with them.",
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
        ui.weak("reasonable values, possibly contradicting combination");
    });

    ui.horizontal(|ui| {
        ui.label("Preview:");
        ui.radio_value(&mut app.workflow_view, BenchView::Noise, BenchView::Noise.label());
        ui.radio_value(&mut app.workflow_view, BenchView::Shape, BenchView::Shape.label());
        match app.workflow_view {
            BenchView::Noise => ui.weak("(dark = deleted by the cut, light = kept)"),
            BenchView::Shape => ui.weak("(beige = region, dark = outside)"),
        }
    });

    if let Some(shape) = app.generated.as_ref().and_then(|l| l.shape.as_ref()) {
        let s = shape.stats;
        let pieces = if shape.mask.count_true() == 0 { 0 } else { 1 };
        ui.weak(format!(
            "{}×{} cells · coverage {:.1}% · pieces {} → {} (bridges {}, speckle cells removed {}, hole cells filled {})",
            shape.mask.width,
            shape.mask.height,
            s.coverage_pct,
            s.raw_blobs,
            pieces,
            s.bridges,
            s.speckle_cells_removed,
            s.hole_cells_filled,
        ));
    }
}
