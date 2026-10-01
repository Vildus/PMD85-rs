//! Desktop application for the Tesla PMD 85 emulator.
//!
//! Owns a winit window painted by egui (via `egui-wgpu`); the emulated
//! screen is an egui texture updated from VRAM each frame. Emulation
//! pacing, speed control and audio live in [`app::App`].

mod app;
mod args;
mod audio;
mod catalog;
mod config;
mod icon;
mod keys;
mod speed;
mod ui;

use std::sync::Arc;
use std::time::Instant;

use egui::{Context, ViewportId};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};

use crate::app::{App, WindowRequest};

/// The application id: the Wayland `app_id` and the X11 `WM_CLASS`,
/// which the taskbar matches against the installed `pmd85.desktop`
/// (see `dist/`) to resolve the icon.
pub const APP_ID: &str = "pmd85";

/// One OS window with its egui painter and input state.
struct Windowing {
    window: Arc<Window>,
    painter: egui_wgpu::winit::Painter,
    egui_winit: egui_winit::State,
}

struct Application {
    app: App,
    windowing: Option<Windowing>,
    /// Left Alt is held: the keyboard is in host-shortcut mode and no
    /// keys reach the machine until it is released.
    alt_held: bool,
    /// The previous frame handed the pointer to the compositor for an
    /// interactive move/resize, which consumes the button release:
    /// the next frame must give egui that release itself (see
    /// [`pointer_release_event`]).
    pointer_release_due: bool,
}

impl ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.windowing.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("PMD 85 \u{2014} Tesla")
            .with_inner_size(winit::dpi::LogicalSize::new(980, 760))
            .with_window_icon(icon::window_icon())
            // The custom titlebar replaces the system frame; the
            // system frame is drawn exactly when the custom bar is
            // off (the persisted setting).
            .with_decorations(!self.app.settings.custom_titlebar);
        // Name the app so desktop environments can identify the
        // window: on Wayland the `app_id`, on X11 the `WM_CLASS`
        // (one call sets both — the same winit field feeds each
        // backend). Taskbars resolve the icon by matching this
        // against the installed `pmd85.desktop` — Wayland ignores
        // the window icon set above entirely.
        #[cfg(target_os = "linux")]
        let attrs =
            winit::platform::wayland::WindowAttributesExtWayland::with_name(attrs, APP_ID, APP_ID);
        let window = Arc::new(event_loop.create_window(attrs).expect("cannot create window"));

        let ctx = self.app.ctx.clone();
        let mut painter = pollster::block_on(egui_wgpu::winit::Painter::new(
            ctx.clone(),
            egui_wgpu::WgpuConfiguration::default(),
            false,
            egui_wgpu::RendererOptions::default(),
        ));
        if let Err(e) = pollster::block_on(painter.set_window(ViewportId::ROOT, Some(window.clone()))) {
            panic!("cannot create wgpu surface: {e}");
        }
        let mut egui_winit = egui_winit::State::new(
            ctx,
            ViewportId::ROOT,
            window.as_ref() as &dyn raw_window_handle::HasDisplayHandle,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        if let Some(render_state) = painter.render_state() {
            egui_winit.set_max_texture_side(
                render_state.device.limits().max_texture_dimension_2d as usize,
            );
        }

        self.windowing = Some(Windowing {
            window,
            painter,
            egui_winit,
        });
        self.app.pacer.resync();
        event_loop.set_control_flow(ControlFlow::Poll);
        self.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(w) = &mut self.windowing else {
            return;
        };
        // egui sees every event first; what it does not consume may go
        // to the emulated machine.
        let response = w.egui_winit.on_window_event(&w.window, &event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let (Some(width), Some(height)) = (
                    std::num::NonZeroU32::new(size.width),
                    std::num::NonZeroU32::new(size.height),
                ) {
                    w.painter.on_window_resized(ViewportId::ROOT, width, height);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if !response.consumed {
                    // Left Alt is the host-key modifier; while it is
                    // held, the keyboard is in host-shortcut mode and
                    // no keys are forwarded to the machine.
                    if let PhysicalKey::Code(code) = event.physical_key {
                        if keys::is_host_modifier(code) {
                            self.alt_held = event.state.is_pressed();
                        }
                        if self.alt_held {
                            if event.state.is_pressed() && !event.repeat {
                                match keys::host_shortcut(code) {
                                    Some(keys::HostShortcut::QuickSave) => {
                                        self.app.quick_save_state()
                                    }
                                    Some(keys::HostShortcut::QuickLoad) => {
                                        self.app.quick_load_state()
                                    }
                                    Some(keys::HostShortcut::DebugStep) => {
                                        self.app.debug_step()
                                    }
                                    Some(keys::HostShortcut::DebugStepOver) => {
                                        self.app.debug_step_over()
                                    }
                                    Some(keys::HostShortcut::DebugContinue) => {
                                        self.app.debug_continue()
                                    }
                                    None => {}
                                }
                            }
                        } else {
                            for key in keys::map(&event) {
                                self.app
                                    .machine
                                    .bus
                                    .keyboard
                                    .set_key(*key, event.state.is_pressed());
                            }
                        }
                    }
                }
            }
            WindowEvent::Focused(false) => {
                self.alt_held = false;
                self.app.machine.bus.keyboard.reset();
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.windowing.is_none() {
            return;
        }
        let now = Instant::now();
        if self.app.wants_redraw(now) {
            self.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.app.next_wake(now)));
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // Remember the session (the tape) and the dock layout before
        // the process goes away; settings are already written on
        // every change.
        self.app.save_session();
        self.app.save_layout();
    }
}

