//! Small DSP building blocks shared by the engines.

use core::f32::consts::PI;

/// xorshift32 white noise.
#[derive(Clone, Debug)]
pub struct Noise {
    state: u32,
}

impl Noise {
    pub fn new(seed: u32) -> Self {
        // Scramble so nearby seeds (0, 1, 2 …) give unrelated sequences.
        let mut s = seed.wrapping_mul(0x9E37_79B9) ^ 0x6A09_E667;
        s ^= s >> 16;
        s = s.wrapping_mul(0x85EB_CA6B);
        s ^= s >> 13;
        Self { state: s.max(1) }
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// Uniform white noise in `[-1, 1)`.
    #[inline]
    pub fn sample(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (2.0 / 16_777_216.0) - 1.0
    }

    /// Uniform in `[0, 1)`.
    #[inline]
    pub fn uniform(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// Approximately standard normal (Irwin–Hall: the sum of four uniforms,
    /// rescaled), bounded to about ±3.5σ, so no single draw runs away.
    #[inline]
    pub fn gauss(&mut self) -> f32 {
        let s = self.uniform() + self.uniform() + self.uniform() + self.uniform();
        (s - 2.0) * 1.732_050_8
    }

    /// True with probability `p`.
    #[inline]
    pub fn chance(&mut self, p: f32) -> bool {
        self.uniform() < p
    }

    /// Uniform integer in `0..n` (`n > 0`).
    #[inline]
    pub fn below(&mut self, n: usize) -> usize {
        ((self.uniform() * n as f32) as usize).min(n.saturating_sub(1))
    }
}

/// Rational tanh approximation, accurate to ~0.2% and exact at the clamp.
#[inline]
pub fn fast_tanh(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

/// `sin(2π·turns)` for any finite input, accurate to ~4e-6.
#[inline]
pub fn sin_turns(turns: f32) -> f32 {
    // Reduce to [-0.5, 0.5), then fold into [-0.25, 0.25].
    let mut t = turns - (turns + 0.5).floor();
    if t > 0.25 {
        t = 0.5 - t;
    } else if t < -0.25 {
        t = -0.5 - t;
    }
    let z = t * (2.0 * PI);
    let z2 = z * z;
    z * (1.0 - z2 / 6.0 * (1.0 - z2 / 20.0 * (1.0 - z2 / 42.0 * (1.0 - z2 / 72.0))))
}

/// `cos(2π·turns)`.
#[inline]
pub fn cos_turns(turns: f32) -> f32 {
    sin_turns(turns + 0.25)
}

/// Coefficient of a one-pole smoother with time constant `tau_s`, applied
/// once every `every` samples at rate `fs`.
#[inline]
pub fn one_pole_coeff(tau_s: f32, fs: f32) -> f32 {
    1.0 - (-1.0 / (tau_s * fs)).exp()
}

/// Per-sample gain that decays by 60 dB in `t60_s`.
#[inline]
pub fn t60_gain(t60_s: f32, fs: f32) -> f32 {
    (-6.907_755 / (t60_s.max(1e-4) * fs)).exp()
}

/// Keep recirculating state out of the (slow on x86) denormal range.
#[inline]
pub fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

/// Phase delay in samples of a first-order section `(b0 + b1 z⁻¹)/(1 + a1 z⁻¹)`
/// at `w` radians per sample.
pub fn phase_delay_first_order(b0: f32, b1: f32, a1: f32, w: f32) -> f32 {
    let (s, c) = w.sin_cos();
    // H = (b0 + b1 e^{-jw}) / (1 + a1 e^{-jw})
    let num = (b0 + b1 * c, -b1 * s);
    let den = (1.0 + a1 * c, -a1 * s);
    let mut phase = num.1.atan2(num.0) - den.1.atan2(den.0);
    // A causal first-order section lags by 0..2π; undo atan2 wrapping.
    if phase > 1e-6 {
        phase -= 2.0 * PI;
    }
    -phase / w.max(1e-6)
}

/// Magnitude of the same first-order section at `w`.
pub fn magnitude_first_order(b0: f32, b1: f32, a1: f32, w: f32) -> f32 {
    let (s, c) = w.sin_cos();
    let num = ((b0 + b1 * c).powi(2) + (b1 * s).powi(2)).sqrt();
    let den = ((1.0 + a1 * c).powi(2) + (a1 * s).powi(2)).sqrt();
    num / den.max(1e-9)
}

/// Zero-delay-feedback state variable filter (Simper/Cytomic).
#[derive(Clone, Debug, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

/// Coefficients for [`Svf`].
#[derive(Clone, Copy, Debug)]
pub struct SvfCoeffs {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl SvfCoeffs {
    pub fn new(fc_hz: f32, q: f32, fs: f32) -> Self {
        let g = (PI * fc_hz.clamp(5.0, 0.45 * fs) / fs).tan();
        Self::from_gk(g, 1.0 / q.max(0.05))
    }

    /// From the integrator gain `g = tan(π·fc/fs)` (possibly prewarped) and
    /// damping `k = 1/Q`.
    pub fn from_gk(g: f32, k: f32) -> Self {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        Self { a1, a2, a3, k }
    }
}

impl Svf {
    /// Returns `(lowpass, bandpass, highpass)`.
    #[inline]
    pub fn process(&mut self, x: f32, c: &SvfCoeffs) -> (f32, f32, f32) {
        let v3 = x - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        (v2, v1, x - c.k * v1 - v2)
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Power-of-two ring buffer with fractional (cubic Hermite) reads.
#[derive(Clone, Debug)]
pub struct DelayLine {
    buf: Vec<f32>,
    mask: usize,
    pos: usize,
}

impl DelayLine {
    /// A line that can delay by at least `max_delay` samples.
    pub fn new(max_delay: usize) -> Self {
        let size = (max_delay + 4).next_power_of_two();
        Self {
            buf: vec![0.0; size],
            mask: size - 1,
            pos: 0,
        }
    }

    /// Longest delay [`DelayLine::read`] supports.
    pub fn max_delay(&self) -> f32 {
        (self.buf.len() - 4) as f32
    }

    #[inline]
    pub fn write(&mut self, x: f32) {
        self.pos = (self.pos + 1) & self.mask;
        self.buf[self.pos] = flush(x);
    }

    /// The sample written `k` writes ago (`0` = most recent).
    #[inline]
    pub fn tap(&self, k: usize) -> f32 {
        self.buf[self.pos.wrapping_sub(k) & self.mask]
    }

    /// Output delayed by `d` samples relative to the next [`write`](Self::write),
    /// i.e. call `read` before `write` in a sample loop. `d` is clamped to
    /// `2..=max_delay`.
    #[inline]
    pub fn read(&self, d: f32) -> f32 {
        let p = d.clamp(2.0, self.max_delay()) - 1.0;
        let i = p as usize;
        let f = p - i as f32;
        let xm1 = self.tap(i - 1);
        let x0 = self.tap(i);
        let x1 = self.tap(i + 1);
        let x2 = self.tap(i + 2);
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * f + c2) * f + c1) * f + x0
    }

    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|x| *x = 0.0);
    }
}

/// Transposed direct form II biquad with RBJ cookbook designs.
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    s1: f32,
    s2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            s1: 0.0,
            s2: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BiquadShape {
    LowShelf,
    Peak,
    HighShelf,
}

impl Biquad {
    /// Set the coefficients, keeping the state. Shelves use slope 1.
    pub fn design(&mut self, shape: BiquadShape, hz: f32, q: f32, db: f32, fs: f32) {
        let a = 10f32.powf(db / 40.0);
        let w = core::f32::consts::TAU * hz.clamp(10.0, 0.45 * fs) / fs;
        let (sn, cs) = w.sin_cos();
        let (b0, b1, b2, a0, a1, a2) = match shape {
            BiquadShape::Peak => {
                let alpha = sn / (2.0 * q.max(0.05));
                (
                    1.0 + alpha * a,
                    -2.0 * cs,
                    1.0 - alpha * a,
                    1.0 + alpha / a,
                    -2.0 * cs,
                    1.0 - alpha / a,
                )
            }
            BiquadShape::LowShelf | BiquadShape::HighShelf => {
                let alpha = sn / 2.0 * core::f32::consts::SQRT_2;
                let k = 2.0 * a.sqrt() * alpha;
                if shape == BiquadShape::LowShelf {
                    (
                        a * ((a + 1.0) - (a - 1.0) * cs + k),
                        2.0 * a * ((a - 1.0) - (a + 1.0) * cs),
                        a * ((a + 1.0) - (a - 1.0) * cs - k),
                        (a + 1.0) + (a - 1.0) * cs + k,
                        -2.0 * ((a - 1.0) + (a + 1.0) * cs),
                        (a + 1.0) + (a - 1.0) * cs - k,
                    )
                } else {
                    (
                        a * ((a + 1.0) + (a - 1.0) * cs + k),
                        -2.0 * a * ((a - 1.0) + (a + 1.0) * cs),
                        a * ((a + 1.0) + (a - 1.0) * cs - k),
                        (a + 1.0) - (a - 1.0) * cs + k,
                        2.0 * ((a - 1.0) - (a + 1.0) * cs),
                        (a + 1.0) - (a - 1.0) * cs - k,
                    )
                }
            }
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.s1;
        self.s1 = flush(self.b1 * x - self.a1 * y + self.s2);
        self.s2 = flush(self.b2 * x - self.a2 * y);
        y
    }

    /// Magnitude response at `hz`.
    pub fn magnitude(&self, hz: f32, fs: f32) -> f32 {
        let w = core::f32::consts::TAU * hz / fs;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let num = (
            self.b0 + self.b1 * c1 + self.b2 * c2,
            -(self.b1 * s1 + self.b2 * s2),
        );
        let den = (
            1.0 + self.a1 * c1 + self.a2 * c2,
            -(self.a1 * s1 + self.a2 * s2),
        );
        ((num.0 * num.0 + num.1 * num.1) / (den.0 * den.0 + den.1 * den.1)).sqrt()
    }
}

#[derive(Clone, Debug, Default)]
pub struct DcBlocker {
    x1: f32,
    y1: f32,
}

impl DcBlocker {
    /// `r` close to 1: `1 - 2π·fc/fs`.
    #[inline]
    pub fn process(&mut self, x: f32, r: f32) -> f32 {
        let y = x - self.x1 + r * self.y1;
        self.x1 = x;
        self.y1 = flush(y);
        y
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_turns_is_accurate() {
        for i in -2000..=2000 {
            let t = i as f32 * 0.001_37;
            let err = (sin_turns(t) - (t * 2.0 * PI).sin()).abs();
            assert!(err < 2e-5, "t={t} err={err}");
            let err = (cos_turns(t) - (t * 2.0 * PI).cos()).abs();
            assert!(err < 2e-5, "t={t} err={err}");
        }
    }

    #[test]
    fn fast_tanh_close() {
        for i in -40..=40 {
            let x = i as f32 * 0.1;
            assert!((fast_tanh(x) - x.tanh()).abs() < 0.025, "x={x}");
        }
    }

    #[test]
    fn noise_is_bounded_and_centered() {
        let mut n = Noise::new(1);
        let mut sum = 0.0f64;
        for _ in 0..100_000 {
            let x = n.sample();
            assert!((-1.0..1.0).contains(&x));
            sum += x as f64;
        }
        assert!((sum / 100_000.0).abs() < 0.01);
    }

    #[test]
    fn delay_line_delays() {
        let mut d = DelayLine::new(64);
        let mut out = Vec::new();
        for n in 0..40 {
            out.push(d.read(10.0));
            d.write(if n == 0 { 1.0 } else { 0.0 });
        }
        assert_eq!(out.iter().position(|&x| x == 1.0), Some(10));
        // Fractional delays land between samples.
        let mut d = DelayLine::new(64);
        let mut out = Vec::new();
        for n in 0..40 {
            out.push(d.read(10.5));
            d.write(n as f32);
        }
        assert!((out[20] - 9.5).abs() < 1e-4, "{}", out[20]);
    }

    #[test]
    fn gauss_has_unit_variance() {
        let mut n = Noise::new(5);
        let xs: Vec<f32> = (0..50_000).map(|_| n.gauss()).collect();
        let mean = xs.iter().sum::<f32>() / xs.len() as f32;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / xs.len() as f32;
        assert!(
            mean.abs() < 0.02 && (var - 1.0).abs() < 0.05,
            "{mean} {var}"
        );
        assert!(xs.iter().all(|x| x.abs() < 3.5));
    }

    #[test]
    fn biquad_shelves_and_peak_hit_their_gains() {
        let fs = 48_000.0;
        let mut b = Biquad::default();
        b.design(BiquadShape::LowShelf, 200.0, 0.7, 12.0, fs);
        assert!((20.0 * b.magnitude(20.0, fs).log10() - 12.0).abs() < 0.5);
        assert!(20.0 * b.magnitude(10_000.0, fs).log10() < 0.5);
        b.design(BiquadShape::HighShelf, 5_000.0, 0.7, -12.0, fs);
        assert!((20.0 * b.magnitude(20_000.0, fs).log10() + 12.0).abs() < 1.0);
        b.design(BiquadShape::Peak, 1_000.0, 0.7, 6.0, fs);
        assert!((20.0 * b.magnitude(1_000.0, fs).log10() - 6.0).abs() < 0.1);
        b.design(BiquadShape::Peak, 1_000.0, 0.7, 0.0, fs);
        assert!((b.magnitude(3_000.0, fs) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn allpass_phase_delay_at_dc() {
        // (a + z⁻¹)/(1 + a z⁻¹) has (1 - a)/(1 + a) samples of delay at DC.
        let a = -0.5;
        let d = phase_delay_first_order(a, 1.0, a, 1e-3);
        assert!((d - (1.0 - a) / (1.0 + a)).abs() < 1e-2, "{d}");
        assert!((magnitude_first_order(a, 1.0, a, 1.0) - 1.0).abs() < 1e-5);
    }
}
