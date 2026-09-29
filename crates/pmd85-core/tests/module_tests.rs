//! ROM module tests: the monitor BIOS must detect an attached ROM module
//! through the module 8255 (ports 0xF8-0xFB), copy its payload into RAM
//! and run it, all by itself. The emulator only presents module bytes on
//! port reads - detection, transfer and boot are the monitor's own code,
//! exactly like on real hardware.
//!
//! Verified per model:
//! - 85-3 + basic3.rmm, 85-2 + basic2.rmm, 85-2A + basic2A.rmm: the
//!   monitor auto-detects the module at boot, copies it to RAM 0x0000
//!   (skipping the 12-byte .rmm header), and BASIC runs from there.
//! - 85-1 + basic1.rmm (headerless): the monitor waits for the explicit
//!   `BASIC G` command, whose TRANSFER routine copies the module to
//!   RAM 0x0000 before jumping to it (RST 0).
//!
//! The "module code actually executes" proof is typing `PRINT 123` and
//! finding the echoed command and printed result on the console rows;
//! while waiting for input BASIC parks inside the monitor's key-scan
//! loop, so the PC alone does not show module execution.

use pmd85_core::keyboard::Key;
use pmd85_core::machine::Machine;
use pmd85_core::model::Model;
use pmd85_core::vram::{self, ColorProfile, WIDTH};

const ROM_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Rom/");

/// Boot `model` with (optionally) a ROM module attached, for `frames`
/// frames.
fn boot(model: Model, module: Option<Vec<u8>>, frames: u64) -> Machine {
    let monitor = std::fs::read(format!("{ROM_DIR}{}", model.default_monitor()))
        .expect("cannot read monitor ROM");
    let mut machine = Machine::new(model, &monitor, module);
    for _ in 0..frames {
        machine.step_frame();
    }
    machine
}

