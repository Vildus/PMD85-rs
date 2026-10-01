//! The custom window titlebar and the resize border, drawn instead
//! of the system frame (the `custom_titlebar` setting). The bar is a
//! plain egui panel; moving, resizing and the window buttons go
//! through window requests, since the UI cannot touch the window
//! itself — Wayland in particular only allows interactive move and
//! resize through the compositor, and the winit loop applies our
//! requests right after the frame that made them, while the pointer
//! press that motivates them is still held.

use crate::app::{App, WindowRequest};
use crate::ui::theme::Theme;
use egui_phosphor::regular as icon;
use winit::window::ResizeDirection;

/// Height of the titlebar: the transport-bar button height (24)
/// plus its vertical margins (2 × 4).
const BAR_HEIGHT: f32 = 32.0;

/// Thickness of the interactive resize border around the window.
const BORDER: f32 = 6.0;

/// The titlebar: app icon, title, and the window buttons, with the
/// empty stretch of the bar dragging the window (double-click
/// maximizes).
pub fn draw(ui: &mut egui::Ui, app: &mut App) {
    // The icon texture, created once (like the screen texture).
    if app.icon.is_none() {
        if let Some((rgba, width, height)) = crate::icon::icon_rgba() {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
            app.icon = Some(
                app.ctx
                    .load_texture("app-icon", image, egui::TextureOptions::default()),
            );
        }
    }

    let theme = app.active_theme_data();
    let c = |field: &[u8; 4]| Theme::color(field);
    egui::Panel::top("titlebar")
        .exact_size(BAR_HEIGHT)
        .frame(
            egui::Frame::new()
                .fill(c(&theme.panel))
                .stroke(egui::Stroke::new(1.0, c(&theme.panel_border)))
                .inner_margin(egui::Margin::symmetric(6, 4)),
        )
        .show_separator_line(false)
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                // The icon and the title.
                if let Some(texture) = &app.icon {
                    ui.add(
                        egui::Image::from_texture(texture)
                            .fit_to_exact_size(egui::vec2(18.0, 18.0)),
                    );
                    ui.add_space(4.0);
                }
                ui.label(
                    egui::RichText::new("PMD 85 \u{2014} Tesla")
                        .size(12.0)
                        .strong()
                        .color(c(&theme.text)),
                );

                // The empty stretch of the bar drags the window;
                // double-click toggles maximize. Registered before
                // the buttons so they sit on top of it and take the
                // clicks aimed at themselves.
                let drag_rect = ui.available_rect_before_wrap();
                let drag =
                    ui.interact(drag_rect, ui.id().with("drag"), egui::Sense::click_and_drag());
                if drag.drag_started_by(egui::PointerButton::Primary) {
                    app.request_window(WindowRequest::Drag);
                }
                if drag.double_clicked() {
                    app.request_window(WindowRequest::ToggleMaximize);
                }

                // The window buttons, right-aligned.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if window_button(ui, &theme, icon::X, "Close", true) {
                        app.request_window(WindowRequest::Close);
                    }
                    if window_button(
                        ui,
                        &theme,
                        if app.window_maximized {
                            icon::CORNERS_OUT
                        } else {
                            icon::SQUARE
                        },
                        if app.window_maximized {
                            "Restore"
                        } else {
                            "Maximize"
                        },
                        false,
                    ) {
                        app.request_window(WindowRequest::ToggleMaximize);
                    }
                    if window_button(ui, &theme, icon::MINUS, "Minimize", false) {
                        app.request_window(WindowRequest::Minimize);
                    }
                });
            });
        });
}

/// One square window-control button, styled exactly like the
/// transport-bar buttons; returns `true` when clicked.
fn window_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    glyph: &str,
    tooltip: &str,
    danger: bool,
) -> bool {
    let color = if danger {
        Theme::color(&theme.danger)
    } else {
        Theme::color(&theme.text)
    };
    let text = egui::RichText::new(glyph).size(14.0).strong().color(color);
    let button = egui::Button::new(text).min_size(egui::vec2(28.0, 24.0));
    ui.add(button).on_hover_text(tooltip).clicked()
}

