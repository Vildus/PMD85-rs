//! Intel 8080 disassembler, Monitor style.
//!
//! Linear, no flow analysis: every address holds a one-to-three-byte
//! instruction, formatted the way the Monitor and its listings do —
//! `MVI A,3E`, `LXI H,2400`, `JMP EDE2`. Registers are A/B/C/D/E/H/L/M
//! (+ SP, PSW), conditions NZ/Z/NC/C/PO/PE/P/M, hex operands uppercase
//! without a prefix (two digits for bytes, four for words).
//!
//! The undocumented opcodes (08/10/18/20/28/30/38, CB, D9, DD, ED, FD)
//! format as data — `DB xx`, one byte — as does any instruction whose
//! operands would run past the end of the buffer.

/// Disassemble one instruction.
///
/// `bytes` is the byte stream starting at `addr`. (The 8080 has no
/// PC-relative operands, so `addr` takes no part in the formatting;
/// it is kept in the signature because callers always have it at
/// hand.) Returns the formatted mnemonic and the instruction's
/// length in bytes. Empty input formats as nothing, length 0.
pub fn disassemble(_addr: u16, bytes: &[u8]) -> (String, usize) {
    let Some(&op) = bytes.first() else {
        return (String::new(), 0);
    };
    // An instruction whose operands run past the end of the buffer
    // is data: nothing past the buffer is ever fetched.
    let len = instruction_len(op);
    if bytes.len() < len {
        return (format!("DB {op:02X}"), 1);
    }
    // Operand accessors — safe, the length guard above guarantees
    // `bytes` is long enough for this instruction.
    let b = || bytes[1];
    let w = || u16::from_le_bytes([bytes[1], bytes[2]]);

    const REGS: [&str; 8] = ["B", "C", "D", "E", "H", "L", "M", "A"];
    const ALU: [&str; 8] = ["ADD", "ADC", "SUB", "SBB", "ANA", "XRA", "ORA", "CMP"];
    const CONDS: [&str; 8] = ["NZ", "Z", "NC", "C", "PO", "PE", "P", "M"];

    let text = match op {
        0x00 => "NOP".into(),
        0x01 => format!("LXI B,{:04X}", w()),
        0x02 => "STAX B".into(),
        0x03 => "INX B".into(),
        0x04 => "INR B".into(),
        0x05 => "DCR B".into(),
        0x06 => format!("MVI B,{:02X}", b()),
        0x07 => "RLC".into(),
        0x09 => "DAD B".into(),
        0x0A => "LDAX B".into(),
        0x0B => "DCX B".into(),
        0x0C => "INR C".into(),
        0x0D => "DCR C".into(),
        0x0E => format!("MVI C,{:02X}", b()),
        0x0F => "RRC".into(),
        0x11 => format!("LXI D,{:04X}", w()),
        0x12 => "STAX D".into(),
        0x13 => "INX D".into(),
        0x14 => "INR D".into(),
        0x15 => "DCR D".into(),
        0x16 => format!("MVI D,{:02X}", b()),
        0x17 => "RAL".into(),
        0x19 => "DAD D".into(),
        0x1A => "LDAX D".into(),
        0x1B => "DCX D".into(),
        0x1C => "INR E".into(),
        0x1D => "DCR E".into(),
        0x1E => format!("MVI E,{:02X}", b()),
        0x1F => "RAR".into(),
        0x21 => format!("LXI H,{:04X}", w()),
        0x22 => format!("SHLD {:04X}", w()),
        0x23 => "INX H".into(),
        0x24 => "INR H".into(),
        0x25 => "DCR H".into(),
        0x26 => format!("MVI H,{:02X}", b()),
        0x27 => "DAA".into(),
        0x29 => "DAD H".into(),
        0x2A => format!("LHLD {:04X}", w()),
        0x2B => "DCX H".into(),
        0x2C => "INR L".into(),
        0x2D => "DCR L".into(),
        0x2E => format!("MVI L,{:02X}", b()),
        0x2F => "CMA".into(),
        0x31 => format!("LXI SP,{:04X}", w()),
        0x32 => format!("STA {:04X}", w()),
        0x33 => "INX SP".into(),
        0x34 => "INR M".into(),
        0x35 => "DCR M".into(),
        0x36 => format!("MVI M,{:02X}", b()),
        0x37 => "STC".into(),
        0x39 => "DAD SP".into(),
        0x3A => format!("LDA {:04X}", w()),
        0x3B => "DCX SP".into(),
        0x3C => "INR A".into(),
        0x3D => "DCR A".into(),
        0x3E => format!("MVI A,{:02X}", b()),
        0x3F => "CMC".into(),
        // 01 DDD SSS: MOV dst,src (0x76 is HLT).
        0x40..=0x75 | 0x77..=0x7F => {
            format!("MOV {},{}", REGS[((op >> 3) & 7) as usize], REGS[(op & 7) as usize])
        }
        0x76 => "HLT".into(),
        // 10 OO O SSS: ALU ops with a register/memory operand.
        0x80..=0xBF => format!("{} {}", ALU[((op >> 3) & 7) as usize], REGS[(op & 7) as usize]),
        0xC1 => "POP B".into(),
        0xC3 => format!("JMP {:04X}", w()),
        0xC5 => "PUSH B".into(),
        0xC6 => format!("ADI {:02X}", b()),
        0xC9 => "RET".into(),
        0xCD => format!("CALL {:04X}", w()),
        0xCE => format!("ACI {:02X}", b()),
        0xD1 => "POP D".into(),
        0xD3 => format!("OUT {:02X}", b()),
        0xDB => format!("IN {:02X}", b()),
        0xD5 => "PUSH D".into(),
        0xD6 => format!("SUI {:02X}", b()),
        0xDE => format!("SBI {:02X}", b()),
        0xE1 => "POP H".into(),
        0xE3 => "XTHL".into(),
        0xE5 => "PUSH H".into(),
        0xE6 => format!("ANI {:02X}", b()),
        0xE9 => "PCHL".into(),
        0xEB => "XCHG".into(),
        0xEE => format!("XRI {:02X}", b()),
        0xF1 => "POP PSW".into(),
        0xF3 => "DI".into(),
        0xF5 => "PUSH PSW".into(),
        0xF6 => format!("ORI {:02X}", b()),
        0xF9 => "SPHL".into(),
        0xFB => "EI".into(),
        0xFE => format!("CPI {:02X}", b()),
        // Undocumented opcodes: data.
        0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 | 0xCB | 0xD9 | 0xDD | 0xED | 0xFD => {
            format!("DB {op:02X}")
        }
        // 11 CRR 000: conditional returns (R NZ, R Z, ... R M).
        o if o & 0xC7 == 0xC0 => format!("R{}", CONDS[((o >> 3) & 7) as usize]),
        // 11 CRR 010: conditional jumps (a literal arm keeps JMP at 0xC3).
        o if o & 0xC7 == 0xC2 => format!("J{} {:04X}", CONDS[((o >> 3) & 7) as usize], w()),
        // 11 CRR 100: conditional calls (CALL 0xCD is a literal arm).
        o if o & 0xC7 == 0xC4 => format!("C{} {:04X}", CONDS[((o >> 3) & 7) as usize], w()),
        // 11 NNN 111: restarts.
        o if o & 0xC7 == 0xC7 => format!("RST {}", (o >> 3) & 7),
        _ => unreachable!("every opcode is covered above"),
    };
    (text, len)
}

