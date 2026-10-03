//! One track: a machine, its ramps, VARIANCE, MUTATE, a ladder filter and
//! EQ, and its own reverb (with an EQ on the return) and delay.
//!
//! ```text
//!  gate/hold ─► RAMP (up/down) ─► drive ─┬──────────────┐ (RAMP→ cutoff)
//!  patch ─► VARIANCE ─► smoothing ─► MACHINE ─► FILTER ─► EQ ─► LEVEL·MUTE ─┬─ pan ───────────────(+)─► out
//!                                                                          ├─ send ─► PLATE ─► EQ ─┤
//!                                                                          └─ send ─► PING-PONG ───┘
//! ```
//!
//! Controls are evaluated once per [`CONTROL_BLOCK`] samples; the ramp,
//! the machine and the effects run every sample. When a new active period
//! starts (the gate or a held RUN opens), MUTATE acts first, then the machine
//! is told to start, so a mutated sound begins exactly on its step.

use crate::delay::PingPong;
use crate::filter::{Ladder, LadderCoeffs, SvfEq, TrackEq};
use crate::machines::parts::Wander;
use crate::machines::{Ctx, Machines};
use crate::mutate::{Locks, mutate_patch};
use crate::params::{
    ALL_PARAMS, FILTER_OFF, KNOB_PARAMS, PARAM_COUNT, Param, ParamKind, TrackPatch, default_patch,
    map,
};
use crate::reverb::{Plate, PlateControls};
use crate::util::{Noise, one_pole_coeff};

pub const CONTROL_BLOCK: usize = 32;
/// Parameter de-zippering.
const SMOOTH_TAU_S: f32 = 0.012;
/// How fast VARIANCE wanders (Hz) and how far it can reach at full setting.
const VARIANCE_HZ: f32 = 0.22;
const VARIANCE_REACH: f32 = 0.28;

/// Reverb return EQ corner/centre frequencies (shared with the master).
pub const EQ_LOW_HZ: f32 = 220.0;
pub const EQ_MID_HZ: f32 = 1_200.0;
pub const EQ_HIGH_HZ: f32 = 4_500.0;

/// Low shelf / mid peak / high shelf on a stereo return.
#[derive(Clone, Debug)]
pub struct ReturnEq {
    eq: SvfEq,
}

impl ReturnEq {
    pub fn new() -> Self {
        Self { eq: SvfEq::new() }
    }

    pub fn update(&mut self, knobs: [f32; 3], fs: f32) {
        let [low, mid, high] = knobs.map(map::eq_db);
        self.eq
            .set([low, mid, EQ_MID_HZ, high], EQ_LOW_HZ, 0.8, EQ_HIGH_HZ, fs);
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        (self.eq.process(0, l), self.eq.process(1, r))
    }
}

