//! Unit tests for the i8080 core: each instruction group, flags, cycle
//! counts and interrupt behavior.

use pmd85_core::cpu::{Bus, Cpu, Flags};

/// Flat 64 KiB RAM bus with port capture, for CPU-level testing.
struct RamBus {
    mem: Vec<u8>,
    in_ports: [u8; 256],
    out_log: Vec<(u8, u8)>,
}

impl RamBus {
    fn new() -> Self {
        RamBus {
            mem: vec![0; 0x10000],
            in_ports: [0; 256],
            out_log: Vec::new(),
        }
    }

    fn load(&mut self, addr: u16, program: &[u8]) {
        self.mem[addr as usize..addr as usize + program.len()].copy_from_slice(program);
    }
}

impl Bus for RamBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.mem[addr as usize]
    }
    fn write(&mut self, addr: u16, data: u8) {
        self.mem[addr as usize] = data;
    }
    fn port_in(&mut self, port: u8) -> u8 {
        self.in_ports[port as usize]
    }
    fn port_out(&mut self, port: u8, data: u8) {
        self.out_log.push((port, data));
    }
}

fn run(program: &[u8]) -> (Cpu, RamBus) {
    let mut cpu = Cpu::new();
    let mut bus = RamBus::new();
    bus.load(0x0000, program);
    for _ in 0..1000 {
        if cpu.halted {
            break;
        }
        cpu.step(&mut bus);
    }
    (cpu, bus)
}

fn flags_of(cpu: &Cpu) -> String {
    let f = cpu.flags;
    format!(
        "z={} s={} p={} cy={} ac={}",
        f.z as u8, f.s as u8, f.p as u8, f.cy as u8, f.ac as u8
    )
}

#[test]
fn mov_registers() {
    // MVI A,0x42 ; MOV B,A ; MOV C,B ; HLT
    let (cpu, _) = run(&[0x3E, 0x42, 0x47, 0x48, 0x76]);
    assert_eq!(cpu.a, 0x42);
    assert_eq!(cpu.b, 0x42);
    assert_eq!(cpu.c, 0x42);
}

#[test]
fn mov_to_from_memory() {
    // LXI H,0x1234 ; MVI M,0x99 ; MOV C,M ; HLT
    let (cpu, bus) = run(&[0x21, 0x34, 0x12, 0x36, 0x99, 0x4E, 0x76]);
    assert_eq!(cpu.c, 0x99);
    assert_eq!(bus.mem[0x1234], 0x99);
}

#[test]
fn add_sets_carry_and_aux() {
    // MVI A,0x0F ; ADD A -> 0x1E, AC set, CY clear
    let (cpu, _) = run(&[0x3E, 0x0F, 0x87, 0x76]);
    assert_eq!(cpu.a, 0x1E);
    assert!(cpu.flags.ac, "AC should be set: {}", flags_of(&cpu));
    assert!(!cpu.flags.cy);
    // MVI A,0xFF ; ADD A -> carry
    let (cpu, _) = run(&[0x3E, 0xFF, 0x87, 0x76]);
    assert_eq!(cpu.a, 0xFE);
    assert!(cpu.flags.cy);
    assert!(cpu.flags.ac);
}

#[test]
fn sub_borrows() {
    // MVI A,0x05 ; SUI 0x03 -> 0x02, CY clear, AC clear
    let (cpu, _) = run(&[0x3E, 0x05, 0xD6, 0x03, 0x76]);
    assert_eq!(cpu.a, 0x02);
    assert!(!cpu.flags.cy);
    // MVI A,0x03 ; SUI 0x05 -> CY set
    let (cpu, _) = run(&[0x3E, 0x03, 0xD6, 0x05, 0x76]);
    assert_eq!(cpu.a, 0xFE);
    assert!(cpu.flags.cy);
}

#[test]
fn sub_aux_carry_means_no_borrow() {
    // MVI A,0x10 ; SUI 0x01: low nibble borrows -> AC clear
    let (cpu, _) = run(&[0x3E, 0x10, 0xD6, 0x01, 0x76]);
    assert_eq!(cpu.a, 0x0F);
    assert!(!cpu.flags.ac, "AC should be clear on nibble borrow: {}", flags_of(&cpu));
    // MVI A,0x12 ; SUI 0x01 -> 0x11, no borrow -> AC set
    let (cpu, _) = run(&[0x3E, 0x12, 0xD6, 0x01, 0x76]);
    assert_eq!(cpu.a, 0x11);
    assert!(cpu.flags.ac, "AC should be set when no borrow: {}", flags_of(&cpu));
}

