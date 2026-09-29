//! Catalog of available monitor ROMs and ROM modules.
//!
//! Scans the `Rom/` directory and joins the file list with the
//! human-written descriptions from `rom-list.txt` (the archive's own
//! inventory, kept in Slovak). Used by the settings window to offer
//! model/module pickers with real descriptions.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// One ROM file on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RomEntry {
    /// File name inside the ROM directory (`basic3.rmm`).
    pub file_name: String,
    /// Full path.
    pub path: PathBuf,
    /// File size in bytes.
    pub size: u64,
    /// Description from `rom-list.txt`, if the file is listed there
    /// (joined bullet lines, original wording).
    pub description: String,
}

impl RomEntry {
    /// Human-readable size ("10 kB").
    pub fn size_label(&self) -> String {
        let kb = self.size as f64 / 1024.0;
        if kb >= 1.0 {
            format!("{kb:.0} kB")
        } else {
            format!("{} B", self.size)
        }
    }
}

/// The parsed contents of `rom-list.txt`: file name -> description.
///
/// Entries start with a `+ file.rom` line and continue with indented
/// `  - text` bullet lines until the next entry or separator.
pub fn parse_rom_list(text: &str) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(name) = line.strip_prefix("+ ") {
            let name = name.trim();
            current = Some(name.to_string());
            map.entry(name.to_string()).or_default();
        } else if trimmed.starts_with("- ") && current.is_some() {
            let bullet = trimmed[2..].trim();
            let entry = map.entry(current.clone().unwrap()).or_default();
            if !entry.is_empty() {
                entry.push(' ');
            }
            entry.push_str(bullet);
        } else if line.starts_with('*') || line.starts_with("---") {
            // Section headers and separators end an entry.
            current = None;
        }
    }
    map
}

/// A scanned ROM directory.
#[derive(Clone, Debug)]
pub struct RomCatalog {
    monitors: Vec<RomEntry>,
    modules: Vec<RomEntry>,
}

impl RomCatalog {
    /// Scan `rom_dir` for `.rom` and `.rmm` files. Missing directory or
    /// unreadable files are not fatal: the catalog just comes back
    /// (possibly empty), the caller decides how loud to complain.
    pub fn scan(rom_dir: impl Into<PathBuf>) -> Self {
        let rom_dir = rom_dir.into();
        let mut descriptions = HashMap::new();
        if let Ok(text) = fs::read_to_string(rom_dir.join("rom-list.txt")) {
            descriptions = parse_rom_list(&text);
        }
        let mut monitors = Vec::new();
        let mut modules = Vec::new();
        let entries = fs::read_dir(&rom_dir).into_iter().flatten().flatten();
        for entry in entries {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            let path = entry.path();
            let Some(ext) = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
            else {
                continue;
            };
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let file_name = file_name.to_string();
            let description = descriptions
                .get(&file_name)
                .cloned()
                .unwrap_or_default();
            let rom = RomEntry {
                file_name,
                path,
                size: meta.len(),
                description,
            };
            match ext.as_str() {
                "rom" => monitors.push(rom),
                "rmm" => modules.push(rom),
                _ => {}
            }
        }
        monitors.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        modules.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        RomCatalog {
            monitors,
            modules,
        }
    }

    pub fn monitors(&self) -> &[RomEntry] {
        &self.monitors
    }

    pub fn modules(&self) -> &[RomEntry] {
        &self.modules
    }

    /// Find a monitor/module entry by file name.
    pub fn find(&self, file_name: &str) -> Option<&RomEntry> {
        self.monitors
            .iter()
            .chain(self.modules.iter())
            .find(|e| e.file_name == file_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pmd85_core::Model;

    const SAMPLE_LIST: &str = "\
***************************************
* header
***************************************
+ monit3.rom
  - zakladny monitor PMD 85-3
  - velkost 8kB
  - umiestnenie v pamati od adresy 0E000h
-------------------------------------------------------------------------------
+ basic3.rmm
  - BASIC G V3.0 pre PMD 85-3
+ notarom.txt
  - this is not a rom
";

    #[test]
    fn parses_entries_and_joins_bullets() {
        let map = parse_rom_list(SAMPLE_LIST);
        assert_eq!(
            map.get("monit3.rom").unwrap(),
            "zakladny monitor PMD 85-3 velkost 8kB umiestnenie v pamati od adresy 0E000h"
        );
        assert_eq!(map.get("basic3.rmm").unwrap(), "BASIC G V3.0 pre PMD 85-3");
        // `notarom.txt` is cut off by the following entry header: the
        // separator is not required to end an entry.
        assert_eq!(map.get("notarom.txt").unwrap(), "this is not a rom");
        assert_eq!(map.len(), 3);
    }

    fn write_test_roms(dir: &std::path::Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("monit3.rom"), [0u8; 8192]).unwrap();
        fs::write(dir.join("basic3.rmm"), [0u8; 10240]).unwrap();
        fs::write(dir.join("README.md"), b"ignore me").unwrap();
        fs::write(dir.join("rom-list.txt"), SAMPLE_LIST).unwrap();
    }

    #[test]
    fn scans_and_classifies_rom_files() {
        let dir = std::env::temp_dir().join(format!("pmd85-catalog-test-{}", std::process::id()));
        write_test_roms(&dir);
        let catalog = RomCatalog::scan(&dir);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(catalog.monitors().len(), 1);
        assert_eq!(catalog.modules().len(), 1);
        let module = &catalog.modules()[0];
        assert_eq!(module.file_name, "basic3.rmm");
        assert_eq!(module.size, 10240);
        assert_eq!(module.size_label(), "10 kB");
        assert_eq!(module.description, "BASIC G V3.0 pre PMD 85-3");
        // Descriptions join with the monitor's too.
        assert!(catalog.monitors()[0].description.contains("0E000h"));
        // Lookup by name, including the model default monitor.
        assert_eq!(
            catalog.find(Model::Pmd853.default_monitor()).map(|e| &e.file_name),
            Some(&"monit3.rom".to_string())
        );
        assert!(catalog.find("nope.rom").is_none());
    }

    #[test]
    fn missing_directory_is_not_fatal() {
        let catalog = RomCatalog::scan("/nonexistent-pmd85-rom-dir");
        assert!(catalog.monitors().is_empty());
        assert!(catalog.modules().is_empty());
    }
}
