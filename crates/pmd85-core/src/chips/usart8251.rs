//! Minimal Intel 8251 USART emulation.
//!
//! The PMD 85 uses the 8251 for the cassette recorder interface and
//! V.24. This implementation models the programming interface (mode
//! word, command word, status register) enough for the Monitor ROM to
//! boot and to run the tape routines:
//!
//! - the DSR input (status bit 7) is driven externally by the tape
//!   deck (the Monitor 3 load routine bit-bangs the IRPS tape signal
//!   through it),
//! - transmitted bytes are collected (the deck's recorder sniffs
//!   them), and
//! - TxRDY/TxEMPTY can be held low by the deck to pace the ROM's save
//!   routine at the authentic 1200-baud tape speed.
//!
//! Received data is never produced from the RxD pin (RxRDY stays
//! low); Monitor 3 reads tape data by demodulating the DSR line.
//!
//! Status register:
//! bit 0 TxRDY, bit 1 RxRDY, bit 2 TxEMPTY, bit 3 PE, bit 4 OE, bit 5 FE,
//! bit 6 SYNDET/BRKDET, bit 7 DSR.

#[derive(Clone, Debug)]
pub struct I8251 {
    mode_word: Option<u8>,
    command: u8,
    /// Internal status (errors, TxRDY/TxEMPTY); DSR and the transmit
    /// gate are combined in `read`.
    status: u8,
    /// Sync-character load phase after a sync mode word.
    sync_chars_pending: u8,
    /// Received data register (always empty in this stub).
    pub rx_data: u8,
    /// Transmitted bytes (tape writes on a real machine).
    pub tx_log: Vec<u8>,
    /// Mirrors of output control lines.
    pub dtr: bool,
    pub rts: bool,
    /// External DSR line level (true = idle/mark).
    dsr: bool,
    /// Transmit-ready gate: `false` while the tape deck paces the
    /// transmitter (see `set_tx_ready`).
    tx_gate: bool,
}

impl Default for I8251 {
    fn default() -> Self {
        Self::new()
    }
}

impl I8251 {
    /// Serialize the interface state into a save state. The DSR and
    /// transmit-gate lines are driven by the cassette deck and are not
    /// part of the snapshot: a restored machine normalizes them through
    /// the (always idle) deck.
    pub(crate) fn save_state(&self, w: &mut crate::state::StateWriter) {
        match self.mode_word {
            None => w.bool(false),
            Some(v) => {
                w.bool(true);
                w.u8(v);
            }
        }
        w.u8(self.command);
        w.u8(self.status);
        w.u8(self.sync_chars_pending);
        w.u8(self.rx_data);
        w.bool(self.dtr);
        w.bool(self.rts);
        w.len(self.tx_log.len());
        w.bytes(&self.tx_log);
    }

    /// Restore the interface state written by [`I8251::save_state`].
    pub(crate) fn load_state(
        &mut self,
        r: &mut crate::state::StateReader,
    ) -> Result<(), crate::state::StateError> {
        self.mode_word = r.bool()?.then(|| r.u8()).transpose()?;
        self.command = r.u8()?;
        self.status = r.u8()?;
        self.sync_chars_pending = r.u8()?;
        self.rx_data = r.u8()?;
        self.dtr = r.bool()?;
        self.rts = r.bool()?;
        self.tx_log = r.vec()?;
        Ok(())
    }
    pub fn new() -> Self {
        I8251 {
            mode_word: None,
            command: 0,
            status: Self::base_status(),
            sync_chars_pending: 0,
            rx_data: 0,
            tx_log: Vec::new(),
            dtr: false,
            rts: false,
            dsr: true,
            tx_gate: true,
        }
    }

    pub fn reset(&mut self) {
        *self = I8251::new();
    }

    fn base_status() -> u8 {
        // TxRDY | TxEMPTY (ready for the next byte; drains instantly
        // unless the tape deck gates it).
        0x05
    }

    /// Drive the external DSR line (status bit 7). The tape deck
    /// toggles this to feed the IRPS signal to the monitor.
    pub fn set_dsr(&mut self, level: bool) {
        self.dsr = level;
    }

    /// Gate TxRDY/TxEMPTY low while the tape deck simulates the byte
    /// shifting out at tape speed; released with `true`.
    pub fn set_tx_ready(&mut self, ready: bool) {
        self.tx_gate = ready;
    }

