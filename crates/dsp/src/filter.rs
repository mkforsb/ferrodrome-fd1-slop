//! The track's insert: a Moog-style ladder filter and a three-band EQ.
//!
//! The ladder is four one-pole lowpasses in series with feedback from the
//! last one, solved without a unit delay in the loop (Zavalishin's
//! topology-preserving transform), so cutoff and resonance track exactly
//! and it stays stable up to the edge of self-oscillation. The input passes
//! a tanh before the poles: DRIVE pushes the machine into it, and it keeps a
//! screaming resonance bounded. The stages are tapped to give 12 and 24 dB
//! lowpasses, a bandpass and 12 and 24 dB highpasses (Huovilainen's mode
//! mixing):
//!
//! | mode | output |
//! | --- | --- |
//! | LP12 | `y2` |
//! | LP24 | `y4` |
//! | BP | `4·y2 − 8·y3 + 4·y4` |
//! | HP12 | `u − 2·y1 + y2` |
//! | HP24 | `u − 4·y1 + 6·y2 − 4·y3 + y4` |
//!
//! where `u` is the input after feedback and `yN` the N-th stage.

use crate::params::{
    FILTER_BP, FILTER_HP12, FILTER_HP24, FILTER_LP12, FILTER_LP24, FILTER_OFF, map,
};
use crate::util::{Biquad, BiquadShape, Svf, SvfCoeffs, fast_tanh, flush};

/// EQ shelf corners.
pub const EQ_LOW_HZ: f32 = 110.0;
pub const EQ_HIGH_HZ: f32 = 5_000.0;
const EQ_MID_Q: f32 = 0.9;

#[derive(Clone, Copy, Debug)]
pub struct LadderCoeffs {
    pub mode: u32,
    /// TPT one-pole gain `g / (1 + g)`.
    big_g: f32,
    k: f32,
    drive: f32,
    /// Input boost making up for the lowpass passband the feedback takes away.
    comp: f32,
    out: f32,
}

impl LadderCoeffs {
    pub fn new(mode: u32, hz: f32, reso: f32, drive: f32, fs: f32) -> Self {
        let g = (core::f32::consts::PI * hz.clamp(10.0, 0.45 * fs) / fs).tan();
        let k = map::reso_k(reso);
        let drive = map::filter_drive(drive);
        let lowpass = matches!(mode, FILTER_LP12 | FILTER_LP24);
        Self {
            mode,
            big_g: g / (1.0 + g),
            k,
            drive,
            comp: if lowpass { 1.0 + 0.5 * k } else { 1.0 },
            out: 1.0 / drive.sqrt(),
        }
    }

    pub fn is_off(&self) -> bool {
        self.mode == FILTER_OFF
    }
}

#[derive(Clone, Debug, Default)]
pub struct Ladder {
    s: [f32; 4],
}

impl Ladder {
    pub fn reset(&mut self) {
        self.s = [0.0; 4];
    }

    #[inline]
    pub fn process(&mut self, x: f32, c: &LadderCoeffs) -> f32 {
        let g = c.big_g;
        let h = 1.0 - g;
        // Each stage is y = G·x + (1 − G)·s, so the whole cascade is
        // y4 = G⁴·u + S with S from the states; solve the feedback for u.
        let s = &self.s;
        let big_s = g * g * g * h * s[0] + g * g * h * s[1] + g * h * s[2] + h * s[3];
        let g4 = g * g * g * g;
        let u = (x * c.drive * c.comp - c.k * big_s) / (1.0 + c.k * g4);
        let u = fast_tanh(u);
        let mut y = [0.0f32; 4];
        let mut input = u;
        for i in 0..4 {
            let v = (input - self.s[i]) * g;
            y[i] = v + self.s[i];
            self.s[i] = flush(y[i] + v);
            input = y[i];
        }
        let out = match c.mode {
            FILTER_LP12 => y[1],
            FILTER_LP24 => y[3],
            FILTER_BP => 4.0 * y[1] - 8.0 * y[2] + 4.0 * y[3],
            FILTER_HP12 => u - 2.0 * y[0] + y[1],
            FILTER_HP24 => u - 4.0 * y[0] + 6.0 * y[1] - 4.0 * y[2] + y[3],
            _ => x,
        };
        out * c.out
    }
}

