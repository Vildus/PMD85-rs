//! Boots a PMD 85 model with a real Monitor ROM, runs some frames and
//! writes the screen as a PNG. Used for visual verification and debugging.
//!
//! Usage: cargo run -p pmd85-core --example boot_dump -- [--model 3]
//!        [--frames N] [--press ABC] [--out out.png]

use pmd85_core::keyboard::Key;
use pmd85_core::machine::Machine;
use pmd85_core::model::Model;
use pmd85_core::vram::{self, ColorProfile};

use std::io::Write;

const ROM_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Rom/");

fn main() {
    let mut model = Model::Pmd853;
    let mut frames = 150u64;
    let mut typed = String::new();
    let mut out = String::from("boot_dump.png");
    let mut settle = 100u64;
    let mut rom_module: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--model" => {
                model = Model::from_str_loose(&args.next().expect("--model needs a value"))
                    .expect("unknown model");
            }
            "--frames" => frames = args.next().expect("--frames needs a value").parse().unwrap(),
            "--press" => typed = args.next().expect("--press needs a value"),
            "--settle" => settle = args.next().expect("--settle needs a value").parse().unwrap(),
            "--out" => out = args.next().expect("--out needs a value"),
            "--rom-module" => rom_module = Some(args.next().expect("--rom-module needs a value")),
            other => panic!("unknown argument {other}"),
        }
    }

    let monitor = std::fs::read(format!("{ROM_DIR}{}", model.default_monitor()))
        .expect("cannot read monitor ROM");
    let rom_module = rom_module.map(|p| {
        let data = if p.contains('/') {
            std::fs::read(&p).unwrap_or_else(|e| panic!("cannot read ROM module {p:?}: {e}"))
        } else {
            std::fs::read(format!("{ROM_DIR}{p}"))
                .unwrap_or_else(|e| panic!("cannot read ROM module {p:?}: {e}"))
        };
        data
    });
    let mut machine = Machine::new(model, &monitor, rom_module);

    for _ in 0..frames {
        machine.step_frame();
    }

    // Debug: sample the keyboard scan activity before typing.
    {
        let mut seen_pa = std::collections::BTreeSet::new();
        let mut pcs = std::collections::BTreeSet::new();
        for _ in 0..5 {
            machine.step_frame();
            seen_pa.insert(machine.bus.ppi_system_scan());
            pcs.insert(machine.cpu.pc >> 4);
        }
        eprintln!("scan columns seen: {:?}", seen_pa);
        eprintln!("pc>>4 seen: {:?}", pcs);
    }

    // Type some keys, held long enough for the monitor scan loop, then let
    // the monitor catch up. Supports [NAME] tokens for special keys, e.g.
    // [SHIFT][K7] or [ENTER].
    let settle = settle;
    let mut tokens: Vec<String> = Vec::new();
    let mut chars = typed.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '[' {
            let mut name = String::new();
            for c2 in chars.by_ref() {
                if c2 == ']' {
                    break;
                }
                name.push(c2);
            }
            tokens.push(name);
        } else {
            tokens.push(c.to_string());
        }
    }
    // `[SHIFT]` is sticky: it stays held for following keys until `[UNSHIFT]`.
    let mut shift_held = false;
    for tok in tokens {
        if tok == "SHIFT" {
            shift_held = true;
            continue;
        }
        if tok == "UNSHIFT" {
            shift_held = false;
            continue;
        }
        if let Some(n) = tok.strip_prefix("WAIT:") {
            // [WAIT:n] - run n frames without touching the keyboard.
            // Used after commands that start a program (e.g. `BASIC G`)
            // so it has time to boot before further keys are typed.
            let n: u32 = n.parse().expect("[WAIT:n] needs a number");
            for _ in 0..n {
                machine.step_frame();
            }
            continue;
        }
        let key = match token_to_key(&tok) {
            Some(k) => k,
            None => {
                eprintln!("skipping unmapped token {tok:?}");
                continue;
            }
        };
        if shift_held {
            machine.bus.keyboard.set_key(Key::Shift, true);
        }
        machine.bus.keyboard.set_key(key, true);
        for _ in 0..8 {
            machine.step_frame();
        }
        machine.bus.keyboard.set_key(key, false);
        machine.bus.keyboard.set_key(Key::Shift, false);
        for _ in 0..8 {
            machine.step_frame();
        }
    }
    // let the monitor settle (command output, blinking cursor)
    for _ in 0..settle {
        machine.step_frame();
    }

    let frame = vram::decode(&machine.bus.memory, ColorProfile::Rgb);
    write_png(&out, vram::WIDTH, vram::HEIGHT, &frame).expect("cannot write PNG");
    eprintln!(
        "wrote {} ({}x{}), model {:?}, cycles={}, pc={:#06x}, halted={}",
        out,
        vram::WIDTH,
        vram::HEIGHT,
        model,
        machine.bus.total_cycles(),
        machine.cpu.pc,
        machine.cpu.halted
    );

    // Coarse ASCII view: one char per VRAM byte (6-pixel group).
    for y in (0..vram::HEIGHT).step_by(2) {
        let mut line = String::new();
        for byte in 0..48 {
            let v = machine.bus.memory.ram[0xC000 + y * 0x40 + byte];
            let ch = match (v & 0x3F, v >> 6) {
                (0, _) => ' ',
                (0x3F, 1) => '#',
                (0x3F, 2) => '+',
                (0x3F, 3) => '*',
                (_, 1) => 'o',
                (_, 2) => '.',
                (_, 3) => ',',
                _ => '?',
            };
            line.push(ch);
        }
        println!("{y:3}: {line}");
    }
}

