//! Immediate-mode UI: panels drawn from the [`App`] state each frame.
//!
//! Each panel is a standalone `draw(ctx, app)` function so panels can
//! later move into an `egui_dock` tree unchanged.

pub mod controls;
pub mod keyboard;
pub mod screen;
pub mod settings;
pub mod status;
pub mod theme;

use crate::app::App;

/// UI-only state (windows, dialogs), distinct from emulator state.
#[derive(Debug, Default)]
pub struct UiState {
    /// The settings window.
    pub settings_open: bool,
    /// The keyboard layout reference window.
    pub keyboard_open: bool,
    /// A configuration awaiting the "restart machine?" confirmation.
    pub confirm_reboot: Option<crate::app::MachineConfig>,
    /// The color-customization expander in the settings window.
    pub theme_customize: bool,
    /// Unsaved working copy in the color editor (theme name + palette).
    pub theme_draft: Option<theme::Theme>,
    /// Emulated key currently held down via a click in the keyboard
    /// view (released when the pointer button goes up).
    pub mouse_key: Option<pmd85_core::keyboard::Key>,
    /// Whether the keyboard view's RST cap is pointer-held.
    pub mouse_rst: bool,
}

/// Draw the whole UI for one frame (between `begin_pass`/`end_pass`).
/// `ui` is the root Ui covering the window.
pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    controls::draw(ui, app);
    status::draw(ui, app);
    screen::draw(ui, app);
    let ctx = ui.ctx().clone();
    keyboard::draw(&ctx, app);
    settings::draw(&ctx, app);
    draw_notifications(&ctx, app);
}

