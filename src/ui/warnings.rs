use eframe::egui;

use crate::model::Warning;

/// Dedicated area listing every warning; blocking ones are highlighted red.
pub fn show(ui: &mut egui::Ui, warnings: &[Warning]) {
    if warnings.is_empty() {
        ui.colored_label(
            egui::Color32::from_rgb(140, 210, 140),
            "No problems found — the configuration is valid.",
        );
        return;
    }

    for w in warnings {
        ui.horizontal_wrapped(|ui| {
            let (icon, text) = if w.blocking {
                (
                    egui::Color32::RED,
                    egui::Color32::from_rgb(255, 135, 135),
                )
            } else {
                (
                    egui::Color32::YELLOW,
                    egui::Color32::from_rgb(235, 218, 130),
                )
            };
            ui.label(egui::RichText::new("⚠").color(icon));
            let mut line = w.message.clone();
            if let Some(field) = w.field {
                line.push_str(&format!("   [{}]", field.label()));
            }
            if w.blocking {
                line.push_str("   (blocks generation)");
            }
            ui.label(egui::RichText::new(line).color(text));
        });
    }
}
