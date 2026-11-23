//! Phase 2 smooth scrolling: pending-distance exponential ease-out and touchpad bypass.

use std::time::{Duration, Instant};

use super::speed::{split_i16_deltas, AxisAccumulator, ACCUMULATOR_RESET_AFTER};

/// Exponential ease-out time constant (SPEC-30 §6: 35 ms, range 20..60 ms).
pub const SMOOTH_TAU_MS: f64 = 35.0;

/// Smoother drain interval (~125 Hz, SPEC-30 §6).
pub const SMOOTH_TICK: Duration = Duration::from_millis(8);

/// Maximum settle duration after the last input before remaining distance flushes (SPEC-30 §6).
pub const SMOOTH_MAX_SETTLE: Duration = Duration::from_millis(150);

/// Cap on pending distance in physical wheel notches (SPEC-30 §6).
pub const SMOOTH_CAP_NOTCHES: f64 = 20.0;

/// Standard Windows wheel delta for one physical notch.
pub const WHEEL_DELTA_NOTCH: i16 = 120;

/// Window for treating an incidental 120-delta event as part of an active touchpad stream.
const TOUCHPAD_STREAM_WINDOW: Duration = Duration::from_millis(150);
const TOUCHPAD_RECENT_GAP: Duration = Duration::from_millis(50);
const TOUCHPAD_HISTORY_LEN: usize = 4;

/// Tracks recent wheel deltas to distinguish precision touchpad input from notched wheel ticks.
///
/// Precision touchpads emit small non-multiple-of-120 deltas (SPEC-30 §5.3). Standard notched
/// wheels always emit non-zero multiples of 120, even during rapid multi-notch spins.
#[derive(Debug, Default)]
struct TouchpadDetector {
    recent: [Option<(bool, Instant)>; TOUCHPAD_HISTORY_LEN],
    head: usize,
}

impl TouchpadDetector {
    fn record(&mut self, is_sub_notch: bool, now: Instant) {
        self.recent[self.head] = Some((is_sub_notch, now));
        self.head = (self.head + 1) % TOUCHPAD_HISTORY_LEN;
    }

    fn is_touchpad_like(&mut self, delta: i16, now: Instant) -> bool {
        let is_sub_notch = delta.abs() % WHEEL_DELTA_NOTCH != 0;
        let result = if is_sub_notch {
            true
        } else {
            // An occasional exact-120 delta inside an active high-rate touchpad gesture
            // stays bypassed if recent events were sub-notch.
            let mut sub_count = 0usize;
            let mut most_recent_sub: Option<Instant> = None;
            for (sub, at) in self.recent.iter().flatten().copied() {
                if sub && now.saturating_duration_since(at) <= TOUCHPAD_STREAM_WINDOW {
                    sub_count += 1;
                    most_recent_sub =
                        Some(most_recent_sub.map_or(at, |prev| if at > prev { at } else { prev }));
                }
            }
            sub_count >= 2
                && most_recent_sub
                    .map(|at| now.saturating_duration_since(at) <= TOUCHPAD_RECENT_GAP)
                    .unwrap_or(false)
        };
        self.record(is_sub_notch, now);
        result
    }
}

/// Per-axis smooth-scrolling state (pending distance `P`, remainder carry, and touchpad bypass).
#[derive(Debug, Default)]
pub struct AxisSmoother {
    pending: f64,
    remainder: f64,
    last_input_at: Option<Instant>,
    touchpad: TouchpadDetector,
    bypass_acc: AxisAccumulator,
}

impl AxisSmoother {
    pub fn new() -> Self {
        Self::default()
    }

    /// True while un-emitted smoothed distance `P` is waiting to drain.
    pub fn has_pending(&self) -> bool {
        self.pending != 0.0
    }

    /// Current pending distance `P` (scaled wheel delta units).
    pub fn pending(&self) -> f64 {
        self.pending
    }

