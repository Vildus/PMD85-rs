//! Intel 8080 CPU core (as used in the PMD 85: Tesla MHB8080A @ 2.048 MHz).
//!
//! Implements the complete documented instruction set plus the common
//! undocumented opcodes (`08/10/18/20/28/30/38` = NOP, `CB` = JMP,
//! `D9` = RET, `DD/ED/FD` = CALL), correct flag semantics (including the
//! half-carry quirks of `ANA`, `INR`/`DCR` and `CMP`), cycle-accurate
//! T-state counts and interrupt handling (INTR with an opcode placed on the
//! bus, INTE gating with the one-instruction delay after `EI`).

/// Memory + port access needed by the CPU. The machine implements this.
pub trait Bus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, data: u8);
    fn port_in(&mut self, port: u8) -> u8;
    fn port_out(&mut self, port: u8, data: u8);
}

/// Condition codes of the 8080.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    pub cy: bool,
    pub p: bool,
    pub ac: bool,
    pub z: bool,
    pub s: bool,
}

impl Flags {
    /// Pack flags into the PSW byte layout: S Z 0 AC 0 P 1 CY.
    pub fn pack(self) -> u8 {
        (self.s as u8) << 7
            | (self.z as u8) << 6
            | (self.ac as u8) << 4
            | (self.p as u8) << 2
            | 0x02
            | (self.cy as u8)
    }

    /// Unpack flags from the PSW byte layout (bits 3 and 5 are ignored).
    pub fn unpack(psw: u8) -> Self {
        Flags {
            s: psw & 0x80 != 0,
            z: psw & 0x40 != 0,
            ac: psw & 0x10 != 0,
            p: psw & 0x04 != 0,
            cy: psw & 0x01 != 0,
        }
    }
}

/// Full CPU state.
#[derive(Clone, Debug)]
pub struct Cpu {
    pub a: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
    pub flags: Flags,
    /// Interrupt flip-flop (INTE).
    pub iff: bool,
    /// Set when EI was just executed; a following instruction runs first.
    ei_delay: bool,
    pub halted: bool,
    /// Pending interrupt: opcode to be jammed onto the bus on acceptance.
    pending_interrupt: Option<u8>,
    /// Cumulative cycle counter.
    pub cycles: u64,
}

impl Default for Cpu {
    fn default() -> Self {
        Cpu::new()
    }
}

impl Cpu {
    pub fn new() -> Self {
        Cpu {
            a: 0,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
            sp: 0,
            pc: 0,
            flags: Flags::default(),
            iff: false,
            ei_delay: false,
            halted: false,
            pending_interrupt: None,
            cycles: 0,
        }
    }

    pub fn reset(&mut self) {
        *self = Cpu::new();
    }

    /// Raise an interrupt; `opcode` (usually `RST n`) will be executed if
    /// interrupts are enabled.
    pub fn interrupt(&mut self, opcode: u8) {
        self.pending_interrupt = Some(opcode);
    }

    /// Serialize the full CPU state (registers, flags, interrupt and
    /// halt logic, cycle counter) into a save state.
    pub(crate) fn save_state(&self, w: &mut crate::state::StateWriter) {
        for r in [self.a, self.b, self.c, self.d, self.e, self.h, self.l] {
            w.u8(r);
        }
        w.u16(self.sp);
        w.u16(self.pc);
        w.u8(self.flags.pack());
        w.bool(self.iff);
        w.bool(self.ei_delay);
        w.bool(self.halted);
        match self.pending_interrupt {
            None => w.u8(0),
            Some(op) => w.u8(1 | (op << 1)),
        }
        w.u64(self.cycles);
    }

    /// Restore the CPU state written by [`Cpu::save_state`].
    pub(crate) fn load_state(&mut self, r: &mut crate::state::StateReader) -> Result<(), crate::state::StateError> {
        self.a = r.u8()?;
        self.b = r.u8()?;
        self.c = r.u8()?;
        self.d = r.u8()?;
        self.e = r.u8()?;
        self.h = r.u8()?;
        self.l = r.u8()?;
        self.sp = r.u16()?;
        self.pc = r.u16()?;
        self.flags = Flags::unpack(r.u8()?);
        self.iff = r.bool()?;
        self.ei_delay = r.bool()?;
        self.halted = r.bool()?;
        let pending = r.u8()?;
        self.pending_interrupt = (pending != 0).then_some(pending >> 1);
        self.cycles = r.u64()?;
        Ok(())
    }

