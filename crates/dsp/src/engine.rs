//! The hall: eight tracks, the step sequencer, the master delay and reverb
//! and the output stage.
//!
//! ```text
//!  sequencer ─► gates ─┐
//!  RUN (held) ─────────┴─► TRACK × 8 ─(+)─► MASTER DELAY ─► MASTER REVERB ─► EQ'd return ─► level ─► soft clip ─► out
//! ```
//!
//! The sequencer runs in 16ths. Every track loops its own length (1–128
//! steps) against one global step counter, so different lengths drift
//! against each other (polymeter) and a run of steps over a track's loop
//! point is one continuous active period. Gates change on their exact
//! sample.

use crate::delay::PingPong;
use crate::params::{
    GLOBAL_PARAM_COUNT, GlobalParam, GlobalPatch, MAX_TRACKS, PARAM_COUNT, Param, STEPS_PER_BEAT,
    TrackPatch, default_global_patch, default_patch, map,
};
use crate::reverb::Plate;
use crate::track::{ReturnEq, Track};
use crate::util::{DcBlocker, fast_tanh, one_pole_coeff};

/// How often the drive meters and VARIANCE read-outs are reported.
pub const METER_HZ: f32 = 20.0;

/// What the panel shows: where the sequencer is, which machines are running,
/// what MUTATE turned each track into and where VARIANCE has taken it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Status {
    /// Global step counter since PLAY, or -1 when stopped. Track `t` is at
    /// step `step % length(t)`.
    pub step: i32,
    pub playing: bool,
    /// Spin-up/down position per track, 0..1.
    pub drive: [f32; MAX_TRACKS],
    /// Whether each track's gate (or RUN) is open.
    pub active: [bool; MAX_TRACKS],
    /// Mutations per track since start; `patches` is new whenever this changes.
    pub mutations: [u32; MAX_TRACKS],
    /// Each track's patch after its latest mutation.
    pub patches: [TrackPatch; MAX_TRACKS],
    /// Each track's patch after VARIANCE.
    pub live: [TrackPatch; MAX_TRACKS],
}

impl Status {
    /// `[step, playing, drive×8, active×8, mutations×8, patches×8, live×8]`.
    pub const WIRE_LEN: usize = 2 + 3 * MAX_TRACKS + 2 * MAX_TRACKS * PARAM_COUNT;

    pub fn to_wire(&self) -> [f32; Self::WIRE_LEN] {
        let mut w = [0.0; Self::WIRE_LEN];
        w[0] = self.step as f32;
        w[1] = self.playing as u8 as f32;
        let t = MAX_TRACKS;
        for i in 0..t {
            w[2 + i] = self.drive[i];
            w[2 + t + i] = self.active[i] as u8 as f32;
            w[2 + 2 * t + i] = self.mutations[i] as f32;
            let p = 2 + 3 * t + i * PARAM_COUNT;
            w[p..p + PARAM_COUNT].copy_from_slice(&self.patches[i]);
            let l = 2 + 3 * t + (t + i) * PARAM_COUNT;
            w[l..l + PARAM_COUNT].copy_from_slice(&self.live[i]);
        }
        w
    }

    pub fn from_wire(w: &[f32]) -> Option<Self> {
        if w.len() < Self::WIRE_LEN {
            return None;
        }
        let t = MAX_TRACKS;
        let patch = |at: usize| {
            let mut p = [0.0; PARAM_COUNT];
            p.copy_from_slice(&w[at..at + PARAM_COUNT]);
            p
        };
        Some(Self {
            step: w[0] as i32,
            playing: w[1] > 0.5,
            drive: core::array::from_fn(|i| w[2 + i]),
            active: core::array::from_fn(|i| w[2 + t + i] > 0.5),
            mutations: core::array::from_fn(|i| w[2 + 2 * t + i] as u32),
            patches: core::array::from_fn(|i| patch(2 + 3 * t + i * PARAM_COUNT)),
            live: core::array::from_fn(|i| patch(2 + 3 * t + (t + i) * PARAM_COUNT)),
        })
    }
}

impl Default for Status {
    fn default() -> Self {
        Self {
            step: -1,
            playing: false,
            drive: [0.0; MAX_TRACKS],
            active: [false; MAX_TRACKS],
            mutations: [0; MAX_TRACKS],
            patches: [default_patch(0); MAX_TRACKS],
            live: [default_patch(0); MAX_TRACKS],
        }
    }
}

