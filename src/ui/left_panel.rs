use eframe::egui;

use crate::model::{
    ColorRgb, FieldId, Gate, InteractiveType, LevelConfig, MobType, SizeRange, TemplateKind,
    TemplateMix, Warning, random_default_color, renormalize_balance,
};
use crate::ui::app::{ConfigStatus, MapGenApp};

pub fn show(ui: &mut egui::Ui, app: &mut MapGenApp) {
    // Cloned once per frame: the generate section needs the whole app while the
    // others only need disjoint config fields.
    let warnings = app.warnings.clone();

    template_section(ui, &mut app.config, &warnings);
    map_section(ui, &mut app.config, &warnings);
    structures_section(ui, &mut app.config, &warnings);
    props_section(ui, &mut app.config, &warnings);
    interactive_section(ui, &mut app.config, &warnings);
    hidden_rooms_section(ui, &mut app.config, &warnings);
    mobs_section(ui, &mut app.config, &warnings);
    gates_section(ui, &mut app.config, &warnings);
    export_section(ui, app);
    config_file_section(
        ui,
        &mut app.config,
        &mut app.config_path,
        &mut app.config_status,
    );
}

// ---------------------------------------------------------------- helpers

/// ⚠ marker shown next to every field that has a warning; red when the
/// warning blocks generation, yellow otherwise. Tooltip carries the messages.
pub fn warn_mark(ui: &mut egui::Ui, warnings: &[Warning], field: FieldId) {
    let messages: Vec<&str> = warnings
        .iter()
        .filter(|w| w.field == Some(field))
        .map(|w| w.message.as_str())
        .collect();
    if messages.is_empty() {
        return;
    }
    let blocking = warnings
        .iter()
        .any(|w| w.field == Some(field) && w.blocking);
    let color = if blocking {
        egui::Color32::RED
    } else {
        egui::Color32::YELLOW
    };
    ui.add_sized([20.0, 18.0], egui::Button::new(egui::RichText::new("⚠").color(color)))
        .on_hover_text(messages.join("\n"));
}

fn field_row<R>(
    ui: &mut egui::Ui,
    label: &str,
    warnings: &[Warning],
    field: FieldId,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.horizontal(|ui| {
        ui.label(label);
        let response = content(ui);
        warn_mark(ui, warnings, field);
        response
    })
    .inner
}

fn section<R>(
    ui: &mut egui::Ui,
    title: &str,
    body: impl FnOnce(&mut egui::Ui) -> R,
) {
    egui::CollapsingHeader::new(egui::RichText::new(title).strong())
        .default_open(true)
        .show(ui, body);
}

fn size_range_row(
    ui: &mut egui::Ui,
    label: &str,
    range: &mut SizeRange,
    warnings: &[Warning],
    field: FieldId,
    min_speed: f32,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(
            egui::DragValue::new(&mut range.min_m)
                .speed(min_speed)
                .suffix(" m"),
        );
        ui.label("–");
        ui.add(
            egui::DragValue::new(&mut range.max_m)
                .speed(min_speed)
                .suffix(" m"),
        );
        warn_mark(ui, warnings, field);
    });
}

fn color_row(ui: &mut egui::Ui, label: &str, color: &mut ColorRgb, warnings: &[Warning], field: FieldId) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.color_edit_button_srgb(color);
        warn_mark(ui, warnings, field);
    });
}

fn display_name(name: &str, index: usize) -> String {
    if name.is_empty() {
        format!("#{}", index + 1)
    } else {
        name.to_owned()
    }
}

// ---------------------------------------------------------------- sections

fn template_section(ui: &mut egui::Ui, config: &mut LevelConfig, warnings: &[Warning]) {
    section(ui, "Template", |ui| {
        let is_mix = config.template_mix.is_mix();
        ui.horizontal(|ui| {
            if ui.radio(!is_mix, "Single template").clicked() && is_mix {
                let first = config.template_mix.kinds()[0];
                config.template_mix = TemplateMix::Single(first);
            }
            if ui.radio(is_mix, "Mix of two").clicked() && !is_mix {
                let first = config.template_mix.kinds()[0];
                let second = TemplateKind::ALL
                    .into_iter()
                    .find(|k| *k != first)
                    .unwrap_or(TemplateKind::Forest);
                config.template_mix = TemplateMix::Mix(first, second);
            }
            warn_mark(ui, warnings, FieldId::TemplateMix);
        });

        match &mut config.template_mix {
            TemplateMix::Single(kind) => template_combo(ui, "Template", kind),
            TemplateMix::Mix(first, second) => {
                template_combo(ui, "First", first);
                template_combo(ui, "Second", second);
            }
        }
        ui.weak("Each template brings its own structure type, density and size rules.");
    });
}