    /// Run one instruction (or service a pending interrupt). Returns the
    /// number of T-states consumed.
    pub fn step<B: Bus>(&mut self, bus: &mut B) -> u32 {
        let start = self.cycles;
        if let Some(opcode) = self.pending_interrupt {
            if self.iff && !self.ei_delay {
                self.pending_interrupt = None;
                self.iff = false;
                self.halted = false;
                // The interrupting device jams an instruction (typically
                // RST n) onto the bus; it executes against real memory so
                // pushes/fetches behave normally.
                self.execute(opcode, bus);
                return (self.cycles - start) as u32;
            }
        }
        if self.halted {
            self.cycles += 4;
            return 4;
        }
        let opcode = self.fetch_byte(bus);
        let ei_was_delayed = self.ei_delay;
        self.execute(opcode, bus);
        // The EI delay only blocks interrupts for the instruction right
        // after EI itself, which just ran.
        if ei_was_delayed {
            self.ei_delay = false;
        }
        (self.cycles - start) as u32
    }

    // ---- helpers ----------------------------------------------------------

    fn fetch_byte<B: Bus>(&mut self, bus: &mut B) -> u8 {
        let v = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    fn fetch_word<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let lo = self.fetch_byte(bus) as u16;
        let hi = self.fetch_byte(bus) as u16;
        (hi << 8) | lo
    }

    fn read_word<B: Bus>(&mut self, bus: &mut B, addr: u16) -> u16 {
        let lo = bus.read(addr) as u16;
        let hi = bus.read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    fn write_word<B: Bus>(&mut self, bus: &mut B, addr: u16, v: u16) {
        bus.write(addr, v as u8);
        bus.write(addr.wrapping_add(1), (v >> 8) as u8);
    }

    pub fn bc(&self) -> u16 {
        ((self.b as u16) << 8) | self.c as u16
    }
    pub fn de(&self) -> u16 {
        ((self.d as u16) << 8) | self.e as u16
    }
    pub fn hl(&self) -> u16 {
        ((self.h as u16) << 8) | self.l as u16
    }
    fn set_bc(&mut self, v: u16) {
        self.b = (v >> 8) as u8;
        self.c = v as u8;
    }
    fn set_de(&mut self, v: u16) {
        self.d = (v >> 8) as u8;
        self.e = v as u8;
    }
    fn set_hl(&mut self, v: u16) {
        self.h = (v >> 8) as u8;
        self.l = v as u8;
    }

    fn push<B: Bus>(&mut self, bus: &mut B, v: u16) {
        self.sp = self.sp.wrapping_sub(2);
        self.write_word(bus, self.sp, v);
    }

    fn pop<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let v = self.read_word(bus, self.sp);
        self.sp = self.sp.wrapping_add(2);
        v
    }

    fn parity(v: u8) -> bool {
        v.count_ones().is_multiple_of(2)
    }

    fn set_zsp(&mut self, v: u8) {
        self.flags.z = v == 0;
        self.flags.s = v & 0x80 != 0;
        self.flags.p = Self::parity(v);
    }

    /// Carry out of bit `bit_no - 1` when computing `a + b + cy`.
    fn carry(bit_no: u32, a: u8, b: u8, cy: bool) -> bool {
        let result = a as u16 + b as u16 + cy as u16;
        let carry = result ^ a as u16 ^ b as u16;
        carry & (1 << bit_no) != 0
    }

    fn add(&mut self, val: u8, cy: bool) {
        let result = self.a.wrapping_add(val).wrapping_add(cy as u8);
        self.flags.cy = Self::carry(8, self.a, val, cy);
        self.flags.ac = Self::carry(4, self.a, val, cy);
        self.set_zsp(result);
        self.a = result;
    }

    fn sub(&mut self, val: u8, cy: bool) {
        self.add(!val, !cy);
        self.flags.cy = !self.flags.cy;
    }

    fn inr(&mut self, val: u8) -> u8 {
        let result = val.wrapping_add(1);
        self.flags.ac = result & 0x0F == 0;
        self.set_zsp(result);
        result
    }