/// Press keys the way boot_dump does: long enough for the monitor scan.
fn type_str(machine: &mut Machine, s: &str) {
    for ch in s.chars() {
        let key = match char_to_key(ch) {
            Some(k) => k,
            None => panic!("no key mapping for {ch:?}"),
        };
        machine.bus.keyboard.set_key(key, true);
        for _ in 0..8 {
            machine.step_frame();
        }
        machine.bus.keyboard.set_key(key, false);
        for _ in 0..8 {
            machine.step_frame();
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

/// Count lit pixel rows inside a horizontal band of the screen.
fn lit_rows(machine: &Machine, band: std::ops::Range<usize>) -> usize {
    let mut buf = Vec::new();
    let fb = vram::decode_into(&machine.bus.memory, ColorProfile::Rgb, &mut buf);
    band.filter(|&y| {
        (y * WIDTH * 4..(y + 1) * WIDTH * 4)
            .step_by(4)
            .any(|i| fb[i] > 100 || fb[i + 1] > 100 || fb[i + 2] > 100)
    })
    .count()
}

/// Shared check for the auto-detecting monitors (85-2, 85-2A, 85-3):
/// boot with the module, verify the monitor copied the payload (module
/// file minus its 12-byte header) to RAM 0, printed the banner, and that
/// a `PRINT 123` line produces console output.
fn check_auto_module(model: Model, module_file: &str, banner: std::ops::Range<usize>) {
    let module = std::fs::read(format!("{ROM_DIR}{module_file}")).expect("cannot read module");
    let mut machine = boot(model, Some(module.clone()), 600);

    // The monitor BIOS detected the module and transferred it to
    // RAM 0x0000 (skipping the .rmm header).
    assert_eq!(
        &machine.bus.memory.ram[0..0x100],
        &module[12..12 + 0x100],
        "{model:?}: module payload not copied to RAM 0"
    );
    assert!(!machine.cpu.halted, "{model:?}: CPU halted after module boot");

    // BASIC banner on the console; nothing below it yet.
    assert!(
        lit_rows(&machine, banner.clone()) >= 5,
        "{model:?}: no BASIC banner on screen"
    );
    let output_band = banner.end + 1..banner.end + 22;
    assert_eq!(lit_rows(&machine, output_band.clone()), 0);

    // The module code executes: PRINT 123 echoes the line and prints
    // the result on the next console rows.
    type_str(&mut machine, "PRINT 123\n");
    for _ in 0..200 {
        machine.step_frame();
    }
    let lit = lit_rows(&machine, output_band);
    assert!(
        lit >= 10,
        "{model:?}: no PRINT output on the console ({lit} rows)"
    );
}

#[test]
fn basic3_module_boots_and_runs_on_85_3() {
    // banner occupies scanlines 10-16
    check_auto_module(Model::Pmd853, "basic3.rmm", 8..20);
}

#[test]
fn basic2_module_boots_and_runs_on_85_2() {
    check_auto_module(Model::Pmd852, "basic2.rmm", 8..20);
}

#[test]
fn basic2a_module_boots_and_runs_on_85_2a() {
    check_auto_module(Model::Pmd852a, "basic2A.rmm", 8..20);
}

#[test]
fn basic1_module_boots_and_runs_on_85_1() {
    // basic1.rmm is headerless: the monitor's TRANSFER routine copies
    // the file as-is to RAM 0, but only after the `BASIC G` command.
    let module = std::fs::read(format!("{ROM_DIR}basic1.rmm")).expect("cannot read module");
    let mut machine = boot(Model::Pmd851, Some(module.clone()), 200);

    // Before the command, nothing is copied and the monitor prompt is
    // the only thing on screen (dialog line at scanlines 245-251).
    assert!(
        machine.bus.memory.ram[0..0x40].iter().all(|&b| b == 0),
        "module copied without the BASIC G command"
    );

    // The command needs an argument (the monitor's command table
    // matches "BASIC " including the trailing space).
    type_str(&mut machine, "BASIC G\n");
    for _ in 0..600 {
        machine.step_frame();
    }

    // TRANSFER copied the module to RAM 0 and BASIC-G booted: banner
    // around scanlines 208-214, dialog "Ok" at 245-251.
    assert_eq!(
        &machine.bus.memory.ram[0..0x100],
        &module[0..0x100],
        "module payload not copied to RAM 0"
    );
    assert!(!machine.cpu.halted);
    assert!(
        lit_rows(&machine, 205..218) >= 5,
        "no BASIC-G banner on screen"
    );
    assert_eq!(lit_rows(&machine, 216..233), 0);

    // BASIC executes: PRINT 123 prints the result right below (the
    // console scrolls, the banner moves up one row).
    type_str(&mut machine, "PRINT 123\n");
    for _ in 0..200 {
        machine.step_frame();
    }
    let lit = lit_rows(&machine, 216..233);
    assert!(lit >= 5, "no PRINT output on the console ({lit} rows)");
}

#[test]
fn no_module_leaves_monitor_in_control() {
    // Control: without a module nothing is copied to RAM 0 and no
    // BASIC banner appears; the monitors stay in their command loops.
    for model in [Model::Pmd851, Model::Pmd852, Model::Pmd852a, Model::Pmd853] {
        let frames = if model == Model::Pmd853 { 500 } else { 200 };
        let machine = boot(model, None, frames);
        assert!(
            machine.bus.memory.ram[0..0x40].iter().all(|&b| b == 0),
            "{model:?}: RAM 0 changed without a module"
        );
        assert_eq!(lit_rows(&machine, 8..20), 0, "{model:?}: banner band lit");
        assert_eq!(
            lit_rows(&machine, 196..210),
            0,
            "{model:?}: 85-1 banner band lit"
        );
        assert!(!machine.cpu.halted, "{model:?}: CPU halted");
        // monitor command loop (high memory), not module code
        assert!(
            machine.cpu.pc >= 0x8000,
            "{model:?}: PC {:#06x} left the monitor",
            machine.cpu.pc
        );
    }
}
