//! Intel 8255 Programmable Peripheral Interface.
//!
//! Implements mode 0 (basic input/output with latched outputs) and the
//! Bit Set/Reset (BSR) mode for port C. Modes 1 and 2 (strobed/bidirectional)
//! are not used by the PMD 85 Monitor and are rejected like on real
//! hardware by simply leaving the previous configuration in place.

/// Port index used by `read`/`write`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Port {
    A,
    B,
    C,
    Control,
}

impl Port {
    pub fn from_index(i: u32) -> Port {
        match i & 3 {
            0 => Port::A,
            1 => Port::B,
            2 => Port::C,
            _ => Port::Control,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct I8255 {
    /// Mode control word (bit 7 set) last written.
    control: u8,
    /// Output latches for ports A, B, C.
    pub outputs: [u8; 3],
    /// Direction bits: true = input.
    pa_in: bool,
    pb_in: bool,
    pc_lower_in: bool,
    pc_upper_in: bool,
}

impl I8255 {
    pub fn new() -> Self {
        // After reset the 8255 is in mode 0 with all ports input.
        I8255 {
            control: 0x9B,
            outputs: [0; 3],
            pa_in: true,
            pb_in: true,
            pc_lower_in: true,
            pc_upper_in: true,
        }
    }

    pub fn reset(&mut self) {
        *self = I8255::new();
    }

    /// The last written mode control word.
    pub fn control(&self) -> u8 {
        self.control
    }

    /// Serialize the interface state into a save state.
    pub(crate) fn save_state(&self, w: &mut crate::state::StateWriter) {
        w.u8(self.control);
        w.bytes(&self.outputs);
        w.bool(self.pa_in);
        w.bool(self.pb_in);
        w.bool(self.pc_lower_in);
        w.bool(self.pc_upper_in);
    }

    /// Restore the interface state written by [`I8255::save_state`].
    pub(crate) fn load_state(
        &mut self,
        r: &mut crate::state::StateReader,
    ) -> Result<(), crate::state::StateError> {
        self.control = r.u8()?;
        r.read_into(&mut self.outputs)?;
        self.pa_in = r.bool()?;
        self.pb_in = r.bool()?;
        self.pc_lower_in = r.bool()?;
        self.pc_upper_in = r.bool()?;
        Ok(())
    }

    pub fn is_mode_set(&self, word: u8) -> bool {
        word & 0x80 != 0
    }

    /// Write to a port register or the control register.
    pub fn write(&mut self, port: Port, data: u8) {
        match port {
            Port::A => self.outputs[0] = data,
            Port::B => self.outputs[1] = data,
            Port::C => self.outputs[2] = data,
            Port::Control => {
                if data & 0x80 != 0 {
                    // Mode set
                    self.control = data;
                    self.pa_in = data & 0x10 != 0;
                    self.pb_in = data & 0x02 != 0;
                    self.pc_lower_in = data & 0x01 != 0;
                    self.pc_upper_in = data & 0x08 != 0;
                } else {
                    // Bit set/reset on port C: 0bbb s
                    let bit = (data >> 1) & 0x07;
                    let set = data & 0x01 != 0;
                    if set {
                        self.outputs[2] |= 1 << bit;
                    } else {
                        self.outputs[2] &= !(1 << bit);
                    }
                }
            }
        }
    }

    /// Read a port. `pa_in`/`pb_in`/`pc_in` are the external input levels,
    /// used for the pins configured as inputs.
    pub fn read(&self, port: Port, pa_in: u8, pb_in: u8, pc_in: u8) -> u8 {
        match port {
            Port::A => {
                if self.pa_in {
                    pa_in
                } else {
                    self.outputs[0]
                }
            }
            Port::B => {
                if self.pb_in {
                    pb_in
                } else {
                    self.outputs[1]
                }
            }
            Port::C => {
                let lower = if self.pc_lower_in { pc_in & 0x0F } else { self.outputs[2] & 0x0F };
                let upper = if self.pc_upper_in { pc_in & 0xF0 } else { self.outputs[2] & 0xF0 };
                lower | upper
            }
            Port::Control => 0xFF,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode0_output_latch() {
        let mut ppi = I8255::new();
        // all outputs, mode 0: 0x80
        ppi.write(Port::Control, 0x80);
        ppi.write(Port::A, 0x5A);
        ppi.write(Port::B, 0x33);
        assert_eq!(ppi.read(Port::A, 0x00, 0x00, 0x00), 0x5A);
        assert_eq!(ppi.read(Port::B, 0x00, 0x00, 0x00), 0x33);
        // inputs on the pins are ignored for output ports
        assert_eq!(ppi.read(Port::A, 0xFF, 0xFF, 0xFF), 0x5A);
    }

    #[test]
    fn mode0_input() {
        let mut ppi = I8255::new();
        // PA and PC in, PB out: 0x99
        ppi.write(Port::Control, 0x99);
        assert_eq!(ppi.read(Port::A, 0x42, 0, 0), 0x42);
        // PC upper in, PA/PB/PC-lower out: word 0b1000_1000 = 0x88
        ppi.write(Port::Control, 0x88);
        ppi.write(Port::C, 0x0E);
        // PC lower output latch (0xE), upper from pins (0x50)
        assert_eq!(ppi.read(Port::C, 0, 0, 0x50), 0x5E);
    }

    #[test]
    fn bsr_set_and_reset() {
        let mut ppi = I8255::new();
        // set PC3: bit=3, set
        ppi.write(Port::Control, 0b0000_0111);
        assert_eq!(ppi.outputs[2], 0x08);
        // reset PC3
        ppi.write(Port::Control, 0b0000_0110);
        assert_eq!(ppi.outputs[2], 0x00);
        // set PC7
        ppi.write(Port::Control, 0b0000_1111);
        assert_eq!(ppi.outputs[2], 0x80);
        // BSR does not change direction configuration
        ppi.write(Port::Control, 0x80); // all outputs
        ppi.write(Port::Control, 0x0F);
        assert_eq!(ppi.read(Port::C, 0x55, 0, 0x55), 0x80);
    }

    #[test]
    fn control_reads_open_bus() {
        let ppi = I8255::new();
        assert_eq!(ppi.read(Port::Control, 0, 0, 0), 0xFF);
    }

    #[test]
    fn reset_state_all_inputs() {
        let mut ppi = I8255::new();
        ppi.write(Port::Control, 0x80);
        ppi.write(Port::A, 0x11);
        ppi.reset();
        assert_eq!(ppi.read(Port::A, 0x77, 0, 0), 0x77);
    }
}
