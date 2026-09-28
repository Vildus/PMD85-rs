//! PMD 85 keyboard: a 16-column x 8-bit matrix scanned through the
//! motherboard 8255 (column number written to port A, row state read from
//! port B). Column 15 carries the special Shift/Stop keys, so it is ANDed
//! into every scan, exactly like the hardware diode arrangement.
//!
//! Matrix positions and legends follow MAME's driver (src/mame/tesla/pmd85.cpp).

/// A key of the emulated PMD 85 keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    K0,
    K1,
    K2,
    K3,
    K4,
    K5,
    K6,
    K7,
    K8,
    K9,
    K10,
    K11,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Digit0,
    Minus,   // PMD key between 0 and backspace (prints = / _)
    Equals,  // PMD key right of Minus (prints { } )
    Q,
    W,
    E,
    R,
    T,
    Y,
    U,
    I,
    O,
    P,
    OpenBracket,  // @ ` position
    CloseBracket, // \ ^ position
    A,
    S,
    D,
    F,
    G,
    H,
    J,
    K,
    L,
    Semicolon, // ; +
    Quote,     // : *
    Z,
    X,
    C,
    V,
    B,
    N,
    M,
    Comma,
    Period,
    Slash,
    Backslash, // [ ]
    Space,
    Enter, // EOL1
    Tab,   // EOL2
    Backspace, // "<-" left arrow
    Insert,    // INS PTL ("|<-")
    Delete,    // DEL
    ClrScr,    // CLR
    Recall,    // RCL
    CursorLeft,
    CursorRight,
    CursorUp,
    CursorDown,
    LineStart, // "|<-"/"->|" style keys are on Alt; use Home/End-like semantics
    LineEnd,
    Wrk,
    Cd, // C-D (clear line)
    Shift,
    Stop,
}

/// Matrix coordinates (column, bit) of each key, active low.
pub fn matrix_pos(key: Key) -> (usize, u8) {
    use Key::*;
    match key {
        K0 => (0, 0x01),
        Digit1 => (0, 0x02),
        Q => (0, 0x04),
        A => (0, 0x08),
        Space => (0, 0x10),

        K1 => (1, 0x01),
        Digit2 => (1, 0x02),
        W => (1, 0x04),
        S => (1, 0x08),
        Z => (1, 0x10),

        K2 => (2, 0x01),
        Digit3 => (2, 0x02),
        E => (2, 0x04),
        D => (2, 0x08),
        X => (2, 0x10),

        K3 => (3, 0x01),
        Digit4 => (3, 0x02),
        R => (3, 0x04),
        F => (3, 0x08),
        C => (3, 0x10),

        K4 => (4, 0x01),
        Digit5 => (4, 0x02),
        T => (4, 0x04),
        G => (4, 0x08),
        V => (4, 0x10),

        K5 => (5, 0x01),
        Digit6 => (5, 0x02),
        Y => (5, 0x04),
        H => (5, 0x08),
        B => (5, 0x10),

        K6 => (6, 0x01),
        Digit7 => (6, 0x02),
        U => (6, 0x04),
        J => (6, 0x08),
        N => (6, 0x10),

        K7 => (7, 0x01),
        Digit8 => (7, 0x02),
        I => (7, 0x04),
        K => (7, 0x08),
        M => (7, 0x10),

        K8 => (8, 0x01),
        Digit9 => (8, 0x02),
        O => (8, 0x04),
        L => (8, 0x08),
        Comma => (8, 0x10),

        K9 => (9, 0x01),
        Digit0 => (9, 0x02),
        P => (9, 0x04),
        Semicolon => (9, 0x08),
        Period => (9, 0x10),

        K10 => (10, 0x01),
        Minus => (10, 0x02),
        OpenBracket => (10, 0x04),
        Quote => (10, 0x08),
        Slash => (10, 0x10),

        K11 => (11, 0x01),
        Equals => (11, 0x02),
        CloseBracket => (11, 0x04),
        Backslash => (11, 0x08),

        Wrk => (12, 0x01),
        Insert => (12, 0x02),
        Backspace => (12, 0x04),
        CursorLeft => (12, 0x04), // same physical key as "<-"
        LineStart => (12, 0x08),

        Cd => (13, 0x01),
        Delete => (13, 0x02),
        CursorUp => (13, 0x04),
        CursorDown => (13, 0x08),
        Enter => (13, 0x10),

        ClrScr => (14, 0x01),
        Recall => (14, 0x02),
        CursorRight => (14, 0x04),
        LineEnd => (14, 0x08),
        Tab => (14, 0x10),

        Shift => (15, 0x20),
        Stop => (15, 0x40),
    }
}