#[derive(Clone, Debug)]
pub struct Engine {
    fs: f32,
    tracks: Vec<Track>,
    globals: GlobalPatch,
    /// Smoothed master controls.
    smooth: GlobalPatch,
    k_smooth: f32,
    plate: Plate,
    eq: ReturnEq,
    delay: PingPong,
    playing: bool,
    /// Steps since PLAY.
    step_count: u64,
    next_step_at: f64,
    time: u64,
    meter_left: u32,
    status: Status,
    dc: [DcBlocker; 2],
    dc_r: f32,
}

impl Engine {
    pub fn new(sample_rate: f32) -> Self {
        let g = default_global_patch();
        let mut e = Self {
            fs: sample_rate,
            tracks: (0..MAX_TRACKS)
                .map(|i| Track::new(sample_rate, i as u32 + 1))
                .collect(),
            globals: g,
            smooth: g,
            k_smooth: one_pole_coeff(0.02, sample_rate),
            plate: Plate::new(sample_rate),
            eq: ReturnEq::new(),
            delay: PingPong::new(sample_rate),
            playing: false,
            step_count: 0,
            next_step_at: 0.0,
            time: 0,
            meter_left: 0,
            status: Status::default(),
            dc: [DcBlocker::default(), DcBlocker::default()],
            dc_r: 1.0 - core::f32::consts::TAU * 8.0 / sample_rate,
        };
        e.update_eq();
        e
    }

    pub fn sample_rate(&self) -> f32 {
        self.fs
    }

    pub fn set_param(&mut self, track: usize, p: Param, value: f32) {
        if let Some(t) = self.tracks.get_mut(track) {
            t.set_param(p, value);
            self.status.patches[track] = t.patch();
        }
    }

    pub fn param(&self, track: usize, p: Param) -> f32 {
        self.tracks[track].param(p)
    }

    pub fn load_patch(&mut self, track: usize, patch: &TrackPatch) {
        for p in crate::params::ALL_PARAMS {
            self.set_param(track, p, patch[p.index()]);
        }
    }

    pub fn set_lock(&mut self, track: usize, p: Param, locked: bool) {
        if let Some(t) = self.tracks.get_mut(track) {
            t.set_lock(p, locked);
        }
    }

    pub fn set_step(&mut self, track: usize, step: usize, on: bool) {
        if let Some(t) = self.tracks.get_mut(track) {
            t.set_step(step, on);
        }
    }

    pub fn set_steps(&mut self, track: usize, steps: u128) {
        for s in 0..crate::params::MAX_STEPS {
            self.set_step(track, s, crate::steps::get(steps, s));
        }
    }

    pub fn steps(&self, track: usize) -> u128 {
        self.tracks[track].steps()
    }

    pub fn set_global(&mut self, p: GlobalParam, value: f32) {
        self.globals[p.index()] = p.sanitize(value);
        if matches!(p, GlobalParam::Tempo | GlobalParam::DlyTime) {
            self.smooth[p.index()] = self.globals[p.index()];
        }
    }

    pub fn global(&self, p: GlobalParam) -> f32 {
        self.globals[p.index()]
    }

    /// Hold a track running (or let it go), independent of the sequencer.
    pub fn hold(&mut self, track: usize, on: bool) {
        if let Some(t) = self.tracks.get_mut(track) {
            t.set_hold(on);
        }
    }

