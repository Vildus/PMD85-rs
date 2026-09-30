//! Save-state serialization: a minimal little-endian byte writer and
//! reader shared by the snapshot methods of the CPU, the memory
//! subsystem and the peripheral chips.
//!
//! The format itself lives in [`crate::machine`] ([`Machine::save_state`]
//! / [`Machine::restore_state`]); this module only provides the
//! byte-level plumbing and the error type, so the emulation core stays
//! dependency-free.

use std::fmt;

/// Everything that can go wrong taking or loading a save state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateError {
    /// The data does not start with the `PMDS` magic.
    BadMagic,
    /// Written by an incompatible format version.
    UnsupportedVersion(u32),
    /// Written for a different PMD 85 model than the target machine.
    ModelMismatch,
    /// The monitor ROM differs from the one the target machine runs.
    MonitorMismatch,
    /// The cassette deck is playing or recording; states can only be
    /// taken while it is idle.
    TapeBusy,
    /// The data ends in the middle of a section.
    UnexpectedEof,
    /// The data has unread bytes after the last section.
    TrailingBytes,
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::BadMagic => write!(f, "not a PMD 85 save state"),
            StateError::UnsupportedVersion(v) => {
                write!(f, "save state format version {v} is not supported")
            }
            StateError::ModelMismatch => write!(f, "the state is for a different PMD 85 model"),
            StateError::MonitorMismatch => {
                write!(f, "the state was taken with a different monitor ROM")
            }
            StateError::TapeBusy => {
                write!(f, "the tape is playing or recording; stop it first")
            }
            StateError::UnexpectedEof => write!(f, "the save state is truncated"),
            StateError::TrailingBytes => write!(f, "the save state has trailing garbage"),
        }
    }
}

impl std::error::Error for StateError {}

/// Binary snapshot builder (little-endian).
pub(crate) struct StateWriter {
    buf: Vec<u8>,
}

impl StateWriter {
    pub(crate) fn new() -> Self {
        StateWriter { buf: Vec::new() }
    }

    pub(crate) fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub(crate) fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn bool(&mut self, v: bool) {
        self.buf.push(v as u8);
    }

    /// Raw bytes without a length prefix (fixed-size payloads).
    pub(crate) fn bytes(&mut self, v: &[u8]) {
        self.buf.extend_from_slice(v);
    }

    /// Length prefix (u32) of an upcoming variable-size section.
    pub(crate) fn len(&mut self, len: usize) {
        self.u32(len as u32);
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// Binary snapshot parser (little-endian). Every read advances; a read
/// past the end fails with [`StateError::UnexpectedEof`].
pub(crate) struct StateReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> StateReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        StateReader { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], StateError> {
        if self.pos + n > self.data.len() {
            return Err(StateError::UnexpectedEof);
        }
        let out = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, StateError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, StateError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, StateError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, StateError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub(crate) fn bool(&mut self) -> Result<bool, StateError> {
        Ok(self.take(1)?[0] != 0)
    }

    /// `n` raw bytes (fixed-size payloads) as a borrowed slice.
    pub(crate) fn take_bytes(&mut self, n: usize) -> Result<&'a [u8], StateError> {
        self.take(n)
    }

    /// `n` raw bytes into `out` (fixed-size payloads).
    pub(crate) fn read_into(&mut self, out: &mut [u8]) -> Result<(), StateError> {
        out.copy_from_slice(self.take(out.len())?);
        Ok(())
    }

    /// Length-prefixed (u32) byte vector.
    pub(crate) fn vec(&mut self) -> Result<Vec<u8>, StateError> {
        let len = self.u32()? as usize;
        Ok(self.take(len)?.to_vec())
    }

    /// The length prefix of an upcoming section.
    pub(crate) fn len(&mut self) -> Result<usize, StateError> {
        Ok(self.u32()? as usize)
    }

    /// Whether every byte has been consumed.
    pub(crate) fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }
}
