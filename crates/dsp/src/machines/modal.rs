//! A bank of 16 struck modes (the Xenokussion XK-1's MODAL engine), used by
//! PRESS for the ringing die and workpiece.
//!
//! Each mode is a complex one-pole resonator, `z ← z·r·e^{iω} + g·x`, whose
//! imaginary part rings as a decaying sine. Unlike a direct-form biquad it
//! stays well-behaved when ω is swept, and a unit impulse gives a
//! unit-amplitude ring at any frequency.
//!
//! STRUCT morphs the mode frequency ratios (interpolating in log frequency)
//! through six measured or textbook materials, so in-between positions are
//! "impossible" objects. POS weights the modes like a strike position
//! (`|sin(π·k·p)|`), and DAMP makes higher modes die faster
//! (`T60_k = DECAY · ratio_k^(−1.3·DAMP)`), from metal (all ring alike) to wood.

use crate::util::{cos_turns, flush, sin_turns, t60_gain};

/// STRUCT: (lower material index, fraction towards the next one).
pub fn material_segment(structure: f32) -> (usize, f32) {
    let s = structure.clamp(0.0, 1.0) * 5.0;
    let i = (s as usize).min(4);
    (i, s - i as f32)
}

/// POS: position along the body, near the edge to the centre.
pub fn strike_position(pos: f32) -> f32 {
    0.02 + 0.48 * pos
}

/// Control values for the bank.
#[derive(Clone, Copy, Debug)]
pub struct ModalCtl {
    pub fs: f32,
    pub hz: f32,
    /// Material morph (STRUCT).
    pub x: f32,
    /// Strike position.
    pub y: f32,
    /// High-mode damping.
    pub z: f32,
    pub decay_s: f32,
}

pub const MODES: usize = 16;

/// Mode frequency ratios for each material in STRUCT order.
pub const MATERIALS: [[f32; MODES]; 6] = [
    // Circular membrane (Bessel function zeros).
    [
        1.0, 1.594, 2.136, 2.296, 2.653, 2.918, 3.156, 3.501, 3.600, 3.652, 4.060, 4.154, 4.227,
        4.601, 4.832, 4.903,
    ],
    // Ideal string.
    [
        1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0,
    ],
    // Marimba bar: undercut so the first three modes sit near 1:4:10.
    [
        1.0, 3.93, 9.87, 17.9, 26.5, 36.6, 48.4, 61.9, 77.0, 93.7, 112.0, 132.0, 153.5, 176.7,
        201.4, 227.7,
    ],
    // Uniform free-free bar: (β_k / β_1)².
    [
        1.0, 2.756, 5.404, 8.933, 13.345, 18.637, 24.81, 31.87, 39.81, 48.63, 58.34, 68.93, 80.39,
        92.74, 105.96, 120.12,
    ],
    // Simply supported square plate: (m² + n²) / 2.
    [
        1.0, 2.5, 4.0, 5.0, 6.5, 8.5, 9.0, 10.0, 12.5, 13.0, 14.5, 16.0, 17.0, 18.5, 20.0, 20.5,
    ],
    // Church bell partials relative to the hum note.
    [
        1.0, 2.0, 2.366, 3.012, 4.0, 5.028, 5.324, 6.022, 8.332, 10.866, 13.592, 16.43, 19.434,
        22.6, 25.86, 29.2,
    ],
];

/// Mode ratios for a STRUCT knob position.
pub fn ratios(structure: f32) -> [f32; MODES] {
    let (i, f) = material_segment(structure);
    let (a, b) = (&MATERIALS[i], &MATERIALS[i + 1]);
    core::array::from_fn(|k| (a[k].ln() * (1.0 - f) + b[k].ln() * f).exp())
}

/// Relative excitation of each mode for a POS knob position (sums to 1).
pub fn position_gains(pos: f32) -> [f32; MODES] {
    let p = strike_position(pos);
    let mut g: [f32; MODES] = core::array::from_fn(|k| {
        let n = (k + 1) as f32;
        sin_turns(0.5 * n * p).abs() / n.sqrt()
    });
    let sum: f32 = g.iter().sum();
    g.iter_mut().for_each(|x| *x /= sum.max(1e-6));
    g
}

