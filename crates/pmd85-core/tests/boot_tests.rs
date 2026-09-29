//! Boot smoke tests: every model must boot its Monitor ROM through the
//! RAM test into the dialog (command) line, accept typed keys, echo them
//! into Video RAM, and (PMD 85-3) execute a real monitor command.

use pmd85_core::keyboard::Key;
use pmd85_core::machine::Machine;
use pmd85_core::model::Model;

const ROM_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Rom/");

/// Dialog line geometry: the last text row of the screen, microrows 246-252
/// with the cursor on 253-254, i.e. VRAM 0xFD80-0xFE00 (VRAM2 for 85-3).
const DIALOG_LINE: std::ops::Range<usize> = 0xFD80..0xFE00;

fn boot(model: Model, frames: u64) -> Machine {
    let monitor = std::fs::read(format!("{ROM_DIR}{}", model.default_monitor()))
        .expect("cannot read monitor ROM");
    let mut machine = Machine::new(model, &monitor, None);
    for _ in 0..frames {
        machine.step_frame();
    }
    machine
}

/// Press a key long enough for the monitor scan loop to pick it up.
fn type_key(machine: &mut Machine, key: Key) {
    machine.bus.keyboard.set_key(key, true);
    for _ in 0..8 {
        machine.step_frame();
    }
    machine.bus.keyboard.set_key(key, false);
    for _ in 0..8 {
        machine.step_frame();
    }
}

fn check_booted_and_echo(model: Model, frames: u64) {
    let mut machine = boot(model, frames);

    // The startup shadow map must have been dropped by the monitor's
    // first I/O write (PPI initialization).
    assert!(
        !machine.bus.memory.startup_map(),
        "{model:?}: startup shadow map still active"
    );
    assert!(!machine.cpu.halted, "{model:?}: CPU halted after boot");
    // Monitors run from 0x8000 up (relocated into RAM / ROM at the top).
    assert!(
        machine.cpu.pc >= 0x8000,
        "{model:?}: suspicious PC {:#06x} after boot",
        machine.cpu.pc
    );

    // Typing a key must echo a glyph into the dialog line.
    let before: Vec<u8> = DIALOG_LINE.clone().map(|a| machine.bus.memory.ram[a]).collect();
    type_key(&mut machine, Key::A);
    let after: Vec<u8> = DIALOG_LINE.clone().map(|a| machine.bus.memory.ram[a]).collect();
    assert_ne!(before, after, "{model:?}: dialog line did not change");

    // The glyph is drawn one character cell (6 pixels) wide in the first
    // column: bytes 0 of microrows 246-252.
    let glyph: Vec<u8> = (0..8).map(|r| machine.bus.memory.ram[0xFD80 + r * 0x40]).collect();
    assert!(
        glyph.iter().any(|&b| b != 0),
        "{model:?}: no 'A' glyph in the dialog line: {glyph:02x?}"
    );
}

#[test]
fn keypress_produces_speaker_click() {
    // The monitor acknowledges every keypress with a short click: a
    // burst of the PC1-gated 4 kHz fixed tone from the video divider,
    // i.e. a short run of speaker edges ~256 cycles apart. Monitor 1
    // (85-1) is the exception: it only beeps for errors and commands,
    // not for plain keypresses.
    for model in [Model::Pmd852, Model::Pmd852a, Model::Pmd853] {
        let frames = if model == Model::Pmd853 { 500 } else { 200 };
        let mut machine = boot(model, frames);
        machine.take_speaker_edges(); // discard boot-time noise, if any

        machine.bus.keyboard.set_key(Key::A, true);
        for _ in 0..8 {
            machine.step_frame();
        }
        machine.bus.keyboard.set_key(Key::A, false);
        for _ in 0..8 {
            machine.step_frame();
        }

        let edges = machine.take_speaker_edges();
        assert!(
            !edges.is_empty(),
            "{model:?}: no speaker activity on keypress"
        );
        // A 4 kHz burst over a ~16 frame keypress: tens of edges at
        // most, each toggling the level, spaced by one 512-cycle tone
        // period (within instruction-timing jitter).
        assert!(edges.len() < 200, "{model:?}: suspicious edge count {}", edges.len());
        for pair in edges.windows(2) {
            assert_eq!(
                pair[0].level, !pair[1].level,
                "{model:?}: click edges do not alternate"
            );
            let spacing = pair[1].cycle - pair[0].cycle;
            assert!(
                (150..350).contains(&spacing),
                "{model:?}: click edge spacing {spacing} is not ~256 cycles"
            );
        }
    }

    // 85-1: plain keypresses are silent.
    let mut machine = boot(Model::Pmd851, 200);
    machine.take_speaker_edges();
    machine.bus.keyboard.set_key(Key::A, true);
    for _ in 0..8 {
        machine.step_frame();
    }
    machine.bus.keyboard.set_key(Key::A, false);
    for _ in 0..8 {
        machine.step_frame();
    }
    assert!(machine.take_speaker_edges().is_empty());
}

#[test]
fn pmd851_boots_and_echoes() {
    check_booted_and_echo(Model::Pmd851, 200);
}

#[test]
fn pmd852_boots_and_echoes() {
    check_booted_and_echo(Model::Pmd852, 200);
}

#[test]
fn pmd852a_boots_and_echoes() {
    check_booted_and_echo(Model::Pmd852a, 200);
}

#[test]
fn pmd853_boots_and_echoes() {
    // Monitor 3 runs a longer RAM test before showing the dialog line.
    check_booted_and_echo(Model::Pmd853, 500);
}

#[test]
fn pmd853_executes_monitor_command() {
    let mut machine = boot(Model::Pmd853, 500);

    // "DUMP 0100" + Enter: the monitor parses the command and fills the
    // console area (top of VRAM1) with a memory dump listing.
    use Key::*;
    let keys = [D, U, M, P, Space, Digit0, Digit1, Digit0, Digit0, Enter];
    for key in keys {
        type_key(&mut machine, key);
    }
    for _ in 0..100 {
        machine.step_frame();
    }

    let console: u32 = machine.bus.memory.ram[0xC000..0xC800]
        .iter()
        .map(|&b| b.count_ones())
        .sum();
    assert!(
        console > 100,
        "DUMP output missing from the console area ({} bits)",
        console
    );
}
