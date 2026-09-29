//! Emulator application state: the machine, run state, speed/pacing,
//! audio, settings and the ROM catalog. UI-agnostic (the `ui` module
//! draws it), windowing-agnostic (main.rs drives it).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use pmd85_core::machine::CPU_CLOCK_HZ;
use pmd85_core::vram::{self, ColorProfile, HEIGHT, WIDTH};
use pmd85_core::{Machine, Model};

use crate::audio;
use crate::catalog::RomCatalog;
use crate::config::{self, AppSettings};
use crate::speed::{Pacer, SPEED_MAX, SPEED_MIN, TURBO_BUDGET};
use crate::ui::theme::Theme;
use crate::ui::UiState;

/// Emulated screen size (pixels).
pub const SCREEN: (usize, usize) = (WIDTH, HEIGHT);
/// How long a notification popup stays on screen.
const NOTIFICATION_TTL: Duration = Duration::from_secs(6);

/// Cold-start configuration of the machine.
#[derive(Clone, Debug, PartialEq)]
pub struct MachineConfig {
    pub model: Model,
    /// Monitor ROM: `None` = the model's default from the ROM
    /// directory; `Some(name)` resolves through the catalog, falling
    /// back to treating the string as a path.
    pub monitor: Option<String>,
    /// ROM module: `None` = none; `Some(name)` as above.
    pub rom_module: Option<String>,
}

impl MachineConfig {
    /// The machine this configuration will produce is not the one
    /// currently running (i.e. a restart is needed)?
    fn differs_from(&self, other: &MachineConfig) -> bool {
        self != other
    }
}

/// Emulation speed: a real-time multiplier plus a turbo toggle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeedState {
    /// Multiplier of real time (1.0 = 50 Hz as on hardware).
    pub multiplier: f64,
    /// Unthrottled: run as many frames as fit in the compute budget,
    /// audio muted.
    pub turbo: bool,
}

impl Default for SpeedState {
    fn default() -> Self {
        SpeedState {
            multiplier: 1.0,
            turbo: false,
        }
    }
}

/// The whole application state, minus windowing.
pub struct App {
    pub ctx: egui::Context,
    pub machine: Machine,
    /// Configuration the running machine was built from.
    pub config: MachineConfig,
    /// Working copy edited by the settings window.
    pub pending_config: MachineConfig,

    pub rom_dir: PathBuf,
    pub catalog: RomCatalog,

    pub profile: ColorProfile,
    pub audio: Option<audio::SpeakerOut>,

    pub running: bool,
    pub speed: SpeedState,
    pub pacer: Pacer,

    /// Emulated frames since the last machine (re)start.
    pub frames_done: u64,

    pub ui: UiState,
    pub settings: AppSettings,
    pub themes: Vec<Theme>,
    /// Name of the theme actually applied (for the settings picker).
    pub active_theme: String,

    /// Emulated screen texture (created lazily on first UI pass).
    pub screen: Option<egui::TextureHandle>,
    pub decode_buf: Vec<u8>,

    /// Transient error/info popups.
    notifications: VecDeque<(String, Instant)>,
    /// Where persisted settings are written (`None` = do not persist).
    settings_dir: Option<PathBuf>,
    /// Rendered frames in the last second (UI fps).
    render_times: VecDeque<Instant>,
    /// (time, total frames) samples for the emulated-speed readout.
    frame_samples: VecDeque<(Instant, u64)>,
}

