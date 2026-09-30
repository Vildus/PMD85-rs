//! Cassette tape editor window (toggled from the transport bar).
//!
//! Lists the tape's files with position, length, name, type, start
//! address and checksum status, grouped with their headerless
//! continuation blocks. Files can be imported from raw binary data
//! (with tape metadata), exported, moved, deleted, and played into
//! the machine — select a file, press Play, then type `MGLD nn` on
//! the PMD to load it; saves made with `MGSV` are harvested back
//! into the tape automatically.

use crate::app::App;
use crate::ui::theme::Theme;
use egui_phosphor::fill as icon;
use pmd85_core::tape::FileHeader;
use std::path::PathBuf;

/// Metadata entry form for a pending import (the raw content file
/// has been picked; the tape fields are being edited).
#[derive(Debug)]
pub struct ImportDraft {
    /// Raw content file chosen in the dialog.
    pub path: PathBuf,
    /// File number on the tape (decimal 0..99, as typed into
    /// MGLD/MGSV on the machine).
    pub number: String,
    /// Block type character (`?`, `B`, ...).
    pub block_type: String,
    /// File name (up to 8 characters).
    pub name: String,
    /// Load start address (hex).
    pub start: String,
}

pub fn draw(ctx: &egui::Context, app: &mut App) {
    let mut open = app.ui.tape_open;
    egui::Window::new("Cassette tape")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(520.0)
        .show(ctx, |ui| {
            let theme = app.active_theme_data();
            toolbar(ui, app, &theme);
            ui.add_space(4.0);
            file_list(ui, app, &theme);
            ui.add_space(4.0);
            status(ui, app, &theme);
        });
    app.ui.tape_open = open;
}

/// New / Open / Save / Import / Export + per-file actions.
fn toolbar(ui: &mut egui::Ui, app: &mut App, _theme: &Theme) {
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(format!("{} New", icon::FILE_PLUS))
            .on_hover_text("Start a fresh empty tape (discards the current one)")
            .clicked()
        {
            app.new_tape();
        }
        if ui
            .button(format!("{} Open\u{2026}", icon::FOLDER_OPEN))
            .on_hover_text("Load a .ptp or old .pmd tape image")
            .clicked()
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("PMD tape", &["ptp", "pmd"])
                .pick_file()
            {
                if let Err(e) = app.open_tape(&path) {
                    app.notify(format!("Tape: {e}"));
                }
            }
        }
        let save_label = if app.tape.dirty {
            format!("{} Save \u{2022}", icon::FLOPPY_DISK)
        } else {
            format!("{} Save", icon::FLOPPY_DISK)
        };
        if ui
            .button(save_label)
            .on_hover_text("Write the tape back to its file")
            .clicked()
        {
            if let Err(e) = app.tape.save() {
                app.notify(format!("Tape: {e}"));
            }
        }
        if ui
            .button(format!("{} Save as\u{2026}", icon::FLOPPY_DISK))
            .on_hover_text("Write the tape as a .ptp image")
            .clicked()
        {
            if let Some(mut path) = rfd::FileDialog::new()
                .add_filter("PMD tape", &["ptp"])
                .set_file_name("tape.ptp")
                .save_file()
            {
                if path.extension().is_none() {
                    path = path.with_extension("ptp");
                }
                if let Err(e) = app.tape.save_as(&path) {
                    app.notify(format!("Tape: {e}"));
                }
            }
        }

        ui.separator();

        if ui
            .button(format!("{} Import\u{2026}", icon::DOWNLOAD_SIMPLE))
            .on_hover_text("Append a file built from a raw binary")
            .clicked()
        {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                let number = app.tape.files().len().min(99);
                app.ui.tape_import = Some(ImportDraft {
                    name: file_stem(&path),
                    number: fmt_number(number as u8),
                    block_type: "?".into(),
                    start: "0000".into(),
                    path,
                });
            }
        }
        let export_enabled = app
            .tape
            .selected
            .is_some_and(|s| app.tape.export_file(s).is_some());
        if ui
            .add_enabled(
                export_enabled,
                egui::Button::new(format!("{} Export\u{2026}", icon::UPLOAD_SIMPLE)),
            )
            .on_hover_text("Write the selected file's content to a raw binary")
            .clicked()
        {
            let selected = app.tape.selected;
            let file_name = export_file_name(app, selected);
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name(file_name)
                .save_file()
            {
                if let Some(content) = selected.and_then(|s| app.tape.export_file(s)) {
                    if let Err(e) = std::fs::write(&path, content) {
                        app.notify(format!("Tape: {}: {e}", path.display()));
                    }
                }
            }
        }
    });

    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        // ---- playback ----
        let playing = app.tape.is_playing();
        let file_count = app.tape.files().len();
        let play_enabled =
            playing || app.tape.selected.is_some_and(|s| s < file_count);
        let play_label = if playing {
            format!("{} Stop", icon::STOP)
        } else {
            format!("{} Play", icon::PLAY)
        };
        if ui
            .add_enabled(
                play_enabled,
                egui::Button::new(play_label).selected(playing),
            )
            .on_hover_text(
                "Feed the selected file to the machine (type MGLD nn on \
                 the PMD, where nn is the file's # number)",
            )
            .clicked()
        {
            if playing {
                app.stop_tape_playback();
            } else if let Some(file) = app.tape.selected {
                app.play_tape_file(file);
            }
        }

        ui.separator();

        // ---- per-file edits ----
        let selected = app.tape.selected;
        let up_clicked = ui
            .add_enabled(
                selected.is_some_and(|s| s > 0),
                egui::Button::new(format!("{} Up", icon::ARROW_UP)),
            )
            .on_hover_text("Move the file one place towards the tape start")
            .clicked();
        let down_clicked = ui
            .add_enabled(
                selected.is_some_and(|s| s + 1 < file_count),
                egui::Button::new(format!("{} Down", icon::ARROW_DOWN)),
            )
            .on_hover_text("Move the file one place towards the tape end")
            .clicked();
        let delete_clicked = ui
            .add_enabled(
                selected.is_some_and(|s| s < file_count),
                egui::Button::new(format!("{} Delete file", icon::TRASH)),
            )
            .on_hover_text("Delete the file and its continuation blocks")
            .clicked();
        if let Some(file) = selected.filter(|&s| s < file_count) {
            if up_clicked {
                app.tape.move_file(file, -1);
            }
            if down_clicked {
                app.tape.move_file(file, 1);
            }
            if delete_clicked {
                app.tape.delete_file(file);
            }
        }

        ui.separator();

        // ---- auto-stop / flash load / data tone ----
        let mut auto_stop = app.tape.auto_stop;
        if ui
            .checkbox(&mut auto_stop, "Stop after file")
            .on_hover_text(
                "Stop playback at the next file header instead of \
                 playing the rest of the tape",
            )
            .changed()
        {
            app.set_tape_autostop(auto_stop);
        }
        let mut flash = app.tape.flash;
        if ui
            .checkbox(&mut flash, "Flash load")
            .on_hover_text(
                "Fast-load played files: the monitor's tape read loops \
                 are intercepted and fed the block data directly, so \
                 MGLD finishes in a fraction of the time. Checksums, \
                 filters and autorun still run for real, and \
                 multi-block games fast-load too.",
            )
            .changed()
        {
            app.set_tape_flash(flash);
        }
        let mut tone = app.settings.tape_monitor;
        if ui
            .checkbox(&mut tone, "Data tone")
            .on_hover_text(
                "Play the tape data signal through the speaker while \
                 loading or saving (the cassette sound)",
            )
            .changed()
        {
            app.set_tape_monitor(tone);
        }
    });
}

