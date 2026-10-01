//! Debugger panels, docked as tabs. The Debugger tab is the whole
//! debugger: a register strip, the step controls and the disassembly
//! listing with breakpoints, all in one place. The Memory tab is the
//! hex+ASCII dump. The host shortcuts Alt+F10/F11/F12 drive the same
//! step controls from anywhere.

use crate::app::App;
use crate::ui::theme::Theme;
use egui_phosphor::regular as icon;
use pmd85_core::disasm;
use pmd85_core::machine::Machine;
use pmd85_core::Model;

/// Debugger view state (session-only, never persisted).
#[derive(Debug)]
pub struct DebugState {
    /// Whether the disassembly follows the PC. While following, the
    /// PC row can never leave the window: scrolling only peeks
    /// around it (see [`CONTEXT`] and [`DebugState::offset`]).
    pub follow_pc: bool,
    /// Follow mode: rows the window is shifted relative to the PC
    /// (0 = the default, the PC with [`CONTEXT`] rows above it).
    /// Clamped so the PC stays visible; survives PC movement.
    pub offset: isize,
    /// Free mode (follow off): the first listed address.
    pub anchor: u16,
    /// Goto-address field text (each panel consumes it on Enter in
    /// its own tab).
    pub goto: String,
    /// Pending scroll target for the memory dump, as a row index
    /// (16 bytes per row — the full 64 KiB is always scrollable).
    pub memory_scroll: Option<usize>,
}

impl Default for DebugState {
    fn default() -> Self {
        DebugState {
            follow_pc: true,
            offset: 0,
            anchor: 0,
            goto: String::new(),
            memory_scroll: None,
        }
    }
}

/// Instructions shown above the PC row in follow mode.
const CONTEXT: isize = 8;
/// Lower bound on the listed rows, whatever the panel height.
const MIN_ROWS: usize = 8;
/// Upper bound, so a giant panel does not render unbounded rows.
const MAX_ROWS: usize = 64;

/// Clamp the follow offset so the PC row — at row `CONTEXT + offset`
/// — stays inside the window of `rows` rows.
fn clamp_offset(offset: isize, rows: usize) -> isize {
    offset.clamp(-CONTEXT, rows as isize - CONTEXT - 1)
}

/// The first listed address: in follow mode a start whose forward
/// decode reaches the PC after `CONTEXT + offset` instructions (so
/// the PC sits at row `CONTEXT + offset`); in free mode the anchor.
fn window_start(m: &Machine, follow: bool, anchor: u16, offset: isize) -> u16 {
    if follow {
        back_anchor(m, m.cpu.pc, (CONTEXT + offset).max(0) as usize)
    } else {
        anchor
    }
}

// ---- the Debugger tab ----------------------------------------------------

/// The Debugger tab: register strip, step controls, disassembly.
pub fn debugger(ui: &mut egui::Ui, app: &mut App) {
    register_strip(ui, app);
    toolbar(ui, app);
    ui.add_space(2.0);
    listing(ui, app);
}

/// A small square icon button in the transport-bar style; returns
/// `true` when clicked.
fn icon_button(ui: &mut egui::Ui, glyph: &str, tooltip: &str, accent: egui::Color32) -> bool {
    let text = egui::RichText::new(glyph).size(14.0).strong().color(accent);
    ui.add(egui::Button::new(text).min_size(egui::vec2(26.0, 22.0)))
        .on_hover_text(tooltip)
        .clicked()
}

/// The CPU, in two compact monospace lines: register pairs, then the
/// stack and program counters, flags and the cycle count. Live.
fn register_strip(ui: &mut egui::Ui, app: &mut App) {
    let theme = app.active_theme_data();
    let text = Theme::color(&theme.text);
    let weak = Theme::color(&theme.text_weak);
    let accent = Theme::color(&theme.accent);
    let warn = Theme::color(&theme.warn);
    let m = &app.machine;
    let value = |s: String, color| {
        egui::RichText::new(s).monospace().size(12.0).color(color)
    };
    let name = |s: &str| {
        egui::RichText::new(s)
            .monospace()
            .size(10.0)
            .color(weak)
    };

    ui.horizontal(|ui| {
        for (label, hi, lo) in [
            ("AF", m.cpu.a, m.cpu.flags.pack()),
            ("BC", m.cpu.b, m.cpu.c),
            ("DE", m.cpu.d, m.cpu.e),
            ("HL", m.cpu.h, m.cpu.l),
        ] {
            ui.label(name(label));
            ui.label(value(format!("{hi:02X} {lo:02X}"), text));
            ui.add_space(10.0);
        }
    });
    ui.horizontal(|ui| {
        ui.label(name("SP"));
        ui.label(value(format!("{:04X}", m.cpu.sp), text));
        ui.add_space(10.0);
        ui.label(name("PC"));
        ui.label(value(format!("{:04X}", m.cpu.pc), accent));
        ui.add_space(10.0);
        for (flag, set) in [
            ("S", m.cpu.flags.s),
            ("Z", m.cpu.flags.z),
            ("AC", m.cpu.flags.ac),
            ("P", m.cpu.flags.p),
            ("CY", m.cpu.flags.cy),
        ] {
            ui.label(
                egui::RichText::new(flag)
                    .monospace()
                    .size(11.0)
                    .color(if set { accent } else { weak }),
            );
        }
        ui.add_space(10.0);
        ui.label(name("cycles"));
        ui.label(value(format!("{}", m.bus.total_cycles()), text));
    });

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
}