impl Application {
    fn request_redraw(&self) {
        if let Some(w) = &self.windowing {
            w.window.request_redraw();
        }
    }

    /// One rendered frame: advance emulation, update the screen
    /// texture, build and paint the UI, schedule the next wakeup.
    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let Some(w) = &mut self.windowing else {
            return;
        };

        // 1. Emulation (real-time paced, turbo, or paused).
        self.app.advance();

        // 2. egui pass: input, UI, platform output.
        self.app.window_maximized = w.window.is_maximized();
        let mut raw_input = w.egui_winit.take_egui_input(&w.window);
        // The compositor that ran the last interactive move/resize
        // consumed the button release; give egui that release now,
        // at the pointer position it last saw, so its state machine
        // ends the drag cleanly (button up, pointer still in the
        // window — hover and cursors keep working).
        if self.pointer_release_due {
            self.pointer_release_due = false;
            if let Some((pos, modifiers)) = self
                .app
                .ctx
                .input(|i| i.pointer.latest_pos().map(|pos| (pos, i.modifiers)))
            {
                raw_input
                    .events
                    .push(pointer_release_event(pos, modifiers));
            }
        }
        let ctx = self.app.ctx.clone();
        let full_output = ctx.run_ui(raw_input, |ui| ui::draw(&mut self.app, ui));
        w.egui_winit
            .handle_platform_output(&w.window, full_output.platform_output);

        // Window management the UI asked for (titlebar buttons, drag,
        // resize edges). Applied right after the frame that requested
        // them, while the pointer press that motivates a drag/resize
        // is still held — Wayland requires its serial.
        for request in self.app.take_window_requests() {
            match request {
                WindowRequest::Drag => {
                    if let Err(e) = w.window.drag_window() {
                        log::warn!("cannot start a window drag: {e}");
                    }
                    // The compositor takes the pointer and consumes
                    // the button release — egui would keep thinking
                    // the button is held (no hover, no new drags)
                    // until some other click cleared it. The next
                    // frame replays the release it never saw.
                    self.pointer_release_due = true;
                }
                WindowRequest::Resize(direction) => {
                    if let Err(e) = w.window.drag_resize_window(direction) {
                        log::warn!("cannot start a window resize: {e}");
                    }
                    self.pointer_release_due = true;
                }
                WindowRequest::Minimize => w.window.set_minimized(true),
                WindowRequest::ToggleMaximize => {
                    w.window.set_maximized(!w.window.is_maximized())
                }
                WindowRequest::Decorate(on) => w.window.set_decorations(on),
                WindowRequest::Close => event_loop.exit(),
            }
        }

