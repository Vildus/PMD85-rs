//! Debugger panels, docked as tabs: the CPU registers, the
//! disassembly with breakpoints, and a memory dump. Driven from the
//! transport bar's debugger group and the Alt+F10/F11/F12 host
//! shortcuts.

use crate::app::App;
use crate::ui::theme::Theme;
use pmd85_core::disasm;
use pmd85_core::machine::Machine;
use pmd85_core::Model;

/// Debugger view state (session-only, never persisted).
#[derive(Debug)]
pub struct DebugState {
    /// Whether the disassembly follows the PC (auto-disengages on
    /// scroll or a goto).
    pub follow_pc: bool,
    /// The disassembly anchor address (the first listed row).
    pub anchor: u16,
    /// Goto-address field text (shared by the disassembly and memory
    /// panels; each consumes it on Enter in its own tab).
    pub goto: String,
    /// Pending scroll target for the memory dump, as a row index
    /// (16 bytes per row — the full 64 KiB is always scrollable).
    pub memory_scroll: Option<usize>,
}

impl Default for DebugState {
    fn default() -> Self {
        DebugState {
            follow_pc: true,
            anchor: 0,
            goto: String::new(),
            memory_scroll: None,
        }
    }
}

// ---- CPU ----------------------------------------------------------------

/// The CPU tab: register pairs, the stack and program counters,
/// flags, cycle count and the instruction at the PC. Live.
pub fn cpu(ui: &mut egui::Ui, app: &mut App) {
    let theme = app.active_theme_data();
    let text = Theme::color(&theme.text);
    let weak = Theme::color(&theme.text_weak);
    let accent = Theme::color(&theme.accent);
    let warn = Theme::color(&theme.warn);
    let m = &app.machine;
    let value = |s: String, color| {
        egui::RichText::new(s).monospace().size(12.0).color(color)
    };
    let caption = |s: String| {
        egui::RichText::new(s)
            .monospace()
            .size(10.0)
            .color(weak)
    };

    // Register pairs: AF, BC, DE, HL, then SP and PC.
    egui::Grid::new("debug-cpu-regs")
        .num_columns(2)
        .spacing([8.0, 3.0])
        .show(ui, |ui| {
            let row = |name: &str, val: String, color, ui: &mut egui::Ui| {
                ui.label(caption(name.into()));
                ui.label(value(val, color));
                ui.end_row();
            };
            row(
                "AF",
                format!("{:02X} {:02X}", m.cpu.a, m.cpu.flags.pack()),
                text,
                ui,
            );
            row(
                "BC",
                format!("{:02X} {:02X}", m.cpu.b, m.cpu.c),
                text,
                ui,
            );
            row(
                "DE",
                format!("{:02X} {:02X}", m.cpu.d, m.cpu.e),
                text,
                ui,
            );
            row(
                "HL",
                format!("{:02X} {:02X}", m.cpu.h, m.cpu.l),
                text,
                ui,
            );
            row("SP", format!("{:04X}", m.cpu.sp), text, ui);
            row("PC", format!("{:04X}", m.cpu.pc), accent, ui);
            row("cycles", format!("{}", m.bus.total_cycles()), text, ui);
        });

    ui.add_space(4.0);
    // Flags: S Z AC P CY, lit when set.
    ui.label(caption("flags".into()));
    ui.horizontal(|ui| {
        for (name, set) in [
            ("S", m.cpu.flags.s),
            ("Z", m.cpu.flags.z),
            ("AC", m.cpu.flags.ac),
            ("P", m.cpu.flags.p),
            ("CY", m.cpu.flags.cy),
        ] {
            ui.label(
                egui::RichText::new(name)
                    .monospace()
                    .size(11.0)
                    .color(if set { accent } else { weak }),
            );
        }
    });

    ui.add_space(4.0);
    // The instruction at the PC, disassembled live from memory.
    let pc = m.cpu.pc;
    let (instr, _) = disassemble_at(m, pc);
    ui.label(caption("PC \u{2192}".into()));
    ui.label(value(instr, text));

    // The stop reason, if the machine sits somewhere interesting.
    if let Some(hit) = m.breakpoint_hit() {
        ui.label(
            egui::RichText::new(format!("breakpoint at {hit:04X}"))
                .monospace()
                .size(11.0)
                .color(warn),
        );
    }
    if m.cpu.halted {
        ui.label(
            egui::RichText::new("HLT \u{2014} CPU halted")
                .monospace()
                .size(11.0)
                .color(warn),
        );
    }

    let bps = m.breakpoints().len();
    ui.label(caption(format!(
        "{bps} breakpoint{} (click the gutter in the disassembly)",
        if bps == 1 { "" } else { "s" }
    )));
}

// ---- disassembly --------------------------------------------------------

/// Instructions listed at once (the panel is sized for ~35 rows;
/// smaller docks simply show fewer).
const ROWS: usize = 35;