    /// Write to the control (1) or data (0) register.
    pub fn write(&mut self, reg: u32, data: u8) {
        match reg & 1 {
            0 => {
                // transmit data; the buffer drains instantly, the
                // pacing (if any) is applied externally
                self.tx_log.push(data);
                self.status = Self::base_status();
            }
            _ => self.write_control(data),
        }
    }

    fn write_control(&mut self, data: u8) {
        if self.mode_word.is_none() {
            // First write after reset is the mode word.
            self.mode_word = Some(data);
            // Sync mode (baud factor bits = 00) requires sync characters.
            if data & 0x03 == 0 {
                self.sync_chars_pending = if data & 0x10 != 0 { 2 } else { 1 };
            }
            return;
        }
        if self.sync_chars_pending > 0 {
            self.sync_chars_pending -= 1;
            return;
        }
        // Otherwise this is a command word.
        self.command = data;
        // Internal reset (bit 6) returns to mode-word expectation.
        if data & 0x40 != 0 {
            self.mode_word = None;
            self.sync_chars_pending = 0;
        }
        self.dtr = data & 0x01 != 0;
        self.rts = data & 0x20 != 0;
        // Error reset (bit 4).
        if data & 0x10 != 0 {
            self.status &= !(0x38);
        }
    }

    /// Read the status (1) or received data (0) register.
    pub fn read(&mut self, reg: u32) -> u8 {
        match reg & 1 {
            0 => {
                // RxRDY stays low; reading empty data returns the last byte.
                self.rx_data
            }
            _ => {
                let mut status = self.status;
                if !self.tx_gate {
                    status &= !(0x05); // TxRDY | TxEMPTY
                }
                if self.dsr {
                    status |= 0x80;
                } else {
                    status &= !0x80;
                }
                status
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_then_command_sequence() {
        let mut uart = I8251::new();
        // async mode word: 8 data bits, x16, 1 stop
        uart.write(1, 0x4E);
        // command: enable tx/rts
        uart.write(1, 0x27);
        // status: TxRDY and TxEMPTY high
        assert_eq!(uart.read(1) & 0x07, 0x05);
        assert!(uart.rts);
    }

    #[test]
    fn internal_reset_returns_to_mode_word() {
        let mut uart = I8251::new();
        uart.write(1, 0x4E);
        uart.write(1, 0x40); // internal reset
        // next control write is a mode word again
        uart.write(1, 0x40); // sync mode word
        uart.write(1, 0x16); // sync char
        uart.write(1, 0x27); // command
        assert_eq!(uart.read(1) & 0x05, 0x05);
    }

    #[test]
    fn tx_data_collected() {
        let mut uart = I8251::new();
        uart.write(1, 0x4E);
        uart.write(1, 0x27);
        uart.write(0, 0x55);
        uart.write(0, 0xAA);
        assert_eq!(uart.tx_log, vec![0x55, 0xAA]);
        // TxRDY remains set since the buffer drains instantly
        assert_eq!(uart.read(1) & 0x01, 0x01);
    }

    #[test]
    fn rx_never_ready() {
        let mut uart = I8251::new();
        assert_eq!(uart.read(1) & 0x02, 0, "RxRDY must stay low without tape");
    }

    #[test]
    fn dsr_line_reflects_in_status() {
        let mut uart = I8251::new();
        assert_eq!(uart.read(1) & 0x80, 0x80, "DSR idles high");
        uart.set_dsr(false);
        assert_eq!(uart.read(1) & 0x80, 0);
        uart.set_dsr(true);
        assert_eq!(uart.read(1) & 0x80, 0x80);
        // A data write must not disturb the external line state.
        uart.write(0, 0x42);
        assert_eq!(uart.read(1) & 0x80, 0x80);
    }

    #[test]
    fn tx_gate_holds_back_txrdy_and_txempty() {
        let mut uart = I8251::new();
        assert_eq!(uart.read(1) & 0x05, 0x05);
        uart.set_tx_ready(false);
        assert_eq!(uart.read(1) & 0x05, 0, "transmitter held busy");
        // Other bits are unaffected.
        assert_eq!(uart.read(1) & 0x80, 0x80);
        uart.set_tx_ready(true);
        assert_eq!(uart.read(1) & 0x05, 0x05);
    }

    #[test]
    fn reset_restores_external_lines() {
        let mut uart = I8251::new();
        uart.set_dsr(false);
        uart.set_tx_ready(false);
        uart.reset();
        assert_eq!(uart.read(1), 0x85);
    }
}