fn template_combo(ui: &mut egui::Ui, label: &str, kind: &mut TemplateKind) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(format!("template_combo_{label}"))
            .selected_text(kind.to_string())
            .show_ui(ui, |ui| {
                for k in TemplateKind::ALL {
                    ui.selectable_value(kind, k, k.to_string());
                }
            });
    });
}

fn map_section(ui: &mut egui::Ui, config: &mut LevelConfig, warnings: &[Warning]) {
    section(ui, "Map", |ui| {
        field_row(ui, "Width", warnings, FieldId::MapWidth, |ui| {
            ui.add(
                egui::DragValue::new(&mut config.map.width_m)
                    .speed(1.0)
                    .range(1.0..=2000.0)
                    .suffix(" m"),
            )
        });
        field_row(ui, "Height", warnings, FieldId::MapHeight, |ui| {
            ui.add(
                egui::DragValue::new(&mut config.map.height_m)
                    .speed(1.0)
                    .range(1.0..=2000.0)
                    .suffix(" m"),
            )
        });
        field_row(ui, "Walkable", warnings, FieldId::WalkablePct, |ui| {
            ui.add(
                egui::Slider::new(&mut config.map.walkable_pct, 1.0..=99.0)
                    .suffix(" %")
                    .text("of total area"),
            )
        });
    });
}

fn structures_section(ui: &mut egui::Ui, config: &mut LevelConfig, warnings: &[Warning]) {
    section(ui, "Structures", |ui| {
        for kind in config.template_mix.kinds() {
            ui.strong(format!("{kind} structures"));
            let s = config.structure_mut(kind);
            field_row(
                ui,
                "Density",
                warnings,
                FieldId::StructureDensity(kind),
                |ui| ui.add(egui::Slider::new(&mut s.density_pct, 0.0..=100.0).suffix(" %")),
            );
            size_range_row(
                ui,
                "Size",
                &mut s.size,
                warnings,
                FieldId::StructureSize(kind),
                0.1,
            );
            color_row(
                ui,
                "Color",
                &mut s.color,
                warnings,
                FieldId::StructureColor(kind),
            );
            ui.add_space(4.0);
        }
    });
}

fn props_section(ui: &mut egui::Ui, config: &mut LevelConfig, warnings: &[Warning]) {
    section(ui, "Props (uninteractive)", |ui| {
        field_row(ui, "Density", warnings, FieldId::PropsDensity, |ui| {
            ui.add(egui::Slider::new(&mut config.props.density_pct, 0.0..=100.0).suffix(" %"))
        });
        size_range_row(
            ui,
            "Size",
            &mut config.props.size,
            warnings,
            FieldId::PropsSize,
            0.05,
        );
        field_row(ui, "Quantity", warnings, FieldId::PropsQuantity, |ui| {
            ui.add(
                egui::DragValue::new(&mut config.props.quantity)
                    .speed(1.0)
                    .range(0..=1_000_000),
            )
        });
        color_row(ui, "Color", &mut config.props.color, warnings, FieldId::PropsColor);
    });
}

