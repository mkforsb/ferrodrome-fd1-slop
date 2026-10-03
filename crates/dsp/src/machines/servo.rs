//! SERVO: a servo axis making one move per RATE (tempo synced).
//!
//! Each move follows a trapezoidal velocity profile (accelerate, cruise,
//! decelerate) lasting TRAVEL of the cycle. The motor whines at a pitch
//! proportional to its speed, so it rises, holds and falls; GEAR adds the
//! gearbox: a mesh tone at a fixed ratio above the motor with sidebands, and
//! grit. Moves alternate direction, and the return is a little slower. At the
//! end stop it clunks. HOLD is what a servo does at rest: the drive's PWM
//! whines while it holds position, and the controller makes a few small
//! corrective moves (hunting) after each stop.

use super::Ctx;
use super::parts::{Cycle, Mode, Thump, advance, harmonics, svf_bp};
use crate::params::map;
use crate::util::{Noise, Svf, SvfCoeffs, sin_turns};

pub fn whine_hz(v: f32) -> f32 {
    map::expo(v, 140.0, 1900.0)
}

pub fn travel(v: f32) -> f32 {
    0.1 + 0.85 * v
}

/// Gear mesh frequency relative to the motor.
const GEAR_RATIO: f32 = 4.62;
const CLUNK_MODES: [(f32, f32); 4] = [(180.0, 0.2), (432.0, 0.13), (985.0, 0.08), (1730.0, 0.05)];
/// Hunting moves: (start after the stop, length, speed).
const HUNT: [(f32, f32, f32); 3] = [(0.05, 0.03, 0.3), (0.11, 0.025, 0.18), (0.16, 0.02, 0.09)];

/// Trapezoid velocity at `u` = 0..1 through a move, smoothed corners.
fn profile(u: f32) -> f32 {
    let ramp = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    if u < 0.25 {
        ramp(u / 0.25)
    } else if u < 0.75 {
        1.0
    } else {
        ramp((1.0 - u) / 0.25)
    }
}

#[derive(Clone, Debug)]
pub struct Servo {
    cycle: Cycle,
    /// Seconds into the current move (negative = no move).
    t: f32,
    len: f32,
    peak: f32,
    forward: bool,
    since_stop: f32,
    phase: f32,
    gear_phase: f32,
    grit: Svf,
    grit_c: SvfCoeffs,
    coef_n: u8,
    clunk: [Mode; 4],
    thump: Thump,
    k: [f32; 6],
    pwm_phase: f32,
    d: f32,
    power: f32,
}

/// Holding whine: the drive's PWM and its harmonics.
const PWM_HZ: f32 = 1180.0;
const PWM_AMPS: [f32; 4] = [1.0, 0.5, 0.7, 0.25];

impl Servo {
    pub fn new() -> Self {
        Self {
            cycle: Cycle::default(),
            t: -1.0,
            len: 0.3,
            peak: 1.0,
            forward: false,
            since_stop: 10.0,
            phase: 0.0,
            gear_phase: 0.0,
            grit: Svf::default(),
            grit_c: SvfCoeffs::new(1000.0, 3.0, 48_000.0),
            coef_n: 0,
            clunk: Default::default(),
            thump: Thump::default(),
            k: [0.5; 6],
            pwm_phase: 0.0,
            d: 0.0,
            power: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.cycle.reset();
        self.t = -1.0;
        self.since_stop = 10.0;
        self.clunk.iter_mut().for_each(Mode::reset);
        self.thump.reset();
        self.grit.reset();
    }

    pub fn control(&mut self, c: &Ctx) {
        self.k = c.k;
        self.cycle.set(map::sync_beats(c.k[0]), c.beat_s, c.fs);
        for (m, (hz, t)) in self.clunk.iter_mut().zip(CLUNK_MODES) {
            m.set(hz, t, c.fs);
        }
    }

    pub fn start(&mut self, c: &Ctx) {
        self.cycle.start(c.fs);
    }

    fn velocity(&self) -> f32 {
        let mut v = 0.0;
        if self.t >= 0.0 && self.t < self.len {
            v = self.peak * profile(self.t / self.len);
        }
        let h = self.k[5];
        if h > 0.0 {
            for (at, len, speed) in HUNT {
                let x = self.since_stop - at;
                if x >= 0.0 && x < len {
                    v += h * speed * profile(x / len);
                }
            }
        }
        v
    }

    #[inline]
    pub fn tick(&mut self, c: &Ctx, d: f32, rng: &mut Noise) -> f32 {
        let fs = c.fs;
        self.d = d;
        if self.cycle.tick(c.gate) {
            self.power = d;
            self.forward = !self.forward;
            self.t = 0.0;
            self.len = (travel(self.k[3]) * self.cycle.period(fs)).max(0.05);
            self.peak = (0.88 + 0.24 * rng.uniform()) * if self.forward { 1.0 } else { 0.86 };
        }
        let dt = 1.0 / fs;
        if self.t >= 0.0 {
            self.t += dt;
            if self.t >= self.len {
                self.t = -1.0;
                self.since_stop = 0.0;
                let a = self.k[4] * self.power * (0.85 + 0.15 * rng.uniform());
                for (i, m) in self.clunk.iter_mut().enumerate() {
                    m.tick(a * [1.0, 0.7, 0.45, 0.3][i]);
                }
                self.thump.trigger(a, 62.0, 1.6, 0.02, 0.09, fs);
            }
        }
        self.since_stop += dt;

        let v = self.velocity();
        let mut whine = 0.0;
        if v > 1e-4 {
            let hz = whine_hz(self.k[1]) * v;
            advance(&mut self.phase, hz / fs);
            advance(&mut self.gear_phase, (hz * GEAR_RATIO).min(0.45 * fs) / fs);
            let p = self.phase;
            let gear = self.k[2];
            let motor = sin_turns(p) + 0.35 * sin_turns(2.0 * p) + 0.12 * sin_turns(3.0 * p);
            let mesh = sin_turns(self.gear_phase) * (1.0 + 0.7 * sin_turns(p));
            if self.coef_n == 0 {
                self.grit_c = SvfCoeffs::new((hz * GEAR_RATIO).min(0.4 * fs), 3.0, fs);
                self.coef_n = 32;
            }
            self.coef_n -= 1;
            let grit = svf_bp(&mut self.grit, &self.grit_c, rng.sample(), 3.0);
            whine = (motor * (1.0 - 0.4 * gear) + gear * (0.6 * mesh + 0.9 * grit)) * v.sqrt();
        }
        let mut clunk = 0.0;
        for m in self.clunk.iter_mut() {
            clunk += m.tick(0.0);
        }
        let body = 0.3 * clunk + 0.5 * self.thump.tick(fs);
        let mut hold = 0.0;
        if d > 0.0 && self.k[5] > 0.0 {
            let inc = PWM_HZ / fs;
            advance(&mut self.pwm_phase, inc);
            // Quieter while moving: the current goes into the motion.
            hold = harmonics(self.pwm_phase, inc, &PWM_AMPS, 0.45)
                * self.k[5]
                * d
                * (1.0 - 0.6 * v.min(1.0));
        }
        (0.22 * whine * self.power.max(d) + body + 0.03 * hold) * 0.9
    }

    pub fn is_quiet(&self) -> bool {
        self.d <= 0.0
            && self.t < 0.0
            && self.since_stop > 0.5
            && self.clunk.iter().all(|m| m.energy() < 1e-10)
    }
}

impl Default for Servo {
    fn default() -> Self {
        Self::new()
    }
}
