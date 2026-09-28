//! Memory subsystem: the 64 KiB RAM (of which 0xC000-0xFFFF is Video RAM),
//! the Monitor ROM, per-model address decoding including the boot-time
//! "startup shadow map" that is cleared by the first CPU I/O write, and the
//! PMD 85-3 ROM/VRAM banking at 0xE000-0xFFFF.
//!
//! Map documentation follows MAME's `src/mame/tesla/pmd85.cpp`.

use crate::model::Model;

/// Unmapped reads on the PMD 85 bus float high.
pub const OPEN_BUS: u8 = 0xFF;

pub struct Memory {
    pub model: Model,
    /// Full 64 KiB RAM image. 0xC000-0xFFFF is the Video RAM page.
    pub ram: Box<[u8; 0x10000]>,
    /// Monitor ROM (up to 8 KiB).
    pub rom: Box<[u8; 0x2000]>,
    rom_len: usize,
    /// ROM module (.rmm) contents, if a module is connected.
    pub rom_module: Option<Vec<u8>>,
    /// Boot-time shadow map active (cleared by the first I/O write).
    startup_map: bool,
    /// PMD 85-3: when true, ROM is readable at 0xE000-0xFFFF.
    /// Set from bit 0 of any value written to the system 8255 control
    /// register. Reset state is true.
    pmd853_mapping: bool,
}

impl Memory {
    pub fn new(model: Model, monitor: &[u8]) -> Self {
        let expected = model.monitor_size();
        assert_eq!(
            monitor.len(),
            expected,
            "monitor ROM for {model:?} must be {expected} bytes, got {}",
            monitor.len()
        );
        let mut rom = Box::new([0u8; 0x2000]);
        rom[..monitor.len()].copy_from_slice(monitor);
        Memory {
            model,
            ram: Box::new([0u8; 0x10000]),
            rom,
            rom_len: monitor.len(),
            rom_module: None,
            startup_map: true,
            pmd853_mapping: true,
        }
    }

    pub fn reset(&mut self) {
        self.startup_map = true;
        self.pmd853_mapping = true;
    }

    /// Whether the boot-time shadow map is still active.
    pub fn startup_map(&self) -> bool {
        self.startup_map
    }

    /// PMD 85-3 ROM/VRAM banking control (system 8255 control word bit 0).
    pub fn set_pmd853_mapping(&mut self, rom_visible: bool) {
        self.pmd853_mapping = rom_visible;
    }

    pub fn pmd853_mapping(&self) -> bool {
        self.pmd853_mapping
    }

    /// Notify that the CPU performed an I/O write: this clears the startup
    /// shadow map (MAME behavior) before the write is dispatched.
    pub fn note_io_write(&mut self) {
        self.startup_map = false;
    }

    pub fn read(&self, addr: u16) -> u8 {
        let addr = addr as usize;
        match self.model {
            Model::Pmd851 | Model::Pmd852 => self.read_pmd851(addr),
            Model::Pmd852a => self.read_pmd852a(addr),
            Model::Pmd853 => self.read_pmd853(addr),
        }
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        let addr = addr as usize;
        match self.model {
            Model::Pmd851 | Model::Pmd852 => self.write_pmd851(addr, data),
            Model::Pmd852a => self.write_pmd852a(addr, data),
            Model::Pmd853 => self.write_pmd853(addr, data),
        }
    }

    /// Read from ROM at a linear offset, guarding against short images.
    fn rom_at(&self, offset: usize) -> u8 {
        if offset < self.rom_len {
            self.rom[offset]
        } else {
            OPEN_BUS
        }
    }

    // PMD-85.1 / PMD-85.2 ----------------------------------------------------

