//! Reference view of the PMD 85-3 keyboard layout, shown in its own
//! window (toggled from the transport bar).
//!
//! Each keycap shows the main engraving (plain keypress) centered,
//! the SHIFT engraving in the top-right corner, the STOP engraving in
//! the bottom-right corner and the SHIFT+STOP engraving in the
//! bottom-left corner — see the legend at the top of the window.
//! Caps light up while the corresponding emulated key is held down.
//!
//! The layout (positions and engravings) follows the reference
//! emulator's PMD 85-3 chart; caps without an emulated key (the
//! numeric keypad, CAPS LOCK, the unassigned function keys) are
//! cosmetic only.

use crate::app::App;
use crate::ui::theme::Theme;
use pmd85_core::keyboard::{Key, Keyboard};

/// Key geometry, in points.
const KEY_W: f32 = 30.0;
const KEY_H: f32 = 26.0;
const GAP: f32 = 4.0;
/// Gap between the main block and the right-hand clusters.
const CLUSTER_GAP: f32 = 14.0;

/// One keycap of the reference layout.
#[derive(Clone, Copy)]
struct Cap {
    /// Emulated key, for the live pressed highlight. `None` = the cap
    /// has no host mapping (cosmetic only).
    key: Option<Key>,
    /// Main engraving (plain keypress).
    main: &'static str,
    /// Small secondary engraving under the main one.
    sub: Option<&'static str>,
    /// SHIFT engraving.
    shift: Option<&'static str>,
    /// STOP engraving.
    stop: Option<&'static str>,
    /// SHIFT+STOP engraving.
    shift_stop: Option<&'static str>,
    /// Width in key units.
    w: f32,
    /// Height in key units (tall keys spill into the row below,
    /// which reserves the space with an empty cap).
    h: f32,
}

fn cap(key: Option<Key>, main: &'static str, w: f32) -> Cap {
    Cap {
        key,
        main,
        sub: None,
        shift: None,
        stop: None,
        shift_stop: None,
        w,
        h: 1.0,
    }
}

