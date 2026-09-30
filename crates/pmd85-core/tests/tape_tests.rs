//! End-to-end tape tests against the real Monitor 3 ROM: MGLD loads a
//! file played from the deck into RAM, and MGSV saves a memory range
//! that the deck recorder captures as a PTP block.

use pmd85_core::keyboard::Key;
use pmd85_core::machine::Machine;
use pmd85_core::model::Model;
use pmd85_core::tape::{make_file, Tape, TapeBlock};
use pmd85_core::tapedeck::PlayItem;

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
    machine.bus.tape_play_session(vec![
        PlayItem {
            data: block.header_bytes.clone(),
            head: true,
            flash: false,
        },
        PlayItem {
            data: block.body_bytes.clone(),
            head: false,
            flash: false,
        },
    ]);
    for _ in 0..frames {
        machine.step_frame();
        if !machine.bus.tape.is_playing() {
            return true;
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
    machine.bus.tape_play_session(vec![PlayItem {
        data: block.header_bytes.clone(),
        head: true,
        flash: false,
    }]);
    for _ in 0..30 {
        machine.step_frame();
    }
    assert!(machine.bus.tape.is_playing());
    machine.reset();
    assert!(!machine.bus.tape.is_playing());
}

/// The block-read intercept contract: entering `EDC4` (BLKLOAD) while
/// the deck serves a flash block skips the routine wholesale and
/// leaves behind exactly what the real one would.
#[test]
fn flash_block_read_contract() {
    let mut machine = boot();
    let data = [1u8, 2, 3, 4, 5];
    let checksum = data.iter().fold(0u8, |a, &b| a.wrapping_add(b));
    machine.bus.tape_play_session(vec![PlayItem {
        data: [data.as_slice(), &[checksum]].concat(),
        head: false,
        flash: true,
    }]);

    // A fake stack whose return address parks on a HLT, so the
    // registers survive the routine's RET untouched.
    machine.bus.memory.ram[0x0500] = 0x76;
    machine.cpu.pc = 0xEDC4;
    machine.cpu.sp = 0xBEF0;
    machine.bus.memory.ram[0xBEF0] = 0x00;
    machine.bus.memory.ram[0xBEF1] = 0x05;
    machine.cpu.h = 0x20;
    machine.cpu.l = 0x00;
    machine.cpu.d = 0x00;
    machine.cpu.e = data.len() as u8 - 1;
    machine.cpu.c = 1;
    machine.run_cycles(100);

    assert_eq!(&machine.bus.memory.ram[0x2000..0x2005], &data, "data written");
    assert!(machine.cpu.halted, "returned through the routine's RET");
    assert_eq!(machine.cpu.pc, 0x0501);
    assert_eq!(machine.cpu.b, checksum, "B = running checksum");
    assert_eq!(machine.cpu.a, 0);
    assert!(machine.cpu.flags.z, "checksum matched");
    assert!(!machine.cpu.flags.cy);
    assert_eq!(u16::from_le_bytes([machine.cpu.l, machine.cpu.h]), 0x2000, "HL restored");
    assert_eq!(u16::from_le_bytes([machine.cpu.e, machine.cpu.d]), 0xFFFF, "DE = 0xFFFF");
    assert!(!machine.bus.tape.flash_armed(), "block consumed");
    assert!(!machine.bus.tape.is_playing(), "session over");
}

/// With C = 0 (check mode) the block-read intercept verifies the
/// checksum without writing anything.
#[test]
fn flash_block_read_contract_check_only() {
    let mut machine = boot();
    let data = [9u8, 8, 7];
    let checksum = data.iter().fold(0u8, |a, &b| a.wrapping_add(b));
    machine.bus.tape_play_session(vec![PlayItem {
        data: [data.as_slice(), &[checksum]].concat(),
        head: false,
        flash: true,
    }]);
    machine.bus.memory.ram[0x2000] = 0xEE;
    machine.bus.memory.ram[0x0500] = 0x76;

    machine.cpu.pc = 0xEDC4;
    machine.cpu.sp = 0xBEF0;
    machine.bus.memory.ram[0xBEF0] = 0x00;
    machine.bus.memory.ram[0xBEF1] = 0x05;
    machine.cpu.h = 0x20;
    machine.cpu.l = 0x00;
    machine.cpu.d = 0x00;
    machine.cpu.e = data.len() as u8 - 1;
    machine.cpu.c = 0;
    machine.run_cycles(100);

    assert_eq!(machine.bus.memory.ram[0x2000], 0xEE, "check mode writes nothing");
    assert!(machine.cpu.flags.z, "checksum matched");
    assert_eq!(machine.cpu.b, checksum);
}

/// A checksum mismatch must surface as carry (the callers' error
/// branch) with Z clear.
#[test]
fn flash_block_read_contract_bad_checksum() {
    let mut machine = boot();
    machine.bus.tape_play_session(vec![PlayItem {
        data: vec![1, 2, 3, 0xFF], // real checksum is 6
        head: false,
        flash: true,
    }]);
    machine.bus.memory.ram[0x0500] = 0x76;
    machine.cpu.pc = 0xEDC4;
    machine.cpu.sp = 0xBEF0;
    machine.bus.memory.ram[0xBEF0] = 0x00;
    machine.bus.memory.ram[0xBEF1] = 0x05;
    machine.cpu.h = 0x20;
    machine.cpu.l = 0x00;
    machine.cpu.d = 0x00;
    machine.cpu.e = 2;
    machine.cpu.c = 1;
    machine.run_cycles(100);
    assert!(machine.cpu.flags.cy, "mismatch must set carry");
    assert!(!machine.cpu.flags.z);
}

/// The byte-read intercept contract: entering `EB6C` while the deck
/// serves a flash block returns the next byte with all flags clear,
/// and reports carry (the timeout path) once the data runs out.
#[test]
fn flash_byte_read_contract() {
    let mut machine = boot();
    machine.bus.tape_play_session(vec![PlayItem {
        data: vec![0xAB, 0xCD, 0x42],
        head: false,
        flash: true,
    }]);
    machine.bus.memory.ram[0x0500] = 0x76; // HLT at the fake return
    for (n, expect) in [0xABu8, 0xCD, 0x42].into_iter().enumerate() {
        machine.cpu.pc = 0xEB6C;
        machine.cpu.sp = 0xBEF0;
        machine.bus.memory.ram[0xBEF0] = 0x00;
        machine.bus.memory.ram[0xBEF1] = 0x05;
        machine.cpu.halted = false;
        machine.cpu.a = 0x00;
        machine.cpu.flags.cy = true;
        machine.cpu.flags.z = true;
        machine.run_cycles(20);
        assert_eq!(machine.cpu.a, expect);
        assert!(machine.cpu.halted, "returned through the routine's RET");
        assert_eq!(machine.cpu.pc, 0x0501);
        assert!(!machine.cpu.flags.cy);
        assert!(!machine.cpu.flags.z, "flags are clear on success");
        // The last byte ends the block (and the session) itself.
        assert_eq!(machine.bus.tape.flash_armed(), n < 2);
    }
    assert!(!machine.bus.tape.is_playing(), "session over");
}

/// MGLD through a flash session: the header plays for real (with the
/// shortened leader), the body is served through the intercepts. The
/// whole load takes barely over a second of emulated time.
#[test]
fn mgld_flash_loads_the_file() {
    let mut machine = boot();
    let block = loadable_file();
    use Key::*;
    type_command(&mut machine, &[M, G, L, D, Space, Digit0, Digit0, Enter]);

    machine.bus.tape_play_session(vec![
        PlayItem {
            data: block.header_bytes.clone(),
            head: true,
            flash: true,
        },
        PlayItem {
            data: block.body_bytes.clone(),
            head: false,
            flash: true,
        },
    ]);
    let mut frames = 0;
    while machine.bus.tape.is_playing() {
        frames += 1;
        assert!(frames < 300, "flash load did not finish");
        machine.step_frame();
    }
    assert!(
        frames < 120,
        "flash load took {frames} frames — barely faster than real playback"
    );

    // Let the monitor leave the load routine and settle.
    for _ in 0..60 {
        machine.step_frame();
    }
    let ram = &machine.bus.memory.ram;
    for (i, &expect) in block.content().iter().enumerate() {
        assert_eq!(ram[0x1000 + i], expect, "RAM mismatch at +{i}");
    }
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
    // decimal field (the ROM's EA54 parser weights the digits ×10
    // and caps the value at 99), so `0` alone is a syntax error.
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

/// The MGLD/MGSV file number is parsed with DECIMAL weighting by the
/// monitor (ROM EA54: value = 10·hi + lo, capped at 99) — not as a
/// hex byte. `MGSV 17` must therefore record the header number as
/// binary 17 (0x11), which is what GPMD85-style tape browsers
/// display and what `MGLD 17` later matches.
#[test]
fn mgsv_file_number_is_decimal() {
    let mut machine = boot();
    for i in 0..17usize {
        machine.bus.memory.ram[0x2000 + i] = 0xC0 ^ i as u8;
    }

    use Key::*;
    type_command(
        &mut machine,
        &[
            M, G, S, V, Space, Digit1, Digit7, Space, Digit2, Digit0, Digit0, Digit0, Space, Digit2,
            Digit0, Digit1, Digit0, Enter,
        ],
    );
    for _ in 0..900 {
        machine.step_frame();
    }
    assert!(!machine.bus.tape.is_recording(), "recorder still active");

    let mut tape = Tape::default();
    tape.append_ptp_stream(&machine.bus.tape.take_recorded());
    let h = tape.blocks[0].header.as_ref().expect("header");
    assert_eq!(h.number, 17, "MGSV 17 must record binary 17 (0x11)");
}

/// The MGLD counterpart: a file whose header number byte is 0x43
/// (67 decimal, like BASIC files in the wild) is loaded by typing
/// `MGLD 67` — and NOT by typing `MGLD 43`, which the monitor
/// parses as 43 decimal (0x2B).
#[test]
fn mgld_file_number_is_decimal() {
    let mut content: Vec<u8> = (0..32u16).map(|i| (i as u8) ^ 0xA5).collect();
    content[0] = 0x76;
    let block = make_file(0x43, b'?', "DECIMAL", 0x1000, &content).unwrap();

    use Key::*;

    let mut machine = boot();
    type_command(&mut machine, &[M, G, L, D, Space, Digit6, Digit7, Enter]);
    assert!(
        play_file(&mut machine, &block, 900),
        "playback did not finish"
    );
    for _ in 0..60 {
        machine.step_frame();
    }
    for (i, &expect) in content.iter().enumerate() {
        assert_eq!(machine.bus.memory.ram[0x1000 + i], expect, "MGLD 67 +{i}");
    }

    // The hex-style reading of the same number must fail to match.
    let mut machine = boot();
    type_command(&mut machine, &[M, G, L, D, Space, Digit4, Digit3, Enter]);
    assert!(
        play_file(&mut machine, &block, 900),
        "playback did not finish"
    );
    for _ in 0..60 {
        machine.step_frame();
    }
    let loaded = (0..content.len())
        .any(|i| machine.bus.memory.ram[0x1000 + i] == content[i]);
    assert!(!loaded, "MGLD 43 must not load file number 0x43");
}
