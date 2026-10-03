//! WELDER: an arc welder.
//!
//! The arc buzzes at twice the mains frequency (the current reverses every
//! half cycle), unsteady as the arc length wanders. Over it the wire
//! short-circuits into the pool at an irregular 40–220 times a second (the
//! "frying bacon" of MIG welding): each short is a burst of band-passed
//! noise and a resonant pop, centred lower or higher with HEAT, with a fine
//! spray of tiny crackles in between. A high sizzle sits on top,
//! and every so often spatter pops and a spark tinks off the bench. PULSE
//! switches to pulsed welding, chopping it all at its rate. When the track
//! starts, the arc strikes with a burst of crackle.

use super::Ctx;
use super::parts::{Decay, Mode, Wander, advance, svf_bp};
use crate::params::map;
use crate::util::{Noise, Svf, SvfCoeffs, fast_tanh, sin_turns};

const MAINS_HZ: f32 = 50.0;

pub fn heat_hz(v: f32) -> f32 {
    map::expo(v, 450.0, 4000.0)
}

/// PULSE rate, or `None` when off.
pub fn pulse_hz(v: f32) -> Option<f32> {
    (v > 0.02).then(|| map::expo((v - 0.02) / 0.98, 0.7, 18.0))
}

#[derive(Clone, Debug)]
pub struct Welder {
    mains: f32,
    arc: Wander,
    crack: Decay,
    short: Decay,
    short_left: u32,
    pop: Mode,
    fine_bp: Svf,
    fine_c: SvfCoeffs,
    crack_bp: Svf,
    crack_c: SvfCoeffs,
    sizzle: Svf,
    sizzle_c: SvfCoeffs,
    spatter: Decay,
    spatter_bp: Svf,
    spatter_c: SvfCoeffs,
    tink: Mode,
    strike: Decay,
    pulse_phase: f32,
    k: [f32; 6],
    d: f32,
}

impl Welder {
    pub fn new() -> Self {
        let fs = 48_000.0;
        Self {
            mains: 0.0,
            arc: Wander::default(),
            crack: Decay::default(),
            short: Decay::default(),
            short_left: 0,
            pop: Mode::default(),
            fine_bp: Svf::default(),
            fine_c: SvfCoeffs::new(5000.0, 0.7, fs),
            crack_bp: Svf::default(),
            crack_c: SvfCoeffs::new(3000.0, 0.7, fs),
            sizzle: Svf::default(),
            sizzle_c: SvfCoeffs::new(5000.0, 0.7, fs),
            spatter: Decay::default(),
            spatter_bp: Svf::default(),
            spatter_c: SvfCoeffs::new(2200.0, 1.0, fs),
            tink: Mode::default(),
            strike: Decay::default(),
            pulse_phase: 0.0,
            k: [0.5; 6],
            d: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.crack.reset();
        self.short.reset();
        self.pop.reset();
        self.fine_bp.reset();
        self.crack_bp.reset();
        self.sizzle.reset();
        self.spatter.reset();
        self.spatter_bp.reset();
        self.tink.reset();
        self.strike.reset();
    }

    pub fn control(&mut self, c: &Ctx) {
        let fs = c.fs;
        self.k = c.k;
        let heat = heat_hz(c.k[3]);
        self.crack_c = SvfCoeffs::new(heat, 0.9, fs);
        self.fine_c = SvfCoeffs::new((heat * 3.0).min(0.4 * fs), 0.7, fs);
        self.sizzle_c = SvfCoeffs::new((heat * 2.5 + 2000.0).min(0.4 * fs), 0.7, fs);
        self.spatter_c = SvfCoeffs::new(2200.0, 1.0, fs);
    }

    pub fn start(&mut self, c: &Ctx) {
        self.strike.trigger(1.0, 0.15, c.fs);
    }

    #[inline]
    pub fn tick(&mut self, c: &Ctx, d: f32, rng: &mut Noise) -> f32 {
        let fs = c.fs;
        self.d = d;
        let k = self.k;
        if d <= 0.0 {
            // Let the last pops ring out.
            let y = 0.2 * self.tink.tick(0.0) + self.spatter.tick() * rng.sample() * 0.2;
            return y;
        }
        let strike = self.strike.tick();
        let arc = 1.0 + 0.4 * self.arc.tick(9.0, fs, rng);

        advance(&mut self.mains, MAINS_HZ / fs);
        let s = fast_tanh(4.0 * sin_turns(self.mains));
        let buzz = (s * s - 0.75) * arc;

        // Short circuits at irregular intervals.
        if self.short_left == 0 {
            let rate = (40.0 + 180.0 * k[1]) * (1.0 + 3.0 * strike);
            self.short_left = ((fs / rate) * (0.35 + 1.3 * rng.uniform())) as u32 + 1;
            let u = rng.uniform();
            let a = 0.35 + 0.65 * u * u + strike;
            self.short.trigger(a, 0.004 + 0.008 * rng.uniform(), fs);
            self.pop
                .set(heat_hz(k[3]) * (0.6 + 0.8 * rng.uniform()), 0.006, fs);
            self.pop.tick(0.6 * a);
        }
        self.short_left -= 1;
        // A fine spray of tiny crackles between them.
        if rng.chance((300.0 + 1500.0 * k[1] * k[1]) / fs) {
            let u = rng.uniform();
            self.crack.trigger_max(0.1 + 0.6 * u * u * u, 0.0008, fs);
        }
        let crackle = svf_bp(&mut self.crack_bp, &self.crack_c, rng.sample(), 0.9)
            * self.short.tick()
            + 0.5 * self.pop.tick(0.0)
            + 0.4 * svf_bp(&mut self.fine_bp, &self.fine_c, rng.sample(), 0.7) * self.crack.tick();

        let hiss = svf_bp(&mut self.sizzle, &self.sizzle_c, rng.sample(), 0.7) * arc * arc;

        let spatter_rate = 0.4 + 14.0 * k[4] * k[4];
        if k[4] > 0.0 && rng.chance(spatter_rate / fs) {
            self.spatter
                .trigger_max(0.5 + 0.5 * rng.uniform(), 0.06, fs);
            let hz = 4200.0 + 4000.0 * rng.uniform();
            self.tink.set(hz, 0.06 + 0.05 * rng.uniform(), fs);
            self.tink.tick(k[4] * rng.uniform());
        }
        let spatter =
            svf_bp(&mut self.spatter_bp, &self.spatter_c, rng.sample(), 1.0) * self.spatter.tick();
        let tink = self.tink.tick(0.0);

        let mut gate = 1.0;
        if let Some(hz) = pulse_hz(k[5]) {
            advance(&mut self.pulse_phase, hz / fs);
            let p = 0.5 + 0.5 * fast_tanh(5.0 * sin_turns(self.pulse_phase));
            gate = 0.2 + 0.8 * p;
        }

        let out = 0.15 * buzz * k[0] + 0.8 * crackle + 0.18 * hiss * k[2] + 0.4 * spatter * k[4];
        out * gate * d + 0.25 * tink
    }

    pub fn is_quiet(&self) -> bool {
        self.d <= 0.0 && self.tink.energy() < 1e-10 && self.spatter.value() < 1e-6
    }
}

impl Default for Welder {
    fn default() -> Self {
        Self::new()
    }
}