fn token_to_key(tok: &str) -> Option<Key> {
    use Key::*;
    if tok.chars().count() == 1 {
        return char_to_key(tok.chars().next().unwrap());
    }
    Some(match tok.to_ascii_uppercase().as_str() {
        "SHIFT" => Shift,
        "STOP" => Stop,
        "ENTER" | "EOL" => Enter,
        "TAB" => Tab,
        "SPACE" => Space,
        "BACKSPACE" => Backspace,
        "DEL" => Delete,
        "INS" => Insert,
        "CLR" => ClrScr,
        "RCL" => Recall,
        "UP" => CursorUp,
        "DOWN" => CursorDown,
        "LEFT" => CursorLeft,
        "RIGHT" => CursorRight,
        "K0" => K0,
        "K1" => K1,
        "K2" => K2,
        "K3" => K3,
        "K4" => K4,
        "K5" => K5,
        "K6" => K6,
        "K7" => K7,
        "K8" => K8,
        "K9" => K9,
        "K10" => K10,
        "K11" => K11,
        _ => return None,
    })
}

fn char_to_key(c: char) -> Option<Key> {
    use Key::*;
    Some(match c.to_ascii_uppercase() {
        'A' => A, 'B' => B, 'C' => C, 'D' => D, 'E' => E, 'F' => F, 'G' => G,
        'H' => H, 'I' => I, 'J' => J, 'K' => K, 'L' => L, 'M' => M, 'N' => N,
        'O' => O, 'P' => P, 'Q' => Q, 'R' => R, 'S' => S, 'T' => T, 'U' => U,
        'V' => V, 'W' => W, 'X' => X, 'Y' => Y, 'Z' => Z,
        '0' => Digit0, '1' => Digit1, '2' => Digit2, '3' => Digit3, '4' => Digit4,
        '5' => Digit5, '6' => Digit6, '7' => Digit7, '8' => Digit8, '9' => Digit9,
        ' ' => Space,
        '\n' | '\r' => Enter,
        '+' => Semicolon,   // shifted ; is + on the PMD layout... unshifted below
        '-' => Minus,
        '.' => Period,
        ',' => Comma,
        ';' => Semicolon,
        _ => return None,
    })
}

/// Minimal PNG writer (RGBA, stored/uncompressed deflate, single filter byte 0
/// per row). Just enough for debug dumps and test fixtures.
pub fn write_png(path: &str, width: usize, height: usize, rgba: &[u8]) -> std::io::Result<()> {
    let mut raw = Vec::with_capacity(height * (1 + width * 4));
    for y in 0..height {
        raw.push(0); // filter: none
        let start = y * width * 4;
        raw.extend_from_slice(&rgba[start..start + width * 4]);
    }

    // zlib stream with stored (uncompressed) deflate blocks
    let mut z = Vec::new();
    z.push(0x78);
    z.push(0x01);
    let mut chunks = raw.chunks(65535).peekable();
    if raw.is_empty() {
        z.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    }
    while let Some(chunk) = chunks.next() {
        let last = chunks.peek().is_none();
        z.push(if last { 1 } else { 0 });
        let len = chunk.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(chunk);
    }
    let adler = adler32(&raw);
    z.extend_from_slice(&adler.to_be_bytes());

    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    png.extend_from_slice(&chunk(b"IHDR", &{
        let mut d = Vec::new();
        d.extend_from_slice(&(width as u32).to_be_bytes());
        d.extend_from_slice(&(height as u32).to_be_bytes());
        d.push(8); // bit depth
        d.push(6); // color type RGBA
        d.push(0); // compression
        d.push(0); // filter
        d.push(0); // interlace
        d
    }));
    png.extend_from_slice(&chunk(b"IDAT", &z));
    png.extend_from_slice(&chunk(b"IEND", &[]));

    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(&png)?;
    f.flush()?;
    Ok(())
}

fn chunk(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + data.len());
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(data);
    let mut crc = crc32(tag);
    crc = crc32_update(crc, data);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    crc32_update(0xFFFF_FFFF, data) ^ 0xFFFF_FFFF
}

fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    crc
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}
