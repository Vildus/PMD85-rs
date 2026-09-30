//! The dock layout: the PMD screen and the tool panels are tabs of
//! an `egui_dock` tree, so they can be docked, split, grouped and
//! pulled out into floating windows. The layout (which panels are
//! visible and where they sit) is persisted as `layout.json` next to
//! the settings and restored on the next start.

use crate::app::App;
use crate::ui::theme::Theme;
use crate::ui::{keyboard, screen, tape};
use egui::{Color32, CornerRadius, Stroke};
use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
use serde::{Deserialize, Serialize};

/// One dockable panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tab {
    /// The emulated screen. Always present; cannot be closed.
    Screen,
    /// The keyboard layout reference.
    Keyboard,
    /// The cassette tape editor.
    Tape,
}

impl Tab {
    /// The tab-bar title.
    fn title(self) -> &'static str {
        match self {
            Tab::Screen => "Screen",
            Tab::Keyboard => "Keyboard",
            Tab::Tape => "Cassette tape",
        }
    }
}

/// The fresh layout: just the screen, filling the whole area.
pub fn default_dock() -> DockState<Tab> {
    DockState::new(vec![Tab::Screen])
}

/// Whether the tab is shown anywhere (the main surface or a floating
/// window).
pub fn tab_visible(dock: &DockState<Tab>, tab: Tab) -> bool {
    dock.find_tab(&tab).is_some()
}

/// Show a tab. With the pristine layout (a lone screen) the tab is
/// docked beside it; otherwise it joins the focused leaf as a tab
/// the user can drag wherever they want.
pub fn show_tab(dock: &mut DockState<Tab>, tab: Tab) {
    if dock.find_tab(&tab).is_some() {
        return;
    }
    let lone_screen = dock
        .main_surface()
        .root_node()
        .is_some_and(egui_dock::Node::is_leaf);
    if lone_screen {
        dock.main_surface_mut()
            .split_right(NodeIndex::root(), 0.3, vec![tab]);
    } else {
        dock.push_to_focused_leaf(tab);
    }
}

/// Remove a tab from wherever it is (main surface or window).
pub fn hide_tab(dock: &mut DockState<Tab>, tab: Tab) {
    while let Some(path) = dock.find_tab(&tab) {
        dock.remove_tab(path);
    }
}

/// Transport-bar toggle: show when hidden, hide when shown.
pub fn toggle_tab(dock: &mut DockState<Tab>, tab: Tab) {
    if tab_visible(dock, tab) {
        hide_tab(dock, tab);
    } else {
        show_tab(dock, tab);
    }
}

/// Draw the dock area (the tab tree plus any floating windows).
pub fn draw(ui: &mut egui::Ui, app: &mut App) {
    // Take the tree out so the tab viewer can borrow the app for
    // the duration of the pass.
    let mut dock = std::mem::replace(&mut app.ui.dock, default_dock());
    let style = dock_style(ui.style(), &app.active_theme_data());
    let mut viewer = Viewer { app };
    DockArea::new(&mut dock)
        .style(style)
        .show_inside(ui, &mut viewer);
    app.ui.dock = dock;
}

