//! Video RAM decoding: 288x256 pixels from RAM at 0xC000-0xFFFF.
//!
//! Layout (see https://pmd85.borik.net/wiki/VideoRAM):
//! - 256 microrows, each 64 bytes; bytes 0-47 are visible, 16 are scratch.
//! - per byte: bits 0-5 are six pixels (bit 0 leftmost), bits 6-7 are the
//!   color attribute of that six-pixel group.

use crate::bus::Memory;

/// Screen geometry.
pub const WIDTH: usize = 288;
pub const HEIGHT: usize = 256;
/// VRAM base address.
pub const VRAM_BASE: usize = 0xC000;

/// Color profiles for the attribute bits (PMD 85-3 style).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorProfile {
    /// Four colors as seen on the RGB output of a PMD 85-3:
    /// green, red, blue, violet on black.
    #[default]
    Rgb,
    /// Green monochrome (TV output, normal brightness).
    Mono,
}

impl ColorProfile {
    fn palette(self) -> [[u8; 4]; 4] {
        match self {
            ColorProfile::Rgb => [
                [0x00, 0xE0, 0x30, 0xFF], // green
                [0xE0, 0x30, 0x20, 0xFF], // red
                [0x30, 0x40, 0xF0, 0xFF], // blue
                [0xD0, 0x60, 0xF0, 0xFF], // violet
            ],
            ColorProfile::Mono => [
                [0x00, 0xD0, 0x00, 0xFF],
                [0x00, 0xD0, 0x00, 0xFF],
                [0x00, 0x80, 0x00, 0xFF],
                [0x00, 0x80, 0x00, 0xFF],
            ],
        }
    }
}

/// Decode the Video RAM into an RGBA framebuffer (WIDTH*HEIGHT*4 bytes).
pub fn decode(mem: &Memory, profile: ColorProfile) -> Vec<u8> {
    let mut buf = Vec::with_capacity(WIDTH * HEIGHT * 4);
    decode_into(mem, profile, &mut buf);
    buf
}

/// Decode into a reusable buffer; returns the buffer for convenience.
pub fn decode_into<'a>(mem: &Memory, profile: ColorProfile, buf: &'a mut Vec<u8>) -> &'a mut Vec<u8> {
    buf.clear();
    buf.reserve(WIDTH * HEIGHT * 4);
    let palette = profile.palette();
    for y in 0..HEIGHT {
        let row = VRAM_BASE + y * 0x40;
        for byte in 0..48 {
            let v = mem.ram[row + byte];
            let color = palette[(v >> 6) as usize & 3];
            // bit 0 is the leftmost of the six pixels (MAME screen_update)
            for bit in 0..6 {
                if v & (1 << bit) != 0 {
                    buf.extend_from_slice(&color);
                } else {
                    buf.extend_from_slice(&[0x00, 0x00, 0x00, 0xFF]);
                }
            }
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;

    fn mem_with_vram() -> Memory {
        let mut m = Memory::new(Model::Pmd853, &vec![0; 0x2000]);
        // row 0, byte 0: single leftmost pixel, green (attr 00)
        m.ram[VRAM_BASE] = 0b0000_0001;
        // row 0, byte 1: all six pixels, red (attr 01)
        m.ram[VRAM_BASE + 1] = 0b0100_0000 | 0b0011_1111;
        // row 255, byte 47: rightmost pixel, violet (attr 11)
        m.ram[VRAM_BASE + 255 * 0x40 + 47] = 0b1100_0000 | 0b0010_0000;
        // scratch byte: must not be displayed
        m.ram[VRAM_BASE + 48] = 0xFF;
        m
    }

    fn px(buf: &[u8], x: usize, y: usize) -> [u8; 4] {
        let i = (y * WIDTH + x) * 4;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    #[test]
    fn framebuffer_size() {
        let m = mem_with_vram();
        let buf = decode(&m, ColorProfile::Rgb);
        assert_eq!(buf.len(), WIDTH * HEIGHT * 4);
    }

    #[test]
    fn pixel_order_and_colors() {
        let m = mem_with_vram();
        let buf = decode(&m, ColorProfile::Rgb);
        // bit 0 = leftmost pixel of the byte
        assert_eq!(px(&buf, 0, 0), [0x00, 0xE0, 0x30, 0xFF]); // green on
        assert_eq!(px(&buf, 1, 0), [0, 0, 0, 0xFF]); // off
        // byte 1: full red group
        for x in 6..12 {
            assert_eq!(px(&buf, x, 0), [0xE0, 0x30, 0x20, 0xFF], "x={x}");
        }
        // bit 5 = rightmost pixel
        assert_eq!(px(&buf, 6 * 47 + 5, 255), [0xD0, 0x60, 0xF0, 0xFF]);
        assert_eq!(px(&buf, 6 * 47 + 4, 255), [0, 0, 0, 0xFF]);
        // scratch bytes never displayed: row 0 stops at x=287
        assert_eq!(px(&buf, 287, 0), [0, 0, 0, 0xFF]);
    }

    #[test]
    fn microrow_addressing() {
        // 0xC030 is a scratch address; row 1 starts at 0xC040
        let mut m = mem_with_vram();
        m.ram[VRAM_BASE + 0x40] = 0b1100_0000 | 0b0000_0001; // attr 11, pixel 0
        let buf = decode(&m, ColorProfile::Rgb);
        assert_eq!(px(&buf, 0, 1), [0xD0, 0x60, 0xF0, 0xFF]);
    }
}
