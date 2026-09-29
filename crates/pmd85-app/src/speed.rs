//! Emulation speed control and real-time pacing.
//!
//! The machine must be stepped at 50 Hz of *emulated* time, but the wall
//! clock is the only valid reference at 1x speed. [`frames_due`] is the
//! pure pacing kernel: given the wall-clock time of the next emulated
//! frame, it computes how many frames to run now. Turbo (unthrottled)
//! bypasses the wall clock entirely and runs frames until a compute
//! budget runs out.

use std::time::{Duration, Instant};

/// Emulated frame period (50 Hz video).
pub const FRAME_PERIOD_SECS: f64 = 0.020;
/// Emulated frames per second.
pub const FRAMES_PER_SEC: f64 = 50.0;

/// Compute budget per render frame while turbo is active: run as many
/// emulated frames as fit, leaving the rest of the frame for the UI.
pub const TURBO_BUDGET: Duration = Duration::from_millis(8);

/// Available speed presets (multipliers of real time).
pub const SPEED_PRESETS: &[f64] = &[0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 25.0, 50.0];

/// Allowed speed multiplier range, inclusive (0.25x to 50x real time).
pub const SPEED_MIN: f64 = 0.25;
pub const SPEED_MAX: f64 = 50.0;

/// How many emulated frames to run now, and the wall-clock time the
/// *next* emulated frame is due.
///
/// `next_due` is the deadline of the next *unrun* frame: a frame whose
/// deadline is exactly `now` is due and runs. `now` and `next_due` are
/// seconds since an arbitrary common epoch. `multiplier` is the speed
/// relative to real time (1.0 = real time). Behind-schedule catch-up
/// is bounded so that a long stall (window drag, resize) does not
/// fast-forward the machine in one giant burst; beyond the bound the
/// pacer resynchronizes to `now`.
pub fn frames_due(now: f64, next_due: f64, multiplier: f64) -> (u32, f64) {
    assert!(multiplier.is_finite() && multiplier > 0.0, "bad multiplier {multiplier}");
    let period = FRAME_PERIOD_SECS / multiplier;
    let mut frames: u32 = 0;
    let mut due = next_due;
    // Bound the burst: at high multipliers a whole wall-frame worth of
    // emulated frames is legitimately owed, so scale the cap with the
    // speed; never less than the 1x catch-up bound.
    let max_catch_up = ((FRAMES_PER_SEC / 60.0 * 2.0 * multiplier).ceil() as u32).max(6);
    while now >= due && frames < max_catch_up {
        frames += 1;
        due += period;
    }
    if now >= due {
        // Too far behind to catch up gracefully: resynchronize.
        due = now;
    }
    (frames, due)
}

/// Wall-clock state of the emulator's frame pacing.
///
/// Holds the wall-clock time at which the next emulated frame is due
/// (see [`frames_due`]); turbo mode ignores it.
#[derive(Debug)]
pub struct Pacer {
    /// Wall-clock time of the next emulated frame.
    next_due: Instant,
    /// Seconds-epoch counterpart of `next_due`, for the pure kernel.
    epoch: Instant,
}

impl Default for Pacer {
    fn default() -> Self {
        Self::new()
    }
}

impl Pacer {
    pub fn new() -> Self {
        let epoch = Instant::now();
        Pacer {
            next_due: epoch,
            epoch,
        }
    }

    /// Frames to run at `multiplier` right now.
    pub fn take_frames(&mut self, multiplier: f64) -> u32 {
        let now = self.epoch.elapsed().as_secs_f64();
        let due = self.next_due.duration_since(self.epoch).as_secs_f64();
        let (frames, new_due) = frames_due(now, due, multiplier);
        self.next_due = self.epoch + Duration::from_secs_f64(new_due);
        frames
    }

    /// Drop accumulated debt (after a pause, turbo run, or machine
    /// reconfiguration) without fast-forwarding.
    pub fn resync(&mut self) {
        self.next_due = Instant::now();
    }