/// Dock chrome styled from the app theme. egui_dock's defaults are
/// light-mode grays that would clash with the phosphor look, so the
/// panels, separators and tab states are re-skinned here (the same
/// fields [`Theme::apply`] sets on egui itself).
fn dock_style(egui_style: &egui::Style, theme: &Theme) -> egui_dock::Style {
    let c = |field: &[u8; 4]| Theme::color(field);
    let panel = c(&theme.panel);
    let border = c(&theme.panel_border);
    let text = c(&theme.text);
    let text_weak = c(&theme.text_weak);
    let accent = c(&theme.accent);
    let mut style = egui_dock::Style::from_egui(egui_style);

    // Tab strip: a panel surface with the hairline under it.
    style.tab_bar.bg_fill = panel;
    style.tab_bar.hline_color = border;
    style.tab_bar.corner_radius = CornerRadius::same(2);

    // Tab states, mirroring the widget visuals of the theme:
    // inactive tabs sit flush with the bar, the active tab lifts to
    // the widget surface, the focused leaf's tab gets the accent edge.
    style.tab.inactive.bg_fill = panel;
    style.tab.inactive.text_color = text_weak;
    style.tab.inactive.outline_color = border;
    style.tab.inactive.corner_radius = CornerRadius::same(2);
    style.tab.active.bg_fill = c(&theme.widget);
    style.tab.active.text_color = text;
    style.tab.active.outline_color = border;
    style.tab.active.corner_radius = CornerRadius::same(2);
    style.tab.hovered.bg_fill = c(&theme.widget_hover);
    style.tab.hovered.text_color = text;
    style.tab.hovered.outline_color = border;
    style.tab.hovered.corner_radius = CornerRadius::same(2);
    style.tab.focused = style.tab.active.clone();
    style.tab.focused.outline_color = accent;
    // Keyboard-focus variants keep their base look.
    style.tab.active_with_kb_focus = style.tab.active.clone();
    style.tab.inactive_with_kb_focus = style.tab.inactive.clone();
    style.tab.focused_with_kb_focus = style.tab.focused.clone();

    // Tab body: the deepest layer, hairline-framed like a window.
    style.tab.tab_body.bg_fill = c(&theme.bg);
    style.tab.tab_body.stroke = Stroke::new(1.0, border);
    style.tab.tab_body.corner_radius = CornerRadius::same(2);

    // Draggable separators and the drop overlay glow with the accent.
    style.separator.color_idle = border;
    style.separator.color_hovered = c(&theme.accent_dim);
    style.separator.color_dragged = accent;
    style.overlay.selection_color = c(&theme.accent_dim);

    // Tab strip buttons (close/collapse): quiet, danger when armed.
    style.buttons.close_tab_color = text_weak;
    style.buttons.close_tab_active_color = c(&theme.danger);
    style.buttons.close_tab_bg_fill = Color32::TRANSPARENT;
    style.buttons.collapse_tabs_color = text_weak;
    style.buttons.collapse_tabs_active_color = text;
    style.buttons.collapse_tabs_border_color = border;
    style
}

/// Draws the tabs and forwards each one to its panel.
struct Viewer<'a> {
    app: &'a mut App,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        match *tab {
            Tab::Screen => screen::draw(ui, self.app),
            Tab::Keyboard => keyboard::ui(ui, self.app),
            Tab::Tape => tape::ui(ui, self.app),
        };
    }

    fn on_close(&mut self, tab: &mut Tab) -> OnCloseResponse {
        // Closing the keyboard must release a pointer-held key.
        if *tab == Tab::Keyboard {
            keyboard::release_pointer(self.app);
        }
        OnCloseResponse::Close
    }

    fn is_closeable(&self, tab: &Tab) -> bool {
        *tab != Tab::Screen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pristine layout is the screen alone, filling the area.
    #[test]
    fn default_layout_is_the_lone_screen() {
        let dock = default_dock();
        assert!(tab_visible(&dock, Tab::Screen));
        assert!(!tab_visible(&dock, Tab::Keyboard));
        assert!(!tab_visible(&dock, Tab::Tape));
        assert_eq!(dock.main_surface().num_tabs(), 1);
    }

    /// Toggling: the keyboard docks beside the screen, and hiding it
    /// restores the pristine lone-screen layout (so showing it again
    /// docks it in the same place).
    #[test]
    fn toggle_docks_and_undocks() {
        let mut dock = default_dock();
        show_tab(&mut dock, Tab::Keyboard);
        assert!(tab_visible(&dock, Tab::Keyboard));
        // Split happened: the root is no longer a lone leaf.
        assert!(!dock
            .main_surface()
            .root_node()
            .is_some_and(egui_dock::Node::is_leaf));

        toggle_tab(&mut dock, Tab::Keyboard);
        assert!(!tab_visible(&dock, Tab::Keyboard));
        assert!(dock
            .main_surface()
            .root_node()
            .is_some_and(egui_dock::Node::is_leaf));
        assert_eq!(dock.main_surface().num_tabs(), 1, "only the screen");

        toggle_tab(&mut dock, Tab::Keyboard);
        assert!(tab_visible(&dock, Tab::Keyboard));
    }

    /// The layout serializes (that is how it persists across
    /// restarts) and survives the round-trip with tabs intact —
    /// through the null-sanitizing path [`App::save_layout`] uses,
    /// since egui_dock's leaf rects serialize as JSON nulls.
    #[test]
    fn layout_serializes_with_visibility() {
        let mut dock = default_dock();
        show_tab(&mut dock, Tab::Keyboard);
        show_tab(&mut dock, Tab::Tape);
        let mut json = serde_json::to_value(&dock).expect("serialize");
        crate::app::sanitize_layout(&mut json);
        let restored: DockState<Tab> = serde_json::from_value(json).expect("deserialize");
        assert!(tab_visible(&restored, Tab::Screen));
        assert!(tab_visible(&restored, Tab::Keyboard));
        assert!(tab_visible(&restored, Tab::Tape));
    }
}