impl App {
    /// Build the app state. The machine is constructed from the merged
    /// CLI/settings configuration; a missing ROM is not fatal (the
    /// machine comes up with a blank monitor and a notification).
    pub fn new(
        ctx: egui::Context,
        args: &crate::args::Args,
        settings: AppSettings,
    ) -> Self {
        let rom_dir = args
            .rom_dir
            .clone()
            .unwrap_or_else(default_rom_dir);
        let catalog = RomCatalog::scan(&rom_dir);

        let model = args
            .model
            .or_else(|| {
                // CLI wins; otherwise the persisted setting.
                Model::from_str_loose(&settings.model)
            })
            .unwrap_or_default();
        let rom_module = args.rom_module.clone().or_else(|| settings.rom_module.clone());
        let config = MachineConfig {
            model,
            monitor: args.monitor.clone(),
            rom_module,
        };

        let mut app = App {
            ctx,
            // Placeholder until rebuild_machine below; sized to satisfy
            // the monitor-size invariant.
            machine: {
                let blank = vec![0u8; model.monitor_size()];
                Machine::new(model, &blank, None)
            },
            config: config.clone(),
            pending_config: config.clone(),
            rom_dir,
            catalog,
            profile: ColorProfile::Rgb,
            audio: None,
            running: true,
            speed: SpeedState {
                multiplier: settings.speed.clamp(SPEED_MIN, SPEED_MAX),
                turbo: false,
            },
            pacer: Pacer::new(),
            frames_done: 0,
            ui: UiState::default(),
            settings,
            themes: Vec::new(),
            active_theme: String::new(),
            screen: None,
            decode_buf: Vec::new(),
            notifications: VecDeque::new(),
            settings_dir: config::config_dir(),
            render_times: VecDeque::new(),
            frame_samples: VecDeque::new(),
        };

        // Themes: built-ins plus custom files from the themes dir.
        app.themes = Theme::presets();
        if let Some(dir) = config::themes_dir() {
            app.themes.extend(Theme::load_dir(&dir));
        }
        let theme = app
            .themes
            .iter()
            .find(|t| t.name == app.settings.theme)
            .cloned()
            .unwrap_or_else(Theme::default);
        app.active_theme = theme.name.clone();
        Theme::install_fonts(&app.ctx);
        theme.apply(&app.ctx);

        // Audio: open the device unless muted from the start.
        if !args.mute && !app.settings.mute {
            app.audio = audio::SpeakerOut::new();
        }

        // The real machine from the configuration.
        app.rebuild_machine(&config);
        app.running = true;
        app
    }
    /// Rebuild the machine from a cold-start configuration. Never
    /// panics: load failures produce a notification and a blank ROM.
    pub fn rebuild_machine(&mut self, config: &MachineConfig) {
        let monitor =
            self.load_rom(&config.monitor, config.model.default_monitor(), config.model);
        let module = config
            .rom_module
            .as_ref()
            .map(|name| self.load_rom(&Some(name.clone()), name, config.model).0);
        let ((monitor, monitor_missing), module) = (monitor, module);
        self.machine = Machine::new(config.model, &monitor, module);
        self.config = config.clone();
        self.pending_config = config.clone();
        self.frames_done = 0;
        self.pacer.resync();
        self.frame_samples.clear();
        // The new machine's cycle counter starts at 0 again; the
        // speaker timeline must follow or it renders nothing ever again.
        if let Some(audio) = &mut self.audio {
            audio.resync();
        }
        if monitor_missing {
            self.notify("Monitor ROM could not be loaded - machine will not boot");
        }
    }

    /// Resolve a ROM reference to (bytes, missing). `None` falls back to
    /// `fallback` (a file name in the ROM directory). A name that is
    /// not in the catalog is treated as a path.
    fn load_rom(
        &self,
        name: &Option<String>,
        fallback: &str,
        model: Model,
    ) -> (Vec<u8>, bool) {
        let name = name.as_deref().unwrap_or(fallback);
        let path = match self.catalog.find(name) {
            Some(entry) => entry.path.clone(),
            None => self.rom_dir.join(name),
        };
        match std::fs::read(&path) {
            Ok(bytes) => (bytes, false),
            Err(e) => {
                log::warn!("cannot read ROM {name:?} ({}): {e}", path.display());
                let size = if name.to_ascii_lowercase().ends_with(".rom") {
                    model.monitor_size()
                } else {
                    0
                };
                (vec![0u8; size], true)
            }
        }
    }

    /// Apply a configuration change (settings window "restart").
    pub fn apply_config(&mut self, config: MachineConfig) {
        if config.differs_from(&self.config) {
            self.rebuild_machine(&config);
            self.running = true;
            self.persist();
        }
    }

