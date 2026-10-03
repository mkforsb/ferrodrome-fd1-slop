//! CONVEYOR: a belt driven by an electric motor over a row of steel rollers.
//!
//! * The motor hums on a harmonic series whose fundamental follows the belt
//!   speed (and so glides up and down with the ramps). HUM sets both its
//!   level and how far up the series it reaches.
//! * Each roller the belt joint passes clicks: a tiny struck steel tube
//!   (four inharmonic modes and a low thunk). ROLLERS morphs between a
//!   scatter of random clicks and a rigid clatter locked to the roller-pass
//!   rate; RATTLE sets how much.
//! * The belt's friction whirrs: band-passed noise that rises with speed and
//!   pulses at the roller-pass rate.
//! * Now and then the belt squeals: a wandering high tone that comes and goes.

use super::Ctx;
use super::parts::{Mode, Pow, Wander, advance, harmonics, svf_bp};
use crate::params::map;
use crate::util::{Noise, Svf, SvfCoeffs, sin_turns};

const HUM_HARMONICS: usize = 12;
/// Hum fundamental per roller click.
const ROLLER_DIVIDER: f32 = 4.0;
/// Click mode ratios of a short steel roller.
const CLICK_RATIOS: [f32; 4] = [1.0, 2.32, 3.41, 4.93];

pub fn hum_hz(v: f32) -> f32 {
    map::expo(v, 30.0, 240.0)
}

#[derive(Clone, Debug)]
pub struct Conveyor {
    pw: Pow,
    hum_phase: f32,
    hum_amps: [f32; HUM_HARMONICS],
    roller_phase: f32,
    clicks: [Mode; 4],
    thunk: Mode,
    whirr: Svf,
    whirr_c: SvfCoeffs,
    rumble: Svf,
    rumble_c: SvfCoeffs,
    squeal_phase: f32,
    squeal_pitch: Wander,
    squeal_gate: Wander,
    squeal_rough: f32,
    hz: f32,
    level: [f32; 5],
    regular: f32,
    d: f32,
}

impl Conveyor {
    pub fn new() -> Self {
        Self {
            pw: Pow::default(),
            hum_phase: 0.0,
            hum_amps: [0.0; HUM_HARMONICS],
            roller_phase: 0.0,
            clicks: Default::default(),
            thunk: Mode::default(),
            whirr: Svf::default(),
            whirr_c: SvfCoeffs::new(500.0, 0.7, 48_000.0),
            rumble: Svf::default(),
            rumble_c: SvfCoeffs::new(320.0, 0.6, 48_000.0),
            squeal_phase: 0.0,
            squeal_pitch: Wander::default(),
            squeal_gate: Wander::default(),
            squeal_rough: 0.0,
            hz: 100.0,
            level: [0.0; 5],
            regular: 0.5,
            d: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.clicks.iter_mut().for_each(Mode::reset);
        self.thunk.reset();
        self.whirr.reset();
        self.rumble.reset();
    }

    pub fn control(&mut self, c: &Ctx) {
        let k = c.k;
        self.hz = hum_hz(k[0]);
        // Brighter hum reaches further up the series.
        let slope = 2.4 - 1.7 * k[1];
        for (i, a) in self.hum_amps.iter_mut().enumerate() {
            *a = ((i + 1) as f32).powf(-slope);
        }
        // Odd harmonics a little weaker: a slightly asymmetric motor.
        for i in (2..HUM_HARMONICS).step_by(2) {
            self.hum_amps[i] *= 0.7;
        }
        self.level = [k[1], k[2], k[4], k[5], 0.0];
        self.regular = k[3];
        let fc = (400.0 + 14.0 * self.hz * self.d.max(0.05)).min(0.4 * c.fs);
        self.whirr_c = SvfCoeffs::new(fc, 0.7, c.fs);
        self.rumble_c = SvfCoeffs::new(320.0, 0.6, c.fs);
    }

    fn click(&mut self, amp: f32, rng: &mut Noise, fs: f32) {
        let base = 1100.0 + 600.0 * rng.uniform();
        for (m, r) in self.clicks.iter_mut().zip(CLICK_RATIOS) {
            m.set(
                base * r * (1.0 + 0.03 * rng.sample()),
                0.025 + 0.03 * rng.uniform(),
                fs,
            );
            m.tick(amp * (0.6 + 0.4 * rng.uniform()));
        }
        self.thunk.set(240.0 + 60.0 * rng.uniform(), 0.04, fs);
        self.thunk.tick(0.8 * amp);
    }

    #[inline]
    pub fn tick(&mut self, c: &Ctx, d: f32, rng: &mut Noise) -> f32 {
        let fs = c.fs;
        self.d = d;
        let f = self.hz * d;
        let inc = f / fs;
        advance(&mut self.hum_phase, inc);
        let hum = harmonics(self.hum_phase, inc, &self.hum_amps, 0.45) * self.level[0];

        // Roller clicks: on the roller clock (rigid) or scattered at random.
        let roller_rate = f / ROLLER_DIVIDER;
        let rattle = self.level[1];
        if rattle > 0.0 && d > 0.01 {
            if advance(&mut self.roller_phase, roller_rate / fs)
                && rng.chance(0.15 + 0.85 * self.regular)
            {
                let a = rattle * (0.5 + 0.5 * rng.uniform());
                self.click(a, rng, fs);
            }
            if rng.chance(roller_rate * 1.5 * (1.0 - self.regular) / fs) {
                let a = rattle * (0.3 + 0.7 * rng.uniform() * rng.uniform());
                self.click(a, rng, fs);
            }
        } else {
            advance(&mut self.roller_phase, roller_rate / fs);
        }
        let mut clatter = 0.0;
        for m in self.clicks.iter_mut() {
            clatter += m.tick(0.0);
        }
        clatter = 0.25 * clatter + 0.2 * self.thunk.tick(0.0);

        let pulse = 1.0 + 0.5 * sin_turns(self.roller_phase);
        let n = rng.sample();
        let whirr = (svf_bp(&mut self.whirr, &self.whirr_c, n, 0.7)
            + 0.8 * self.rumble.process(n, &self.rumble_c).0)
            * pulse
            * self.level[2]
            * d;

        let mut squeal = 0.0;
        if self.level[3] > 0.0 {
            let pitch = self.squeal_pitch.tick(0.3, fs, rng);
            let gate = self.squeal_gate.tick(0.2, fs, rng);
            let open = ((gate - 0.25) * 2.5).clamp(0.0, 1.0);
            if open > 0.0 {
                self.squeal_rough += (rng.sample() - self.squeal_rough) * 0.002;
                let hz = (720.0 + 160.0 * pitch) * (0.85 + 0.15 * d);
                advance(&mut self.squeal_phase, hz / fs);
                let p = self.squeal_phase;
                squeal = (sin_turns(p)
                    + 0.45 * sin_turns(2.0 * p)
                    + 0.3 * sin_turns(3.0 * p)
                    + 0.2 * sin_turns(4.0 * p))
                    * open
                    * (1.0 + 4.0 * self.squeal_rough)
                    * self.level[3]
                    * d
                    * d;
            }
        }

        let power = self.pw.get(d, 0.7);
        0.3 * hum * power + 0.6 * clatter + 1.6 * whirr + 0.12 * squeal
    }

    pub fn is_quiet(&self) -> bool {
        self.d <= 0.0 && self.clicks.iter().all(|m| m.energy() < 1e-10)
    }
}

impl Default for Conveyor {
    fn default() -> Self {
        Self::new()
    }
}
