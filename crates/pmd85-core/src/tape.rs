//! Cassette tape images for the PMD 85 monitor tape interface.
//!
//! A tape is a sequence of *blocks*. Every block on the wire is just a
//! run of bytes; files on the tape start with a 63-byte *header block*
//! followed by a *body block*, and can be followed by headerless
//! *continuation blocks* (pictures, saved game positions) that the
//! program reads after it has been started.
//!
//! Two container formats exist:
//!
//! - **PTP** (used by GPMD85 and this emulator): every block is stored
//!   as `u16 LE length` + payload bytes.
//! - the old **PMD** format: the same blocks with no length prefixes;
//!   header blocks are recognized by their 48-byte leader
//!   (`FF`×16, `00`×16, `55`×16) and bodies by the length in the
//!   preceding header.
//!
//! Block layout of a file header (63 bytes):
//!
//! ```text
//! offset 0..48   leader (FF×16, 00×16, 55×16)
//! offset 48      file number
//! offset 49      block type (ASCII char; headerless blocks have none)
//! offset 50..52  load start address (u16 LE)
//! offset 52..54  wLength (u16 LE) = content length − 1
//! offset 54..62  file name (8 bytes, space padded)
//! offset 62      checksum: sum of bytes 48..62, mod 256
//! ```
//!
//! The body block that follows a header is `wLength + 2` bytes:
//! `wLength + 1` content bytes plus a checksum byte (sum of the
//! content bytes, mod 256). The off-by-one in `wLength` mirrors the
//! monitor ROM, which counts its transfer loop from `wLength + 1`
//! (the `ED6C`/`EDC4` routines of Monitor 3).

use std::path::Path;

/// The 48-byte leader that starts every file header block
/// (`FF`×16, `00`×16, `55`×16 — the monitor's sync pattern).
pub const HEADER_LEADER: [u8; 48] = {
    let mut leader = [0u8; 48];
    let mut i = 0;
    while i < 16 {
        leader[i] = 0xFF;
        leader[16 + i] = 0x00;
        leader[32 + i] = 0x55;
        i += 1;
    }
    leader
};

/// Largest block payload we accept (the PTP length prefix is `u16`).
pub const MAX_BLOCK_SIZE: usize = 0xFFFF;

/// Header fields of a file block (bytes 48..62 of the header block).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHeader {
    /// Sequential file number on the tape.
    pub number: u8,
    /// Block type character (`'B'`, `'A'`, `'M'`, ...).
    pub block_type: u8,
    /// Address the monitor loads the content to.
    pub start: u16,
    /// `wLength`: content length − 1 (the ROM quirk, see module docs).
    pub length: u16,
    /// Space-padded file name.
    pub name: [u8; 8],
}

impl FileHeader {
    /// Content byte count the header describes (`wLength + 1`).
    pub fn content_len(&self) -> usize {
        self.length as usize + 1
    }

    /// The file name as (approximately) typed, trailing spaces trimmed.
    pub fn name_str(&self) -> String {
        let end = self
            .name
            .iter()
            .rposition(|&b| b != b' ')
            .map_or(0, |p| p + 1);
        self.name[..end].iter().map(|&b| b as char).collect()
    }

    /// Decode from the 14 field bytes (header block offset 48..62).
    fn decode(fields: &[u8; 14]) -> FileHeader {
        FileHeader {
            number: fields[0],
            block_type: fields[1],
            start: u16::from_le_bytes([fields[2], fields[3]]),
            length: u16::from_le_bytes([fields[4], fields[5]]),
            name: fields[6..14].try_into().unwrap(),
        }
    }

    /// Encode into the 14 field bytes.
    fn encode(&self) -> [u8; 14] {
        let mut fields = [0u8; 14];
        fields[0] = self.number;
        fields[1] = self.block_type;
        fields[2..4].copy_from_slice(&self.start.to_le_bytes());
        fields[4..6].copy_from_slice(&self.length.to_le_bytes());
        fields[6..14].copy_from_slice(&self.name);
        fields
    }
}

