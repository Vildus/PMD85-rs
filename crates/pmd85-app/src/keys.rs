//! Host keyboard mapping: winit physical keys to PMD 85 keys.
//!
//! The map is positional: the host key sitting at the same physical
//! location as a PMD 85 key presses that key. The PMD 85 is QWERTZ
//! (Z in the Q row, Y in the bottom row), so the host Y/Z keys swap
//! meanings, and the keys right of P map to the PMD `@` and `\` keys.
//! The PMD 85 has no numeric keypad: the host numpad duplicates the
//! main digits, and its operators press SHIFT combos of the PMD keys
//! that print them (`+` is SHIFT + the `; +` key, `-` SHIFT + `0`,
//! `*` SHIFT + the `: *` key).

use pmd85_core::keyboard::Key;
use winit::event::KeyEvent;
use winit::keyboard::{KeyCode, PhysicalKey};

/// The PMD keys pressed by a host key: usually one, but numpad
/// operators also press SHIFT.
fn keys_for(code: KeyCode) -> &'static [Key] {
    use Key::*;
    match code {
        // Q row, positional (QWERTZ): host Y sits where the PMD has Z.
        KeyCode::KeyQ => &[Q],
        KeyCode::KeyW => &[W],
        KeyCode::KeyE => &[E],
        KeyCode::KeyR => &[R],
        KeyCode::KeyT => &[T],
        KeyCode::KeyY => &[Z],
        KeyCode::KeyU => &[U],
        KeyCode::KeyI => &[I],
        KeyCode::KeyO => &[O],
        KeyCode::KeyP => &[P],
        KeyCode::BracketLeft => &[OpenBracket],  // @ `
        KeyCode::BracketRight => &[CloseBracket], // \ ^
        KeyCode::Backslash => &[Backslash],      // [ ]

        // A row.
        KeyCode::KeyA => &[A],
        KeyCode::KeyS => &[S],
        KeyCode::KeyD => &[D],
        KeyCode::KeyF => &[F],
        KeyCode::KeyG => &[G],
        KeyCode::KeyH => &[H],
        KeyCode::KeyJ => &[J],
        KeyCode::KeyK => &[K],
        KeyCode::KeyL => &[L],
        KeyCode::Semicolon => &[Semicolon], // ; +
        KeyCode::Quote => &[Quote],         // : *

        // Y row, positional: host Z sits where the PMD has Y.
        KeyCode::KeyZ => &[Y],
        KeyCode::KeyX => &[X],
        KeyCode::KeyC => &[C],
        KeyCode::KeyV => &[V],
        KeyCode::KeyB => &[B],
        KeyCode::KeyN => &[N],
        KeyCode::KeyM => &[M],
        KeyCode::Comma => &[Comma],
        KeyCode::Period => &[Period],
        KeyCode::Slash => &[Slash],

        // Digit row.
        KeyCode::Digit1 => &[Digit1],
        KeyCode::Digit2 => &[Digit2],
        KeyCode::Digit3 => &[Digit3],
        KeyCode::Digit4 => &[Digit4],
        KeyCode::Digit5 => &[Digit5],
        KeyCode::Digit6 => &[Digit6],
        KeyCode::Digit7 => &[Digit7],
        KeyCode::Digit8 => &[Digit8],
        KeyCode::Digit9 => &[Digit9],
        KeyCode::Digit0 => &[Digit0],
        KeyCode::Minus => &[Minus],   // _ =
        KeyCode::Equal => &[Equals], // { }

        KeyCode::Space => &[Space],
        KeyCode::Enter | KeyCode::NumpadEnter => &[Enter],
        KeyCode::ShiftLeft | KeyCode::ShiftRight => &[Shift],
        // STOP sits where the host has its Control keys.
        KeyCode::Escape | KeyCode::ControlLeft | KeyCode::ControlRight => &[Stop],

        // Control cluster: positional neighbors of the cluster grid.
        KeyCode::Backspace => &[Backspace], // left arrow key
        KeyCode::Insert => &[Insert],       // PTL/INS
        KeyCode::Delete => &[Delete],
        KeyCode::Tab => &[Cd],
        KeyCode::Backquote => &[Wrk],
        KeyCode::PageUp => &[Recall],
        KeyCode::PageDown => &[ClrScr],
        KeyCode::ArrowLeft => &[CursorLeft],
        KeyCode::ArrowUp => &[CursorUp],     // the home-arrow key
        KeyCode::ArrowDown => &[CursorDown], // the END key
        KeyCode::ArrowRight => &[CursorRight],
        KeyCode::Home => &[LineStart], // |<-
        KeyCode::End => &[LineEnd],   // ->|

        // F1..F12 -> K0..K11 function keys.
        KeyCode::F1 => &[K0],
        KeyCode::F2 => &[K1],
        KeyCode::F3 => &[K2],
        KeyCode::F4 => &[K3],
        KeyCode::F5 => &[K4],
        KeyCode::F6 => &[K5],
        KeyCode::F7 => &[K6],
        KeyCode::F8 => &[K7],
        KeyCode::F9 => &[K8],
        KeyCode::F10 => &[K9],
        KeyCode::F11 => &[K10],
        KeyCode::F12 => &[K11],

        // Numpad: duplicates of the main keys (the PMD 85 has none).
        KeyCode::Numpad0 => &[Digit0],
        KeyCode::Numpad1 => &[Digit1],
        KeyCode::Numpad2 => &[Digit2],
        KeyCode::Numpad3 => &[Digit3],
        KeyCode::Numpad4 => &[Digit4],
        KeyCode::Numpad5 => &[Digit5],
        KeyCode::Numpad6 => &[Digit6],
        KeyCode::Numpad7 => &[Digit7],
        KeyCode::Numpad8 => &[Digit8],
        KeyCode::Numpad9 => &[Digit9],
        KeyCode::NumpadDecimal => &[Period],
        KeyCode::NumpadDivide => &[Slash],
        KeyCode::NumpadMultiply => &[Shift, Quote],    // *
        KeyCode::NumpadAdd => &[Shift, Semicolon],     // +
        KeyCode::NumpadSubtract => &[Shift, Digit0],  // -

        _ => &[],
    }
}