impl Default for ReturnEq {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct Track {
    fs: f32,
    /// The patch as set by the panel (or MUTATE).
    base: TrackPatch,
    locks: Locks,
    /// After VARIANCE.
    eff: TrackPatch,
    smooth: TrackPatch,
    k_smooth: f32,
    wander: [Wander; PARAM_COUNT],
    /// Per parameter, for the current machine: whether it is de-zippered
    /// and whether VARIANCE may move it.
    smoothed: [bool; PARAM_COUNT],
    varies: [bool; PARAM_COUNT],
    /// Last values the expensive control-rate state was derived from.
    derived: [f32; 6],
    machines: Machines,
    machine: u32,
    ctx: Ctx,
    block_pos: usize,
    steps: u128,
    gate: bool,
    hold: bool,
    ramp: f32,
    up_inc: f32,
    down_inc: f32,
    gain: f32,
    gain_target: f32,
    k_gain: f32,
    pan: (f32, f32),
    plate: Plate,
    plate_ctl: PlateControls,
    ladder: Ladder,
    ladder_c: LadderCoeffs,
    eq: TrackEq,
    rev_eq: ReturnEq,
    rev_quiet: u32,
    rev_idle_after: u32,
    delay: PingPong,
    dly_seconds: f32,
    dly_feedback: f32,
    dly_tone: f32,
    rng: Noise,
    mutations: u32,
}

impl Track {
    pub fn new(fs: f32, seed: u32) -> Self {
        let patch = default_patch(0);
        let plate = Plate::new(fs);
        let plate_ctl = plate.controls(0.3, 0.5, 0.0);
        let mut t = Self {
            fs,
            base: patch,
            locks: [false; PARAM_COUNT],
            eff: patch,
            smooth: patch,
            k_smooth: one_pole_coeff(SMOOTH_TAU_S, fs / CONTROL_BLOCK as f32),
            wander: core::array::from_fn(|_| Wander::default()),
            smoothed: [false; PARAM_COUNT],
            varies: [false; PARAM_COUNT],
            derived: [f32::NAN; 6],
            machines: Machines::new(fs),
            machine: 0,
            ctx: Ctx::new(fs),
            block_pos: 0,
            steps: 0,
            gate: false,
            hold: false,
            ramp: 0.0,
            up_inc: 0.0,
            down_inc: 0.0,
            gain: 0.0,
            gain_target: 0.0,
            k_gain: one_pole_coeff(0.004, fs),
            pan: (1.0, 1.0),
            plate,
            plate_ctl,
            ladder: Ladder::default(),
            ladder_c: LadderCoeffs::new(0, 1_000.0, 0.0, 0.0, fs),
            eq: TrackEq::new(),
            rev_eq: ReturnEq::new(),
            rev_quiet: u32::MAX,
            rev_idle_after: 0,
            delay: PingPong::new(fs),
            dly_seconds: 0.25,
            dly_feedback: 0.0,
            dly_tone: 0.5,
            rng: Noise::new(seed.wrapping_mul(7919) + 17),
            mutations: 0,
        };
        t.update_flags();
        t.control(0.5);
        t
    }

    fn update_flags(&mut self) {
        for p in ALL_PARAMS {
            let i = p.index();
            self.smoothed[i] =
                p.info().kind == ParamKind::Continuous && p.steps_for(self.machine).is_none();
            self.varies[i] = p.varies(self.machine);
        }
    }

    pub fn enabled(&self) -> bool {
        self.base[Param::Enabled.index()] >= 0.5
    }

    pub fn param(&self, p: Param) -> f32 {
        self.base[p.index()]
    }

    pub fn patch(&self) -> TrackPatch {
        self.base
    }

    /// The patch after VARIANCE (what the machine is heading for right now).
    pub fn live(&self) -> TrackPatch {
        self.eff
    }

    pub fn length(&self) -> usize {
        self.base[Param::Length.index()] as usize
    }

    pub fn set_param(&mut self, p: Param, value: f32) {
        let v = p.sanitize(value);
        let i = p.index();
        let was_enabled = self.enabled();
        self.base[i] = v;
        self.eff[i] = v;
        if !self.smoothed[i] {
            self.smooth[i] = v;
        }
        if p == Param::Machine && v as u32 != self.machine {
            self.switch_machine(v as u32);
        }
        if p == Param::Enabled && was_enabled != self.enabled() {
            // A track added (or removed) starts from rest.
            self.machines.reset(self.machine);
            self.ladder.reset();
            self.ramp = 0.0;
            self.gain = 0.0;
            self.plate = Plate::new(self.fs);
            self.delay.clear();
            self.rev_quiet = u32::MAX;
        }
    }

    pub fn set_lock(&mut self, p: Param, locked: bool) {
        self.locks[p.index()] = locked;
    }

    pub fn locks(&self) -> &Locks {
        &self.locks
    }

    pub fn set_step(&mut self, s: usize, on: bool) {
        crate::steps::set(&mut self.steps, s, on);
    }

    pub fn steps(&self) -> u128 {
        self.steps
    }