/// One row of the tape browser: a file (header block + body block) or
/// a headerless continuation block.
#[derive(Clone, Debug)]
pub struct TapeBlock {
    /// Header fields; `None` for headerless continuation blocks.
    pub header: Option<FileHeader>,
    /// The full 63-byte header block payload (leader included); empty
    /// for headerless blocks.
    pub header_bytes: Vec<u8>,
    /// The body block payload (content + checksum); for headerless
    /// blocks, the raw block bytes.
    pub body_bytes: Vec<u8>,
    /// The header block checksum matches byte 62.
    pub header_crc_ok: bool,
    /// The body checksum matches the last body byte. Always `true`
    /// for headerless blocks (there is no announced length to verify
    /// the checksum position against).
    pub body_crc_ok: bool,
    /// The body block on tape had a different length than the header
    /// announced (`wLength + 2`); holds the payload length found.
    pub body_length_error: Option<usize>,
    /// Whether the block was read from the old length-less format.
    pub old_format: bool,
}

impl TapeBlock {
    /// The content bytes of the body (payload without the checksum
    /// byte). For headerless blocks, the payload as-is.
    pub fn content(&self) -> &[u8] {
        match self.header {
            Some(_) if !self.body_bytes.is_empty() => {
                &self.body_bytes[..self.body_bytes.len() - 1]
            }
            _ => &self.body_bytes,
        }
    }

    /// Recompute and store both checksums.
    pub fn fix_crcs(&mut self) {
        if self.header_bytes.len() == 63 {
            self.header_bytes[62] = crc8(&self.header_bytes[48..62]);
            self.header_crc_ok = true;
        }
        if !self.body_bytes.is_empty() {
            let n = self.body_bytes.len();
            self.body_bytes[n - 1] = crc8(&self.body_bytes[..n - 1]);
            self.body_crc_ok = true;
        }
    }

    /// Approximate playback duration of the block in seconds
    /// (leader tones and the 1200-baud byte transfer, see `tapedeck`).
    pub fn duration_secs(&self) -> f64 {
        let bytes = (self.header_bytes.len() + self.body_bytes.len()) as f64;
        let leaders = if self.header.is_some() { 2.8 + 0.5 } else { 0.5 };
        leaders + 0.2 + bytes * (22.0 / 2400.0)
    }
}

/// The checksum used by the tape format: byte sum, mod 256.
pub fn crc8(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |a, &b| a.wrapping_add(b))
}

/// Build a file block pair (header + body) from raw content — the
/// tape-image equivalent of the monitor's `MGSV`, used by import.
pub fn make_file(
    number: u8,
    block_type: u8,
    name: &str,
    start: u16,
    content: &[u8],
) -> Result<TapeBlock, String> {
    // wLength = content len − 1 must fit u16.
    if content.is_empty() || content.len() > 0x1_0000 {
        return Err(format!(
            "content length {} out of range 1..=65536 bytes",
            content.len()
        ));
    }
    let mut name_bytes = [b' '; 8];
    for (slot, &b) in name_bytes.iter_mut().zip(name.as_bytes()) {
        *slot = b;
    }
    let header = FileHeader {
        number,
        block_type,
        start,
        length: (content.len() - 1) as u16,
        name: name_bytes,
    };
    let mut header_bytes = Vec::with_capacity(63);
    header_bytes.extend_from_slice(&HEADER_LEADER);
    header_bytes.extend_from_slice(&header.encode());
    header_bytes.push(0); // checksum placeholder
    let mut body_bytes = content.to_vec();
    body_bytes.push(0); // checksum placeholder
    let mut block = TapeBlock {
        header: Some(header),
        header_bytes,
        body_bytes,
        header_crc_ok: true,
        body_crc_ok: true,
        body_length_error: None,
        old_format: false,
    };
    block.fix_crcs();
    Ok(block)
}

/// A parsed tape image.
#[derive(Clone, Debug, Default)]
pub struct Tape {
    /// Rows in tape order.
    pub blocks: Vec<TapeBlock>,
    /// Non-fatal problems found while parsing (bad checksums, length
    /// mismatches, truncated or trailing bytes).
    pub warnings: Vec<String>,
}