fn interactive_section(ui: &mut egui::Ui, config: &mut LevelConfig, warnings: &[Warning]) {
    section(ui, "Interactive objects", |ui| {
        let mut remove: Option<usize> = None;
        for i in 0..config.interactive.len() {
            let title = format!("Type: {}", display_name(&config.interactive[i].name, i));
            let it = &mut config.interactive[i];
            egui::CollapsingHeader::new(egui::RichText::new(title))
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.add(egui::TextEdit::singleline(&mut it.name).desired_width(160.0));
                        if ui.small_button("Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                    field_row(ui, "Size", warnings, FieldId::InteractiveSize(i), |ui| {
                        ui.add(
                            egui::DragValue::new(&mut it.size_m)
                                .speed(0.05)
                                .range(0.01..=100.0)
                                .suffix(" m"),
                        )
                    });
                    field_row(ui, "Quantity", warnings, FieldId::InteractiveQuantity(i), |ui| {
                        ui.add(
                            egui::DragValue::new(&mut it.quantity)
                                .speed(1.0)
                                .range(0..=1_000_000),
                        )
                    });
                    size_range_row(
                        ui,
                        "Group",
                        &mut it.group,
                        warnings,
                        FieldId::InteractiveGroup(i),
                        0.1,
                    );
                    ui.weak("Group = how many identical objects may sit in one place");
                    color_row(ui, "Color", &mut it.color, warnings, FieldId::InteractiveColor(i));
                });
        }

        ui.horizontal(|ui| {
            if ui.button("+ Add object type").clicked() {
                let color = random_default_color(&mut rand::rng());
                let n = config.interactive.len() + 1;
                config.interactive.push(InteractiveType {
                    name: format!("Object {n}"),
                    size_m: 0.5,
                    quantity: 20,
                    group: SizeRange::new(1.0, 3.0),
                    color,
                });
            }
            ui.weak(format!("{} type(s)", config.interactive.len()));
        });

        if let Some(i) = remove {
            config.interactive.remove(i);
        }
    });
}

fn hidden_rooms_section(ui: &mut egui::Ui, config: &mut LevelConfig, warnings: &[Warning]) {
    section(ui, "Hidden rooms", |ui| {
        field_row(ui, "Count", warnings, FieldId::HiddenRoomCount, |ui| {
            ui.add(
                egui::DragValue::new(&mut config.hidden_rooms.count)
                    .speed(1.0)
                    .range(0..=1000),
            )
        });
        size_range_row(
            ui,
            "Size",
            &mut config.hidden_rooms.size,
            warnings,
            FieldId::HiddenRoomSize,
            0.1,
        );
        color_row(
            ui,
            "Room color",
            &mut config.hidden_rooms.color_room,
            warnings,
            FieldId::HiddenRoomColor,
        );
        color_row(
            ui,
            "Path color",
            &mut config.hidden_rooms.color_path,
            warnings,
            FieldId::HiddenRoomPathColor,
        );
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut config.hidden_rooms.mob_passable,
                "Access paths passable for mobs",
            )
            .on_hover_text(
                "Connections are always passable for the player. \
                 Tick to also let mobs use them.",
            );
        });
        ui.weak("Each hidden room gets one small access path to the main area");
    });
}

fn mobs_section(ui: &mut egui::Ui, config: &mut LevelConfig, warnings: &[Warning]) {
    section(ui, "Mobs", |ui| {
        field_row(ui, "Budget", warnings, FieldId::MobBudget, |ui| {
            ui.add(
                egui::DragValue::new(&mut config.mobs.budget)
                    .speed(10.0)
                    .range(0.0..=1_000_000.0),
            )
        });
        field_row(ui, "Max entities", warnings, FieldId::MobMaxEntities, |ui| {
            ui.add(
                egui::DragValue::new(&mut config.mobs.max_entities)
                    .speed(1.0)
                    .range(0..=1_000_000),
            )
        });

        mob_types_section(ui, config, warnings);
        combinations_section(ui, config);
        balance_section(ui, config, warnings);
    });
}

