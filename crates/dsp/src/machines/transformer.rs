//! TRANSFORMER: mains hum from magnetostriction.
//!
//! The core's flux follows the mains, `B = sin(2π·f·t)`, and the core
//! lengthens with the square of it, so the hum's fundamental is twice the
//! mains frequency (100 Hz on 50 Hz mains). A saturating core
//! (`tanh(s·B)²`) adds the higher even harmonics real transformers have,
//! more of them the harder it is driven; when the track energizes, inrush
//! drives it hard for a moment.
//!
//! * BUZZ: loose laminations slap together near the flux peaks, twice a
//!   cycle: a gated, band-passed rattle with a struck plate ring.
//! * ARC: corona crackle, most likely at the voltage peaks.
//! * BEAT: a second transformer on a slightly different supply beating
//!   against the first, and slow load fluctuation.
//! * TANK: the oil tank's resonances colour the hum.

use super::Ctx;
use super::parts::{Decay, Lp1, Mode, Wander, advance, svf_bp};
use crate::util::{DcBlocker, Noise, Svf, SvfCoeffs, fast_tanh, sin_turns};

pub fn mains_hz(v: f32) -> f32 {
    40.0 + 30.0 * v
}

pub fn beat_hz(v: f32) -> f32 {
    0.04 + 0.9 * v * v
}

const TANK_HZ: [f32; 3] = [205.0, 470.0, 830.0];

#[derive(Clone, Debug)]
pub struct Transformer {
    phase: f32,
    phase2: f32,
    inrush: Decay,
    buzz_bp: Svf,
    buzz_c: SvfCoeffs,
    slap: Mode,
    in_contact: bool,
    arc_env: Decay,
    arc_hp: Lp1,
    tank: [Svf; 3],
    tank_c: [SvfCoeffs; 3],
    load: Wander,
    dc: DcBlocker,
    k: [f32; 6],
    d: f32,
}

impl Transformer {
    pub fn new() -> Self {
        let fs = 48_000.0;
        Self {
            phase: 0.0,
            phase2: 0.37,
            inrush: Decay::default(),
            buzz_bp: Svf::default(),
            buzz_c: SvfCoeffs::new(2800.0, 1.5, fs),
            slap: Mode::default(),
            in_contact: false,
            arc_env: Decay::default(),
            arc_hp: Lp1::default(),
            tank: Default::default(),
            tank_c: TANK_HZ.map(|hz| SvfCoeffs::new(hz, 6.0, fs)),
            load: Wander::default(),
            dc: DcBlocker::default(),
            k: [0.5; 6],
            d: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.buzz_bp.reset();
        self.slap.reset();
        self.arc_env.reset();
        self.tank.iter_mut().for_each(Svf::reset);
        self.dc.reset();
        self.inrush.reset();
    }

    pub fn control(&mut self, c: &Ctx) {
        let fs = c.fs;
        self.k = c.k;
        self.buzz_c = SvfCoeffs::new(2400.0 + 1600.0 * c.k[2], 1.5, fs);
        self.slap.set(1350.0, 0.025, fs);
        self.tank_c = TANK_HZ.map(|hz| SvfCoeffs::new(hz, 6.0, fs));
        self.arc_hp.set(2500.0, fs);
    }

    pub fn start(&mut self, c: &Ctx) {
        self.inrush.trigger(1.0, 0.7, c.fs);
    }

    #[inline]
    fn magnetostriction(b: f32, s: f32) -> f32 {
        let m = fast_tanh(s * b) / fast_tanh(s);
        m * m
    }

    #[inline]
    pub fn tick(&mut self, c: &Ctx, d: f32, rng: &mut Noise) -> f32 {
        let fs = c.fs;
        self.d = d;
        if d <= 0.0 {
            self.in_contact = false;
            self.slap.tick(0.0);
            self.arc_env.tick();
            return 0.0;
        }
        let k = self.k;
        let mains = mains_hz(k[0]);
        let inrush = self.inrush.tick();
        let s = 0.7 + 7.0 * k[1] * k[1] + 3.0 * inrush;
        advance(&mut self.phase, mains / fs);
        let beat = if k[4] > 0.0 { beat_hz(k[4]) } else { 0.0 };
        advance(&mut self.phase2, (mains + beat) / fs);
        let b = sin_turns(self.phase);
        let lambda = Self::magnetostriction(b, s);
        let lambda2 = Self::magnetostriction(sin_turns(self.phase2), s);
        let load = 1.0 + 0.25 * k[4] * self.load.tick(0.15, fs, rng);
        let hum = (lambda + 0.85 * k[4] * lambda2) * load;

        // Lamination buzz: contact near the flux peaks.
        let th = 0.97 - 0.45 * (k[2] + 0.5 * inrush).min(1.0);
        let contact = ((lambda - th) / (1.0 - th)).max(0.0);
        if contact > 0.0 && !self.in_contact {
            self.slap.tick(k[2]);
        }
        self.in_contact = contact > 0.0;
        let buzz = svf_bp(&mut self.buzz_bp, &self.buzz_c, rng.sample(), 1.5)
            * contact
            * contact
            * k[2]
            * 3.0
            + 0.4 * self.slap.tick(0.0);

        // Corona: crackles cluster at the voltage peaks.
        let arc_rate = 4.0 + 300.0 * k[3] * k[3];
        if k[3] > 0.0 && rng.chance(arc_rate * 2.0 * b.abs() / fs) {
            self.arc_env
                .trigger_max(0.3 + 0.7 * rng.uniform() * rng.uniform(), 0.003, fs);
        }
        let n = rng.sample();
        let hp = n - self.arc_hp.tick(n);
        let b4 = b * b * b * b;
        let arc = hp * (self.arc_env.tick() * 2.0 + 0.04 * k[3] * b4);

        let raw = self.dc.process(hum, 0.9995);
        let mut tank = 0.0;
        for (f, coeffs) in self.tank.iter_mut().zip(self.tank_c.iter()) {
            tank += svf_bp(f, coeffs, raw, 6.0);
        }
        let body = raw * (1.0 - 0.6 * k[5]) + 0.5 * tank * k[5];
        (0.5 * body + 0.25 * buzz + 0.35 * arc) * d
    }

    pub fn is_quiet(&self) -> bool {
        self.d <= 0.0 && self.slap.energy() < 1e-10 && self.arc_env.value() < 1e-6
    }
}

impl Default for Transformer {
    fn default() -> Self {
        Self::new()
    }
}
