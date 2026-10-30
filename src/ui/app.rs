use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

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
    /// The generation running on a background thread, if any.
    pub job: Option<GenJob>,
    /// Live sync: reload the config file when it changes on disk (e.g. edited
    /// by the MCP server, `mapgen-mcp`) and regenerate.
    pub live_sync: bool,
    /// Modification time of the config file when last checked.
    pub config_mtime: Option<SystemTime>,
    pub last_sync_check: Instant,
    /// A reload arrived while a generation was running: generate again after.
    pub regen_pending: bool,
}

/// How often live sync looks at the config file.
const SYNC_EVERY: Duration = Duration::from_millis(500);

/// A generation running on a background thread.
pub struct GenJob {
    /// Latest (stage label, overall fraction) reported by the generator.
    pub progress: Arc<Mutex<(&'static str, f32)>>,
    pub result: Receiver<GeneratedLevel>,
    pub started: Instant,
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
            job: None,
            live_sync: true,
            config_mtime: None,
            last_sync_check: Instant::now() - SYNC_EVERY,
            regen_pending: false,
        }
    }
}

impl MapGenApp {
    /// Start the generation engine with the current config and seed on a
    /// background thread; the UI keeps running and shows its progress. Does
    /// nothing while a generation is already running.
    pub fn generate_level(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            return;
        }
        let progress = Arc::new(Mutex::new(("Starting", 0.0f32)));
        let (tx, rx) = mpsc::channel();
        let config = self.config.clone();
        let shared = Arc::clone(&progress);
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let report = |label: &'static str, fraction: f32| {
                if let Ok(mut p) = shared.lock() {
                    *p = (label, fraction);
                }
                repaint.request_repaint();
            };
            let level = crate::generator::generate_with_progress(&config, config.seed, &report);
            // The receiver is gone only if the app closed; nothing to do then.
            let _ = tx.send(level);
            repaint.request_repaint();
        });
        self.job = Some(GenJob {
            progress,
            result: rx,
            started: Instant::now(),
        });
    }

    /// Place only the mobs again on the level shown, with the current mob
    /// settings; with `reroll`, a new random placement first. Instant (no
    /// background job); the rest of the map stays.
    pub fn redo_mobs(&mut self, ctx: &egui::Context, reroll: bool) {
        if reroll {
            self.config.mobs.reroll = rand::random::<u64>();
        }
        if self.job.is_some() {
            return;
        }
        let Some(level) = self.generated.as_ref() else { return };
        let mut level = level.clone();
        level.config.mobs = self.config.mobs.clone();
        crate::generator::place_mobs(&mut level, &|_, _| {});
        self.finish_level(ctx, level);
    }

    /// Pick up a finished background generation, if there is one.
    fn poll_job(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.job else { return };
        match job.result.try_recv() {
            Ok(level) => {
                self.job = None;
                self.finish_level(ctx, level);
                if std::mem::take(&mut self.regen_pending) {
                    self.generate_level(ctx);
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                // The worker panicked: report it instead of spinning forever.
                self.job = None;
                self.gen_warnings = vec![Warning {
                    field: None,
                    message: "Generation crashed — try other settings or another seed".to_owned(),
                    blocking: false,
                }];
            }
        }
    }

    /// Live sync: when the config file changed on disk and now differs from
    /// the config in the app, load it and regenerate. The app's own Save
    /// writes the same config, so it never triggers a reload.
    fn sync_config_file(&mut self, ctx: &egui::Context) {
        if !self.live_sync {
            return;
        }
        ctx.request_repaint_after(SYNC_EVERY);
        if self.last_sync_check.elapsed() < SYNC_EVERY {
            return;
        }
        self.last_sync_check = Instant::now();
        let Ok(mtime) = std::fs::metadata(&self.config_path).and_then(|m| m.modified()) else {
            return;
        };
        if self.config_mtime == Some(mtime) {
            return;
        }
        self.config_mtime = Some(mtime);
        let loaded = std::fs::read_to_string(&self.config_path)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str::<LevelConfig>(&text).map_err(|e| e.to_string()));
        match loaded {
            Ok(config) if config != self.config => {
                self.config = config;
                self.config_status = Some(ConfigStatus {
                    ok: true,
                    text: format!("Reloaded {} (changed on disk)", self.config_path),
                });
                if !validate(&self.config).iter().any(|w| w.blocking) {
                    if self.job.is_some() {
                        self.regen_pending = true;
                    } else {
                        self.generate_level(ctx);
                    }
                }
            }
            Ok(_) => {}
            // Half-written files show up mid-save; the next change retries.
            Err(e) => {
                self.config_status = Some(ConfigStatus { ok: false, text: format!("Live sync: {e}") });
            }
        }
    }

    /// Show a finished level: notes, bench textures, preview.
    fn finish_level(&mut self, ctx: &egui::Context, level: GeneratedLevel) {
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
        // A level without a layer (a swamp has no noise view or town shape)
        // must not keep showing the previous level's.
        self.bench_noise = None;
        self.bench_shape = None;
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
            if let Some(regions) = &level.regions {
                // Each region paints in its palette color over the beige base.
                for (px, id) in pixels.iter_mut().zip(&regions.map.ids) {
                    if *id > 0 {
                        let c = crate::render::region_color(*id);
                        *px = egui::Color32::from_rgb(c[0], c[1], c[2]);
                    }
                }
            }
            if let Some(roads) = &level.roads {
                let c = |rgb: [u8; 3]| egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                for (px, on) in pixels.iter_mut().zip(&roads.mask.cells) {
                    if *on {
                        *px = c(crate::render::ROAD);
                    }
                }
            }
            if let Some(region_walls) = &level.region_walls {
                let c = crate::render::REGION_WALL;
                for (px, on) in pixels.iter_mut().zip(&region_walls.mask.cells) {
                    if *on {
                        *px = egui::Color32::from_rgb(c[0], c[1], c[2]);
                    }
                }
            }
            if let Some(walls) = &level.walls {
                for (px, is_wall) in pixels.iter_mut().zip(&walls.mask.cells) {
                    if *is_wall {
                        *px = wall;
                    }
                }
            }
            if let Some(rooms) = &level.hidden_rooms {
                for (px, on) in pixels.iter_mut().zip(&rooms.wall_mask.cells) {
                    if *on {
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
        } else if let Some(islands) = level.swamp.as_ref().and_then(|s| s.islands.as_ref()) {
            // Swamp: islands on the dark preview background.
            let c = level.config.swamp.islands.color;
            let land = egui::Color32::from_rgb(c[0], c[1], c[2]);
            let water = egui::Color32::from_gray(38);
            let mut pixels: Vec<egui::Color32> = islands.ids.iter().map(|&id| if id > 0 { land } else { water }).collect();
            if let Some(depth) = level.swamp.as_ref().and_then(|s| s.depth.as_ref()) {
                let d = &level.config.swamp.depth;
                for (mask, c) in [(&depth.deep, d.deep_color), (&depth.mud, d.mud_color), (&depth.shallow, d.shallow_color), (&depth.thicket, d.thicket_color)] {
                    for (px, &on) in pixels.iter_mut().zip(&mask.cells) {
                        if on {
                            *px = egui::Color32::from_rgb(c[0], c[1], c[2]);
                        }
                    }
                }
            }
            if let Some(paths) = level.swamp.as_ref().and_then(|s| s.paths.as_ref()) {
                let p = &level.config.swamp.paths;
                for (mask, c) in [(&paths.mud, p.mud_color), (&paths.boardwalk, p.boardwalk_color)] {
                    for (px, &on) in pixels.iter_mut().zip(&mask.cells) {
                        if on {
                            *px = egui::Color32::from_rgb(c[0], c[1], c[2]);
                        }
                    }
                }
            }
            self.bench_shape = Some(upload("bench_shape", pixels, islands.width, islands.height));
        }

        self.generated = Some(level);
    }
}

impl eframe::App for MapGenApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_job(ctx);
        self.sync_config_file(ctx);
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
                let running = self.job.is_some();
                let generate = ui.add_enabled(
                    blocking == 0 && !running,
                    egui::Button::new(
                        egui::RichText::new("⚙  Generate").strong().size(16.0),
                    )
                    .min_size(egui::vec2(110.0, 28.0)),
                );
                if running {
                    generate.on_disabled_hover_text("Generation is running…");
                } else if blocking == 0 {
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
                    .add_enabled(!running && blocking == 0, egui::Button::new("🎲 Re-roll"))
                    .on_hover_text("New random seed and regenerate")
                    .clicked()
                {
                    self.config.seed = rand::rng().random();
                    self.generate_level(ctx);
                }
                ui.separator();

                if let Some(job) = &self.job {
                    let (label, fraction) = job.progress.lock().map(|p| *p).unwrap_or(("…", 0.0));
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .desired_width(260.0)
                            .animate(true)
                            .text(format!("{label} · {:.0}%", fraction * 100.0)),
                    );
                    ui.weak(format!("{:.1} s", job.started.elapsed().as_secs_f32()));
                } else if blocking > 0 {
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
