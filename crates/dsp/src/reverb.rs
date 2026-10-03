//! Plate reverb after J. Dattorro, "Effect Design Part 1: Reverberator and
//! Other Filters", JAES 45(9), 1997.
//!
//! Mono in, stereo out: a predelay, an input bandwidth filter and four input
//! diffusers feed a figure-of-eight "tank" of two cross-coupled halves, each
//! with a modulated allpass, a delay, a damping lowpass, a second allpass and
//! a second delay. The stereo outputs are sums of taps spread over the tank.
//!
//! All delay lengths come from the paper (specified at 29.761 kHz) and are
//! rescaled to the running sample rate.

use crate::params::map;

const REF_SR: f32 = 29_761.0;

// Delay lengths at REF_SR.
const INPUT_DIFFUSERS: [(usize, f32); 4] = [(142, 0.75), (107, 0.75), (379, 0.625), (277, 0.625)];
const L_MOD_AP: usize = 672;
const L_DELAY_1: usize = 4453;
const L_AP: usize = 1800;
const L_DELAY_2: usize = 3720;
const R_MOD_AP: usize = 908;
const R_DELAY_1: usize = 4217;
const R_AP: usize = 2656;
const R_DELAY_2: usize = 3163;
/// Peak modulation excursion of the tank's first allpasses.
const EXCURSION: f32 = 16.0;
const MOD_HZ: f32 = 0.9;
const DECAY_DIFFUSION_1: f32 = 0.7;

/// Time for one full trip around the tank, in seconds. The decay gain is
/// applied four times per trip, which is what [`Plate::decay_gain`] uses to
/// turn an RT60 into a gain.
const LOOP_SECONDS: f32 =
    (L_MOD_AP + L_DELAY_1 + L_AP + L_DELAY_2 + R_MOD_AP + R_DELAY_1 + R_AP + R_DELAY_2) as f32
        / REF_SR;

/// Output taps as (source, offset at REF_SR, sign).
#[derive(Clone, Copy, Debug)]
enum Src {
    LDelay1,
    LAp,
    LDelay2,
    RDelay1,
    RAp,
    RDelay2,
}

const LEFT_TAPS: [(Src, usize, f32); 7] = [
    (Src::RDelay1, 266, 1.0),
    (Src::RDelay1, 2974, 1.0),
    (Src::RAp, 1913, -1.0),
    (Src::RDelay2, 1996, 1.0),
    (Src::LDelay1, 1990, -1.0),
    (Src::LAp, 187, -1.0),
    (Src::LDelay2, 1066, -1.0),
];

const RIGHT_TAPS: [(Src, usize, f32); 7] = [
    (Src::LDelay1, 353, 1.0),
    (Src::LDelay1, 3627, 1.0),
    (Src::LAp, 1228, -1.0),
    (Src::LDelay2, 2673, 1.0),
    (Src::RDelay1, 2111, -1.0),
    (Src::RAp, 335, -1.0),
    (Src::RDelay2, 121, -1.0),
];

/// Longest pre-delay the [`Plate`] allocates for, in seconds.
pub const MAX_PREDELAY_S: f32 = 0.2;

#[inline]
fn flush(x: f32) -> f32 {
    // Keep recirculating state out of the (slow on x86) denormal range.
    if x.abs() < 1e-18 { 0.0 } else { x }
}

/// Power-of-two ring buffer.
#[derive(Clone, Debug)]
struct Delay {
    buf: Vec<f32>,
    mask: usize,
    pos: usize,
    len: usize,
}

impl Delay {
    /// `len` is the nominal delay; `headroom` extra samples may be read.
    fn new(len: usize, headroom: usize) -> Self {
        let size = (len + headroom + 2).next_power_of_two();
        Self {
            buf: vec![0.0; size],
            mask: size - 1,
            pos: 0,
            len: len.max(1),
        }
    }

    #[inline]
    fn write(&mut self, x: f32) {
        self.pos = (self.pos + 1) & self.mask;
        self.buf[self.pos] = x;
    }

    /// The sample written `d` writes ago (`0` = most recent).
    #[inline]
    fn tap(&self, d: usize) -> f32 {
        self.buf[self.pos.wrapping_sub(d) & self.mask]
    }

    #[inline]
    fn tap_frac(&self, d: f32) -> f32 {
        let d = d.max(0.0);
        let i = d as usize;
        let f = d - i as f32;
        let a = self.tap(i);
        a + (self.tap(i + 1) - a) * f
    }

    /// Output of the delay line, i.e. the sample that is `len` samples old
    /// once the next input is written.
    #[inline]
    fn output(&self) -> f32 {
        self.tap(self.len - 1)
    }
}

/// Schroeder allpass with an optional modulated (fractional) length.
#[derive(Clone, Debug)]
struct Allpass {
    d: Delay,
}

impl Allpass {
    fn new(len: usize, headroom: usize) -> Self {
        Self {
            d: Delay::new(len, headroom),
        }
    }

