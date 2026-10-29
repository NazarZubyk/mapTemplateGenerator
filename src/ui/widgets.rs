//! Small shared UI helpers for the node panels and the workflow bench.

use eframe::egui;

/// One grid row: underlined setting name (hover = explanation) + value editor
/// (the same explanation on hover).
pub(crate) fn param_row(
    ui: &mut egui::Ui,
    name: &str,
    tip: &str,
    edit: impl FnOnce(&mut egui::Ui) -> egui::Response,
) {
    ui.label(egui::RichText::new(name).underline())
        .on_hover_text(tip);
    edit(ui).on_hover_text(tip);
    ui.end_row();
}