    // ----- transport -----

    pub fn set_running(&mut self, running: bool) {
        if running && !self.running {
            self.pacer.resync();
        }
        self.running = running;
    }

    /// Stop: reset the machine and hold it at the power-on state.
    pub fn stop(&mut self) {
        self.machine.reset();
        self.frames_done = 0;
        self.set_running(false);
    }

    /// Reset: warm reboot (CPU, memory maps, peripherals; RAM
    /// preserved), keeping the run state.
    pub fn reset(&mut self) {
        self.machine.reset();
        self.frames_done = 0;
        self.pacer.resync();
    }

    pub fn set_multiplier(&mut self, multiplier: f64) {
        self.speed.multiplier = multiplier.clamp(SPEED_MIN, SPEED_MAX);
        self.settings.speed = self.speed.multiplier;
        self.persist();
    }

    pub fn set_turbo(&mut self, turbo: bool) {
        if self.speed.turbo && !turbo {
            // Leaving turbo: drop the wall-clock debt.
            self.pacer.resync();
        }
        self.speed.turbo = turbo;
    }

    pub fn set_mute(&mut self, mute: bool) {
        self.settings.mute = mute;
        self.persist();
    }

    // ----- emulation -----

    /// Run the emulation for this render frame, feed the speaker, and
    /// update the screen texture. Called once per render frame.
    pub fn advance(&mut self) {
        let frames = if self.running {
            if self.speed.turbo {
                let start = Instant::now();
                let mut frames = 0u64;
                while start.elapsed() < TURBO_BUDGET {
                    self.machine.step_frame();
                    frames += 1;
                }
                frames
            } else {
                let frames = self.pacer.take_frames(self.speed.multiplier) as u64;
                for _ in 0..frames {
                    self.machine.step_frame();
                }
                frames
            }
        } else {
            // While paused keep the pacing anchored so resuming does
            // not fast-forward through the pause.
            self.pacer.resync();
            0
        };
        self.frames_done += frames;

        // Speaker: only at exactly 1x real time; otherwise drain (the
        // edge log is bounded either way).
        let edges = self.machine.take_speaker_edges();
        if self.wants_audio() {
            if let Some(audio) = &mut self.audio {
                audio.submit(edges, self.machine.bus.total_cycles());
            }
        }

        // Screen texture: decode VRAM and upload.
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [SCREEN.0, SCREEN.1],
            vram::decode_into(&self.machine.bus.memory, self.profile, &mut self.decode_buf),
        );
        match &mut self.screen {
            Some(tex) => tex.set_partial(
                [0, 0],
                image,
                egui::TextureOptions::NEAREST,
            ),
            None => {
                self.screen = Some(self.ctx.load_texture(
                    "pmd85-screen",
                    image,
                    egui::TextureOptions::NEAREST,
                ));
            }
        }
    }

    /// Whether speaker audio should be submitted this frame: only at
    /// exactly 1x real time (otherwise tones would play at the wrong
    /// pitch relative to wall time).
    pub fn wants_audio(&self) -> bool {
        self.running
            && !self.settings.mute
            && !self.speed.turbo
            && (self.speed.multiplier - 1.0).abs() < 1e-9
    }

    // ----- pacing for the event loop -----

    /// Whether the emulator wants a redraw right now (frame due).
    ///
    /// Also true while paused/stopped: the event loop then wakes lazily
    /// (~10 fps, plus on every input event), and those repaints keep
    /// the UI responsive so the machine can be unpaused again.
    pub fn wants_redraw(&self, now: Instant) -> bool {
        if !self.running {
            return true;
        }
        self.speed.turbo || now >= self.pacer.next_due()
    }

    /// When the event loop should wake up next.
    pub fn next_wake(&self, now: Instant) -> Instant {
        if self.running {
            if self.speed.turbo {
                now
            } else {
                self.pacer.next_due()
            }
        } else {
            // Paused/stopped: lazy UI refresh.
            now + Duration::from_millis(100)
        }
    }

    /// Bookkeeping after presenting a frame (fps meters).
    pub fn on_frame_presented(&mut self) {
        let now = Instant::now();
        while self
            .render_times
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(1))
        {
            self.render_times.pop_front();
        }
        self.render_times.push_back(now);

        self.frame_samples.push_back((now, self.frames_done));
        while self
            .frame_samples
            .front()
            .is_some_and(|(t, _)| now.duration_since(*t) > Duration::from_secs(1))
        {
            self.frame_samples.pop_front();
        }

        // Expire notifications.
        self.notifications
            .retain(|(_, t)| now.duration_since(*t) < NOTIFICATION_TTL);
    }

    /// Rendered frames per second (UI).
    pub fn render_fps(&self) -> f64 {
        match (self.render_times.front(), self.render_times.back()) {
            (Some(first), Some(last)) if first != last => {
                let secs = last.duration_since(*first).as_secs_f64().max(1e-9);
                (self.render_times.len() - 1) as f64 / secs
            }
            _ => 0.0,
        }
    }

    /// Emulated frames per second (averaged over the last second).
    pub fn emulated_fps(&self) -> f64 {
        let (Some((t0, f0)), Some((t1, f1))) =
            (self.frame_samples.front().copied(), self.frame_samples.back().copied())
        else {
            return 0.0;
        };
        let frames = f1.saturating_sub(f0) as f64;
        let secs = t1.duration_since(t0).as_secs_f64().max(1e-9);
        frames / secs
    }

    /// Emulated wall time since (re)start, as (h, m, s).
    pub fn emulated_time(&self) -> (u64, u64, u64) {
        let secs = self.machine.bus.total_cycles() / CPU_CLOCK_HZ;
        (secs / 3600, (secs / 60) % 60, secs % 60)
    }

    // ----- misc -----

    pub fn notify(&mut self, message: impl Into<String>) {
        let message: String = message.into();
        log::warn!("{message}");
        self.notifications.push_back((message, Instant::now()));
        // Keep the queue bounded even if nothing expires.
        while self.notifications.len() > 5 {
            self.notifications.pop_front();
        }
    }

    pub fn notifications(&self) -> &VecDeque<(String, Instant)> {
        &self.notifications
    }

    /// Set and apply a theme by index in the theme list.
    pub fn set_theme(&mut self, index: usize) {
        if let Some(theme) = self.themes.get(index).cloned() {
            self.active_theme = theme.name.clone();
            self.settings.theme = theme.name.clone();
            theme.apply(&self.ctx);
            self.persist();
        }
    }

    /// The currently applied theme (clone; used by UI panels that
    /// need direct palette access).
    pub fn active_theme_data(&self) -> Theme {
        self.themes
            .iter()
            .find(|t| t.name == self.active_theme)
            .cloned()
            .unwrap_or_default()
    }

    /// Apply and remember a modified theme (in-place customization).
    /// The theme replaces any same-name entry in the theme list so the
    /// settings picker reflects the change.
    pub fn apply_theme(&mut self, theme: Theme) {
        if let Some(slot) = self.themes.iter_mut().find(|t| t.name == theme.name) {
            *slot = theme.clone();
        } else {
            self.themes.push(theme.clone());
        }
        self.active_theme = theme.name.clone();
        self.settings.theme = theme.name.clone();
        theme.apply(&self.ctx);
        self.persist();
    }

    /// Write the persisted settings (model, mute, theme, speed...).
    pub fn persist(&self) {
        if let Some(dir) = &self.settings_dir {
            self.settings.save(dir);
        }
    }

    /// Point persisted settings at `dir` (tests write to a scratch
    /// directory instead of the user's real config).
    #[cfg(test)]
    pub(crate) fn set_settings_dir(&mut self, dir: PathBuf) {
        self.settings_dir = Some(dir);
    }
}

/// Default ROM directory: the repo's `Rom/` next to the crate.
pub fn default_rom_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Rom")
}
