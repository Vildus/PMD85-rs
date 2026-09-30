//! Immediate-mode UI: panels drawn from the [`App`] state each frame.
//!
//! Each panel is a standalone `draw(ctx, app)` function so panels can
//! later move into an `egui_dock` tree unchanged.

pub mod controls;
pub mod dock;
pub mod keyboard;
pub mod screen;
pub mod settings;
pub mod status;
pub mod tape;
pub mod theme;

use crate::app::App;

/// UI-only state (windows, dialogs), distinct from emulator state.
#[derive(Debug)]
pub struct UiState {
    /// The settings window (a floating dialog).
    pub settings_open: bool,
    /// The dock layout: which panels are visible and where they are
    /// docked. Replaces the former per-window `*_open` flags — the
    /// tree itself is the visibility state, and it persists in
    /// `layout.json`.
    pub dock: egui_dock::DockState<dock::Tab>,
    /// Metadata entry form for a pending tape import.
    pub tape_import: Option<crate::ui::tape::ImportDraft>,
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

impl Default for UiState {
    fn default() -> Self {
        UiState {
            settings_open: false,
            dock: dock::default_dock(),
            tape_import: None,
            confirm_reboot: None,
            theme_customize: false,
            theme_draft: None,
            mouse_key: None,
            mouse_rst: false,
        }
    }
}

/// Draw the whole UI for one frame (between `begin_pass`/`end_pass`).
/// `ui` is the root Ui covering the window.
pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    controls::draw(ui, app);
    status::draw(ui, app);
    // Everything between the bars is the dock area; the screen is its
    // non-closable center tab, the tools dock around it.
    dock::draw(ui, app);
    let ctx = ui.ctx().clone();
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
                Box::new(|a| crate::ui::dock::show_tab(&mut a.ui.dock, crate::ui::dock::Tab::Keyboard)),
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
            (
                "tape-open",
                Box::new(|a| crate::ui::dock::show_tab(&mut a.ui.dock, crate::ui::dock::Tab::Tape)),
            ),
            (
                "tape-with-file",
                Box::new(|a| {
                    crate::ui::dock::show_tab(&mut a.ui.dock, crate::ui::dock::Tab::Tape);
                    let block = pmd85_core::tape::make_file(
                        0,
                        b'?',
                        "TESTFILE",
                        0x1000,
                        &[1u8, 2, 3, 4],
                    )
                    .unwrap();
                    a.tape.tape.blocks.push(block);
                    a.tape.selected = Some(0);
                }),
            ),
            (
                "tape-playing",
                Box::new(|a| {
                    crate::ui::dock::show_tab(&mut a.ui.dock, crate::ui::dock::Tab::Tape);
                    a.tape.flash = false;
                    let block = pmd85_core::tape::make_file(
                        0,
                        b'?',
                        "TESTFILE",
                        0x1000,
                        &[1u8, 2, 3, 4],
                    )
                    .unwrap();
                    a.tape.tape.blocks.push(block);
                    a.play_tape_file(0);
                }),
            ),
            (
                "tape-flash-loading",
                Box::new(|a| {
                    crate::ui::dock::show_tab(&mut a.ui.dock, crate::ui::dock::Tab::Tape);
                    let block = pmd85_core::tape::make_file(
                        0,
                        b'?',
                        "TESTFILE",
                        0x1000,
                        &[1u8, 2, 3, 4],
                    )
                    .unwrap();
                    a.tape.tape.blocks.push(block);
                    a.play_tape_file(0);
                }),
            ),
            (
                "tape-import-form",
                Box::new(|a| {
                    crate::ui::dock::show_tab(&mut a.ui.dock, crate::ui::dock::Tab::Tape);
                    let block = pmd85_core::tape::make_file(
                        0,
                        b'?',
                        "TESTFILE",
                        0x1000,
                        &[1u8, 2, 3, 4],
                    )
                    .unwrap();
                    a.tape.tape.blocks.push(block);
                    a.ui.tape_import = Some(crate::ui::tape::ImportDraft {
                        path: "raw.bin".into(),
                        number: "01".into(),
                        block_type: "?".into(),
                        name: "RAW".into(),
                        start: "1000".into(),
                    });
                }),
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
    fn keyboard_tab_paints_within_the_screen() {
        let mut app = test_app();
        crate::ui::dock::show_tab(&mut app.ui.dock, crate::ui::dock::Tab::Keyboard);
        let ctx = app.ctx.clone();
        let screen = egui::vec2(1024.0, 768.0);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
            ..Default::default()
        };
        let mut right_edge: f32 = 0.0;
        for _ in 0..10 {
            let out = ctx.run_ui(raw.clone(), |ui| {
                crate::ui::dock::draw(ui, &mut app);
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
        // The dock (screen tab + the keyboard pane beside it) must
        // paint across the area and never overflow the window.
        assert!(right_edge > 100.0, "dock area never painted");
        assert!(
            right_edge <= screen.x + 1.0,
            "dock area overflows the screen (right edge {:.0} > {:.0})",
            right_edge,
            screen.x
        );
    }

    /// The dock layout persists through `layout.json`: what was
    /// visible and docked when the app exited comes back, and a
    /// corrupt file falls back to the default instead of panicking.
    #[test]
    fn dock_layout_survives_a_restart() {
        let dir = state_scratch_dir("dock-layout");
        let mut app = test_app();
        app.set_settings_dir(dir.clone());
        crate::ui::dock::show_tab(&mut app.ui.dock, crate::ui::dock::Tab::Keyboard);
        crate::ui::dock::show_tab(&mut app.ui.dock, crate::ui::dock::Tab::Tape);
        app.save_layout();
        assert!(app.layout_path().unwrap().is_file());

        let mut app2 = test_app();
        app2.set_settings_dir(dir.clone());
        app2.restore_layout();
        assert!(crate::ui::dock::tab_visible(&app2.ui.dock, crate::ui::dock::Tab::Screen));
        assert!(crate::ui::dock::tab_visible(&app2.ui.dock, crate::ui::dock::Tab::Keyboard));
        assert!(crate::ui::dock::tab_visible(&app2.ui.dock, crate::ui::dock::Tab::Tape));

        // A corrupt layout file must not take the app down.
        std::fs::write(app.layout_path().unwrap(), "{ not json").unwrap();
        let mut app3 = test_app();
        app3.set_settings_dir(dir.clone());
        app3.restore_layout();
        assert_eq!(
            app3.ui.dock.main_surface().num_tabs(),
            1,
            "falls back to the lone screen"
        );

        // A layout without the (non-closable) screen tab is refused
        // as well — it cannot happen through the UI, only by hand
        // editing the file.
        let screenless =
            serde_json::to_string(&crate::ui::dock::default_dock()).unwrap();
        let screenless = screenless.replace("\"Screen\"", "\"Keyboard\"");
        std::fs::write(app.layout_path().unwrap(), screenless).unwrap();
        let mut app4 = test_app();
        app4.set_settings_dir(dir.clone());
        app4.restore_layout();
        assert!(
            crate::ui::dock::tab_visible(&app4.ui.dock, crate::ui::dock::Tab::Screen),
            "screen-less layout replaced by the default"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A breakpoint mid-frame pauses the transport and the tape deck
    /// (the pump must stay in lockstep with the machine); continuing
    /// runs on without re-tripping the same address.
    #[test]
    fn breakpoint_pauses_the_run_mid_frame() {
        let mut app = test_app();
        // The very first boot instruction (0x0000 under the startup
        // shadow map) carries a breakpoint: the first emulated frame
        // must stop right there and pause the transport.
        app.machine.toggle_breakpoint(0x0000);
        app.run_frames(5);
        assert!(!app.running, "transport paused at the breakpoint");
        assert_eq!(app.machine.breakpoint_hit(), Some(0x0000));
        assert_eq!(app.machine.cpu.pc, 0x0000);
        assert!(
            app.machine.bus.total_cycles() < pmd85_core::machine::CYCLES_PER_FRAME,
            "stopped mid-frame, not after it"
        );

        // Continuing boots on without re-tripping the hit address.
        app.machine.resume();
        app.set_running(true);
        app.run_frames(5);
        assert!(app.running, "no re-trip at the hit address");
        assert_eq!(app.machine.breakpoint_hit(), None);
        assert!(
            app.machine.bus.total_cycles() >= 4 * pmd85_core::machine::CYCLES_PER_FRAME,
            "the frames really ran"
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

    // ----- tape -----

    use pmd85_core::keyboard::Key;
    use pmd85_core::tape::{self, Tape};

    /// Press and release an emulated key (16 frames, like a human
    /// typist the monitor can follow), keeping the tape deck pumped.
    fn type_key(app: &mut App, k: Key) {
        app.machine.bus.keyboard.set_key(k, true);
        for _ in 0..8 {
            app.machine.step_frame();
            let _ = app.tape.pump(&mut app.machine);
        }
        app.machine.bus.keyboard.set_key(k, false);
        for _ in 0..8 {
            app.machine.step_frame();
            let _ = app.tape.pump(&mut app.machine);
        }
    }

    /// Type a command on the machine keyboard and press Enter.
    fn type_command(app: &mut App, keys: &[Key]) {
        for &k in keys {
            type_key(app, k);
        }
    }

    /// Name of tape file `file` (for assertions).
    fn file_name(app: &App, file: usize) -> String {
        let files = app.tape.files();
        app.tape.tape.blocks[files[file].block]
            .header
            .as_ref()
            .unwrap()
            .name_str()
    }

    #[test]
    fn tape_editor_roundtrip() {
        let mut app = test_app();
        let dir = std::env::temp_dir().join(format!("pmd85-tape-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let c1 = vec![0x76u8, 1, 2, 3];
        let c2 = vec![9u8, 8, 7, 6, 5];
        let mut image = Tape::default();
        image
            .blocks
            .push(tape::make_file(0, b'?', "FIRST", 0x1000, &c1).unwrap());
        image
            .blocks
            .push(tape::make_file(1, b'?', "SECOND", 0x2000, &c2).unwrap());
        let path = dir.join("tape.ptp");
        tape::save(&path, &image).unwrap();

        app.open_tape(&path).unwrap();
        assert_eq!(app.tape.files().len(), 2);
        assert!(!app.tape.dirty);
        assert_eq!(app.tape.path.as_deref(), Some(path.as_path()));
        assert_eq!(app.tape.export_file(0).unwrap(), c1);
        assert_eq!(app.tape.export_file(1).unwrap(), c2);

        // A headerless block appended after SECOND groups beneath it.
        let cont = vec![0xAAu8, 0xBB];
        let mut stream = Vec::new();
        stream.extend_from_slice(&(cont.len() as u16).to_le_bytes());
        stream.extend_from_slice(&cont);
        app.tape.tape.append_ptp_stream(&stream);
        app.tape.dirty = true; // appending is an edit
        let files = app.tape.files();
        assert_eq!(files.len(), 2);
        assert_eq!(files[1].continuations.len(), 1, "continuation block");
        app.tape.dirty = false; // not under test below

        // Move SECOND (with its continuation) up.
        app.tape.move_file(1, -1);
        assert_eq!(file_name(&app, 0), "SECOND");
        assert_eq!(file_name(&app, 1), "FIRST");
        assert_eq!(
            app.tape.files()[0].continuations.len(),
            1,
            "continuation moved along"
        );
        assert!(app.tape.dirty);
        assert_eq!(app.tape.selected, Some(0));

        // Delete SECOND; FIRST remains (one header+body block).
        app.tape.delete_file(0);
        assert_eq!(app.tape.files().len(), 1);
        assert_eq!(file_name(&app, 0), "FIRST");
        assert_eq!(app.tape.tape.blocks.len(), 1, "the file's block");

        // Save under a new name and reopen.
        let path2 = dir.join("copy.ptp");
        app.tape.save_as(&path2).unwrap();
        assert!(!app.tape.dirty);
        assert_eq!(app.tape.path.as_deref(), Some(path2.as_path()));
        app.open_tape(&path2).unwrap();
        assert_eq!(app.tape.files().len(), 1);
        assert_eq!(app.tape.export_file(0).unwrap(), c1);

        // Import appends a new file and selects it.
        let index = app
            .tape
            .import_file(5, b'?', "IMPORTD", 0x0300, b"hello tape")
            .unwrap();
        assert_eq!(index, 1);
        assert_eq!(app.tape.selected, Some(1));
        assert_eq!(app.tape.files().len(), 2);
        assert_eq!(app.tape.export_file(1).unwrap(), b"hello tape");

        // Saving without a path fails (fresh tape).
        app.new_tape();
        assert!(app.tape.save().is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tape_play_file_loads_via_the_machine() {
        let mut app = test_app();
        app.tape.flash = false; // exercise the real tape interface
        let mut content: Vec<u8> = (1..17u16).map(|i| i as u8).collect();
        content.insert(0, 0x76); // halt, so the loaded file stops cleanly
        app.tape
            .tape
            .blocks
            .push(tape::make_file(0, b'?', "LOADTST", 0x1000, &content).unwrap());

        // Boot the monitor, then type MGLD 00.
        app.run_frames(500);
        use Key::*;
        type_command(&mut app, &[M, G, L, D, Space, Digit0, Digit0, Enter]);

        // Play: the header and body are fed, then the session ends
        // (auto-stop is on by default).
        app.play_tape_file(0);
        assert!(app.tape.is_playing());
        assert!(app.machine.bus.tape.is_playing());
        for _ in 0..1500 {
            if !app.tape.is_playing() {
                break;
            }
            app.run_frames(1);
        }
        assert!(!app.tape.is_playing(), "playback session did not end");
        assert!(!app.machine.bus.tape.is_playing());
        for (i, &expect) in content.iter().enumerate() {
            assert_eq!(app.machine.bus.memory.ram[0x1000 + i], expect, "+{i}");
        }
    }

    /// Play the file with flash loading off but the emulation sped up:
    /// the deck must be pumped per emulated frame (not per rendered
    /// frame) or the load corrupts — this pins that contract.
    #[test]
    fn tape_play_file_survives_a_speedup() {
        let mut app = test_app();
        app.tape.flash = false;
        let mut content: Vec<u8> = (1..33u16).map(|i| i as u8).collect();
        content.insert(0, 0x76);
        app.tape
            .tape
            .blocks
            .push(tape::make_file(0, b'?', "FASTTST", 0x1000, &content).unwrap());
        app.set_multiplier(10.0);

        app.run_frames(500);
        use Key::*;
        type_command(&mut app, &[M, G, L, D, Space, Digit0, Digit0, Enter]);
        app.play_tape_file(0);
        for _ in 0..3000 {
            if !app.tape.is_playing() {
                break;
            }
            app.run_frames(1);
        }
        assert!(!app.tape.is_playing(), "playback session did not end");
        for (i, &expect) in content.iter().enumerate() {
            assert_eq!(app.machine.bus.memory.ram[0x1000 + i], expect, "+{i}");
        }
    }

    #[test]
    fn tape_flash_loads_the_file() {
        let mut app = test_app();
        let mut content: Vec<u8> = (1..17u16).map(|i| i as u8).collect();
        content.insert(0, 0x76);
        app.tape
            .tape
            .blocks
            .push(tape::make_file(2, b'?', "FLASHTST", 0x1000, &content).unwrap());

        // Boot the monitor and enter the MGLD 02 wait.
        app.run_frames(500);
        use Key::*;
        type_command(&mut app, &[M, G, L, D, Space, Digit0, Digit2, Enter]);

        // Flash: the header plays for real (shortened leader), the
        // body is served through the intercepts — the whole load
        // takes barely over a second of emulated time.
        assert!(app.tape.flash, "flash load is the default");
        assert!(app.tape.request_play(0, &mut app.machine));
        assert!(app.machine.bus.tape.is_playing(), "deck is feeding");
        let mut frames = 0;
        while app.tape.is_playing() {
            frames += 1;
            assert!(frames < 300, "flash load did not finish");
            app.run_frames(1);
        }
        assert!(
            frames < 120,
            "flash load took {frames} frames — not much faster than real"
        );
        assert_eq!(app.machine.bus.memory.ram[0x1000..0x1000 + content.len()], content[..]);

        // A few frames later the machine is back at the monitor
        // prompt, not stuck in the tape wait.
        app.run_frames(60);
        assert!(app.machine.cpu.pc >= 0xE000, "left the monitor ROM");
        let mut wait_loop = 0;
        for _ in 0..50 {
            app.run_frames(1);
            if (0xE890..0xE8B0).contains(&app.machine.cpu.pc) {
                wait_loop += 1;
            }
        }
        assert_eq!(wait_loop, 0, "still stuck in the tape wait loop");
    }

    /// The full multi-block game flow in one: MGLD flash-loads a loader
    /// whose body covers the monitor stack (so the block read's RET
    /// jumps to the vector the body placed there — autorun), and the
    /// loader itself reads the continuation block through BLKLOAD.
    #[test]
    fn tape_flash_loads_multi_block_games_with_autorun() {
        let mut app = test_app();
        const HIJACK: usize = 0xBEFB; // the EDE2 return slot on the monitor-3 stack

        // A loader program at 0x1000: mark that it ran, then read the
        // continuation block (16 bytes) into 0x2000 through BLKLOAD
        // (EDC4), then report success and halt.
        let mut loader = vec![0u8; HIJACK + 2 - 0x1000];
        loader[0..2].copy_from_slice(&[0x3E, 0xA5]); // MVI A,A5
        loader[2..5].copy_from_slice(&[0x32, 0x00, 0x30]); // STA 3000
        loader[5..8].copy_from_slice(&[0x21, 0x00, 0x20]); // LXI HL,2000
        loader[8..11].copy_from_slice(&[0x11, 0x0F, 0x00]); // LXI DE,15
        loader[11..13].copy_from_slice(&[0x0E, 0x01]); // MVI C,1
        loader[13..16].copy_from_slice(&[0xCD, 0xC4, 0xED]); // CALL EDC4
        loader[16..19].copy_from_slice(&[0xD2, 0x40, 0x10]); // JNC 1040
        loader[19..21].copy_from_slice(&[0x3E, 0xEE]); // MVI A,EE (error)
        loader[21..24].copy_from_slice(&[0x32, 0x01, 0x30]); // STA 3001
        loader[24] = 0x76; // HLT
        loader[0x40..0x43].copy_from_slice(&[0x32, 0x01, 0x30]); // STA 3001
        loader[0x43] = 0x76; // HLT
        // Autorun vector: the block read's RET at EDE1 pops this word.
        loader[HIJACK - 0x1000] = 0x00;
        loader[HIJACK - 0x1000 + 1] = 0x10;

        let payload: Vec<u8> = (0..16u16).map(|i| 0x60 ^ i as u8).collect();
        app.tape
            .tape
            .blocks
            .push(tape::make_file(0, b'?', "AUTORUN", 0x1000, &loader).unwrap());
        let mut body = payload.clone();
        body.push(tape::crc8(&payload));
        app.tape.tape.blocks.push(tape::TapeBlock {
            header: None,
            header_bytes: Vec::new(),
            body_bytes: body,
            header_crc_ok: true,
            body_crc_ok: true,
            body_length_error: None,
            old_format: false,
        });

        app.run_frames(500);
        use Key::*;
        type_command(&mut app, &[M, G, L, D, Space, Digit0, Digit0, Enter]);
        assert!(app.tape.request_play(0, &mut app.machine));
        let mut frames = 0;
        while app.tape.is_playing() {
            frames += 1;
            assert!(frames < 300, "flash load did not finish");
            app.run_frames(1);
        }
        // The loader ran (via autorun), read the continuation block
        // through BLKLOAD, and halted on the success path.
        assert!(app.machine.cpu.halted, "the loader never halted");
        assert_eq!(app.machine.bus.memory.ram[0x3000], 0xA5, "autorun did not run");
        assert_eq!(app.machine.bus.memory.ram[0x3001], 0x00, "checksum mismatch");
        for (i, &expect) in payload.iter().enumerate() {
            assert_eq!(app.machine.bus.memory.ram[0x2000 + i], expect, "+{i}");
        }
    }

    #[test]
    fn machine_reset_clears_tape_playback() {
        let mut app = test_app();
        app.tape.flash = false; // exercise the real tape interface
        app.tape
            .tape
            .blocks
            .push(tape::make_file(1, b'?', "ANYFILE", 0x1000, &[0x76, 0]).unwrap());
        app.play_tape_file(0);
        assert!(app.machine.bus.tape.is_playing());
        app.machine.reset();
        let _ = app.tape.pump(&mut app.machine);
        assert!(!app.tape.is_playing(), "session survived the reset");
        assert!(!app.machine.bus.tape.is_playing());
    }

    #[test]
    fn tape_recording_is_harvested_into_the_editor() {
        let mut app = test_app();
        for _ in 0..500 {
            app.machine.step_frame();
        }
        let content: Vec<u8> = (0..17u16).map(|i| 0xC0 ^ i as u8).collect();
        for (i, &b) in content.iter().enumerate() {
            app.machine.bus.memory.ram[0x2000 + i] = b;
        }
        use Key::*;
        type_command(
            &mut app,
            &[
                M, G, S, V, Space, Digit0, Digit0, Space, Digit2, Digit0, Digit0, Digit0, Space,
                Digit2, Digit0, Digit1, Digit0, Enter,
            ],
        );
        for _ in 0..1500 {
            app.machine.step_frame();
            let _ = app.tape.pump(&mut app.machine);
            if !app.machine.bus.tape.is_recording() && !app.tape.tape.blocks.is_empty() {
                break;
            }
        }
        assert!(!app.machine.bus.tape.is_recording(), "recorder still active");
        assert_eq!(app.tape.files().len(), 1, "recorded file not harvested");
        assert!(app.tape.dirty);
        assert_eq!(app.tape.export_file(0).unwrap(), content);
    }

    // ----- save states -------------------------------------------------

    use std::path::PathBuf;

    /// A scratch settings dir of the test's own (tests run in
    /// parallel; the quick slot lives next to the settings).
    fn state_scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pmd85-app-state-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_and_load_state_round_trips() {
        let mut app = test_app();
        let dir = state_scratch_dir("roundtrip");
        let path = dir.join("state.pss");

        // Boot, leave a mark and take a state.
        app.run_frames(500);
        for i in 0..32u16 {
            app.machine.bus.memory.ram[0x3000 + i as usize] = (i ^ 0x99) as u8;
        }
        let pc_at_save = app.machine.cpu.pc;
        app.save_state_to(&path).expect("save while idle");

        // Move on, then come back.
        app.run_frames(400);
        assert_ne!(app.machine.cpu.pc, pc_at_save, "the machine never moved");
        app.load_state_from(&path).expect("load round-trips");

        assert_eq!(app.machine.cpu.pc, pc_at_save);
        for i in 0..32u16 {
            assert_eq!(
                app.machine.bus.memory.ram[0x3000 + i as usize],
                (i ^ 0x99) as u8
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quick_save_and_load_use_the_settings_dir() {
        let mut app = test_app();
        let dir = state_scratch_dir("quick");
        app.set_settings_dir(dir.clone());
        app.run_frames(500);

        // Quick load before anything was saved: a notification, no
        // panic, the machine untouched.
        let pc = app.machine.cpu.pc;
        app.quick_load_state();
        assert_eq!(
            app.notifications().back().map(|(m, _)| m.clone()),
            Some("State load failed: No such file or directory (os error 2)".into())
        );
        assert_eq!(app.machine.cpu.pc, pc);

        app.quick_save_state();
        assert!(dir.join("states/quick.pss").exists(), "quick slot missing");
        assert_eq!(
            app.notifications().back().map(|(m, _)| m.clone()),
            Some("State saved (quick slot)".into())
        );

        app.run_frames(300);
        app.quick_load_state();
        assert_eq!(app.machine.cpu.pc, pc, "quick load did not restore");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_state_is_refused_while_the_tape_runs() {
        let mut app = test_app();
        let dir = state_scratch_dir("tapebusy");
        let path = dir.join("state.pss");
        app.tape.flash = false;
        app.tape.tape.blocks.push(
            tape::make_file(0, b'?', "LOADTST", 0x1000, &[0x76, 1, 2, 3]).unwrap(),
        );

        app.run_frames(500);
        use Key::*;
        type_command(&mut app, &[M, G, L, D, Space, Digit0, Digit0, Enter]);
        app.play_tape_file(0);
        assert!(app.tape.is_playing());
        assert!(!app.can_save_state(), "save offered while the tape plays");

        // Both the file save and the quick slot are refused.
        assert!(app.save_state_to(&path).is_err());
        assert!(!path.exists());
        app.quick_save_state();
        assert!(
            !dir.join("states/quick.pss").exists(),
            "quick slot written while the tape plays"
        );
        assert!(
            app.notifications()
                .back()
                .map(|(m, _)| m.contains("tape"))
                .unwrap_or(false),
            "the refusal must explain itself"
        );

        // Once the session is over, saving works again.
        for _ in 0..2000 {
            if !app.tape.is_playing() {
                break;
            }
            app.run_frames(1);
        }
        assert!(app.can_save_state());
        app.save_state_to(&path).expect("save after the session ends");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_state_reports_corrupt_files_and_repaints() {        let mut app = test_app();
        let dir = state_scratch_dir("corrupt");
        let bad = dir.join("bad.pss");
        std::fs::write(&bad, b"definitely not a save state").unwrap();

        app.run_frames(500);
        let pc = app.machine.cpu.pc;
        let cycles = app.machine.bus.total_cycles();
        let err = app.load_state_from(&bad).unwrap_err();
        assert_eq!(err, "State: not a PMD 85 save state");
        assert_eq!(app.machine.cpu.pc, pc, "corrupt load mutated the machine");
        assert_eq!(app.machine.bus.total_cycles(), cycles);

        // Loading while paused repaints the screen immediately.
        app.set_running(false);
        assert!(app.decode_buf.is_empty(), "nothing decoded yet");
        let good = dir.join("good.pss");
        app.save_state_to(&good).unwrap();
        app.machine.bus.memory.ram[0xC000] ^= 0xFF; // scribble VRAM
        app.load_state_from(&good).unwrap();
        assert!(
            !app.decode_buf.is_empty(),
            "the screen was not decoded on load"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ----- session (remembered across restarts) ------------------------

    #[test]
    fn machine_config_changes_persist() {
        let mut app = test_app();
        let dir = state_scratch_dir("config");
        app.set_settings_dir(dir.clone());

        // Switch to a 85-2 with the default monitor: the choice must
        // become the boot configuration for the next start.
        let cfg = crate::app::MachineConfig {
            model: pmd85_core::Model::Pmd852,
            monitor: None,
            rom_module: None,
        };
        app.apply_config(cfg);
        assert_eq!(app.machine.model(), pmd85_core::Model::Pmd852);

        let loaded = crate::config::AppSettings::load(&dir);
        assert_eq!(
            pmd85_core::Model::from_str_loose(&loaded.model),
            Some(pmd85_core::Model::Pmd852)
        );
        assert_eq!(loaded.monitor, None);

        // A fresh app built from those settings boots the 85-2.
        let app2 = app_with_settings(loaded);
        assert_eq!(app2.config.model, pmd85_core::Model::Pmd852);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_round_trips_the_tape() {
        let mut app = test_app();
        let dir = state_scratch_dir("session");
        app.set_settings_dir(dir.clone());

        // A tape with unsaved edits, a path and a selection.
        let tape_path = dir.join("mygame.ptp");
        app.tape.tape.blocks.push(
            tape::make_file(0, b'?', "GAME", 0x1000, &[1, 2, 3, 4]).unwrap(),
        );
        app.tape.tape.blocks[0].body_bytes[1] ^= 0xFF; // an unsaved edit
        app.tape.path = Some(tape_path.clone());
        app.tape.dirty = true;
        app.tape.selected = Some(0);
        app.save_session();

        let mut app2 = test_app();
        app2.set_settings_dir(dir.clone());
        app2.restore_session();
        assert_eq!(app2.tape.tape.blocks.len(), 1);
        assert_eq!(app2.tape.tape.blocks[0].body_bytes, app.tape.tape.blocks[0].body_bytes);
        assert_eq!(app2.tape.path, Some(tape_path), "Save keeps writing there");
        assert!(app2.tape.dirty, "the dirty dot survived");
        assert_eq!(app2.tape.selected, Some(0));

        // An untitled tape (no path) round-trips too.
        app2.tape.clear();
        app2.tape.tape.blocks.push(
            tape::make_file(3, b'?', "SCRATCH", 0x2000, &[9]).unwrap(),
        );
        app2.tape.dirty = true;
        app2.save_session();
        let mut app3 = test_app();
        app3.set_settings_dir(dir.clone());
        app3.restore_session();
        assert_eq!(app3.tape.path, None);
        assert_eq!(app3.tape.files().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_or_missing_sessions_are_a_clean_start() {
        let mut app = test_app();
        let dir = state_scratch_dir("corrupt-session");
        app.set_settings_dir(dir.clone());

        // Nothing saved yet: clean, empty tape.
        app.restore_session();
        assert!(app.tape.tape.blocks.is_empty());
        assert_eq!(app.tape.path, None);

        // Garbage snapshot files must not panic or wedge the editor.
        std::fs::create_dir_all(dir.join("session")).unwrap();
        std::fs::write(dir.join("session/tape.ptp"), b"not a tape").unwrap();
        std::fs::write(dir.join("session/session.json"), "{not json").unwrap();
        app.restore_session();
        assert!(app.tape.tape.blocks.is_empty(), "corrupt tape snapshot ignored");
        assert_eq!(app.tape.path, None, "corrupt sidecar ignored");
        assert!(!app.tape.dirty);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
