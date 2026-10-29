use eframe::egui;

use crate::ui::app::MapGenApp;

/// Which section is being built right now; update as steps start and finish
/// (step list in docs/workflow.md).
pub fn current_step_label() -> &'static str {
    "step 0: shell (no section in development)"
}

/// The test-bench area, rendered inside the "Current workflow" panel below its
/// header. Experimental controls for the section under development go here and
/// render into the preview via Generate. When the section is done, its logic
/// moves into `src/generator`, its settings UI into the left panel, and this
/// bench is cleared for the next step (docs/workflow.md).
pub fn show_bench(ui: &mut egui::Ui, _app: &mut MapGenApp) {
    ui.weak(
        "Nothing in development right now. Build the next section here, test it \
         with Generate, then promote it to the settings panel. Process and step \
         list: docs/workflow.md",
    );
}