    fn dcr(&mut self, val: u8) -> u8 {
        let result = val.wrapping_sub(1);
        self.flags.ac = result & 0x0F != 0x0F;
        self.set_zsp(result);
        result
    }

    fn ana(&mut self, val: u8) {
        let result = self.a & val;
        self.flags.cy = false;
        // 8080 quirk: AC is set if either operand has bit 3 set.
        self.flags.ac = (self.a | val) & 0x08 != 0;
        self.set_zsp(result);
        self.a = result;
    }

    fn xra(&mut self, val: u8) {
        self.a ^= val;
        self.flags.cy = false;
        self.flags.ac = false;
        self.set_zsp(self.a);
    }

    fn ora(&mut self, val: u8) {
        self.a |= val;
        self.flags.cy = false;
        self.flags.ac = false;
        self.set_zsp(self.a);
    }

    fn cmp(&mut self, val: u8) {
        let result = self.a.wrapping_sub(val);
        self.flags.cy = self.a < val;
        self.flags.ac = (self.a ^ result ^ !val) & 0x10 != 0;
        self.set_zsp(result);
    }

    fn dad(&mut self, val: u16) {
        let hl = self.hl();
        self.flags.cy = hl as u32 + val as u32 > 0xFFFF;
        self.set_hl(hl.wrapping_add(val));
    }

    fn push_psw<B: Bus>(&mut self, bus: &mut B) {
        let psw = (self.a as u16) << 8 | self.flags.pack() as u16;
        self.push(bus, psw);
    }

    fn pop_psw<B: Bus>(&mut self, bus: &mut B) {
        let af = self.pop(bus);
        self.a = (af >> 8) as u8;
        self.flags = Flags::unpack(af as u8);
    }

    // ---- the instruction set ----------------------------------------------

