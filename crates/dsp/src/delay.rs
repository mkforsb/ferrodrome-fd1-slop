//! Tempo-synced ping-pong delay with a damped feedback path.
//!
//! Mono in, stereo out: the first echo comes back on the left, the next on
//! the right one delay later, and so on, each `FDBK` quieter and darker
//! (TONE lowpasses the feedback). Changing the time glides the read heads
//! like tape rather than jumping.

use crate::util::{DelayLine, flush, one_pole_coeff};

/// Longest delay, in seconds.
pub const MAX_DELAY_S: f32 = 2.0;

#[derive(Clone, Debug)]
pub struct PingPong {
    fs: f32,
    l: DelayLine,
    r: DelayLine,
    lp_l: f32,
    lp_r: f32,
    time: f32,
    k_glide: f32,
    /// Samples since anything audible went in or came out.
    quiet: u32,
}

impl PingPong {
    pub fn new(fs: f32) -> Self {
        let max = (MAX_DELAY_S * fs) as usize + 8;
        Self {
            fs,
            l: DelayLine::new(max),
            r: DelayLine::new(max),
            lp_l: 0.0,
            lp_r: 0.0,
            time: fs * 0.25,
            k_glide: one_pole_coeff(0.08, fs),
            quiet: u32::MAX,
        }
    }

    pub fn clear(&mut self) {
        self.l.clear();
        self.r.clear();
        self.lp_l = 0.0;
        self.lp_r = 0.0;
        self.quiet = u32::MAX;
    }

    /// Jump the read heads to `seconds` (no glide).
    pub fn snap_time(&mut self, seconds: f32) {
        self.time = (seconds * self.fs).clamp(2.0, self.l.max_delay());
    }

    /// Whether the delay has gone silent and can be skipped while its input is silent.
    pub fn is_idle(&self) -> bool {
        self.quiet > (MAX_DELAY_S * self.fs) as u32 * 2
    }

    /// One sample. `seconds` is the delay time, `feedback` 0..1, `tone` the
    /// one-pole coefficient of the feedback lowpass.
    #[inline]
    pub fn process(&mut self, x: f32, seconds: f32, feedback: f32, tone: f32) -> (f32, f32) {
        if x == 0.0 && self.is_idle() {
            return (0.0, 0.0);
        }
        let target = (seconds * self.fs).clamp(2.0, self.l.max_delay());
        self.time += (target - self.time) * self.k_glide;
        let dl = self.l.read(self.time);
        let dr = self.r.read(self.time);
        self.lp_l = flush(self.lp_l + (dl - self.lp_l) * tone);
        self.lp_r = flush(self.lp_r + (dr - self.lp_r) * tone);
        self.l.write(x + feedback * self.lp_r);
        self.r.write(feedback * self.lp_l);
        if x.abs() > 1e-6 || dl.abs() > 1e-5 || dr.abs() > 1e-5 {
            self.quiet = 0;
        } else {
            self.quiet = self.quiet.saturating_add(1);
        }
        (dl, dr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echoes_alternate_sides_and_decay() {
        let fs = 48_000.0;
        let mut d = PingPong::new(fs);
        d.snap_time(0.1);
        let n = (fs * 0.45) as usize;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            out.push(d.process(if i == 0 { 1.0 } else { 0.0 }, 0.1, 0.5, 1.0));
        }
        let at = |i: usize| out[i];
        let p = 4_800;
        assert!(at(p).0 > 0.9 && at(p).1.abs() < 1e-3);
        assert!((at(2 * p).1 - 0.5).abs() < 0.05 && at(2 * p).0.abs() < 1e-3);
        assert!((at(3 * p).0 - 0.25).abs() < 0.05);
    }

    #[test]
    fn goes_idle() {
        let fs = 8_000.0;
        let mut d = PingPong::new(fs);
        d.process(1.0, 0.1, 0.9, 0.5);
        for _ in 0..(fs * 30.0) as usize {
            d.process(0.0, 0.1, 0.9, 0.5);
        }
        assert!(d.is_idle());
    }
}