    #[inline]
    fn process(&mut self, x: f32, g: f32) -> f32 {
        let delayed = self.d.output();
        let v = x - g * delayed;
        self.d.write(flush(v));
        delayed + g * v
    }

    #[inline]
    fn process_mod(&mut self, x: f32, g: f32, offset: f32) -> f32 {
        let delayed = self.d.tap_frac(self.d.len as f32 - 1.0 + offset);
        let v = x - g * delayed;
        self.d.write(flush(v));
        delayed + g * v
    }
}

/// Per-sample control values, derived from the channel's knobs.
#[derive(Clone, Copy, Debug)]
pub struct PlateControls {
    /// Tank loop gain (from DECAY).
    pub decay: f32,
    /// One-pole lowpass coefficient for the input bandwidth and tank damping (from TONE).
    pub tone: f32,
    /// Pre-delay in samples.
    pub predelay: f32,
}

#[derive(Clone, Debug)]
pub struct Plate {
    sample_rate: f32,
    predelay: Delay,
    bandwidth_state: f32,
    input: [Allpass; 4],
    input_g: [f32; 4],
    l_mod_ap: Allpass,
    l_delay_1: Delay,
    l_damp: f32,
    l_ap: Allpass,
    l_delay_2: Delay,
    r_mod_ap: Allpass,
    r_delay_1: Delay,
    r_damp: f32,
    r_ap: Allpass,
    r_delay_2: Delay,
    excursion: f32,
    lfo_phase: f32,
    lfo_inc: f32,
    left_taps: [(Src, usize, f32); 7],
    right_taps: [(Src, usize, f32); 7],
}

