//! STEAM: a steam vent on a boiler.
//!
//! The vent hisses: noise around the VENT tone, plus a bright edge, made
//! turbulent by fast random wander and louder and brighter with PRESSURE.
//! WHISTLE adds a breathy tone at a resonance of the vent: very narrow
//! band-passed noise and a little vibrato. CHUFF chops the vent into
//! tempo-synced chuffs (or leaves it continuous). The boiler rumbles, and
//! condensate sputters in quick downward blips. Opening the valve (the start
//! of each active period) gives a burst of extra pressure.

use super::parts::{Cycle, Decay, Lp1, Wander, advance, svf_bp};
use super::{CHUFF_BEATS, Ctx, MACHINE_STEAM, step_of};
use crate::params::map;
use crate::util::{Noise, Svf, SvfCoeffs, sin_turns, t60_gain};

pub fn vent_hz(v: f32) -> f32 {
    map::expo(v, 400.0, 7000.0)
}

#[derive(Clone, Debug)]
pub struct Steam {
    hiss: Svf,
    hiss_c: SvfCoeffs,
    edge: Svf,
    edge_c: SvfCoeffs,
    whistle: Svf,
    whistle_c: SvfCoeffs,
    whistle_hz: f32,
    coef_n: u8,
    vib: f32,
    turb: Wander,
    burst: Decay,
    chuff: Cycle,
    chuff_on: bool,
    chuff_env: Decay,
    chuff_t60: f32,
    rumble: Lp1,
    low: Lp1,
    rumble2: Lp1,
    drone: f32,
    drone_wander: Wander,
    blip: f32,
    blip_hz: f32,
    blip_env: f32,
    blip_mul: f32,
    k: [f32; 6],
    d: f32,
}

impl Steam {
    pub fn new() -> Self {
        let fs = 48_000.0;
        Self {
            hiss: Svf::default(),
            hiss_c: SvfCoeffs::new(2000.0, 0.7, fs),
            edge: Svf::default(),
            edge_c: SvfCoeffs::new(7000.0, 0.7, fs),
            whistle: Svf::default(),
            whistle_c: SvfCoeffs::new(1000.0, 60.0, fs),
            whistle_hz: 1000.0,
            coef_n: 0,
            vib: 0.0,
            turb: Wander::default(),
            burst: Decay::default(),
            chuff: Cycle::default(),
            chuff_on: false,
            chuff_env: Decay::default(),
            chuff_t60: 0.3,
            rumble: Lp1::default(),
            low: Lp1::default(),
            rumble2: Lp1::default(),
            drone: 0.0,
            drone_wander: Wander::default(),
            blip: 0.0,
            blip_hz: 2000.0,
            blip_env: 0.0,
            blip_mul: 0.0,
            k: [0.5; 6],
            d: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.hiss.reset();
        self.edge.reset();
        self.whistle.reset();
        self.burst.reset();
        self.chuff.reset();
        self.chuff_env.reset();
        self.rumble.reset();
        self.rumble2.reset();
        self.blip_env = 0.0;
    }

    pub fn control(&mut self, c: &Ctx) {
        let fs = c.fs;
        self.k = c.k;
        let vent = vent_hz(c.k[1]);
        let p = c.k[0];
        self.hiss_c = SvfCoeffs::new((vent * (0.8 + 0.4 * p)).min(0.4 * fs), 0.5, fs);
        self.edge_c = SvfCoeffs::new((5000.0 + 6000.0 * p).min(0.4 * fs), 0.7, fs);
        self.whistle_hz = (vent * 0.4).clamp(350.0, 3200.0);
        let beats = CHUFF_BEATS[step_of(MACHINE_STEAM, 3, c.k[3])];
        self.chuff_on = beats > 0.0;
        if self.chuff_on {
            self.chuff.set(beats, c.beat_s, fs);
            self.chuff_t60 = (beats * c.beat_s * 0.8).min(0.6);
        }
        self.rumble.set(90.0, fs);
        self.low.set(700.0, fs);
        self.rumble2.set(90.0, fs);
    }

    pub fn start(&mut self, c: &Ctx) {
        self.burst.trigger(1.0, 0.35, c.fs);
        self.chuff.start(c.fs);
    }

    #[inline]
    pub fn tick(&mut self, c: &Ctx, d: f32, rng: &mut Noise) -> f32 {
        let fs = c.fs;
        self.d = d;
        let k = self.k;
        if d <= 0.0 && self.blip_env < 1e-5 {
            return 0.0;
        }
        let burst = self.burst.tick();
        let pressure = (k[0] * (1.0 + 0.6 * burst)).min(1.4);
        let turb = 1.0 + 0.3 * self.turb.tick(14.0, fs, rng);

        let mut gate = 1.0;
        if self.chuff_on {
            if self.chuff.tick(c.gate) {
                self.chuff_env.trigger(1.0, self.chuff_t60, fs);
            }
            gate = 0.08 + 0.92 * self.chuff_env.tick();
        }

        let n = rng.sample();
        let hiss = svf_bp(&mut self.hiss, &self.hiss_c, n, 0.5)
            + 0.5 * self.edge.process(n, &self.edge_c).2 * pressure;
        let hiss = hiss - self.low.tick(hiss);

        let mut whistle = 0.0;
        if k[2] > 0.0 {
            advance(&mut self.vib, 5.3 / fs);
            let hz = self.whistle_hz * (1.0 + 0.004 * sin_turns(self.vib));
            if self.coef_n == 0 {
                self.whistle_c = SvfCoeffs::new(hz, 60.0, fs);
                self.coef_n = 32;
            }
            self.coef_n -= 1;
            whistle =
                svf_bp(&mut self.whistle, &self.whistle_c, rng.sample(), 60.0) * 2.5 * pressure;
        }

        let r = self.rumble2.tick(self.rumble.tick(rng.sample()));
        let dw = self.drone_wander.tick(0.3, fs, rng);
        advance(&mut self.drone, (47.0 + 2.0 * dw) / fs);
        let rumble = 6.0 * r + 0.25 * sin_turns(self.drone);

        // Condensate: quick downward blips.
        let sputter_rate = 0.5 + 45.0 * k[5] * k[5];
        if k[5] > 0.0 && d > 0.0 && rng.chance(sputter_rate / fs) {
            self.blip_hz = 900.0 + 2600.0 * rng.uniform();
            self.blip_env = k[5] * (0.4 + 0.6 * rng.uniform());
            self.blip_mul = t60_gain(0.012, fs);
        }
        let mut sputter = 0.0;
        if self.blip_env > 1e-5 {
            self.blip_hz *= 0.9997;
            advance(&mut self.blip, self.blip_hz / fs);
            sputter = sin_turns(self.blip) * self.blip_env;
            self.blip_env *= self.blip_mul;
        }

        let vent = (0.5 * hiss * pressure * turb + 0.15 * whistle * k[2]) * gate * d;
        vent + 0.3 * rumble * k[4] * d + 0.12 * sputter
    }

    pub fn is_quiet(&self) -> bool {
        self.d <= 0.0 && self.blip_env < 1e-5
    }
}

impl Default for Steam {
    fn default() -> Self {
        Self::new()
    }
}
