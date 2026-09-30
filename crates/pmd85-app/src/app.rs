//! Emulator application state: the machine, run state, speed/pacing,
//! audio, settings and the ROM catalog. UI-agnostic (the `ui` module
//! draws it), windowing-agnostic (main.rs drives it).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pmd85_core::machine::CPU_CLOCK_HZ;
use pmd85_core::tape::{self, Tape};
use pmd85_core::tapedeck::PlayItem;
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

/// A file on the tape: its block (the header + body pair) plus any
/// headerless continuation blocks that follow it (pictures, saved
/// positions).
#[derive(Clone, Debug)]
pub struct TapeFile {
    /// Index of the file's block in `Tape::blocks`. The block has no
    /// header for a headerless run with no file ahead of it (e.g. a
    /// truncated tape).
    pub block: usize,
    /// Headerless continuation block indices, in tape order.
    pub continuations: Vec<usize>,
}

impl TapeFile {
    /// Contiguous block-index span `[start, start + len)` of the file.
    fn span(&self) -> (usize, usize) {
        let end = self.continuations.last().copied().unwrap_or(self.block);
        (self.block, end - self.block + 1)
    }
}

/// Cassette tape editor and transport state: the tape image being
/// edited, the selected file and the playback session feeding the
/// machine. Also harvests blocks the machine records.
#[derive(Debug)]
pub struct TapeState {
    /// The tape image being edited (empty = no tape).
    pub tape: Tape,
    /// File the tape was loaded from / was last saved to.
    pub path: Option<PathBuf>,
    /// Unsaved edits since the last load/save.
    pub dirty: bool,
    /// Selected file, an index into [`TapeState::files`].
    pub selected: Option<usize>,
    /// Stop playback after the selected file's blocks (at the next
    /// file header); when off, the rest of the tape follows.
    pub auto_stop: bool,
    /// Fast-load played files: body blocks are served straight to the
    /// monitor's read loops instead of through the tape signal, so
    /// MGLD (and multi-block loaders) finish in a fraction of the
    /// time while checksums, filters and autorun still run for real.
    pub flash: bool,
    /// Active playback session (the block queue itself lives in the
    /// machine's deck, which advances blocks synchronously).
    play: bool,
    /// The recorder was active at the previous pump (edge detector
    /// for harvesting the recorded stream).
    was_recording: bool,
}

impl Default for TapeState {
    fn default() -> Self {
        TapeState {
            tape: Tape::default(),
            path: None,
            dirty: false,
            selected: None,
            auto_stop: true,
            flash: true,
            play: false,
            was_recording: false,
        }
    }
}

impl TapeState {
    /// The tape's blocks grouped into files, in tape order.
    pub fn files(&self) -> Vec<TapeFile> {
        let mut files: Vec<TapeFile> = Vec::new();
        for (i, block) in self.tape.blocks.iter().enumerate() {
            if block.header.is_some() || files.is_empty() {
                // A new file, or a headerless run at the tape start
                // with no file ahead of it.
                files.push(TapeFile {
                    block: i,
                    continuations: Vec::new(),
                });
            } else {
                files.last_mut().unwrap().continuations.push(i);
            }
        }
        files
    }

    /// Load a `.ptp`/`.pmd` image, replacing the current tape. The
    /// caller is responsible for stopping playback first.
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        let tape = tape::load(path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.play = false;
        self.tape = tape;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        self.selected = None;
        Ok(())
    }

    /// Start a fresh empty tape. The caller stops playback first.
    pub fn clear(&mut self) {
        self.play = false;
        self.tape = Tape::default();
        self.path = None;
        self.dirty = false;
        self.selected = None;
    }

    /// Save to the tape's current path.
    pub fn save(&mut self) -> Result<(), String> {
        let path = self.path.clone().ok_or("no file chosen yet")?;
        self.save_as(&path)
    }

    /// Save the tape as a PTP image to `path`.
    pub fn save_as(&mut self, path: &Path) -> Result<(), String> {
        tape::save(path, &self.tape).map_err(|e| format!("{}: {e}", path.display()))?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        Ok(())
    }

    /// Append a new file built from raw content (import). Returns
    /// the new file's index.
    pub fn import_file(
        &mut self,
        number: u8,
        block_type: u8,
        name: &str,
        start: u16,
        content: &[u8],
    ) -> Result<usize, String> {
        let block = tape::make_file(number, block_type, name, start, content)?;
        self.tape.blocks.push(block);
        self.dirty = true;
        let index = self.files().len() - 1;
        self.selected = Some(index);
        Ok(index)
    }