    pub fn play(&mut self, on: bool) {
        if on == self.playing {
            return;
        }
        self.playing = on;
        if on {
            self.step_count = 0;
            self.next_step_at = self.time as f64;
        } else {
            for t in self.tracks.iter_mut() {
                t.set_gate(false);
            }
            self.status.step = -1;
        }
        self.status.playing = on;
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Jump smoothed parameters straight to their targets.
    pub fn snap_params(&mut self) {
        for t in self.tracks.iter_mut() {
            t.snap();
        }
        self.smooth = self.globals;
        self.update_eq();
    }

    pub fn status(&self) -> Status {
        self.status
    }

    fn beat_s(&self) -> f32 {
        map::seconds_per_beat(self.globals[GlobalParam::Tempo.index()])
    }

    fn update_eq(&mut self) {
        let g = |p: GlobalParam| self.smooth[p.index()];
        self.eq.update(
            [
                g(GlobalParam::RevLow),
                g(GlobalParam::RevMid),
                g(GlobalParam::RevHigh),
            ],
            self.fs,
        );
    }

    fn step(&mut self) {
        let n = self.step_count;
        for t in self.tracks.iter_mut() {
            if !t.enabled() {
                t.set_gate(false);
                continue;
            }
            let len = t.length().max(1) as u64;
            let s = (n % len) as usize;
            t.set_gate(crate::steps::get(t.steps(), s));
        }
        self.status.step = (n % (1 << 24)) as i32;
        self.step_count += 1;
        let step_s = self.beat_s() as f64 / STEPS_PER_BEAT as f64;
        self.next_step_at += step_s * self.fs as f64;
    }

    fn report(&mut self) {
        for (i, t) in self.tracks.iter().enumerate() {
            self.status.drive[i] = (t.drive() * 1000.0).round() / 1000.0;
            self.status.active[i] = t.is_active();
            self.status.live[i] = t.live();
        }
    }

    /// Render stereo audio. `left` and `right` must be the same length.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        debug_assert_eq!(left.len(), right.len());
        let n = left.len();

        // Control-rate master smoothing, once per block.
        let k = 1.0 - (1.0 - self.k_smooth).powi(n as i32);
        for i in 0..GLOBAL_PARAM_COUNT {
            self.smooth[i] += (self.globals[i] - self.smooth[i]) * k;
        }
        self.update_eq();
        let g = |p: GlobalParam| self.smooth[p.index()];
        let beat_s = self.beat_s();
        let plate_ctl = self.plate.controls(
            g(GlobalParam::RevDecay),
            g(GlobalParam::RevTone),
            g(GlobalParam::RevPredelay),
        );
        let rev_send = map::send_gain(g(GlobalParam::RevMix));
        let dly_send = map::send_gain(g(GlobalParam::DlyMix));
        let dly_s = map::delay_beats(g(GlobalParam::DlyTime)) * beat_s;
        let dly_fb = map::feedback(g(GlobalParam::DlyFeedback));
        let tone_hz = map::tone_hz(g(GlobalParam::DlyTone)).min(0.45 * self.fs);
        let dly_tone = 1.0 - (-core::f32::consts::TAU * tone_hz / self.fs).exp();
        let master = map::level_gain(g(GlobalParam::Master)) * 1.2;

        // Solo: if any track is soloed, only soloed tracks are heard.
        let any_solo = self
            .tracks
            .iter()
            .any(|t| t.enabled() && t.param(Param::Solo) >= 0.5);
        let audible: [bool; MAX_TRACKS] = core::array::from_fn(|i| {
            let t = &self.tracks[i];
            t.param(Param::Mute) < 0.5 && (!any_solo || t.param(Param::Solo) >= 0.5)
        });

        let meter_every = (self.fs / METER_HZ) as u32;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if self.playing && self.time as f64 >= self.next_step_at {
                self.step();
            }
            let (mut yl, mut yr) = (0.0, 0.0);
            for (i, t) in self.tracks.iter_mut().enumerate() {
                if !t.enabled() {
                    continue;
                }
                let (a, b) = t.tick(audible[i], beat_s);
                yl += a;
                yr += b;
            }
            let mono = 0.5 * (yl + yr);
            let (dl, dr) = self.delay.process(mono * dly_send, dly_s, dly_fb, dly_tone);
            yl += dl;
            yr += dr;
            let (wl, wr) = self.plate.process(0.5 * (yl + yr) * rev_send, &plate_ctl);
            let (wl, wr) = self.eq.process(wl, wr);
            yl += wl;
            yr += wr;
            *l = soft_clip(self.dc[0].process(yl * master, self.dc_r));
            *r = soft_clip(self.dc[1].process(yr * master, self.dc_r));
            self.time += 1;

            if self.meter_left == 0 {
                self.meter_left = meter_every;
                self.report();
            }
            self.meter_left -= 1;
        }
        for (i, t) in self.tracks.iter().enumerate() {
            self.status.mutations[i] = t.mutations();
            if self.status.mutations[i] != 0 {
                self.status.patches[i] = t.patch();
            }
        }
    }
}