/// The main alpha-numeric block: function row plus five rows.
fn main_block() -> Vec<Vec<Cap>> {
    use Key::*;
    let mut rows: Vec<Vec<Cap>> = Vec::new();

    // Function row: STOP, K0..K11, three unassigned keys.
    let mut row = vec![cap(Some(Stop), "STOP", 1.2)];
    let fn_keys = [
        (K0, "K0"), (K1, "K1"), (K2, "K2"), (K3, "K3"),
        (K4, "K4"), (K5, "K5"), (K6, "K6"), (K7, "K7"),
        (K8, "K8"), (K9, "K9"), (K10, "K10"), (K11, "K11"),
    ];
    for (k, name) in fn_keys {
        row.push(cap(Some(k), name, 1.0));
    }
    row.extend([cap(None, "", 1.0), cap(None, "", 1.0), cap(None, "", 1.0)]);
    rows.push(row);

    // Row 1.
    rows.push(vec![
        cap(Some(Wrk), "WRK", 1.4),
        Cap { shift: Some("!"), ..cap(Some(Digit1), "1", 1.0) },
        Cap { shift: Some("\""), ..cap(Some(Digit2), "2", 1.0) },
        Cap { shift: Some("#"), ..cap(Some(Digit3), "3", 1.0) },
        Cap { shift: Some("$"), ..cap(Some(Digit4), "4", 1.0) },
        Cap { shift: Some("%"), ..cap(Some(Digit5), "5", 1.0) },
        Cap { shift: Some("&"), ..cap(Some(Digit6), "6", 1.0) },
        Cap { shift: Some("'"), ..cap(Some(Digit7), "7", 1.0) },
        Cap { shift: Some("("), ..cap(Some(Digit8), "8", 1.0) },
        Cap { shift: Some(")"), ..cap(Some(Digit9), "9", 1.0) },
        cap(Some(Digit0), "0", 1.0),
        Cap { shift: Some("="), ..cap(Some(Minus), "-", 1.0) },
        Cap { shift: Some("["), ..cap(Some(Equals), "]", 1.0) },
        cap(Some(Backspace), "\u{2190}", 1.6),
    ]);

    // Row 2 (QWERTZ: Z is on this row, Y below).
    rows.push(vec![
        cap(Some(Cd), "C-D", 1.4),
        Cap { stop: Some("\u{e4}"), ..cap(Some(Q), "Q", 1.0) },   // \u{e4} = a-diaeresis
        Cap { stop: Some("\u{e9}"), ..cap(Some(W), "W", 1.0) },   // e-acute
        Cap { stop: Some("\u{11b}"), ..cap(Some(E), "E", 1.0) },   // e-caron
        Cap { stop: Some("\u{159}"), ..cap(Some(R), "R", 1.0) },   // r-caron
        Cap { stop: Some("\u{165}"), ..cap(Some(T), "T", 1.0) },   // t-caron
        Cap { stop: Some("\u{17e}"), ..cap(Some(Z), "Z", 1.0) },   // z-caron
        Cap { stop: Some("\u{fa}"), ..cap(Some(U), "U", 1.0) },   // u-acute
        Cap { stop: Some("\u{ed}"), ..cap(Some(I), "I", 1.0) },   // i-acute
        Cap { stop: Some("\u{f3}"), ..cap(Some(O), "O", 1.0) },   // o-acute
        Cap { stop: Some("\u{f4}"), ..cap(Some(P), "P", 1.0) },   // o-circumflex
        Cap { shift: Some("\u{3c0}"), stop: Some("\u{222b}"), ..cap(Some(OpenBracket), "@", 1.0) }, // pi, integral
        Cap { shift: Some("^"), ..cap(Some(CloseBracket), "\\", 1.0) },
        Cap { shift: Some("{"), stop: Some("CAPS"), ..cap(Some(Backslash), "}", 1.3) },
    ]);

    // Row 3.
    rows.push(vec![
        Cap { sub: Some("LOCK"), ..cap(None, "CAPS", 1.7) },
        Cap { stop: Some("\u{e1}"), ..cap(Some(A), "A", 1.0) },   // a-acute
        Cap { stop: Some("\u{161}"), ..cap(Some(S), "S", 1.0) },   // s-caron
        Cap { stop: Some("\u{10f}"), ..cap(Some(D), "D", 1.0) },   // d-caron
        cap(Some(F), "F", 1.0),
        cap(Some(G), "G", 1.0),
        Cap { stop: Some("\u{fc}"), ..cap(Some(H), "H", 1.0) },   // u-diaeresis
        Cap { stop: Some("\u{16f}"), ..cap(Some(J), "J", 1.0) },   // u-ring
        Cap { stop: Some("\u{13e}"), ..cap(Some(K), "K", 1.0) },   // l-caron
        Cap { stop: Some("\u{13a}"), ..cap(Some(L), "L", 1.0) },   // l-acute
        Cap { shift: Some("+"), ..cap(Some(Semicolon), ":", 1.0) },
        Cap { shift: Some("*"), ..cap(Some(Quote), ";", 1.0) },
        cap(Some(Enter), "EOL", 1.6),
    ]);

    // Row 4.
    rows.push(vec![
        cap(Some(Shift), "SHIFT", 2.2),
        Cap { stop: Some("\u{fd}"), ..cap(Some(Y), "Y", 1.0) },   // y-acute
        Cap { stop: Some("\u{e0}"), ..cap(Some(X), "X", 1.0) },   // a-grave
        Cap { stop: Some("\u{10d}"), ..cap(Some(C), "C", 1.0) },   // c-caron
        Cap { stop: Some("\u{3b2}"), shift_stop: Some("\u{3b4}"), ..cap(Some(V), "V", 1.0) }, // beta, delta
        Cap { stop: Some("\u{3b1}"), shift_stop: Some("\u{3b3}"), ..cap(Some(B), "B", 1.0) }, // alpha, gamma
        Cap { stop: Some("\u{148}"), ..cap(Some(N), "N", 1.0) },   // n-caron
        Cap { stop: Some("\u{f6}"), ..cap(Some(M), "M", 1.0) },   // o-diaeresis
        Cap { shift: Some("<"), ..cap(Some(Comma), ",", 1.0) },
        Cap { shift: Some(">"), ..cap(Some(Period), ".", 1.0) },
        Cap { shift: Some("?"), ..cap(Some(Slash), "/", 1.0) },
        cap(Some(Shift), "SHIFT", 2.2),
    ]);

    // Row 5.
    rows.push(vec![
        cap(Some(Stop), "STOP", 2.2),
        cap(Some(Space), "SPACE", 10.0),
        cap(Some(Stop), "STOP", 2.2),
    ]);

    rows
}

/// The right-hand clusters, laid out next to the main block (see
/// [`cluster_geometry`]): the control grid at the top, the cursor
/// arrows bottom-aligned with the main block, the numeric keypad to
/// the right of the arrows.
fn control_rows() -> Vec<Vec<Cap>> {
    use Key::*;
    vec![
        vec![
            Cap { sub: Some("INS"), ..cap(Some(Insert), "PTL", 1.4) },
            cap(None, "\u{2196}", 1.0),
            cap(Some(Recall), "RCL", 1.0),
        ],
        vec![
            cap(Some(Delete), "DEL", 1.4),
            cap(None, "END", 1.0),
            cap(Some(ClrScr), "CLR", 1.0),
        ],
    ]
}