#[test]
fn cmp_flags() {
    // MVI A,0x0A ; CPI 0x0A -> equal: Z set, CY clear
    let (cpu, _) = run(&[0x3E, 0x0A, 0xFE, 0x0A, 0x76]);
    assert!(cpu.flags.z);
    assert!(!cpu.flags.cy);
    // MVI A,0x0A ; CPI 0x0B -> less: CY set
    let (cpu, _) = run(&[0x3E, 0x0A, 0xFE, 0x0B, 0x76]);
    assert!(!cpu.flags.z);
    assert!(cpu.flags.cy);
    // accumulator unchanged
    assert_eq!(cpu.a, 0x0A);
}

#[test]
fn ana_sets_aux_from_bit3() {
    // MVI A,0xFF ; ANI 0xF7 -> AC set (operand bit 3 set), result 0xF7
    let (cpu, _) = run(&[0x3E, 0xFF, 0xE6, 0xF7, 0x76]);
    assert_eq!(cpu.a, 0xF7);
    assert!(cpu.flags.ac, "ANA AC quirk: {}", flags_of(&cpu));
    // MVI A,0x07 ; ANI 0x07 -> AC clear (neither operand has bit 3 set)
    let (cpu, _) = run(&[0x3E, 0x07, 0xE6, 0x07, 0x76]);
    assert_eq!(cpu.a, 0x07);
    assert!(!cpu.flags.ac);
}

#[test]
fn xra_clears_flags_and_a() {
    // MVI A,0x5A ; XRA A -> 0, Z set, CY/AC clear
    let (cpu, _) = run(&[0x3E, 0x5A, 0xAF, 0x76]);
    assert_eq!(cpu.a, 0);
    assert!(cpu.flags.z);
    assert!(!cpu.flags.cy);
    assert!(!cpu.flags.ac);
}

#[test]
fn inr_dcr_aux_and_flags() {
    // MVI C,0x0F ; INR C -> 0x10, AC set, no CY change
    let (cpu, _) = run(&[0x0E, 0x0F, 0x0C, 0x76]);
    assert_eq!(cpu.c, 0x10);
    assert!(cpu.flags.ac);
    // MVI C,0x12 ; DCR C -> 0x11, AC set (no borrow from bit 3)
    let (cpu, _) = run(&[0x0E, 0x12, 0x0D, 0x76]);
    assert_eq!(cpu.c, 0x11);
    assert!(cpu.flags.ac, "DCR sets AC when no borrow: {}", flags_of(&cpu));
    // MVI C,0x00 ; DCR C -> 0xFF, AC clear (borrow from bit 3), Z clear
    let (cpu, _) = run(&[0x0E, 0x00, 0x0D, 0x76]);
    assert_eq!(cpu.c, 0xFF);
    assert!(!cpu.flags.ac, "DCR clears AC on nibble borrow: {}", flags_of(&cpu));
    assert!(!cpu.flags.z);
    // MVI C,0x10 ; DCR C -> 0x0F, AC clear (borrow happened)
    let (cpu, _) = run(&[0x0E, 0x10, 0x0D, 0x76]);
    assert_eq!(cpu.c, 0x0F);
    assert!(!cpu.flags.ac);
}

#[test]
fn inr_dcr_do_not_touch_carry() {
    // STC ; MVI A,0x0F ; INR A -> CY stays set
    let (cpu, _) = run(&[0x37, 0x3E, 0x0F, 0x3C, 0x76]);
    assert!(cpu.flags.cy, "INR must not affect CY");
    // STC ; MVI A,0x00 ; DCR A -> CY stays set
    let (cpu, _) = run(&[0x37, 0x3E, 0x00, 0x3D, 0x76]);
    assert!(cpu.flags.cy, "DCR must not affect CY");
}

#[test]
fn dad_sets_carry() {
    // LXI H,0xFFFF ; LXI D,0x0001 ; DAD D -> HL=0, CY set
    let (cpu, _) = run(&[0x21, 0xFF, 0xFF, 0x11, 0x01, 0x00, 0x19, 0x76]);
    assert_eq!(cpu.hl(), 0);
    assert!(cpu.flags.cy);
}

#[test]
fn daa_basic() {
    // MVI A,0x2B ; DAA -> 0x31 (classic manual example)
    let (cpu, _) = run(&[0x3E, 0x2B, 0x27, 0x76]);
    assert_eq!(cpu.a, 0x31, "DAA 0x2B -> 0x31, flags: {}", flags_of(&cpu));
}

