use eframe::egui;
use rand::Rng;

use crate::level::GeneratedLevel;
use crate::model::{LevelConfig, Warning};
use crate::ui::{left_panel, preview, warnings, workflow};
use crate::validate::validate;

pub struct MapGenApp {
    pub config: LevelConfig,
    pub warnings: Vec<Warning>,
    /// Path used by the Save/Load buttons.
    pub config_path: String,
    pub config_status: Option<ConfigStatus>,
    /// The last generated level, rendered on the preview.
    pub generated: Option<GeneratedLevel>,
    /// Non-blocking notes from the last generation, merged into `warnings`.
    pub gen_warnings: Vec<Warning>,
    /// Output dir of the last export + status of the Export button.
    pub export_dir: String,
    /// Resolution of the exported PNGs in pixels per meter.
    pub export_px_per_m: f32,
    pub export_status: Option<ConfigStatus>,
    pub preview: preview::PreviewState,
    pub warnings_open: bool,
    /// Whether the workflow bottom panel is expanded.
    pub workflow_open: bool,
    /// What the preview shows for the section under development.
    pub workflow_view: workflow::BenchView,
    /// Grayscale noise field / region shape from the last generation, as GPU
    /// textures for the bench preview.
    pub bench_noise: Option<egui::TextureHandle>,
    pub bench_shape: Option<egui::TextureHandle>,
}

#[derive(Debug, Clone)]
pub struct ConfigStatus {
    pub ok: bool,
    pub text: String,
}

impl Default for MapGenApp {
    fn default() -> Self {
        Self {
            config: LevelConfig::default(),
            warnings: Vec::new(),
            config_path: "configs/config.json".to_owned(),
            config_status: None,
            generated: None,
            gen_warnings: Vec::new(),
            export_dir: "exports".to_owned(),
            export_px_per_m: 20.0,
            export_status: None,
            preview: preview::PreviewState::default(),
            warnings_open: false,
            workflow_open: true,
            workflow_view: workflow::BenchView::Shape,
            bench_noise: None,
            bench_shape: None,
        }
    }
}

impl MapGenApp {
    /// Run the generation engine with the current config and seed.
    pub fn generate_level(&mut self, ctx: &egui::Context) {
        let level = crate::generator::generate(&self.config, self.config.seed);
        self.gen_warnings = level
            .notes
            .iter()
            .map(|n| Warning {
                field: None,
                message: n.clone(),
                blocking: false,
            })
            .collect();

        // Bench textures: one pixel per grid cell, drawn crisp (NEAREST).
        let upload = |name: &str, pixels: Vec<egui::Color32>, w: usize, h: usize| {
            ctx.load_texture(
                name,
                egui::ColorImage::new([w, h], pixels),
                egui::TextureOptions::NEAREST,
            )
        };
        if let Some(noise) = &level.noise_preview {
            let pixels = noise
                .values
                .iter()
                .map(|v| egui::Color32::from_gray(*v))
                .collect();
            self.bench_noise = Some(upload(
                "bench_noise",
                pixels,
                noise.width,
                noise.height,
            ));
        }
        if let Some(shape) = &level.shape {
            // Preview-only colors: dark background so the region silhouette
            // pops, brown wall ring over it. (The exported map.png keeps its
            // paper colors.)
            let empty = egui::Color32::from_gray(38);
            let filled = egui::Color32::from_rgb(
                crate::render::REGION[0],
                crate::render::REGION[1],
                crate::render::REGION[2],
            );
            let wall = egui::Color32::from_rgb(
                crate::render::WALL[0],
                crate::render::WALL[1],
                crate::render::WALL[2],
            );
            let mut pixels: Vec<egui::Color32> = shape
                .mask
                .cells
                .iter()
                .map(|cell| if *cell { filled } else { empty })
                .collect();
            if let Some(walls) = &level.walls {
                for (px, is_wall) in pixels.iter_mut().zip(&walls.mask.cells) {
                    if *is_wall {
                        *px = wall;
                    }
                }
            }
            self.bench_shape = Some(upload(
                "bench_shape",
                pixels,
                shape.mask.width,
                shape.mask.height,
            ));
        }

        self.generated = Some(level);
    }
}

impl eframe::App for MapGenApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // The config is small: re-validate every frame so warnings stay current.
        self.warnings = validate(&self.config);
        self.warnings.extend(self.gen_warnings.iter().cloned());

        // Bottom panels: the first registered sits at the very bottom edge.
        self.show_workflow_panel(ctx);
        self.show_warnings_panel(ctx);
        self.show_action_bar(ctx);
        self.show_settings_panel(ctx);
        self.show_preview_panel(ctx);
    }
}

