//! PUMP: a reciprocating piston pump, one stroke cycle per RATE (tempo synced).
//!
//! Each cycle has two halves. The discharge stroke starts on the beat: the
//! piston thumps (a pitch-dropping sine), the outlet valve clicks shut and
//! fluid hisses out. The suction stroke follows just past half the cycle: a
//! softer, duller hiss and the inlet valve's click. Under it all the drive
//! motor drones (MOTOR), audibly dragged down in speed by every stroke's
//! load. Everything sounds through a pipe: a feedback comb tuned by PIPE,
//! plus a low pipe knock.

use super::Ctx;
use super::parts::{Cycle, Decay, Follow, Mode, Thump, advance, harmonics, svf_bp};
use crate::params::map;
use crate::util::{DcBlocker, DelayLine, Noise, Svf, SvfCoeffs, one_pole_coeff};

pub fn pipe_hz(v: f32) -> f32 {
    map::expo(v, 60.0, 520.0)
}

/// Share of the cycle the discharge stroke takes.
pub const SPLIT: f32 = 0.55;
/// Drive motor speed (its hum fundamental) at full drive.
const MOTOR_HZ: f32 = 49.0;
const MOTOR_AMPS: [f32; 10] = [1.0, 0.6, 0.55, 0.3, 0.32, 0.18, 0.2, 0.1, 0.12, 0.07];

#[derive(Clone, Debug)]
pub struct Pump {
    cycle: Cycle,
    thump: Thump,
    valve: [Mode; 3],
    hiss: Svf,
    hiss_out: SvfCoeffs,
    hiss_in: SvfCoeffs,
    hiss_env: Follow,
    hiss_target: f32,
    hiss_dull: bool,
    hiss_decay: Decay,
    pipe: DelayLine,
    pipe_lp: f32,
    knock: Mode,
    dc: DcBlocker,
    k: [f32; 6],
    motor_phase: f32,
    motor_whirr: Svf,
    motor_c: SvfCoeffs,
    sucked: bool,
    d: f32,
    power: f32,
}

impl Pump {
    pub fn new(fs: f32) -> Self {
        Self {
            cycle: Cycle::default(),
            thump: Thump::default(),
            valve: Default::default(),
            hiss: Svf::default(),
            hiss_out: SvfCoeffs::new(1800.0, 0.9, fs),
            hiss_in: SvfCoeffs::new(800.0, 0.9, fs),
            hiss_env: Follow::default(),
            hiss_target: 0.0,
            hiss_dull: false,
            hiss_decay: Decay::default(),
            pipe: DelayLine::new((fs / 50.0) as usize + 8),
            pipe_lp: 0.0,
            knock: Mode::default(),
            dc: DcBlocker::default(),
            k: [0.5; 6],
            motor_phase: 0.0,
            motor_whirr: Svf::default(),
            motor_c: SvfCoeffs::new(1600.0, 0.8, fs),
            sucked: true,
            d: 0.0,
            power: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.cycle.reset();
        self.thump.reset();
        self.valve.iter_mut().for_each(Mode::reset);
        self.knock.reset();
        self.hiss.reset();
        self.hiss_env.reset();
        self.hiss_decay.reset();
        self.pipe.clear();
        self.pipe_lp = 0.0;
    }

    pub fn control(&mut self, c: &Ctx) {
        self.k = c.k;
        self.cycle.set(map::sync_beats(c.k[0]), c.beat_s, c.fs);
        let pipe = pipe_hz(c.k[4]);
        self.knock.set(pipe * 0.5, 0.12, c.fs);
    }

    pub fn start(&mut self, c: &Ctx) {
        self.cycle.start(c.fs);
    }

    fn discharge(&mut self, fs: f32, rng: &mut Noise) {
        let p = self.power;
        let period = self.cycle.period(fs);
        let thump = self.k[1];
        self.thump.trigger(
            p * thump * (0.9 + 0.1 * rng.uniform()),
            48.0 + 30.0 * (1.0 - thump),
            1.8,
            0.05,
            0.3,
            fs,
        );
        self.knock.tick(p * thump * 0.6);
        let v = self.k[3] * p;
        for (m, (hz, t)) in
            self.valve
                .iter_mut()
                .zip([(2300.0, 0.02), (3900.0, 0.012), (640.0, 0.03)])
        {
            m.set(hz * (1.0 + 0.02 * rng.sample()), t, fs);
            m.tick(v);
        }
        self.hiss_target = p;
        self.hiss_dull = false;
        let len = (SPLIT * period).min(0.7);
        self.hiss_decay.trigger(1.0, len, fs);
        self.sucked = false;
    }

    fn suction(&mut self, fs: f32, rng: &mut Noise) {
        let p = self.power;
        let period = self.cycle.period(fs);
        let v = 0.6 * self.k[3] * p;
        for (m, (hz, t)) in
            self.valve
                .iter_mut()
                .zip([(1600.0, 0.02), (2700.0, 0.01), (520.0, 0.02)])
        {
            m.set(hz * (1.0 + 0.02 * rng.sample()), t, fs);
            m.tick(v);
        }
        self.hiss_target = 0.55 * p;
        self.hiss_dull = true;
        let len = ((1.0 - SPLIT) * period).min(0.6);
        self.hiss_decay.trigger(1.0, len, fs);
        self.sucked = true;
    }

    #[inline]
    pub fn tick(&mut self, c: &Ctx, d: f32, rng: &mut Noise) -> f32 {
        let fs = c.fs;
        self.d = d;
        self.power = d;
        if self.cycle.tick(c.gate) {
            self.discharge(fs, rng);
        }
        if !self.sucked && self.cycle.phase() >= SPLIT {
            self.suction(fs, rng);
        }

        let attack = one_pole_coeff(0.006, fs);
        let env = self
            .hiss_env
            .tick(self.hiss_target * self.hiss_decay.tick(), attack);
        let coeffs = if self.hiss_dull {
            self.hiss_in
        } else {
            self.hiss_out
        };
        let hiss = svf_bp(&mut self.hiss, &coeffs, rng.sample(), 0.9) * env * self.k[2];

        let mut valves = 0.0;
        for m in self.valve.iter_mut() {
            valves += m.tick(0.0);
        }
        let body = self.thump.tick(fs) + 0.5 * self.knock.tick(0.0);
        // The drive motor, slowed a little by each stroke's load.
        let mut motor = 0.0;
        if d > 0.0 && self.k[5] > 0.0 {
            let hz = MOTOR_HZ * (0.8 + 0.2 * d) * (1.0 - 0.06 * env);
            let inc = hz / fs;
            advance(&mut self.motor_phase, inc);
            motor = (harmonics(self.motor_phase, inc, &MOTOR_AMPS, 0.45)
                + 0.5 * svf_bp(&mut self.motor_whirr, &self.motor_c, rng.sample(), 0.8))
                * self.k[5]
                * d;
        }
        let x = body + 0.3 * valves + 0.8 * hiss + 0.25 * motor;

        // Pipe: a damped feedback comb.
        let period = fs / pipe_hz(self.k[4]);
        let fb = self.pipe.read(period);
        self.pipe_lp += (fb - self.pipe_lp) * 0.5;
        let y = x + 0.62 * self.pipe_lp;
        self.pipe.write(y);
        let out = 0.55 * x + 0.45 * y;
        0.8 * self.dc.process(out, 0.9995)
    }

    pub fn is_quiet(&self) -> bool {
        self.d <= 0.0
            && self.hiss_env.value() < 1e-5
            && self.knock.energy() < 1e-10
            && self.valve.iter().all(|m| m.energy() < 1e-10)
    }
}