#[derive(Clone, Debug)]
pub struct Modal {
    re: [f32; MODES],
    im: [f32; MODES],
    cr: [f32; MODES],
    ci: [f32; MODES],
    gain: [f32; MODES],
}

/// Level trim so MODAL sits with the other engines.
const OUTPUT_GAIN: f32 = 2.4;

impl Modal {
    pub fn new() -> Self {
        Self {
            re: [0.0; MODES],
            im: [0.0; MODES],
            cr: [0.0; MODES],
            ci: [0.0; MODES],
            gain: [0.0; MODES],
        }
    }

    pub fn reset(&mut self) {
        self.re = [0.0; MODES];
        self.im = [0.0; MODES];
    }

    pub fn control(&mut self, c: &ModalCtl) {
        let ratios = ratios(c.x);
        let pos = position_gains(c.y);
        let damp = 1.3 * c.z;
        let limit = 0.45 * c.fs;
        for k in 0..MODES {
            let f = c.hz * ratios[k];
            let t60 = c.decay_s * ratios[k].powf(-damp);
            let r = t60_gain(t60, c.fs);
            // A mode swept above the limit keeps ringing, parked there and
            // muted, so a hit that starts high (SWEEP) chirps down into range
            // instead of losing the strike.
            let w = f.min(limit) / c.fs;
            self.cr[k] = r * cos_turns(w);
            self.ci[k] = r * sin_turns(w);
            self.gain[k] = if f < limit { pos[k] * OUTPUT_GAIN } else { 0.0 };
            self.re[k] = flush(self.re[k]);
            self.im[k] = flush(self.im[k]);
        }
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let mut out = 0.0;
        for k in 0..MODES {
            let (re, im) = (self.re[k], self.im[k]);
            self.re[k] = re * self.cr[k] - im * self.ci[k] + x;
            self.im[k] = re * self.ci[k] + im * self.cr[k];
            out += self.im[k] * self.gain[k];
        }
        out
    }

    pub fn energy(&self) -> f32 {
        self.re
            .iter()
            .zip(&self.im)
            .map(|(a, b)| a * a + b * b)
            .sum()
    }
}

impl Default for Modal {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_tables_are_ascending_from_one() {
        for m in MATERIALS {
            assert_eq!(m[0], 1.0);
            assert!(m.windows(2).all(|w| w[1] > w[0]));
        }
    }

    #[test]
    fn struct_endpoints_hit_the_tables() {
        assert_eq!(ratios(0.0)[3], MATERIALS[0][3]);
        assert!((ratios(1.0)[3] - MATERIALS[5][3]).abs() < 1e-4);
    }

    #[test]
    fn a_mode_struck_above_the_limit_rings_once_it_comes_down() {
        let fs = 48_000.0;
        let mut m = Modal::new();
        let mut c = ModalCtl {
            fs,
            hz: 200.0,
            x: 0.5,
            y: 0.5,
            z: 0.5,
            decay_s: 0.5,
        };
        c.decay_s = 2.0;
        c.hz = 30_000.0;
        m.control(&c);
        for n in 0..16 {
            assert_eq!(m.tick(if n == 0 { 1.0 } else { 0.0 }), 0.0);
        }
        c.hz = 500.0;
        m.control(&c);
        let peak = (0..4_800)
            .map(|_| m.tick(0.0))
            .fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(peak > 0.1, "{peak}");
    }

    #[test]
    fn a_single_mode_rings_at_its_frequency_with_unit_amplitude() {
        let fs = 48_000.0;
        let mut m = Modal::new();
        let mut c = ModalCtl {
            fs,
            hz: 200.0,
            x: 0.5,
            y: 0.5,
            z: 0.5,
            decay_s: 0.5,
        };
        c.hz = 480.0;
        c.x = 0.2; // pure string: harmonic ratios
        c.y = 1.0; // centre strike
        c.decay_s = 10.0;
        m.control(&c);
        // Only feed mode 0 to check the resonator itself.
        m.gain = [0.0; MODES];
        m.gain[0] = 1.0;
        let mut out = Vec::new();
        for n in 0..4_800 {
            out.push(m.tick(if n == 0 { 1.0 } else { 0.0 }));
        }
        let peak = out.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!((peak - 1.0).abs() < 0.02, "peak {peak}");
        let crossings = out.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        assert!((crossings as i32 - 48).abs() <= 1, "{crossings}");
    }
}
