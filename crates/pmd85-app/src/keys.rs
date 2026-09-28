//! Host keyboard mapping: winit physical keys to PMD 85 keys.
//!
//! The PMD 85 has a QWERTZ layout; we map by physical position for the
//! letters that coincide and note that Y/Z swap on real QWERTZ hardware.

use pmd85_core::keyboard::Key;
use winit::event::ElementState;
use winit::keyboard::{KeyCode, PhysicalKey};

/// Map a winit keyboard event to an emulated key press/release.
pub fn map(event: &winit::event::KeyEvent) -> Option<Key> {
    let PhysicalKey::Code(code) = event.physical_key else {
        return None;
    };
    match code {
        KeyCode::KeyA => Some(Key::A),
        KeyCode::KeyB => Some(Key::B),
        KeyCode::KeyC => Some(Key::C),
        KeyCode::KeyD => Some(Key::D),
        KeyCode::KeyE => Some(Key::E),
        KeyCode::KeyF => Some(Key::F),
        KeyCode::KeyG => Some(Key::G),
        KeyCode::KeyH => Some(Key::H),
        KeyCode::KeyI => Some(Key::I),
        KeyCode::KeyJ => Some(Key::J),
        KeyCode::KeyK => Some(Key::K),
        KeyCode::KeyL => Some(Key::L),
        KeyCode::KeyM => Some(Key::M),
        KeyCode::KeyN => Some(Key::N),
        KeyCode::KeyO => Some(Key::O),
        KeyCode::KeyP => Some(Key::P),
        KeyCode::KeyQ => Some(Key::Q),
        KeyCode::KeyR => Some(Key::R),
        KeyCode::KeyS => Some(Key::S),
        KeyCode::KeyT => Some(Key::T),
        KeyCode::KeyU => Some(Key::U),
        KeyCode::KeyV => Some(Key::V),
        KeyCode::KeyW => Some(Key::W),
        KeyCode::KeyX => Some(Key::X),
        // PMD 85 is QWERTZ: host Z is at the PMD Y position and vice versa.
        KeyCode::KeyY => Some(Key::Z),
        KeyCode::KeyZ => Some(Key::Y),
        KeyCode::Digit0 => Some(Key::Digit0),
        KeyCode::Digit1 => Some(Key::Digit1),
        KeyCode::Digit2 => Some(Key::Digit2),
        KeyCode::Digit3 => Some(Key::Digit3),
        KeyCode::Digit4 => Some(Key::Digit4),
        KeyCode::Digit5 => Some(Key::Digit5),
        KeyCode::Digit6 => Some(Key::Digit6),
        KeyCode::Digit7 => Some(Key::Digit7),
        KeyCode::Digit8 => Some(Key::Digit8),
        KeyCode::Digit9 => Some(Key::Digit9),
        KeyCode::Space => Some(Key::Space),
        KeyCode::Enter | KeyCode::NumpadEnter => Some(Key::Enter),
        KeyCode::Tab => Some(Key::Tab),
        KeyCode::Backspace => Some(Key::Backspace),
        KeyCode::Delete => Some(Key::Delete),
        KeyCode::Insert => Some(Key::Insert),
        KeyCode::ArrowLeft => Some(Key::CursorLeft),
        KeyCode::ArrowRight => Some(Key::CursorRight),
        KeyCode::ArrowUp => Some(Key::CursorUp),
        KeyCode::ArrowDown => Some(Key::CursorDown),
        KeyCode::Home => Some(Key::LineStart),
        KeyCode::End => Some(Key::LineEnd),
        KeyCode::ShiftLeft | KeyCode::ShiftRight => Some(Key::Shift),
        // STOP doubles as the Break-like key on the host.
        KeyCode::Escape | KeyCode::ControlLeft | KeyCode::ControlRight => Some(Key::Stop),
        // F1..F12 -> K0..K11 function keys.
        KeyCode::F1 => Some(Key::K0),
        KeyCode::F2 => Some(Key::K1),
        KeyCode::F3 => Some(Key::K2),
        KeyCode::F4 => Some(Key::K3),
        KeyCode::F5 => Some(Key::K4),
        KeyCode::F6 => Some(Key::K5),
        KeyCode::F7 => Some(Key::K6),
        KeyCode::F8 => Some(Key::K7),
        KeyCode::F9 => Some(Key::K8),
        KeyCode::F10 => Some(Key::K9),
        KeyCode::F11 => Some(Key::K10),
        KeyCode::F12 => Some(Key::K11),
        _ => None,
    }
}

/// Whether the event is a press or release (helper kept for readability).
#[allow(dead_code)]
pub fn is_press(event: &winit::event::KeyEvent) -> bool {
    event.state == ElementState::Pressed
}
