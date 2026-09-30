//! Settings window: machine model, monitor ROM, ROM module, audio,
//! themes (with live palette customization and export), plus the
//! "restart machine?" confirmation dialog.

use crate::app::{App, MachineConfig};
use crate::ui::theme::Theme;
use egui_phosphor::regular as icon;
use pmd85_core::Model;

/// Width of the dropdown selectors. Deliberately fixed: sizing them
/// from `available_width()` while more widgets (labels, the file
/// buttons) follow in the same row makes the row wider than the
/// window, and since the window auto-sizes to its content it would
/// keep growing every frame until it ran off the screen.
const COMBO_WIDTH: f32 = 200.0;

pub fn draw(ctx: &egui::Context, app: &mut App) {
    draw_settings_window(ctx, app);
    draw_confirm_restart(ctx, app);
}

fn draw_settings_window(ctx: &egui::Context, app: &mut App) {
    let mut open = app.ui.settings_open;
    egui::Window::new("Settings")
        .open(&mut open)
        .default_width(470.0)
        .min_width(380.0)
        .show(ctx, |ui| {
            let theme = app.active_theme_data();
            let c = |field: &[u8; 4]| Theme::color(field);

            let mut cfg = app.pending_config.clone();

            // ---- machine ----
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("MACHINE")
                    .size(11.0)
                    .strong()
                    .color(c(&theme.accent)),
            );
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label("Model");
                ui.separator();
                for model in [
                    Model::Pmd851,
                    Model::Pmd852,
                    Model::Pmd852a,
                    Model::Pmd853,
                ] {
                    if ui
                        .selectable_label(cfg.model == model, model.name())
                        .on_hover_text(model_hint(model))
                        .clicked()
                    {
                        cfg.model = model;
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label("Monitor");
                let mut monitor_label = cfg
                    .monitor
                    .clone()
                    .unwrap_or_else(|| default_monitor_label(cfg.model));
                egui::ComboBox::from_id_salt("monitor")
                    .selected_text(&monitor_label)
                    .width(COMBO_WIDTH)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut monitor_label, default_monitor_label(cfg.model), default_monitor_label(cfg.model));
                        for entry in app.catalog.monitors() {
                            let name = format!("{} ({})", entry.file_name, entry.size_label());
                            ui.selectable_value(&mut monitor_label, name.clone(), name);
                        }
                    });
                cfg.monitor = if monitor_label == default_monitor_label(cfg.model) {
                    None
                } else {
                    // Strip the size suffix back to the file name.
                    Some(monitor_label.split(" (").next().unwrap_or(&monitor_label).to_string())
                };
                if ui
                    .button(format!("{} file\u{2026}", icon::FLOPPY_DISK))
                    .on_hover_text("Pick a monitor ROM file")
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Monitor ROM", &["rom"])
                        .pick_file()
                    {
                        cfg.monitor = Some(path.display().to_string());
                    }
                }
            });

            // ---- ROM module ----
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("ROM MODULE")
                    .size(11.0)
                    .strong()
                    .color(c(&theme.accent)),
            );
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label("Module");
                let mut module_label = cfg
                    .rom_module
                    .clone()
                    .unwrap_or_else(|| "\u{2014} none \u{2014}".to_string());
                egui::ComboBox::from_id_salt("rom-module")
                    .selected_text(&module_label)
                    .width(COMBO_WIDTH)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut module_label,
                            "\u{2014} none \u{2014}".to_string(),
                            "\u{2014} none \u{2014}",
                        );
                        for entry in app.catalog.modules() {
                            let name = format!("{} ({})", entry.file_name, entry.size_label());
                            ui.selectable_value(&mut module_label, name.clone(), name);
                        }
                    });
                cfg.rom_module = if module_label == "\u{2014} none \u{2014}" {
                    None
                } else {
                    Some(module_label.split(" (").next().unwrap_or(&module_label).to_string())
                };
                if ui
                    .button(format!("{} file\u{2026}", icon::FLOPPY_DISK))
                    .on_hover_text("Pick a ROM module (.rmm) file")
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ROM module", &["rmm"])
                        .pick_file()
                    {
                        cfg.rom_module = Some(path.display().to_string());
                    }
                }
            });
            // Description of the selected module (from rom-list.txt).
            if let Some(name) = &cfg.rom_module {
                if let Some(entry) = app.catalog.find(name) {
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(&entry.description)
                            .size(11.0)
                            .color(c(&theme.text_weak))
                            .weak(),
                    );
                }
            }

            // ---- restart required? ----
            if cfg != app.config {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(icon::WARNING_CIRCLE)
                            .color(c(&theme.warn))
                            .size(13.0),
                    )
                    .on_hover_text("Changing the machine requires a restart");
                    ui.label(
                        egui::RichText::new("Machine configuration changed \u{2014} restart required")
                            .color(c(&theme.warn))
                            .size(11.0),
                    );
                    if ui
                        .button(egui::RichText::new("Restart machine").strong())
                        .clicked()
                    {
                        request_restart(app, cfg.clone());
                    }
                });
            }
            app.pending_config = cfg;

            ui.separator();

            // ---- audio ----
            let mute_before = app.settings.mute;
            ui.checkbox(&mut app.settings.mute, "Mute speaker");
            if app.settings.mute != mute_before {
                app.set_mute(app.settings.mute);
            }

            ui.separator();

            // ---- theme ----
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("THEME")
                    .size(11.0)
                    .strong()
                    .color(c(&theme.accent)),
            );
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label("Theme");
                let selected = app
                    .themes
                    .iter()
                    .position(|t| t.name == app.active_theme)
                    .unwrap_or(0);
                let mut names: Vec<String> = app.themes.iter().map(|t| t.name.clone()).collect();
                if names.is_empty() {
                    names.push(DEFAULT.into());
                }
                let mut choice = selected.min(names.len() - 1);
                egui::ComboBox::from_id_salt("theme")
                    .selected_text(&names[choice])
                    .width(COMBO_WIDTH)
                    .show_ui(ui, |ui| {
                        for (i, name) in names.iter().enumerate() {
                            ui.selectable_value(&mut choice, i, name);
                        }
                    });
                if choice != selected {
                    app.set_theme(choice);
                }
            });

            let mut customize = app.ui.theme_customize;
            ui.checkbox(&mut customize, "Customize colors");
            app.ui.theme_customize = customize;
            if customize {
                // Work on a local copy so the rest of the window can
                // still borrow `app`; stored back afterwards.
                let mut draft: Theme = app
                    .ui
                    .theme_draft
                    .take()
                    .unwrap_or_else(|| app.active_theme_data());
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut draft.name);
                });
                egui::Grid::new("theme-colors")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        color_row(ui, "Background", &mut draft.bg);
                        color_row(ui, "Panel", &mut draft.panel);
                        color_row(ui, "Panel border", &mut draft.panel_border);
                        color_row(ui, "Widget", &mut draft.widget);
                        color_row(ui, "Widget hover", &mut draft.widget_hover);
                        color_row(ui, "Widget active", &mut draft.widget_active);
                        color_row(ui, "Text", &mut draft.text);
                        color_row(ui, "Dim text", &mut draft.text_weak);
                        color_row(ui, "Accent", &mut draft.accent);
                        color_row(ui, "Accent dim", &mut draft.accent_dim);
                        color_row(ui, "Warning", &mut draft.warn);
                        color_row(ui, "Danger", &mut draft.danger);
                        color_row(ui, "Screen bezel", &mut draft.screen_bezel);
                    });
                ui.add_space(4.0);
                let mut apply = false;
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        apply = true;
                    }
                    if ui.button("Save theme\u{2026}").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name(format!("{}.json", draft.name))
                            .add_filter("Theme", &["json"])
                            .save_file()
                        {
                            let json = draft.to_json();
                            if let Err(e) = std::fs::write(&path, json) {
                                app.notify(format!(
                                    "cannot save theme {}: {e}",
                                    path.display()
                                ));
                            } else {
                                app.notify(format!("theme saved to {}", path.display()));
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new("saved themes load from the themes directory")
                                .size(10.0)
                                .color(c(&theme.text_weak)),
                        );
                    });
                });
                if apply {
                    app.apply_theme(draft.clone());
                }
                app.ui.theme_draft = Some(draft);
            }
        });
    if open != app.ui.settings_open {
        app.ui.settings_open = open;
    }
    if !app.ui.settings_open {
        // Closing the window discards unapplied machine edits.
        app.pending_config = app.config.clone();
        app.ui.theme_customize = false;
    }
}

