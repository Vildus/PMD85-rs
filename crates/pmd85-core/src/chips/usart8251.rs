//! Minimal Intel 8251 USART emulation.
//!
//! The PMD 85 uses the 8251 for the cassette recorder interface and V.24.
//! This implementation models the programming interface (mode word,
//! command word, status register) enough for the Monitor ROM to boot and
//! to sit idle waiting for tape data, but it never produces received data
//! (RxRDY stays low) and transmitted bytes are collected but ignored.
//!
//! Status register:
//! bit 0 TxRDY, bit 1 RxRDY, bit 2 TxEMPTY, bit 3 PE, bit 4 OE, bit 5 FE,
//! bit 6 SYNDET/BRKDET, bit 7 DSR.

#[derive(Clone, Debug)]
pub struct I8251 {
    mode_word: Option<u8>,
    command: u8,
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
}

impl Default for I8251 {
    fn default() -> Self {
        Self::new()
    }
}

impl I8251 {
    pub fn new() -> Self {
        I8251 {
            mode_word: None,
            command: 0,
            status: Self::idle_status(),
            sync_chars_pending: 0,
            rx_data: 0,
            tx_log: Vec::new(),
            dtr: false,
            rts: false,
        }
    }

    pub fn reset(&mut self) {
        *self = I8251::new();
    }

    fn idle_status() -> u8 {
        // TxRDY | TxEMPTY | DSR
        0x05 | 0x80
    }

    /// Write to the control (1) or data (0) register.
    pub fn write(&mut self, reg: u32, data: u8) {
        match reg & 1 {
            0 => {
                // transmit data; buffer drains instantly
                self.tx_log.push(data);
                self.status = Self::idle_status();
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
            _ => self.status,
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
}