#[test]
fn daa_carry_out() {
    // MVI A,0x9C ; DAA -> 0x02 with CY set (0x9C + 0x66 = 0x102)
    let (cpu, _) = run(&[0x3E, 0x9C, 0x27, 0x76]);
    assert_eq!(cpu.a, 0x02, "flags: {}", flags_of(&cpu));
    assert!(cpu.flags.cy);
}

#[test]
fn rotates() {
    // MVI A,0x85 ; RLC -> A=0x0B, CY=1
    let (cpu, _) = run(&[0x3E, 0x85, 0x07, 0x76]);
    assert_eq!(cpu.a, 0x0B);
    assert!(cpu.flags.cy);
    // MVI A,0x85 ; RRC -> A=0xC2, CY=1
    let (cpu, _) = run(&[0x3E, 0x85, 0x0F, 0x76]);
    assert_eq!(cpu.a, 0xC2);
    assert!(cpu.flags.cy);
    // MVI A,0x80 ; RAL -> A=0x00, CY=1
    let (cpu, _) = run(&[0x3E, 0x80, 0x17, 0x76]);
    assert_eq!(cpu.a, 0x00);
    assert!(cpu.flags.cy);
    // STC ; MVI A,0x01 ; RAR -> A=0x80, CY=1
    let (cpu, _) = run(&[0x37, 0x3E, 0x01, 0x1F, 0x76]);
    assert_eq!(cpu.a, 0x80);
    assert!(cpu.flags.cy);
}

#[test]
fn parity_flag() {
    // MVI A,0x03 ; ORA A -> parity even (2 bits) -> P set
    let (cpu, _) = run(&[0x3E, 0x03, 0xB7, 0x76]);
    assert!(cpu.flags.p);
    // MVI A,0x07 ; ORA A -> parity odd -> P clear
    let (cpu, _) = run(&[0x3E, 0x07, 0xB7, 0x76]);
    assert!(!cpu.flags.p);
}

#[test]
fn stack_operations() {
    // LXI SP,0x0100 ; MVI A,0x12 ; PUSH PSW ; POP B
    let (cpu, bus) = run(&[
        0x31, 0x00, 0x01, 0x3E, 0x12, 0xF5, 0xC1, 0x76,
    ]);
    assert_eq!(cpu.b, 0x12);
    // PSW pushed at 0x00FE/0x00FF, bit 1 of the flags byte always set
    assert_eq!(bus.mem[0x00FF], 0x12);
    assert_eq!(cpu.c & 0x02, 0x02);
    assert_eq!(bus.mem[0x00FE] & 0x02, 0x02);
}

#[test]
fn call_ret_roundtrip() {
    // program: CALL 0x0100 ; HLT ; at 0x0100: RET
    let mut cpu = Cpu::new();
    let mut bus = RamBus::new();
    bus.load(0x0000, &[0xCD, 0x00, 0x01, 0x76]);
    bus.load(0x0100, &[0xC9]);
    cpu.sp = 0x0200;
    cpu.step(&mut bus); // CALL
    assert_eq!(cpu.pc, 0x0100);
    assert_eq!(cpu.sp, 0x01FE);
    assert_eq!(bus.mem[0x01FE], 0x03); // return address pushed
    cpu.step(&mut bus); // RET
    assert_eq!(cpu.pc, 0x0003);
    assert_eq!(cpu.sp, 0x0200);
}

#[test]
fn conditional_ret_cycles() {
    // XRA A (Z set) ; RZ at 0x0002 -> taken
    let mut cpu = Cpu::new();
    let mut bus = RamBus::new();
    bus.load(0x0000, &[0xAF, 0xC8, 0x76]);
    cpu.sp = 0x0100;
    bus.load(0x0100, &[0x00, 0x00]);
    cpu.step(&mut bus); // XRA A: 4
    let c = cpu.step(&mut bus); // RZ taken: 11
    assert_eq!(c, 11);
    // SET CY ; RNC not taken
    let mut cpu = Cpu::new();
    let mut bus = RamBus::new();
    bus.load(0x0000, &[0x37, 0xD0, 0x76]);
    cpu.step(&mut bus); // STC
    let c = cpu.step(&mut bus); // RNC not taken: 5
    assert_eq!(c, 5);
}