const DEFAULT: &str = "Brigadier";

fn draw_confirm_restart(ctx: &egui::Context, app: &mut App) {
    let Some(cfg) = app.ui.confirm_reboot.clone() else {
        return;
    };
    let theme = app.active_theme_data();
    let c = |field: &[u8; 4]| Theme::color(field);
    egui::Window::new("Restart machine?")
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(c(&theme.panel))
                .stroke(egui::Stroke::new(1.0, c(&theme.warn)))
                .inner_margin(14),
        )
        .show(ctx, |ui| {
            ui.set_width(320.0);
            ui.label("The machine will be restarted with the new");
            ui.label("configuration. The current program state");
            ui.label("will be lost.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .button(egui::RichText::new("Restart").strong())
                    .clicked()
                {
                    app.ui.confirm_reboot = None;
                    app.apply_config(cfg);
                }
                if ui.button("Cancel").clicked() {
                    app.ui.confirm_reboot = None;
                }
            });
        });
}

/// Ask for a restart: immediately when the machine has not run yet,
/// with a confirmation dialog otherwise.
fn request_restart(app: &mut App, cfg: MachineConfig) {
    if app.frames_done == 0 && app.machine.bus.total_cycles() == 0 {
        app.apply_config(cfg);
    } else {
        app.ui.confirm_reboot = Some(cfg);
    }
}

fn default_monitor_label(model: Model) -> String {
    format!("default ({})", model.default_monitor())
}

fn model_hint(model: Model) -> &'static str {
    match model {
        Model::Pmd851 => "Tesla PMD 85-1 (1984): 48 KiB RAM, 4 KiB monitor",
        Model::Pmd852 => "Tesla PMD 85-2 (1986): 48 KiB RAM, rewritten monitor",
        Model::Pmd852a => "Tesla PMD 85-2A (1987): 64 KiB RAM, extra pages",
        Model::Pmd853 => "Tesla PMD 85-3 (1988): 64 KiB RAM, ROM/VRAM banking",
    }
}

/// One color field row in the theme editor.
fn color_row(ui: &mut egui::Ui, label: &str, field: &mut [u8; 4]) {
    ui.label(egui::RichText::new(label).size(11.0));
    let mut color = Theme::color(field);
    ui.color_edit_button_srgba(&mut color);
    let [r, g, b, a] = color.to_array();
    *field = [r, g, b, a];
    ui.end_row();
}
