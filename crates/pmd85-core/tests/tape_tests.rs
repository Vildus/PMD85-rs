//! End-to-end tape tests against the real Monitor 3 ROM: MGLD loads a
//! file played from the deck into RAM, and MGSV saves a memory range
//! that the deck recorder captures as a PTP block.

use pmd85_core::keyboard::Key;
use pmd85_core::machine::Machine;
use pmd85_core::model::Model;
use pmd85_core::tape::{make_file, Tape, TapeBlock};

const ROM_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Rom/");

/// Boot an 85-3 through the RAM test into the dialog line (as in the
/// boot tests).
fn boot() -> Machine {
    let monitor = std::fs::read(format!("{ROM_DIR}{}", Model::Pmd853.default_monitor()))
        .expect("cannot read monitor ROM");
    let mut machine = Machine::new(Model::Pmd853, &monitor, None);
    for _ in 0..500 {
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

fn type_command(machine: &mut Machine, keys: &[Key]) {
    for key in keys {
        type_key(machine, *key);
    }
}

/// Play a tape row (header block followed by its body), stepping the
/// machine until both blocks have been fed. Returns whether playback
/// ran to the end within `frames`.
fn play_file(machine: &mut Machine, block: &TapeBlock, frames: u64) -> bool {
    machine
        .bus
        .tape_play(&block.header_bytes, true, true);
    let mut body_started = false;
    for _ in 0..frames {
        machine.step_frame();
        if machine.bus.tape.take_block_finished() {
            if body_started {
                return true;
            }
            machine.bus.tape_play(&block.body_bytes, false, false);
            body_started = true;
        }
    }
    false
}

/// A distinctive file to load: 0x76 (HLT) first so that an auto-start
/// after MGLD parks the CPU harmlessly.
fn loadable_file() -> TapeBlock {
    let mut content: Vec<u8> = (0..256u16).map(|i| (i as u8) ^ 0xA5).collect();
    content[0] = 0x76;
    make_file(0, b'?', "LOADTST", 0x1000, &content).unwrap()
}

#[test]
fn mgld_loads_a_played_file() {
    let mut machine = boot();
    let block = loadable_file();

    // Enter the load command; the monitor then waits for tape
    // activity (its EEBE loop spins until the leader tone appears).
    use Key::*;
    type_command(
        &mut machine,
        &[M, G, L, D, Space, Digit0, Digit0, Enter],
    );

    assert!(
        play_file(&mut machine, &block, 900),
        "playback did not finish"
    );

    // Give the monitor time to process the tail of the block.
    for _ in 0..60 {
        machine.step_frame();
    }

    // The file content must sit at the load address. (If the monitor
    // auto-started the program, byte 0 is HLT and the rest survived.)
    let ram = &machine.bus.memory.ram;
    for (i, &expect) in block.content().iter().enumerate() {
        assert_eq!(
            ram[0x1000 + i],
            expect,
            "RAM mismatch at +{i} (offset {:#x})",
            0x1000 + i
        );
    }
    assert!(!machine.bus.tape.is_playing());
}

#[test]
fn machine_reset_stops_playback() {
    let mut machine = boot();
    let block = loadable_file();
    machine.bus.tape_play(&block.header_bytes, true, true);
    for _ in 0..30 {
        machine.step_frame();
    }
    assert!(machine.bus.tape.is_playing());
    machine.reset();
    assert!(!machine.bus.tape.is_playing());
}

#[test]
fn mgsv_records_a_save() {
    let mut machine = boot();

    // A recognizable memory range to save (inclusive range, so 17
    // bytes: 0x2000..=0x2010).
    let content: Vec<u8> = (0..17u16).map(|i| 0xC0 ^ i as u8).collect();
    for (i, &b) in content.iter().enumerate() {
        machine.bus.memory.ram[0x2000 + i] = b;
    }

    // MGSV number start end — the monitor pauses, sends the leader,
    // the 15 header bytes and the body through the 8251, all paced by
    // the deck's transmitter throttle. The file number is a two-digit
    // hex field (parsed by PAIRIN), so `0` alone is a syntax error.
    use Key::*;
    type_command(
        &mut machine,
        &[
            M, G, S, V, Space, Digit0, Digit0, Space, Digit2, Digit0, Digit0, Digit0, Space, Digit2,
            Digit0, Digit1, Digit0, Enter,
        ],
    );

    // The save takes a few seconds of leader delays plus the
    // throttled byte transfer, then the recorder finalizes the block
    // after 2 s of transmitter silence.
    for _ in 0..900 {
        machine.step_frame();
    }
    assert!(!machine.bus.tape.is_recording(), "recorder still active");

    let recorded = machine.bus.tape.take_recorded();
    assert!(!recorded.is_empty(), "no blocks recorded");
    let mut tape = Tape::default();
    tape.append_ptp_stream(&recorded);
    assert_eq!(tape.blocks.len(), 1, "one file expected: {:?}", tape.blocks.len());

    let saved = &tape.blocks[0];
    let h = saved.header.as_ref().expect("recorded block has a header");
    assert_eq!(h.number, 0);
    assert_eq!(h.block_type, b'?', "default monitor file type");
    assert_eq!(h.start, 0x2000);
    assert_eq!(h.content_len(), 17, "wLength");
    assert_eq!(saved.content(), &content[..], "saved content mismatch");
    assert!(saved.header_crc_ok);
    assert!(saved.body_crc_ok);
    assert!(saved.body_length_error.is_none());

    // The saved file round-trips through MGLD.
    let mut machine2 = boot();
    type_command(&mut machine2, &[M, G, L, D, Space, Digit0, Digit0, Enter]);
    assert!(play_file(&mut machine2, saved, 900));
    for _ in 0..60 {
        machine2.step_frame();
    }
    for (i, &expect) in content.iter().enumerate() {
        assert_eq!(machine2.bus.memory.ram[0x2000 + i], expect, "reload +{i}");
    }
}
