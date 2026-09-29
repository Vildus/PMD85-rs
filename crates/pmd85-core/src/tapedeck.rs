//! The cassette deck: playback signal generation and recording.
//!
//! This is a faithful port of Roman Borik's GPMD85 emulator tape
//! interface (`IifTapePMD85.cpp`, `IifTape.cpp`) for the IRPS/"V2"
//! interface used by the PMD 85-2/3 monitors:
//!
//! - The tape runs at a 1200 Hz bit clock; every *half-clock*
//!   (853 CPU cycles at 1× speed) the DSR line carries
//!   `tape_clock XOR data_bit`. Monitor 3 demodulates this bit-banged
//!   signal through the 8251 status register (port 0x1F, bit 7) —
//!   it even measures the leader frequency to time its sampling.
//! - One byte is framed as 2 half-clocks start bit, 16 half-clocks of
//!   data (LSB first, one bit per 2 half-clocks) and 4 half-clocks
//!   stop bit: 22 half-clocks in total.
//! - Blocks are introduced by leader tones: 2.8 s before a file
//!   header (plus a 1.5 s gap when continuing from a previous block),
//!   0.5 s before a body/continuation block, and 0.2 s of silence
//!   (steady DSR) after a body.
//! - Recording sniffs the bytes the CPU writes to the 8251
//!   transmitter: 16×`FF`, 16×`00`, 16×`55` announce a file header,
//!   followed by the 15 header bytes and a body of `wLength + 2`
//!   bytes. Further bytes within 0.2 s of each other continue as
//!   extra headerless blocks until 2 s of silence finalize the save.
//!
//! A `speed` multiplier fast-forwards the deck in both directions:
//! playback feeds the signal faster, and the ROM's save routine is
//! throttled proportionally less (at [`MAX_SPEED`] the transmitter is
//! never gated, making saves effectively instant). Playback above
//! ~8× relies on the monitor's adaptive clock recovery staying within
//! its polling granularity; MAX is best-effort.

use crate::audio::SpeakerEdge;
use crate::chips::usart8251::I8251;
use crate::machine::CPU_CLOCK_HZ;

/// Tape bit clock in Hz (IRPS).
pub const TAPE_FREQ: u64 = 1200;
/// CPU cycles per half-clock at 1× speed (853 at 2.048 MHz).
pub const HALF_CLOCK: f64 = (CPU_CLOCK_HZ / TAPE_FREQ) as f64 / 2.0;
/// Highest fast-forward multiplier. At this speed the transmitter
/// throttle is removed entirely.
pub const MAX_SPEED: f64 = 16.0;