    /// Feed a wheel delta into the smoother.
    ///
    /// Touchpad-like deltas bypass smoothing and return scaled `i16` chunks immediately.
    /// Standard wheel notches add to pending distance `P` (capped at 20 notches) and
    /// return an empty vector to be drained via [`Self::tick`].
    pub fn push(&mut self, delta: i16, multiplier: f32, now: Instant) -> Vec<i16> {
        if delta == 0 || !multiplier.is_finite() {
            return Vec::new();
        }

        if self.touchpad.is_touchpad_like(delta, now) {
            return self.bypass_acc.push(delta, multiplier, now);
        }

        if self.pending == 0.0 {
            let idle_timed_out = self
                .last_input_at
                .map(|prev| now.saturating_duration_since(prev) >= ACCUMULATOR_RESET_AFTER)
                .unwrap_or(false);
            if idle_timed_out {
                self.remainder = 0.0;
            }
        }

        let mult = multiplier as f64;
        let cap = SMOOTH_CAP_NOTCHES * (WHEEL_DELTA_NOTCH as f64) * mult.abs().max(1.0);
        self.pending = (self.pending + (delta as f64) * mult).clamp(-cap, cap);
        self.last_input_at = Some(now);
        Vec::new()
    }

    /// Advance the exponential ease-out curve by `dt` and return integer deltas to emit.
    pub fn tick(&mut self, dt: Duration, now: Instant) -> Vec<i16> {
        if self.pending == 0.0 {
            return Vec::new();
        }
        let dt_ms = dt.as_secs_f64() * 1000.0;
        if dt_ms <= 0.0 {
            return Vec::new();
        }

        let alpha = 1.0 - (-dt_ms / SMOOTH_TAU_MS).exp();
        let step = self.pending * alpha;
        let next_pending = self.pending - step;
        let settle_expired = self
            .last_input_at
            .map(|t| now.saturating_duration_since(t) >= SMOOTH_MAX_SETTLE)
            .unwrap_or(false);

        if next_pending.abs() < 1.0 || settle_expired {
            return self.flush();
        }

        self.pending = next_pending;
        let total = step + self.remainder;
        let emit = total.trunc();
        self.remainder = total - emit;
        split_i16_deltas(emit as i64)
    }

