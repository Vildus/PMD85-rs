//! The emulated machine: CPU, memory, peripheral chips and the I/O port
//! dispatch, tied together.
//!
//! [`MachineBus`] implements [`cpu::Bus`] and holds everything except the
//! CPU; [`Machine`] pairs the two and exposes a frame-based stepping API
//! for the presentation layer.

use crate::audio::{SpeakerEdge, SpeakerEdgeLog};
use crate::bus::Memory;
use crate::chips::ppi8255::{I8255, Port};
use crate::chips::pit8253::I8253;
use crate::chips::usart8251::I8251;
use crate::cpu::{Bus, Cpu, Flags};
use crate::keyboard::Keyboard;
use crate::model::Model;
use crate::state::{StateError, StateReader, StateWriter};
use crate::tapedeck::{PlayItem, TapeDeck};

use std::collections::HashSet;

/// CPU clock: Tesla MHB8080A at 2.048 MHz.
pub const CPU_CLOCK_HZ: u64 = 2_048_000;
/// Video refresh rate.
pub const FRAMES_PER_SECOND: u64 = 50;
/// CPU cycles per frame.
pub const CYCLES_PER_FRAME: u64 = CPU_CLOCK_HZ / FRAMES_PER_SECOND;

/// Everything the CPU talks to: memory and I/O devices.
pub struct MachineBus {
    pub model: Model,
    pub memory: Memory,
    pub keyboard: Keyboard,
    /// Motherboard 8255: keyboard scan, speaker, LEDs.
    ppi_system: I8255,
    /// I/O board 8255 for GPIO/0 and GPIO/1 (K3/K4 connectors).
    ppi_gpio: I8255,
    /// I/O board 8255 for the IMS-2 interface (K5 connector).
    ppi_ims2: I8255,
    /// ROM module 8255.
    ppi_rom: I8255,
    pub pit: I8253,
    pub uart: I8251,
    /// The cassette deck, wired to the 8251 (DSR in, TX out).
    pub tape: TapeDeck,
    /// Speaker level (combined PC2/PC0/PC1 sound circuit, see
    /// [`speaker_level_from`]).
    speaker_level: bool,
    /// LED driven from PC3 of the system 8255 (red).
    pub led: bool,
    /// Cycle-stamped speaker transitions, drained by the audio frontend.
    speaker_edges: SpeakerEdgeLog,
    /// The speaker level changed during the current instruction; the
    /// edge gets stamped (with the end-of-instruction cycle count) by
    /// `advance_time`.
    pending_speaker_edge: bool,
    /// Emulated-time bookkeeping.
    total_cycles: u64,
}

impl MachineBus {
    pub fn new(model: Model, monitor: &[u8], rom_module: Option<Vec<u8>>) -> Self {
        let mut memory = Memory::new(model, monitor);
        memory.rom_module = rom_module;
        let mut bus = MachineBus {
            model,
            memory,
            keyboard: Keyboard::new(),
            ppi_system: I8255::new(),
            ppi_gpio: I8255::new(),
            ppi_ims2: I8255::new(),
            ppi_rom: I8255::new(),
            pit: I8253::new(),
            uart: I8251::new(),
            tape: TapeDeck::new(),
            speaker_level: false,
            led: false,
            speaker_edges: SpeakerEdgeLog::default(),
            pending_speaker_edge: false,
            total_cycles: 0,
        };
        bus.reset();
        bus
    }

    /// Cold/warm reset: memory maps and chips return to their boot state
    /// (RAM contents are preserved, like on hardware).
    pub fn reset(&mut self) {
        self.memory.reset();
        self.keyboard.reset();
        self.ppi_system.reset();
        self.ppi_gpio.reset();
        self.ppi_ims2.reset();
        self.ppi_rom.reset();
        self.pit.reset();
        self.uart.reset();
        self.tape.hard_reset(&mut self.uart);
        self.speaker_level = false;
        self.led = false;
        self.speaker_edges.clear();
        self.pending_speaker_edge = false;
    }

    /// The piezo transducer on the PMD 85 keyboard is driven by a small
    /// sound circuit fed from the system 8255 port C (see pmd85.borik.net,
    /// "Klávesnica PMD 85"):
    ///
    /// - PC2 drives the transducer directly: software can generate a
    ///   square wave of arbitrary pitch by toggling it. A constant PC2=1
    ///   is DC, i.e. silence.
    /// - PC0 and PC1 gate fixed ~1 kHz / ~4 kHz tones derived from the
    ///   video divider; this is how the monitors make their key clicks
    ///   (`IN F6; ORA 2; OUT F6` bursts).
    ///
    /// The effective level is the OR of all three contributions.
    fn speaker_level_from(pc: u8, cycles: u64) -> bool {
        if pc & 0x04 != 0 {
            return true;
        }
        // 1 kHz: period 2048 cycles; 4 kHz: period 512 cycles. Phase is
        // tied to the video divider; any phase is authentic.
        let clk_1k = (cycles >> 10) & 1 == 1;
        let clk_4k = (cycles >> 8) & 1 == 1;
        (pc & 0x01 != 0 && clk_1k) || (pc & 0x02 != 0 && clk_4k)
    }