// State durations, in half-clocks (see the module docs).
const TC_HEAD_LEADER: i64 = 6720; // 2.8 s
const TC_BODY_LEADER: i64 = 1200; // 0.5 s
const TC_STOP_TAIL: i64 = 480; // 0.2 s
const TC_GAP_SIZE: i64 = 3600; // 1.5 s
const TC_START: i64 = 2;
const TC_DATA: i64 = 16;
const TC_STOP: i64 = 4;
const TC_EB_GAP: i64 = 480; // 0.2 s between extended-body bytes
const TC_EB_MAX: i64 = 4800; // 2.0 s to finalize a save
/// Half-clocks per serialized byte (2 start + 16 data + 4 stop).
const HALF_CLOCKS_PER_BYTE: f64 = 22.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RxState {
    Idle,
    Gap,
    Leader,
    Start,
    Data,
    Stop,
    Tail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TxState {
    /// Waiting for the 16×FF leader (idle).
    Ff,
    Zero,
    Lead55,
    /// Collecting the 63 header bytes.
    Head,
    /// Collecting the body block.
    Body,
    /// Body complete, waiting to see whether more bytes follow.
    WaitEb,
    /// Collecting an extra headerless block.
    ExtBody,
}

/// The cassette deck attached to the 8251.
pub struct TapeDeck {
    /// Emulated cycles at the last `tick`.
    cycle: u64,
    /// Fast-forward multiplier (1.0 = real time).
    speed: f64,
    /// Fractional half-clocks accumulated since the last one fired.
    half_clocks: f64,
    /// The 1200 Hz tape clock phase.
    clk: bool,

    // ----- playback -----
    rx_state: RxState,
    /// Half-clocks left in the current playback sub-state.
    rx_tick: i64,
    /// The block currently being fed.
    data: Vec<u8>,
    /// Next byte of `data` to load into the serializer.
    pos: usize,
    /// Serializer shifter and current data bit.
    byte: u8,
    bit: bool,
    /// Whether `data` is a file header block (followed by a body
    /// without a tail; see the module docs).
    head: bool,
    /// The current block finished; the host advances to the next one.
    finished: bool,

    // ----- recording -----
    tx_state: TxState,
    /// Bytes stored in the working buffer so far (the first two bytes
    /// are the PTP length prefix under construction).
    tx_counter: usize,
    /// End of the body block in the working buffer.
    tx_body_end: usize,
    /// Half-clocks until the current recorder timeout fires.
    tx_tick: i64,
    /// Working buffer: complete PTP blocks accumulate here.
    buff: Vec<u8>,
    /// Completed PTP bytes saved since the last drain.
    stream: Vec<u8>,

    // ----- transmitter throttle -----
    /// Cycle at which TxRDY is released again.
    tx_ready_cycle: u64,
    tx_gated: bool,

    // ----- sound monitor -----
    /// Last DSR level driven to the 8251.
    dsr: bool,
    /// Last level emitted to the monitor edge log (false = silence).
    monitor_level: bool,
    monitor_edges: Vec<SpeakerEdge>,
}

impl Default for TapeDeck {
    fn default() -> Self {
        Self::new()
    }
}

impl TapeDeck {
    pub fn new() -> Self {
        TapeDeck {
            cycle: 0,
            speed: 1.0,
            half_clocks: 0.0,
            clk: true,
            rx_state: RxState::Idle,
            rx_tick: 0,
            data: Vec::new(),
            pos: 0,
            byte: 0,
            bit: true,
            head: false,
            finished: false,
            tx_state: TxState::Ff,
            tx_counter: 2,
            tx_body_end: 0,
            tx_tick: 0,
            buff: Vec::new(),
            stream: Vec::new(),
            tx_ready_cycle: 0,
            tx_gated: false,
            dsr: true,
            monitor_level: false,
            monitor_edges: Vec::new(),
        }
    }

    /// Full reset (machine reset): stop playback and abandon any
    /// half-finished recording.
    pub fn hard_reset(&mut self, uart: &mut I8251) {
        self.stop(uart);
        self.rec_reset();
        self.clk = true;
        self.half_clocks = 0.0;
        self.tx_gated = false;
        uart.set_tx_ready(true);
    }

    /// Stop playback (the recorder keeps listening, like a real
    /// tape recorder left in record-standby).
    pub fn stop(&mut self, uart: &mut I8251) {
        self.rx_state = RxState::Idle;
        self.data.clear();
        self.pos = 0;
        self.finished = false;
        self.set_quiet(uart);
    }

    /// Start feeding a block to the machine.
    ///
    /// - `head = true`: the block is a file header block; `on_play`
    ///   selects the fresh-play leader (2.8 s) versus the
    ///   continuing-from-previous-block sequence (1.5 s gap + 2.8 s
    ///   leader).
    /// - `head = false`: a body or continuation block, introduced by
    ///   the 0.5 s leader.
    ///
    /// An empty block finishes immediately instead of stalling.
    pub fn play_block(&mut self, data: &[u8], head: bool, on_play: bool) {
        self.finished = false;
        if data.is_empty() {
            self.rx_state = RxState::Idle;
            self.data.clear();
            self.finished = true;
            return;
        }
        self.data = data.to_vec();
        self.pos = 0;
        self.head = head;
        self.bit = true;
        if head {
            if on_play {
                self.rx_state = RxState::Leader;
                self.rx_tick = TC_HEAD_LEADER;
            } else {
                self.rx_state = RxState::Gap;
                self.rx_tick = TC_GAP_SIZE;
            }
        } else {
            self.rx_state = RxState::Leader;
            self.rx_tick = TC_BODY_LEADER;
        }
    }

    /// Whether a block is being fed.
    pub fn is_playing(&self) -> bool {
        self.rx_state != RxState::Idle
    }

    /// Whether the current block just finished (the host advances to
    /// the next block then). Drained by reading.
    pub fn take_block_finished(&mut self) -> bool {
        std::mem::take(&mut self.finished)
    }

    /// Playback progress through the current block: bytes fed so far
    /// and the total.
    pub fn progress(&self) -> Option<(usize, usize)> {
        if self.rx_state == RxState::Idle {
            None
        } else {
            Some((self.pos, self.data.len()))
        }
    }

    /// Whether the machine is saving to tape (the recorder has
    /// matched a leader or is collecting/finalizing blocks).
    pub fn is_recording(&self) -> bool {
        !matches!(self.tx_state, TxState::Ff)
    }

    /// Set the fast-forward multiplier (clamped to 1..=MAX_SPEED).
    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.clamp(1.0, MAX_SPEED);
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }

    /// Take the blocks recorded since the last call, as a PTP byte
    /// stream (ready for `Tape::append_ptp_stream`).
    pub fn take_recorded(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.stream)
    }

    /// Take the monitor (tape sound) edges recorded since the last
    /// call.
    pub fn take_monitor_edges(&mut self) -> Vec<SpeakerEdge> {
        std::mem::take(&mut self.monitor_edges)
    }

    /// A CPU write to the 8251 transmit data register.
    pub fn on_tx_byte(&mut self, val: u8, uart: &mut I8251) {
        // Pace the transmitter: one byte time at tape speed.
        let per_byte = HALF_CLOCKS_PER_BYTE * HALF_CLOCK / self.speed;
        uart.set_tx_ready(false);
        self.tx_gated = true;
        self.tx_ready_cycle = self.cycle + per_byte.ceil() as u64;

        // Bytes written while playing mean the machine is not saving
        // (or the user is doing something exotic): drop the match.
        if self.rx_state != RxState::Idle {
            self.rec_reset();
            return;
        }
        if !self.rec_byte(val) {
            self.rec_reset();
        }
    }

    /// Advance the deck by `cycles` emulated CPU cycles.
    pub fn tick(&mut self, cycles: u64, uart: &mut I8251) {
        self.cycle += cycles;
        if self.tx_gated && self.cycle >= self.tx_ready_cycle {
            self.tx_gated = false;
            uart.set_tx_ready(true);
        }
        self.half_clocks += cycles as f64 * self.speed / HALF_CLOCK;
        while self.half_clocks >= 1.0 {
            self.half_clocks -= 1.0;
            self.clk = !self.clk;
            self.half_clock(uart);
        }
    }

    fn half_clock(&mut self, uart: &mut I8251) {
        // Drive the DSR line with the tape signal while playing.
        if !matches!(self.rx_state, RxState::Idle | RxState::Gap) {
            let level = self.clk ^ self.bit;
            if level != self.dsr {
                self.dsr = level;
                uart.set_dsr(level);
                self.monitor_edge(level);
            }
        } else if self.recording_tone_gate() {
            // Recording: an approximate 1200 Hz tone while the ROM is
            // actively writing blocks (silence between them).
            self.monitor_edge(!self.monitor_level);
        }

        // Recorder timeouts (GPMD85 returns from the half-clock
        // service while in these states).
        match self.tx_state {
            TxState::WaitEb => {
                self.tx_tick -= 1;
                if self.tx_tick == 0 {
                    if self.tx_counter > 2 {
                        self.flush_ext_block();
                    }
                    self.rec_reset();
                }
                return;
            }
            TxState::ExtBody => {
                self.tx_tick -= 1;
                if self.tx_tick == 0 {
                    self.flush_ext_block();
                    self.tx_state = TxState::WaitEb;
                    self.tx_counter = 2;
                    self.tx_tick = TC_EB_MAX - TC_EB_GAP;
                }
                return;
            }
            _ => {}
        }

        // Playback state machine.
        match self.rx_state {
            RxState::Idle => {}
            RxState::Gap => {
                self.rx_tick -= 1;
                if self.rx_tick == 0 {
                    self.rx_state = RxState::Leader;
                    self.rx_tick = TC_HEAD_LEADER;
                    self.set_quiet(uart);
                }
            }
            RxState::Leader => {
                self.rx_tick -= 1;
                if self.rx_tick == 0 {
                    self.rx_state = RxState::Start;
                    self.rx_tick = TC_START;
                    self.bit = false;
                }
            }
            RxState::Start => {
                self.rx_tick -= 1;
                if self.rx_tick == 0 {
                    self.rx_state = RxState::Data;
                    self.rx_tick = TC_DATA;
                    self.byte = self.data[self.pos];
                    self.pos += 1;
                    self.bit = self.byte & 1 != 0;
                }
            }
            RxState::Data => {
                self.rx_tick -= 1;
                if self.rx_tick == 0 {
                    self.rx_state = RxState::Stop;
                    self.rx_tick = TC_STOP;
                    self.bit = true;
                } else if self.rx_tick & 1 == 0 {
                    self.byte >>= 1;
                    self.bit = self.byte & 1 != 0;
                }
            }
            RxState::Stop => {
                self.rx_tick -= 1;
                if self.rx_tick == 0 {
                    if self.pos == self.data.len() {
                        if self.head {
                            // A header block runs straight into its
                            // body; the host starts it right away.
                            self.rx_state = RxState::Idle;
                            self.finished = true;
                        } else {
                            self.rx_state = RxState::Tail;
                            self.rx_tick = TC_STOP_TAIL;
                            self.set_quiet(uart);
                        }
                    } else {
                        self.rx_state = RxState::Start;
                        self.rx_tick = TC_START;
                        self.bit = false;
                    }
                }
            }
            RxState::Tail => {
                self.rx_tick -= 1;
                if self.rx_tick == 0 {
                    self.rx_state = RxState::Idle;
                    self.finished = true;
                }
            }
        }
    }

    /// DSR quiet (mark) level; also silences the sound monitor.
    fn set_quiet(&mut self, uart: &mut I8251) {
        self.dsr = true;
        uart.set_dsr(true);
        self.monitor_edge(false);
    }

    fn monitor_edge(&mut self, level: bool) {
        if level != self.monitor_level {
            self.monitor_level = level;
            self.monitor_edges.push(SpeakerEdge {
                cycle: self.cycle,
                level,
            });
        }
    }

    fn recording_tone_gate(&self) -> bool {
        matches!(self.tx_state, TxState::Head | TxState::Body | TxState::ExtBody)
    }

    // ----- recorder -----

    fn rec_reset(&mut self) {
        self.tx_state = TxState::Ff;
        self.tx_counter = 2;
    }

    fn rec_byte(&mut self, val: u8) -> bool {
        if self.buff.len() <= self.tx_counter {
            self.buff.resize(self.tx_counter + 1, 0);
        }
        self.buff[self.tx_counter] = val;
        match self.tx_state {
            TxState::Ff => {
                if val == 0xFF {
                    self.tx_counter += 1;
                    if self.tx_counter == 18 {
                        self.tx_state = TxState::Zero;
                    }
                    true
                } else {
                    false
                }
            }
            TxState::Zero => {
                if val == 0x00 {
                    self.tx_counter += 1;
                    if self.tx_counter == 34 {
                        self.tx_state = TxState::Lead55;
                    }
                    true
                } else {
                    false
                }
            }
            TxState::Lead55 => {
                if val == 0x55 {
                    self.tx_counter += 1;
                    if self.tx_counter == 50 {
                        self.tx_state = TxState::Head;
                    }
                    true
                } else {
                    false
                }
            }
            TxState::Head => {
                self.tx_counter += 1;
                if self.tx_counter == 65 {
                    // 63 header bytes collected: emit the header
                    // block prefix, then prepare the body block.
                    let w_length =
                        u16::from_le_bytes([self.buff[54], self.buff[55]]);
                    let len = w_length as usize + 2;
                    self.buff[0..2].copy_from_slice(&63u16.to_le_bytes());
                    self.buff.resize(67 + len, 0);
                    self.buff[65..67].copy_from_slice(&(len as u16).to_le_bytes());
                    self.tx_counter = 67;
                    self.tx_body_end = 67 + len;
                    self.tx_state = TxState::Body;
                }
                true
            }
            TxState::Body => {
                self.tx_counter += 1;
                if self.tx_counter == self.tx_body_end {
                    // Complete file (header block + body block).
                    self.stream
                        .extend_from_slice(&self.buff[..self.tx_counter]);
                    self.tx_state = TxState::WaitEb;
                    self.tx_counter = 2;
                    self.tx_tick = TC_EB_MAX;
                }
                true
            }
            TxState::WaitEb => {
                self.tx_state = TxState::ExtBody;
                self.tx_counter += 1;
                self.tx_tick = TC_EB_GAP;
                true
            }
            TxState::ExtBody => {
                self.tx_counter += 1;
                self.tx_tick = TC_EB_GAP;
                true
            }
        }
    }

    /// Store the accumulated extended-body bytes as one headerless
    /// PTP block.
    fn flush_ext_block(&mut self) {
        let payload = self.tx_counter - 2;
        self.buff[0..2].copy_from_slice(&(payload as u16).to_le_bytes());
        self.stream
            .extend_from_slice(&self.buff[..self.tx_counter]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck() -> (TapeDeck, I8251) {
        (TapeDeck::new(), I8251::new())
    }

    /// Feed `bytes` to the recorder as if the CPU wrote them.
    fn send(deck: &mut TapeDeck, uart: &mut I8251, bytes: &[u8]) {
        for &b in bytes {
            deck.on_tx_byte(b, uart);
        }
    }

    /// Advance by whole half-clocks.
    fn advance(deck: &mut TapeDeck, uart: &mut I8251, half_clocks: u64) {
        deck.tick((half_clocks as f64 * HALF_CLOCK) as u64, uart);
    }

    fn leader() -> Vec<u8> {
        let mut v = vec![0xFF; 16];
        v.extend(std::iter::repeat_n(0, 16));
        v.extend(std::iter::repeat_n(0x55, 16));
        v
    }

    /// A well-formed save of `content` under `name`.
    fn save_stream(number: u8, name: &str, start: u16, content: &[u8]) -> Vec<u8> {
        let block = crate::tape::make_file(number, b'B', name, start, content).unwrap();
        let mut bytes = leader();
        bytes.extend_from_slice(&block.header_bytes[48..63]); // fields + crc
        bytes.extend_from_slice(&block.body_bytes);
        bytes
    }

    #[test]
    fn constants_match_gpmd85() {
        assert_eq!(HALF_CLOCK, 853.0);
        assert_eq!(TC_HEAD_LEADER, 6720);
        assert_eq!(TC_BODY_LEADER, 1200);
        assert_eq!(TC_STOP_TAIL, 480);
        assert_eq!(TC_GAP_SIZE, 3600);
        assert_eq!(TC_EB_GAP, 480);
        assert_eq!(TC_EB_MAX, 4800);
        assert_eq!(HALF_CLOCKS_PER_BYTE, 22.0);
    }

    #[test]
    fn idle_deck_drives_nothing() {
        let (mut d, mut u) = deck();
        advance(&mut d, &mut u, 100);
        assert!(!d.is_playing());
        assert!(!d.is_recording());
        assert_eq!(u.read(1) & 0x80, 0x80, "DSR idles high");
        assert!(d.take_monitor_edges().is_empty());
        assert!(d.take_recorded().is_empty());
        assert!(!d.take_block_finished());
    }

    #[test]
    fn recorder_captures_a_save() {
        let (mut d, mut u) = deck();
        let content: Vec<u8> = (0..64u16).map(|i| i as u8).collect();
        let stream = save_stream(1, "TEST", 0x2000, &content);
        send(&mut d, &mut u, &stream);
        // In the WaitEb finalize window now.
        assert!(d.is_recording());
        advance(&mut d, &mut u, TC_EB_MAX as u64 + 2);
        assert!(!d.is_recording(), "save finalized after 2 s of silence");
        let recorded = d.take_recorded();
        assert!(!recorded.is_empty());
        // The stream parses back into the original file.
        let mut tape = crate::tape::Tape::default();
        tape.append_ptp_stream(&recorded);
        assert_eq!(tape.blocks.len(), 1);
        let block = &tape.blocks[0];
        let h = block.header.as_ref().unwrap();
        assert_eq!(h.number, 1);
        assert_eq!(h.block_type, b'B');
        assert_eq!(h.start, 0x2000);
        assert_eq!(h.name_str(), "TEST");
        assert_eq!(block.content(), &content[..]);
        assert!(block.header_crc_ok);
        assert!(block.body_crc_ok);
        assert!(d.take_recorded().is_empty());
    }

    #[test]
    fn recorder_resyncs_on_garbage() {
        let (mut d, mut u) = deck();
        // Random bytes (not starting with the leader) must not
        // confuse the matcher; a following real save still records.
        send(&mut d, &mut u, &[0x12, 0x34, 0xFF, 0x00, 0x55, 0xFF, 0x00]);
        let content = vec![9u8; 20];
        send(&mut d, &mut u, &save_stream(2, "OK", 0x0000, &content));
        advance(&mut d, &mut u, TC_EB_MAX as u64 + 2);
        let recorded = d.take_recorded();
        let mut tape = crate::tape::Tape::default();
        tape.append_ptp_stream(&recorded);
        assert_eq!(tape.blocks.len(), 1);
        assert_eq!(tape.blocks[0].header.as_ref().unwrap().name_str(), "OK");
    }

    #[test]
    fn recorder_collects_extended_body_blocks() {
        let (mut d, mut u) = deck();
        let content = vec![7u8; 16];
        send(&mut d, &mut u, &save_stream(3, "EXT", 0x0100, &content));
        // Bytes arriving with less than 0.2 s between them extend as
        // headerless blocks. (The half-clock advance below paces them
        // apart but inside the window.)
        for i in 0..8u8 {
            d.on_tx_byte(0xA0 + i, &mut u);
            advance(&mut d, &mut u, 100); // ~0.08 s, within the 0.2 s gap
        }
        advance(&mut d, &mut u, TC_EB_MAX as u64 + 2);
        let recorded = d.take_recorded();
        let mut tape = crate::tape::Tape::default();
        tape.append_ptp_stream(&recorded);
        assert_eq!(tape.blocks.len(), 2, "file + one extended block");
        assert_eq!(tape.blocks[1].body_bytes, (0xA0..0xA8).collect::<Vec<_>>());
    }

    #[test]
    fn tx_bytes_while_playing_reset_the_matcher() {
        let (mut d, mut u) = deck();
        d.play_block(&[0x00; 4], false, true);
        send(&mut d, &mut u, &leader());
        assert!(!d.is_recording(), "recorder disabled during playback");
        d.stop(&mut u);
        send(&mut d, &mut u, &leader());
        assert!(d.is_recording(), "recorder armed again after stop");
    }

    #[test]
    fn recording_tone_goes_to_the_monitor() {
        let (mut d, mut u) = deck();
        let long: Vec<u8> = (0..200u16).map(|i| i as u8).collect();
        // Leader + header fields, then feed part of the body one byte
        // at a time while the clock runs: the deck must be mid-body
        // and emit a toggling tone.
        let block = crate::tape::make_file(9, b'B', "TON", 0, &long).unwrap();
        send(&mut d, &mut u, &leader());
        send(&mut d, &mut u, &block.header_bytes[48..63]);
        assert!(d.take_monitor_edges().is_empty(), "no tone before clocking");
        for &b in &block.body_bytes {
            d.on_tx_byte(b, &mut u);
            advance(&mut d, &mut u, 20); // well within the 0.2 s gap
        }
        let edges = d.take_monitor_edges();
        assert!(!edges.is_empty(), "recording tone missing");
        for pair in edges.windows(2) {
            assert_eq!(pair[0].level, !pair[1].level);
        }
        // Body complete: the deck sits in the WaitEb window without a
        // tone, and after 2 s of silence the save finalizes.
        advance(&mut d, &mut u, TC_EB_MAX as u64 + 2);
        assert!(d.take_monitor_edges().is_empty());
        assert!(!d.is_recording());
    }

    #[test]
    fn speed_is_clamped() {
        let (mut d, _) = deck();
        d.set_speed(0.1);
        assert_eq!(d.speed(), 1.0);
        d.set_speed(100.0);
        assert_eq!(d.speed(), MAX_SPEED);
    }

    #[test]
    fn speed_multiplies_the_feed_rate() {
        let (mut d, mut u) = deck();
        let (mut e, mut v) = deck();
        e.set_speed(2.0);
        d.play_block(&[0x00; 4], false, true);
        e.play_block(&[0x00; 4], false, true);
        // The same number of cycles advances the 2x deck twice as far
        // into the leader; it finishes its whole block with half the
        // cycles the 1x deck needs.
        d.tick(HALF_CLOCK as u64 * 100, &mut u);
        e.tick(HALF_CLOCK as u64 * 100, &mut v);
        let full =
            (TC_BODY_LEADER as f64 + 4.0 * 22.0 + TC_STOP_TAIL as f64 + 2.0) * HALF_CLOCK;
        e.tick(full as u64, &mut v);
        assert!(!e.is_playing(), "2x deck finished");
        assert!(d.is_playing(), "1x deck still playing");
    }
}
