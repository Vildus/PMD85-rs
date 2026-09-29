//! Status bar: machine, module, timing, LED, audio state.

use crate::app::App;
use crate::ui::theme::Theme;
use egui_phosphor::regular as icon;

pub fn draw(ui: &mut egui::Ui, app: &mut App) {
    let theme = app.active_theme_data();
    let c = |field: &[u8; 4]| Theme::color(field);
    egui::Panel::bottom("status")
        .frame(
            egui::Frame::new()
                .fill(c(&theme.panel))
                .stroke(egui::Stroke::new(1.0, c(&theme.panel_border)))
                .inner_margin(egui::Margin::symmetric(8, 3)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // LED (PC3 of the system 8255).
                let led_on = app.machine.bus.led;
                let (led_color, led_note) = if led_on {
                    (c(&theme.danger), "red LED on")
                } else {
                    (egui::Color32::from_rgb(40, 20, 20), "red LED off")
                };
                let (led_rect, resp) =
                    ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(led_rect.center(), 5.0, led_color);
                resp.on_hover_text(led_note);

                label(ui, "MODEL", &theme);
                ui.label(value_text(&format!("PMD {}", app.config.model.name()), &theme));

                label(ui, "MODULE", &theme);
                ui.label(value_text(
                    app.config.rom_module.as_deref().unwrap_or("\u{2014}"),
                    &theme,
                ));

                label(ui, "TIME", &theme);
                let (h, m, s) = app.emulated_time();
                ui.label(value_text(&format!("{h:02}:{m:02}:{s:02}"), &theme));

                label(ui, "CYCLES", &theme);
                ui.label(value_text(
                    &format!("{}", app.machine.bus.total_cycles()),
                    &theme,
                ));

                label(ui, "SPEED", &theme);
                let emulated = app.emulated_fps();
                let pct = emulated / crate::speed::FRAMES_PER_SEC * 100.0;
                ui.label(value_text(&format!("{emulated:5.1} Hz  {pct:5.1}%"), &theme));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Audio state.
                    let (glyph, color, note) = if app.settings.mute {
                        (icon::SPEAKER_SLASH, c(&theme.warn), "speaker muted")
                    } else if app.audio.is_none() {
                        (icon::SPEAKER_SLASH, c(&theme.text_weak), "no audio device")
                    } else if !app.wants_audio() {
                        (icon::SPEAKER_X, c(&theme.text_weak), "audio suspended (speed \u{2260} 1\u{d7} or paused)")
                    } else {
                        (icon::SPEAKER_HIGH, c(&theme.accent), "speaker live")
                    };
                    ui.label(
                        egui::RichText::new(glyph)
                            .size(13.0)
                            .color(color),
                    )
                    .on_hover_text(note);

                    // Render fps.
                    ui.label(caption(
                        &format!("UI {:5.1} fps", app.render_fps()),
                        &theme,
                    ));
                });
            });
        });
}

fn label(ui: &mut egui::Ui, text: &str, theme: &Theme) {
    ui.label(caption(text, theme));
}

fn caption(text: &str, theme: &Theme) -> egui::RichText {
    egui::RichText::new(text)
        .color(Theme::color(&theme.text_weak))
        .size(11.0)
        .strong()
}

fn value_text(text: &str, theme: &Theme) -> egui::RichText {
    egui::RichText::new(text).color(Theme::color(&theme.text)).size(11.0)
}
