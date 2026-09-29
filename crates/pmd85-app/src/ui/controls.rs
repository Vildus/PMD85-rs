//! Transport bar: run/pause/stop/reset, speed control with turbo,
//! machine summary, settings toggle.

use crate::app::App;
use crate::ui::theme::Theme;
use egui_phosphor::fill as icon;

/// Small square icon button; returns `true` when clicked.
fn icon_button(
    ui: &mut egui::Ui,
    glyph: &str,
    enabled: bool,
    tooltip: &str,
    accent: egui::Color32,
) -> bool {
    let text = egui::RichText::new(glyph).size(14.0).strong().color(accent);
    ui.add_enabled(
        enabled,
        egui::Button::new(text).min_size(egui::vec2(28.0, 24.0)),
    )
    .on_hover_text(tooltip)
    .clicked()
}

pub fn draw(ui: &mut egui::Ui, app: &mut App) {
    let theme = app.active_theme_data();
    let c = |field: &[u8; 4]| Theme::color(field);
    egui::Panel::top("transport")
        .frame(
            egui::Frame::new()
                .fill(c(&theme.panel))
                .stroke(egui::Stroke::new(1.0, c(&theme.panel_border)))
                .inner_margin(egui::Margin::symmetric(8, 4)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // ---- transport ----
                let running = app.running;
                let accent = c(&theme.accent);
                let dim = c(&theme.text);
                if icon_button(ui, icon::PLAY, !running, "Run the machine", accent) {
                    app.set_running(true);
                }
                if icon_button(ui, icon::PAUSE, running, "Pause", dim) {
                    app.set_running(false);
                }
                if icon_button(
                    ui,
                    icon::STOP,
                    true,
                    "Reset and hold at the power-on state",
                    dim,
                ) {
                    app.stop();
                }
                if icon_button(
                    ui,
                    icon::ARROW_CLOCKWISE,
                    true,
                    "Warm reboot (monitor restart)",
                    dim,
                ) {
                    app.reset();
                }

                ui.separator();

                // ---- speed ----
                ui.strong("SPEED");
                let mut multiplier = app.speed.multiplier;
                let speed_edit = ui.add(
                    egui::DragValue::new(&mut multiplier)
                        .range(crate::speed::SPEED_MIN..=crate::speed::SPEED_MAX)
                        .speed(0.05)
                        .suffix("\u{d7}")
                        .fixed_decimals(2),
                );
                if speed_edit.changed() {
                    app.set_multiplier(multiplier);
                }
                for &preset in crate::speed::SPEED_PRESETS {
                    let selected = (app.speed.multiplier - preset).abs() < 1e-9;
                    if ui
                        .selectable_label(selected, preset_label(preset))
                        .clicked()
                    {
                        app.set_multiplier(preset);
                    }
                }
                // TURBO toggle: unthrottled, speaker muted.
                let turbo = app.speed.turbo;
                let turbo_label = if turbo {
                    egui::RichText::new("TURBO")
                        .strong()
                        .color(c(&theme.warn))
                } else {
                    egui::RichText::new("TURBO").color(c(&theme.text_weak))
                };
                if ui
                    .add(egui::Button::new(turbo_label).selected(turbo))
                    .on_hover_text("Unthrottled; speaker muted")
                    .clicked()
                {
                    app.set_turbo(!turbo);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Settings gear.
                    if ui
                        .button(egui::RichText::new(icon::GEAR).size(14.0))
                        .on_hover_text("Settings")
                        .clicked()
                    {
                        app.ui.settings_open = !app.ui.settings_open;
                    }
                    // Machine summary on the right.
                    let module = app.config.rom_module.as_deref().unwrap_or("no module");
                    let summary = if running {
                        format!("PMD {} \u{b7} {module}", app.config.model.name())
                    } else {
                        format!("PMD {} \u{b7} {module} \u{b7} HELD", app.config.model.name())
                    };
                    let state_color = if running {
                        c(&theme.accent)
                    } else {
                        c(&theme.warn)
                    };
                    ui.label(egui::RichText::new(summary).color(state_color).size(12.0));
                });
            });
        });
}

/// Preset labels: 0.25x -> "0.25×", 2x -> "2×".
fn preset_label(preset: f64) -> String {
    if preset < 1.0 {
        format!("{preset:.2}\u{d7}")
    } else {
        format!("{preset:.0}\u{d7}")
    }
}