/// Magnitude of the (linear, analog prototype) ladder at `f` Hz.
pub fn ladder_magnitude(mode: u32, cutoff_hz: f32, reso: f32, f: f32) -> f32 {
    if mode == FILTER_OFF {
        return 1.0;
    }
    let k = map::reso_k(reso);
    let comp = if matches!(mode, FILTER_LP12 | FILTER_LP24) {
        1.0 + 0.5 * k
    } else {
        1.0
    };
    // One pole: 1 / (1 + jw).
    let w = f / cutoff_hz.max(1.0);
    let d = 1.0 + w * w;
    let h1 = C(1.0 / d, -w / d);
    let h2 = h1.mul(h1);
    let h3 = h2.mul(h1);
    let h4 = h2.mul(h2);
    let u = C(comp, 0.0).div(C(1.0 + k * h4.0, k * h4.1));
    let y = |h: C| u.mul(h);
    let tap = match mode {
        FILTER_LP12 => y(h2),
        FILTER_LP24 => y(h4),
        FILTER_BP => y(h2.scale(4.0).add(h3.scale(-8.0)).add(h4.scale(4.0))),
        FILTER_HP12 => y(C(1.0, 0.0).add(h1.scale(-2.0)).add(h2)),
        _ => y(C(1.0, 0.0)
            .add(h1.scale(-4.0))
            .add(h2.scale(6.0))
            .add(h3.scale(-4.0))
            .add(h4)),
    };
    tap.abs()
}

#[derive(Clone, Copy, Debug)]
struct C(f32, f32);

impl C {
    fn mul(self, o: C) -> C {
        C(self.0 * o.0 - self.1 * o.1, self.0 * o.1 + self.1 * o.0)
    }
    fn add(self, o: C) -> C {
        C(self.0 + o.0, self.1 + o.1)
    }
    fn scale(self, k: f32) -> C {
        C(self.0 * k, self.1 * k)
    }
    fn div(self, o: C) -> C {
        let d = (o.0 * o.0 + o.1 * o.1).max(1e-20);
        C(
            (self.0 * o.0 + self.1 * o.1) / d,
            (self.1 * o.0 - self.0 * o.1) / d,
        )
    }
    fn abs(self) -> f32 {
        (self.0 * self.0 + self.1 * self.1).sqrt()
    }
}

/// One EQ band as a state-variable filter: `m0·x + m1·band + m2·low`
/// (A. Simper, "Linear trapezoidal state variable filter"). The same
/// responses as the RBJ cookbook biquads, but built from the SVF's
/// integrators, so the coefficients can move every block (VARIANCE, MUTATE,
/// a knob being turned) without clicks: a direct-form biquad whose
/// coefficients jump while it holds a lot of low-frequency energy does not.
#[derive(Clone, Copy, Debug)]
struct Band {
    c: SvfCoeffs,
    m: [f32; 3],
}

impl Band {
    fn bell(hz: f32, q: f32, db: f32, fs: f32) -> Self {
        let a = 10f32.powf(db / 40.0);
        let g = prewarp(hz, fs);
        let k = 1.0 / (q * a);
        Self {
            c: SvfCoeffs::from_gk(g, k),
            m: [1.0, k * (a * a - 1.0), 0.0],
        }
    }

    fn low_shelf(hz: f32, db: f32, fs: f32) -> Self {
        let a = 10f32.powf(db / 40.0);
        let g = prewarp(hz, fs) / a.sqrt();
        let k = core::f32::consts::SQRT_2;
        Self {
            c: SvfCoeffs::from_gk(g, k),
            m: [1.0, k * (a - 1.0), a * a - 1.0],
        }
    }