impl Tape {
    /// Parse a `.ptp` or old-format tape image. Never fails outright:
    /// unrecognized trailing bytes produce a warning and the parsed
    /// prefix is kept.
    pub fn parse(bytes: &[u8]) -> Tape {
        let mut tape = Tape::default();
        // Once a block established the container flavor, it is sticky
        // for the whole file (mixed files are not a thing).
        let mut old_type: Option<bool> = None;
        let mut pos = 0usize;

        while pos < bytes.len() {
            let remaining = bytes.len() - pos;
            if remaining < 2 {
                tape.warnings
                    .push(format!("{} trailing byte(s) ignored", remaining));
                break;
            }
            let peek = remaining.min(67);
            let w = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);

            // Detect a file header block starting at `pos`:
            // - old format: the leader sits here directly,
            // - PTP: a `u16 == 63` length prefix, then the leader,
            //   then the body block's own length prefix.
            let mut header: Option<(usize, usize, Option<u16>)> = None; // (header_off, body_off, body prefix)
            if peek >= 63 && bytes[pos..pos + 48] == HEADER_LEADER {
                old_type = Some(true);
                header = Some((pos, pos + 63, None));
            } else if peek >= 67 && w == 63 && bytes[pos + 2..pos + 50] == HEADER_LEADER {
                old_type = Some(false);
                header = Some((
                    pos + 2,
                    pos + 2 + 63 + 2,
                    Some(u16::from_le_bytes([bytes[pos + 65], bytes[pos + 66]])),
                ));
            }

            if let Some((hb, body_off, body_prefix)) = header {
                let fields: [u8; 14] = bytes[hb + 48..hb + 62].try_into().unwrap();
                let file_header = FileHeader::decode(&fields);
                let header_crc_ok = crc8(&fields) == bytes[hb + 62];
                let expect = file_header.content_len() + 1;
                let avail = bytes.len().saturating_sub(body_off);

                let (take, mismatch) = match (old_type, body_prefix) {
                    // PTP: the body block is whatever the next length
                    // prefix says (even when it disagrees with the
                    // header, mirroring GPMD85's TapeBrowser).
                    (Some(false), Some(actual)) if (actual as usize) <= avail => {
                        let actual = actual as usize;
                        if actual == expect {
                            (actual, None)
                        } else {
                            (actual, Some(actual))
                        }
                    }
                    // PTP header at the end of the file (no body).
                    (Some(false), _) => (avail, Some(avail)),
                    // Old format: the body follows directly.
                    (Some(true), _) => {
                        if expect <= avail {
                            (expect, None)
                        } else {
                            (avail, Some(avail))
                        }
                    }
                    _ => unreachable!("header implies a container flavor"),
                };

                let body_crc_ok = match mismatch {
                    None => {
                        take > 0 && crc8(&bytes[body_off..body_off + take - 1])
                            == bytes[body_off + take - 1]
                    }
                    Some(_) => true, // unverifiable: unknown split of data/checksum
                };

                tape.blocks.push(TapeBlock {
                    header: Some(file_header),
                    header_bytes: bytes[hb..hb + 63].to_vec(),
                    body_bytes: bytes[body_off..body_off + take].to_vec(),
                    header_crc_ok,
                    body_crc_ok,
                    body_length_error: mismatch,
                    old_format: old_type == Some(true),
                });
                pos = body_off + take;
                continue;
            }

            // No header here: a headerless continuation block.
            match old_type {
                // Old format: everything that follows is one block.
                Some(true) => {
                    if remaining > MAX_BLOCK_SIZE {
                        tape.warnings.push(format!(
                            "old-format continuation at offset {pos} exceeds {} bytes; stopped",
                            MAX_BLOCK_SIZE
                        ));
                        break;
                    }
                    tape.blocks.push(TapeBlock {
                        header: None,
                        header_bytes: Vec::new(),
                        body_bytes: bytes[pos..].to_vec(),
                        header_crc_ok: true,
                        body_crc_ok: true,
                        body_length_error: None,
                        old_format: true,
                    });
                    pos = bytes.len();
                }
                // PTP (or unknown so far): a length-prefixed block.
                _ => {
                    let avail = remaining - 2;
                    if w as usize > avail {
                        tape.warnings.push(format!(
                            "block at offset {pos} truncated ({} of {} bytes)",
                            avail, w
                        ));
                        break;
                    }
                    tape.blocks.push(TapeBlock {
                        header: None,
                        header_bytes: Vec::new(),
                        body_bytes: bytes[pos + 2..pos + 2 + w as usize].to_vec(),
                        header_crc_ok: true,
                        body_crc_ok: true,
                        body_length_error: None,
                        old_format: false,
                    });
                    pos += 2 + w as usize;
                }
            }
        }