    /// Recompute the speaker level from the port C latch at the current
    /// cycle count; mark a pending edge if it changed.
    fn refresh_speaker(&mut self) {
        let level = Self::speaker_level_from(self.ppi_system.outputs[2], self.total_cycles);
        if level != self.speaker_level {
            self.speaker_level = level;
            self.pending_speaker_edge = true;
        }
    }

    pub fn speaker_level(&self) -> bool {
        self.speaker_level
    }

    /// Drain recorded speaker edges (cycle-stamped PC2 transitions),
    /// oldest first. Intended to be called once per frame by the audio
    /// frontend.
    pub fn take_speaker_edges(&mut self) -> Vec<SpeakerEdge> {
        self.speaker_edges.drain()
    }

    /// Current keyboard scan column (system 8255 port A output).
    pub fn ppi_system_scan(&self) -> u8 {
        self.ppi_system.outputs[0]
    }

    /// Stop tape playback (convenience for the frontend).
    pub fn tape_stop(&mut self) {
        self.tape.stop(&mut self.uart);
    }

    /// Start a tape playback session (convenience for the frontend):
    /// the blocks are queued in the deck and fed one after another.
    pub fn tape_play_session(&mut self, items: Vec<PlayItem>) {
        self.tape.play_session(items);
    }

    /// Serialize the bus contents (everything but the CPU, which is
    /// written separately by [`Machine::save_state`]) into a save
    /// state. The cassette deck is deliberately not covered: a state
    /// can only be taken while it is idle.
    pub(crate) fn save_state(&self, w: &mut StateWriter) {
        self.keyboard.save_state(w);
        self.memory.save_state(w);
        self.ppi_system.save_state(w);
        self.ppi_gpio.save_state(w);
        self.ppi_ims2.save_state(w);
        self.ppi_rom.save_state(w);
        self.pit.save_state(w);
        self.uart.save_state(w);
        w.bool(self.speaker_level);
        w.bool(self.led);
        w.len(self.speaker_edges.len());
        for edge in self.speaker_edges.edges() {
            w.u64(edge.cycle);
            w.bool(edge.level);
        }
        w.bool(self.pending_speaker_edge);
        w.u64(self.total_cycles);
    }

    /// Restore the bus state written by [`MachineBus::save_state`].
    /// The deck lines (8251 DSR, transmit gate) are normalized to the
    /// idle state afterwards, as the deck itself always restarts idle.
    pub(crate) fn load_state(&mut self, r: &mut StateReader) -> Result<(), StateError> {
        self.keyboard.load_state(r)?;
        self.memory.load_state(r)?;
        self.ppi_system.load_state(r)?;
        self.ppi_gpio.load_state(r)?;
        self.ppi_ims2.load_state(r)?;
        self.ppi_rom.load_state(r)?;
        self.pit.load_state(r)?;
        self.uart.load_state(r)?;
        self.speaker_level = r.bool()?;
        self.led = r.bool()?;
        for _ in 0..r.len()? {
            let edge = crate::audio::SpeakerEdge {
                cycle: r.u64()?,
                level: r.bool()?,
            };
            self.speaker_edges.push(edge);
        }
        self.pending_speaker_edge = r.bool()?;
        self.total_cycles = r.u64()?;
        self.tape.hard_reset(&mut self.uart);
        Ok(())
    }

    /// Emulated CPU cycles executed since power-on.
    pub fn total_cycles(&self) -> u64 {
        self.total_cycles
    }

    /// Advance peripheral clocks by `cycles` CPU cycles.
    fn advance_time(&mut self, cycles: u64) {
        self.total_cycles += cycles;
        // The speaker level can change both through port C writes (which
        // call refresh_speaker inside the instruction) and through the
        // fixed-tone clock phases flipping; either way the transition is
        // stamped at the end of the current instruction.
        self.refresh_speaker();
        if self.pending_speaker_edge {
            self.pending_speaker_edge = false;
            self.speaker_edges.push(SpeakerEdge {
                cycle: self.total_cycles,
                level: self.speaker_level,
            });
        }
        // 8253 counter 1 is clocked by the ~2 MHz system clock, counter 2
        // by a 1 Hz generator.
        let seconds_before = (self.total_cycles - cycles) / CPU_CLOCK_HZ;
        for _ in 0..cycles {
            self.pit.tick(1, true);
        }
        if self.total_cycles / CPU_CLOCK_HZ != seconds_before {
            self.pit.tick(2, true);
        }
        // The cassette deck runs off the CPU clock (its half-clock is
        // 853 cycles) and drives the 8251 DSR line.
        self.tape.tick(cycles, &mut self.uart);
    }

