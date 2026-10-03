//! TURBINE: a gas turbine or a big fan.
//!
//! Every blade passing a vane makes a pressure pulse, so the turbine whines at
//! the blade-pass frequency, shaft speed × BLADES. Its roar is broadband
//! noise whose bandwidth opens up with speed. Fans running near or above the
//! speed of sound at the tips also make "buzz-saw" tones at every shaft
//! order, from tiny blade-to-blade differences (BUZZSAW, a fixed irregular
//! spectrum). An unbalanced rotor makes everything flutter once per turn.
//! The drive is the shaft speed, so the ramps are a spool-up and run-down.

use super::parts::{Pow, Wander, advance, harmonics, svf_bp};
use super::{BLADE_COUNTS, Ctx, MACHINE_TURBINE, step_of};
use crate::params::map;
use crate::util::{Noise, Svf, SvfCoeffs, sin_turns};

pub fn shaft_hz(v: f32) -> f32 {
    map::expo(v, 15.0, 450.0)
}

/// Fixed shaft-order amplitudes (blade-to-blade irregularity).
const BUZZ: [f32; 16] = [
    0.9, 0.35, 0.6, 0.2, 0.75, 0.3, 0.5, 0.55, 0.15, 0.45, 0.3, 0.4, 0.12, 0.3, 0.22, 0.18,
];

#[derive(Clone, Debug)]
pub struct Turbine {
    pw: Pow,
    shaft_phase: f32,
    blade_phase: f32,
    blades: f32,
    roar: Svf,
    roar_c: SvfCoeffs,
    hump: Svf,
    hump_c: SvfCoeffs,
    wobble: Wander,
    buzz: [f32; 16],
    k: [f32; 6],
    d: f32,
}

impl Turbine {
    pub fn new() -> Self {
        let mut buzz = BUZZ;
        for (i, b) in buzz.iter_mut().enumerate() {
            *b /= ((i + 1) as f32).sqrt();
        }
        Self {
            pw: Pow::default(),
            shaft_phase: 0.0,
            blade_phase: 0.0,
            blades: 17.0,
            roar: Svf::default(),
            roar_c: SvfCoeffs::new(1000.0, 0.6, 48_000.0),
            hump: Svf::default(),
            hump_c: SvfCoeffs::new(1000.0, 1.2, 48_000.0),
            wobble: Wander::default(),
            buzz,
            k: [0.5; 6],
            d: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.roar.reset();
        self.hump.reset();
    }

    pub fn control(&mut self, c: &Ctx) {
        let fs = c.fs;
        self.k = c.k;
        self.blades = BLADE_COUNTS[step_of(MACHINE_TURBINE, 1, c.k[1])] as f32;
        let d = self.d;
        let speed = c.k[0];
        self.roar_c = SvfCoeffs::new(
            (150.0 + 12000.0 * d * d * (0.2 + 0.8 * speed)).min(0.4 * fs),
            0.6,
            fs,
        );
        let bpf = shaft_hz(speed) * d * self.blades;
        self.hump_c = SvfCoeffs::new(bpf.clamp(40.0, 0.4 * fs), 1.2, fs);
    }

    #[inline]
    pub fn tick(&mut self, c: &Ctx, d: f32, rng: &mut Noise) -> f32 {
        let fs = c.fs;
        self.d = d;
        if d <= 0.0 {
            return 0.0;
        }
        let k = self.k;
        let wob = self.wobble.tick(0.7, fs, rng);
        let shaft = shaft_hz(k[0]) * d * (1.0 + 0.003 * wob);
        let inc = shaft / fs;
        advance(&mut self.shaft_phase, inc);
        let binc = inc * self.blades;
        advance(&mut self.blade_phase, binc);
        let flutter = sin_turns(self.shaft_phase) * k[5];

        // Blade-pass whine fades out as it nears Nyquist.
        let fade = ((0.42 - binc) / 0.05).clamp(0.0, 1.0);
        let bp = self.blade_phase;
        let whine = (sin_turns(bp) + 0.3 * fade * sin_turns(2.0 * bp)) * fade;
        let buzz = harmonics(self.shaft_phase, inc, &self.buzz, 0.45);
        let n = rng.sample();
        let roar = self.roar.process(n, &self.roar_c).0
            + 0.6 * svf_bp(&mut self.hump, &self.hump_c, n, 1.2);

        let tones = (0.22 * whine * k[2] + 0.12 * buzz * k[4]) * (1.0 + 0.5 * flutter) * d * d;
        let noise = 0.45 * roar * k[3] * self.pw.get(d, 1.5) * (1.0 + 0.3 * flutter);
        tones + noise
    }

    pub fn is_quiet(&self) -> bool {
        self.d <= 0.0
    }
}

impl Default for Turbine {
    fn default() -> Self {
        Self::new()
    }
}
