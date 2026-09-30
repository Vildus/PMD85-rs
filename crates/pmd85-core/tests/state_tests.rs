//! Save-state tests against the real Monitor 3 ROM: round-trips are
//! deterministic, the format refuses foreign or corrupt data, and no
//! state can be taken while the tape runs.

use pmd85_core::keyboard::Key;
use pmd85_core::machine::Machine;
use pmd85_core::model::Model;
use pmd85_core::state::StateError;
use pmd85_core::tapedeck::PlayItem;

const ROM_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Rom/");

fn monitor() -> Vec<u8> {
    std::fs::read(format!("{ROM_DIR}{}", Model::Pmd853.default_monitor()))
        .expect("cannot read monitor ROM")
}

/// Boot an 85-3 through the RAM test into the dialog line (as in the
/// tape tests).
fn boot() -> Machine {
    let monitor = monitor();
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

/// Two machines run in lockstep after a restore: the snapshot
/// captures everything execution depends on.
#[test]
fn restore_continues_deterministically() {
    let mut a = boot();
    // Make the state non-trivial: RAM content, typed keys on the
    // monitor line, a held key and some cycles on the clock.
    for i in 0..64u16 {
        a.bus.memory.ram[0x3000 + i as usize] = (i ^ 0x5A) as u8;
    }
    type_command(
        &mut a,
        &[Key::M, Key::G, Key::L, Key::D, Key::Space, Key::Digit0, Key::Digit0],
    );
    a.bus.keyboard.set_key(Key::Space, true);
    for _ in 0..25 {
        a.step_frame();
    }

    let snapshot = a.save_state().expect("state can be taken when idle");
    let monitor = monitor();
    let mut b = Machine::new(Model::Pmd853, &monitor, None);
    b.restore_state(&snapshot).expect("state restores");

    // The restored machine picks up exactly where the first one was.
    assert_eq!(b.cpu.pc, a.cpu.pc);
    assert_eq!(b.cpu.sp, a.cpu.sp);
    assert_eq!(b.cpu.flags.pack(), a.cpu.flags.pack());
    assert_eq!(b.bus.total_cycles(), a.bus.total_cycles());
    assert!(b.bus.keyboard.is_pressed(Key::Space), "held key survived");
    assert_eq!(&b.bus.memory.ram[0x3000..0x3040], &a.bus.memory.ram[0x3000..0x3040]);

    // And both develop identically from there.
    for _ in 0..200 {
        a.step_frame();
        b.step_frame();
    }
    assert_eq!(b.cpu.pc, a.cpu.pc);
    assert_eq!(b.bus.total_cycles(), a.bus.total_cycles());
    assert_eq!(&b.bus.memory.ram[..], &a.bus.memory.ram[..]);
}

/// The snapshot includes the ROM module image; a restore installs it
/// even into a machine booted without one.
#[test]
fn restore_reinstalls_the_rom_module() {
    let monitor = monitor();
    let module: Vec<u8> = (0..2048u32).map(|i| (i ^ 0xA5) as u8).collect();
    let mut a = Machine::new(Model::Pmd853, &monitor, Some(module.clone()));
    for _ in 0..100 {
        a.step_frame();
    }
    let snapshot = a.save_state().unwrap();

    let mut b = Machine::new(Model::Pmd853, &monitor, None);
    assert!(b.bus.memory.rom_module.is_none());
    b.restore_state(&snapshot).unwrap();
    assert_eq!(b.bus.memory.rom_module.as_deref(), Some(&module[..]));
}

/// A state taken while the deck plays or records is refused.
#[test]
fn save_state_refuses_while_the_tape_runs() {
    // Playback: enter the MGLD wait and start feeding a block.
    let mut machine = boot();
    type_command(
        &mut machine,
        &[Key::M, Key::G, Key::L, Key::D, Key::Space, Key::Digit0, Key::Digit0],
    );
    machine.bus.tape_play_session(vec![PlayItem {
        data: vec![0xFF; 256],
        head: true,
        flash: false,
    }]);
    machine.step_frame();
    assert!(machine.bus.tape.is_playing());
    assert_eq!(machine.save_state(), Err(StateError::TapeBusy));

    // Recording: MGSV a range and let the recorder see the leader
    // (the monitor pauses a couple of seconds before sending).
    let mut machine = boot();
    for i in 0..16u16 {
        machine.bus.memory.ram[0x2000 + i as usize] = i as u8;
    }
    type_command(
        &mut machine,
        &[
            Key::M, Key::G, Key::S, Key::V, Key::Space, Key::Digit0, Key::Digit0,
            Key::Space, Key::Digit2, Key::Digit0, Key::Digit0, Key::Digit0,
            Key::Space, Key::Digit2, Key::Digit0, Key::Digit1, Key::Digit0,
            Key::Enter,
        ],
    );
    for _ in 0..250 {
        machine.step_frame();
    }
    assert!(machine.bus.tape.is_recording(), "the recorder should be running");
    assert_eq!(machine.save_state(), Err(StateError::TapeBusy));
}

/// Bad magic, wrong version, truncation and trailing garbage are all
/// rejected, and a failed load leaves the machine untouched.
#[test]
fn restore_rejects_bad_data() {
    let machine = boot();
    let good = machine.save_state().unwrap();

    // Not a state at all.
    let mut other = boot();
    assert_eq!(other.restore_state(b""), Err(StateError::UnexpectedEof));
    assert_eq!(other.restore_state(b"NOTASTATE........"), Err(StateError::BadMagic));

    // Wrong version.
    let mut bad_version = good.clone();
    bad_version[4] = 0xFF;
    assert_eq!(
        other.restore_state(&bad_version),
        Err(StateError::UnsupportedVersion(0xFF))
    );

    // Truncated in the middle of the RAM image.
    let truncated = &good[..good.len() / 2];
    assert_eq!(other.restore_state(truncated), Err(StateError::UnexpectedEof));

    // Trailing garbage.
    let mut trailing = good.clone();
    trailing.push(0);
    assert_eq!(other.restore_state(&trailing), Err(StateError::TrailingBytes));

    // A failed load never half-mutated the machine.
    assert_eq!(other.cpu.pc, machine.cpu.pc);
    assert_eq!(other.bus.total_cycles(), machine.bus.total_cycles());
    assert_eq!(&other.bus.memory.ram[..], &machine.bus.memory.ram[..]);
    // And the good state still loads.
    other.restore_state(&good).unwrap();
    assert_eq!(other.cpu.pc, machine.cpu.pc);
}

/// A state restores only into the same model with the same monitor.
#[test]
fn restore_rejects_foreign_machines() {
    let mut machine = boot();
    let snapshot = machine.save_state().unwrap();

    // Different model (PMD 85-2 with its own 4 KiB monitor).
    let mon2 = std::fs::read(format!("{ROM_DIR}monit2.rom")).expect("cannot read monit2");
    let mut m2 = Machine::new(Model::Pmd852, &mon2, None);
    assert_eq!(m2.restore_state(&snapshot), Err(StateError::ModelMismatch));

    // Same model, different monitor bytes.
    let mut flipped = monitor();
    flipped[0x100] ^= 0xFF;
    let mut m3 = Machine::new(Model::Pmd853, &flipped, None);
    assert_eq!(m3.restore_state(&snapshot), Err(StateError::MonitorMismatch));

    // The unknown model tag of a future format.
    let mut unknown = snapshot.clone();
    unknown[8] = 0x7F;
    assert_eq!(machine.restore_state(&unknown), Err(StateError::ModelMismatch));
}

/// The header layout is pinned: magic, version, model tag, monitor
/// fingerprint. Bump `STATE_VERSION` whenever the sections change.
#[test]
fn snapshot_header_is_stable() {
    let machine = boot();
    let snapshot = machine.save_state().unwrap();
    assert_eq!(&snapshot[0..4], b"PMDS");
    assert_eq!(u32::from_le_bytes([snapshot[4], snapshot[5], snapshot[6], snapshot[7]]), 1);
    assert_eq!(snapshot[8], 4, "model tag for PMD 85-3");
    // Monitor 3: 8 KiB, CRC of its first byte lives at offset 11.
    assert_eq!(u16::from_le_bytes([snapshot[9], snapshot[10]]), 0x2000);
}