    fn read_pmd851(&self, addr: usize) -> u8 {
        if self.startup_map {
            match addr {
                0x0000..=0x0FFF => self.rom_at(addr),
                0x2000..=0x2FFF => self.rom_at(addr - 0x2000),
                // Video RAM mirror #1
                0x4000..=0x7FFF => self.ram[0xC000 + (addr - 0x4000)],
                0x8000..=0x8FFF => self.rom_at(addr - 0x8000),
                0xA000..=0xAFFF => self.rom_at(addr - 0xA000),
                0xC000..=0xFFFF => self.ram[addr],
                // between the ROM mirrors: reads as zero (MAME nop_read)
                0x1000..=0x1FFF | 0x3000..=0x3FFF => 0x00,
                // unmapped holes
                _ => OPEN_BUS,
            }
        } else {
            match addr {
                0x0000..=0x7FFF => self.ram[addr],
                0x8000..=0x8FFF => self.rom_at(addr - 0x8000),
                0xA000..=0xAFFF => self.rom_at(addr - 0xA000),
                0xC000..=0xFFFF => self.ram[addr],
                _ => OPEN_BUS,
            }
        }
    }

    fn write_pmd851(&mut self, addr: usize, data: u8) {
        if self.startup_map {
            match addr {
                // Video RAM mirror #1
                0x4000..=0x7FFF => self.ram[0xC000 + (addr - 0x4000)] = data,
                0xC000..=0xFFFF => self.ram[addr] = data,
                // ROM areas and holes: not writable
                _ => {}
            }
        } else {
            match addr {
                // linear RAM in the low 32K
                0x0000..=0x7FFF => self.ram[addr] = data,
                0xC000..=0xFFFF => self.ram[addr] = data,
                // ROM at 0x8000/0xA000 and unmapped holes
                _ => {}
            }
        }
    }

    // PMD-85.2A --------------------------------------------------------------

    fn read_pmd852a(&self, addr: usize) -> u8 {
        if self.startup_map {
            match addr {
                0x0000..=0x0FFF => self.rom_at(addr),
                // RAM #2 mirror
                0x1000..=0x1FFF => self.ram[0x9000 + (addr - 0x1000)],
                0x2000..=0x2FFF => self.rom_at(addr - 0x2000),
                // RAM #3 mirror
                0x3000..=0x3FFF => self.ram[0xB000 + (addr - 0x3000)],
                // Video RAM mirror #1
                0x4000..=0x7FFF => self.ram[0xC000 + (addr - 0x4000)],
                0x8000..=0x8FFF => self.rom_at(addr - 0x8000),
                // RAM #2
                0x9000..=0x9FFF => self.ram[addr],
                0xA000..=0xAFFF => self.rom_at(addr - 0xA000),
                // RAM #3
                0xB000..=0xBFFF => self.ram[addr],
                0xC000..=0xFFFF => self.ram[addr],
                _ => OPEN_BUS,
            }
        } else {
            match addr {
                0x0000..=0x2FFF => self.ram[addr],
                // hardware quirk: this range selects the ram+0x5000 rows
                0x3000..=0x3FFF => self.ram[0x5000 + (addr - 0x3000)],
                0x4000..=0x7FFF => self.ram[addr],
                0x8000..=0x8FFF => self.rom_at(addr - 0x8000),
                0xA000..=0xAFFF => self.rom_at(addr - 0xA000),
                0x9000..=0x9FFF | 0xB000..=0xFFFF => self.ram[addr],
                _ => OPEN_BUS,
            }
        }
    }

    fn write_pmd852a(&mut self, addr: usize, data: u8) {
        if self.startup_map {
            match addr {
                // RAM #2 mirror
                0x1000..=0x1FFF => self.ram[0x9000 + (addr - 0x1000)] = data,
                // RAM #3 mirror
                0x3000..=0x3FFF => self.ram[0xB000 + (addr - 0x3000)] = data,
                // Video RAM mirror #1
                0x4000..=0x7FFF => self.ram[0xC000 + (addr - 0x4000)] = data,
                // RAM #2 / RAM #3 / Video RAM
                0x9000..=0x9FFF | 0xB000..=0xBFFF | 0xC000..=0xFFFF => {
                    self.ram[addr] = data
                }
                // ROM areas and unmapped holes are not writable
                _ => {}
            }
        } else {
            match addr {
                0x0000..=0x2FFF => self.ram[addr] = data,
                // hardware quirk: this range selects the ram+0x5000 rows
                0x3000..=0x3FFF => self.ram[0x5000 + (addr - 0x3000)] = data,
                0x4000..=0x7FFF => self.ram[addr] = data,
                0x8000..=0x8FFF | 0xA000..=0xAFFF => {}
                _ => self.ram[addr] = data,
            }
        }
    }