/// The interactive resize border around the whole window, drawn last
/// so it sits on top of everything: four edges and four corners, each
/// starting a compositor resize in its direction. Without the system
/// frame this is the only way to resize, and — exactly like the
/// invisible CSD margins of GTK-style apps — it is not painted at
/// all: the window looks the same as it does maximized. Hidden while
/// maximized, as the system frame's resize borders would be.
pub fn border(ui: &mut egui::Ui, app: &mut App) {
    if app.window_maximized {
        return;
    }
    for (index, (direction, region, cursor)) in regions(ui.max_rect()).into_iter().enumerate() {
        let response = ui.interact(
            region,
            egui::Id::new("resize-border").with(index),
            egui::Sense::drag(),
        );
        if response.drag_started_by(egui::PointerButton::Primary) {
            app.request_window(WindowRequest::Resize(direction));
        }
        response.on_hover_cursor(cursor);
    }
}

/// The eight interactive regions of the resize border for a window
/// `rect`: edges first, corners after — where they overlap, the
/// corners (registered later) sit on top and take the pointer.
fn regions(
    rect: egui::Rect,
) -> Vec<(ResizeDirection, egui::Rect, egui::CursorIcon)> {
    let left = rect.left();
    let right = rect.right();
    let top = rect.top();
    let bottom = rect.bottom();
    vec![
        (
            ResizeDirection::North,
            egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, top + BORDER)),
            egui::CursorIcon::ResizeNorth,
        ),
        (
            ResizeDirection::South,
            egui::Rect::from_min_max(egui::pos2(left, bottom - BORDER), egui::pos2(right, bottom)),
            egui::CursorIcon::ResizeSouth,
        ),
        (
            ResizeDirection::West,
            egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(left + BORDER, bottom)),
            egui::CursorIcon::ResizeWest,
        ),
        (
            ResizeDirection::East,
            egui::Rect::from_min_max(egui::pos2(right - BORDER, top), egui::pos2(right, bottom)),
            egui::CursorIcon::ResizeEast,
        ),
        (
            ResizeDirection::NorthWest,
            egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(left + BORDER, top + BORDER)),
            egui::CursorIcon::ResizeNorthWest,
        ),
        (
            ResizeDirection::NorthEast,
            egui::Rect::from_min_max(egui::pos2(right - BORDER, top), egui::pos2(right, top + BORDER)),
            egui::CursorIcon::ResizeNorthEast,
        ),
        (
            ResizeDirection::SouthWest,
            egui::Rect::from_min_max(egui::pos2(left, bottom - BORDER), egui::pos2(left + BORDER, bottom)),
            egui::CursorIcon::ResizeSouthWest,
        ),
        (
            ResizeDirection::SouthEast,
            egui::Rect::from_min_max(egui::pos2(right - BORDER, bottom - BORDER), egui::pos2(right, bottom)),
            egui::CursorIcon::ResizeSouthEast,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The direction for a screen position on the window frame:
    /// [`BORDER`] pixels of edge/corner around `rect`, `None` inside
    /// or outside it.
    fn edge_direction(rect: egui::Rect, pos: egui::Pos2) -> Option<ResizeDirection> {
        if !rect.contains(pos) {
            return None;
        }
        let near_west = pos.x - rect.left() < BORDER;
        let near_east = rect.right() - pos.x < BORDER;
        let near_north = pos.y - rect.top() < BORDER;
        let near_south = rect.bottom() - pos.y < BORDER;
        match (near_north, near_south, near_west, near_east) {
            (true, _, true, _) => Some(ResizeDirection::NorthWest),
            (true, _, _, true) => Some(ResizeDirection::NorthEast),
            (_, true, true, _) => Some(ResizeDirection::SouthWest),
            (_, true, _, true) => Some(ResizeDirection::SouthEast),
            (true, _, _, _) => Some(ResizeDirection::North),
            (_, true, _, _) => Some(ResizeDirection::South),
            (_, _, true, _) => Some(ResizeDirection::West),
            (_, _, _, true) => Some(ResizeDirection::East),
            (false, false, false, false) => None,
        }
    }

    /// Every interactive border region drags in exactly its own
    /// direction (the corners it overlaps aside — they sit on top),
    /// and the frame leaves the middle of the window alone.
    #[test]
    fn border_regions_drag_in_their_directions() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        for (direction, region, _cursor) in regions(rect) {
            // The center of a region (for corners, its very center)
            // must map back to that region's own direction.
            assert_eq!(
                edge_direction(rect, region.center()),
                Some(direction),
                "region {direction:?} at {region:?}"
            );
            assert!(region.width() > 0.0 && region.height() > 0.0);
        }
        // The middle of the window is no resize direction.
        assert_eq!(edge_direction(rect, egui::pos2(400.0, 300.0)), None);
        // Neither is outside the window.
        assert_eq!(edge_direction(rect, egui::pos2(-5.0, 300.0)), None);
    }
}
