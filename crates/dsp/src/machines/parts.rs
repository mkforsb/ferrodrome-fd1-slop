//! Small parts the machines are built from: resonant modes, tempo-synced
//! cycles, smooth random wander, envelopes and a pitch-dropping thump.

use crate::util::{Noise, cos_turns, flush, one_pole_coeff, sin_turns, t60_gain};

/// A complex one-pole resonator, `z ← z·r·e^{iω} + x`: a unit impulse rings
/// as a decaying sine of unit amplitude at any frequency (the XK-1 MODAL mode).
#[derive(Clone, Debug, Default)]
pub struct Mode {
    re: f32,
    im: f32,
    cr: f32,
    ci: f32,
}

impl Mode {
    pub fn set(&mut self, hz: f32, t60: f32, fs: f32) {
        let r = t60_gain(t60, fs);
        let w = hz.clamp(1.0, 0.45 * fs) / fs;
        self.cr = r * cos_turns(w);
        self.ci = r * sin_turns(w);
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let (re, im) = (self.re, self.im);
        self.re = flush(re * self.cr - im * self.ci + x);
        self.im = flush(re * self.ci + im * self.cr);
        self.im
    }

    pub fn energy(&self) -> f32 {
        self.re * self.re + self.im * self.im
    }

    pub fn reset(&mut self) {
        self.re = 0.0;
        self.im = 0.0;
    }
}

/// A tempo-synced machine cycle. [`Cycle::start`] arms it so the first event
/// lands a few milliseconds after an active period begins (by then even a
/// short spin-up has the machine at power); after that it fires once per
/// cycle while the gate is open.
#[derive(Clone, Debug, Default)]
pub struct Cycle {
    // f64: a slow cycle adds tiny increments for seconds on end.
    phase: f64,
    inc: f64,
    pending: u32,
}

/// Delay of the first event after an active period starts.
pub const START_LEAD_S: f32 = 0.004;

impl Cycle {
    pub fn set(&mut self, beats: f32, beat_s: f32, fs: f32) {
        self.inc = 1.0 / (beats as f64 * beat_s as f64 * fs as f64).max(1.0);
    }

    pub fn start(&mut self, fs: f32) {
        self.pending = (START_LEAD_S * fs) as u32 + 1;
    }

    pub fn phase(&self) -> f32 {
        self.phase as f32
    }

    /// Cycle length in seconds.
    pub fn period(&self, fs: f32) -> f32 {
        1.0 / (self.inc as f32 * fs).max(1e-6)
    }

    /// Advance one sample; true on an event.
    #[inline]
    pub fn tick(&mut self, gate: bool) -> bool {
        if self.pending > 0 {
            self.pending -= 1;
            if self.pending == 0 {
                self.phase = 0.0;
                return gate;
            }
            return false;
        }
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            return gate;
        }
        false
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.pending = 0;
    }
}

/// Smooth random wander in about `-1..=1`: an Ornstein–Uhlenbeck process
/// (always pulled back to the centre, so it never drifts off) followed by a
/// one-pole smoother. `rate_hz` sets how fast it moves.
#[derive(Clone, Debug, Default)]
pub struct Wander {
    x: f32,
    y: f32,
    n: u8,
}

/// [`Wander::tick`] steps the process once per this many samples.
const WANDER_EVERY: u8 = 16;

impl Wander {
    /// Per-sample use: steps every [`WANDER_EVERY`] samples (plenty for
    /// wander rates up to ~20 Hz) and holds in between.
    #[inline]
    pub fn tick(&mut self, rate_hz: f32, fs: f32, rng: &mut Noise) -> f32 {
        if self.n == 0 {
            self.n = WANDER_EVERY;
            self.step(rate_hz, WANDER_EVERY as f32 / fs, rng);
        }
        self.n -= 1;
        self.y.clamp(-1.0, 1.0)
    }