    /// Wall-clock time the next emulated frame is due (turbo ignores it).
    pub fn next_due(&self) -> Instant {
        self.next_due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `next_due` is the deadline of the next *unrun* frame: at
    // `now == next_due` that frame is due and runs. All frame
    // deadlines before it have already been consumed.

    #[test]
    fn no_frames_before_deadline() {
        // 10 ms short of the next deadline at 1x.
        let (frames, due) = frames_due(10.010, 10.020, 1.0);
        assert_eq!(frames, 0);
        assert_eq!(due, 10.020);
    }

    #[test]
    fn frame_runs_at_its_deadline() {
        // The pending frame (due at 10.010) runs; the next one is
        // due at 10.030.
        let (frames, due) = frames_due(10.025, 10.010, 1.0);
        assert_eq!(frames, 1);
        assert!((due - 10.030).abs() < 1e-9);
    }

    #[test]
    fn consecutive_deadlines_all_run() {
        // 25 ms past the deadline at 1x: the frames due at 10.000,
        // 10.020 both run; the next is due at 10.040.
        let (frames, due) = frames_due(10.025, 10.0, 1.0);
        assert_eq!(frames, 2);
        assert!((due - 10.040).abs() < 1e-9);
    }

    #[test]
    fn slow_speed_stretches_the_period() {
        // At 0.5x a frame is due every 40 ms.
        // 30 ms past the deadline: only the one pending frame runs.
        let (frames, due) = frames_due(10.030, 10.0, 0.5);
        assert_eq!(frames, 1);
        assert!((due - 10.040).abs() < 1e-9);
        // 10 ms short of the next deadline: nothing due.
        let (frames, due) = frames_due(10.030, 10.040, 0.5);
        assert_eq!(frames, 0);
        assert_eq!(due, 10.040);
    }

    #[test]
    fn fast_speed_shrinks_the_period() {
        // At 2x a frame is due every 10 ms; 25 ms past the deadline
        // owes three frames (10.000, 10.010, 10.020), the fourth at
        // 10.030.
        let (frames, due) = frames_due(10.025, 10.0, 2.0);
        assert_eq!(frames, 3);
        assert!((due - 10.030).abs() < 1e-9);
    }

    #[test]
    fn catch_up_is_bounded_at_1x() {
        // A full second behind at 1x: only the catch-up bound of frames
        // run now, and the pacer resynchronizes.
        let (frames, due) = frames_due(11.0, 10.0, 1.0);
        assert_eq!(frames, 6);
        assert_eq!(due, 11.0);
    }

    #[test]
    fn catch_up_scales_with_speed() {
        // 100 ms behind at 10x (period 2 ms): 50 frames owed, bound is
        // ceil(50/60*2*10) = 17, then resync.
        let (frames, due) = frames_due(10.100, 10.0, 10.0);
        assert_eq!(frames, 17);
        assert_eq!(due, 10.100);
        // A full second behind at 10x: same bound, resync.
        let (frames, due) = frames_due(11.0, 10.0, 10.0);
        assert_eq!(frames, 17);
        assert_eq!(due, 11.0);
    }

    #[test]
    #[should_panic]
    fn rejects_bad_multiplier() {
        frames_due(0.0, 0.0, 0.0);
    }

    #[test]
    fn pacer_tracks_wall_clock() {
        let mut pacer = Pacer::new();
        // Freshly created: the first frame is due immediately (the
        // machine starts running without a startup delay).
        let f = pacer.take_frames(1.0);
        assert_eq!(f, 1);
        // Before the next 20 ms deadline: nothing more is owed.
        let f = pacer.take_frames(1.0);
        assert_eq!(f, 0);
        // After sleeping past one frame period at 1x, a frame is owed.
        std::thread::sleep(Duration::from_millis(21));
        let f = pacer.take_frames(1.0);
        assert_eq!(f, 1);
    }
}