/// Transient error popups, bottom-right.
fn draw_notifications(ctx: &egui::Context, app: &mut App) {
    if app.notifications().is_empty() {
        return;
    }
    egui::Area::new(egui::Id::new("notifications"))
        .anchor(egui::Align2::RIGHT_BOTTOM, [-8.0, -8.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let theme = app.active_theme_data();
            egui::Frame::new()
                .fill(theme::Theme::color(&theme.panel))
                .stroke(egui::Stroke::new(1.0, theme::Theme::color(&theme.danger)))
                .inner_margin(8)
                .corner_radius(2)
                .show(ui, |ui| {
                    ui.set_min_width(220.0);
                    ui.vertical(|ui| {
                        for (message, _) in app.notifications() {
                            ui.label(
                                egui::RichText::new(message)
                                    .color(theme::Theme::color(&theme.text))
                                    .size(12.0),
                            );
                        }
                    });
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::Args;
    use crate::config::AppSettings;
    use pmd85_core::Model;

    /// One named UI smoke-test case: a setup function run on a fresh
    /// `App` before frames are built.
    type UiCase = Box<dyn Fn(&mut App)>;

    /// Build an App against the repo's Rom/ directory (headless: no
    /// window, no audio device requirements). Persisted settings go
    /// to a scratch directory, never the user's real config.
    pub fn test_app() -> App {
        app_with_settings(AppSettings::default())
    }

    /// [`test_app`] with explicit initial settings.
    fn app_with_settings(settings: AppSettings) -> App {
        let mut app = App::new(egui::Context::default(), &Args::default(), settings);
        let dir = std::env::temp_dir().join(format!("pmd85-app-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        app.set_settings_dir(dir);
        app
    }

    /// The UI must build a full frame without panicking in every
    /// interesting state.
    #[test]
    fn ui_builds_frames_in_all_states() {
        let cases: Vec<(&str, UiCase)> = vec![
            ("boot", Box::new(|_| {})),
            ("paused", Box::new(|a| a.set_running(false))),
            (
                "turbo",
                Box::new(|a| {
                    a.set_turbo(true);
                }),
            ),
            (
                "slow",
                Box::new(|a| a.set_multiplier(0.25)),
            ),
            (
                "fast",
                Box::new(|a| a.set_multiplier(10.0)),
            ),
            (
                "settings-open",
                Box::new(|a| a.ui.settings_open = true),
            ),
            (
                "keyboard-open",
                Box::new(|a| a.ui.keyboard_open = true),
            ),
            (
                "settings-changed",
                Box::new(|a| {
                    a.ui.settings_open = true;
                    a.pending_config.model = Model::Pmd851;
                    a.pending_config.rom_module = Some("basic1.rmm".into());
                }),
            ),
            (
                "confirm-reboot",
                Box::new(|a| {
                    a.ui.confirm_reboot = Some(a.pending_config.clone());
                }),
            ),
            (
                "muted",
                Box::new(|a| a.set_mute(true)),
            ),
            (
                "error-notification",
                Box::new(|a| a.notify("test error")),
            ),
        ];
        for (name, setup) in cases {
            let mut app = test_app();
            setup(&mut app);
            // A few frames, running emulation so the screen updates.
            for _ in 0..5 {
                app.advance();
                let ctx = app.ctx.clone();
                let out = ctx.run_ui(egui::RawInput::default(), |ui| draw(&mut app, ui));
                assert!(!out.shapes.is_empty(), "{name}: no shapes");
                out.drop_without_applying_deltas();
            }
        }
    }

    #[test]
    fn advance_emulates_and_updates_screen() {
        let mut app = test_app();
        let before = app.machine.bus.total_cycles();
        app.advance();
        assert!(app.machine.bus.total_cycles() > before);
        assert!(app.frames_done >= 1);
        assert!(app.screen.is_some(), "screen texture created");
    }

    #[test]
    fn paused_advance_does_not_emulate() {
        let mut app = test_app();
        app.set_running(false);
        let before = app.machine.bus.total_cycles();
        app.advance();
        assert_eq!(app.machine.bus.total_cycles(), before);
        assert_eq!(app.frames_done, 0);
    }

    #[test]
    fn stop_resets_and_holds() {
        let mut app = test_app();
        app.advance();
        app.advance();
        let cycles = app.machine.bus.total_cycles();
        app.stop();
        assert!(!app.running);
        assert_eq!(app.frames_done, 0);
        // Held: no further emulation happens.
        app.advance();
        assert_eq!(app.machine.bus.total_cycles(), cycles);
    }

    #[test]
    fn reset_keeps_running() {
        let mut app = test_app();
        app.advance();
        let before = app.machine.bus.total_cycles();
        app.reset();
        assert!(app.running);
        // The machine is live again: cycles keep accumulating past the
        // (monotonic) counter value from before the reset.
        app.advance();
        assert!(app.machine.bus.total_cycles() > before);
    }

    #[test]
    fn rebuild_machine_switches_model_and_module() {
        let mut app = test_app();
        app.advance();
        let cfg = crate::app::MachineConfig {
            model: Model::Pmd852,
            monitor: None,
            rom_module: Some("basic2.rmm".into()),
        };
        app.apply_config(cfg);
        assert_eq!(app.machine.model(), Model::Pmd852);
        assert!(app.machine.bus.memory.rom_module.is_some());
        assert!(app.running);
        assert_eq!(app.frames_done, 0);
        // pending_config follows the applied configuration.
        assert_eq!(app.pending_config, app.config);
    }

    #[test]
    fn rebuild_with_missing_rom_is_not_fatal() {
        let mut app = test_app();
        let cfg = crate::app::MachineConfig {
            model: Model::Pmd851,
            monitor: Some("does-not-exist.rom".into()),
            rom_module: None,
        };
        app.apply_config(cfg);
        assert!(!app.notifications().is_empty());
        // Machine still exists and can run (blank monitor).
        app.advance();
    }

    #[test]
    fn multiplier_clamps_to_the_planned_range() {
        let mut app = test_app();
        app.set_multiplier(0.01);
        assert_eq!(app.speed.multiplier, crate::speed::SPEED_MIN);
        app.set_multiplier(1000.0);
        assert_eq!(app.speed.multiplier, crate::speed::SPEED_MAX);
        app.set_multiplier(2.0);
        assert_eq!(app.speed.multiplier, 2.0);
        app.set_multiplier(1.0);
        // Settings loaded from disk are clamped too.
        let settings = AppSettings { speed: 500.0, ..AppSettings::default() };
        let app = app_with_settings(settings);
        assert_eq!(app.speed.multiplier, crate::speed::SPEED_MAX);
    }

    #[test]
    fn paused_ui_keeps_painting() {
        // Regression: while paused/stopped the event loop must keep
        // requesting (lazy) redraws, otherwise the UI freezes and the
        // machine can never be unpaused.
        let mut app = test_app();
        app.set_running(false);
        assert!(app.wants_redraw(std::time::Instant::now()));
        // The lazy wake must also be scheduled in the future, so the
        // loop keeps ticking instead of parking forever.
        let now = std::time::Instant::now();
        assert!(app.next_wake(now) > now);
    }

    #[test]
    fn settings_window_stays_within_the_screen() {
        // Regression: the settings combo boxes used to be sized from
        // `available_width` while more widgets followed in the same
        // row. The window sizes itself to its content, so the row was
        // always a bit wider than the window: a feedback loop that
        // grew the window past the screen edge within a few frames.
        let mut app = test_app();
        app.ui.settings_open = true;
        // Show the "restart required" warning row too (pending config
        // differs from the running one).
        app.pending_config.model = Model::Pmd851;
        app.pending_config.rom_module = Some("basic1.rmm".into());
        let ctx = app.ctx.clone();
        let screen = egui::vec2(800.0, 600.0);
        let raw = |screen: egui::Vec2| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
            ..Default::default()
        };
        let mut right_edge: f32 = 0.0;
        for _ in 0..30 {
            let out = ctx.run_ui(raw(screen), |ui| {
                crate::ui::settings::draw(ui.ctx(), &mut app);
            });
            // Frames while the window is opening may paint nothing.
            if let Some(bbox) = out
                .shapes
                .iter()
                .map(|s| s.shape.visual_bounding_rect())
                .filter(|r| r.is_finite() && r.width() > 0.0 && r.height() > 0.0)
                .fold(None::<egui::Rect>, |acc, r| {
                    Some(acc.map_or(r, |a| a.union(r)))
                })
            {
                right_edge = right_edge.max(bbox.right());
            }
            out.drop_without_applying_deltas();
        }
        assert!(right_edge > 0.0, "settings window never painted");
        assert!(
            right_edge <= screen.x + 1.0,
            "settings window overflows the screen (right edge {:.0} > {:.0})",
            right_edge,
            screen.x
        );
    }

    #[test]
    fn keyboard_window_paints_within_the_screen() {
        let mut app = test_app();
        app.ui.keyboard_open = true;
        let ctx = app.ctx.clone();
        let screen = egui::vec2(1024.0, 768.0);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
            ..Default::default()
        };
        let mut right_edge: f32 = 0.0;
        for _ in 0..10 {
            let out = ctx.run_ui(raw.clone(), |ui| {
                crate::ui::keyboard::draw(ui.ctx(), &mut app);
            });
            if let Some(bbox) = out
                .shapes
                .iter()
                .map(|s| s.shape.visual_bounding_rect())
                .filter(|r| r.is_finite() && r.width() > 0.0 && r.height() > 0.0)
                .fold(None::<egui::Rect>, |acc, r| {
                    Some(acc.map_or(r, |a| a.union(r)))
                })
            {
                right_edge = right_edge.max(bbox.right());
            }
            out.drop_without_applying_deltas();
        }
        assert!(right_edge > 100.0, "keyboard window never painted");
        // The window must open at (at least) the content size: the
        // full layout is painted, not squeezed into a narrow
        // scrollable strip.
        assert!(
            right_edge > 480.0,
            "keyboard window too narrow (right edge {right_edge:.0})"
        );
        assert!(
            right_edge <= screen.x + 1.0,
            "keyboard window overflows the screen (right edge {:.0} > {:.0})",
            right_edge,
            screen.x
        );
    }

    #[test]
    fn audio_gating() {
        let mut app = test_app();
        assert!(app.wants_audio());
        app.set_multiplier(2.0);
        assert!(!app.wants_audio());
        app.set_multiplier(1.0);
        app.set_turbo(true);
        assert!(!app.wants_audio());
        app.set_turbo(false);
        app.set_mute(true);
        assert!(!app.wants_audio());
        app.set_mute(false);
        app.set_running(false);
        assert!(!app.wants_audio());
    }
}
