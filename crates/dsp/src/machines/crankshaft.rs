//! CRANKSHAFT: a combustion engine as a pulse train through resonators.
//!
//! An engine's sound is a train of exhaust pressure pulses, one per firing,
//! at `RPM / 60 × cylinders / 2` (four-stroke), shaped by the exhaust pipe
//! and the block. Here each firing is a short decaying pressure pulse
//! (sharper and brighter with LOAD), with a fixed per-cylinder imbalance and,
//! with ROUGH, random strength, timing slop and the odd misfire. The pulses
//! drive a Karplus–Strong exhaust pipe (EXHAUST) and two block modes; KNOCK
//! adds the valve train's ticking half a firing later. The drive scales the
//! engine speed, so the ramps are a start-up and a run-down.

use super::parts::{Decay, Lp1, Mode, Pow, advance};
use super::{CYL_COUNTS, Ctx, MACHINE_CRANKSHAFT, step_of};
use crate::params::map;
use crate::util::{DcBlocker, DelayLine, Noise};

pub fn rpm(v: f32) -> f32 {
    map::expo(v, 400.0, 5200.0)
}

pub fn exhaust_hz(v: f32) -> f32 {
    map::expo(v, 35.0, 420.0)
}

/// Fixed imbalance between cylinders.
const CYL_GAIN: [f32; 12] = [
    1.0, 0.86, 1.08, 0.93, 1.12, 0.9, 1.03, 0.88, 1.1, 0.95, 0.84, 1.06,
];

#[derive(Clone, Debug)]
pub struct Crankshaft {
    pw: Pow,
    fs: f32,
    phase: f32,
    ticked: bool,
    cyl: usize,
    cylinders: usize,
    pulse: f32,
    pulse_mul: f32,
    pulse_gain: f32,
    pulse_lp: Lp1,
    pipe: DelayLine,
    pipe_lp: f32,
    pipe_period: f32,
    block: [Mode; 2],
    tick_modes: [Mode; 2],
    knock_env: Decay,
    clatter: Decay,
    clatter_hp: Lp1,
    dc: DcBlocker,
    k: [f32; 6],
    d: f32,
}

impl Crankshaft {
    pub fn new(fs: f32) -> Self {
        Self {
            pw: Pow::default(),
            fs,
            phase: 0.0,
            ticked: true,
            cyl: 0,
            cylinders: 4,
            pulse: 0.0,
            pulse_mul: 0.0,
            pulse_gain: 0.0,
            pulse_lp: Lp1::default(),
            pipe: DelayLine::new((fs / 30.0) as usize + 8),
            pipe_lp: 0.0,
            pipe_period: 100.0,
            block: Default::default(),
            tick_modes: Default::default(),
            knock_env: Decay::default(),
            clatter: Decay::default(),
            clatter_hp: Lp1::default(),
            dc: DcBlocker::default(),
            k: [0.5; 6],
            d: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.pulse = 0.0;
        self.pulse_lp.reset();
        self.pipe.clear();
        self.pipe_lp = 0.0;
        self.block.iter_mut().for_each(Mode::reset);
        self.tick_modes.iter_mut().for_each(Mode::reset);
        self.dc.reset();
    }

    pub fn control(&mut self, c: &Ctx) {
        let fs = c.fs;
        self.k = c.k;
        self.cylinders = CYL_COUNTS[step_of(MACHINE_CRANKSHAFT, 1, c.k[1])] as usize;
        let load = c.k[5];
        let tau = map::expo(1.0 - load, 0.0007, 0.005);
        self.pulse_mul = (-1.0 / (tau * fs)).exp();
        self.pulse_lp.set(900.0 + 9000.0 * load * load, fs);
        self.clatter_hp.set(1400.0, fs);
        self.pipe_period = fs / exhaust_hz(c.k[3]);
        self.block[0].set(92.0, 0.09, fs);
        self.block[1].set(310.0, 0.05, fs);
        self.tick_modes[0].set(3300.0, 0.008, fs);
        self.tick_modes[1].set(5900.0, 0.006, fs);
    }

    fn fire(&mut self, rng: &mut Noise) {
        let rough = self.k[2];
        self.cyl = (self.cyl + 1) % self.cylinders.max(1);
        let mut g = CYL_GAIN[self.cyl] * (1.0 + 0.5 * rough * rng.gauss() * 0.5).max(0.1);
        if rng.chance(rough * rough * 0.18) {
            g *= 0.08;
        }
        // Timing slop: the next firing comes a little early or late.
        self.phase -= rough * 0.12 * rng.uniform();
        self.pulse = 1.0;
        self.pulse_gain = g * (0.55 + 0.45 * self.k[5]);
        // Combustion clatter: a burst of broadband noise with every firing.
        let clatter = 0.25 + 0.5 * self.k[4] + 0.25 * self.k[5];
        self.clatter.trigger(g * clatter, 0.01, self.fs);
        self.block[0].tick(0.9 * g);
        self.block[1].tick(0.5 * g);
        self.ticked = false;
    }

    #[inline]
    pub fn tick(&mut self, c: &Ctx, d: f32, rng: &mut Noise) -> f32 {
        let fs = c.fs;
        self.d = d;
        let firing_hz = rpm(self.k[0]) * d / 60.0 * self.cylinders as f32 / 2.0;
        if d > 0.002 && advance(&mut self.phase, firing_hz / fs) {
            self.fire(rng);
        }
        if !self.ticked && self.phase >= 0.5 {
            self.ticked = true;
            let k = self.k[4];
            if k > 0.0 {
                let a = k * (0.6 + 0.4 * rng.uniform());
                self.tick_modes[0].tick(a);
                self.tick_modes[1].tick(0.6 * a);
                self.knock_env.trigger(k, 0.004, fs);
            }
        }
        let p = self.pulse * self.pulse_gain * (0.75 + 0.25 * rng.sample());
        self.pulse *= self.pulse_mul;
        let p = self.pulse_lp.tick(p);

        let fb = self.pipe.read(self.pipe_period);
        self.pipe_lp += (fb - self.pipe_lp) * 0.45;
        let y = p + 0.82 * self.pipe_lp;
        self.pipe.write(y);

        let block = self.block[0].tick(0.0) + self.block[1].tick(0.0);
        let ticks = self.tick_modes[0].tick(0.0) + self.tick_modes[1].tick(0.0);
        let knock = self.knock_env.tick() * rng.sample();
        let n = rng.sample();
        let clatter = (n - self.clatter_hp.tick(n)) * self.clatter.tick();
        let out = 0.45 * y + 0.35 * p + 0.06 * block + 0.1 * ticks + 0.05 * knock + 0.2 * clatter;
        let power = self.pw.get(d, 0.5);
        self.dc.process(out, 0.9995) * power * 1.3
    }

    pub fn is_quiet(&self) -> bool {
        self.d <= 0.0 && self.pulse < 1e-6 && self.block.iter().all(|m| m.energy() < 1e-10)
    }
}
