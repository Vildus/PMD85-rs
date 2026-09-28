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
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--model" => {
                model = Model::from_str_loose(&args.next().expect("--model needs a value"))
                    .expect("unknown model");
            }
            "--frames" => frames = args.next().expect("--frames needs a value").parse().unwrap(),
            "--press" => typed = args.next().expect("--press needs a value"),
            other => panic!("unknown argument {other}"),
        }
    }

    let monitor = std::fs::read(format!("{ROM_DIR}{}", model.default_monitor()))
        .expect("cannot read monitor ROM");
    let mut machine = Machine::new(model, &monitor, None);
    for _ in 0..frames {
        machine.step_frame();
    }

    // dump the monitor's system area in hidden VRAM bytes
    println!("ram[0xC100..0xC180] = {:02x?}", &machine.bus.memory.ram[0xC100..0xC180]);
    println!(
        "tune ptr @0xC130 = {:#06x}",
        machine.bus.memory.ram[0xC130] as u16 | (machine.bus.memory.ram[0xC131] as u16) << 8
    );
    let p = (machine.bus.memory.ram[0xC072] as usize)
        | ((machine.bus.memory.ram[0xC073] as usize) << 8);
    println!(
        "line ptr @0xC072 = {p:#06x}, bytes there: {:02x?}",
        &machine.bus.memory.ram[p..p + 24]
    );
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
        if key == Key::Enter {
            let p = (machine.bus.memory.ram[0xC072] as usize)
                | ((machine.bus.memory.ram[0xC073] as usize) << 8);
            println!(
                "  LINE ptr @0xC072 = {p:#06x}, bytes: {:02x?}",
                &machine.bus.memory.ram[p & 0xFFFF..(p & 0xFFFF) + 24]
            );
        }
        machine.bus.keyboard.set_key(key, true);
        trace_frames(&mut machine, 6);
        println!("=== release {ch:?}");
        machine.bus.keyboard.set_key(key, false);
        if key == Key::Enter {
            let mut frame_no = 0;
            let mut hist: std::collections::BTreeMap<u16, usize> = std::collections::BTreeMap::new();
            let mut seen: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
            for _ in 0..200u64 {
                let frame_end = machine.bus.total_cycles() / 40960 + 1;
                while machine.bus.total_cycles() / 40960 < frame_end {
                    let pc = machine.cpu.pc;
                    *hist.entry(pc).or_insert(0) += 1;
                    let tag = if (0xE8A3..=0xE8B7).contains(&pc) {
                        "melody"
                    } else if (0xEBCB..=0xEBD7).contains(&pc) {
                        "delay"
                    } else if (0xE877..=0xE8A2).contains(&pc) {
                        "lookup"
                    } else if (0xEBB0..=0xEBD0).contains(&pc) {
                        "dispatch"
                    } else if (0xE049..=0xE113).contains(&pc) {
                        "e049-e113"
                    } else {
                        ""
                    };
                    if !tag.is_empty() {
                        seen.entry(tag.to_string()).or_insert(frame_no);
                    }
                    machine.step_once();
                }
                frame_no += 1;
            }
            for (tag, f) in &seen {
                println!("  TIMELINE {tag} first seen at frame {f}");
            }
            let mut top: Vec<(u16, usize)> = hist.into_iter().collect();
            top.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            for (pc, n) in top.iter().take(20) {
                println!("  HIST pc={pc:#06x} x{n}");
            }
            println!("  final pc={:#06x} sp={:#06x}", machine.cpu.pc, machine.cpu.sp);
            vram_diff(&mut machine);
            return;
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

/// Trace PCs through an address range (watchpoint-style), dumping registers.
#[allow(dead_code)]
fn watch(machine: &mut Machine, frames: u64, range: (u16, u16)) {
    for _ in 0..frames {
        let frame_end = machine.bus.total_cycles() / 40960 + 1;
        while machine.bus.total_cycles() / 40960 < frame_end {
            let pc = machine.cpu.pc;
            if pc == range.0 {
                println!(
                    "  WATCH pc={pc:#06x} a={:#04x} sp={:#06x} hl={:#06x} de={:#06x} b={:#04x} c={:#04x}",
                    machine.cpu.a, machine.cpu.sp, machine.cpu.hl(), machine.cpu.de(), machine.cpu.b, machine.cpu.c
                );
            } else if pc > range.0 && pc <= range.1 {
                // silent
            }
            machine.step_once();
        }
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