impl MapGenApp {
    /// Always-visible bar with the Generate button and seed control, so the user
    /// can regenerate at any time after changing values.
    fn show_action_bar(&mut self, ctx: &egui::Context) {
        let blocking = self.warnings.iter().filter(|w| w.blocking).count();
        let generated_info = self
            .generated
            .as_ref()
            .map(|l| format!("{} objects · seed {}", l.objects.len(), l.seed));

        egui::TopBottomPanel::top("action_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let generate = ui.add_enabled(
                    blocking == 0,
                    egui::Button::new(
                        egui::RichText::new("⚙  Generate").strong().size(16.0),
                    )
                    .min_size(egui::vec2(110.0, 28.0)),
                );
                if blocking == 0 {
                    if generate.clicked() {
                        self.generate_level(ctx);
                    }
                } else {
                    generate.on_disabled_hover_text(format!(
                        "Fix the {blocking} blocking warning(s) first (see the warnings panel below)"
                    ));
                }

                ui.separator();
                ui.label("Seed:");
                ui.add_sized(
                    [130.0, 20.0],
                    egui::DragValue::new(&mut self.config.seed).speed(1.0),
                );
                if ui
                    .button("🎲 Re-roll")
                    .on_hover_text("New random seed and regenerate")
                    .clicked()
                {
                    self.config.seed = rand::rng().random();
                    self.generate_level(ctx);
                }
                ui.separator();

                if blocking > 0 {
                    ui.label(
                        egui::RichText::new(format!(
                            "Generation blocked by {blocking} blocking warning(s)"
                        ))
                        .color(egui::Color32::from_rgb(255, 140, 140)),
                    );
                } else if let Some(info) = generated_info {
                    ui.weak(info);
                } else {
                    ui.weak("Same settings + same seed always produce the same map");
                }
            });
            ui.add_space(4.0);
        });
    }

    fn show_workflow_panel(&mut self, ctx: &egui::Context) {
        let panel = egui::TopBottomPanel::bottom("workflow_panel")
            .resizable(true)
            .default_height(170.0)
            .height_range(40.0..=400.0);
        let panel = if self.workflow_open {
            panel
        } else {
            panel.exact_height(24.0)
        };
        panel.show(ctx, |ui| {
            ui.horizontal(|ui| {
                let arrow = if self.workflow_open { "▼" } else { "▶" };
                if ui.small_button(arrow).clicked() {
                    self.workflow_open = !self.workflow_open;
                }
                ui.strong("Current workflow");
                ui.separator();
                ui.weak(workflow::current_step_label());
            });
            if self.workflow_open {
                ui.separator();
                workflow::show_bench(ui, self);
            }
        });
    }

    fn show_warnings_panel(&mut self, ctx: &egui::Context) {
        let total = self.warnings.len();
        let blocking = self.warnings.iter().filter(|w| w.blocking).count();
        let header = format!("Warnings: {total} total, {blocking} blocking");
        let open = self.warnings_open;

        let panel = egui::TopBottomPanel::bottom("warnings_panel");
        let panel = if open {
            panel.resizable(true).default_height(150.0).height_range(60.0..=420.0)
        } else {
            panel.exact_height(30.0)
        };

        panel.show(ctx, |ui| {
            ui.horizontal(|ui| {
                let arrow = if open { "▼" } else { "▶" };
                if ui.small_button(arrow).clicked() {
                    self.warnings_open = !self.warnings_open;
                }
                let color = if blocking > 0 {
                    egui::Color32::from_rgb(255, 140, 140)
                } else {
                    egui::Color32::from_rgb(150, 200, 150)
                };
                ui.label(egui::RichText::new(header).color(color));
            });
            if open {
                egui::ScrollArea::vertical().show(ui, |ui| warnings::show(ui, &self.warnings));
            }
        });
    }

    fn show_settings_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("settings_panel")
            .resizable(true)
            .width_range(320.0..=480.0)
            .default_width(380.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    left_panel::show(ui, self);
                });
            });
    }

    fn show_preview_panel(&mut self, ctx: &egui::Context) {
        let bench = match self.workflow_view {
            workflow::BenchView::Noise => self.bench_noise.as_ref(),
            workflow::BenchView::Shape => self.bench_shape.as_ref(),
        };
        egui::CentralPanel::default().show(ctx, |ui| {
            preview::show(
                ui,
                &mut self.preview,
                &self.config,
                self.generated.as_ref(),
                bench,
            );
        });
    }
}
