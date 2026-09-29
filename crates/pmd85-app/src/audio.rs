//! Speaker audio output: takes the cycle-stamped edges drained from the
//! emulated machine each frame, expands them into a mono f32 square-wave
//! stream with the core's [`EdgeExpander`], and feeds them to the default
//! output device through cpal.
//!
//! The callback pulls from a bounded ring buffer shared with the main
//! thread; on overflow the oldest samples are dropped (latency never
//! grows), on underrun the callback plays silence.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use pmd85_core::audio::{EdgeExpander, SpeakerEdge};
use std::sync::{Arc, Mutex};

/// How much audio to buffer ahead, in seconds. Small enough to keep the
/// key-click latency low, large enough to ride out a few dropped frames.
const BUFFER_SECONDS: f32 = 0.15;

/// Output volume (the square wave is loud at full scale).
const GAIN: f32 = 0.25;

/// Fixed-capacity ring of mono samples shared with the audio thread.
struct SharedRing {
    buf: Vec<f32>,
    /// Monotonic sample counters; `len()` is `write - read`.
    read: u64,
    write: u64,
}

impl SharedRing {
    fn with_capacity(capacity: usize) -> Self {
        SharedRing {
            buf: vec![0.0; capacity.max(1)],
            read: 0,
            write: 0,
        }
    }

    fn len(&self) -> usize {
        (self.write - self.read) as usize
    }

    /// Append samples, dropping the oldest ones if the buffer is full.
    fn push(&mut self, samples: &[f32]) {
        let cap = self.buf.len() as u64;
        for &s in samples {
            if self.len() >= cap as usize {
                self.read += 1;
            }
            self.buf[(self.write % cap) as usize] = s;
            self.write += 1;
        }
    }

    /// Pop one sample, or `None` when empty.
    fn pop(&mut self) -> Option<f32> {
        if self.len() == 0 {
            return None;
        }
        let cap = self.buf.len() as u64;
        let v = self.buf[(self.read % cap) as usize];
        self.read += 1;
        Some(v)
    }
}

/// Speaker output for the app: expander plus a live cpal stream.
pub struct SpeakerOut {
    expander: EdgeExpander,
    ring: Arc<Mutex<SharedRing>>,
    /// Kept alive for the lifetime of the app: dropping the stream
    /// would stop the audio (hence the underscore).
    _stream: cpal::Stream,
    /// Per-frame scratch buffer handed to the ring.
    scratch: Vec<f32>,
}

impl SpeakerOut {
    /// Open the default output device as an f32 mono-ish stream. Returns
    /// `None` (after logging) when there is no usable device, so the app
    /// can run silently rather than failing to start.
    pub fn new() -> Option<Self> {
        let host = cpal::default_host();
        let device = host.default_output_device().or_else(|| {
            log::info!("no audio output device, running silent");
            None
        })?;
        let supported = device.default_output_config().ok()?;
        let config: cpal::StreamConfig = supported.config();
        let sample_rate = config.sample_rate;

        let ring = Arc::new(Mutex::new(SharedRing::with_capacity(
            (sample_rate as f32 * BUFFER_SECONDS) as usize,
        )));
        let channels = (config.channels as usize).max(1);
        let cb_ring = ring.clone();
        let err_fn = |e| log::warn!("audio stream error: {e}");
        // The callback converts our mono samples to the device's channel
        // layout by duplication; underruns play as silence.
        let stream = device
            .build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    let mut ring = match cb_ring.lock() {
                        Ok(r) => r,
                        Err(_) => return,
                    };
                    let mut i = 0;
                    while i + channels <= data.len() {
                        let val = ring.pop().unwrap_or(0.0);
                        for slot in &mut data[i..i + channels] {
                            *slot = val;
                        }
                        i += channels;
                    }
                    for slot in &mut data[i..] {
                        *slot = 0.0;
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| {
                log::warn!("cannot open audio stream ({e}), running silent");
                e
            })
            .ok()?;

        stream.play().ok()?;
        log::info!(
            "speaker output: {} Hz, {} channels",
            sample_rate,
            channels
        );
        Some(SpeakerOut {
            expander: EdgeExpander::new(sample_rate, 0, false),
            ring,
            _stream: stream,
            scratch: Vec::new(),
        })
    }

    /// Feed a frame's worth of edges (from `Machine::take_speaker_edges`)
    /// and render everything up to the machine's current cycle count into
    /// the output buffer.
    pub fn submit(&mut self, edges: Vec<SpeakerEdge>, until_cycle: u64) {
        self.expander.push_edges(edges);
        self.scratch.clear();
        self.expander.render_until(until_cycle, &mut self.scratch);
        // The expander emits +1/-1 for speaker high/low; map that to a
        // positive pulse at GAIN and silence when at rest, so idle output
        // has no DC offset.
        if let Ok(mut ring) = self.ring.lock() {
            for s in &mut self.scratch {
                *s = if *s > 0.0 { GAIN } else { 0.0 };
            }
            ring.push(&self.scratch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_fifo_and_underrun() {
        let mut r = SharedRing::with_capacity(4);
        r.push(&[1.0, 2.0, 3.0]);
        assert_eq!(r.pop(), Some(1.0));
        assert_eq!(r.pop(), Some(2.0));
        assert_eq!(r.pop(), Some(3.0));
        // empty: pop yields nothing (the callback substitutes silence)
        assert_eq!(r.pop(), None);
    }

    #[test]
    fn ring_drops_oldest_on_overflow() {
        let mut r = SharedRing::with_capacity(2);
        r.push(&[1.0, 2.0]);
        r.push(&[3.0, 4.0]);
        assert_eq!(r.len(), 2);
        assert_eq!(r.pop(), Some(3.0));
        assert_eq!(r.pop(), Some(4.0));
        assert_eq!(r.pop(), None);
    }

    #[test]
    fn ring_wraps_around_forever() {
        let mut r = SharedRing::with_capacity(3);
        for round in 0..100u32 {
            r.push(&[round as f32, (round + 1) as f32, (round + 2) as f32]);
            assert_eq!(r.pop(), Some(round as f32));
            assert_eq!(r.pop(), Some((round + 1) as f32));
            assert_eq!(r.pop(), Some((round + 2) as f32));
        }
    }

    #[test]
    fn speaker_out_stream_drains() {
        // Live smoke test: opens the real default output device and
        // checks that the callback pulls samples out of the ring.
        // Skips when no device is available.
        let Some(out) = SpeakerOut::new() else {
            return;
        };
        // Half a second of silence at 48 kHz-ish; whatever the rate, the
        // callback must consume it in well under a second.
        let silence = vec![0.0f32; 24_000];
        {
            let mut ring = out.ring.lock().unwrap();
            let before = ring.len();
            ring.push(&silence);
            assert!(ring.len() > before);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        let left = out.ring.lock().unwrap().len();
        assert!(
            left < silence.len(),
            "audio callback consumed nothing ({left} samples left)"
        );
    }
}
