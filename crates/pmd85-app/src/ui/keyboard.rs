//! Reference view of the PMD 85 keyboard layout, shown in its own
//! window (toggled from the transport bar).
//!
//! The layout follows the reference chart: a function row and four
//! key rows plus the space bar, each row staggered half a key further
//! right, with the control keys in a grid at the row ends —
//! WRK/C-D/RCL above INS-PTL/DEL/CLR above the arrow block above
//! STOP/EOL. The main engraving is centered, the SHIFT engraving sits
//! in the top-right corner. Caps light up while the corresponding
//! emulated key is held down, and they are clickable: holding the
//! pointer on a cap presses its key (sliding across caps slides the
//! keypress), like a piano. RST is the hardware reset line (not a
//! matrix key); pressing it resets the machine.

use crate::app::App;
use crate::ui::theme::Theme;
use pmd85_core::keyboard::{Key, Keyboard};

/// Key geometry, in points.
const KEY_W: f32 = 30.0;
const KEY_H: f32 = 26.0;
const GAP: f32 = 4.0;

/// One keycap of the reference layout.
#[derive(Clone, Copy)]
struct Cap {
    /// Emulated key, for the live pressed highlight. `None` = the cap
    /// has no matrix position (RST) and is cosmetic only.
    key: Option<Key>,
    /// Main engraving (plain keypress).
    main: &'static str,
    /// Small secondary engraving under the main one.
    sub: Option<&'static str>,
    /// SHIFT engraving.
    shift: Option<&'static str>,
    /// Width in key units.
    w: f32,
    /// Height in key units.
    h: f32,
    /// Invisible spacer carrying the row stagger.
    hidden: bool,
    /// RST: fires the hardware reset line instead of a matrix key.
    reset: bool,
}

fn cap(key: Option<Key>, main: &'static str, w: f32) -> Cap {
    Cap {
        key,
        main,
        sub: None,
        shift: None,
        w,
        h: 1.0,
        hidden: false,
        reset: false,
    }
}

/// An invisible cap used to indent a row (the physical stagger).
fn spacer(w: f32) -> Cap {
    Cap {
        hidden: true,
        w,
        ..cap(None, "", 1.0)
    }
}