    pub fn mutations(&self) -> u32 {
        self.mutations
    }

    fn switch_machine(&mut self, m: u32) {
        self.machine = m;
        self.machines.reset(m);
        self.update_flags();
    }

    pub fn is_active(&self) -> bool {
        self.gate || self.hold
    }

    /// Spin-up/down position, 0..1, shaped.
    pub fn drive(&self) -> f32 {
        let r = self.ramp;
        r * r * (3.0 - 2.0 * r)
    }

    /// Open or close the sequencer gate.
    pub fn set_gate(&mut self, on: bool) {
        let was = self.is_active();
        self.gate = on;
        if !was && self.is_active() {
            self.begin_period();
        }
    }

    /// Hold the machine running (RUN button, keyboard).
    pub fn set_hold(&mut self, on: bool) {
        let was = self.is_active();
        self.hold = on;
        if !was && self.is_active() {
            self.begin_period();
        }
    }

    /// A new active period: MUTATE first, then start the machine.
    fn begin_period(&mut self) {
        let amount = self.base[Param::Mutate.index()];
        if amount > 0.0 {
            let next = mutate_patch(&self.base, &self.locks, amount, &mut self.rng);
            for p in ALL_PARAMS {
                let i = p.index();
                if next[i] != self.base[i] {
                    self.set_param(p, next[i]);
                }
            }
            self.control(self.ctx.beat_s);
            // The new sound starts on the step rather than gliding in.
            self.smooth = self.eff;
            self.mutations = self.mutations.wrapping_add(1);
        }
        self.ctx.gate = true;
        self.control(self.ctx.beat_s);
        self.machines.start(self.machine, &self.ctx);
    }

    /// Jump smoothed values to their targets.
    pub fn snap(&mut self) {
        self.eff = self.base;
        self.smooth = self.base;
        self.gain = self.gain_target;
        self.delay.snap_time(self.dly_seconds);
    }

    /// Recompute the control-rate state. `beat_s` is seconds per beat.
    pub fn control(&mut self, beat_s: f32) {
        let fs = self.fs;
        let dt = CONTROL_BLOCK as f32 / fs;
        let variance = self.base[Param::Variance.index()];
        let reach = VARIANCE_REACH * variance;
        for i in 0..PARAM_COUNT {
            // Off stays off: a control at zero (a send, SQUEAL, PULSE…) is left alone.
            if reach > 0.0 && self.varies[i] && !self.locks[i] && self.base[i] > 0.0 {
                let w = self.wander[i].step(VARIANCE_HZ, dt, &mut self.rng);
                self.eff[i] = (self.base[i] + reach * w).clamp(0.0, 1.0);
            } else {
                self.eff[i] = self.base[i];
            }
            if self.smoothed[i] {
                self.smooth[i] += (self.eff[i] - self.smooth[i]) * self.k_smooth;
            } else {
                self.smooth[i] = self.eff[i];
            }
        }
        let s = self.smooth;
        for (slot, p) in KNOB_PARAMS.iter().enumerate() {
            self.ctx.k[slot] = s[p.index()];
        }
        self.ctx.beat_s = beat_s;
        self.ctx.gate = self.is_active();
        self.machines.control(self.machine, &self.ctx);

        // The rest only changes when its knobs move.
        let key = [
            s[Param::RampUp.index()],
            s[Param::RampDown.index()],
            s[Param::Pan.index()],
            s[Param::RevDecay.index()],
            s[Param::RevTone.index()],
            s[Param::DlyTone.index()],
        ];
        if key != self.derived {
            self.derived = key;
            self.derive(key, fs);
        }
        self.gain_target = map::level_gain(s[Param::Level.index()]);
        self.rev_eq.update(
            [
                s[Param::RevLow.index()],
                s[Param::RevMid.index()],
                s[Param::RevHigh.index()],
            ],
            fs,
        );
        self.dly_seconds = map::delay_beats(s[Param::DlyTime.index()]) * beat_s;
        self.dly_feedback = map::feedback(s[Param::DlyFeedback.index()]);

        // The filter follows the ramp (RAMP→) at control rate.
        let mode = s[Param::FltType.index()] as u32;
        if mode != FILTER_OFF {
            let hz = map::cutoff_hz(s[Param::FltCutoff.index()])
                * (map::env_octaves(s[Param::FltEnv.index()]) * self.drive()).exp2();
            self.ladder_c = LadderCoeffs::new(
                mode,
                hz,
                s[Param::FltReso.index()],
                s[Param::FltDrive.index()],
                fs,
            );
        } else if !self.ladder_c.is_off() {
            self.ladder_c = LadderCoeffs::new(FILTER_OFF, 1_000.0, 0.0, 0.0, fs);
            self.ladder.reset();
        }
        self.eq.update(
            s[Param::EqLow.index()],
            s[Param::EqMid.index()],
            s[Param::EqFreq.index()],
            s[Param::EqHigh.index()],
            fs,
        );
    }