    /// True if a ROM module is connected.
    fn rom_module_present(&self) -> bool {
        self.memory.rom_module.is_some()
    }

    fn io_read(&mut self, port: u8) -> u8 {
        // All I/O reads return open bus while the startup shadow map is
        // still active.
        if self.memory.startup_map() {
            return 0xFF;
        }
        let reg = (port & 3) as u32;
        match port & 0x0C {
            0x04 => {
                // Motherboard 8255
                if port & 0x80 != 0 {
                    let pb = self.keyboard.read_rows(self.ppi_system.outputs[0]);
                    return self.ppi_system.read(Port::from_index(reg), 0xFF, pb, 0xFF);
                }
            }
            0x08 => {
                // ROM module 8255
                if port & 0x80 != 0 && self.rom_module_present() {
                    let addr = self.ppi_rom.outputs[1] as usize
                        | ((self.ppi_rom.outputs[2] as usize) << 8);
                    let pa = match &self.memory.rom_module {
                        Some(module) if addr < module.len() => module[addr],
                        _ => 0,
                    };
                    return self.ppi_rom.read(Port::from_index(reg), pa, 0xFF, 0xFF);
                }
            }
            0x0C if port & 0x80 == 0 => {
                // I/O board interfaces
                match port & 0x70 {
                    0x10 => return self.uart.read(port as u32 & 1),
                    0x40 => return self.ppi_gpio.read(Port::from_index(reg), 0xFF, 0xFF, 0xFF),
                    0x50 => return self.pit.read(port as u32 & 3),
                    0x70 => return self.ppi_ims2.read(Port::from_index(reg), 0xFF, 0xFF, 0xFF),
                    _ => {}
                }
            }
            _ => {}
        }
        0xFF
    }