/// The PMD 85 keyboard: function row, digit row, three letter rows
/// and the space bar, staggered half a key per row. The control keys
/// form a grid at the right end of each row (matrix columns 12-14).
fn main_block() -> Vec<Vec<Cap>> {
    use Key::*;
    vec![
        // Row 0: function keys, then WRK, C-D, RCL and the reset key.
        vec![
            cap(Some(K0), "K0", 1.0),
            cap(Some(K1), "K1", 1.0),
            cap(Some(K2), "K2", 1.0),
            cap(Some(K3), "K3", 1.0),
            cap(Some(K4), "K4", 1.0),
            cap(Some(K5), "K5", 1.0),
            cap(Some(K6), "K6", 1.0),
            cap(Some(K7), "K7", 1.0),
            cap(Some(K8), "K8", 1.0),
            cap(Some(K9), "K9", 1.0),
            cap(Some(K10), "K10", 1.0),
            cap(Some(K11), "K11", 1.0),
            cap(Some(Wrk), "WRK", 1.0),
            cap(Some(Cd), "C-D", 1.0),
            cap(Some(Recall), "RCL", 1.0),
            Cap {
                reset: true,
                ..cap(None, "RST", 1.0)
            },
        ],
        // Row 1: digits (SHIFT engravings on top), then the PTL/INS,
        // DEL and CLR column of the control grid.
        vec![
            Cap { shift: Some("!"), ..cap(Some(Digit1), "1", 1.0) },
            Cap { shift: Some("\""), ..cap(Some(Digit2), "2", 1.0) },
            Cap { shift: Some("#"), ..cap(Some(Digit3), "3", 1.0) },
            Cap { shift: Some("$"), ..cap(Some(Digit4), "4", 1.0) },
            Cap { shift: Some("%"), ..cap(Some(Digit5), "5", 1.0) },
            Cap { shift: Some("&"), ..cap(Some(Digit6), "6", 1.0) },
            Cap { shift: Some("'"), ..cap(Some(Digit7), "7", 1.0) },
            Cap { shift: Some("("), ..cap(Some(Digit8), "8", 1.0) },
            Cap { shift: Some(")"), ..cap(Some(Digit9), "9", 1.0) },
            Cap { shift: Some("-"), ..cap(Some(Digit0), "0", 1.0) },
            Cap { shift: Some("="), ..cap(Some(Minus), "_", 1.0) },
            Cap { shift: Some("}"), ..cap(Some(Equals), "{", 1.0) },
            Cap { sub: Some("INS"), ..cap(Some(Insert), "PTL", 1.0) },
            cap(Some(Delete), "DEL", 1.0),
            cap(Some(ClrScr), "CLR", 1.0),
        ],
        // Row 2: QWERTZ (Z on this row), then the arrow column.
        vec![
            spacer(0.5),
            cap(Some(Q), "Q", 1.0),
            cap(Some(W), "W", 1.0),
            cap(Some(E), "E", 1.0),
            cap(Some(R), "R", 1.0),
            cap(Some(T), "T", 1.0),
            cap(Some(Z), "Z", 1.0),
            cap(Some(U), "U", 1.0),
            cap(Some(I), "I", 1.0),
            cap(Some(O), "O", 1.0),
            cap(Some(P), "P", 1.0),
            Cap { shift: Some("`"), ..cap(Some(OpenBracket), "@", 1.0) },
            Cap { shift: Some("^"), ..cap(Some(CloseBracket), "\\", 1.0) },
            cap(Some(Backspace), "\u{2190}", 1.0),
            cap(Some(CursorUp), "\u{2196}", 1.0),
            cap(Some(CursorRight), "\u{2192}", 1.0),
        ],
        // Row 3: home row punctuation after L, then the |<- / END /
        // ->| column.
        vec![
            spacer(1.0),
            cap(Some(A), "A", 1.0),
            cap(Some(S), "S", 1.0),
            cap(Some(D), "D", 1.0),
            cap(Some(F), "F", 1.0),
            cap(Some(G), "G", 1.0),
            cap(Some(H), "H", 1.0),
            cap(Some(J), "J", 1.0),
            cap(Some(K), "K", 1.0),
            cap(Some(L), "L", 1.0),
            Cap { shift: Some("+"), ..cap(Some(Semicolon), ";", 1.0) },
            Cap { shift: Some("*"), ..cap(Some(Quote), ":", 1.0) },
            Cap { shift: Some("]"), ..cap(Some(Backslash), "[", 1.0) },
            cap(Some(LineStart), "|\u{2190}", 1.0),
            cap(Some(CursorDown), "END", 1.0),
            cap(Some(LineEnd), "\u{2192}|", 1.0),
        ],
        // Row 4: Y row (QWERTZ), flanked by SHIFT, then STOP and the
        // two EOL keys.
        vec![
            spacer(1.5),
            cap(Some(Shift), "SHIFT", 1.5),
            cap(Some(Y), "Y", 1.0),
            cap(Some(X), "X", 1.0),
            cap(Some(C), "C", 1.0),
            cap(Some(V), "V", 1.0),
            cap(Some(B), "B", 1.0),
            cap(Some(N), "N", 1.0),
            cap(Some(M), "M", 1.0),
            Cap { shift: Some("<"), ..cap(Some(Comma), ",", 1.0) },
            Cap { shift: Some(">"), ..cap(Some(Period), ".", 1.0) },
            Cap { shift: Some("?"), ..cap(Some(Slash), "/", 1.0) },
            cap(Some(Shift), "SHIFT", 1.5),
            cap(Some(Stop), "STOP", 1.0),
            cap(Some(Enter), "EOL", 1.0),
            cap(Some(Tab), "EOL", 1.0),
        ],
        // Row 5: the space bar.
        vec![spacer(4.5), cap(Some(Space), "SPACE", 8.0)],
    ]
}

fn cap_width(cap: &Cap) -> f32 {
    cap.w * KEY_W + (cap.w - 1.0) * GAP
}

fn cap_height(cap: &Cap) -> f32 {
    cap.h * KEY_H + (cap.h - 1.0) * GAP
}

/// Row height: tall keys (h > 1) spill into the row below, so they do
/// not make their own row taller.
fn row_height(row: &[Cap]) -> f32 {
    row.iter()
        .map(|c| cap_height(c).min(KEY_H))
        .fold(None::<f32>, |a, h| Some(a.map_or(h, |m: f32| m.max(h))))
        .unwrap_or(KEY_H)
}