    /// The loadable content of a file (its block's body payload).
    pub fn export_file(&self, file: usize) -> Option<Vec<u8>> {
        let files = self.files();
        Some(self.tape.blocks.get(files.get(file)?.block)?.content().to_vec())
    }

    /// Delete a file: its block and continuation blocks.
    pub fn delete_file(&mut self, file: usize) {
        let files = self.files();
        let Some(f) = files.get(file) else {
            return;
        };
        let (start, len) = f.span();
        for i in (start..start + len).rev() {
            self.tape.blocks.remove(i);
        }
        self.dirty = true;
        let count = self.files().len();
        self.selected = self.selected.map(|s| s.min(count.saturating_sub(1)));
    }

    /// Move a file one place up (`delta < 0`) or down the tape.
    pub fn move_file(&mut self, file: usize, delta: i32) {
        let delta = delta.signum();
        let files = self.files();
        let target = file as i64 + delta as i64;
        if !((0..files.len() as i64).contains(&target)) {
            return;
        }
        let (m_start, m_len) = files[file].span();
        let (o_start, o_len) = files[target as usize].span();
        let moved: Vec<_> = self.tape.blocks.drain(m_start..m_start + m_len).collect();
        // After the drain the neighbour below shifted up by m_len;
        // the neighbour above did not move.
        let insert_at = if delta > 0 {
            o_start + o_len - m_len
        } else {
            o_start
        };
        self.tape
            .blocks
            .splice(insert_at..insert_at, moved);
        self.dirty = true;
        self.selected = Some(target as usize);
    }

    /// Request playback of `file`: its header, body and continuation
    /// blocks; with `auto_stop` off the rest of the tape follows. The
    /// blocks are queued in the machine's deck, which feeds them one
    /// after another. With `flash` set, body blocks are served through
    /// the flash-load intercepts instead of the tape signal. Returns
    /// whether playback started.
    pub fn request_play(&mut self, file: usize, machine: &mut Machine) -> bool {
        let files = self.files();
        let Some(f) = files.get(file) else {
            return false;
        };
        self.selected = Some(file);
        let flash = self.flash;
        let mut rows: Vec<(usize, bool)> = Vec::new();
        if self.tape.blocks[f.block].header.is_some() {
            rows.push((f.block, true));
        }
        rows.push((f.block, false));
        for &c in &f.continuations {
            rows.push((c, false));
        }
        if !self.auto_stop {
            let (_, len) = f.span();
            for i in f.block + len..self.tape.blocks.len() {
                if self.tape.blocks[i].header.is_some() {
                    rows.push((i, true));
                }
                rows.push((i, false));
            }
        }
        if rows.is_empty() {
            return false;
        }
        let queue: Vec<PlayItem> = rows
            .into_iter()
            .map(|(idx, head)| {
                let block = &self.tape.blocks[idx];
                PlayItem {
                    data: if head {
                        block.header_bytes.clone()
                    } else {
                        block.body_bytes.clone()
                    },
                    head,
                    flash,
                }
            })
            .collect();
        machine.bus.tape_play_session(queue);
        self.play = true;
        true
    }

    /// Stop playback (nothing more is fed to the machine).
    pub fn stop_playback(&mut self, machine: &mut Machine) {
        self.play = false;
        machine.bus.tape_stop();
    }

    /// Whether a playback session is active.
    pub fn is_playing(&self) -> bool {
        self.play
    }

