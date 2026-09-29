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
mod keys;
mod speed;
mod ui;

use std::sync::Arc;
use std::time::Instant;

use egui::{Context, ViewportId};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::app::App;

/// One OS window with its egui painter and input state.
struct Windowing {
    window: Arc<Window>,
    painter: egui_wgpu::winit::Painter,
    egui_winit: egui_winit::State,
}

struct Application {
    app: App,
    windowing: Option<Windowing>,
}

impl ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.windowing.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("PMD 85 \u{2014} Tesla")
            .with_inner_size(winit::dpi::LogicalSize::new(980, 760));
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
                    for key in keys::map(&event) {
                        self.app
                            .machine
                            .bus
                            .keyboard
                            .set_key(*key, event.state.is_pressed());
                    }
                }
            }
            WindowEvent::Focused(false) => {
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
        let raw_input = w.egui_winit.take_egui_input(&w.window);
        let ctx = self.app.ctx.clone();
        let full_output = ctx.run_ui(raw_input, |ui| ui::draw(&mut self.app, ui));
        w.egui_winit
            .handle_platform_output(&w.window, full_output.platform_output);

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
    let app = App::new(ctx, &args, settings);

    let event_loop = EventLoop::new().expect("cannot create event loop");
    let mut application = Application { app, windowing: None };
    event_loop.run_app(&mut application).expect("event loop failed");
}