    fn high_shelf(hz: f32, db: f32, fs: f32) -> Self {
        let a = 10f32.powf(db / 40.0);
        let g = prewarp(hz, fs) * a.sqrt();
        let k = core::f32::consts::SQRT_2;
        Self {
            c: SvfCoeffs::from_gk(g, k),
            m: [a * a, k * (1.0 - a) * a, 1.0 - a * a],
        }
    }

    #[inline]
    fn process(&self, svf: &mut Svf, x: f32) -> f32 {
        let (low, band, _) = svf.process(x, &self.c);
        self.m[0] * x + self.m[1] * band + self.m[2] * low
    }
}

fn prewarp(hz: f32, fs: f32) -> f32 {
    (core::f32::consts::PI * hz.clamp(10.0, 0.45 * fs) / fs).tan()
}

/// Low shelf, peak and high shelf on up to two channels, built from SVFs
/// (see [`Band`]). Always running: switching an EQ in and out is a click.
#[derive(Clone, Debug)]
pub struct SvfEq {
    bands: [Band; 3],
    state: [[Svf; 3]; 2],
    /// (low dB, mid dB, mid Hz, high dB) the bands were set for.
    key: [f32; 4],
}

impl SvfEq {
    pub fn new() -> Self {
        let flat = Band::bell(1_000.0, 1.0, 0.0, 48_000.0);
        Self {
            bands: [flat; 3],
            state: Default::default(),
            key: [f32::NAN; 4],
        }
    }

    /// Set the bands (cheap enough to call every control block).
    pub fn set(&mut self, key: [f32; 4], low_hz: f32, mid_q: f32, high_hz: f32, fs: f32) {
        if key == self.key {
            return;
        }
        self.key = key;
        let [low, mid, mid_hz, high] = key;
        self.bands = [
            Band::low_shelf(low_hz, low, fs),
            Band::bell(mid_hz, mid_q, mid, fs),
            Band::high_shelf(high_hz, high, fs),
        ];
    }

    #[inline]
    pub fn process(&mut self, ch: usize, x: f32) -> f32 {
        let state = &mut self.state[ch];
        let mut y = x;
        for (band, svf) in self.bands.iter().zip(state.iter_mut()) {
            y = band.process(svf, y);
        }
        y
    }
}

impl Default for SvfEq {
    fn default() -> Self {
        Self::new()
    }
}

/// The track EQ: low shelf, sweepable mid peak and high shelf, mono.
#[derive(Clone, Debug, Default)]
pub struct TrackEq {
    eq: SvfEq,
}

impl TrackEq {
    pub fn new() -> Self {
        Self { eq: SvfEq::new() }
    }

    /// From knob positions.
    pub fn update(&mut self, low: f32, mid: f32, freq: f32, high: f32, fs: f32) {
        let key = [
            map::eq_db(low),
            map::eq_db(mid),
            map::eq_freq_hz(freq),
            map::eq_db(high),
        ];
        self.eq.set(key, EQ_LOW_HZ, EQ_MID_Q, EQ_HIGH_HZ, fs);
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.eq.process(0, x)
    }

    /// Same response as the running EQ, as biquads, for drawing.
    fn design([low, mid, freq, high]: [f32; 4], fs: f32) -> [Biquad; 3] {
        let mut b = [Biquad::default(); 3];
        b[0].design(BiquadShape::LowShelf, EQ_LOW_HZ, 0.7, low, fs);
        b[1].design(BiquadShape::Peak, freq, EQ_MID_Q, mid, fs);
        b[2].design(BiquadShape::HighShelf, EQ_HIGH_HZ, 0.7, high, fs);
        b
    }
}