    fn io_write(&mut self, port: u8, data: u8) {
        // The first I/O write clears the startup shadow map, before the
        // write is dispatched anywhere.
        self.memory.note_io_write();
        let reg = (port & 3) as u32;

        match port & 0x0C {
            0x04 => {
                // Motherboard
                if port & 0x80 != 0 {
                    // PMD-85.3 memory banking comes from bit 0 of any value
                    // written to the system 8255 control register.
                    if reg == 3 {
                        self.memory.set_pmd853_mapping(data & 0x01 != 0);
                    }
                    // PC2 = speaker, PC3 = LED; both follow the output
                    // latch (full writes and BSR operations alike).
                    self.ppi_system.write(Port::from_index(reg), data);
                    self.led = self.ppi_system.outputs[2] & 0x08 != 0;
                    self.refresh_speaker();
                }
            }
            0x08 => {
                // ROM module connector
                if port & 0x80 != 0 && self.rom_module_present() {
                    self.ppi_rom.write(Port::from_index(reg), data);
                }
            }
            0x0C if port & 0x80 == 0 => {
                // I/O board interfaces
                match port & 0x70 {
                    0x10 => {
                        let reg = port as u32 & 1;
                        self.uart.write(reg, data);
                        // Data writes to the transmitter are sniffed by
                        // the tape deck recorder (and paced by it).
                        if reg == 0 {
                            self.tape.on_tx_byte(data, &mut self.uart);
                        }
                    }
                    0x40 => self.ppi_gpio.write(Port::from_index(reg), data),
                    0x50 => self.pit.write(port as u32 & 3, data),
                    0x70 => self.ppi_ims2.write(Port::from_index(reg), data),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

impl Bus for MachineBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.memory.read(addr)
    }

    fn write(&mut self, addr: u16, data: u8) {
        self.memory.write(addr, data)
    }

    fn port_in(&mut self, port: u8) -> u8 {
        self.io_read(port)
    }

    fn port_out(&mut self, port: u8, data: u8) {
        self.io_write(port, data)
    }
}

/// A complete emulated PMD 85.
pub struct Machine {
    pub cpu: Cpu,
    pub bus: MachineBus,
    /// Debugger breakpoints (session-only: not part of save states).
    breakpoints: HashSet<u16>,
    /// The address the last breakpoint stopped at, if any.
    breakpoint_hit: Option<u16>,
    /// Whether the run loop stops at breakpoints (a hit disarms
    /// until [`Machine::resume`] or [`Machine::step_once`] re-arms).
    breakpoints_armed: bool,
    /// Skip the breakpoint check for one instruction (the resume
    /// path, so the hit address executes once without re-tripping).
    breakpoint_skip: bool,
}

impl Machine {
    pub fn new(model: Model, monitor: &[u8], rom_module: Option<Vec<u8>>) -> Self {
        Machine {
            cpu: Cpu::new(),
            bus: MachineBus::new(model, monitor, rom_module),
            breakpoints: HashSet::new(),
            breakpoint_hit: None,
            breakpoints_armed: true,
            breakpoint_skip: false,
        }
    }

    /// Reset (CPU registers, memory maps, peripherals; RAM preserved).
    pub fn reset(&mut self) {
        self.cpu.reset();
        self.bus.reset();
    }

    pub fn model(&self) -> Model {
        self.bus.model
    }

    /// Current keyboard scan column selected through the system 8255
    /// (debugging aid).
    pub fn keyboard_scan_column(&self) -> u8 {
        self.bus.ppi_system_scan()
    }

    /// Drain recorded speaker edges (cycle-stamped PC2 transitions),
    /// oldest first. Intended to be called once per frame by the audio
    /// frontend.
    pub fn take_speaker_edges(&mut self) -> Vec<SpeakerEdge> {
        self.bus.take_speaker_edges()
    }

    /// Run one video frame worth of emulation (~20 ms).
    pub fn step_frame(&mut self) {
        self.run_cycles(CYCLES_PER_FRAME);
    }

    /// Run a number of CPU cycles (used by tests and the debugger).
    /// Stops early, exactly at a breakpoint: the PC then sits on the
    /// breakpoint address, before its instruction executes.
    pub fn run_cycles(&mut self, cycles: u64) {
        let target = self.bus.total_cycles() + cycles;
        while self.bus.total_cycles() < target {
            if !self.step_checked() {
                return;
            }
        }
    }

    /// Execute exactly one CPU instruction (or interrupt), advancing
    /// peripheral clocks. Returns the cycles consumed. Never stops at
    /// a breakpoint; re-arms them for the next run.
    pub fn step_once(&mut self) -> u32 {
        self.breakpoint_hit = None;
        self.breakpoints_armed = true;
        self.breakpoint_skip = false;
        self.step_instruction()
    }

    /// One instruction through the flash-load interception and the
    /// breakpoint machinery. Returns false when an (armed) breakpoint
    /// stopped the run before the instruction at the PC executed.
    fn step_checked(&mut self) -> bool {
        if self.breakpoint_skip {
            self.breakpoint_skip = false;
        } else if self.breakpoints_armed && self.breakpoints.contains(&self.cpu.pc) {
            self.breakpoint_hit = Some(self.cpu.pc);
            self.breakpoints_armed = false;
            return false;
        }
        self.step_instruction();
        true
    }

    /// The raw instruction step: flash-load interception, CPU step,
    /// peripheral clock advance.
    fn step_instruction(&mut self) -> u32 {
        self.tape_flash_intercept();
        let c = self.cpu.step(&mut self.bus) as u64;
        self.bus.advance_time(c);
        c as u32
    }

    // ----- breakpoints ---------------------------------------------------

    /// Toggle a breakpoint at `addr`. Returns true when it is now
    /// set, false when it was removed.
    pub fn toggle_breakpoint(&mut self, addr: u16) -> bool {
        if self.breakpoints.remove(&addr) {
            false
        } else {
            self.breakpoints.insert(addr);
            true
        }
    }

    /// The set breakpoint addresses.
    pub fn breakpoints(&self) -> &HashSet<u16> {
        &self.breakpoints
    }

    /// The address the last breakpoint stopped at, if any.
    pub fn breakpoint_hit(&self) -> Option<u16> {
        self.breakpoint_hit
    }

    /// Continue after a breakpoint hit: breakpoints re-arm, and the
    /// instruction at the hit address executes once unchecked so the
    /// same hit does not immediately repeat.
    pub fn resume(&mut self) {
        self.breakpoint_hit = None;
        self.breakpoints_armed = true;
        self.breakpoint_skip = true;
    }

    /// Debugger step-over: a single step — unless the instruction at
    /// the PC is a CALL, in which case the whole call runs until the
    /// PC is the return address (or a breakpoint stops inside). A
    /// cycle cap keeps a call that never returns from hanging the
    /// caller.
    pub fn step_over(&mut self) {
        let opcode = self.bus.read(self.cpu.pc);
        if !crate::disasm::is_call(opcode) {
            self.step_once();
            return;
        }
        let ret = self.cpu.pc.wrapping_add(3);
        self.breakpoint_hit = None;
        self.breakpoints_armed = true;
        self.breakpoint_skip = false;
        let deadline = self.bus.total_cycles() + CYCLES_PER_FRAME * 10;
        while self.bus.total_cycles() < deadline && self.cpu.pc != ret {
            if !self.step_checked() {
                return;
            }
        }
    }

    /// Flash-load interception of the monitor ROM's tape read loops,
    /// a port of GPMD85's flash loading. While the deck serves a
    /// flash block, the entry points of the block reader (`EDC4`,
    /// or its compatibility-mode copy at `8DC4`) and of the byte
    /// reader (`EB6C` / `8B6C`) never execute: they take their data
    /// straight from the deck instead of demodulating the tape
    /// signal. Everything around them — header filters, checksums,
    /// the return into the caller, autorun stack overwrites — runs
    /// for real, so a multi-block game fast-loads exactly as it would
    /// from a real tape, just without the transfer time.
    ///
    /// The entry signatures (bytes at fixed offsets around the entry
    /// point) are verified through the bus first, so both the native
    /// ROM and the monitor-3 copy at `0x8000` (compatibility mode)
    /// are covered, and coincidental callers elsewhere in RAM are
    /// left alone.
    fn tape_flash_intercept(&mut self) {
        if !self.bus.tape.flash_armed() {
            return;
        }
        match self.cpu.pc {
            0xEDC4 | 0x8DC4 => self.flash_block_read(),
            0xEB6C | 0x8B6C => self.flash_byte_read(),
            _ => {}
        }
    }

    /// Replace the block-read loop entered at `EDC4`/`8DC4` (the
    /// entry's 256-byte page carries the copy in use). Post-conditions
    /// mirror the ROM's exit at `EDE1`: HL restored to the start
    /// address, DE = 0xFFFF, B = the running checksum, C preserved,
    /// A = 0 with Z set on a checksum match and CY set on a mismatch
    /// (GPMD85 semantics; the ROM's callers test both).
    fn flash_block_read(&mut self) {
        let page = self.cpu.pc & 0xFF00;
        // Signature: INX HL at page|0xD3, RET at page|0xE1.
        if self.bus.read(page | 0xD3) != 0x23 || self.bus.read(page | 0xE1) != 0xC9 {
            return;
        }
        let Some(len) = self.bus.tape.flash_len() else {
            return;
        };
        let start = u16::from_le_bytes([self.cpu.l, self.cpu.h]);
        let count = u16::from_le_bytes([self.cpu.e, self.cpu.d]) as usize + 1;
        let check_only = self.cpu.c == 0;
        // The block must still hold `count` data bytes plus the
        // checksum byte.
        if len < count + 1 {
            return;
        }
        let mut crc = 0u8;
        for i in 0..count {
            let b = self.bus.tape.flash_at(i).unwrap_or(0);
            if !check_only {
                self.bus.write(start.wrapping_add(i as u16), b);
            }
            crc = crc.wrapping_add(b);
        }
        let checksum = self.bus.tape.flash_at(count).unwrap_or(0);
        self.cpu.d = 0xFF;
        self.cpu.e = 0xFF;
        self.cpu.b = crc;
        self.cpu.a = 0;
        self.cpu.flags = Flags {
            z: crc == checksum,
            cy: crc != checksum,
            ..Flags::default()
        };
        self.cpu.pc = page | 0xE1;
        self.bus.tape.flash_accept(count + 1);
    }

    /// Replace the byte reader entered at `EB6C`/`8B6C`. One byte is
    /// served with all flags clear; with no flash data left the
    /// reader returns with carry set, its timeout path.
    fn flash_byte_read(&mut self) {
        let pc = self.cpu.pc;
        let page = pc & 0xFF00;
        // Signature: PUSH BC at the entry, RET at page|0x9B.
        if self.bus.read(pc) != 0xC5 || self.bus.read(page | 0x9B) != 0xC9 {
            return;
        }
        let byte = self.bus.tape.flash_byte();
        self.cpu.flags = Flags {
            cy: byte.is_none(),
            ..Flags::default()
        };
        if let Some(b) = byte {
            self.cpu.a = b;
        }
        self.cpu.pc = page | 0x9B;
    }

    // ----- save states -------------------------------------------------

    /// Serialize the machine into a self-contained save state: CPU,
    /// RAM, keyboard, peripheral chips and the cycle counter, plus the
    /// ROM module image. The monitor ROM itself is only fingerprinted
    /// (length + CRC): on restore it must match the one the target
    /// machine runs.
    ///
    /// The cassette deck is not part of the snapshot, so this fails
    /// with [`StateError::TapeBusy`] while a tape session (playback
    /// or recording) is in progress.
    pub fn save_state(&self) -> Result<Vec<u8>, StateError> {
        if self.bus.tape.is_active() {
            return Err(StateError::TapeBusy);
        }
        let mut w = StateWriter::new();
        w.bytes(MAGIC);
        w.u32(STATE_VERSION);
        w.u8(model_tag(self.bus.model));
        let (rom_len, rom_crc) = self.bus.memory.monitor_signature();
        w.u16(rom_len);
        w.u8(rom_crc);
        match &self.bus.memory.rom_module {
            Some(module) => {
                w.len(module.len());
                w.bytes(module);
            }
            None => w.len(0),
        }
        self.bus.save_state(&mut w);
        self.cpu.save_state(&mut w);
        Ok(w.finish())
    }

    /// Restore a state written by [`Machine::save_state`]. The load
    /// is transactional: the state is parsed into a fresh machine and
    /// only applied on success, so a corrupt or foreign file never
    /// half-mutates the running one. The cassette deck always comes
    /// back idle (a state cannot be taken while it runs).
    pub fn restore_state(&mut self, data: &[u8]) -> Result<(), StateError> {
        let mut r = StateReader::new(data);
        let mut magic = [0u8; 4];
        r.read_into(&mut magic)?;
        if &magic != MAGIC {
            return Err(StateError::BadMagic);
        }
        let version = r.u32()?;
        if version != STATE_VERSION {
            return Err(StateError::UnsupportedVersion(version));
        }
        let tag = r.u8()?;
        if model_from_tag(tag) != Some(self.bus.model) {
            return Err(StateError::ModelMismatch);
        }
        let rom_len = r.u16()?;
        let rom_crc = r.u8()?;
        if (rom_len, rom_crc) != self.bus.memory.monitor_signature() {
            return Err(StateError::MonitorMismatch);
        }
        let module = match r.len()? {
            0 => None,
            len => Some(r.take_bytes(len)?.to_vec()),
        };

        // Parse into a fresh machine; only a complete success replaces
        // the running one.
        let monitor = self.bus.memory.monitor().to_vec();
        let mut bus = MachineBus::new(self.bus.model, &monitor, module);
        bus.load_state(&mut r)?;
        let mut cpu = Cpu::new();
        cpu.load_state(&mut r)?;
        if !r.is_empty() {
            return Err(StateError::TrailingBytes);
        }
        self.bus = bus;
        self.cpu = cpu;
        Ok(())
    }
}

/// Save-state format magic and version.
const MAGIC: &[u8; 4] = b"PMDS";
const STATE_VERSION: u32 = 1;

fn model_tag(model: Model) -> u8 {
    match model {
        Model::Pmd851 => 1,
        Model::Pmd852 => 2,
        Model::Pmd852a => 3,
        Model::Pmd853 => 4,
    }
}

fn model_from_tag(tag: u8) -> Option<Model> {
    match tag {
        1 => Some(Model::Pmd851),
        2 => Some(Model::Pmd852),
        3 => Some(Model::Pmd852a),
        4 => Some(Model::Pmd853),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard::Key;

    fn rom(size: usize) -> Vec<u8> {
        vec![0x00; size]
    }

    /// A fresh 85-3 with the startup shadow map dropped, so code in
    /// RAM runs: `prog` lands at 0x0500, the PC starts on it.
    fn code(prog: &[u8]) -> Machine {
        let mut m = Machine::new(Model::Pmd853, &rom(0x2000), None);
        m.bus.io_write(0x87, 0x00); // any write clears the startup map
        m.bus.memory.ram[0x0500..0x0500 + prog.len()].copy_from_slice(prog);
        m.cpu.pc = 0x0500;
        m.cpu.sp = 0x0F00;
        m
    }

    /// Breakpoints stop the run exactly at the address, before its
    /// instruction executes; a step re-arms them; resume runs the hit
    /// address once unchecked before checking again.
    #[test]
    fn breakpoints_stop_step_and_resume() {
        // 0x0500: NOP ; NOP ; JMP 0500 — a tight loop over the
        // breakpoint at 0x0502.
        let mut m = code(&[0x00, 0x00, 0xC3, 0x00, 0x05]);

        assert!(m.toggle_breakpoint(0x0502), "now set");
        assert_eq!(m.breakpoints().len(), 1);
        m.run_cycles(100_000);
        assert_eq!(m.breakpoint_hit(), Some(0x0502));
        assert_eq!(m.cpu.pc, 0x0502, "stopped exactly at the breakpoint");
        assert_eq!(m.bus.total_cycles(), 8, "exactly the two NOPs ran");

        // A step executes the JMP (re-armed) and the next run trips
        // again on arrival.
        m.step_once();
        assert_eq!(m.cpu.pc, 0x0500, "the JMP executed");
        assert_eq!(m.breakpoint_hit(), None, "the step clears the hit");
        m.run_cycles(100_000);
        assert_eq!(m.breakpoint_hit(), Some(0x0502));
        assert_eq!(m.bus.total_cycles(), 26, "JMP + NOP + NOP ran");

        // Resume: the instruction at the hit address runs once
        // unchecked (cycles advance by the whole lap), then the
        // breakpoint trips again on arrival.
        m.resume();
        m.run_cycles(100_000);
        assert_eq!(m.breakpoint_hit(), Some(0x0502));
        assert_eq!(m.bus.total_cycles(), 44, "the hit address ran once");

        // Removing the breakpoint frees the loop.
        assert!(!m.toggle_breakpoint(0x0502), "now cleared");
        assert!(m.breakpoints().is_empty());
        m.resume();
        m.run_cycles(1_000);
        assert_eq!(m.breakpoint_hit(), None);
    }

    /// Step-over runs a whole CALL and lands on the return address;
    /// over anything else it is a single step.
    #[test]
    fn step_over_crosses_a_call() {
        // 0x0500: CALL 0506 ; 0x0503: HLT ; 0x0506: RET
        let mut m = code(&[0xCD, 0x06, 0x05, 0x76, 0x00, 0x00, 0xC9]);
        m.step_over();
        assert_eq!(m.cpu.pc, 0x0503, "the call ran to its return address");
        assert!(!m.cpu.halted, "the HLT past it did not run");
        // Over the HLT (not a call): one plain step.
        m.step_over();
        assert!(m.cpu.halted, "HLT executed by the single step");
    }

    /// A breakpoint inside a called routine stops a step-over there.
    #[test]
    fn step_over_stops_at_a_breakpoint_inside() {
        // 0x0500: CALL 0506 ; 0x0503: HLT ; 0x0506: MVI A,FF ; RET
        let mut m = code(&[0xCD, 0x06, 0x05, 0x76, 0x00, 0x00, 0x3E, 0xFF, 0xC9]);
        m.toggle_breakpoint(0x0508);
        m.step_over();
        assert_eq!(m.breakpoint_hit(), Some(0x0508));
        assert_eq!(m.cpu.pc, 0x0508, "stopped inside the call, not over it");
    }

    #[test]
    fn names_round_trip() {
        for model in [
            Model::Pmd851,
            Model::Pmd852,
            Model::Pmd852a,
            Model::Pmd853,
        ] {
            assert_eq!(Model::from_str_loose(model.name()), Some(model));
        }
    }

    #[test]
    fn io_reads_blocked_during_startup_map() {
        let mut m = Machine::new(Model::Pmd853, &rom(0x2000), None);
        // While the startup map is active, I/O reads float high.
        assert_eq!(m.bus.io_read(0x87), 0xFF);
        assert_eq!(m.bus.io_read(0x1C), 0xFF);
        // A single I/O write clears it.
        m.bus.io_write(0x87, 0x00);
        assert!(!m.bus.memory.startup_map());
    }

    #[test]
    fn keyboard_scan_through_system_ppi() {
        let mut m = Machine::new(Model::Pmd853, &rom(0x2000), None);
        m.bus.io_write(0x87, 0x00); // clear startup map
        // configure: mode 0, PA out, PB in, PC out: 0x82
        m.bus.io_write(0x87, 0x82);
        // write column 3 to port A (0x84 pattern: 1xxx01 00)
        m.bus.io_write(0x84, 0x03);
        m.bus.keyboard.set_key(Key::R, true); // col 3, bit 2
        assert_eq!(m.bus.io_read(0x85), !0x04);
        // column 5 must not see it
        m.bus.io_write(0x84, 0x05);
        assert_eq!(m.bus.io_read(0x85), 0xFF);
    }

    #[test]
    fn pmd853_banking_via_control_write() {
        let mut m = Machine::new(Model::Pmd853, &rom(0x2000), None);
        m.bus.io_write(0x87, 0x00); // clear startup map; ROM visible at 0xE000
        assert_eq!(m.bus.memory.read(0xE000), 0x00);
        m.bus.memory.write(0xE000, 0x77);
        // control word with bit 0 clear -> AllRAM mode
        m.bus.io_write(0x87, 0x82);
        assert_eq!(m.bus.memory.read(0xE000), 0x77);
        // control word with bit 0 set -> ROM visible again
        m.bus.io_write(0x87, 0x83);
        assert_eq!(m.bus.memory.read(0xE000), 0x00);
    }

    #[test]
    fn speaker_follows_pc2() {
        let mut m = Machine::new(Model::Pmd851, &rom(0x1000), None);
        m.bus.io_write(0x87, 0x00);
        // BSR set PC2: bit select 2, set -> 0b0000_0101
        m.bus.io_write(0x87, 0b0000_0101);
        assert!(m.bus.speaker_level());
        assert!(!m.bus.led);
        // BSR set PC3: bit select 3, set -> 0b0000_0111
        m.bus.io_write(0x87, 0b0000_0111);
        assert!(m.bus.led);
        // BSR reset PC2: bit select 2, reset -> 0b0000_0100
        m.bus.io_write(0x87, 0b0000_0100);
        assert!(!m.bus.speaker_level());
    }

    #[test]
    fn speaker_edges_are_cycle_stamped() {
        // MVI A,0x05; OUT 0x87 (BSR: set PC2); then MVI A,0x04; OUT 0x87
        // (reset PC2). The first two instructions execute from the ROM
        // via the startup shadow map; the OUT write drops the shadow map,
        // so the rest is fetched from RAM, where it is pre-placed (the
        // 0x00 filler is NOP). MVI takes 7 cycles, OUT 10.
        let mut monitor = vec![0x00; 0x2000];
        monitor[0] = 0x3E;
        monitor[1] = 0x05;
        monitor[2] = 0xD3;
        monitor[3] = 0x87;
        let mut m = Machine::new(Model::Pmd853, &monitor, None);
        m.bus.memory.ram[4] = 0x3E;
        m.bus.memory.ram[5] = 0x04;
        m.bus.memory.ram[6] = 0xD3;
        m.bus.memory.ram[7] = 0x87;
        m.run_cycles(40);
        assert_eq!(
            m.take_speaker_edges(),
            vec![
                SpeakerEdge {
                    cycle: 17,
                    level: true
                },
                SpeakerEdge {
                    cycle: 34,
                    level: false
                },
            ]
        );
        // drained: nothing left
        assert!(m.take_speaker_edges().is_empty());
    }

    #[test]
    fn speaker_pc0_gates_1khz_tone() {
        let mut m = Machine::new(Model::Pmd852, &rom(0x1000), None);
        m.bus.io_write(0x87, 0x00); // any write clears the startup map
        // BSR: set PC0 -> gates the 1 kHz divider tone onto the speaker
        m.bus.io_write(0x87, 0b0000_0001);
        m.step_frame();
        let edges = m.take_speaker_edges();
        // 1 kHz square wave over a 20 ms frame: ~40 level transitions
        assert!(
            (38..=42).contains(&edges.len()),
            "unexpected 1 kHz edge count {}",
            edges.len()
        );
        for pair in edges.windows(2) {
            assert_eq!(pair[0].level, !pair[1].level);
        }
        // clearing the gate silences it again
        m.bus.io_write(0x87, 0b0000_0000);
        m.step_frame();
        assert!(m.take_speaker_edges().is_empty());
    }

    #[test]
    fn rom_module_read() {
        let module = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let mut m = Machine::new(Model::Pmd853, &rom(0x2000), Some(module));
        m.bus.io_write(0x87, 0x00); // clear startup map
        // ROM module PPI at pattern 1xxx10aa -> 0x88..0x8B:
        // PA = data read, PB/C = address select.
        // mode word 0x98: PA in, PB out, PC out
        m.bus.io_write(0x8B, 0x98);
        m.bus.io_write(0x89, 0x02); // address low
        m.bus.io_write(0x8A, 0x00); // address high
        assert_eq!(m.bus.io_read(0x88), 0xBE);
        // reading PB (an output) returns its latch
        assert_eq!(m.bus.io_read(0x89), 0x02);
    }

    #[test]
    fn uart_visible_on_io_board() {
        let mut m = Machine::new(Model::Pmd853, &rom(0x2000), None);
        m.bus.io_write(0x87, 0x00);
        // 8251 data write (0x1C), control write (0x1D), status read
        m.bus.io_write(0x1D, 0x4E); // mode word
        m.bus.io_write(0x1D, 0x27); // command
        m.bus.io_write(0x1C, 0x42);
        assert_eq!(m.bus.uart.tx_log, vec![0x42]);
        // The tape deck paces the transmitter for one byte time
        // (22 half-clocks at 853 cycles); let it elapse.
        assert_eq!(m.bus.io_read(0x1D) & 0x05, 0, "transmitter busy");
        m.bus.advance_time(22 * 853 + 1);
        assert_eq!(m.bus.io_read(0x1D) & 0x05, 0x05);
    }

    #[test]
    fn pit_visible_on_io_board() {
        let mut m = Machine::new(Model::Pmd853, &rom(0x2000), None);
        m.bus.io_write(0x87, 0x00);
        // 8253 at pattern 010111aa: 0x5C-0x5F
        m.bus.io_write(0x5F, 0b0011_0000); // counter 0, mode 0, lsb/msb
        m.bus.io_write(0x5C, 0x10);
        m.bus.io_write(0x5C, 0x00);
        assert_eq!(m.bus.pit.counters[0].initial, 0x10);
    }

    #[test]
    fn frame_stepping_makes_progress() {
        let mut m = Machine::new(Model::Pmd853, &rom(0x2000), None);
        let before = m.bus.total_cycles();
        m.step_frame();
        assert_eq!(m.bus.total_cycles() - before, CYCLES_PER_FRAME);
    }

    #[test]
    fn cpu_executes_from_mapped_rom() {
        // A tiny program in the monitor ROM area: MVI A,0x42; MOV B,A; HLT
        let mut monitor = vec![0x00; 0x2000];
        monitor[0] = 0x3E;
        monitor[1] = 0x42;
        monitor[2] = 0x47;
        monitor[3] = 0x76;
        let mut m = Machine::new(Model::Pmd853, &monitor, None);
        // With the startup map, ROM is visible at 0x0000.
        m.run_cycles(100);
        assert_eq!(m.cpu.a, 0x42);
        assert_eq!(m.cpu.b, 0x42);
        assert!(m.cpu.halted);
    }
}