/// State of all PMD 85 keys as a 16-column active-low matrix.
#[derive(Clone, Debug)]
pub struct Keyboard {
    /// Active-low columns: a clear bit means the key is pressed.
    matrix: [u8; 16],
}

impl Default for Keyboard {
    fn default() -> Self {
        Keyboard::new()
    }
}

impl Keyboard {
    pub fn new() -> Self {
        Keyboard { matrix: [0xFF; 16] }
    }

    pub fn reset(&mut self) {
        self.matrix = [0xFF; 16];
    }

    pub fn set_key(&mut self, key: Key, pressed: bool) {
        let (col, bit) = matrix_pos(key);
        if pressed {
            self.matrix[col] &= !bit;
        } else {
            self.matrix[col] |= bit;
        }
    }

    /// Read the row lines for the given scan column (motherboard 8255
    /// port B): the selected column ANDed with the special Shift/Stop
    /// column 15.
    pub fn read_rows(&self, scan_column: u8) -> u8 {
        let col = (scan_column & 0x0F) as usize;
        self.matrix[col] & self.matrix[15]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_matrix_is_high() {
        let kb = Keyboard::new();
        for col in 0..=15u8 {
            assert_eq!(kb.read_rows(col), 0xFF);
        }
    }

    #[test]
    fn pressing_a_key_is_visible_in_its_column() {
        let mut kb = Keyboard::new();
        kb.set_key(Key::A, true);
        assert_eq!(kb.read_rows(0), !0x08);
        assert_eq!(kb.read_rows(1), 0xFF);
        kb.set_key(Key::A, false);
        assert_eq!(kb.read_rows(0), 0xFF);
    }

    #[test]
    fn shift_affects_all_columns() {
        let mut kb = Keyboard::new();
        kb.set_key(Key::Q, true);
        kb.set_key(Key::Shift, true);
        // both bits low in column 0
        assert_eq!(kb.read_rows(0), !(0x04 | 0x20));
        // other columns only report the shift bit
        assert_eq!(kb.read_rows(7), !0x20);
    }

    #[test]
    fn scan_column_uses_low_nibble() {
        let mut kb = Keyboard::new();
        kb.set_key(Key::M, true);
        // Monitor writes e.g. 0xF0 + column; only the low nibble matters
        assert_eq!(kb.read_rows(0xF7), !0x10);
    }

    #[test]
    fn matrix_positions_are_unique() {
        use std::collections::HashSet;
        let keys = [
            Key::K0, Key::K11, Key::Digit0, Key::A, Key::Z, Key::M, Key::Space,
            Key::Enter, Key::Tab, Key::Backspace, Key::Insert, Key::Delete,
            Key::ClrScr, Key::Recall, Key::CursorLeft, Key::CursorRight,
            Key::CursorUp, Key::CursorDown, Key::LineStart, Key::LineEnd,
            Key::Wrk, Key::Cd, Key::Shift, Key::Stop, Key::Minus, Key::Equals,
            Key::OpenBracket, Key::CloseBracket, Key::Backslash, Key::Quote,
            Key::Semicolon, Key::Slash, Key::Period, Key::Comma,
        ];
        let mut seen = HashSet::new();
        for k in keys {
            // Backspace and CursorLeft are intentionally the same physical
            // key, so skip one of them for the uniqueness check.
            if k == Key::Backspace {
                continue;
            }
            assert!(seen.insert(matrix_pos(k)), "duplicate position for {k:?}");
        }
    }
}