/// Cursor arrows: the classic inverted T, flanked by the line keys.
/// `|\u{2190}` and `\u{2192}|` share the top row so that the up arrow
/// sits directly above the down arrow.
fn arrow_rows() -> Vec<Vec<Cap>> {
    use Key::*;
    vec![
        vec![
            cap(Some(LineStart), "|\u{2190}", 1.0),
            cap(Some(CursorUp), "\u{2191}", 1.0),
            cap(Some(LineEnd), "\u{2192}|", 1.0),
        ],
        vec![
            cap(Some(CursorLeft), "\u{2190}", 1.0),
            cap(Some(CursorDown), "\u{2193}", 1.0),
            cap(Some(CursorRight), "\u{2192}", 1.0),
        ],
    ]
}

/// Numeric keypad (cosmetic: the core has no keypad keys, except the
/// second EOL). Tall keys spill into the row below, which reserves
/// the space with an empty cap.
fn numpad_rows() -> Vec<Vec<Cap>> {
    vec![
        vec![
            cap(None, "", 1.0), cap(None, "/", 1.0),
            cap(None, "*", 1.0), cap(None, "-", 1.0),
        ],
        vec![
            cap(None, "7", 1.0), cap(None, "8", 1.0),
            cap(None, "9", 1.0), Cap { h: 2.0, ..cap(None, "+", 1.0) },
        ],
        vec![
            cap(None, "4", 1.0), cap(None, "5", 1.0),
            cap(None, "6", 1.0), cap(None, "", 1.0),
        ],
        vec![
            cap(None, "1", 1.0), cap(None, "2", 1.0),
            cap(None, "3", 1.0), Cap { h: 2.0, ..cap(Some(Key::Tab), "EOL", 1.0) },
        ],
        vec![
            Cap { w: 2.0, ..cap(None, "0", 1.0) },
            cap(None, ".", 1.0),
            cap(None, "", 1.0),
        ],
    ]
}

/// Placement of the right-hand clusters relative to the main block.
///
/// The control grid and the keypad are top-aligned with the main
/// block; the arrows are bottom-aligned with it (keeping clear of the
/// control grid above) and centered in their column; the keypad is to
/// the right of the arrows. Returns the total content size and the
/// origins of the three clusters.
fn cluster_geometry(
    main: egui::Vec2,
    control: egui::Vec2,
    arrows: egui::Vec2,
    numpad: egui::Vec2,
) -> (egui::Vec2, egui::Pos2, egui::Pos2, egui::Pos2) {
    let col_x = main.x + CLUSTER_GAP;
    let col_w = control.x.max(arrows.x);
    let control_pos = egui::pos2(col_x, 0.0);
    let arrows_pos = egui::pos2(
        col_x + (col_w - arrows.x) / 2.0,
        (main.y - arrows.y).max(control.y + CLUSTER_GAP),
    );
    let numpad_pos = egui::pos2(col_x + col_w + CLUSTER_GAP, 0.0);
    let total = egui::vec2(
        numpad_pos.x + numpad.x,
        main
            .y
            .max(arrows_pos.y + arrows.y)
            .max(numpad.y)
            .max(control.y),
    );
    (total, control_pos, arrows_pos, numpad_pos)
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
    let inset = 4.0;
    if let Some(shift) = cap.shift {
        p.text(
            rect.right_top() + egui::vec2(-inset, inset),
            egui::Align2::RIGHT_TOP,
            shift,
            mono(9.0),
            c(&theme.accent),
        );
    }
    if let Some(stop) = cap.stop {
        p.text(
            rect.right_bottom() + egui::vec2(-inset, -2.0),
            egui::Align2::RIGHT_BOTTOM,
            stop,
            mono(9.0),
            c(&theme.danger),
        );
    }
    if let Some(shift_stop) = cap.shift_stop {
        p.text(
            rect.left_bottom() + egui::vec2(inset, -2.0),
            egui::Align2::LEFT_BOTTOM,
            shift_stop,
            mono(9.0),
            c(&theme.warn),
        );
    }
}

/// Paint a block of rows at `origin`; returns its size. Rows are
/// painted bottom-up so tall keys cover the empty caps reserving
/// their space in the row below.
fn draw_block(
    p: &egui::Painter,
    origin: egui::Pos2,
    rows: &[Vec<Cap>],
    theme: &Theme,
    kb: &Keyboard,
) {
    let mut row_tops = Vec::with_capacity(rows.len());
    let mut y = origin.y;
    for row in rows {
        row_tops.push(y);
        y += row_height(row) + GAP;
    }
    for (ri, row) in rows.iter().enumerate().rev() {
        let mut x = origin.x;
        for cap in row {
            let rect =
                egui::Rect::from_min_size(egui::pos2(x, row_tops[ri]), egui::vec2(cap_width(cap), cap_height(cap)));
            paint_cap(p, rect, cap, theme, kb);
            x += cap_width(cap) + GAP;
        }
    }
}