/// The disassembly tab: a window of instructions from the anchor, a
/// breakpoint gutter (click to toggle), the PC row highlighted,
/// follow-PC by default and a goto field.
pub fn listing(ui: &mut egui::Ui, app: &mut App) {
    let theme = app.active_theme_data();
    let d = &mut app.ui.debug;

    // Goto + follow controls.
    ui.horizontal(|ui| {
        let edit = ui.add(
            egui::TextEdit::singleline(&mut d.goto)
                .hint_text("goto hex")
                .desired_width(64.0)
                .font(egui::TextStyle::Monospace),
        );
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(addr) = parse_hex16(&d.goto) {
                d.anchor = addr;
                d.follow_pc = false;
            }
            d.goto.clear();
        }
        if ui
            .add(egui::Button::new(egui::RichText::new("follow PC").size(11.0)))
            .on_hover_text("Scroll along with the program counter")
            .clicked()
        {
            d.follow_pc = !d.follow_pc;
        }
        ui.label(
            egui::RichText::new(if d.follow_pc { "\u{25CF} on" } else { "\u{25CB} off" })
                .monospace()
                .size(11.0)
                .color(if d.follow_pc {
                    Theme::color(&theme.accent)
                } else {
                    Theme::color(&theme.text_weak)
                }),
        );
    });
    ui.add_space(2.0);

    // The wheel disengages follow-PC and scrolls the anchor.
    let wheel = ui.ctx().input(|i| i.smooth_scroll_delta.y);
    if wheel != 0.0 && ui.ui_contains_pointer() {
        d.follow_pc = false;
        let rows = ((wheel.abs() / 14.0).ceil() as usize).clamp(1, 8);
        if wheel > 0.0 {
            d.anchor = back_anchor(&app.machine, d.anchor, rows);
        } else {
            let mut addr = d.anchor;
            for _ in 0..rows {
                let (_, len) = disassemble_at(&app.machine, addr);
                addr = addr.wrapping_add(len as u16);
            }
            d.anchor = addr;
        }
    }

    if d.follow_pc {
        d.anchor = app.machine.cpu.pc;
    }

    // Data pass: decode the rows (a plain snapshot, so the UI below
    // may toggle breakpoints on the machine).
    let pc = app.machine.cpu.pc;
    let breakpoints = app.machine.breakpoints().clone();
    let mut rows = Vec::with_capacity(ROWS);
    let mut addr = d.anchor;
    for _ in 0..ROWS {
        let (text, len) = disassemble_at(&app.machine, addr);
        rows.push((addr, text, breakpoints.contains(&addr)));
        addr = addr.wrapping_add(len as u16);
    }

    // Row rendering.
    let text = Theme::color(&theme.text);
    let weak = Theme::color(&theme.text_weak);
    let accent = Theme::color(&theme.accent);
    let danger = Theme::color(&theme.danger);
    let fill = Theme::color(&theme.accent_dim);
    for (addr, instr, has_bp) in rows {
        let is_pc = addr == pc;
        egui::Frame::new()
            .fill(if is_pc { fill } else { egui::Color32::TRANSPARENT })
            .inner_margin(egui::Margin::symmetric(2, 0))
            .corner_radius(egui::CornerRadius::same(2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Breakpoint gutter.
                    let gutter = ui.add(
                        egui::Label::new(
                            egui::RichText::new(if has_bp { "\u{25CF}" } else { "\u{00B7}" })
                                .monospace()
                                .size(12.0)
                                .color(if has_bp { danger } else { weak }),
                        )
                        .sense(egui::Sense::click()),
                    );
                    if gutter.clicked() {
                        app.machine.toggle_breakpoint(addr);
                    }
                    // Address.
                    ui.label(
                        egui::RichText::new(format!("{addr:04X}"))
                            .monospace()
                            .size(12.0)
                            .color(if is_pc { accent } else { weak }),
                    );
                    // Instruction.
                    ui.label(
                        egui::RichText::new(instr)
                            .monospace()
                            .size(12.0)
                            .color(text),
                    );
                });
            });
    }
}

/// Disassemble at `addr` on the machine's memory (banking-aware: what
/// the CPU sees).
fn disassemble_at(m: &Machine, addr: u16) -> (String, usize) {
    let bytes = [
        m.bus.memory.read(addr),
        m.bus.memory.read(addr.wrapping_add(1)),
        m.bus.memory.read(addr.wrapping_add(2)),
    ];
    disasm::disassemble(addr, &bytes)
}