        tape
    }

    /// Serialize as PTP (the old format is read-only).
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for block in &self.blocks {
            if !block.header_bytes.is_empty() {
                out.extend_from_slice(&(block.header_bytes.len() as u16).to_le_bytes());
                out.extend_from_slice(&block.header_bytes);
            }
            out.extend_from_slice(&(block.body_bytes.len() as u16).to_le_bytes());
            out.extend_from_slice(&block.body_bytes);
        }
        out
    }

    /// Append the blocks of a PTP byte stream (what the tape deck
    /// recorder produces while the machine saves to tape).
    pub fn append_ptp_stream(&mut self, bytes: &[u8]) {
        let mut parsed = Tape::parse(bytes);
        self.blocks.append(&mut parsed.blocks);
        self.warnings.append(&mut parsed.warnings);
    }

    /// Total payload bytes (blocks without their length prefixes).
    pub fn total_block_bytes(&self) -> usize {
        self.blocks
            .iter()
            .map(|b| b.header_bytes.len() + b.body_bytes.len())
            .sum()
    }

    /// Approximate playback duration of the whole tape in seconds.
    pub fn duration_secs(&self) -> f64 {
        self.blocks.iter().map(|b| b.duration_secs()).sum()
    }
}

/// Read a tape image file (PTP or old format).
pub fn load(path: &Path) -> std::io::Result<Tape> {
    let bytes = std::fs::read(path)?;
    Ok(Tape::parse(&bytes))
}