/// The insert's response in dB at `f` Hz for a patch's knob positions, for
/// the panel's curve. `drive` is the machine's spin-up (for RAMP→).
#[allow(clippy::too_many_arguments)]
pub fn response_db(
    mode: u32,
    cutoff: f32,
    reso: f32,
    env: f32,
    drive: f32,
    eq: [f32; 4],
    f: f32,
    fs: f32,
) -> f32 {
    let hz = map::cutoff_hz(cutoff) * (map::env_octaves(env) * drive).exp2();
    let ladder = ladder_magnitude(mode, hz.clamp(10.0, 0.45 * fs), reso, f);
    let bands = TrackEq::design(
        [
            map::eq_db(eq[0]),
            map::eq_db(eq[1]),
            map::eq_freq_hz(eq[2]),
            map::eq_db(eq[3]),
        ],
        fs,
    );
    let eq = bands.iter().map(|b| b.magnitude(f, fs)).product::<f32>();
    20.0 * (ladder * eq).max(1e-6).log10()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::Noise;

    /// Gain of the running filter for a sine at `f`.
    fn sine_gain(mode: u32, cutoff: f32, reso: f32, f: f32) -> f32 {
        let fs = 48_000.0;
        let c = LadderCoeffs::new(mode, cutoff, reso, 0.0, fs);
        let mut l = Ladder::default();
        let n = 48_000;
        let amp = 0.05;
        let mut peak = 0.0f32;
        for i in 0..n {
            let x = amp * (core::f32::consts::TAU * f * i as f32 / fs).sin();
            let y = l.process(x, &c);
            if i > n / 2 {
                peak = peak.max(y.abs());
            }
        }
        peak / amp
    }

    #[test]
    fn modes_pass_and_stop_where_they_should() {
        let fc = 1_000.0;
        assert!(sine_gain(FILTER_LP24, fc, 0.0, 100.0) > 0.9);
        assert!(sine_gain(FILTER_LP24, fc, 0.0, 8_000.0) < 0.01);
        assert!(sine_gain(FILTER_LP12, fc, 0.0, 8_000.0) < 0.05);
        assert!(
            sine_gain(FILTER_LP12, fc, 0.0, 8_000.0) > sine_gain(FILTER_LP24, fc, 0.0, 8_000.0)
        );
        assert!(sine_gain(FILTER_HP24, fc, 0.0, 10_000.0) > 0.9);
        assert!(sine_gain(FILTER_HP24, fc, 0.0, 120.0) < 0.01);
        assert!(sine_gain(FILTER_HP12, fc, 0.0, 120.0) > sine_gain(FILTER_HP24, fc, 0.0, 120.0));
        let bp = sine_gain(FILTER_BP, fc, 0.0, fc);
        assert!(bp > 0.8 && bp < 1.2, "{bp}");
        assert!(sine_gain(FILTER_BP, fc, 0.0, 100.0) < 0.1);
        assert!(sine_gain(FILTER_BP, fc, 0.0, 10_000.0) < 0.1);
    }

    #[test]
    fn resonance_peaks_at_the_cutoff() {
        let flat = sine_gain(FILTER_LP24, 1_000.0, 0.0, 1_000.0);
        let res = sine_gain(FILTER_LP24, 1_000.0, 0.85, 1_000.0);
        assert!(res > 2.0 * flat, "{flat} {res}");
    }

    #[test]
    fn running_filter_matches_the_drawn_response() {
        for mode in [
            FILTER_LP12,
            FILTER_LP24,
            FILTER_BP,
            FILTER_HP12,
            FILTER_HP24,
        ] {
            for f in [200.0, 1_000.0, 3_000.0] {
                let run = sine_gain(mode, 1_000.0, 0.3, f);
                let drawn = ladder_magnitude(mode, 1_000.0, 0.3, f);
                // The digital filter warps a little towards Nyquist.
                assert!(
                    (20.0 * (run / drawn).log10()).abs() < 1.5,
                    "mode {mode} at {f}: {run} vs {drawn}"
                );
            }
        }
    }

    #[test]
    fn stable_and_bounded_at_full_resonance_and_drive() {
        let fs = 48_000.0;
        let mut rng = Noise::new(1);
        for mode in 1..=5 {
            for hz in [20.0, 500.0, 15_000.0, 30_000.0] {
                let c = LadderCoeffs::new(mode, hz, 1.0, 1.0, fs);
                let mut l = Ladder::default();
                for i in 0..48_000 {
                    let x = if i < 24_000 { rng.sample() } else { 0.0 };
                    let y = l.process(x, &c);
                    assert!(y.is_finite() && y.abs() < 20.0, "mode {mode} {hz} Hz: {y}");
                }
            }
        }
    }

    #[test]
    fn flat_eq_is_transparent_and_boosts_where_asked() {
        let fs = 48_000.0;
        let mut eq = TrackEq::new();
        eq.update(0.5, 0.5, 0.5, 0.5, fs);
        let mut rng = Noise::new(2);
        for _ in 0..1_000 {
            let x = rng.sample();
            assert!((eq.process(x) - x).abs() < 1e-5);
        }
        let db = response_db(
            FILTER_OFF,
            0.5,
            0.0,
            0.5,
            1.0,
            [1.0, 0.5, 0.5, 0.5],
            30.0,
            fs,
        );
        assert!((db - 12.0).abs() < 1.0, "{db}");
        let db = response_db(
            FILTER_OFF,
            0.5,
            0.0,
            0.5,
            1.0,
            [0.5, 0.0, 0.5, 0.5],
            map::eq_freq_hz(0.5),
            fs,
        );
        assert!((db + 12.0).abs() < 0.5, "{db}");
    }

    /// The SVF bands are the cookbook responses the panel draws.
    #[test]
    fn svf_eq_matches_the_drawn_biquads() {
        let fs = 48_000.0;
        for knobs in [
            [1.0, 0.5, 0.0, 0.5],
            [0.2, 0.9, 0.6, 0.85],
            [0.5, 0.1, 0.3, 0.0],
        ] {
            let mut eq = TrackEq::new();
            eq.update(knobs[0], knobs[1], knobs[2], knobs[3], fs);
            let drawn = TrackEq::design(
                [
                    map::eq_db(knobs[0]),
                    map::eq_db(knobs[1]),
                    map::eq_freq_hz(knobs[2]),
                    map::eq_db(knobs[3]),
                ],
                fs,
            );
            for f in [40.0, 110.0, 400.0, 1_500.0, 5_000.0, 12_000.0] {
                let n = 48_000;
                let mut peak = 0.0f32;
                for i in 0..n {
                    let y = eq.process((core::f32::consts::TAU * f * i as f32 / fs).sin());
                    if i > n / 2 {
                        peak = peak.max(y.abs());
                    }
                }
                let want: f32 = drawn.iter().map(|b| b.magnitude(f, fs)).product();
                let err = 20.0 * (peak / want).log10();
                assert!(err.abs() < 0.6, "{knobs:?} at {f} Hz: {err:.2} dB");
            }
        }
    }

    /// The reported click: an EQ wobbled around its setting every block
    /// while a loud low tone runs through it must stay smooth.
    #[test]
    fn modulated_eq_does_not_click() {
        let fs = 48_000.0;
        let mut rng = Noise::new(9);
        let run = |modulate: bool, rng: &mut Noise| {
            let mut eq = TrackEq::new();
            let mut worst = 0.0f32;
            let mut prev = [0.0f32; 2];
            for i in 0..96_000 {
                if i % 32 == 0 {
                    // Smooth, as VARIANCE and the parameter smoothing make it.
                    let t = i as f32 / fs;
                    let w = if modulate {
                        0.03 * (core::f32::consts::TAU * 0.7 * t).sin() + 0.0 * rng.sample()
                    } else {
                        0.0
                    };
                    eq.update(0.5 + w, 1.0 - w.abs(), 0.0, 0.5 - w, fs);
                }
                let x = 0.5 * (core::f32::consts::TAU * 80.0 * i as f32 / fs).sin();
                let y = eq.process(x);
                worst = worst.max((y - 2.0 * prev[1] + prev[0]).abs());
                prev = [prev[1], y];
            }
            worst
        };
        let steady = run(false, &mut rng);
        let wobbling = run(true, &mut rng);
        assert!(
            wobbling < 1.5 * steady,
            "steady {steady}, wobbling {wobbling}"
        );
    }
}