    #[inline]
    pub fn step(&mut self, rate_hz: f32, dt: f32, rng: &mut Noise) -> f32 {
        let theta = core::f32::consts::TAU * rate_hz;
        let k = (theta * dt).min(1.0);
        // Stationary standard deviation ≈ 0.45.
        let sigma = 0.45 * (2.0 * theta).sqrt();
        self.x += -self.x * k + sigma * dt.sqrt() * rng.gauss();
        self.x = self.x.clamp(-1.5, 1.5);
        self.y += (self.x - self.y) * k;
        self.y.clamp(-1.0, 1.0)
    }

    pub fn value(&self) -> f32 {
        self.y.clamp(-1.0, 1.0)
    }
}

/// `d^e`, recomputed only when `d` has moved: the drive changes slowly and
/// `powf` per sample is expensive.
#[derive(Clone, Debug, Default)]
pub struct Pow {
    d: f32,
    v: f32,
}

impl Pow {
    #[inline]
    pub fn get(&mut self, d: f32, e: f32) -> f32 {
        if (d - self.d).abs() > 1e-3
            || (d == 0.0) != (self.d == 0.0)
            || (d == 1.0) != (self.d == 1.0)
        {
            self.d = d;
            self.v = d.max(0.0).powf(e);
        }
        self.v
    }
}

/// Exponentially decaying envelope.
#[derive(Clone, Debug, Default)]
pub struct Decay {
    v: f32,
    mul: f32,
}

impl Decay {
    pub fn trigger(&mut self, level: f32, t60: f32, fs: f32) {
        self.v = level;
        self.mul = t60_gain(t60, fs);
    }

    /// Re-trigger only if louder than what is still sounding.
    pub fn trigger_max(&mut self, level: f32, t60: f32, fs: f32) {
        if level >= self.v {
            self.trigger(level, t60, fs);
        }
    }

    #[inline]
    pub fn tick(&mut self) -> f32 {
        let v = self.v;
        self.v = flush(self.v * self.mul);
        v
    }

    pub fn value(&self) -> f32 {
        self.v
    }

    pub fn reset(&mut self) {
        self.v = 0.0;
    }
}

/// One-pole follower towards a target (attack/release envelope).
#[derive(Clone, Debug, Default)]
pub struct Follow {
    v: f32,
}

impl Follow {
    #[inline]
    pub fn tick(&mut self, target: f32, k: f32) -> f32 {
        self.v = flush(self.v + (target - self.v) * k);
        self.v
    }

    pub fn value(&self) -> f32 {
        self.v
    }

    pub fn reset(&mut self) {
        self.v = 0.0;
    }
}

/// One-pole lowpass.
#[derive(Clone, Debug, Default)]
pub struct Lp1 {
    y: f32,
    k: f32,
}

impl Lp1 {
    pub fn set(&mut self, hz: f32, fs: f32) {
        self.k = one_pole_coeff(1.0 / (core::f32::consts::TAU * hz.max(1.0)), fs).min(1.0);
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        self.y = flush(self.y + (x - self.y) * self.k);
        self.y
    }

    pub fn reset(&mut self) {
        self.y = 0.0;
    }
}

/// A sine "thump" whose pitch drops from `ratio × hz` to `hz`: the body of an
/// impact (press, clunk, piston).
#[derive(Clone, Debug, Default)]
pub struct Thump {
    phase: f32,
    hz: f32,
    sweep: f32,
    sweep_mul: f32,
    amp: Decay,
}

impl Thump {
    pub fn trigger(&mut self, level: f32, hz: f32, ratio: f32, drop_s: f32, t60: f32, fs: f32) {
        self.phase = 0.0;
        self.hz = hz;
        self.sweep = (ratio - 1.0).max(0.0);
        self.sweep_mul = t60_gain(drop_s, fs);
        self.amp.trigger(level, t60, fs);
    }

