//! The emulated machine: CPU, memory, peripheral chips and the I/O port
//! dispatch, tied together.
//!
//! [`MachineBus`] implements [`cpu::Bus`] and holds everything except the
//! CPU; [`Machine`] pairs the two and exposes a frame-based stepping API
//! for the presentation layer.

use crate::bus::Memory;
use crate::chips::ppi8255::{I8255, Port};
use crate::chips::pit8253::I8253;
use crate::chips::usart8251::I8251;
use crate::cpu::{Bus, Cpu};
use crate::keyboard::Keyboard;
use crate::model::Model;

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
    /// Speaker level (PC2 of the system 8255).
    speaker_level: bool,
    /// LED driven from PC3 of the system 8255.
    pub led: bool,
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
            speaker_level: false,
            led: false,
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
        self.speaker_level = false;
        self.led = false;
    }

    pub fn speaker_level(&self) -> bool {
        self.speaker_level
    }

    /// Current keyboard scan column (system 8255 port A output).
    pub fn ppi_system_scan(&self) -> u8 {
        self.ppi_system.outputs[0]
    }

    /// Emulated CPU cycles executed since power-on.
    pub fn total_cycles(&self) -> u64 {
        self.total_cycles
    }

    /// Advance peripheral clocks by `cycles` CPU cycles.
    fn advance_time(&mut self, cycles: u64) {
        self.total_cycles += cycles;
        // 8253 counter 1 is clocked by the ~2 MHz system clock, counter 2
        // by a 1 Hz generator.
        let seconds_before = (self.total_cycles - cycles) / CPU_CLOCK_HZ;
        for _ in 0..cycles {
            self.pit.tick(1, true);
        }
        if self.total_cycles / CPU_CLOCK_HZ != seconds_before {
            self.pit.tick(2, true);
        }
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
            0x0C => {
                if port & 0x80 == 0 {
                    // I/O board interfaces
                    match port & 0x70 {
                        0x10 => return self.uart.read(port as u32 & 1),
                        0x40 => return self.ppi_gpio.read(Port::from_index(reg), 0xFF, 0xFF, 0xFF),
                        0x50 => return self.pit.read(port as u32 & 3),
                        0x70 => return self.ppi_ims2.read(Port::from_index(reg), 0xFF, 0xFF, 0xFF),
                        _ => {}
                    }
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
                    self.speaker_level = self.ppi_system.outputs[2] & 0x04 != 0;
                    self.led = self.ppi_system.outputs[2] & 0x08 != 0;
                }
            }
            0x08 => {
                // ROM module connector
                if port & 0x80 != 0 && self.rom_module_present() {
                    self.ppi_rom.write(Port::from_index(reg), data);
                }
            }
            0x0C => {
                if port & 0x80 == 0 {
                    // I/O board interfaces
                    match port & 0x70 {
                        0x10 => self.uart.write(port as u32 & 1, data),
                        0x40 => self.ppi_gpio.write(Port::from_index(reg), data),
                        0x50 => self.pit.write(port as u32 & 3, data),
                        0x70 => self.ppi_ims2.write(Port::from_index(reg), data),
                        _ => {}
                    }
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
}

impl Machine {
    pub fn new(model: Model, monitor: &[u8], rom_module: Option<Vec<u8>>) -> Self {
        Machine {
            cpu: Cpu::new(),
            bus: MachineBus::new(model, monitor, rom_module),
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

    /// Run one video frame worth of emulation (~20 ms).
    pub fn step_frame(&mut self) {
        self.run_cycles(CYCLES_PER_FRAME);
    }

    /// Run a number of CPU cycles (used by tests and future debugger).
    pub fn run_cycles(&mut self, cycles: u64) {
        let target = self.bus.total_cycles() + cycles;
        while self.bus.total_cycles() < target {
            let c = self.cpu.step(&mut self.bus) as u64;
            self.bus.advance_time(c);
        }
    }

    /// Execute exactly one CPU instruction (or interrupt), advancing
    /// peripheral clocks. Returns the cycles consumed.
    pub fn step_once(&mut self) -> u32 {
        let c = self.cpu.step(&mut self.bus) as u64;
        self.bus.advance_time(c);
        c as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard::Key;

    fn rom(size: usize) -> Vec<u8> {
        vec![0x00; size]
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