#[test]
fn cycle_counts() {
    // spot-check with single explicit steps (no trailing HLT programs)
    let mut cpu = Cpu::new();
    let mut bus = RamBus::new();
    bus.load(0x0000, &[0x00]); // NOP
    assert_eq!(cpu.step(&mut bus), 4);

    let mut cpu = Cpu::new();
    let mut bus = RamBus::new();
    bus.load(0x0000, &[0x3E, 0x01]); // MVI A
    assert_eq!(cpu.step(&mut bus), 7);

    let mut cpu = Cpu::new();
    let mut bus = RamBus::new();
    bus.load(0x0000, &[0x21, 0x00, 0x10]); // LXI H
    assert_eq!(cpu.step(&mut bus), 10);

    let mut cpu = Cpu::new();
    let mut bus = RamBus::new();
    bus.load(0x0000, &[0x76]); // HLT
    assert_eq!(cpu.step(&mut bus), 7);

    let mut cpu = Cpu::new();
    let mut bus = RamBus::new();
    bus.load(0x0000, &[0xCD, 0x00, 0x00]); // CALL
    cpu.sp = 0x0100;
    assert_eq!(cpu.step(&mut bus), 17);
}

#[test]
fn undocumented_opcodes() {
    // CB = undocumented JMP
    let mut cpu = Cpu::new();
    let mut bus = RamBus::new();
    bus.load(0x0000, &[0xCB, 0x10, 0x00]);
    bus.load(0x0010, &[0x76]); // HLT at the jump target
    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x0010);
    // D9 = undocumented RET
    let mut cpu = Cpu::new();
    let mut bus = RamBus::new();
    bus.load(0x0000, &[0xD9]);
    cpu.sp = 0x0100;
    bus.mem[0x0100] = 0x42;
    bus.mem[0x0101] = 0x00;
    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x0042);
}

#[test]
fn in_out_ports() {
    // IN 0x42 ; OUT 0x43
    let mut cpu = Cpu::new();
    let mut bus = RamBus::new();
    bus.load(0x0000, &[0xDB, 0x42, 0xD3, 0x43, 0x76]);
    bus.in_ports[0x42] = 0x5A;
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.a, 0x5A);
    assert_eq!(bus.out_log, vec![(0x43, 0x5A)]);
}

#[test]
fn psw_roundtrip() {
    let mut cpu = Cpu::new();
    cpu.a = 0xAB;
    cpu.flags = Flags {
        cy: true,
        p: false,
        ac: true,
        z: true,
        s: false,
    };
    let packed = cpu.flags.pack();
    let unpacked = Flags::unpack(packed);
    assert_eq!(cpu.flags, unpacked);
    assert_eq!(packed & 0x02, 0x02, "bit 1 of PSW always set");
    assert_eq!(packed & 0x28, 0, "bits 3 and 5 always clear");
}

#[test]
fn interrupt_sequence() {
    // EI ; NOP ; <- interrupt serviced here ; DI...
    let mut cpu = Cpu::new();
    let mut bus = RamBus::new();
    bus.load(0x0000, &[0xFB, 0x00, 0x00, 0x00, 0x76]);
    cpu.interrupt(0xCF); // RST 1
    cpu.step(&mut bus); // EI
    assert!(cpu.iff);
    cpu.step(&mut bus); // NOP - the EI delay instruction
    let pc = cpu.pc;
    cpu.step(&mut bus); // interrupt accepted: RST 1 -> PC = 0x0008, INTE off
    assert_eq!(cpu.pc, 0x0008);
    assert!(!cpu.iff);
    assert_eq!(cpu.sp, 0xFFFE);
    assert_eq!(bus.mem[0xFFFE], pc as u8);
    assert_eq!(bus.mem[0xFFFF], 0x00);
}

#[test]
fn interrupts_gated_by_iff() {
    let mut cpu = Cpu::new();
    let mut bus = RamBus::new();
    bus.load(0x0000, &[0x00, 0x00, 0x76]);
    cpu.interrupt(0xFF); // RST 7
    cpu.step(&mut bus); // NOP - interrupt must NOT be taken (iff off)
    assert_eq!(cpu.pc, 0x0001);
    assert!(!cpu.halted);
    cpu.iff = true;
    cpu.interrupt(0xFF);
    cpu.step(&mut bus); // now accepted
    assert_eq!(cpu.pc, 0x0038);
}

#[test]
fn interrupt_wakes_halt() {
    let mut cpu = Cpu::new();
    let mut bus = RamBus::new();
    bus.load(0x0000, &[0x76]); // HLT
    cpu.iff = true;
    cpu.step(&mut bus);
    assert!(cpu.halted);
    cpu.interrupt(0xC7); // RST 0
    cpu.step(&mut bus);
    assert!(!cpu.halted);
    assert_eq!(cpu.pc, 0x0000);
}