    fn derive(&mut self, [up, down, pan, rev_decay, rev_tone, dly_tone]: [f32; 6], fs: f32) {
        self.up_inc = 1.0 / (map::ramp_seconds(up) * fs);
        self.down_inc = 1.0 / (map::ramp_seconds(down) * fs);
        let a = pan * core::f32::consts::FRAC_PI_2;
        self.pan = (
            a.cos() * core::f32::consts::SQRT_2,
            a.sin() * core::f32::consts::SQRT_2,
        );
        self.plate_ctl = self.plate.controls(rev_decay, rev_tone, 0.0);
        self.rev_idle_after = ((map::reverb_seconds(rev_decay) * 1.5 + 0.5) * fs) as u32;
        let tone_hz = map::tone_hz(dly_tone).min(0.45 * fs);
        self.dly_tone = 1.0 - (-core::f32::consts::TAU * tone_hz / fs).exp();
    }

    /// One stereo sample. `audible` is false when muted (or another track
    /// is soloed); the machine keeps running and the effect tails ring out.
    #[inline]
    pub fn tick(&mut self, audible: bool, beat_s: f32) -> (f32, f32) {
        if self.block_pos == 0 {
            self.control(beat_s);
        }
        self.block_pos = (self.block_pos + 1) % CONTROL_BLOCK;

        if self.is_active() {
            self.ramp = (self.ramp + self.up_inc).min(1.0);
        } else {
            self.ramp = (self.ramp - self.down_inc).max(0.0);
        }
        let d = self.drive();
        let target = if audible { self.gain_target } else { 0.0 };
        self.gain += (target - self.gain) * self.k_gain;

        let mut y = if d <= 0.0 && !self.is_active() && self.machines.is_quiet(self.machine) {
            0.0
        } else {
            self.machines
                .tick(self.machine, &self.ctx, d, &mut self.rng)
        };
        if !self.ladder_c.is_off() {
            y = self.ladder.process(y, &self.ladder_c);
        }
        let y = self.eq.process(y) * self.gain;

        let s = &self.smooth;
        let rev_send = y * map::send_gain(s[Param::RevMix.index()]);
        let dly_send = y * map::send_gain(s[Param::DlyMix.index()]);
        let (mut l, mut r) = (y * self.pan.0, y * self.pan.1);

        if rev_send.abs() > 1e-7 {
            self.rev_quiet = 0;
        } else {
            self.rev_quiet = self.rev_quiet.saturating_add(1);
        }
        if self.rev_quiet <= self.rev_idle_after {
            let (wl, wr) = self.plate.process(rev_send, &self.plate_ctl);
            let (wl, wr) = self.rev_eq.process(wl, wr);
            l += wl;
            r += wr;
        }
        let (dl, dr) =
            self.delay
                .process(dly_send, self.dly_seconds, self.dly_feedback, self.dly_tone);
        (l + dl, r + dr)
    }
}