    /// Execute a single opcode. Cycle counts follow the Intel 8080 manual.
    pub fn execute<B: Bus>(&mut self, opcode: u8, bus: &mut B) {
        match opcode {
            // NOP (and undocumented NOPs)
            0x00 | 0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 => {
                self.cycles += 4;
            }

            // ---- moves -----------------------------------------------------
            0x40..=0x75 | 0x77..=0x7F => {
                // MOV r,r' / MOV r,M / MOV M,r
                let (dst, src) = Reg::decode_move(opcode);
                let v = self.read_reg(bus, src);
                self.write_reg(bus, dst, v);
                if dst == Reg::M || src == Reg::M {
                    self.cycles += 7;
                } else {
                    self.cycles += 5;
                }
            }
            0x76 => {
                // HLT
                self.halted = true;
                self.cycles += 7;
            }
            0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x36 | 0x3E => {
                // MVI r,d8
                let dst = Reg::from_opcode(opcode >> 3);
                let v = self.fetch_byte(bus);
                self.write_reg(bus, dst, v);
                self.cycles += if dst == Reg::M { 10 } else { 7 };
            }
            0x01 | 0x11 | 0x21 | 0x31 => {
                // LXI rp,d16
                let v = self.fetch_word(bus);
                match opcode {
                    0x01 => self.set_bc(v),
                    0x11 => self.set_de(v),
                    0x21 => self.set_hl(v),
                    _ => self.sp = v,
                }
                self.cycles += 10;
            }
            0x0A => {
                // LDAX B
                self.a = bus.read(self.bc());
                self.cycles += 7;
            }
            0x1A => {
                // LDAX D
                self.a = bus.read(self.de());
                self.cycles += 7;
            }
            0x2A => {
                // LHLD a16
                let addr = self.fetch_word(bus);
                self.l = bus.read(addr);
                self.h = bus.read(addr.wrapping_add(1));
                self.cycles += 16;
            }
            0x3A => {
                // LDA a16
                let addr = self.fetch_word(bus);
                self.a = bus.read(addr);
                self.cycles += 13;
            }
            0x02 => {
                // STAX B
                bus.write(self.bc(), self.a);
                self.cycles += 7;
            }
            0x12 => {
                // STAX D
                bus.write(self.de(), self.a);
                self.cycles += 7;
            }
            0x22 => {
                // SHLD a16
                let addr = self.fetch_word(bus);
                bus.write(addr, self.l);
                bus.write(addr.wrapping_add(1), self.h);
                self.cycles += 16;
            }
            0x32 => {
                // STA a16
                let addr = self.fetch_word(bus);
                bus.write(addr, self.a);
                self.cycles += 13;
            }
            0xEB => {
                // XCHG
                std::mem::swap(&mut self.h, &mut self.d);
                std::mem::swap(&mut self.l, &mut self.e);
                self.cycles += 4;
            }
            0xE3 => {
                // XTHL
                let sp_val = self.read_word(bus, self.sp);
                let hl = self.hl();
                self.write_word(bus, self.sp, hl);
                self.set_hl(sp_val);
                self.cycles += 18;
            }
            0xF9 => {
                // SPHL
                self.sp = self.hl();
                self.cycles += 5;
            }
            0xE9 => {
                // PCHL
                self.pc = self.hl();
                self.cycles += 5;
            }

            // ---- arithmetic / logic ----------------------------------------
            0x80..=0x87 => {
                // ADD r
                let v = self.read_reg(bus, Reg::from_opcode(opcode));
                self.add(v, false);
                self.cycles += if opcode == 0x86 { 7 } else { 4 };
            }
            0x88..=0x8F => {
                // ADC r
                let v = self.read_reg(bus, Reg::from_opcode(opcode - 0x88));
                let cy = self.flags.cy;
                self.add(v, cy);
                self.cycles += if opcode == 0x8E { 7 } else { 4 };
            }
            0x90..=0x97 => {
                // SUB r
                let v = self.read_reg(bus, Reg::from_opcode(opcode - 0x90));
                self.sub(v, false);
                self.cycles += if opcode == 0x96 { 7 } else { 4 };
            }
            0x98..=0x9F => {
                // SBB r
                let v = self.read_reg(bus, Reg::from_opcode(opcode - 0x98));
                let cy = self.flags.cy;
                self.sub(v, cy);
                self.cycles += if opcode == 0x9E { 7 } else { 4 };
            }
            0xA0..=0xA7 => {
                // ANA r
                let v = self.read_reg(bus, Reg::from_opcode(opcode - 0xA0));
                self.ana(v);
                self.cycles += if opcode == 0xA6 { 7 } else { 4 };
            }
            0xA8..=0xAF => {
                // XRA r
                let v = self.read_reg(bus, Reg::from_opcode(opcode - 0xA8));
                self.xra(v);
                self.cycles += if opcode == 0xAE { 7 } else { 4 };
            }
            0xB0..=0xB7 => {
                // ORA r
                let v = self.read_reg(bus, Reg::from_opcode(opcode - 0xB0));
                self.ora(v);
                self.cycles += if opcode == 0xB6 { 7 } else { 4 };
            }
            0xB8..=0xBF => {
                // CMP r
                let v = self.read_reg(bus, Reg::from_opcode(opcode - 0xB8));
                self.cmp(v);
                self.cycles += if opcode == 0xBE { 7 } else { 4 };
            }
            0xC6 => {
                // ADI d8
                let v = self.fetch_byte(bus);
                self.add(v, false);
                self.cycles += 7;
            }
            0xCE => {
                // ACI d8
                let v = self.fetch_byte(bus);
                let cy = self.flags.cy;
                self.add(v, cy);
                self.cycles += 7;
            }
            0xD6 => {
                // SUI d8
                let v = self.fetch_byte(bus);
                self.sub(v, false);
                self.cycles += 7;
            }
            0xDE => {
                // SBI d8
                let v = self.fetch_byte(bus);
                let cy = self.flags.cy;
                self.sub(v, cy);
                self.cycles += 7;
            }
            0xE6 => {
                // ANI d8
                let v = self.fetch_byte(bus);
                self.ana(v);
                self.cycles += 7;
            }
            0xEE => {
                // XRI d8
                let v = self.fetch_byte(bus);
                self.xra(v);
                self.cycles += 7;
            }
            0xF6 => {
                // ORI d8
                let v = self.fetch_byte(bus);
                self.ora(v);
                self.cycles += 7;
            }
            0xFE => {
                // CPI d8
                let v = self.fetch_byte(bus);
                self.cmp(v);
                self.cycles += 7;
            }
            0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C => {
                // INR r
                let r = Reg::from_opcode(opcode >> 3);
                let v = self.read_reg(bus, r);
                let v = self.inr(v);
                self.write_reg(bus, r, v);
                self.cycles += if r == Reg::M { 10 } else { 5 };
            }
            0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D => {
                // DCR r
                let r = Reg::from_opcode(opcode >> 3);
                let v = self.read_reg(bus, r);
                let v = self.dcr(v);
                self.write_reg(bus, r, v);
                self.cycles += if r == Reg::M { 10 } else { 5 };
            }
            0x03 | 0x13 | 0x23 | 0x33 => {
                // INX rp
                match opcode {
                    0x03 => self.set_bc(self.bc().wrapping_add(1)),
                    0x13 => self.set_de(self.de().wrapping_add(1)),
                    0x23 => self.set_hl(self.hl().wrapping_add(1)),
                    _ => self.sp = self.sp.wrapping_add(1),
                }
                self.cycles += 5;
            }
            0x0B | 0x1B | 0x2B | 0x3B => {
                // DCX rp
                match opcode {
                    0x0B => self.set_bc(self.bc().wrapping_sub(1)),
                    0x1B => self.set_de(self.de().wrapping_sub(1)),
                    0x2B => self.set_hl(self.hl().wrapping_sub(1)),
                    _ => self.sp = self.sp.wrapping_sub(1),
                }
                self.cycles += 5;
            }
            0x09 | 0x19 | 0x29 | 0x39 => {
                // DAD rp
                match opcode {
                    0x09 => self.dad(self.bc()),
                    0x19 => self.dad(self.de()),
                    0x29 => self.dad(self.hl()),
                    _ => self.dad(self.sp),
                }
                self.cycles += 10;
            }
            0x27 => {
                // DAA
                let mut cy = self.flags.cy;
                let mut correction: u8 = 0;
                let lsb = self.a & 0x0F;
                let msb = self.a >> 4;
                if self.flags.ac || lsb > 9 {
                    correction += 0x06;
                }
                if self.flags.cy || msb > 9 || (msb >= 9 && lsb > 9) {
                    correction += 0x60;
                    cy = true;
                }
                self.add(correction, false);
                self.flags.cy = cy;
                self.cycles += 4;
            }
            0x2F => {
                // CMA
                self.a = !self.a;
                self.cycles += 4;
            }
            0x37 => {
                // STC
                self.flags.cy = true;
                self.cycles += 4;
            }
            0x3F => {
                // CMC
                self.flags.cy = !self.flags.cy;
                self.cycles += 4;
            }
            0x07 => {
                // RLC
                self.flags.cy = self.a >> 7 == 1;
                self.a = self.a.rotate_left(1);
                self.cycles += 4;
            }
            0x0F => {
                // RRC
                self.flags.cy = self.a & 1 == 1;
                self.a = self.a.rotate_right(1);
                self.cycles += 4;
            }
            0x17 => {
                // RAL
                let old_cy = self.flags.cy;
                self.flags.cy = self.a >> 7 == 1;
                self.a = (self.a << 1) | old_cy as u8;
                self.cycles += 4;
            }
            0x1F => {
                // RAR
                let old_cy = self.flags.cy;
                self.flags.cy = self.a & 1 == 1;
                self.a = (self.a >> 1) | (old_cy as u8) << 7;
                self.cycles += 4;
            }

            // ---- stack -------------------------------------------------------
            0xC5 | 0xD5 | 0xE5 => {
                // PUSH rp
                let v = match opcode {
                    0xC5 => self.bc(),
                    0xD5 => self.de(),
                    _ => self.hl(),
                };
                self.push(bus, v);
                self.cycles += 11;
            }
            0xF5 => {
                // PUSH PSW
                self.push_psw(bus);
                self.cycles += 11;
            }
            0xC1 | 0xD1 | 0xE1 => {
                // POP rp
                let v = self.pop(bus);
                match opcode {
                    0xC1 => self.set_bc(v),
                    0xD1 => self.set_de(v),
                    _ => self.set_hl(v),
                }
                self.cycles += 10;
            }
            0xF1 => {
                // POP PSW
                self.pop_psw(bus);
                self.cycles += 10;
            }

            // ---- control flow --------------------------------------------------
            0xC3 | 0xCB => {
                // JMP (CB is undocumented JMP)
                let addr = self.fetch_word(bus);
                self.pc = addr;
                self.cycles += 10;
            }
            0xC2 | 0xCA | 0xD2 | 0xDA | 0xE2 | 0xEA | 0xF2 | 0xFA => {
                // Jcondition
                let cond = Self::condition(opcode);
                let addr = self.fetch_word(bus);
                if cond(self) {
                    self.pc = addr;
                }
                self.cycles += 10;
            }
            0xCD | 0xDD | 0xED | 0xFD => {
                // CALL (DD/ED/FD are undocumented CALLs)
                let addr = self.fetch_word(bus);
                let ret = self.pc;
                self.push(bus, ret);
                self.pc = addr;
                self.cycles += 17;
            }
            0xC4 | 0xCC | 0xD4 | 0xDC | 0xE4 | 0xEC | 0xF4 | 0xFC => {
                // Ccondition
                let cond = Self::condition(opcode);
                let addr = self.fetch_word(bus);
                if cond(self) {
                    let ret = self.pc;
                    self.push(bus, ret);
                    self.pc = addr;
                    self.cycles += 17;
                } else {
                    self.cycles += 11;
                }
            }
            0xC9 | 0xD9 => {
                // RET (D9 is undocumented RET)
                self.pc = self.pop(bus);
                self.cycles += 10;
            }
            0xC0 | 0xC8 | 0xD0 | 0xD8 | 0xE0 | 0xE8 | 0xF0 | 0xF8 => {
                // Rcondition
                let cond = Self::condition(opcode);
                if cond(self) {
                    self.pc = self.pop(bus);
                    self.cycles += 11;
                } else {
                    self.cycles += 5;
                }
            }
            0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => {
                // RST n
                let ret = self.pc;
                self.push(bus, ret);
                self.pc = (opcode & 0x38) as u16;
                self.cycles += 11;
            }

            // ---- I/O and machine ---------------------------------------------
            0xDB => {
                // IN p8
                let port = self.fetch_byte(bus);
                self.a = bus.port_in(port);
                self.cycles += 10;
            }
            0xD3 => {
                // OUT p8
                let port = self.fetch_byte(bus);
                bus.port_out(port, self.a);
                self.cycles += 10;
            }
            0xFB => {
                // EI
                self.iff = true;
                self.ei_delay = true;
                self.cycles += 4;
            }
            0xF3 => {
                // DI
                self.iff = false;
                self.cycles += 4;
            }
        }
    }