/// Transparent below 0.7, then a tanh knee into ±1.
#[inline]
fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.7 {
        x
    } else {
        x.signum() * (0.7 + 0.3 * fast_tanh((a - 0.7) / 0.3))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machines::{MACHINE_COUNT, MACHINE_PRESS};

    fn hall(machine: u32) -> Engine {
        let mut e = Engine::new(48_000.0);
        e.load_patch(0, &default_patch(machine));
        e.set_param(0, Param::Enabled, 1.0);
        e.snap_params();
        e
    }

    fn run(e: &mut Engine, seconds: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (seconds * e.sample_rate()) as usize;
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            e.render(a, b);
        }
        (l, r)
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    #[test]
    fn wire_roundtrip() {
        let mut e = hall(2);
        e.set_step(0, 0, true);
        e.play(true);
        run(&mut e, 0.3);
        let s = e.status();
        assert_eq!(Status::from_wire(&s.to_wire()), Some(s));
    }

    #[test]
    fn silent_until_a_step_is_drawn_then_runs() {
        for m in 0..MACHINE_COUNT as u32 {
            let mut e = hall(m);
            e.play(true);
            let (l, _) = run(&mut e, 1.0);
            assert!(rms(&l) < 1e-4, "machine {m} sounds with no steps");
            for s in 0..16 {
                e.set_step(0, s, true);
            }
            let (l, r) = run(&mut e, 3.0);
            let late = &l[l.len() / 2..];
            assert!(rms(late) > 0.005, "machine {m}: {}", rms(late));
            assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() <= 1.0));
        }
    }

    #[test]
    fn a_run_over_the_loop_point_is_one_period() {
        // Steps 14, 15, 0, 1 of 16 on, with MUTATE: one mutation per loop,
        // not two.
        let mut e = hall(0);
        for s in [14, 15, 0, 1] {
            e.set_step(0, s, true);
        }
        e.set_param(0, Param::Mutate, 0.2);
        e.set_param(0, Param::Length, 16.0);
        e.set_global(GlobalParam::Tempo, 120.0);
        e.play(true);
        // 120 BPM: 16 steps = 2 s. Run 4 loops plus a bit.
        run(&mut e, 8.2);
        // Steps 0–1 open the first period (a fresh start), then each loop's
        // 14–15–0–1 is one period: 1 + 4 (loops 1..4 begin at 14 in loops 0..3).
        assert_eq!(e.status().mutations[0], 5);
    }

    #[test]
    fn polymeter_tracks_loop_their_own_length() {
        let mut e = Engine::new(8_000.0);
        e.set_global(GlobalParam::Tempo, 120.0);
        for t in 0..2 {
            e.set_param(t, Param::Enabled, 1.0);
            e.set_param(t, Param::Machine, MACHINE_PRESS as f32);
            e.set_param(t, Param::Mutate, 0.1);
            e.set_step(t, 0, true);
        }
        e.set_param(0, Param::Length, 4.0);
        e.set_param(1, Param::Length, 3.0);
        e.play(true);
        // 12 steps at 8 steps/s: track 0 starts 3 periods, track 1 four.
        let n = (12.0 / 8.0 * 8_000.0) as usize - 10;
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        e.render(&mut l, &mut r);
        assert_eq!(e.status().mutations[0], 3);
        assert_eq!(e.status().mutations[1], 4);
    }

    #[test]
    fn ramp_down_after_stop() {
        let mut e = hall(6);
        for s in 0..16 {
            e.set_step(0, s, true);
        }
        e.play(true);
        run(&mut e, 4.0);
        assert!(e.status().drive[0] > 0.9);
        e.play(false);
        let (l, _) = run(&mut e, 0.5);
        assert!(rms(&l) > 1e-3, "turbine should spin down, not stop dead");
        run(&mut e, 12.0);
        assert!(e.status().drive[0] < 1e-3);
    }

    #[test]
    fn mute_and_solo() {
        let mut e = Engine::new(48_000.0);
        for t in 0..2 {
            e.set_param(t, Param::Enabled, 1.0);
            e.set_param(t, Param::Machine, 3.0);
            e.set_param(t, Param::RevMix, 0.0);
            e.hold(t, true);
        }
        e.set_global(GlobalParam::RevMix, 0.0);
        run(&mut e, 1.0);
        let (both, _) = run(&mut e, 0.5);
        e.set_param(1, Param::Solo, 1.0);
        run(&mut e, 0.2);
        let (solo, _) = run(&mut e, 0.5);
        e.set_param(1, Param::Mute, 1.0);
        run(&mut e, 0.2);
        let (none, _) = run(&mut e, 0.5);
        assert!(rms(&solo) < rms(&both) * 0.9);
        assert!(rms(&none) < 1e-4, "{}", rms(&none));
    }

    #[test]
    fn variance_wanders_but_stays_near_the_setting() {
        let mut e = hall(0);
        e.set_param(0, Param::Variance, 1.0);
        e.set_lock(0, Param::K2, true);
        e.hold(0, true);
        let base = e.status().patches[0];
        let mut moved = 0.0f32;
        for _ in 0..60 {
            run(&mut e, 0.5);
            let live = e.status().live[0];
            let k1 = Param::K1.index();
            moved = moved.max((live[k1] - base[k1]).abs());
            assert!((live[k1] - base[k1]).abs() <= 0.281);
            assert_eq!(live[Param::K2.index()], base[Param::K2.index()], "locked");
            assert_eq!(
                e.status().patches[0],
                base,
                "VARIANCE never changes the patch"
            );
        }
        assert!(moved > 0.03, "{moved}");
    }
}
