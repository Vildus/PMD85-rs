//! Traces CPU IN/OUT instructions while typing on the emulated keyboard:
//! boots the machine, then logs every I/O access (PC, port, value) around
//! key press/release events. Debugging aid for the Monitor ROM behavior.

use pmd85_core::keyboard::Key;
use pmd85_core::machine::Machine;
use pmd85_core::model::Model;

const ROM_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Rom/");

fn main() {
    let mut model = Model::Pmd853;
    let mut frames = 400u64;
    let mut typed = String::new();
    let mut rom_module: Option<String> = None;
    let mut trace_module_ports = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--model" => {
                model = Model::from_str_loose(&args.next().expect("--model needs a value"))
                    .expect("unknown model");
            }
            "--frames" => frames = args.next().expect("--frames needs a value").parse().unwrap(),
            "--press" => typed = args.next().expect("--press needs a value"),
            "--rom-module" => rom_module = Some(args.next().expect("--rom-module needs a value")),
            "--trace-module-ports" => trace_module_ports = true,
            other => panic!("unknown argument {other}"),
        }
    }

    let monitor = std::fs::read(format!("{ROM_DIR}{}", model.default_monitor()))
        .expect("cannot read monitor ROM");
    let rom_module = rom_module.map(|p| {
        let path = if p.contains('/') {
            p
        } else {
            format!("{ROM_DIR}{p}")
        };
        std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read ROM module {path:?}: {e}"))
    });
    let mut machine = Machine::new(model, &monitor, rom_module);
    for _ in 0..frames {
        if trace_module_ports {
            // log all I/O touching the ROM module connector (0x88-0x8B)
            let frame_end = machine.bus.total_cycles() / 40960 + 1;
            while machine.bus.total_cycles() / 40960 < frame_end {
                let pc = machine.cpu.pc;
                let opcode = machine.bus.memory.read(pc);
                if opcode == 0xDB || opcode == 0xD3 {
                    let port = machine.bus.memory.read(pc.wrapping_add(1));
                    machine.step_once();
                    if port & 0x0C == 0x08 {
                        println!(
                            "  boot pc={pc:#06x} {} port={port:#04x} a={:#04x}",
                            if opcode == 0xDB { "IN " } else { "OUT" },
                            machine.cpu.a
                        );
                    }
                } else {
                    machine.step_once();
                }
            }
        } else {
            machine.step_frame();
        }
    }

    // dump the monitor's system area in hidden VRAM bytes
    println!("ram[0xC100..0xC180] = {:02x?}", &machine.bus.memory.ram[0xC100..0xC180]);
    println!(
        "tune ptr @0xC130 = {:#06x}",
        machine.bus.memory.ram[0xC130] as u16 | (machine.bus.memory.ram[0xC131] as u16) << 8
    );
    let p = (machine.bus.memory.ram[0xC072] as usize)
        | ((machine.bus.memory.ram[0xC073] as usize) << 8);
    if p + 24 <= 0x10000 {
        println!(
            "line ptr @0xC072 = {p:#06x}, bytes there: {:02x?}",
            &machine.bus.memory.ram[p..p + 24]
        );
    } else {
        println!("line ptr @0xC072 = {p:#06x} (out of range)");
    }
    println!(
        "ram[0xC1B0..0xC1C0] = {:02x?}",
        &machine.bus.memory.ram[0xC1B0..0xC1C0]
    );

    for ch in typed.chars() {
        let key = match char_to_key(ch) {
            Some(k) => k,
            None => {
                eprintln!("skipping unmapped char {ch:?}");
                continue;
            }
        };
        println!("=== press {ch:?} -> key {key:?}");
        machine.bus.keyboard.set_key(key, true);
        trace_frames(&mut machine, 8);
        println!("=== release {ch:?}");
        machine.bus.keyboard.set_key(key, false);
        if key == Key::Enter {
            // Collapsed PC-path trace for a while after Enter: shows where the
            // typed line gets dispatched. Run-length encoded, capped output.
            let mut path: Vec<(u16, u64)> = Vec::new();
            for _ in 0..120u64 {
                let frame_end = machine.bus.total_cycles() / 40960 + 1;
                while machine.bus.total_cycles() / 40960 < frame_end {
                    let pc = machine.cpu.pc;
                    match path.last_mut() {
                        Some((p, n)) if *p == pc => *n += 1,
                        _ => path.push((pc, 1)),
                    }
                    machine.step_once();
                }
            }
            for (i, (pc, n)) in path.iter().enumerate() {
                if i >= 200 {
                    println!("  PATH ... {} entries total", path.len());
                    break;
                }
                println!("  PATH pc={pc:#06x} x{n}");
            }
            println!(
                "  after-Enter pc={:#06x} sp={:#06x}",
                machine.cpu.pc, machine.cpu.sp
            );
            println!(
                "  C078 buffer: {:02x?}",
                &machine.bus.memory.ram[0xC078..0xC0A0]
            );
        }
        trace_frames(&mut machine, 6);
        vram_diff(&mut machine);
    }
}

/// Print which VRAM bytes are currently non-zero (compact), to watch the
/// monitor draw characters.
fn vram_diff(machine: &mut Machine) {
    let mut regions: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut addr = 0xC000;
    while addr < 0x10000 {
        let v = machine.bus.memory.ram[addr];
        if v != 0 {
            let start = regions.len();
            regions.push((addr, Vec::new()));
            while addr < 0x10000 && machine.bus.memory.ram[addr] != 0 {
                let byte = machine.bus.memory.ram[addr];
                regions[start].1.push(byte);
                addr += 1;
            }
        } else {
            addr += 1;
        }
    }
    for (start, bytes) in regions.iter() {
        if *start >= 0xE000 {
            println!(
                "  {start:#06x}: {:02x?}{}",
                bytes,
                if bytes.len() > 8 { "..." } else { "" }
            );
        }
    }
}

fn trace_frames(machine: &mut Machine, frames: u64) {
    let mut histogram: std::collections::BTreeMap<u16, usize> = std::collections::BTreeMap::new();
    for _ in 0..frames {
        let frame_end = machine.bus.total_cycles() / 40960 + 1;
        while machine.bus.total_cycles() / 40960 < frame_end {
            let pc = machine.cpu.pc;
            let opcode = machine.bus.memory.read(pc);
            if opcode == 0xDB || opcode == 0xD3 {
                let port = machine.bus.memory.read(pc.wrapping_add(1));
                machine.step_once();
                println!(
                    "  pc={pc:#06x} {} port={port:#04x} a={:#04x} scan_pa={:#04x}",
                    if opcode == 0xDB { "IN " } else { "OUT" },
                    machine.cpu.a,
                    machine.bus.ppi_system_scan()
                );
            } else {
                machine.step_once();
                *histogram.entry(pc).or_insert(0) += 1;
            }
        }
    }
    let mut top: Vec<(u16, usize)> = histogram.into_iter().collect();
    top.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (pc, n) in top.iter().take(60) {
        println!("  HIST pc={pc:#06x} x{n}");
    }
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
        _ => return None,
    })
}