fn mob_types_section(ui: &mut egui::Ui, config: &mut LevelConfig, warnings: &[Warning]) {
    ui.strong("Mob types");
    let mut remove: Option<usize> = None;
    for i in 0..config.mobs.types.len() {
        let title = format!("Mob: {}", display_name(&config.mobs.types[i].name, i));
        let mob = &mut config.mobs.types[i];
        egui::CollapsingHeader::new(egui::RichText::new(title))
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.add(egui::TextEdit::singleline(&mut mob.name).desired_width(160.0));
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                });
                field_row(ui, "Price", warnings, FieldId::MobTypePrice(i), |ui| {
                    ui.add(
                        egui::DragValue::new(&mut mob.price)
                            .speed(1.0)
                            .range(0.0..=1_000_000.0),
                    )
                });
                field_row(ui, "Size", warnings, FieldId::MobTypeSize(i), |ui| {
                    ui.add(
                        egui::DragValue::new(&mut mob.size_m)
                            .speed(0.05)
                            .range(0.01..=100.0)
                            .suffix(" m"),
                    )
                });
                color_row(ui, "Color", &mut mob.color, warnings, FieldId::MobTypeColor(i));
            });
    }

    ui.horizontal(|ui| {
        if ui.button("+ Add mob type").clicked() {
            let color = random_default_color(&mut rand::rng());
            let n = config.mobs.types.len() + 1;
            config.mobs.add_type(MobType {
                name: format!("Mob {n}"),
                price: 10.0,
                size_m: 1.0,
                color,
            });
        }
        ui.weak(format!(
            "{} type(s), budget spent greedily up to the entity cap",
            config.mobs.types.len()
        ));
    });

    if let Some(i) = remove {
        config.mobs.remove_type(i);
    }
}

fn combinations_section(ui: &mut egui::Ui, config: &mut LevelConfig) {
    ui.separator();
    ui.strong("Combinations");
    ui.weak("Check = the two types may appear together at one spawn point");
    let n = config.mobs.types.len();
    if n < 2 {
        ui.weak("Add a second mob type to configure combinations.");
        return;
    }

    let names: Vec<String> = config
        .mobs
        .types
        .iter()
        .enumerate()
        .map(|(i, t)| display_name(&t.name, i))
        .collect();

    egui::Grid::new("mob_combinations")
        .striped(true)
        .min_col_width(18.0)
        .show(ui, |ui| {
            ui.weak("");
            for name in &names {
                ui.label(egui::RichText::new(name.clone()).small());
            }
            ui.end_row();
            for (i, name) in names.iter().enumerate() {
                ui.label(egui::RichText::new(name.clone()).small());
                for j in 0..n {
                    if i == j || j < i {
                        ui.weak("·");
                    } else {
                        let allowed = &mut config.mobs.combinations[i][j];
                        ui.checkbox(allowed, "");
                    }
                }
                ui.end_row();
            }
        });
}

fn balance_section(ui: &mut egui::Ui, config: &mut LevelConfig, warnings: &[Warning]) {
    ui.separator();
    ui.strong("Count balance");
    ui.weak("Raising one slider lowers the others; values are % and sum to 100");
    for i in 0..config.mobs.types.len() {
        let name = display_name(&config.mobs.types[i].name, i);
        ui.horizontal(|ui| {
            ui.add_sized([90.0, 18.0], egui::Label::new(name));
            let mut weight = config.mobs.balance.get(i).copied().unwrap_or(0.0);
            let response = ui.add(
                egui::Slider::new(&mut weight, 0.0..=100.0)
                    .show_value(true)
                    .smart_aim(false),
            );
            warn_mark(ui, warnings, FieldId::MobBalance);
            if response.changed() {
                config.mobs.balance = renormalize_balance(&config.mobs.balance, i, weight);
            }
        });
    }
}

fn gates_section(ui: &mut egui::Ui, config: &mut LevelConfig, warnings: &[Warning]) {
    section(ui, "Gates", |ui| {
        ui.weak("Direction in degrees, measured from the center of the map");

        let mut remove: Option<usize> = None;
        for i in 0..config.gates.len() {
            let gate = &mut config.gates[i];
            ui.horizontal(|ui| {
                ui.weak(format!("#{}", i + 1));
                ui.add(
                    egui::DragValue::new(&mut gate.direction_deg)
                        .speed(1.0)
                        .range(0.0..=360.0)
                        .suffix("°"),
                );
                ui.color_edit_button_srgb(&mut gate.color);
                warn_mark(ui, warnings, FieldId::GateDirection(i));
                warn_mark(ui, warnings, FieldId::GateColor(i));
                if ui.small_button("Remove").clicked() {
                    remove = Some(i);
                }
            });
        }

        if ui.button("+ Add gate").clicked() {
            config.gates.push(Gate {
                direction_deg: 0.0,
                color: random_default_color(&mut rand::rng()),
            });
        }
        if let Some(i) = remove {
            config.gates.remove(i);
        }
    });
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
                    Ok(mut loaded) => {
                        loaded.normalize();
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
