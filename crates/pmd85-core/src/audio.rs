//! Speaker audio support.
//!
//! The PMD 85 speaker is driven by bit 2 of port C on the motherboard 8255
//! (key clicks, monitor beeps). [`crate::machine::MachineBus`] records a
//! cycle-stamped [`SpeakerEdge`] whenever that bit changes; the audio
//! frontend drains them via `Machine::take_speaker_edges` and feeds them to
//! [`EdgeExpander`], which turns them into a mono f32 square-wave stream at
//! an arbitrary sample rate.

use crate::machine::CPU_CLOCK_HZ;
use std::collections::VecDeque;

/// A speaker level transition: the absolute CPU cycle at which it took
/// effect (stamped at the end of the OUT instruction that caused it) and
/// the speaker level from then on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeakerEdge {
    pub cycle: u64,
    pub level: bool,
}

/// Bounded FIFO of speaker edges recorded by the machine, drained by the
/// audio frontend once per frame. If the frontend stops draining (muted
/// output, no audio device), the oldest edges are dropped instead of
/// growing without bound.
///
/// The default capacity (32768) covers ~160 ms even for the tightest
/// possible toggle loop (an unrolled sequence of 10-cycle OUT
/// instructions), far more than the edges a single 20 ms frame can produce.
#[derive(Debug)]
pub struct SpeakerEdgeLog {
    edges: VecDeque<SpeakerEdge>,
    capacity: usize,
}

impl Default for SpeakerEdgeLog {
    fn default() -> Self {
        SpeakerEdgeLog::with_capacity(32768)
    }
}

impl SpeakerEdgeLog {
    pub fn with_capacity(capacity: usize) -> Self {
        SpeakerEdgeLog {
            edges: VecDeque::new(),
            capacity,
        }
    }

    /// Record an edge, dropping the oldest one if the log is full.
    pub fn push(&mut self, edge: SpeakerEdge) {
        if self.edges.len() >= self.capacity {
            self.edges.pop_front();
        }
        self.edges.push_back(edge);
    }

    /// Remove and return all recorded edges, oldest first.
    pub fn drain(&mut self) -> Vec<SpeakerEdge> {
        self.edges.drain(..).collect()
    }

    pub fn clear(&mut self) {
        self.edges.clear();
    }

    pub fn len(&self) -> usize {
        self.edges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }
}

/// Converts cycle-stamped speaker edges into a mono f32 sample stream.
///
/// Samples are taken at fixed intervals of `CPU_CLOCK_HZ / sample_rate`
/// cycles, starting at the cycle the expander was created with. Each
/// sample is the speaker level at that instant: `+1.0` high, `-1.0` low.
/// The frontend calls [`EdgeExpander::push_edges`] with the edges drained
/// from the machine and then [`EdgeExpander::render_until`] with the
/// machine's current cycle count, once per frame.
///
/// Sample positions are tracked with exact integer arithmetic (sample `k`
/// sits at cycle `(start_cycle + k * CPU_CLOCK_HZ) / sample_rate`), so
/// there is no drift over long sessions.
///
/// Edges stamped before the next sample time (the machine stamps at the
/// end of an OUT instruction, so an edge can land a few cycles "in the
/// past" of an already rendered frame boundary) are folded into the level
/// of the next sample instead of being lost.
pub struct EdgeExpander {
    sample_rate: u64,
    start_cycle: u64,
    /// Index of the next sample to produce.
    next_sample: u64,
    /// Speaker level in effect at the next sample.
    level: bool,
    /// Edges at or after the next sample time, not yet folded into
    /// `level`.
    pending: VecDeque<SpeakerEdge>,
}

impl EdgeExpander {
    /// Create an expander whose sample stream starts at `start_cycle`,
    /// with the speaker at `start_level` there.
    pub fn new(sample_rate: u32, start_cycle: u64, start_level: bool) -> Self {
        assert!(sample_rate > 0, "sample rate must be nonzero");
        EdgeExpander {
            sample_rate: sample_rate as u64,
            start_cycle,
            next_sample: 0,
            level: start_level,
            pending: VecDeque::new(),
        }
    }

    /// The numerator of the next sample's cycle position, over
    /// `sample_rate`: the sample time is
    /// `(start_cycle + next_sample * CPU_CLOCK_HZ) / sample_rate` cycles.
    fn next_sample_num(&self) -> u128 {
        (self.start_cycle as u128) * self.sample_rate as u128
            + (self.next_sample as u128) * CPU_CLOCK_HZ as u128
    }

    /// True if the next sample time is strictly before `cycle`.
    fn next_sample_before(&self, cycle: u64) -> bool {
        self.next_sample_num() < (cycle as u128) * self.sample_rate as u128
    }

    /// True if an edge at `cycle` takes effect at or before the next
    /// sample time.
    fn edge_at_or_before_next_sample(&self, cycle: u64) -> bool {
        (cycle as u128) * self.sample_rate as u128 <= self.next_sample_num()
    }

