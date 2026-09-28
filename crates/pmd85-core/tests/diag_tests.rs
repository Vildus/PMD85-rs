//! Integration tests running the classic CP/M 8080 diagnostic programs
//! (8080PRE, TST8080, CPUTEST, 8080EXM) against the CPU core on a minimal
//! CP/M harness: program loaded at 0x0100, BDOS entry at 0x0005
//! (console output collected), warm boot at 0x0000 ends the run.

use pmd85_core::cpu::{Bus, Cpu};

const DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/");

/// Minimal CP/M machine.
struct CpmBus {
    mem: Vec<u8>,
    output: Vec<u8>,
}

impl CpmBus {
    fn new(program: &[u8]) -> Self {
        let mut mem = vec![0u8; 0x10000];
        mem[0x0100..0x0100 + program.len()].copy_from_slice(program);
        CpmBus { mem, output: Vec::new() }
    }
}

impl Bus for CpmBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.mem[addr as usize]
    }
    fn write(&mut self, addr: u16, data: u8) {
        self.mem[addr as usize] = data;
    }
    fn port_in(&mut self, _port: u8) -> u8 {
        0xFF
    }
    fn port_out(&mut self, _port: u8, _data: u8) {}
}

struct Cpm {
    cpu: Cpu,
    bus: CpmBus,
    finished: bool,
}

impl Cpm {
    fn new(program: &[u8]) -> Self {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.sp = 0xFF00;
        Cpm {
            cpu,
            bus: CpmBus::new(program),
            finished: false,
        }
    }

    fn output(&self) -> String {
        String::from_utf8_lossy(&self.bus.output).to_string()
    }

    /// Handle a BDOS call at PC=5: C=2 prints E, C=9 prints a '$'-terminated
    /// string at DE. Simulates RET afterwards.
    fn bdos(&mut self) {
        match self.cpu.c {
            2 => self.bus.output.push(self.cpu.e),
            9 => {
                let mut addr = self.cpu.de();
                loop {
                    let ch = self.bus.mem[addr as usize];
                    if ch == b'$' {
                        break;
                    }
                    self.bus.output.push(ch);
                    addr = addr.wrapping_add(1);
                }
            }
            _ => {}
        }
        // RET
        let sp = self.cpu.sp as usize;
        let lo = self.bus.mem[sp] as u16;
        let hi = self.bus.mem[sp + 1] as u16;
        self.cpu.pc = (hi << 8) | lo;
        self.cpu.sp = self.cpu.sp.wrapping_add(2);
    }

    fn run(&mut self, max_cycles: u64) {
        let mut cycles = 0u64;
        while !self.finished && cycles < max_cycles && !self.cpu.halted {
            if self.cpu.pc == 0x0005 {
                self.bdos();
                continue;
            }
            if self.cpu.pc == 0x0000 {
                self.finished = true;
                return;
            }
            cycles += self.cpu.step(&mut self.bus) as u64;
        }
    }
}

fn run_diagnostic(name: &str, max_cycles: u64) -> String {
    let program = std::fs::read(format!("{DATA_DIR}{name}"))
        .unwrap_or_else(|e| panic!("cannot read {name}: {e}"));
    let mut cpm = Cpm::new(&program);
    cpm.run(max_cycles);
    let out = cpm.output();
    print!("{out}");
    out
}

#[test]
fn diagnostic_8080pre() {
    let out = run_diagnostic("8080PRE.COM", 100_000_000);
    assert!(
        out.contains("Preliminary tests complete"),
        "8080PRE did not finish successfully:\n{out}"
    );
    assert!(!out.contains("ERROR"), "8080PRE reported an error:\n{out}");
}

#[test]
fn diagnostic_tst8080() {
    let out = run_diagnostic("TST8080.COM", 500_000_000);
    assert!(
        out.contains("CPU IS OPERATIONAL"),
        "TST8080 did not pass:\n{out}"
    );
}

#[test]
fn diagnostic_cputest() {
    let out = run_diagnostic("CPUTEST.COM", 5_000_000_000);
    assert!(
        out.contains("CPU TESTS OK"),
        "CPUTEST did not pass:\n{out}"
    );
}

/// The full exerciser takes a long time even natively; only run with
/// `cargo test -- --ignored` (best in release mode).
#[test]
#[ignore]
fn diagnostic_8080exm() {
    let out = run_diagnostic("8080EXM.COM", 200_000_000_000);
    assert!(out.contains("Tests complete"), "8080EXM did not finish:\n{out}");
    assert!(!out.contains("fail"), "8080EXM reported failure:\n{out}");
}