    /// Immediately emit all remaining pending distance and clear `P`.
    pub fn flush(&mut self) -> Vec<i16> {
        if self.pending == 0.0 {
            return Vec::new();
        }
        let total = self.pending + self.remainder;
        self.pending = 0.0;

        let rounded = total.round();
        let emit = if (total - rounded).abs() < 1e-6 {
            self.remainder = 0.0;
            rounded
        } else {
            let truncated = total.trunc();
            self.remainder = total - truncated;
            truncated
        };
        split_i16_deltas(emit as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain_until_idle(
        smoother: &mut AxisSmoother,
        start: Instant,
        max_ms: u64,
    ) -> (Vec<i16>, Duration) {
        let mut emitted = Vec::new();
        let mut elapsed_ms = 0u64;
        while smoother.has_pending() && elapsed_ms < max_ms {
            let step_ms = (max_ms - elapsed_ms).min(SMOOTH_TICK.as_millis() as u64);
            elapsed_ms += step_ms;
            let now = start + Duration::from_millis(elapsed_ms);
            let out = smoother.tick(Duration::from_millis(step_ms), now);
            emitted.extend(out.into_iter().filter(|&d| d != 0));
        }
        (emitted, Duration::from_millis(elapsed_ms))
    }

    #[test]
    fn one_notch_emits_at_least_8_steps_within_150ms_with_exact_sum() {
        for (delta, mult) in [(120i16, 1.0f32), (-120, 1.0), (120, 2.5), (-120, 3.25)] {
            let mut smoother = AxisSmoother::new();
            let t0 = Instant::now();
            assert!(smoother.push(delta, mult, t0).is_empty());
            assert!(smoother.has_pending());

            let (steps, settled_in) = drain_until_idle(&mut smoother, t0, 150);
            assert!(!smoother.has_pending(), "did not settle within 150 ms");
            assert!(settled_in <= SMOOTH_MAX_SETTLE);
            assert!(
                steps.len() >= 8,
                "expected >= 8 steps for delta={delta}, mult={mult}, got {}",
                steps.len()
            );

            let sum: i64 = steps.iter().map(|&v| v as i64).sum();
            let expected = ((delta as f64) * (mult as f64)).round() as i64;
            assert_eq!(sum, expected, "sum mismatch for delta={delta}, mult={mult}");
        }
    }

    #[test]
    fn three_notches_within_60ms_merge_and_sum_exactly() {
        let mut smoother = AxisSmoother::new();
        let t0 = Instant::now();
        let mult = 2.5f32;
        let mut emitted = Vec::new();

        // Notch 1 at 0 ms, notch 2 at 24 ms, notch 3 at 56 ms (all within 60 ms).
        assert!(smoother.push(120, mult, t0).is_empty());
        for ms in [8, 16, 24] {
            emitted.extend(smoother.tick(SMOOTH_TICK, t0 + Duration::from_millis(ms)));
        }
        let step_before_notch2 = *emitted.last().unwrap();
        assert!(smoother
            .push(120, mult, t0 + Duration::from_millis(24))
            .is_empty());

        for ms in [32, 40, 48, 56] {
            emitted.extend(smoother.tick(SMOOTH_TICK, t0 + Duration::from_millis(ms)));
        }
        assert!(smoother
            .push(120, mult, t0 + Duration::from_millis(56))
            .is_empty());

        // Merged curve continues without dropping back to zero or stalling.
        let after_merge = smoother.tick(SMOOTH_TICK, t0 + Duration::from_millis(64));
        emitted.extend(after_merge.iter().copied());
        assert!(
            after_merge.iter().map(|&v| v as i64).sum::<i64>() > step_before_notch2 as i64,
            "merging notches should boost ongoing step smoothly"
        );

        let (rest, _) = drain_until_idle(&mut smoother, t0 + Duration::from_millis(64), 200);
        emitted.extend(rest);
        assert!(!smoother.has_pending());
        let sum: i64 = emitted.iter().map(|&v| v as i64).sum();
        assert_eq!(sum, 3 * 120 * 25 / 10);
    }

    #[test]
    fn opposite_directions_cancel() {
        // Immediate cancellation before any tick runs.
        let mut s1 = AxisSmoother::new();
        let t0 = Instant::now();
        s1.push(120, 2.5, t0);
        s1.push(-120, 2.5, t0 + Duration::from_millis(2));
        assert!(!s1.has_pending());
        assert!(s1
            .tick(SMOOTH_TICK, t0 + Duration::from_millis(8))
            .is_empty());

        // Mid-drain cancellation preserves exact net sum of 0.
        let mut s2 = AxisSmoother::new();
        let mut emitted = Vec::new();
        s2.push(120, 2.0, t0);
        for ms in [8, 16, 24] {
            emitted.extend(s2.tick(SMOOTH_TICK, t0 + Duration::from_millis(ms)));
        }
        assert!(emitted.iter().map(|&v| v as i64).sum::<i64>() > 0);
        s2.push(-120, 2.0, t0 + Duration::from_millis(25));
        let (rest, _) = drain_until_idle(&mut s2, t0 + Duration::from_millis(25), 200);
        emitted.extend(rest);
        assert!(!s2.has_pending());
        assert_eq!(emitted.iter().map(|&v| v as i64).sum::<i64>(), 0);
    }

    #[test]
    fn twenty_notch_cap_clamps_pending_distance() {
        for mult in [1.0f32, 2.5f32] {
            let mut smoother = AxisSmoother::new();
            let t0 = Instant::now();
            for i in 0..30 {
                smoother.push(120, mult, t0 + Duration::from_millis(i));
            }
            let expected_cap = 20.0 * 120.0 * (mult as f64);
            assert!((smoother.pending() - expected_cap).abs() < 1e-6);

            let (emitted, _) = drain_until_idle(&mut smoother, t0 + Duration::from_millis(30), 300);
            let sum: i64 = emitted.iter().map(|&v| v as i64).sum();
            assert_eq!(sum, expected_cap.round() as i64);
        }

        // Negative runaway spin also caps at -20 notches.
        let mut neg = AxisSmoother::new();
        let t0 = Instant::now();
        for i in 0..25 {
            neg.push(-120, 1.5, t0 + Duration::from_millis(i));
        }
        assert!((neg.pending() + 20.0 * 120.0 * 1.5).abs() < 1e-6);
    }

    #[test]
    fn property_sum_emitted_equals_sum_input_times_multiplier() {
        let mut seed: u64 = 0x5EED_3002_CAFE_BABE;
        let mut next_u32 = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            (seed >> 32) as u32
        };

        let multipliers = [1.0f32, 1.25, 1.5, 2.0, 2.5, 3.0, 3.75, 4.0, 5.0];

        for trial in 0..100 {
            let mult = multipliers[(next_u32() as usize) % multipliers.len()];
            let mut smoother = AxisSmoother::new();
            let mut now = Instant::now();
            let mut input_sum: i64 = 0;
            let mut emitted_sum: i64 = 0;

            // Keep cumulative net input comfortably within the 20-notch cap per gesture.
            let events = 1 + ((next_u32() as usize) % 12);
            for _ in 0..events {
                let notches = match (next_u32() % 5) as i16 {
                    0 => -2,
                    1 => -1,
                    2 => 1,
                    3 => 1,
                    _ => 2,
                };
                let delta = notches * WHEEL_DELTA_NOTCH;
                let candidate = (smoother.pending() + (delta as f64) * (mult as f64)).abs();
                let cap = SMOOTH_CAP_NOTCHES * (WHEEL_DELTA_NOTCH as f64) * (mult as f64);
                if candidate >= cap * 0.9 {
                    continue;
                }

                input_sum += delta as i64;
                let immediate = smoother.push(delta, mult, now);
                emitted_sum += immediate.iter().map(|&v| v as i64).sum::<i64>();

                let gap_ms = (next_u32() % 40) as u64;
                let steps = gap_ms / 8;
                for _ in 0..steps {
                    now += SMOOTH_TICK;
                    let out = smoother.tick(SMOOTH_TICK, now);
                    emitted_sum += out.iter().map(|&v| v as i64).sum::<i64>();
                }
                now += Duration::from_millis(gap_ms % 8);
            }

            let (tail, _) = drain_until_idle(&mut smoother, now, 300);
            emitted_sum += tail.iter().map(|&v| v as i64).sum::<i64>();
            assert!(!smoother.has_pending());

            let expected = ((input_sum as f64) * (mult as f64)).round() as i64;
            assert_eq!(
                emitted_sum, expected,
                "trial {trial}: mult={mult}, input_sum={input_sum}"
            );
        }
    }

    #[test]
    fn touchpad_like_input_bypasses_smoothing() {
        let mut smoother = AxisSmoother::new();
        let t0 = Instant::now();

        // Small non-120 deltas bypass immediately with the multiplier applied.
        let mut out = Vec::new();
        for (i, d) in [3i16, 7, 12, 18].into_iter().enumerate() {
            let t = t0 + Duration::from_millis((i as u64) * 10);
            out.extend(smoother.push(d, 2.5, t));
        }
        assert!(!smoother.has_pending());
        assert_eq!(out, vec![7, 18, 30, 45]);

        // An incidental 120 inside an active rapid touchpad stream also bypasses.
        let t_incidental = t0 + Duration::from_millis(40);
        let inc = smoother.push(120, 2.5, t_incidental);
        assert!(!smoother.has_pending());
        assert_eq!(inc, vec![300]);

        // After a pause, a standard 120 wheel notch enters smoothing again.
        let t_wheel = t0 + Duration::from_millis(300);
        assert!(smoother.push(120, 2.5, t_wheel).is_empty());
        assert!(smoother.has_pending());
    }

    #[test]
    fn flushes_early_when_pending_drops_below_one_unit() {
        let mut s = AxisSmoother::new();
        let t0 = Instant::now();
        // 360 drained for 40 ms leaves ~114.9; subtracting 120 leaves P ~ -5.1,
        // which drops below 1 unit in ~64 ms (< 150 ms max settle) and flushes early.
        s.push(360, 1.0, t0);
        let mut sum = 0i64;
        for i in 1..=5 {
            sum += s
                .tick(SMOOTH_TICK, t0 + Duration::from_millis(i * 8))
                .iter()
                .map(|&v| v as i64)
                .sum::<i64>();
        }
        let t_rev = t0 + Duration::from_millis(40);
        s.push(-120, 1.0, t_rev);
        let (rest, settled_in) = drain_until_idle(&mut s, t_rev, 140);
        sum += rest.iter().map(|&v| v as i64).sum::<i64>();
        assert!(!s.has_pending());
        assert!(settled_in < SMOOTH_MAX_SETTLE);
        assert_eq!(sum, 240);
    }
}
