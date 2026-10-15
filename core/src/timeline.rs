//! The reveal animation: the split sweeps from all-A to all-B.

use crate::geometry::{Frame, Split};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reveal {
    /// Seconds for one sweep (1 to 6 in the UI).
    pub sweep: f64,
    /// Seconds held at each end.
    pub hold: f64,
    /// Sweep back again, so the clip loops cleanly.
    pub round_trip: bool,
}

/// Cubic ease in-out: zero velocity at both ends.
pub fn ease_in_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
}

impl Reveal {
    pub fn duration(&self) -> f64 {
        let one = self.hold + self.sweep + self.hold;
        if self.round_trip { one + self.sweep } else { one }
    }

    /// Progress from all-A (0) to all-B (1) at `t` seconds.
    pub fn progress(&self, t: f64) -> f64 {
        let s = self.sweep.max(1e-6);
        let t = t - self.hold;
        if t <= 0.0 {
            0.0
        } else if t < s {
            ease_in_out(t / s)
        } else if !self.round_trip || t < s + self.hold {
            1.0
        } else {
            1.0 - ease_in_out((t - s - self.hold) / s)
        }
    }

    /// The split at `t`: same angle, offset swept past both edges far enough
    /// that a feathered edge (`feather` frame units wide) clears the frame.
    pub fn split_at(&self, t: f64, base: &Split, frame: &Frame, feather: f64) -> Split {
        let reach = base.extent(frame) + feather / 2.0 + 1e-3;
        let p = self.progress(t);
        Split { angle_deg: base.angle_deg, offset: reach * (1.0 - 2.0 * p) }
    }

    pub fn frames(&self, fps: f64) -> u32 {
        (self.duration() * fps).round().max(1.0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::v2;

    #[test]
    fn ease_is_symmetric_and_flat_at_the_ends() {
        assert_eq!(ease_in_out(0.0), 0.0);
        assert_eq!(ease_in_out(1.0), 1.0);
        assert!((ease_in_out(0.5) - 0.5).abs() < 1e-12);
        for t in [0.1, 0.27, 0.4] {
            assert!((ease_in_out(t) + ease_in_out(1.0 - t) - 1.0).abs() < 1e-12);
        }
        let h = 1e-5;
        assert!(ease_in_out(h) / h < 1e-6);
    }

    #[test]
    fn holds_then_sweeps() {
        let r = Reveal { sweep: 2.0, hold: 0.5, round_trip: false };
        assert_eq!(r.duration(), 3.0);
        assert_eq!(r.progress(0.25), 0.0);
        assert!((r.progress(1.5) - 0.5).abs() < 1e-12);
        assert_eq!(r.progress(2.75), 1.0);
        assert_eq!(r.frames(30.0), 90);
    }

    #[test]
    fn round_trip_returns_to_a() {
        let r = Reveal { sweep: 1.0, hold: 0.25, round_trip: true };
        assert_eq!(r.duration(), 2.5);
        assert_eq!(r.progress(1.4), 1.0);
        assert!((r.progress(2.0) - 0.5).abs() < 1e-12);
        assert_eq!(r.progress(2.5), 0.0);
    }

    #[test]
    fn sweep_clears_the_frame_at_both_ends() {
        let f = Frame::new(1080.0, 1920.0);
        let base = Split { angle_deg: 30.0, offset: 0.1 };
        let r = Reveal { sweep: 3.0, hold: 0.0, round_trip: false };
        let corners = [v2(-0.5, -0.889), v2(0.5, -0.889), v2(-0.5, 0.889), v2(0.5, 0.889)];
        let start = r.split_at(0.0, &base, &f, 0.2);
        let end = r.split_at(3.0, &base, &f, 0.2);
        for c in corners {
            assert!(start.distance(c) < -0.1, "all A at the start");
            assert!(end.distance(c) > 0.1, "all B at the end");
        }
        assert_eq!(start.angle_deg, 30.0);
    }
}