fn block_size(rows: &[Vec<Cap>]) -> egui::Vec2 {
    let mut w: f32 = 0.0;
    let mut h: f32 = 0.0;
    for row in rows {
        let row_w: f32 = row.iter().map(|c| cap_width(c) + GAP).sum();
        w = w.max(row_w - GAP);
        h += row_height(row) + GAP;
    }
    egui::vec2(w, (h - GAP).max(0.0))
}

fn paint_cap(
    p: &egui::Painter,
    rect: egui::Rect,
    cap: &Cap,
    theme: &Theme,
    kb: &Keyboard,
) {
    if cap.hidden {
        return;
    }
    let c = |f: &[u8; 4]| Theme::color(f);
    let pressed = cap.key.is_some_and(|k| kb.is_pressed(k));
    let fill = if pressed { c(&theme.accent_dim) } else { c(&theme.widget) };
    let stroke = if pressed { c(&theme.accent) } else { c(&theme.panel_border) };
    p.rect_filled(rect, egui::CornerRadius::same(2), fill);
    p.rect_stroke(
        rect,
        egui::CornerRadius::same(2),
        egui::Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    if cap.main.is_empty() {
        return;
    }
    let mono = egui::FontId::monospace;
    let main_color = if pressed { c(&theme.accent) } else { c(&theme.text) };
    let main_size = if cap.main.chars().count() > 1 { 11.0 } else { 13.0 };
    let dy = if cap.sub.is_some() { -5.0 } else { 0.0 };
    p.text(
        rect.center() + egui::vec2(0.0, dy),
        egui::Align2::CENTER_CENTER,
        cap.main,
        mono(main_size),
        main_color,
    );
    if let Some(sub) = cap.sub {
        p.text(
            rect.center() + egui::vec2(0.0, 5.0),
            egui::Align2::CENTER_CENTER,
            sub,
            mono(8.0),
            c(&theme.text_weak),
        );
    }
    if let Some(shift) = cap.shift {
        p.text(
            rect.right_top() + egui::vec2(-4.0, 4.0),
            egui::Align2::RIGHT_TOP,
            shift,
            mono(9.0),
            c(&theme.accent),
        );
    }
}

/// The rect of every cap in the block, in paint order (spacers
/// included).
fn cap_rects(origin: egui::Pos2, rows: &[Vec<Cap>]) -> Vec<(egui::Rect, &Cap)> {
    let mut row_tops = Vec::with_capacity(rows.len());
    let mut y = origin.y;
    for row in rows {
        row_tops.push(y);
        y += row_height(row) + GAP;
    }
    let mut out = Vec::new();
    for (ri, row) in rows.iter().enumerate() {
        let mut x = origin.x;
        for cap in row {
            let rect = egui::Rect::from_min_size(
                egui::pos2(x, row_tops[ri]),
                egui::vec2(cap_width(cap), cap_height(cap)),
            );
            out.push((rect, cap));
            x += cap_width(cap) + GAP;
        }
    }
    out
}

/// The visible cap under `pos`, if any.
fn hit_cap(origin: egui::Pos2, rows: &[Vec<Cap>], pos: egui::Pos2) -> Option<&Cap> {
    cap_rects(origin, rows)
        .into_iter()
        .find(|(r, c)| !c.hidden && r.contains(pos))
        .map(|(_, c)| c)
}

fn draw_block(
    p: &egui::Painter,
    origin: egui::Pos2,
    rows: &[Vec<Cap>],
    theme: &Theme,
    kb: &Keyboard,
) {
    for (rect, cap) in cap_rects(origin, rows) {
        paint_cap(p, rect, cap, theme, kb);
    }
}

/// Apply the cap currently under a held pointer button to the
/// emulator (`None`: the button is up or the pointer left the caps).
/// Holding a cap presses its key, sliding across caps slides the
/// keypress. RST fires the machine reset once per press.
fn apply_pointer(app: &mut App, held: Option<&Cap>) {
    let target = held.and_then(|c| c.key);
    // Unchanged, unless the matrix was reset behind our back (window
    // focus loss) while the same key was pointer-held.
    let unchanged = app.ui.mouse_key == target
        && target.is_none_or(|k| app.machine.bus.keyboard.is_pressed(k));
    if !unchanged {
        if let Some(old) = app.ui.mouse_key.take() {
            app.machine.bus.keyboard.set_key(old, false);
        }
        if let Some(key) = target {
            app.machine.bus.keyboard.set_key(key, true);
        }
        app.ui.mouse_key = target;
    }
    // RST is the hardware reset line, not a matrix key.
    let rst = held.is_some_and(|c| c.reset);
    if rst && !app.ui.mouse_rst {
        app.reset();
    }
    app.ui.mouse_rst = rst;
}

/// Color-coding legend above the layout.
fn legend(ui: &mut egui::Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        let entries = [
            (&theme.text, "key"),
            (&theme.accent, "SHIFT+key"),
        ];
        for (color, label) in entries {
            let (dot, _) =
                ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(dot, egui::CornerRadius::same(1), Theme::color(color));
            ui.label(
                egui::RichText::new(label)
                    .size(10.0)
                    .color(Theme::color(&theme.text_weak)),
            );
            ui.add_space(6.0);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new("PMD 85 \u{b7} caps light up while pressed")
                    .size(10.0)
                    .color(Theme::color(&theme.text_weak)),
            );
        });
    });
}

