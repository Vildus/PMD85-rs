//! The main view: the emulated CRT screen, integer-scaled and centered
//! on a dark bezel.

use crate::app::App;
use crate::ui::theme::Theme;
use pmd85_core::vram::{HEIGHT, WIDTH};

pub fn draw(ui: &mut egui::Ui, app: &mut App) {
    let theme = app.active_theme_data();
    let c = |field: &[u8; 4]| Theme::color(field);
    egui::CentralPanel::default_margins()
        .frame(
            egui::Frame::new()
                .fill(c(&theme.bg))
                .inner_margin(egui::Margin::same(12)),
        )
        .show(ui, |ui| {
            let Some(tex) = &app.screen else {
                return;
            };
            let tex = tex.clone();

            // Integer scale of 288x256 into the available space.
            let avail = ui.available_size();
            let max_scale_x = (avail.x / WIDTH as f32).floor();
            let max_scale_y = (avail.y / HEIGHT as f32).floor();
            let scale = max_scale_x.min(max_scale_y).max(1.0).floor();
            let image_size = egui::vec2(WIDTH as f32 * scale, HEIGHT as f32 * scale);

            // Bezel: slightly larger than the image, drawn as a flat
            // dark frame with a hairline accent.
            let bezel = 8.0;
            let outer = image_size + egui::vec2(2.0 * bezel, 2.0 * bezel);

            let (outer_rect, _) = ui.allocate_exact_size(outer, egui::Sense::hover());
            // Center within leftovers when the available space is not
            // an exact multiple.
            let leftover = avail - outer;
            let shift = egui::vec2(
                (leftover.x / 2.0).floor().max(0.0),
                (leftover.y / 2.0).floor().max(0.0),
            );
            let outer_rect = outer_rect.translate(shift);
            let inner_rect = egui::Rect::from_min_size(
                outer_rect.min + egui::vec2(bezel, bezel),
                image_size,
            );

            // Soft glow behind the bezel, then the bezel itself.
            ui.painter().rect_filled(
                outer_rect,
                egui::CornerRadius::same(3),
                c(&theme.screen_bezel),
            );
            ui.painter().rect_stroke(
                outer_rect,
                egui::CornerRadius::same(3),
                egui::Stroke::new(1.0, c(&theme.panel_border)),
                egui::StrokeKind::Inside,
            );

            ui.put(
                inner_rect,
                egui::Image::from_texture(&tex).fit_to_exact_size(image_size),
            );
        });
}
