use eframe::egui;

use crate::ui::app::MapGenApp;

/// Which layer the preview shows while a section is under development.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BenchView {
    /// The raw normalized noise field, before the threshold cut.
    Noise,
    /// The region shape (wall ring, then region colors as they land).
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
    "no step in development"
}

/// The test-bench area, rendered inside the "Current workflow" panel below its
/// header. Experimental controls for the section under development go here and
/// render into the preview via Generate. When the section is done, its logic
/// moves into `src/generator`, its settings UI into the left panel, and this
/// bench is cleared for the next step (docs/workflow.md).
pub fn show_bench(ui: &mut egui::Ui, app: &mut MapGenApp) {
    ui.weak("Nothing in development — every finished step lives in its node on the left.");
    ui.horizontal(|ui| {
        ui.label("Preview:");
        ui.radio_value(&mut app.workflow_view, BenchView::Noise, BenchView::Noise.label());
        ui.radio_value(&mut app.workflow_view, BenchView::Shape, BenchView::Shape.label());
        match app.workflow_view {
            BenchView::Noise => ui.weak("(dark = deleted by the cut, light = kept)"),
            BenchView::Shape => ui.weak("(regions in color, cream = road, brown = wall)"),
        }
    });
}