    /// Feed the machine from the playback session and harvest
    /// recorded blocks. Called once per emulated frame (and once per
    /// rendered frame while paused); returns a user notification when
    /// something was recorded.
    pub fn pump(&mut self, machine: &mut Machine) -> Option<String> {
        // A session that played to the end, or that the machine
        // reset out from under us.
        if self.play && !machine.bus.tape.is_playing() {
            self.play = false;
        }

        // Recording: harvest the stream once the recorder goes quiet.
        let recording = machine.bus.tape.is_recording();
        let mut notification = None;
        if self.was_recording && !recording {
            let recorded = machine.bus.tape.take_recorded();
            let before = self.tape.blocks.len();
            self.tape.append_ptp_stream(&recorded);
            let added = self.tape.blocks.len() - before;
            if added > 0 {
                self.dirty = true;
                notification = Some(format!(
                    "Tape: recorded {added} block{} from the machine",
                    if added == 1 { "" } else { "s" }
                ));
            }
        }
        self.was_recording = recording;
        notification
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

    /// Cassette tape editor/deck state.
    pub tape: TapeState,

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
            tape: TapeState {
                auto_stop: settings.tape_autostop,
                flash: settings.tape_warp,
                ..TapeState::default()
            },
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

    // ----- tape -----

    /// Open a tape image in the editor, stopping playback first.
    /// Parse warnings become notifications.
    pub fn open_tape(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.tape.stop_playback(&mut self.machine);
        self.tape.open(path)?;
        let warnings = self.tape.tape.warnings.join("; ");
        if !warnings.is_empty() {
            self.notify(format!("Tape: {warnings}"));
        }
        Ok(())
    }

    /// Start a fresh empty tape, stopping playback first.
    pub fn new_tape(&mut self) {
        self.tape.stop_playback(&mut self.machine);
        self.tape.clear();
    }

    /// Begin playing tape file `file` into the machine (the blocks are
    /// queued in the deck immediately).
    pub fn play_tape_file(&mut self, file: usize) {
        let flash = self.tape.flash;
        if self.tape.request_play(file, &mut self.machine) && flash {
            let name = self
                .tape
                .files()
                .get(file)
                .and_then(|f| self.tape.tape.blocks.get(f.block))
                .and_then(|b| b.header.as_ref())
                .map(|h| h.name_str())
                .unwrap_or_else(|| "file".into());
            self.notify(format!("Tape: flash-loading {name}"));
        }
        let _ = self.tape.pump(&mut self.machine);
    }

    /// Stop tape playback.
    pub fn stop_tape_playback(&mut self) {
        self.tape.stop_playback(&mut self.machine);
    }

    /// Toggle the auto-stop-at-next-header behavior and persist it.
    pub fn set_tape_autostop(&mut self, auto_stop: bool) {
        self.tape.auto_stop = auto_stop;
        self.settings.tape_autostop = auto_stop;
        self.persist();
    }

    /// Toggle the tape data-tone monitor and persist it.
    pub fn set_tape_monitor(&mut self, on: bool) {
        self.settings.tape_monitor = on;
        self.persist();
    }

    /// Toggle flash loading and persist it.
    pub fn set_tape_flash(&mut self, on: bool) {
        self.tape.flash = on;
        self.settings.tape_warp = on;
        self.persist();
    }

    // ----- emulation -----

    /// Run `frames` emulated frames, pumping the tape deck between
    /// them. The pump must run per *emulated* frame, not per rendered
    /// frame: at a speedup several emulated frames pass per render
    /// frame, and the deck's block transitions (and the session
    /// bookkeeping around them) must stay in lockstep with the
    /// machine or loads corrupt.
    pub(crate) fn run_frames(&mut self, frames: u64) {
        for _ in 0..frames {
            self.machine.step_frame();
            if let Some(message) = self.tape.pump(&mut self.machine) {
                self.notify(message);
            }
        }
    }

    /// Run the emulation for this render frame, feed the speaker, and
    /// update the screen texture. Called once per render frame.
    pub fn advance(&mut self) {
        let frames = if self.running {
            if self.speed.turbo {
                let start = Instant::now();
                let mut frames = 0u64;
                while start.elapsed() < TURBO_BUDGET {
                    self.run_frames(1);
                    frames += 1;
                }
                frames
            } else {
                let frames = self.pacer.take_frames(self.speed.multiplier) as u64;
                self.run_frames(frames);
                frames
            }
        } else {
            // While paused keep the pacing anchored so resuming does
            // not fast-forward through the pause.
            self.pacer.resync();
            if let Some(message) = self.tape.pump(&mut self.machine) {
                self.notify(message);
            }
            0
        };
        self.frames_done += frames;

        // Speaker (and the tape data-tone monitor, when enabled):
        // only at exactly 1x real time; otherwise drain (the edge logs
        // are bounded either way).
        let edges = self.machine.take_speaker_edges();
        let tape_edges = self.machine.bus.tape.take_monitor_edges();
        if self.wants_audio() {
            if let Some(audio) = &mut self.audio {
                if self.settings.tape_monitor {
                    audio.submit_mixed(edges, tape_edges, self.machine.bus.total_cycles());
                } else {
                    audio.submit(edges, self.machine.bus.total_cycles());
                }
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