    /// Feed edges drained from the machine (ascending cycle order).
    pub fn push_edges<I: IntoIterator<Item = SpeakerEdge>>(&mut self, edges: I) {
        for edge in edges {
            if (edge.cycle as u128) * (self.sample_rate as u128) < self.next_sample_num() {
                // Older than the next sample time: it becomes the level
                // going forward.
                self.level = edge.level;
            } else {
                self.pending.push_back(edge);
            }
        }
    }

    /// Append samples for every sample time before `until_cycle`
    /// (an absolute machine cycle count).
    pub fn render_until(&mut self, until_cycle: u64, out: &mut Vec<f32>) {
        while self.next_sample_before(until_cycle) {
            while let Some(&SpeakerEdge { cycle, level }) = self.pending.front() {
                if self.edge_at_or_before_next_sample(cycle) {
                    self.level = level;
                    self.pending.pop_front();
                } else {
                    break;
                }
            }
            out.push(if self.level { 1.0 } else { -1.0 });
            self.next_sample += 1;
        }
    }

    /// Number of edges pushed but not yet consumed by rendering.
    pub fn pending_edges(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(cycle: u64, level: bool) -> SpeakerEdge {
        SpeakerEdge { cycle, level }
    }

    #[test]
    fn edge_log_drains_in_order() {
        let mut log = SpeakerEdgeLog::default();
        log.push(edge(10, true));
        log.push(edge(20, false));
        assert_eq!(log.drain(), vec![edge(10, true), edge(20, false)]);
        assert!(log.is_empty());
        // draining again yields nothing
        assert!(log.drain().is_empty());
    }

    #[test]
    fn edge_log_capacity_drops_oldest() {
        let mut log = SpeakerEdgeLog::with_capacity(2);
        log.push(edge(1, true));
        log.push(edge(2, false));
        log.push(edge(3, true));
        assert_eq!(log.len(), 2);
        assert_eq!(log.drain(), vec![edge(2, false), edge(3, true)]);
    }

    #[test]
    fn edge_log_clear() {
        let mut log = SpeakerEdgeLog::default();
        log.push(edge(1, true));
        log.clear();
        assert!(log.is_empty());
    }

    #[test]
    fn expander_idle_speaker_is_low() {
        // 2048 Hz sample rate -> exactly 1000 cycles per sample
        let mut exp = EdgeExpander::new(2048, 0, false);
        let mut out = Vec::new();
        exp.render_until(3000, &mut out);
        assert_eq!(out, vec![-1.0, -1.0, -1.0]);
    }

    #[test]
    fn expander_square_wave() {
        let mut exp = EdgeExpander::new(2048, 0, false);
        exp.push_edges([edge(500, true), edge(1500, false), edge(2500, true)]);
        let mut out = Vec::new();
        exp.render_until(3000, &mut out);
        // samples at cycles 0, 1000, 2000
        assert_eq!(out, vec![-1.0, 1.0, -1.0]);
    }

    #[test]
    fn expander_continues_across_renders() {
        let mut exp = EdgeExpander::new(2048, 0, false);
        exp.push_edges([edge(500, true), edge(1500, false), edge(2500, true)]);
        let mut out = Vec::new();
        exp.render_until(3000, &mut out);
        // next frame: edges at 3500 and 4500, samples at 3000 and 4000
        exp.push_edges([edge(3500, false), edge(4500, true)]);
        exp.render_until(5000, &mut out);
        assert_eq!(out, vec![-1.0, 1.0, -1.0, 1.0, -1.0]);
        // nothing new: the speaker just holds its level
        exp.render_until(7000, &mut out);
        assert_eq!(out, vec![-1.0, 1.0, -1.0, 1.0, -1.0, 1.0, 1.0]);
    }

    #[test]
    fn expander_edge_exactly_at_sample_time_counts() {
        let mut exp = EdgeExpander::new(2048, 0, false);
        exp.push_edges([edge(1000, true)]);
        let mut out = Vec::new();
        exp.render_until(2000, &mut out);
        // sample at 0 is low, sample at 1000 already sees the edge
        assert_eq!(out, vec![-1.0, 1.0]);
    }

    #[test]
    fn expander_late_edge_folds_into_level() {
        let mut exp = EdgeExpander::new(2048, 0, false);
        let mut out = Vec::new();
        exp.render_until(2000, &mut out);
        assert_eq!(out, vec![-1.0, -1.0]);
        // an edge stamped inside the already rendered range
        exp.push_edges([edge(1500, true)]);
        exp.render_until(3000, &mut out);
        assert_eq!(out, vec![-1.0, -1.0, 1.0]);
    }

    #[test]
    fn expander_sample_count_at_48k() {
        let mut exp = EdgeExpander::new(48000, 0, false);
        let mut out = Vec::new();
        exp.render_until(crate::machine::CYCLES_PER_FRAME, &mut out);
        // 2048000 / 48000 = 960 samples per frame
        assert_eq!(out.len(), 960);
    }

    #[test]
    fn expander_pending_edges_tracked() {
        let mut exp = EdgeExpander::new(2048, 0, false);
        exp.push_edges([edge(5000, true)]);
        assert_eq!(exp.pending_edges(), 1);
        let mut out = Vec::new();
        exp.render_until(1000, &mut out);
        assert_eq!(exp.pending_edges(), 1);
    }
}