    // PMD-85.3 ----------------------------------------------------------------

    fn read_pmd853(&self, addr: usize) -> u8 {
        if self.startup_map {
            // 8 KiB ROM mirrored across the whole address space
            self.rom_at(addr & 0x1FFF)
        } else {
            match addr {
                0xE000..=0xFFFF if self.pmd853_mapping => self.rom_at(addr - 0xE000),
                _ => self.ram[addr],
            }
        }
    }

    fn write_pmd853(&mut self, addr: usize, data: u8) {
        // Writes always go to RAM (the ROM/VRAM split is read-only).
        self.ram[addr] = data;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom4k() -> Vec<u8> {
        let mut v = vec![0u8; 0x1000];
        for (i, b) in v.iter_mut().enumerate() {
            *b = (i & 0xFF) as u8;
        }
        v
    }

    fn rom8k() -> Vec<u8> {
        let mut v = vec![0u8; 0x2000];
        for (i, b) in v.iter_mut().enumerate() {
            *b = ((i + 7) & 0xFF) as u8;
        }
        v
    }

    #[test]
    fn pmd851_startup_map() {
        let m = Memory::new(Model::Pmd851, &rom4k());
        assert_eq!(m.read(0x0000), 0x00);
        assert_eq!(m.read(0x0002), 0x02);
        // ROM mirror at 0x2000
        assert_eq!(m.read(0x2000), m.read(0x0000));
        // between the ROM mirrors: reads as zero
        assert_eq!(m.read(0x1000), 0x00);
        assert_eq!(m.read(0x3000), 0x00);
        // VRAM mirror at 0x4000
        assert_eq!(m.read(0x4000), m.read(0xC000));
        // ROM at 0x8000 and mirror at 0xA000
        assert_eq!(m.read(0x8000), 0x00);
        assert_eq!(m.read(0xA000), m.read(0x8000));
    }

    #[test]
    fn pmd851_switch_to_normal_map() {
        let mut m = Memory::new(Model::Pmd851, &rom4k());
        m.note_io_write();
        // low 32K becomes RAM
        m.write(0x0000, 0x55);
        assert_eq!(m.read(0x0000), 0x55);
        assert_eq!(m.ram[0], 0x55);
        // ROM still readable at 0x8000, and write-protected
        assert_eq!(m.read(0x8000), 0x00);
        m.write(0x8000, 0xAA);
        assert_eq!(m.read(0x8000), 0x00);
        // VRAM still at 0xC000
        m.write(0xC000, 0x11);
        assert_eq!(m.ram[0xC000], 0x11);
        // 0x9000/0xB000 unmapped
        assert_eq!(m.read(0x9000), OPEN_BUS);
        assert_eq!(m.read(0xB000), OPEN_BUS);
    }

    #[test]
    fn pmd851_startup_writes_go_to_vram_mirror() {
        let mut m = Memory::new(Model::Pmd851, &rom4k());
        m.write(0x4000, 0x77); // VRAM mirror
        assert_eq!(m.ram[0xC000], 0x77);
        // low memory not writable while ROM mirrored there
        m.write(0x0000, 0x99);
        assert_eq!(m.read(0x0000), 0x00);
    }

    #[test]
    fn pmd851_normal_map_linear_ram() {
        let mut m = Memory::new(Model::Pmd851, &rom4k());
        m.note_io_write();
        // after the startup map is dropped, 0x4000-0x7FFF is plain RAM,
        // not the Video RAM mirror (SP is typically set to 0x8000, so the
        // monitor's stack lives exactly here)
        m.write(0x7FFF, 0x55);
        assert_eq!(m.ram[0x7FFF], 0x55);
        assert_eq!(m.ram[0xFFFF], 0x00);
        assert_eq!(m.read(0x7FFF), 0x55);
        m.write(0x4000, 0x66);
        assert_eq!(m.ram[0x4000], 0x66);
        assert_eq!(m.ram[0xC000], 0x00);
        // writes to ROM areas are still dropped
        m.write(0x8000, 0xAA);
        assert_eq!(m.read(0x8000), 0x00);
    }

    #[test]
    fn pmd852a_normal_map_quirk_3000() {
        let mut m = Memory::new(Model::Pmd852a, &rom4k());
        m.note_io_write();
        // 0x3000-0x3FFF selects the ram+0x5000 rows (address decoding quirk)
        m.write(0x3000, 0x42);
        assert_eq!(m.ram[0x5000], 0x42);
        assert_eq!(m.read(0x3000), 0x42);
        assert_eq!(m.ram[0x3000], 0x00);
        // the rest of the low 32K is linear
        m.write(0x2500, 0x43);
        assert_eq!(m.ram[0x2500], 0x43);
    }

    #[test]
    fn pmd852a_startup_ram_mirrors() {
        let mut m = Memory::new(Model::Pmd852a, &rom4k());
        m.write(0x1000, 0x21); // RAM #2 mirror
        assert_eq!(m.ram[0x9000], 0x21);
        m.write(0x3000, 0x22); // RAM #3 mirror
        assert_eq!(m.ram[0xB000], 0x22);
        assert_eq!(m.read(0x9000), 0x21);
        assert_eq!(m.read(0xB000), 0x22);
        m.note_io_write();
        // normal map: linear RAM up to 0x7FFF
        m.write(0x0000, 0x33);
        assert_eq!(m.read(0x0000), 0x33);
        // RAM #2 and #3 still accessible at their normal addresses
        assert_eq!(m.read(0x9000), 0x21);
        assert_eq!(m.read(0xB000), 0x22);
    }

    #[test]
    fn pmd853_startup_map() {
        let m = Memory::new(Model::Pmd853, &rom8k());
        // whole address space mirrors the 8K ROM
        assert_eq!(m.read(0x0000), m.rom[0]);
        assert_eq!(m.read(0x0FFF), m.rom[0x0FFF]);
        assert_eq!(m.read(0x2000), m.rom[0]);
        assert_eq!(m.read(0x8000), m.rom[0]);
        assert_eq!(m.read(0xFFFF), m.rom[0x1FFF]);
    }

    #[test]
    fn pmd853_writes_under_startup_map() {
        let mut m = Memory::new(Model::Pmd853, &rom8k());
        m.write(0x0123, 0x5A);
        m.write(0xE456, 0x5B);
        assert_eq!(m.ram[0x0123], 0x5A);
        assert_eq!(m.ram[0xE456], 0x5B);
        // reads still show ROM
        assert_eq!(m.read(0x0123), m.rom[0x0123]);
    }

    #[test]
    fn pmd853_banking() {
        let mut m = Memory::new(Model::Pmd853, &rom8k());
        m.note_io_write();
        // after boot switch, ROM visible at 0xE000 (mapping = true)
        assert_eq!(m.read(0xE000), m.rom[0]);
        // writes always land in RAM underneath
        m.write(0xE000, 0x66);
        assert_eq!(m.ram[0xE000], 0x66);
        assert_eq!(m.read(0xE000), m.rom[0]);
        // AllRAM mode: control word bit 0 = 0
        m.set_pmd853_mapping(false);
        assert_eq!(m.read(0xE000), 0x66);
        assert_eq!(m.read(0xC000), m.ram[0xC000]);
        // and back
        m.set_pmd853_mapping(true);
        assert_eq!(m.read(0xE000), m.rom[0]);
    }

    #[test]
    fn reset_restores_startup_map() {
        let mut m = Memory::new(Model::Pmd853, &rom8k());
        m.note_io_write();
        m.set_pmd853_mapping(false);
        m.reset();
        assert!(m.startup_map());
        assert!(m.pmd853_mapping());
        assert_eq!(m.read(0x0000), m.rom[0]);
    }
}