/// Color-coding legend above the layout.
fn legend(ui: &mut egui::Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        let entries = [
            (&theme.text, "key"),
            (&theme.accent, "SHIFT+key"),
            (&theme.danger, "STOP+key"),
            (&theme.warn, "SHIFT+STOP+key"),
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
                egui::RichText::new("PMD 85-3 \u{b7} caps light up while pressed")
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
            let main = main_block();
            let control = control_rows();
            let arrows = arrow_rows();
            let numpad = numpad_rows();
            let main_size = block_size(&main);
            let (total, control_pos, arrows_pos, numpad_pos) = cluster_geometry(
                main_size,
                block_size(&control),
                block_size(&arrows),
                block_size(&numpad),
            );
            // auto_shrink: the window opens at the content size (no
            // scrollbars); scrolling only kicks in if the user shrinks
            // the resizable window below it.
            egui::ScrollArea::both()
                .auto_shrink(true)
                .show(ui, |ui| {
                    let (rect, _) = ui.allocate_exact_size(total, egui::Sense::hover());
                    let p = ui.painter();
                    draw_block(p, rect.min, &main, &theme, &kb);
                    draw_block(p, rect.min + control_pos.to_vec2(), &control, &theme, &kb);
                    draw_block(p, rect.min + arrows_pos.to_vec2(), &arrows, &theme, &kb);
                    draw_block(p, rect.min + numpad_pos.to_vec2(), &numpad, &theme, &kb);
                });
        });
    if open != app.ui.keyboard_open {
        app.ui.keyboard_open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_rows_have_content() {
        let main = main_block();
        assert_eq!(main.len(), 6, "function row + five main rows");
        // Every row has at least the reference keys.
        assert!(main[1].len() >= 13);
        assert!(main[5].iter().any(|c| c.main == "SPACE"));
        let arrows = arrow_rows();
        // Classic inverted T: the up arrow directly above the down arrow.
        assert_eq!(arrows.len(), 2);
        assert_eq!(arrows[0][1].main, "\u{2191}");
        assert_eq!(arrows[1][1].main, "\u{2193}");
        assert_eq!(arrows[0][1].key, Some(Key::CursorUp));
        assert_eq!(arrows[1][1].key, Some(Key::CursorDown));
        // The tall numpad keys are followed by an empty cap below them.
        let numpad = numpad_rows();
        assert_eq!(numpad.len(), 5);
        assert!(numpad[1].iter().any(|c| c.h > 1.0 && c.main == "+"));
        assert!(numpad[2].last().unwrap().main.is_empty());
    }

    #[test]
    fn clusters_place_numpad_right_and_arrows_bottom_aligned() {
        let main = block_size(&main_block());
        let control = block_size(&control_rows());
        let arrows = block_size(&arrow_rows());
        let numpad = block_size(&numpad_rows());
        let (total, control_pos, arrows_pos, numpad_pos) =
            cluster_geometry(main, control, arrows, numpad);

        // The arrows sit clear of the control grid, bottom-aligned
        // with the main block.
        assert!(arrows_pos.y >= control_pos.y + control.y + CLUSTER_GAP);
        assert!(
            (arrows_pos.y + arrows.y - main.y).abs() < 1e-3,
            "arrows not bottom-aligned with the main block"
        );
        // The numpad is to the right of the arrows.
        assert!(numpad_pos.x >= arrows_pos.x + arrows.x);
        // Everything fits within the total size.
        assert!(total.x >= numpad_pos.x + numpad.x - 1e-3);
        assert!(total.y >= arrows_pos.y + arrows.y - 1e-3);
        // The whole board fits a normal screen without scrolling.
        assert!(
            total.x > 700.0 && total.x < 900.0,
            "total width {total:?}"
        );
    }

    #[test]
    fn mapped_caps_use_valid_keys() {
        // Every mapped cap must be a real emulated key (spot-check the
        // mapping compiles against the enum and covers the letters).
        let main = main_block();
        let letters: Vec<_> = main
            .iter()
            .flatten()
            .filter_map(|c| c.key)
            .collect();
        for key in [Key::Q, Key::Z, Key::Y, Key::Space, Key::Enter, Key::Stop, Key::Shift] {
            assert!(letters.contains(&key), "{key:?} missing from the layout");
        }
    }
}