/// Walk `rows` instructions back from `addr`, keeping instruction
/// boundaries: find a start below `addr` whose forward decode reaches
/// `addr` on a boundary after exactly `rows` instructions, and return
/// where that walk starts. Falls back to a byte-wise retreat when the
/// bytes below never align (data, or the bottom of memory).
fn back_anchor(m: &Machine, addr: u16, rows: usize) -> u16 {
    let max_back = (rows * 3 + 16) as u32;
    for back in 1..=max_back {
        if (addr as u32) < back {
            break; // hit the bottom of memory
        }
        let start = addr - back as u16;
        let mut a = start;
        let mut count = 0;
        while (a as u32) < addr as u32 && count <= rows {
            let (_, len) = disassemble_at(m, a);
            a = a.wrapping_add(len as u16);
            count += 1;
        }
        if a == addr && count == rows {
            return start;
        }
    }
    addr.wrapping_sub(rows as u16 * 2)
}

// ---- memory --------------------------------------------------------------

/// Bytes per row of the memory dump.
const BYTES_PER_ROW: usize = 16;

/// The memory tab: a hex+ASCII dump of the whole 64 KiB, read
/// through the bus (banking-aware: exactly what the CPU sees), live
/// every frame, with region annotations and a goto field.
pub fn memory(ui: &mut egui::Ui, app: &mut App) {
    let theme = app.active_theme_data();
    let text = Theme::color(&theme.text);
    let weak = Theme::color(&theme.text_weak);
    let accent = Theme::color(&theme.accent);
    let d = &mut app.ui.debug;

    // Goto field.
    ui.horizontal(|ui| {
        let edit = ui.add(
            egui::TextEdit::singleline(&mut d.goto)
                .hint_text("goto hex")
                .desired_width(64.0)
                .font(egui::TextStyle::Monospace),
        );
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(addr) = parse_hex16(&d.goto) {
                // Scroll the dump there, a few lines from the top.
                d.memory_scroll = Some(addr as usize / BYTES_PER_ROW);
            }
            d.goto.clear();
        }
    });
    ui.add_space(2.0);

    let total_rows = 0x10000 / BYTES_PER_ROW;
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    let mut scroll_area = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible);
    if let Some(row) = d.memory_scroll.take() {
        scroll_area = scroll_area
            .vertical_scroll_offset((row.saturating_sub(3)) as f32 * row_height);
    }
    let mem = &app.machine.bus.memory;
    scroll_area.show_rows(ui, row_height, total_rows, |ui, row_range| {
        for row in row_range {
            let addr = (row * BYTES_PER_ROW) as u16;
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{addr:04X}"))
                        .monospace()
                        .color(weak),
                );
                let mut hex = String::new();
                let mut ascii = String::new();
                for i in 0..BYTES_PER_ROW {
                    let b = mem.read(addr.wrapping_add(i as u16));
                    hex.push_str(&format!("{b:02X} "));
                    ascii.push(if b.is_ascii_graphic() || b == b' ' {
                        b as char
                    } else {
                        '.'
                    });
                }
                ui.label(egui::RichText::new(hex).monospace().color(text));
                ui.label(egui::RichText::new(ascii).monospace().color(weak));
                // Region annotation on rows that open one.
                if (addr as usize).is_multiple_of(0x400) {
                    ui.label(
                        egui::RichText::new(region_name(app, addr))
                            .monospace()
                            .color(accent),
                    );
                }
            });
        }
    });
}

/// What the region at `addr` is, per the machine's memory map. The
/// dump's bytes always come from the bus, so banking is already
/// reflected; this names the region itself.
fn region_name(app: &App, addr: u16) -> String {
    let m = &app.machine;
    let mem = &m.bus.memory;
    // The boot-time shadow map: ROM mirrored everywhere.
    if mem.startup_map() {
        return "shadow map: monitor ROM mirrored".into();
    }
    match m.model() {
        Model::Pmd853 => {
            if addr >= 0xE000 && mem.pmd853_mapping() {
                "monitor ROM".into()
            } else {
                "RAM".into()
            }
        }
        Model::Pmd851 | Model::Pmd852 | Model::Pmd852a => match addr {
            0x8000..=0x8FFF | 0xA000..=0xAFFF => "monitor ROM".into(),
            0xC000..=0xFFFF => "video RAM".into(),
            _ => "RAM".into(),
        },
    }
}

/// Parse a 1-4 digit hex address ("e000", "0xE000").
fn parse_hex16(s: &str) -> Option<u16> {
    let s = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    if s.is_empty() {
        return None;
    }
    u16::from_str_radix(s, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_addresses_parse() {
        assert_eq!(parse_hex16("EDE2"), Some(0xEDE2));
        assert_eq!(parse_hex16("ede2"), Some(0xEDE2));
        assert_eq!(parse_hex16("0xEDE2"), Some(0xEDE2));
        assert_eq!(parse_hex16("5"), Some(0x0005));
        assert_eq!(parse_hex16(""), None);
        assert_eq!(parse_hex16("xyz"), None);
        assert_eq!(parse_hex16("12345"), None);
    }
}