    #[inline]
    pub fn tick(&mut self, fs: f32) -> f32 {
        let a = self.amp.tick();
        if a == 0.0 {
            return 0.0;
        }
        let f = self.hz * (1.0 + self.sweep);
        self.sweep *= self.sweep_mul;
        self.phase += f / fs;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        a * sin_turns(self.phase)
    }

    pub fn reset(&mut self) {
        self.amp.reset();
    }
}

/// `Σ a_k sin(k·φ)` for k = 1..=n by the Chebyshev recurrence: one sine and
/// one cosine per sample for any number of harmonics. Harmonics at or above
/// `limit` cycles per sample are left out.
#[inline]
pub fn harmonics(phase: f32, inc: f32, amps: &[f32], limit: f32) -> f32 {
    let s1 = sin_turns(phase);
    let c2 = 2.0 * cos_turns(phase);
    let (mut prev, mut cur) = (0.0f32, s1);
    let mut out = 0.0;
    for (k, &a) in amps.iter().enumerate() {
        if (k + 1) as f32 * inc >= limit {
            break;
        }
        out += a * cur;
        let next = c2 * cur - prev;
        prev = cur;
        cur = next;
    }
    out
}

/// Advance a phase in turns, wrapping to `0..1`. True when it wrapped.
#[inline]
pub fn advance(phase: &mut f32, inc: f32) -> bool {
    *phase += inc;
    if *phase >= 1.0 {
        *phase -= phase.floor();
        true
    } else {
        false
    }
}

/// Bandpass of an SVF output normalized to unit peak gain.
#[inline]
pub fn svf_bp(svf: &mut crate::util::Svf, c: &crate::util::SvfCoeffs, x: f32, q: f32) -> f32 {
    svf.process(x, c).1 / q.max(0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harmonics_match_direct_sum() {
        let amps = [1.0, 0.5, 0.25, 0.125];
        for i in 0..100 {
            let p = i as f32 * 0.0137;
            let direct: f32 = amps
                .iter()
                .enumerate()
                .map(|(k, a)| a * sin_turns((k + 1) as f32 * p))
                .sum();
            assert!((harmonics(p, 0.001, &amps, 0.45) - direct).abs() < 1e-4);
        }
        // Band limit drops the top harmonics.
        assert!(
            (harmonics(0.1, 0.2, &amps, 0.45) - sin_turns(0.1) - 0.5 * sin_turns(0.2)).abs() < 1e-4
        );
    }

    #[test]
    fn cycle_fires_soon_after_start_then_once_per_cycle() {
        let fs = 48_000.0;
        let mut c = Cycle::default();
        c.set(1.0, 0.5, fs); // 0.5 s
        c.start(fs);
        let events: Vec<usize> = (0..48_000).filter(|_| c.tick(true)).collect();
        assert_eq!(events.len(), 2);
        assert!(events[0] <= (START_LEAD_S * fs) as usize + 1);
        assert!((events[1] as i64 - events[0] as i64 - 24_000).abs() <= 1);
        // A closed gate fires nothing.
        assert!((0..48_000).all(|_| !c.tick(false)));
    }

    #[test]
    fn wander_stays_centred() {
        let mut w = Wander::default();
        let mut rng = Noise::new(3);
        let mut sum = 0.0;
        let n = 200_000;
        for _ in 0..n {
            let v = w.step(1.0, 1.0 / 1500.0, &mut rng);
            assert!(v.abs() <= 1.0);
            sum += v;
        }
        assert!((sum / n as f32).abs() < 0.1);
    }

    #[test]
    fn mode_rings_at_unit_amplitude() {
        let fs = 48_000.0;
        let mut m = Mode::default();
        m.set(1000.0, 10.0, fs);
        let peak = (0..480)
            .map(|n| m.tick(if n == 0 { 1.0 } else { 0.0 }))
            .fold(0.0f32, |a, b| a.max(b.abs()));
        assert!((peak - 1.0).abs() < 0.01);
    }
}