/// Change the follow mode without the view jumping: turning follow
/// off captures the window the listing is showing right now as the
/// free-mode anchor (the view stays exactly where it is); turning it
/// on re-centers on the PC.
fn set_follow(d: &mut DebugState, follow: bool, m: &Machine) {
    if follow == d.follow_pc {
        return;
    }
    if follow {
        d.follow_pc = true;
        d.offset = 0;
    } else {
        d.anchor = window_start(m, true, d.anchor, d.offset);
        d.follow_pc = false;
    }
}

/// The step controls (Step / Over / Continue), the follow-PC checkbox
/// and the goto field.
fn toolbar(ui: &mut egui::Ui, app: &mut App) {
    let theme = app.active_theme_data();
    let accent = Theme::color(&theme.accent);
    ui.horizontal(|ui| {
        if icon_button(
            ui,
            icon::STEPS,
            "Step one instruction (Alt+F10)",
            accent,
        ) {
            app.debug_step();
        }
        if icon_button(
            ui,
            icon::SKIP_FORWARD,
            "Step over a call (Alt+F11)",
            accent,
        ) {
            app.debug_step_over();
        }
        if icon_button(
            ui,
            icon::PLAY,
            "Continue after a breakpoint (Alt+F12)",
            accent,
        ) {
            app.debug_continue();
        }
        ui.separator();

        // Follow PC: while checked the PC stays in view; unchecking
        // freezes the view where it is, re-checking snaps back onto
        // the PC.
        let mut follow = app.ui.debug.follow_pc;
        ui.checkbox(&mut follow, "follow PC")
            .on_hover_text("Keep the PC in view; scrolling only peeks around it");
        set_follow(&mut app.ui.debug, follow, &app.machine);

        ui.add_space(8.0);
        let edit = ui.add(
            egui::TextEdit::singleline(&mut app.ui.debug.goto)
                .hint_text("goto hex")
                .desired_width(64.0)
                .font(egui::TextStyle::Monospace),
        );
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(addr) = parse_hex16(&app.ui.debug.goto) {
                app.ui.debug.anchor = addr;
                app.ui.debug.follow_pc = false;
            }
            app.ui.debug.goto.clear();
        }
    });
}

