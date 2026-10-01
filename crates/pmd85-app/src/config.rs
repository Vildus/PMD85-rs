//! Persistent application settings, stored as JSON in the user config
//! directory (`~/.config/pmd85/` on Linux via the `directories` crate).
//!
//! CLI arguments override the persisted values for that run; changes
//! made in the UI are written back immediately (the file is small).

use std::fs;
use std::path::{Path, PathBuf};

use pmd85_core::Model;
use serde::{Deserialize, Serialize};

/// Settings that survive an app restart.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Model to boot (`Model::from_str_loose` form, e.g. `"85-3"`).
    pub model: String,
    /// Monitor ROM file name to boot with (`None` = the model's
    /// default from the ROM directory).
    pub monitor: Option<String>,
    /// ROM module file name to attach at boot (in the ROM directory).
    pub rom_module: Option<String>,
    /// Speaker muted.
    pub mute: bool,
    /// Theme name (built-in preset or a custom theme in the themes dir).
    pub theme: String,
    /// Speed multiplier (1.0 = real time).
    pub speed: f64,
    /// Play the tape data tone through the speaker (tape monitor).
    pub tape_monitor: bool,
    /// Stop tape playback at the next file header.
    pub tape_autostop: bool,
    /// Flash-load played files: the monitor's tape read loops are
    /// intercepted and fed the block data directly (settings key kept
    /// from the old "warp loads" for compatibility).
    pub tape_warp: bool,
    /// Draw the custom titlebar (minimize/maximize/close buttons,
    /// drag) instead of the system window frame.
    pub custom_titlebar: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            model: Model::Pmd853.name().to_string(),
            monitor: None,
            rom_module: None,
            mute: false,
            theme: crate::ui::theme::DEFAULT_THEME.to_string(),
            speed: 1.0,
            tape_monitor: true,
            tape_autostop: true,
            tape_warp: true,
            custom_titlebar: true,
        }
    }
}

/// Where persistent state lives. `None` when the platform does not
/// provide a home directory (rare); settings are then simply not
/// persisted.
pub fn config_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "pmd85")
        .map(|dirs| dirs.config_dir().to_path_buf())
}

/// Subdirectory holding custom theme files.
pub fn themes_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join("themes"))
}

/// The part of the session that is not the tape image itself:
/// what the tape editor looked like when the app exited.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    /// File the tape was last saved to (`None` = untitled).
    pub tape_path: Option<PathBuf>,
    /// Unsaved edits since the last load/save.
    pub tape_dirty: bool,
    /// Selected file in the tape editor.
    pub selected: Option<usize>,
}

impl SessionState {
    /// Load from `session.json` in `dir`, falling back to defaults
    /// (and logging) on a missing or corrupt file.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("session.json");
        match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<SessionState>(&text) {
                Ok(state) => return state,
                Err(e) => log::warn!("cannot parse {}: {e}", path.display()),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("cannot read {}: {e}", path.display()),
        }
        SessionState::default()
    }

    /// Write to `session.json` in `dir`. Failures are logged, not fatal.
    pub fn save(&self, dir: &Path) {
        let path = dir.join("session.json");
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(e) = fs::create_dir_all(dir).and_then(|()| fs::write(&path, text)) {
                    log::warn!("cannot write {}: {e}", path.display());
                }
            }
            Err(e) => log::warn!("cannot serialize the session: {e}"),
        }
    }
}

impl AppSettings {
    /// Load from `settings.json` in `dir`, falling back to defaults
    /// (and logging) on a missing or corrupt file.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("settings.json");
        match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<AppSettings>(&text) {
                Ok(settings) => return settings,
                Err(e) => log::warn!("cannot parse {}: {e}", path.display()),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("cannot read {}: {e}", path.display()),
        }
        AppSettings::default()
    }

    /// Write to `settings.json` in `dir`. Failures are logged, not fatal.
    pub fn save(&self, dir: &Path) {
        let path = dir.join("settings.json");
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(e) = fs::create_dir_all(dir).and_then(|()| fs::write(&path, text)) {
                    log::warn!("cannot write {}: {e}", path.display());
                }
            }
            Err(e) => log::warn!("cannot serialize settings: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pmd85-settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn defaults_are_valid() {
        let s = AppSettings::default();
        assert_eq!(Model::from_str_loose(&s.model), Some(Model::Pmd853));
        assert_eq!(s.speed, 1.0);
        assert!(!s.mute);
        assert!(s.tape_monitor);
        assert!(s.tape_autostop);
        assert!(s.tape_warp);
    }

    #[test]
    fn round_trip_through_disk() {
        let dir = test_dir("roundtrip");
        let s = AppSettings {
            model: "85-2A".into(),
            monitor: Some("monit2A.rom".into()),
            rom_module: Some("basic2A.rmm".into()),
            mute: true,
            theme: "Amber Terminal".into(),
            speed: 5.0,
            tape_monitor: false,
            tape_autostop: false,
            tape_warp: false,
            custom_titlebar: false,
        };
        s.save(&dir);
        let loaded = AppSettings::load(&dir);
        assert_eq!(loaded, s);
        assert_eq!(Model::from_str_loose(&loaded.model), Some(Model::Pmd852a));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = test_dir("missing");
        assert_eq!(AppSettings::load(&dir), AppSettings::default());
    }

    #[test]
    fn corrupt_file_gives_defaults() {
        let dir = test_dir("corrupt");
        fs::write(dir.join("settings.json"), "{not json").unwrap();
        assert_eq!(AppSettings::load(&dir), AppSettings::default());
    }

    #[test]
    fn partial_file_keeps_defaults_for_missing_fields() {
        let dir = test_dir("partial");
        fs::write(dir.join("settings.json"), r#"{"mute": true}"#).unwrap();
        let s = AppSettings::load(&dir);
        assert!(s.mute);
        assert_eq!(Model::from_str_loose(&s.model), Some(Model::Pmd853));
        assert_eq!(s.speed, 1.0);
        assert_eq!(s.monitor, None, "monitor defaults to the model's own");
        // New settings keep their defaults when absent from the file.
        assert!(s.tape_monitor);
        assert!(s.tape_autostop);
        assert!(s.tape_warp);
    }
}