        // 3. Paint: tessellate and hand to the wgpu painter.
        let pixels_per_point = full_output.pixels_per_point;
        let clipped_primitives = ctx.tessellate(full_output.shapes, pixels_per_point);
        let mut textures_delta = full_output.textures_delta;
        let clear_color = clear_color(&self.app);
        w.painter.paint_and_update_textures(
            ViewportId::ROOT,
            pixels_per_point,
            clear_color,
            &clipped_primitives,
            &mut textures_delta,
            Vec::new(),
            &w.window,
        );

        // 4. Bookkeeping and pacing.
        self.app.on_frame_presented();
        let now = Instant::now();
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.app.next_wake(now)));
        if self.app.running && self.app.speed.turbo {
            self.request_redraw();
        }
    }
}

/// Window clear color from the active theme background.
fn clear_color(app: &App) -> [f32; 4] {
    let c = app.active_theme_data();
    [
        c.bg[0] as f32 / 255.0,
        c.bg[1] as f32 / 255.0,
        c.bg[2] as f32 / 255.0,
        c.bg[3] as f32 / 255.0,
    ]
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = args::parse();

    // Persisted settings, overridden by CLI arguments.
    let settings = config::config_dir()
        .map(|dir| config::AppSettings::load(&dir))
        .unwrap_or_default();

    let ctx = Context::default();
    let mut app = App::new(ctx, &args, settings);
    // The session (the tape that was loaded when the app last
    // exited) and the dock layout come back; a missing snapshot is a
    // clean start.
    app.restore_session();
    app.restore_layout();

    let event_loop = EventLoop::new().expect("cannot create event loop");
    let mut application = Application {
        app,
        windowing: None,
        alt_held: false,
        pointer_release_due: false,
    };
    event_loop.run_app(&mut application).expect("event loop failed");
}

/// The button-release event for a compositor-driven interactive
/// move/resize: the compositor consumes the real release, so this
/// replays it to egui — same button, released, at the pointer
/// position egui last saw, with the current keyboard modifiers.
fn pointer_release_event(pos: egui::Pos2, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped desktop entry stays in sync with the app id the
    /// window announces: the taskbar matches the two to resolve the
    /// icon (on Wayland it comes only from the desktop entry).
    #[test]
    fn desktop_entry_matches_the_app_id() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../dist/pmd85.desktop");
        let desktop = std::fs::read_to_string(path).expect("dist/pmd85.desktop exists");
        assert!(desktop.contains("Type=Application"));
        assert!(
            desktop.contains(&format!("StartupWMClass={APP_ID}")),
            "the taskbar matches the window's id"
        );
        assert!(
            desktop.contains(&format!("Icon={APP_ID}")),
            "the installed icon name"
        );
        assert!(
            desktop.contains("Exec=@BIN@"),
            "the placeholder dist/install-user.sh substitutes"
        );
    }

    /// The synthetic release the winit loop injects after a
    /// compositor drag/resize is exactly a primary-button release at
    /// the pointer's last known position.
    #[test]
    fn pointer_release_replays_a_primary_release() {
        let pos = egui::pos2(100.0, 200.0);
        let modifiers = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        match pointer_release_event(pos, modifiers) {
            egui::Event::PointerButton {
                pos: event_pos,
                button,
                pressed,
                modifiers: event_modifiers,
            } => {
                assert_eq!(event_pos, pos);
                assert_eq!(button, egui::PointerButton::Primary);
                assert!(!pressed, "the release is a release");
                assert!(event_modifiers.alt);
            }
            event => panic!("not a PointerButton event: {event:?}"),
        }
    }
}