/// The disassembly listing: a window of instructions from the anchor
/// (follow-PC or free), a clickable breakpoint gutter, the PC row
/// highlighted.
fn listing(ui: &mut egui::Ui, app: &mut App) {
    let theme = app.active_theme_data();
    // The real stride of one listed row: the rendered 12 pt monospace
    // row plus the spacing between rows. Measured, not guessed, so
    // the listing fills the panel without ever overflowing it (the
    // tab body does not scroll — the listing scrolls itself).
    let row_font = egui::FontId::monospace(12.0);
    let stride = ui.fonts_mut(|f| f.row_height(&row_font)) + ui.spacing().item_spacing.y;
    let rows = ((ui.available_height() / stride) as usize).clamp(MIN_ROWS, MAX_ROWS);

    // The wheel scrolls: in follow mode by peeking around the PC
    // (clamped so it stays visible), in free mode by walking the
    // anchor.
    let wheel = ui.ctx().input(|i| i.smooth_scroll_delta.y);
    if wheel != 0.0 && ui.ui_contains_pointer() {
        let lines =
            ((wheel.abs() / 14.0).ceil() as isize).clamp(1, 8) * wheel.signum() as isize;
        if app.ui.debug.follow_pc {
            app.ui.debug.offset = clamp_offset(app.ui.debug.offset + lines, rows);
        } else if lines > 0 {
            app.ui.debug.anchor =
                back_anchor(&app.machine, app.ui.debug.anchor, lines as usize);
        } else {
            let mut addr = app.ui.debug.anchor;
            for _ in 0..(-lines) {
                let (_, len) = disassemble_at(&app.machine, addr);
                addr = addr.wrapping_add(len as u16);
            }
            app.ui.debug.anchor = addr;
        }
    }

    // Data pass: decode the rows (a plain snapshot, so the UI below
    // may toggle breakpoints on the machine).
    let pc = app.machine.cpu.pc;
    let anchor = window_start(
        &app.machine,
        app.ui.debug.follow_pc,
        app.ui.debug.anchor,
        app.ui.debug.offset,
    );
    let breakpoints = app.machine.breakpoints().clone();
    let mut lines = Vec::with_capacity(rows);
    let mut addr = anchor;
    for _ in 0..rows {
        let (text, len) = disassemble_at(&app.machine, addr);
        lines.push((addr, text, breakpoints.contains(&addr)));
        addr = addr.wrapping_add(len as u16);
    }

    // Row rendering.
    let text = Theme::color(&theme.text);
    let weak = Theme::color(&theme.text_weak);
    let accent = Theme::color(&theme.accent);
    let danger = Theme::color(&theme.danger);
    let fill = Theme::color(&theme.accent_dim);
    for (addr, instr, has_bp) in lines {
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

// ---- shared helpers ------------------------------------------------------

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

/// Parse a 1-4 digit hex address ("e000", "0xE000").
fn parse_hex16(s: &str) -> Option<u16> {
    let s = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    if s.is_empty() {
        return None;
    }
    u16::from_str_radix(s, 16).ok()
}

// ---- the Memory tab ------------------------------------------------------

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

    /// The follow offset is clamped so the PC row — at
    /// CONTEXT + offset — never leaves the window.
    #[test]
    fn follow_offset_clamps_to_keep_the_pc_visible() {
        assert_eq!(clamp_offset(0, 20), 0);
        // Pushing up: at most the PC on the last row.
        assert_eq!(clamp_offset(100, 20), 20 - CONTEXT - 1);
        // Pulling down: at most the PC on the first row.
        assert_eq!(clamp_offset(-100, 20), -CONTEXT);
        // A window exactly CONTEXT rows: the PC can only be at or
        // below its default position, never off the top.
        assert_eq!(clamp_offset(100, CONTEXT as usize), -1);
        assert_eq!(clamp_offset(-100, CONTEXT as usize), -CONTEXT);
        // In range: untouched.
        assert_eq!(clamp_offset(3, 20), 3);
    }

    /// In follow mode the window starts exactly CONTEXT + offset
    /// instructions before the PC, so the PC sits at that row.
    #[test]
    fn follow_window_starts_context_plus_offset_rows_before_the_pc() {
        let mut m = Machine::new(Model::Pmd853, &[0u8; 0x2000], None);
        m.cpu.pc = 0x0500;
        for offset in [-CONTEXT, -3, 0, 5] {
            let start = window_start(&m, true, 0, offset);
            let mut a = start;
            let mut count = 0;
            while a < m.cpu.pc {
                let (_, len) = disassemble_at(&m, a);
                a = a.wrapping_add(len as u16);
                count += 1;
            }
            assert_eq!(a, m.cpu.pc, "the walk must land on the PC");
            assert_eq!(count as isize, CONTEXT + offset, "offset {offset}");
        }
        // Free mode ignores the PC and starts at the anchor.
        assert_eq!(window_start(&m, false, 0xE000, 7), 0xE000);
    }

    /// Unchecking follow PC must freeze the view where it is (the
    /// free anchor becomes the window the listing is showing), not
    /// teleport to a stale anchor; re-checking re-centers on the PC.
    #[test]
    fn toggling_follow_off_keeps_the_view() {
        let mut m = Machine::new(Model::Pmd853, &[0u8; 0x2000], None);
        m.cpu.pc = 0x0500;
        let mut d = DebugState {
            offset: 3,
            ..DebugState::default()
        };
        let shown = window_start(&m, true, d.anchor, d.offset);

        set_follow(&mut d, false, &m);
        assert!(!d.follow_pc);
        assert_eq!(d.anchor, shown, "the view stays where it was");
        // The offset is left alone (free mode does not use it).
        assert_eq!(d.offset, 3);

        // Free mode: the anchor is used as-is.
        assert_eq!(window_start(&m, d.follow_pc, d.anchor, d.offset), shown);

        // Re-checking follows again from the default offset.
        set_follow(&mut d, true, &m);
        assert!(d.follow_pc);
        assert_eq!(d.offset, 0);

        // A no-op toggle changes nothing.
        set_follow(&mut d, true, &m);
        assert_eq!(d.offset, 0);
    }
}