/// Instruction length per the 8080 encoding: immediates carry one
/// operand byte, loads/stores/jumps/calls carry a word.
fn instruction_len(op: u8) -> usize {
    match op {
        0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x36 | 0x3E | 0xC6 | 0xCE | 0xD3 | 0xD6
        | 0xDB | 0xDE | 0xE6 | 0xEE | 0xF6 | 0xFE => 2,
        0x01 | 0x11 | 0x21 | 0x22 | 0x2A | 0x31 | 0x32 | 0x3A | 0xC2 | 0xC3 | 0xC4 | 0xCA
        | 0xCC | 0xCD | 0xD2 | 0xD4 | 0xDA | 0xDC | 0xE2 | 0xE4 | 0xEA | 0xEC | 0xF2 | 0xF4
        | 0xFA | 0xFC => 3,
        _ => 1,
    }
}

/// Whether the opcode is a CALL — the unconditional one plus the
/// conditional calls (the debugger's step-over runs over these).
/// The undocumented opcodes that alias the conditional-call bit
/// pattern (DD, ED, FD) are not.
pub fn is_call(op: u8) -> bool {
    op == 0xCD || (op & 0xC7 == 0xC4 && !matches!(op, 0xDD | 0xED | 0xFD))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Disassemble from a fixed instruction stream.
    fn at(op: u8) -> (String, usize) {
        disassemble(0x0100, &[op, 0x12, 0x34])
    }

    /// Golden table: every opcode of the 0x00..=0x3F block (except
    /// the MOV/ALU ranges and the undocumented bytes).
    #[test]
    fn golden_low_block() {
        for (op, text, len) in [
            (0x00, "NOP", 1),
            (0x01, "LXI B,3412", 3),
            (0x02, "STAX B", 1),
            (0x03, "INX B", 1),
            (0x04, "INR B", 1),
            (0x05, "DCR B", 1),
            (0x06, "MVI B,12", 2),
            (0x07, "RLC", 1),
            (0x09, "DAD B", 1),
            (0x0A, "LDAX B", 1),
            (0x0B, "DCX B", 1),
            (0x0C, "INR C", 1),
            (0x0D, "DCR C", 1),
            (0x0E, "MVI C,12", 2),
            (0x0F, "RRC", 1),
            (0x11, "LXI D,3412", 3),
            (0x12, "STAX D", 1),
            (0x13, "INX D", 1),
            (0x14, "INR D", 1),
            (0x15, "DCR D", 1),
            (0x16, "MVI D,12", 2),
            (0x17, "RAL", 1),
            (0x19, "DAD D", 1),
            (0x1A, "LDAX D", 1),
            (0x1B, "DCX D", 1),
            (0x1C, "INR E", 1),
            (0x1D, "DCR E", 1),
            (0x1E, "MVI E,12", 2),
            (0x1F, "RAR", 1),
            (0x21, "LXI H,3412", 3),
            (0x22, "SHLD 3412", 3),
            (0x23, "INX H", 1),
            (0x24, "INR H", 1),
            (0x25, "DCR H", 1),
            (0x26, "MVI H,12", 2),
            (0x27, "DAA", 1),
            (0x29, "DAD H", 1),
            (0x2A, "LHLD 3412", 3),
            (0x2B, "DCX H", 1),
            (0x2C, "INR L", 1),
            (0x2D, "DCR L", 1),
            (0x2E, "MVI L,12", 2),
            (0x2F, "CMA", 1),
            (0x31, "LXI SP,3412", 3),
            (0x32, "STA 3412", 3),
            (0x33, "INX SP", 1),
            (0x34, "INR M", 1),
            (0x35, "DCR M", 1),
            (0x36, "MVI M,12", 2),
            (0x37, "STC", 1),
            (0x39, "DAD SP", 1),
            (0x3A, "LDA 3412", 3),
            (0x3B, "DCX SP", 1),
            (0x3C, "INR A", 1),
            (0x3D, "DCR A", 1),
            (0x3E, "MVI A,12", 2),
            (0x3F, "CMC", 1),
        ] {
            let (got, got_len) = at(op);
            assert_eq!(got, text, "opcode {op:02X}");
            assert_eq!(got_len, len, "opcode {op:02X}");
        }
    }

    /// Golden table for the stack, control, immediate and interrupt
    /// half — including the undocumented opcodes as `DB`.
    #[test]
    fn golden_high_block() {
        for (op, text, len) in [
            (0xC0, "RNZ", 1),
            (0xC1, "POP B", 1),
            (0xC2, "JNZ 3412", 3),
            (0xC3, "JMP 3412", 3),
            (0xC4, "CNZ 3412", 3),
            (0xC5, "PUSH B", 1),
            (0xC6, "ADI 12", 2),
            (0xC7, "RST 0", 1),
            (0xC8, "RZ", 1),
            (0xC9, "RET", 1),
            (0xCA, "JZ 3412", 3),
            (0xCB, "DB CB", 1),
            (0xCC, "CZ 3412", 3),
            (0xCD, "CALL 3412", 3),
            (0xCE, "ACI 12", 2),
            (0xCF, "RST 1", 1),
            (0xD0, "RNC", 1),
            (0xD1, "POP D", 1),
            (0xD2, "JNC 3412", 3),
            (0xD3, "OUT 12", 2),
            (0xD4, "CNC 3412", 3),
            (0xD5, "PUSH D", 1),
            (0xD6, "SUI 12", 2),
            (0xD7, "RST 2", 1),
            (0xD8, "RC", 1),
            (0xD9, "DB D9", 1),
            (0xDA, "JC 3412", 3),
            (0xDB, "IN 12", 2),
            (0xDC, "CC 3412", 3),
            (0xDD, "DB DD", 1),
            (0xDE, "SBI 12", 2),
            (0xDF, "RST 3", 1),
            (0xE0, "RPO", 1),
            (0xE1, "POP H", 1),
            (0xE2, "JPO 3412", 3),
            (0xE3, "XTHL", 1),
            (0xE4, "CPO 3412", 3),
            (0xE5, "PUSH H", 1),
            (0xE6, "ANI 12", 2),
            (0xE7, "RST 4", 1),
            (0xE8, "RPE", 1),
            (0xE9, "PCHL", 1),
            (0xEA, "JPE 3412", 3),
            (0xEB, "XCHG", 1),
            (0xEC, "CPE 3412", 3),
            (0xED, "DB ED", 1),
            (0xEE, "XRI 12", 2),
            (0xEF, "RST 5", 1),
            (0xF0, "RP", 1),
            (0xF1, "POP PSW", 1),
            (0xF2, "JP 3412", 3),
            (0xF3, "DI", 1),
            (0xF4, "CP 3412", 3),
            (0xF5, "PUSH PSW", 1),
            (0xF6, "ORI 12", 2),
            (0xF7, "RST 6", 1),
            (0xF8, "RM", 1),
            (0xF9, "SPHL", 1),
            (0xFA, "JM 3412", 3),
            (0xFB, "EI", 1),
            (0xFC, "CM 3412", 3),
            (0xFD, "DB FD", 1),
            (0xFE, "CPI 12", 2),
            (0xFF, "RST 7", 1),
        ] {
            let (got, got_len) = at(op);
            assert_eq!(got, text, "opcode {op:02X}");
            assert_eq!(got_len, len, "opcode {op:02X}");
        }
    }

    /// MOV and the ALU groups follow the 01 DDD SSS / 10 OO O SSS
    /// encodings; 0x76 in the middle is HLT.
    #[test]
    fn golden_mov_and_alu() {
        let regs = ["B", "C", "D", "E", "H", "L", "M", "A"];
        let alu = ["ADD", "ADC", "SUB", "SBB", "ANA", "XRA", "ORA", "CMP"];
        for dst in 0..8u16 {
            for src in 0..8u16 {
                let op = 0x40 | dst << 3 | src;
                if op == 0x76 {
                    continue; // HLT, checked below
                }
                let (text, len) = at(op as u8);
                assert_eq!(text, format!("MOV {},{}", regs[dst as usize], regs[src as usize]));
                assert_eq!(len, 1);
            }
        }
        let (text, _) = at(0x76);
        assert_eq!(text, "HLT");
        for group in 0..8u16 {
            for src in 0..8u16 {
                let (text, len) = at((0x80 | group << 3 | src) as u8);
                assert_eq!(text, format!("{} {}", alu[group as usize], regs[src as usize]));
                assert_eq!(len, 1);
            }
        }
    }

    /// Instructions whose operands run off the end of the buffer
    /// (or an empty buffer) never read past it.
    #[test]
    fn truncated_instructions_are_data() {
        assert_eq!(disassemble(0, &[]), (String::new(), 0));
        assert_eq!(disassemble(0, &[0x01, 0x34]), ("DB 01".into(), 1));
        assert_eq!(disassemble(0, &[0x21]), ("DB 21".into(), 1));
        // One byte short of a word operand.
        assert_eq!(disassemble(0, &[0xC3, 0x00]), ("DB C3".into(), 1));
        // Exactly enough bytes is fine.
        assert_eq!(disassemble(0, &[0xC3, 0x00, 0x00]), ("JMP 0000".into(), 3));
    }

    /// The plan's formatting examples, verbatim.
    #[test]
    fn plan_examples() {
        assert_eq!(disassemble(0, &[0x3E, 0x3E]).0, "MVI A,3E");
        assert_eq!(disassemble(0, &[0x21, 0x00, 0x24]).0, "LXI H,2400");
        assert_eq!(disassemble(0, &[0xC3, 0xE2, 0xED]).0, "JMP EDE2");
    }

    /// Disassemble a stretch of the real Monitor 3 ROM (loaded at
    /// 0xE000) and compare with a hand-decoded listing: the decimal
    /// parser at 0xEA54 that turns a typed number into a word.
    #[test]
    fn monitor3_decimal_parser_at_ea54() {
        let rom = monitor3();
        let expected = [
            "LXI H,E2B0",
            "SHLD C074",
            "LHLD C072",
            "CALL E0F7",
            "RC",
            "INX H",
            "SHLD C072",
            "CPI 9A",
            "CMC",
            "RC",
            "MOV C,A",
            "RRC",
            "RRC",
            "RRC",
            "RRC",
            "ANI 0F",
            "ADD A",
            "MOV B,A",
            "ADD A",
            "ADD A",
            "ADD B",
            "MOV B,A",
            "MOV A,C",
            "ANI 0F",
            "ADD B",
            "PUSH H",
            "LXI H,C300",
        ];
        disassemble_stretch(&rom, 0xEA54, &expected);
    }

    /// The flash-load block reader at 0xEDC4 — a tight loop of byte
    /// reads, checksummed in B, until DE counts down to zero.
    #[test]
    fn monitor3_block_reader_at_edc4() {
        let rom = monitor3();
        let expected = [
            "PUSH H",
            "MVI B,00",
            "CALL EB6C",
            "JC E577",
            "INR C",
            "DCR C",
            "JZ EDD3",
            "MOV M,A",
            "INX H",
            "ADD B",
            "MOV B,A",
            "MOV A,D",
            "ORA E",
            "DCX D",
            "JNZ EDC7",
            "CALL EB6C",
        ];
        disassemble_stretch(&rom, 0xEDC4, &expected);
    }

    /// The tape byte reader at 0xEB6C.
    #[test]
    fn monitor3_byte_reader_at_eb6c() {
        let rom = monitor3();
        let expected = [
            "PUSH B",
            "PUSH D",
            "PUSH H",
            "LHLD C173",
            "MVI C,26",
            "CALL E895",
            "STC",
            "JZ EB98",
            "MOV A,C",
            "CMP L",
            "JP EB72",
        ];
        disassemble_stretch(&rom, 0xEB6C, &expected);
    }

    fn monitor3() -> Vec<u8> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Rom/monit3.rom"))
            .expect("cannot read the Monitor 3 ROM")
    }

    /// Walk `expected` over the ROM starting at `start`: each entry
    /// must disassemble to exactly that text, and the lengths must
    /// walk the stream in lock-step.
    fn disassemble_stretch(rom: &[u8], start: u16, expected: &[&str]) {
        assert_eq!(rom.len(), 0x2000, "Monitor 3 is 8 KiB");
        let mut addr = start;
        for want in expected {
            let (text, len) = disassemble(addr, &rom[(addr - 0xE000) as usize..]);
            assert_eq!(&text, want, "at {addr:04X}");
            addr += len as u16;
        }
    }

    /// Exhaustive: every opcode of the 256 formats to something
    /// non-empty (catches table arms that fall through to the
    /// unreachable catch-all).
    #[test]
    fn every_opcode_formats() {
        for op in 0u16..=255 {
            let (text, len) = disassemble(0x0100, &[op as u8, 0x12, 0x34]);
            assert!(!text.is_empty(), "opcode {op:02X} formats to nothing");
            assert_eq!(len, instruction_len(op as u8), "opcode {op:02X}");
        }
    }
}
