//! Intel 8253 Programmable Interval Timer.
//!
//! Three independent 16-bit counters, modes 0-5, binary or BCD counting.
//! On the PMD 85 the PIT sits on the I/O board: counter 0 is wired to the
//! external K2 connector, counter 1 is clocked by the ~2 MHz system clock
//! and counter 2 by a 1 Hz generator.
//!
//! The counters are stepped by explicit clock ticks so the machine can
//! advance timer 1 once per emulated CPU cycle.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessMode {
    Latch,
    Lsb,
    Msb,
    LsbMsb,
}

impl AccessMode {
    fn from_bits(bits: u8) -> AccessMode {
        match bits {
            0 => AccessMode::Latch,
            1 => AccessMode::Lsb,
            2 => AccessMode::Msb,
            _ => AccessMode::LsbMsb,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Counter {
    pub mode: u8,
    pub bcd: bool,
    access: AccessMode,
    /// Current counting element.
    pub count: u16,
    /// Reload value written by software.
    pub initial: u16,
    /// Latched count for reads.
    latch: Option<u16>,
    /// True when a new count has been written but not yet loaded.
    null_count: bool,
    /// Read/write byte flip for two-byte accesses.
    msb_next: bool,
    pub out: bool,
    gate: bool,
    /// Helper for square-wave (mode 3) phase.
    phase_high: bool,
}

impl Counter {
    fn new() -> Self {
        Counter {
            mode: 0,
            bcd: false,
            access: AccessMode::LsbMsb,
            count: 0,
            initial: 0,
            latch: None,
            null_count: true,
            msb_next: false,
            out: false,
            gate: true,
            phase_high: true,
        }
    }

    fn reset(&mut self) {
        *self = Counter::new();
    }

    /// Write the control word relevant to this counter.
    fn set_control(&mut self, rw_bits: u8, mode: u8, bcd: bool) {
        self.access = AccessMode::from_bits(rw_bits);
        self.bcd = bcd;
        self.mode = mode;
        self.msb_next = false;
    }

    fn write(&mut self, data: u8) {
        match self.access {
            AccessMode::Lsb => {
                self.initial = (self.initial & 0xFF00) | data as u16;
                self.null_count = true;
            }
            AccessMode::Msb => {
                self.initial = (self.initial & 0x00FF) | ((data as u16) << 8);
                self.null_count = true;
            }
            AccessMode::LsbMsb => {
                if !self.msb_next {
                    self.initial = (self.initial & 0xFF00) | data as u16;
                    self.msb_next = true;
                } else {
                    self.initial = (self.initial & 0x00FF) | ((data as u16) << 8);
                    self.msb_next = false;
                    self.null_count = true;
                }
            }
            AccessMode::Latch => {
                self.latch = Some(self.count);
            }
        }
    }

    fn read(&mut self) -> u8 {
        let value = self.latch.unwrap_or(self.count);
        match self.access {
            AccessMode::Lsb => (value & 0xFF) as u8,
            AccessMode::Msb => (value >> 8) as u8,
            AccessMode::LsbMsb => {
                if !self.msb_next {
                    self.msb_next = true;
                    (value & 0xFF) as u8
                } else {
                    self.msb_next = false;
                    (value >> 8) as u8
                }
            }
            AccessMode::Latch => {
                let v = self.latch.take().unwrap_or(value);
                // after latched read, subsequent reads behave as before
                (v & 0xFF) as u8
            }
        }
    }

    /// Decrement in binary or BCD.
    fn decrement(&mut self) {
        if self.bcd {
            let mut lo = self.count & 0x0F;
            let mut hi = (self.count >> 4) & 0x0F;
            let mut hi2 = (self.count >> 8) & 0x0F;
            let mut hi3 = (self.count >> 12) & 0x0F;
            if lo == 0 {
                lo = 9;
                if hi == 0 {
                    hi = 9;
                    if hi2 == 0 {
                        hi2 = 9;
                        hi3 = hi3.wrapping_sub(1) & 0x0F;
                    } else {
                        hi2 -= 1;
                    }
                } else {
                    hi -= 1;
                }
            } else {
                lo -= 1;
            }
            self.count = ((hi3 as u16) << 12)
                | ((hi2 as u16) << 8)
                | ((hi as u16) << 4)
                | lo as u16;
        } else {
            self.count = self.count.wrapping_sub(1);
        }
    }

    fn is_zero(&self) -> bool {
        self.count == 0
    }

    fn reload(&mut self) {
        self.count = if self.initial == 0 && self.mode == 3 {
            0
        } else if self.initial == 0 {
            if self.bcd {
                0x9999
            } else {
                0xFFFF
            }
        } else {
            self.initial
        };
        self.null_count = false;
    }

    /// Apply the next clock edge. `gate` is the current gate input level.
    pub fn tick(&mut self, gate: bool) {
        let gate_rising = gate && !self.gate;
        self.gate = gate;

        // Load pending initial counts on the next clock (all modes).
        if self.null_count {
            self.reload();
            // modes start their output accordingly
            match self.mode {
                0 | 1 => self.out = false,
                2 | 3 => {
                    self.phase_high = true;
                    self.out = true;
                }
                4 | 5 => self.out = true,
                _ => {}
            }
            return;
        }

        // Counting is inhibited while gate is low (mode 1 keeps counting
        // once triggered). For modes 2 and 3 the output is also forced high.
        if !gate && self.mode != 1 {
            if matches!(self.mode, 2 | 3) {
                self.out = true;
            }
            return;
        }

        match self.mode {
            0 => {
                // Interrupt on terminal count: out goes high when count
                // reaches zero and stays high.
                if self.is_zero() {
                    self.out = true;
                } else {
                    self.decrement();
                    if self.is_zero() {
                        self.out = true;
                    }
                }
            }
            1 => {
                // Hardware retriggerable one-shot.
                if gate_rising {
                    self.reload();
                    self.out = false;
                } else if !self.is_zero() {
                    self.decrement();
                }
            }
            2 => {
                // Rate generator: low for one clock period.
                if self.is_zero() {
                    self.out = false;
                    self.reload();
                    self.out = false;
                    // one clock low, then high again next tick
                } else {
                    self.out = true;
                    self.decrement();
                    if self.is_zero() {
                        self.out = false;
                    }
                }
            }
            3 => {
                // Square wave: decrement by 2 each clock.
                if self.is_zero() {
                    self.reload();
                    self.phase_high = !self.phase_high;
                }
                if self.count >= 2 {
                    self.decrement();
                    self.decrement();
                } else {
                    self.count = 0;
                }
                if self.is_zero() {
                    self.phase_high = !self.phase_high;
                }
                self.out = self.phase_high;
            }
            4 => {
                // Software triggered strobe.
                if self.is_zero() {
                    self.out = false;
                    self.reload();
                    self.out = true;
                } else {
                    self.decrement();
                    if self.is_zero() {
                        self.out = false;
                    }
                }
            }
            5 => {
                // Hardware triggered strobe.
                if gate_rising {
                    self.reload();
                    self.out = true;
                } else if !self.is_zero() {
                    self.decrement();
                    if self.is_zero() {
                        self.out = false;
                    }
                }
            }
            _ => {}
        }
    }
}

#[derive(Clone, Debug)]
pub struct I8253 {
    pub counters: [Counter; 3],
}

impl I8253 {
    pub fn new() -> Self {
        I8253 {
            counters: [Counter::new(), Counter::new(), Counter::new()],
        }
    }

    pub fn reset(&mut self) {
        for c in &mut self.counters {
            c.reset();
        }
    }

    pub fn write(&mut self, reg: u32, data: u8) {
        match reg & 3 {
            0..=2 => self.counters[(reg & 3) as usize].write(data),
            _ => {
                // Control word: sc rw m bcd
                let sc = (data >> 6) & 0x03;
                let rw = (data >> 4) & 0x03;
                if sc == 3 {
                    // Read-back command: not supported, ignore.
                    return;
                }
                // Counter latch command: latch the current count without
                // touching the programmed access mode.
                if rw == 0 {
                    self.counters[sc as usize].latch = Some(self.counters[sc as usize].count);
                    return;
                }
                let mode = (data >> 1) & 0x07;
                let bcd = data & 0x01 != 0;
                let mode = if mode >= 6 { mode - 4 } else { mode };
                self.counters[sc as usize].set_control(rw, mode, bcd);
            }
        }
    }

    pub fn read(&mut self, reg: u32) -> u8 {
        match reg & 3 {
            0..=2 => {
                let idx = (reg & 3) as usize;
                self.counters[idx].read()
            }
            _ => 0xFF,
        }
    }

    /// Advance the given clock input by one tick.
    pub fn tick(&mut self, counter: usize, gate: bool) {
        self.counters[counter].tick(gate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode0_terminal_count() {
        let mut pit = I8253::new();
        // counter 0, mode 0, binary, lsb/msb
        pit.write(3, 0b0011_0000);
        pit.write(0, 3); // lsb
        pit.write(0, 0); // msb -> count 3
        assert!(!pit.counters[0].out);
        // 1st clock loads CE, 3 more bring it to zero
        for _ in 0..4 {
            pit.tick(0, true);
        }
        assert!(pit.counters[0].out, "out high after terminal count");
    }

    #[test]
    fn mode0_count_inhibited_by_gate() {
        let mut pit = I8253::new();
        pit.write(3, 0b0011_0000);
        pit.write(0, 5);
        pit.write(0, 0);
        for _ in 0..10 {
            pit.tick(0, false);
        }
        assert!(!pit.counters[0].out, "mode 0 counts while gate low");
        // still has the initial count pending
        assert_eq!(pit.counters[0].count, 5);
    }

    #[test]
    fn reading_count_latch() {
        let mut pit = I8253::new();
        pit.write(3, 0b0011_0000);
        pit.write(0, 0x34);
        pit.write(0, 0x12); // initial 0x1234
        pit.tick(0, true); // loads
        pit.tick(0, true); // 0x1233
        // latch counter 0
        pit.write(3, 0b0000_0000);
        let lsb = pit.read(0);
        let msb = pit.read(0);
        assert_eq!(((msb as u16) << 8) | lsb as u16, 0x1233);
    }

    #[test]
    fn lsb_only_mode() {
        let mut pit = I8253::new();
        // counter 1, lsb only, mode 2
        pit.write(3, 0b0101_0100);
        pit.write(1, 4);
        assert_eq!(pit.counters[1].initial, 4);
    }

    #[test]
    fn mode3_square_wave_toggles() {
        let mut pit = I8253::new();
        // counter 1, mode 3, lsb/msb
        pit.write(3, 0b0111_0110);
        pit.write(1, 4);
        pit.write(1, 0);
        let mut saw_high = false;
        let mut saw_low = false;
        for _ in 0..16 {
            pit.tick(1, true);
            if pit.counters[1].out {
                saw_high = true;
            } else {
                saw_low = true;
            }
        }
        assert!(saw_high && saw_low, "square wave must toggle");
    }
}
