// No console window behind the app on Windows (the MCP server keeps its console).
#![cfg_attr(windows, windows_subsystem = "windows")]

use map_template_generator::ui::app::MapGenApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1440.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native(
        "MapTemplateGenerator",
        options,
        Box::new(|_cc| Ok(Box::new(MapGenApp::default()))),
    )
}