/// Write a tape image as PTP.
pub fn save(path: &Path, tape: &Tape) -> std::io::Result<()> {
    std::fs::write(path, tape.serialize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_block(number: u8, name: &str, start: u16, content: &[u8]) -> TapeBlock {
        make_file(number, b'B', name, start, content).unwrap()
    }

    /// Serialize an explicit block list (keeps test data literal).
    fn serialize(blocks: &[TapeBlock]) -> Vec<u8> {
        Tape {
            blocks: blocks.to_vec(),
            warnings: Vec::new(),
        }
        .serialize()
    }

    #[test]
    fn leader_pattern() {
        assert_eq!(&HEADER_LEADER[0..16], &[0xFF; 16]);
        assert_eq!(&HEADER_LEADER[16..32], &[0x00; 16]);
        assert_eq!(&HEADER_LEADER[32..48], &[0x55; 16]);
    }

    #[test]
    fn make_file_round_trips_through_parse() {
        let content: Vec<u8> = (0..300u16).map(|i| i as u8).collect();
        let block = file_block(3, "HELLO", 0x2000, &content);
        let tape = Tape::parse(&serialize(std::slice::from_ref(&block)));
        assert_eq!(tape.blocks.len(), 1);
        assert!(tape.warnings.is_empty(), "{:?}", tape.warnings);
        let parsed = &tape.blocks[0];
        assert_eq!(parsed.header.as_ref(), block.header.as_ref());
        assert!(parsed.header_crc_ok);
        assert!(parsed.body_crc_ok);
        assert!(parsed.body_length_error.is_none());
        assert_eq!(parsed.content(), &content[..]);
        assert_eq!(parsed.header.as_ref().unwrap().name_str(), "HELLO");
        assert_eq!(parsed.header.as_ref().unwrap().content_len(), 300);
    }

    #[test]
    fn wlength_is_content_minus_one() {
        let block = file_block(0, "X", 0x1000, &[1, 2, 3, 4]);
        let h = block.header.as_ref().unwrap();
        assert_eq!(h.length, 3);
        assert_eq!(h.content_len(), 4);
        // header layout: number, type, start LE, wLength LE, name, crc
        assert_eq!(block.header_bytes[48], 0);
        assert_eq!(block.header_bytes[49], b'B');
        assert_eq!(block.header_bytes[50..52], 0x1000u16.to_le_bytes());
        assert_eq!(block.header_bytes[52..54], 3u16.to_le_bytes());
        assert_eq!(&block.header_bytes[54..62], b"X       ");
        assert_eq!(block.header_bytes.len(), 63);
        assert_eq!(block.body_bytes.len(), 5); // 4 content + crc
    }

    #[test]
    fn serialization_format() {
        let block = file_block(0, "A", 0x0000, &[0xEE; 10]);
        let bytes = serialize(&[block]);
        // header block: u16 prefix 63 + payload
        assert_eq!(u16::from_le_bytes([bytes[0], bytes[1]]), 63);
        assert_eq!(&bytes[2..50], &HEADER_LEADER[..]);
        // body block: u16 prefix wLength+2 = 11
        assert_eq!(u16::from_le_bytes([bytes[65], bytes[66]]), 11);
        assert_eq!(bytes.len(), 2 + 63 + 2 + 11);
    }

    #[test]
    fn empty_tape_and_garbage() {
        let tape = Tape::parse(&[]);
        assert!(tape.blocks.is_empty());
        assert!(tape.warnings.is_empty());

        let tape = Tape::parse(&[0xAB]);
        assert!(tape.blocks.is_empty());
        assert_eq!(tape.warnings.len(), 1);
    }

    #[test]
    fn headerless_ptp_blocks() {
        let bytes = [4u8, 0, 1, 2, 3, 4, 2, 0, 9, 9];
        let tape = Tape::parse(&bytes);
        assert_eq!(tape.blocks.len(), 2);
        assert!(tape.blocks.iter().all(|b| b.header.is_none()));
        assert_eq!(tape.blocks[0].body_bytes, vec![1, 2, 3, 4]);
        assert_eq!(tape.blocks[1].body_bytes, vec![9, 9]);
        assert_eq!(tape.blocks[0].content(), &[1, 2, 3, 4]);
        // round-trips
        let again = Tape::parse(&tape.serialize());
        assert_eq!(again.blocks.len(), 2);
        assert_eq!(again.blocks[1].body_bytes, vec![9, 9]);
    }

    #[test]
    fn truncated_ptp_block_warns() {
        let bytes = [10u8, 0, 1, 2, 3];
        let tape = Tape::parse(&bytes);
        assert!(tape.blocks.is_empty());
        assert_eq!(tape.warnings.len(), 1);
        assert!(tape.warnings[0].contains("truncated"));
    }

    #[test]
    fn bad_crcs_flagged_and_fixable() {
        let mut block = file_block(7, "BAD", 0x8000, &[5; 100]);
        block.header_bytes[62] ^= 0x55;
        block.body_bytes[100] ^= 0xAA;
        let tape = Tape::parse(&serialize(&[block]));
        let parsed = &tape.blocks[0];
        assert!(!parsed.header_crc_ok);
        assert!(!parsed.body_crc_ok);

        let mut fixed = parsed.clone();
        fixed.fix_crcs();
        assert!(fixed.header_crc_ok);
        assert!(fixed.body_crc_ok);
        let re = Tape::parse(&serialize(&[fixed]));
        assert!(re.blocks[0].header_crc_ok);
        assert!(re.blocks[0].body_crc_ok);
    }

    #[test]
    fn old_format_detected_and_upgraded() {
        let block = file_block(1, "OLD", 0x1234, &[7; 20]);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&block.header_bytes); // no length prefix
        bytes.extend_from_slice(&block.body_bytes);
        let tape = Tape::parse(&bytes);
        assert_eq!(tape.blocks.len(), 1);
        assert!(tape.blocks[0].old_format);
        assert_eq!(tape.blocks[0].body_bytes, block.body_bytes);
        // Serialized as PTP now:
        let ptp = tape.serialize();
        let again = Tape::parse(&ptp);
        assert_eq!(again.blocks.len(), 1);
        assert!(!again.blocks[0].old_format);
        assert!(again.blocks[0].header_crc_ok);
        assert_eq!(again.blocks[0].body_bytes, block.body_bytes);
    }

    #[test]
    fn old_format_trailing_headerless_swallowed() {
        let block = file_block(2, "T", 0x0000, &[9; 8]);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&block.header_bytes);
        bytes.extend_from_slice(&block.body_bytes);
        bytes.extend_from_slice(&[1, 2, 3, 4, 5]); // stray continuation
        let tape = Tape::parse(&bytes);
        assert_eq!(tape.blocks.len(), 2);
        assert!(tape.blocks[1].header.is_none());
        assert!(tape.blocks[1].old_format);
        assert_eq!(tape.blocks[1].body_bytes, &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn body_length_mismatch_flagged() {
        let block = file_block(0, "MM", 0x0000, &[0x11; 16]);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&63u16.to_le_bytes());
        bytes.extend_from_slice(&block.header_bytes);
        // but store a shorter body block: prefix says 5, then 5 bytes
        bytes.extend_from_slice(&5u16.to_le_bytes());
        bytes.extend_from_slice(&[1, 2, 3, 4, 5]);
        let tape = Tape::parse(&bytes);
        assert_eq!(tape.blocks.len(), 1);
        let parsed = &tape.blocks[0];
        assert_eq!(parsed.body_length_error, Some(5));
        assert_eq!(parsed.body_bytes, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn continuation_blocks_after_files() {
        // A realistic file + picture block sequence, PTP.
        let file = file_block(0, "GAME", 0x0500, &vec![0u8; 512]);
        let mut bytes = serialize(&[file]);
        // picture continuation block
        bytes.extend_from_slice(&(1024u16).to_le_bytes());
        bytes.extend(std::iter::repeat_n(0xA5, 1024));
        let tape = Tape::parse(&bytes);
        assert_eq!(tape.blocks.len(), 2);
        assert!(tape.blocks[1].header.is_none());
        assert_eq!(tape.blocks[1].body_bytes.len(), 1024);
        assert!(tape.duration_secs() > 5.0);
        assert_eq!(tape.total_block_bytes(), 63 + 513 + 1024);
    }

    #[test]
    fn header_without_body_round_trips() {
        let mut block = file_block(0, "HB", 0x0000, &[1; 4]);
        block.body_bytes.clear();
        let bytes = serialize(&[block]);
        let tape = Tape::parse(&bytes);
        assert_eq!(tape.blocks.len(), 1);
        assert_eq!(tape.blocks[0].body_bytes.len(), 0);
        assert_eq!(tape.blocks[0].body_length_error, Some(0));
        let again = Tape::parse(&tape.serialize());
        assert_eq!(again.blocks.len(), 1);
        assert_eq!(again.blocks[0].body_bytes.len(), 0);
    }

    #[test]
    fn append_ptp_stream_from_recorder() {
        let file = file_block(5, "REC", 0x2000, &[0x77; 32]);
        let stream = serialize(&[file]);
        let mut tape = Tape::default();
        tape.append_ptp_stream(&stream);
        assert_eq!(tape.blocks.len(), 1);
        assert_eq!(tape.blocks[0].header.as_ref().unwrap().name_str(), "REC");
    }

    #[test]
    fn make_file_validates_length() {
        assert!(make_file(0, b'B', "X", 0, &[]).is_err());
        assert!(make_file(0, b'B', "X", 0, &vec![0; 0x1_0001]).is_err());
        assert!(make_file(0, b'B', "X", 0, &vec![0; 0x1_0000]).is_ok());
    }

    #[test]
    fn long_name_truncated_to_eight() {
        let block = file_block(0, "VERYLONGNAME", 0, &[1]);
        assert_eq!(&block.header.as_ref().unwrap().name, b"VERYLONG");
    }

    #[test]
    fn duration_estimate_is_sane() {
        let block = file_block(0, "D", 0, &[0; 700]); // ~6.4 s of data
        let secs = block.duration_secs();
        assert!(secs > 9.0 && secs < 13.0, "{secs}");
    }

    /// Round-trip against the real tape sample if it is present
    /// (downloaded during development; the test self-skips).
    #[test]
    fn real_tape_sample_round_trip() {
        let path = std::path::Path::new("/tmp/opencode/ptp_sample/games-vbg.ptp");
        let Ok(bytes) = std::fs::read(path) else {
            return; // sample not available
        };
        let tape = Tape::parse(&bytes);
        assert!(
            tape.blocks.len() >= 30,
            "expected a full tape, got {} blocks",
            tape.blocks.len()
        );
        assert!(
            tape.blocks
                .iter()
                .filter(|b| b.header.is_some())
                .all(|b| b.header_crc_ok),
            "header checksum failure on the real tape"
        );
        // The sample contains one genuinely corrupt body block
        // (stored checksum 0x00, "BOULDER"); it must be flagged, and
        // it must be the only one.
        let bad: Vec<_> = tape
            .blocks
            .iter()
            .filter(|b| b.header.is_some() && !b.body_crc_ok)
            .collect();
        assert_eq!(bad.len(), 1, "unexpected body checksum failures");
        assert_eq!(bad[0].header.as_ref().unwrap().name_str(), "BOULDER");
        // PTP round-trip is byte-stable.
        assert_eq!(tape.serialize(), bytes);
        // The first file is a numbered, named block.
        let first = tape.blocks[0].header.as_ref().unwrap();
        assert_eq!(first.number, 0);
        assert!(!first.name_str().is_empty());
    }
}