impl Plate {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / REF_SR;
        let n = |len: usize| ((len as f32 * scale).round() as usize).max(1);
        let excursion = EXCURSION * scale;
        let exc_room = excursion.ceil() as usize + 2;
        let scale_taps =
            |taps: [(Src, usize, f32); 7]| taps.map(|(s, off, sign)| (s, n(off), sign));
        Self {
            sample_rate,
            predelay: Delay::new((MAX_PREDELAY_S * sample_rate) as usize + 2, 2),
            bandwidth_state: 0.0,
            input: INPUT_DIFFUSERS.map(|(len, _)| Allpass::new(n(len), 0)),
            input_g: INPUT_DIFFUSERS.map(|(_, g)| g),
            l_mod_ap: Allpass::new(n(L_MOD_AP), exc_room),
            l_delay_1: Delay::new(n(L_DELAY_1), 0),
            l_damp: 0.0,
            l_ap: Allpass::new(n(L_AP), 0),
            l_delay_2: Delay::new(n(L_DELAY_2), 0),
            r_mod_ap: Allpass::new(n(R_MOD_AP), exc_room),
            r_delay_1: Delay::new(n(R_DELAY_1), 0),
            r_damp: 0.0,
            r_ap: Allpass::new(n(R_AP), 0),
            r_delay_2: Delay::new(n(R_DELAY_2), 0),
            excursion,
            lfo_phase: 0.0,
            lfo_inc: MOD_HZ / sample_rate,
            left_taps: scale_taps(LEFT_TAPS),
            right_taps: scale_taps(RIGHT_TAPS),
        }
    }

    /// Tank gain that gives roughly `rt60` seconds of decay.
    pub fn decay_gain(rt60: f32) -> f32 {
        (-6.907_755 * LOOP_SECONDS / (4.0 * rt60.max(0.05)))
            .exp()
            .min(0.98)
    }

    /// Map normalized knob positions to per-sample controls.
    pub fn controls(&self, decay: f32, tone: f32, predelay: f32) -> PlateControls {
        let cutoff = map::reverb_tone_hz(tone).min(0.45 * self.sample_rate);
        PlateControls {
            decay: Self::decay_gain(map::reverb_seconds(decay)),
            tone: 1.0 - (-core::f32::consts::TAU * cutoff / self.sample_rate).exp(),
            predelay: (map::reverb_predelay_seconds(predelay) * self.sample_rate)
                .min(MAX_PREDELAY_S * self.sample_rate),
        }
    }

    #[inline]
    fn read(&self, src: Src, offset: usize) -> f32 {
        match src {
            Src::LDelay1 => self.l_delay_1.tap(offset),
            Src::LAp => self.l_ap.d.tap(offset),
            Src::LDelay2 => self.l_delay_2.tap(offset),
            Src::RDelay1 => self.r_delay_1.tap(offset),
            Src::RAp => self.r_ap.d.tap(offset),
            Src::RDelay2 => self.r_delay_2.tap(offset),
        }
    }

    /// Process one mono sample; returns the wet stereo pair.
    #[inline]
    pub fn process(&mut self, x: f32, c: &PlateControls) -> (f32, f32) {
        self.predelay.write(x);
        let x = self.predelay.tap_frac(c.predelay);
        self.bandwidth_state = flush(self.bandwidth_state + c.tone * (x - self.bandwidth_state));
        let mut s = self.bandwidth_state;
        for (ap, &g) in self.input.iter_mut().zip(&self.input_g) {
            s = ap.process(s, g);
        }

        self.lfo_phase += self.lfo_inc;
        if self.lfo_phase >= 1.0 {
            self.lfo_phase -= 1.0;
        }
        let angle = self.lfo_phase * core::f32::consts::TAU;
        let (mod_l, mod_r) = (self.excursion * angle.sin(), self.excursion * angle.cos());
        let decay_diffusion_2 = (c.decay + 0.15).clamp(0.25, 0.5);

        // Each half is fed by the other half's final delay.
        let from_right = self.r_delay_2.output();
        let from_left = self.l_delay_2.output();

        let a = self
            .l_mod_ap
            .process_mod(s + c.decay * from_right, -DECAY_DIFFUSION_1, mod_l);
        let d = self.l_delay_1.output();
        self.l_delay_1.write(a);
        self.l_damp = flush(self.l_damp + c.tone * (d - self.l_damp));
        let b = self.l_ap.process(self.l_damp * c.decay, decay_diffusion_2);
        self.l_delay_2.write(b);

        let a = self
            .r_mod_ap
            .process_mod(s + c.decay * from_left, -DECAY_DIFFUSION_1, mod_r);
        let d = self.r_delay_1.output();
        self.r_delay_1.write(a);
        self.r_damp = flush(self.r_damp + c.tone * (d - self.r_damp));
        let b = self.r_ap.process(self.r_damp * c.decay, decay_diffusion_2);
        self.r_delay_2.write(b);

        let sum = |taps: &[(Src, usize, f32); 7]| {
            taps.iter()
                .map(|&(src, off, sign)| sign * self.read(src, off))
                .sum::<f32>()
                * 0.6
        };
        (sum(&self.left_taps), sum(&self.right_taps))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse_response(sr: f32, decay: f32, tone: f32, seconds: f32) -> (Vec<f32>, Vec<f32>) {
        let mut p = Plate::new(sr);
        let c = p.controls(decay, tone, 0.0);
        let n = (sr * seconds) as usize;
        let mut l = Vec::with_capacity(n);
        let mut r = Vec::with_capacity(n);
        for i in 0..n {
            let (a, b) = p.process(if i == 0 { 1.0 } else { 0.0 }, &c);
            l.push(a);
            r.push(b);
        }
        (l, r)
    }

    fn energy(x: &[f32]) -> f32 {
        x.iter().map(|v| v * v).sum()
    }

    #[test]
    fn finite_and_decaying_at_many_rates() {
        for sr in [22_050.0, 44_100.0, 48_000.0, 96_000.0, 192_000.0] {
            for decay in [0.0, 0.5, 1.0] {
                let (l, r) = impulse_response(sr, decay, 1.0, 1.0);
                assert!(
                    l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 4.0),
                    "sr {sr}"
                );
                let (l, _) = impulse_response(sr, decay, 1.0, 30.0);
                let s = sr as usize;
                assert!(
                    energy(&l[l.len() - s..]) < 1e-6 * energy(&l[..s]).max(1e-9),
                    "sr {sr} decay {decay}"
                );
            }
        }
    }

    #[test]
    fn stereo_outputs_are_decorrelated() {
        let (l, r) = impulse_response(48_000.0, 0.5, 0.7, 1.0);
        let dot: f32 = l.iter().zip(&r).map(|(a, b)| a * b).sum();
        let corr = dot / (energy(&l) * energy(&r)).sqrt();
        assert!(corr.abs() < 0.5, "correlation {corr}");
    }

    #[test]
    fn decay_knob_tracks_rt60() {
        // Measure the time the smoothed energy envelope takes to fall 30 dB
        // after the build-up and extrapolate to 60 dB.
        let sr = 48_000.0;
        for knob in [0.3f32, 0.6, 0.9] {
            let expected = map::reverb_seconds(knob);
            let (l, r) = impulse_response(sr, knob, 1.0, expected * 2.0 + 1.0);
            let win = (sr * 0.02) as usize;
            let env: Vec<f32> = l
                .chunks(win)
                .zip(r.chunks(win))
                .map(|(a, b)| 10.0 * ((energy(a) + energy(b)) / win as f32 + 1e-30).log10())
                .collect();
            let start = 10; // 200 ms: past the build-up
            let target = env[start] - 30.0;
            let idx = env[start..]
                .iter()
                .position(|&e| e < target)
                .expect("never fell 30 dB");
            let rt60 = 2.0 * idx as f32 * win as f32 / sr;
            let ratio = rt60 / expected;
            assert!(
                (0.5..2.0).contains(&ratio),
                "knob {knob}: measured {rt60:.2}s vs {expected:.2}s"
            );
        }
    }
}