/// Map a winit keyboard event to the emulated keys it presses.
pub fn map(event: &KeyEvent) -> &'static [Key] {
    let PhysicalKey::Code(code) = event.physical_key else {
        return &[];
    };
    keys_for(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qwertz_swaps_y_and_z() {
        assert_eq!(keys_for(KeyCode::KeyY), &[Key::Z]);
        assert_eq!(keys_for(KeyCode::KeyZ), &[Key::Y]);
        assert_eq!(keys_for(KeyCode::KeyQ), &[Key::Q]);
        assert_eq!(keys_for(KeyCode::KeyP), &[Key::P]);
    }

    #[test]
    fn punctuation_maps_to_the_keys_that_print_it() {
        // Host position -> PMD key whose engraving matches (see the
        // monitor decode table): ; + : * @ ` \ ^ _ = { } [ ].
        assert_eq!(keys_for(KeyCode::Semicolon), &[Key::Semicolon]);
        assert_eq!(keys_for(KeyCode::Quote), &[Key::Quote]);
        assert_eq!(keys_for(KeyCode::BracketLeft), &[Key::OpenBracket]);
        assert_eq!(keys_for(KeyCode::BracketRight), &[Key::CloseBracket]);
        assert_eq!(keys_for(KeyCode::Backslash), &[Key::Backslash]);
        assert_eq!(keys_for(KeyCode::Minus), &[Key::Minus]);
        assert_eq!(keys_for(KeyCode::Equal), &[Key::Equals]);
        assert_eq!(keys_for(KeyCode::Comma), &[Key::Comma]);
        assert_eq!(keys_for(KeyCode::Period), &[Key::Period]);
        assert_eq!(keys_for(KeyCode::Slash), &[Key::Slash]);
    }

    #[test]
    fn numpad_duplicates_the_main_keys() {
        assert_eq!(keys_for(KeyCode::Numpad7), &[Key::Digit7]);
        assert_eq!(keys_for(KeyCode::Numpad0), &[Key::Digit0]);
        assert_eq!(keys_for(KeyCode::NumpadDecimal), &[Key::Period]);
        assert_eq!(keys_for(KeyCode::NumpadDivide), &[Key::Slash]);
        assert_eq!(keys_for(KeyCode::NumpadEnter), &[Key::Enter]);
    }

    #[test]
    fn numpad_operators_press_shift_combos() {
        // + is SHIFT + the `; +` key, - SHIFT + `0`, * SHIFT + `: *`.
        assert_eq!(keys_for(KeyCode::NumpadAdd), &[Key::Shift, Key::Semicolon]);
        assert_eq!(keys_for(KeyCode::NumpadSubtract), &[Key::Shift, Key::Digit0]);
        assert_eq!(keys_for(KeyCode::NumpadMultiply), &[Key::Shift, Key::Quote]);
    }

    #[test]
    fn control_cluster_maps_positionally() {
        assert_eq!(keys_for(KeyCode::Tab), &[Key::Cd]);
        assert_eq!(keys_for(KeyCode::Backquote), &[Key::Wrk]);
        assert_eq!(keys_for(KeyCode::PageUp), &[Key::Recall]);
        assert_eq!(keys_for(KeyCode::PageDown), &[Key::ClrScr]);
        assert_eq!(keys_for(KeyCode::Backspace), &[Key::Backspace]);
        assert_eq!(keys_for(KeyCode::Home), &[Key::LineStart]);
        assert_eq!(keys_for(KeyCode::End), &[Key::LineEnd]);
    }

    #[test]
    fn unknown_keys_map_to_nothing() {
        assert!(keys_for(KeyCode::NumLock).is_empty());
    }
}