pub fn draw(ctx: &egui::Context, app: &mut App) {
    let mut open = app.ui.keyboard_open;
    egui::Window::new("Keyboard layout")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .show(ctx, |ui| {
            let theme = app.active_theme_data();
            legend(ui, &theme);
            ui.add_space(4.0);
            let kb = app.machine.bus.keyboard.clone();
            let rows = main_block();
            let total = block_size(&rows);
            // auto_shrink: the window opens at the content size (no
            // scrollbars); scrolling only kicks in if the user shrinks
            // the resizable window below it.
            egui::ScrollArea::both()
                .auto_shrink(true)
                .show(ui, |ui| {
                    let (rect, _) = ui.allocate_exact_size(total, egui::Sense::click());
                    draw_block(ui.painter(), rect.min, &rows, &theme, &kb);
                    // Click-to-press: the cap under a held pointer
                    // button is pressed, sliding slides the keypress.
                    let held = if ui.input(|i| i.pointer.primary_down()) {
                        ui.input(|i| i.pointer.interact_pos())
                            .and_then(|pos| hit_cap(rect.min, &rows, pos))
                    } else {
                        None
                    };
                    apply_pointer(app, held);
                });
        });
    if open != app.ui.keyboard_open {
        app.ui.keyboard_open = open;
    }
    // Closing the window must release a pointer-held key.
    if !app.ui.keyboard_open {
        apply_pointer(app, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All caps with an emulated key in the layout, as (engraving, key).
    fn engraved_keys() -> Vec<(&'static str, Key)> {
        main_block()
            .iter()
            .flatten()
            .filter(|c| !c.hidden)
            .filter_map(|c| c.key.map(|k| (c.main, k)))
            .collect()
    }

    #[test]
    fn layout_rows_have_content() {
        let rows = main_block();
        assert_eq!(rows.len(), 6, "function row + four key rows + space bar");
        // QWERTZ: Z sits between T and U on the Q row, Y below.
        let q_row: Vec<&str> = rows[2].iter().map(|c| c.main).collect();
        let z = q_row.iter().position(|m| *m == "Z").expect("no Z");
        assert!(q_row[..z].contains(&"T"));
        assert!(q_row[z + 1..].contains(&"U"));
        assert!(rows[4].iter().any(|c| c.main == "Y"));
        // Control cluster grid at the row ends.
        assert!(rows[0].iter().any(|c| c.main == "RST"));
        assert!(rows[1].iter().any(|c| c.main == "CLR"));
        assert!(rows[2].iter().any(|c| c.main == "\u{2190}"));
        assert!(rows[3].iter().any(|c| c.main == "END"));
        assert!(rows[4].iter().any(|c| c.main == "STOP"));
        assert!(rows[5].iter().any(|c| c.main == "SPACE"));
        // The rows stagger: rows 2-5 start with an invisible spacer.
        for row in &rows[2..] {
            assert!(row[0].hidden, "row does not start with a spacer");
        }
    }

    #[test]
    fn engravings_match_their_matrix_keys() {
        // The cap engraved X must be keyed to the matrix position that
        // prints X on the real machine (see the monitor decode table).
        let caps = engraved_keys();
        let by_main = |m: &str| {
            caps.iter()
                .find(|(main, _)| *main == m)
                .map(|(_, key)| *key)
                .unwrap_or_else(|| panic!("no cap engraved {m}"))
        };
        assert_eq!(by_main("Z"), Key::Z);
        assert_eq!(by_main("Y"), Key::Y);
        assert_eq!(by_main(";"), Key::Semicolon);
        assert_eq!(by_main(":"), Key::Quote);
        assert_eq!(by_main("@"), Key::OpenBracket);
        assert_eq!(by_main("_"), Key::Minus);
        assert_eq!(by_main("{"), Key::Equals);
        assert_eq!(by_main("["), Key::Backslash);
        assert_eq!(by_main("RCL"), Key::Recall);
        assert_eq!(by_main("CLR"), Key::ClrScr);
    }

    #[test]
    fn mapped_caps_use_valid_keys() {
        let caps = engraved_keys();
        for key in [
            Key::Q, Key::Z, Key::Y, Key::Space, Key::Enter, Key::Tab,
            Key::Stop, Key::Shift, Key::Cd, Key::Wrk, Key::Recall,
            Key::ClrScr, Key::Insert, Key::Delete, Key::LineStart,
            Key::LineEnd, Key::CursorUp, Key::CursorDown,
            Key::CursorRight, Key::Backspace,
        ] {
            assert!(caps.iter().any(|&(_, k)| k == key), "{key:?} missing from the layout");
        }
    }

    #[test]
    fn block_has_sane_size() {
        let rows = main_block();
        let size = block_size(&rows);
        assert!(size.x > 450.0 && size.x < 700.0, "width {size:?}");
        assert!(size.y > 100.0 && size.y < 300.0, "height {size:?}");
    }

    #[test]
    fn hit_cap_hits_keys_but_not_spacers() {
        let rows = main_block();
        let origin = egui::Pos2::ZERO;
        let rects = cap_rects(origin, &rows);
        let a = rects.iter().find(|(_, c)| c.main == "A").unwrap();
        assert_eq!(hit_cap(origin, &rows, a.0.center()).unwrap().key, Some(Key::A));
        // The stagger spacers are not clickable, even though they sit
        // in the row's geometry.
        let spacer = rects.iter().find(|(_, c)| c.hidden).unwrap();
        assert!(hit_cap(origin, &rows, spacer.0.center()).is_none());
        // Neither is the space around the block.
        assert!(hit_cap(origin, &rows, egui::pos2(-50.0, -50.0)).is_none());
        assert!(hit_cap(origin, &rows, egui::pos2(5000.0, 5000.0)).is_none());
    }

    #[test]
    fn clicking_caps_presses_slides_and_releases_keys() {
        let mut app = crate::ui::tests::test_app();
        let rows = main_block();
        let rects = cap_rects(egui::Pos2::ZERO, &rows);
        let find = |name: &str| {
            rects
                .iter()
                .find(|(_, c)| c.main == name)
                .map(|(_, c)| *c)
                .unwrap_or_else(|| panic!("no cap {name}"))
        };
        let (a, s) = (find("A"), find("S"));

        // Holding a cap presses its key.
        apply_pointer(&mut app, Some(a));
        assert!(app.machine.bus.keyboard.is_pressed(Key::A));
        assert_eq!(app.ui.mouse_key, Some(Key::A));
        // Sliding to the neighbouring key swaps the keypress.
        apply_pointer(&mut app, Some(s));
        assert!(!app.machine.bus.keyboard.is_pressed(Key::A));
        assert!(app.machine.bus.keyboard.is_pressed(Key::S));
        // Releasing the pointer releases the key.
        apply_pointer(&mut app, None);
        assert!(!app.machine.bus.keyboard.is_pressed(Key::S));
        assert_eq!(app.ui.mouse_key, None);
    }

    #[test]
    fn clicking_rst_resets_the_machine_once() {
        let mut app = crate::ui::tests::test_app();
        app.advance();
        assert!(app.frames_done > 0);
        let rows = main_block();
        let rst = rows
            .iter()
            .flatten()
            .find(|c| c.reset)
            .expect("no RST cap in the layout");
        // Pressing RST fires the reset line...
        apply_pointer(&mut app, Some(rst));
        assert!(app.ui.mouse_rst);
        assert_eq!(app.frames_done, 0, "RST did not reset the machine");
        // ...and holding it does not fire it again.
        apply_pointer(&mut app, Some(rst));
        assert_eq!(app.frames_done, 0);
        // Releasing the pointer arms it for the next press.
        apply_pointer(&mut app, None);
        assert!(!app.ui.mouse_rst);
        // No matrix key was touched by RST.
        assert_eq!(app.ui.mouse_key, None);
    }
}