    fn condition(opcode: u8) -> impl Fn(&Cpu) -> bool {
        move |cpu: &Cpu| match (opcode >> 3) & 0x07 {
            0 => !cpu.flags.z, // NZ
            1 => cpu.flags.z,  // Z
            2 => !cpu.flags.cy, // NC
            3 => cpu.flags.cy, // C
            4 => !cpu.flags.p, // PO
            5 => cpu.flags.p,  // PE
            6 => !cpu.flags.s, // P
            _ => cpu.flags.s,  // M
        }
    }

    fn read_reg<B: Bus>(&self, bus: &mut B, r: Reg) -> u8 {
        match r {
            Reg::B => self.b,
            Reg::C => self.c,
            Reg::D => self.d,
            Reg::E => self.e,
            Reg::H => self.h,
            Reg::L => self.l,
            Reg::M => bus.read(self.hl()),
            Reg::A => self.a,
        }
    }

    fn write_reg<B: Bus>(&mut self, bus: &mut B, r: Reg, v: u8) {
        match r {
            Reg::B => self.b = v,
            Reg::C => self.c = v,
            Reg::D => self.d = v,
            Reg::E => self.e = v,
            Reg::H => self.h = v,
            Reg::L => self.l = v,
            Reg::M => bus.write(self.hl(), v),
            Reg::A => self.a = v,
        }
    }
}

/// Operand register encoding used by the MOV/MVI/INR/DCR group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reg {
    B,
    C,
    D,
    E,
    H,
    L,
    M,
    A,
}

impl Reg {
    fn from_opcode(code: u8) -> Reg {
        match code & 0x07 {
            0 => Reg::B,
            1 => Reg::C,
            2 => Reg::D,
            3 => Reg::E,
            4 => Reg::H,
            5 => Reg::L,
            6 => Reg::M,
            _ => Reg::A,
        }
    }

    /// Decode the source and destination registers of a MOV opcode.
    fn decode_move(opcode: u8) -> (Reg, Reg) {
        (Reg::from_opcode(opcode >> 3), Reg::from_opcode(opcode))
    }
}