/// The grouped file list, and the import form when one is pending.
fn file_list(ui: &mut egui::Ui, app: &mut App, theme: &Theme) {
    let c = |field: &[u8; 4]| Theme::color(field);

    // Import form: the draft is taken out for the duration of the
    // form (the buttons need `app` mutably) and put back unless it
    // was submitted or cancelled.
    if let Some(mut draft) = app.ui.tape_import.take() {
        let mut close = false;
        egui::Frame::new()
            .stroke(egui::Stroke::new(1.0, c(&theme.panel_border)))
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!("IMPORT {}", draft.path.display()))
                        .size(11.0)
                        .strong()
                        .color(c(&theme.accent)),
                );
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.name)
                            .desired_width(90.0)
                            .char_limit(8),
                    );
                    ui.label("Number");
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.number)
                            .desired_width(32.0)
                            .char_limit(2),
                    );
                    ui.label("Type");
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.block_type)
                            .desired_width(24.0)
                            .char_limit(1),
                    );
                    ui.label("Start");
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.start)
                            .desired_width(48.0)
                            .char_limit(4),
                    );
                });
                ui.horizontal(|ui| {
                    if ui.button("Import").clicked() {
                        match import_draft(app, &draft) {
                            Ok(()) => close = true,
                            Err(e) => app.notify(format!("Tape: {e}")),
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        if !close {
            app.ui.tape_import = Some(draft);
        }
        ui.add_space(4.0);
    }

    let files = app.tape.files();
    if files.is_empty() {
        ui.label(
            egui::RichText::new(
                "The tape is empty \u{2014} Open a .ptp image, import a file, \
                 or record a save with MGSV on the machine.",
            )
            .color(c(&theme.text_weak)),
        );
        return;
    }

    // Per-block on-disk byte offset and tape time (a file's position
    // is that of its first block).
    let mut offsets = Vec::with_capacity(app.tape.tape.blocks.len());
    let mut times = Vec::with_capacity(app.tape.tape.blocks.len());
    let (mut byte, mut secs) = (0usize, 0.0);
    for block in &app.tape.tape.blocks {
        offsets.push(byte);
        times.push(secs);
        byte += 2 + block.header_bytes.len() + block.body_bytes.len();
        secs += block.duration_secs();
    }

    egui::ScrollArea::vertical()
        .max_height(240.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("tape-files")
                .num_columns(8)
                .spacing(egui::vec2(12.0, 3.0))
                .striped(true)
                .show(ui, |ui| {
                    let head = |t: &str| {
                        egui::RichText::new(t)
                            .size(11.0)
                            .strong()
                            .color(c(&theme.accent))
                    };
                    for title in ["#", "pos", "time", "name", "type", "start", "length", "crc"] {
                        ui.label(head(title));
                    }
                    ui.end_row();

                    for (file_no, file) in files.iter().enumerate() {
                        let selected = app.tape.selected == Some(file_no);
                        let blocks = &app.tape.tape.blocks;
                        let block = &blocks[file.block];

                        // ---- file row ----
                        // The number is the two-digit decimal ID
                        // typed into MGLD/MGSV (the monitor parses
                        // it with decimal weighting, 00..99).
                        let number = block
                            .header
                            .as_ref()
                            .map(|h| fmt_number(h.number))
                            .unwrap_or_else(|| "\u{2014}".into());
                        ui.label(
                            egui::RichText::new(number).size(12.0).monospace(),
                        )
                        .on_hover_text(format!(
                            "file {file_no}, block {} \u{2014} load with MGLD {}",
                            file.block,
                            block
                                .header
                                .as_ref()
                                .map(|h| fmt_number(h.number))
                                .unwrap_or_else(|| "?".into())
                        ));
                        ui.label(
                            egui::RichText::new(format!("@{}", offsets[file.block]))
                                .size(11.0)
                                .monospace(),
                        );
                        ui.label(
                            egui::RichText::new(tape_time(times[file.block]))
                                .size(11.0)
                                .monospace(),
                        );
                        let (name, rest): (String, [String; 4]) = match &block.header {
                            Some(h) => (
                                h.name_str(),
                                [
                                    fmt_type(h.block_type),
                                    format!("{:04X}", h.start),
                                    format!("{}", h.content_len()),
                                    String::new(),
                                ],
                            ),
                            None => (
                                "(no header)".into(),
                                [
                                    "\u{2014}".into(),
                                    "\u{2014}".into(),
                                    format!("{}", block.body_bytes.len()),
                                    String::new(),
                                ],
                            ),
                        };
                        let name_label =
                            ui.selectable_label(selected, egui::RichText::new(name).monospace());
                        if name_label.clicked() {
                            app.tape.selected = Some(file_no);
                        }
                        let crc_ok = block.header_crc_ok && block.body_crc_ok;
                        let crc = if crc_ok {
                            egui::RichText::new("ok")
                                .color(c(&theme.accent))
                                .size(11.0)
                                .monospace()
                        } else {
                            egui::RichText::new("bad")
                                .color(c(&theme.warn))
                                .size(11.0)
                                .monospace()
                        };
                        for col in rest {
                            ui.label(egui::RichText::new(col).monospace());
                        }
                        ui.label(crc).on_hover_text(if crc_ok {
                            "checksums verified"
                        } else {
                            "checksum mismatch"
                        });
                        ui.end_row();

                        // ---- continuation block rows ----
                        for (n, &part) in file.continuations.iter().enumerate() {
                            let cont = &blocks[part];
                            ui.label(
                                egui::RichText::new(format!("+{n}"))
                                    .size(11.0)
                                    .monospace()
                                    .weak(),
                            );
                            ui.label(
                                egui::RichText::new(format!("@{}", offsets[part]))
                                    .size(11.0)
                                    .monospace()
                                    .weak(),
                            )
                            .on_hover_text(format!("continuation block {part}"));
                            ui.label(
                                egui::RichText::new(tape_time(times[part]))
                                    .size(11.0)
                                    .monospace()
                                    .weak(),
                            );
                            ui.label(
                                egui::RichText::new(format!(
                                    "\u{2500} {} bytes",
                                    cont.body_bytes.len()
                                ))
                                .monospace()
                                .weak(),
                            );
                            for _ in 0..5 {
                                ui.label(egui::RichText::new("").monospace());
                            }
                            ui.end_row();
                        }
                    }
                });
        });
}

/// Validate the draft and append the file to the tape.
fn import_draft(app: &mut App, draft: &ImportDraft) -> Result<(), String> {
    let number = parse_number(&draft.number)?;
    let start = u16::from_str_radix(draft.start.trim(), 16)
        .map_err(|_| format!("start address {:?} is not hex", draft.start))?;
    let block_type = draft
        .block_type
        .chars()
        .next()
        .ok_or("missing block type")? as u8;
    let content =
        std::fs::read(&draft.path).map_err(|e| format!("{}: {e}", draft.path.display()))?;
    app.tape
        .import_file(number, block_type, draft.name.trim(), start, &content)?;
    Ok(())
}

/// Playback progress, recording indicator, hints.
fn status(ui: &mut egui::Ui, app: &mut App, theme: &Theme) {
    let c = |field: &[u8; 4]| Theme::color(field);
    ui.separator();
    ui.horizontal(|ui| {
        if app.machine.bus.tape.is_recording() {
            ui.label(
                egui::RichText::new(format!("{} REC", icon::RECORD))
                    .color(c(&theme.danger))
                    .strong(),
            )
            .on_hover_text(
                "The machine is saving to tape (MGSV); the file \
                 appears here when it finishes",
            );
        }
        if app.tape.is_playing() {
            let (done, total) = app.machine.bus.tape.progress().unwrap_or((0, 1));
            let frac = if total > 0 {
                done as f32 / total as f32
            } else {
                0.0
            };
            ui.add(
                egui::ProgressBar::new(frac).show_percentage().text(format!(
                    "feeding block\u{2026} {done}/{total} bytes"
                )),
            );
        }
    });
    ui.label(
        egui::RichText::new(
            "Load: type MGLD nn (file number, 00-99 decimal) on the machine. \
             Save: MGSV nn start end \u{2014} recorded back into this tape.",
        )
        .size(11.0)
        .color(c(&theme.text_weak)),
    );
}

/// mm:ss for a tape position in seconds.
fn tape_time(secs: f64) -> String {
    let secs = secs as u64;
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// The block type as a printable character.
fn fmt_type(block_type: u8) -> String {
    match char::from_u32(block_type as u32) {
        Some(ch) if ch.is_ascii_graphic() => ch.to_string(),
        _ => format!("{block_type:02X}"),
    }
}

/// The file number as displayed in the tape list. The monitor parses
/// the MGLD/MGSV number with decimal weighting (ROM `EA54`: value =
/// 10·hi + lo, capped at 99), so a header byte of 0x43 means file
/// 67 — and `MGLD 67` is what loads it.
fn fmt_number(number: u8) -> String {
    format!("{number:02}")
}

/// Parse the import draft's file number: a decimal 0..=99, the
/// range the monitor's MGLD/MGSV accepts.
fn parse_number(text: &str) -> Result<u8, String> {
    let n: u32 = text
        .trim()
        .parse()
        .map_err(|_| format!("file number {text:?} is not a decimal number"))?;
    if n > 99 {
        Err(format!(
            "file number {n} is out of range: MGLD/MGSV accept 00..=99"
        ))
    } else {
        Ok(n as u8)
    }
}

/// Export file name from the selected file's tape name.
fn export_file_name(app: &App, selected: Option<usize>) -> String {
    let files = app.tape.files();
    selected
        .and_then(|s| files.get(s))
        .map(|f| &app.tape.tape.blocks[f.block])
        .and_then(|b| b.header.as_ref())
        .map(|h: &FileHeader| h.name_str().trim().replace(' ', "_"))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "file.bin".into())
}

/// File stem as a (short) tape name suggestion.
fn file_stem(path: &std::path::Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("FILE")
        .chars()
        .take(8)
        .collect::<String>()
        .to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ID column shows the file number the way the monitor (and
    /// GPMD85's tape browser) understand it: decimal. A BASIC file
    /// with header byte 0x43 is file 67, loaded with `MGLD 67`.
    #[test]
    fn number_column_is_decimal() {
        assert_eq!(fmt_number(0x43), "67");
        assert_eq!(fmt_number(0x37), "55");
        assert_eq!(fmt_number(0x11), "17");
        assert_eq!(fmt_number(0), "00");
        assert_eq!(fmt_number(99), "99");
    }

    /// The import draft accepts decimal 0..=99 only — the range
    /// MGLD/MGSV can type — so every imported file stays loadable.
    #[test]
    fn import_number_is_decimal_0_to_99() {
        assert_eq!(parse_number("67").unwrap(), 0x43);
        assert_eq!(parse_number(" 00 ").unwrap(), 0);
        assert_eq!(parse_number("99").unwrap(), 99);
        assert!(parse_number("43A").is_err(), "hex suffix rejected");
        assert!(parse_number("1A").is_err(), "hex digits rejected");
        assert!(parse_number("100").is_err(), "over the monitor's cap");
        assert!(parse_number("").is_err());
    }
}
