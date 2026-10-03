//! Panel state, the controller shared through the Dioxus context, and
//! persistence (localStorage on the web, `~/.config/ferrodrome` on desktop).
//!
//! MUTATE and VARIANCE run inside the audio engine, where the sequencer
//! starts the active periods. The engine reports each mutated patch back
//! and the panel adopts it ([`Synth::adopt`]), so the knobs move with every
//! period; VARIANCE is only shown (a ghost dot on each knob), never written
//! into the patch.
//!
//! Locks (Ctrl-click any control) live here and are sent to the engine,
//! which needs them for MUTATE and VARIANCE; RND runs here and reads them
//! directly.

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use ferrodrome_dsp::machines::{MACHINE_COUNT, spec};
use ferrodrome_dsp::mutate::{
    mutates, never_randomized, random_globals, random_patch, random_steps,
};
use ferrodrome_dsp::params::{
    ALL_GLOBAL_PARAMS, ALL_PARAMS, GLOBAL_PARAM_COUNT, GlobalPatch, MAX_STEPS, MAX_TRACKS,
    PARAM_COUNT, ParamKind, apply_machine_defaults, default_global_patch, default_patch,
};
use ferrodrome_dsp::util::Noise;
use ferrodrome_dsp::{GlobalParam, Param, Status, TrackPatch, seq};

use crate::audio::{AudioHandle, Command, Snapshot};

#[derive(Clone, Debug, PartialEq)]
pub struct PanelState {
    pub tracks: [TrackPatch; MAX_TRACKS],
    pub steps: [u128; MAX_TRACKS],
    pub locks: [[bool; PARAM_COUNT]; MAX_TRACKS],
    /// Locks the track's sequence (and length) against RND.
    pub pattern_locks: [bool; MAX_TRACKS],
    pub globals: GlobalPatch,
    pub global_locks: [bool; GLOBAL_PARAM_COUNT],
}

fn bits(steps: &[usize]) -> u128 {
    let mut s = 0;
    for &i in steps {
        seq::set(&mut s, i, true);
    }
    s
}

impl Default for PanelState {
    /// A small hall to start from.
    fn default() -> Self {
        use ferrodrome_dsp::machines::*;
        let mut tracks = [default_patch(0); MAX_TRACKS];
        let mut steps = [0u128; MAX_TRACKS];
        let hall: [(u32, usize, Vec<usize>, f32, f32); 6] = [
            (
                MACHINE_TRANSFORMER,
                64,
                (0..40).chain(48..60).collect(),
                0.5,
                0.55,
            ),
            (
                MACHINE_CONVEYOR,
                32,
                (0..12).chain(20..28).collect(),
                0.32,
                0.45,
            ),
            (MACHINE_PRESS, 16, vec![0, 6, 10], 0.5, 0.3),
            (MACHINE_PUMP, 16, vec![3, 4, 11, 12, 13], 0.7, 0.2),
            (MACHINE_SERVO, 12, vec![0, 1, 6, 7], 0.3, 0.15),
            (MACHINE_STEAM, 48, (36..44).collect(), 0.62, 0.5),
        ];
        for (t, (m, len, on, pan, rev)) in hall.into_iter().enumerate() {
            let mut p = default_patch(m);
            p[Param::Enabled.index()] = 1.0;
            p[Param::Length.index()] = len as f32;
            p[Param::Pan.index()] = pan;
            p[Param::RevMix.index()] = rev;
            tracks[t] = p;
            steps[t] = bits(&on);
        }
        tracks[0][Param::Variance.index()] = 0.3;
        tracks[2][Param::DlyMix.index()] = 0.45;
        tracks[2][Param::DlyTime.index()] = 7.0;
        Self {
            tracks,
            steps,
            locks: [[false; PARAM_COUNT]; MAX_TRACKS],
            pattern_locks: [false; MAX_TRACKS],
            globals: default_global_patch(),
            global_locks: [false; GLOBAL_PARAM_COUNT],
        }
    }
}

impl PanelState {
    pub fn global(&self, p: GlobalParam) -> f32 {
        self.globals[p.index()]
    }

    pub fn enabled(&self, t: usize) -> bool {
        self.tracks[t][Param::Enabled.index()] >= 0.5
    }

    pub fn machine(&self, t: usize) -> u32 {
        self.tracks[t][Param::Machine.index()] as u32
    }

    pub fn length(&self, t: usize) -> usize {
        (self.tracks[t][Param::Length.index()] as usize).clamp(1, MAX_STEPS)
    }

    /// Tracks in use, in order.
    pub fn track_list(&self) -> Vec<usize> {
        (0..MAX_TRACKS).filter(|&t| self.enabled(t)).collect()
    }

    /// Columns the sequencer shows: the longest track, rounded up to a bar.
    pub fn view_steps(&self) -> usize {
        let longest = self
            .track_list()
            .iter()
            .map(|&t| self.length(t))
            .max()
            .unwrap_or(16);
        longest.div_ceil(16).max(1) * 16
    }

    pub fn to_snapshot(&self) -> Snapshot {
        Snapshot {
            tracks: self.tracks,
            globals: self.globals,
            steps: self.steps,
            locks: self.locks,
        }
    }

    pub fn serialize(&self) -> String {
        let mut out = format!("{FORMAT_HEADER}\n");
        for p in ALL_GLOBAL_PARAMS {
            out += &format!("g {p:?} {}\n", self.globals[p.index()]);
            if self.global_locks[p.index()] {
                out += &format!("gl {p:?}\n");
            }
        }
        for t in 0..MAX_TRACKS {
            if !self.enabled(t) {
                continue;
            }
            for p in ALL_PARAMS {
                out += &format!("t {t} {p:?} {}\n", self.tracks[t][p.index()]);
                if self.locks[t][p.index()] {
                    out += &format!("l {t} {p:?}\n");
                }
            }
            out += &format!("s {t} {:x}\n", self.steps[t]);
            if self.pattern_locks[t] {
                out += &format!("pl {t}\n");
            }
        }
        out
    }

    /// Lenient: unknown lines are skipped and missing values keep defaults.
    pub fn deserialize(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        if lines.next()?.trim() != FORMAT_HEADER {
            return None;
        }
        let mut s = PanelState {
            tracks: [default_patch(0); MAX_TRACKS],
            steps: [0; MAX_TRACKS],
            ..Default::default()
        };
        let num = |v: &str| v.parse::<f32>().ok().filter(|v| v.is_finite());
        let track = |v: &str| v.parse::<usize>().ok().filter(|&t| t < MAX_TRACKS);
        let param = |name: &str| {
            ALL_PARAMS
                .iter()
                .copied()
                .find(|p| format!("{p:?}") == name)
        };
        let global = |name: &str| {
            ALL_GLOBAL_PARAMS
                .iter()
                .copied()
                .find(|p| format!("{p:?}") == name)
        };
        for line in lines {
            let parts: Vec<&str> = line.split_whitespace().collect();
            match parts.as_slice() {
                ["g", name, v] => {
                    if let (Some(p), Some(v)) = (global(name), num(v)) {
                        s.globals[p.index()] = p.sanitize(v);
                    }
                }
                ["gl", name] => {
                    if let Some(p) = global(name) {
                        s.global_locks[p.index()] = true;
                    }
                }
                ["t", t, name, v] => {
                    if let (Some(t), Some(p), Some(v)) = (track(t), param(name), num(v)) {
                        s.tracks[t][p.index()] = p.sanitize(v);
                    }
                }
                ["l", t, name] => {
                    if let (Some(t), Some(p)) = (track(t), param(name)) {
                        s.locks[t][p.index()] = true;
                    }
                }
                ["s", t, hex] => {
                    if let (Some(t), Ok(v)) = (track(t), u128::from_str_radix(hex, 16)) {
                        s.steps[t] = v;
                    }
                }
                ["pl", t] => {
                    if let Some(t) = track(t) {
                        s.pattern_locks[t] = true;
                    }
                }
                _ => {}
            }
        }
        Some(s)
    }
}

const FORMAT_HEADER: &str = "ferrodrome-state 1";

/// What a knob, button or drag gesture is adjusting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Param(usize, Param),
    Global(GlobalParam),
}

impl Target {
    pub fn is_bipolar(self) -> bool {
        match self {
            Target::Param(_, p) => p.is_bipolar(),
            Target::Global(p) => p.is_bipolar(),
        }
    }

    /// Whether Ctrl-click can lock it (everything RND or MUTATE can touch).
    pub fn lockable(self) -> bool {
        match self {
            Target::Param(_, p) => !never_randomized(p),
            Target::Global(p) => !ferrodrome_dsp::mutate::global_never_randomized(p),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub target: Target,
    pub start_y: f64,
    pub start_value: f32,
    /// Pixels of vertical travel for the full range.
    pub span_px: f64,
    pub fine: bool,
}

/// What the machine menu is open for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Menu {
    Add,
    Change(usize),
}

/// Track colours (also in the CSS as `--t0` … `--t7`).
pub const TRACK_COUNT_LABEL: [&str; MAX_TRACKS] = ["01", "02", "03", "04", "05", "06", "07", "08"];

/// Everything the controls need, shared via `use_context::<Synth>()`.
#[derive(Clone)]
pub struct Synth {
    pub state: Signal<PanelState>,
    pub audio: AudioHandle,
    pub drag: Signal<Option<Drag>>,
    /// Painting steps: the track and whether it draws or erases.
    pub paint: Signal<Option<(usize, bool)>>,
    pub menu: Signal<Option<Menu>>,
    /// Last touched control, for the display.
    pub touched: Signal<Option<Target>>,
    /// Whether the sequencer is running (as requested by the panel).
    pub playing: Signal<bool>,
    /// Tracks held running by RUN or the keyboard.
    pub held: Signal<[bool; MAX_TRACKS]>,
    /// The engine's mutation counters the panel has caught up with.
    seen_mutations: Rc<RefCell<[u32; MAX_TRACKS]>>,
    rng: Rc<RefCell<Noise>>,
}

impl Synth {
    pub fn new() -> Self {
        let initial = load().unwrap_or_default();
        Self {
            audio: AudioHandle::new(initial.to_snapshot()),
            state: Signal::new(initial),
            drag: Signal::new(None),
            paint: Signal::new(None),
            menu: Signal::new(None),
            touched: Signal::new(None),
            playing: Signal::new(false),
            held: Signal::new([false; MAX_TRACKS]),
            seen_mutations: Rc::new(RefCell::new([0; MAX_TRACKS])),
            rng: Rc::new(RefCell::new(Noise::new(random_seed()))),
        }
    }

    // --- knobs -------------------------------------------------------------

    fn kind(&self, target: Target) -> ParamKind {
        match target {
            Target::Param(t, p) => match p.steps_for(self.state.peek().machine(t)) {
                Some(names) => ParamKind::Choice(names),
                None => p.info().kind,
            },
            Target::Global(p) => p.info().kind,
        }
    }

    /// Knob position `0..=1` of a target.
    pub fn value(&self, target: Target) -> f32 {
        let s = self.state.read();
        match target {
            // Machine knobs are stored as knob positions, stepped or not.
            Target::Param(t, p) if p.knob_slot().is_some() => s.tracks[t][p.index()],
            Target::Param(t, p) => p.info().kind.to_knob(s.tracks[t][p.index()]),
            Target::Global(p) => p.info().kind.to_knob(s.globals[p.index()]),
        }
    }

    /// Knob position of a value in the target's own units.
    pub fn knob_of(&self, target: Target, v: f32) -> f32 {
        match target {
            Target::Param(_, p) if p.knob_slot().is_some() => v,
            Target::Param(_, p) => p.info().kind.to_knob(v),
            Target::Global(p) => p.info().kind.to_knob(v),
        }
    }

    /// Panel label; machine knobs take the machine's names.
    pub fn label(&self, target: Target) -> String {
        match target {
            Target::Param(t, p) => p.label_for(self.state.read().machine(t)).into(),
            Target::Global(p) => p.info().label.into(),
        }
    }

    pub fn name(&self, target: Target) -> String {
        match target {
            Target::Param(t, p) => {
                let s = self.state.read();
                format!(
                    "{} {} · {}",
                    TRACK_COUNT_LABEL[t],
                    spec(s.machine(t)).name,
                    p.name_for(s.machine(t))
                )
            }
            Target::Global(p) => p.info().name.into(),
        }
    }

    /// Read-out for a target at knob position `k`.
    pub fn display(&self, target: Target, k: f32) -> String {
        match target {
            Target::Param(t, p) if p.knob_slot().is_some() => {
                p.display_for(self.state.read().machine(t), k)
            }
            Target::Param(t, p) => p.display_for(
                self.state.read().machine(t),
                p.sanitize(p.info().kind.from_knob(k)),
            ),
            Target::Global(p) => p.display(p.sanitize(p.info().kind.from_knob(k))),
        }
    }

    /// Steps of a stepped control (for wheel nudges), if any.
    pub fn steps(&self, target: Target) -> Option<usize> {
        match self.kind(target) {
            ParamKind::Choice(names) => Some(names.len()),
            ParamKind::Int { min, max } => Some((max - min + 1) as usize),
            ParamKind::Continuous => None,
        }
    }

    /// Set a target from a knob position.
    pub fn set(&self, target: Target, k: f32) {
        let k = k.clamp(0.0, 1.0);
        match target {
            Target::Param(t, p) if p.knob_slot().is_some() => {
                // Stepped machine knobs snap to their positions.
                let v = match p.steps_for(self.state.peek().machine(t)) {
                    Some(names) => {
                        let n = (names.len() - 1) as f32;
                        (k * n).round() / n
                    }
                    None => k,
                };
                self.set_param(t, p, v)
            }
            Target::Param(t, p) => self.set_param(t, p, p.info().kind.from_knob(k)),
            Target::Global(p) => self.set_global(p, p.info().kind.from_knob(k)),
        }
        let mut touched = self.touched;
        if *touched.peek() != Some(target) {
            touched.set(Some(target));
        }
    }

    pub fn reset(&self, target: Target) {
        let k = match target {
            Target::Param(t, p) if p.knob_slot().is_some() => {
                spec(self.state.peek().machine(t)).knobs[p.knob_slot().unwrap()].default
            }
            Target::Param(t, p @ (Param::RampUp | Param::RampDown)) => {
                let r = spec(self.state.peek().machine(t)).ramp;
                p.info()
                    .kind
                    .to_knob(if p == Param::RampUp { r.0 } else { r.1 })
            }
            Target::Param(_, p) => p.info().kind.to_knob(p.default_value()),
            Target::Global(p) => p.info().kind.to_knob(p.default_value()),
        };
        self.set(target, k);
        self.save();
    }

    pub fn set_param(&self, track: usize, param: Param, value: f32) {
        let value = param.sanitize(value);
        let mut state = self.state;
        if state.peek().tracks[track][param.index()] != value {
            state.write().tracks[track][param.index()] = value;
            self.audio.send(Command::Param {
                track,
                param,
                value,
            });
        }
    }

    pub fn set_global(&self, param: GlobalParam, value: f32) {
        let value = param.sanitize(value);
        let mut state = self.state;
        if state.peek().globals[param.index()] != value {
            state.write().globals[param.index()] = value;
            self.audio.send(Command::Global { param, value });
        }
    }

    /// Follow the engine's MUTATE: adopt each newly mutated patch (only the
    /// controls MUTATE changes, so a knob being turned isn't yanked back).
    pub fn adopt(&self, status: &Status) {
        let mut seen = self.seen_mutations.borrow_mut();
        for t in 0..MAX_TRACKS {
            if status.mutations[t] == seen[t] {
                continue;
            }
            seen[t] = status.mutations[t];
            let mut state = self.state;
            let current = state.peek().tracks[t];
            let mut patch = current;
            for p in ALL_PARAMS {
                if mutates(p) {
                    patch[p.index()] = status.patches[t][p.index()];
                }
            }
            if patch != current {
                state.write().tracks[t] = patch;
            }
        }
    }

    // --- locks -------------------------------------------------------------

    pub fn is_locked(&self, target: Target) -> bool {
        let s = self.state.read();
        match target {
            Target::Param(t, p) => s.locks[t][p.index()],
            Target::Global(p) => s.global_locks[p.index()],
        }
    }

    /// Ctrl-click: lock or unlock a control against RND, MUTATE and VARIANCE.
    pub fn toggle_lock(&self, target: Target) {
        if !target.lockable() {
            return;
        }
        let locked = !self.is_locked(target);
        let mut state = self.state;
        match target {
            Target::Param(track, param) => {
                state.write().locks[track][param.index()] = locked;
                self.audio.send(Command::Lock {
                    track,
                    param,
                    locked,
                });
            }
            Target::Global(p) => state.write().global_locks[p.index()] = locked,
        }
        let mut touched = self.touched;
        touched.set(Some(target));
        self.save();
    }

    pub fn toggle_pattern_lock(&self, track: usize) {
        let mut state = self.state;
        let locked = !state.peek().pattern_locks[track];
        state.write().pattern_locks[track] = locked;
        self.save();
    }

    // --- steps -------------------------------------------------------------

    pub fn set_step(&self, track: usize, step: usize, on: bool) {
        let mut state = self.state;
        if seq::get(state.peek().steps[track], step) != on {
            seq::set(&mut state.write().steps[track], step, on);
            self.audio.send(Command::Step { track, step, on });
        }
    }

    /// Replace a track's whole sequence, sending only the steps that changed.
    pub fn set_steps(&self, track: usize, steps: u128) {
        for s in 0..MAX_STEPS {
            self.set_step(track, s, seq::get(steps, s));
        }
    }

    pub fn clear_steps(&self, track: usize) {
        self.set_steps(track, 0);
        self.save();
    }

    pub fn rotate_steps(&self, track: usize, by: i32) {
        let s = self.state.peek();
        let next = seq::rotate(s.steps[track], s.length(track), by);
        drop(s);
        self.set_steps(track, next);
        self.save();
    }

    /// Fill a track with a fresh random sequence of its length.
    pub fn random_steps(&self, track: usize) {
        let len = self.state.peek().length(track);
        let steps = random_steps(len, &mut self.rng.borrow_mut());
        self.set_steps(track, steps);
        self.save();
    }

    // --- tracks ------------------------------------------------------------

    /// Load a whole patch, sending only the values that changed.
    fn load_track(&self, track: usize, patch: &TrackPatch) {
        for p in ALL_PARAMS {
            self.set_param(track, p, patch[p.index()]);
        }
    }

    /// Add a machine on the first free track. Returns the track.
    pub fn add_track(&self, machine: u32) -> Option<usize> {
        let free = (0..MAX_TRACKS).find(|&t| !self.state.peek().enabled(t))?;
        let mut p = default_patch(machine);
        p[Param::Enabled.index()] = 1.0;
        // Give it something to do: the first bar's first half.
        self.load_track(free, &p);
        self.set_steps(free, bits(&(0..8).collect::<Vec<_>>()));
        self.save();
        Some(free)
    }

    pub fn remove_track(&self, track: usize) {
        self.hold(track, false);
        let mut p = default_patch(0);
        p[Param::Enabled.index()] = 0.0;
        self.load_track(track, &p);
        self.set_steps(track, 0);
        for param in ALL_PARAMS {
            if self.state.peek().locks[track][param.index()] {
                self.toggle_lock(Target::Param(track, param));
            }
        }
        let mut state = self.state;
        state.write().pattern_locks[track] = false;
        self.save();
    }

    /// Switch a track's machine, loading its default knobs and ramps.
    pub fn set_machine(&self, track: usize, machine: u32) {
        let mut p = self.state.peek().tracks[track];
        p[Param::Machine.index()] = machine as f32;
        apply_machine_defaults(&mut p, machine);
        self.load_track(track, &p);
        self.save();
    }

    /// RND on one track: its sound, and its sequence unless that is locked.
    pub fn randomize_track(&self, track: usize, with_steps: bool) {
        let s = self.state.peek().clone();
        let mut rng = self.rng.borrow_mut();
        let mut locks = s.locks[track];
        // The sequence lock also covers the length.
        if s.pattern_locks[track] {
            locks[Param::Length.index()] = true;
        }
        let patch = random_patch(&s.tracks[track], &locks, &mut rng);
        drop(rng);
        self.load_track(track, &patch);
        if with_steps && !s.pattern_locks[track] {
            let len = self.state.peek().length(track);
            let steps = machine_steps(
                patch[Param::Machine.index()] as u32,
                len,
                &mut self.rng.borrow_mut(),
            );
            self.set_steps(track, steps);
        }
        self.save();
    }

    /// RND: everything that is not locked. An empty hall gets machines.
    pub fn randomize_all(&self) {
        if self.state.peek().track_list().is_empty() {
            let n = 4 + self.rng.borrow_mut().below(3);
            for _ in 0..n {
                let m = self.rng.borrow_mut().below(MACHINE_COUNT) as u32;
                self.add_track(m);
            }
        }
        let tracks = self.state.peek().track_list();
        for t in tracks {
            self.randomize_track(t, true);
        }
        let s = self.state.peek().clone();
        let g = random_globals(&s.globals, &s.global_locks, &mut self.rng.borrow_mut());
        for p in ALL_GLOBAL_PARAMS {
            self.set_global(p, g[p.index()]);
        }
        self.save();
    }

    // --- playing -------------------------------------------------------------

    pub fn toggle_play(&self) {
        self.audio.user_gesture();
        let mut playing = self.playing;
        let on = !*playing.peek();
        playing.set(on);
        self.audio.send(Command::Play(on));
        // MUTATE may have been rewriting the tracks: keep what they became.
        if !on {
            self.save();
        }
    }

    /// Hold a machine running (or let it go).
    pub fn hold(&self, track: usize, on: bool) {
        self.audio.user_gesture();
        let mut held = self.held;
        if held.peek()[track] != on {
            held.write()[track] = on;
            self.audio.send(Command::Hold { track, on });
        }
    }

    pub fn close_menu(&self) {
        let mut m = self.menu;
        if m.peek().is_some() {
            m.set(None);
        }
    }

    pub fn save(&self) {
        save(&self.state.peek());
    }
}

/// A random sequence that suits the machine: the rotating machines get long
/// runs (they need time to spin up), the stroke machines short ones.
fn machine_steps(machine: u32, len: usize, rng: &mut Noise) -> u128 {
    use ferrodrome_dsp::machines::*;
    let long = matches!(
        machine,
        MACHINE_CONVEYOR
            | MACHINE_CRANKSHAFT
            | MACHINE_TURBINE
            | MACHINE_GRINDER
            | MACHINE_TRANSFORMER
    );
    // Draw a few and keep the one whose mean period length suits best.
    let want: f32 = if long { 8.0 } else { 2.0 };
    let mut best = random_steps(len, rng);
    let mut best_err = f32::MAX;
    for _ in 0..6 {
        let s = random_steps(len, rng);
        let periods = seq::periods(s, len);
        let mean = periods.iter().map(|p| p.1).sum::<usize>() as f32 / periods.len().max(1) as f32;
        let err = (mean.min(len as f32) - want.min(len as f32)).abs();
        if err < best_err {
            best = s;
            best_err = err;
        }
    }
    best
}

#[cfg(all(feature = "web", not(feature = "desktop")))]
fn random_seed() -> u32 {
    (js_sys::Math::random() * u32::MAX as f64) as u32
}

#[cfg(not(all(feature = "web", not(feature = "desktop"))))]
fn random_seed() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() ^ d.as_secs() as u32)
        .unwrap_or(12345)
}

// --- persistence -------------------------------------------------------------

#[cfg(all(feature = "web", not(feature = "desktop")))]
const STORAGE_KEY: &str = "ferrodrome.state";

#[cfg(all(feature = "web", not(feature = "desktop")))]
fn load() -> Option<PanelState> {
    let storage = web_sys::window()?.local_storage().ok()??;
    PanelState::deserialize(&storage.get_item(STORAGE_KEY).ok()??)
}

#[cfg(all(feature = "web", not(feature = "desktop")))]
fn save(state: &PanelState) {
    if let Some(Ok(Some(storage))) = web_sys::window().map(|w| w.local_storage()) {
        let _ = storage.set_item(STORAGE_KEY, &state.serialize());
    }
}

#[cfg(not(all(feature = "web", not(feature = "desktop"))))]
fn state_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
        })?;
    Some(base.join("ferrodrome").join("state.txt"))
}

#[cfg(not(all(feature = "web", not(feature = "desktop"))))]
fn load() -> Option<PanelState> {
    PanelState::deserialize(&std::fs::read_to_string(state_path()?).ok()?)
}

#[cfg(not(all(feature = "web", not(feature = "desktop"))))]
fn save(state: &PanelState) {
    let Some(path) = state_path() else { return };
    let result = path
        .parent()
        .map(std::fs::create_dir_all)
        .unwrap_or(Ok(()))
        .and_then(|_| std::fs::write(&path, state.serialize()));
    if let Err(e) = result {
        eprintln!(
            "ferrodrome: could not save state to {}: {e}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hall_has_machines_and_steps() {
        let s = PanelState::default();
        assert_eq!(s.track_list().len(), 6);
        assert!(s.track_list().iter().all(|&t| s.steps[t] != 0));
        assert_eq!(s.view_steps(), 64);
    }

    #[test]
    fn state_roundtrip() {
        let mut s = PanelState::default();
        s.globals[GlobalParam::Tempo.index()] = 133.0;
        s.global_locks[GlobalParam::RevMix.index()] = true;
        s.tracks[1][Param::K3.index()] = 0.123;
        s.locks[1][Param::K3.index()] = true;
        s.pattern_locks[2] = true;
        s.steps[5] = u128::MAX - 7;
        s.tracks[5][Param::Length.index()] = 128.0;
        let back = PanelState::deserialize(&s.serialize()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn rejects_foreign_text_and_tolerates_garbage() {
        assert!(PanelState::deserialize("hello").is_none());
        let text = format!(
            "{FORMAT_HEADER}\ng Tempo 150\nt 0 Nope 1\nt 9 K1 0.2\ns 0 zz\nt 1 Enabled 1\nt 1 Mutate 2\nl 1 K2\n"
        );
        let s = PanelState::deserialize(&text).unwrap();
        assert_eq!(s.global(GlobalParam::Tempo), 150.0);
        assert_eq!(s.track_list(), vec![1]);
        assert_eq!(s.tracks[1][Param::Mutate.index()], 1.0);
        assert!(s.locks[1][Param::K2.index()]);
    }

    #[test]
    fn snapshot_replays_everything() {
        let mut s = PanelState::default();
        s.locks[0][Param::K1.index()] = true;
        let snap = s.to_snapshot();
        let mut rebuilt = Snapshot {
            tracks: [default_patch(0); MAX_TRACKS],
            globals: default_global_patch(),
            steps: [0; MAX_TRACKS],
            locks: [[false; PARAM_COUNT]; MAX_TRACKS],
        };
        for cmd in snap.commands() {
            rebuilt.apply(cmd);
        }
        assert_eq!(rebuilt.tracks, s.tracks);
        assert_eq!(rebuilt.globals, s.globals);
        assert_eq!(rebuilt.steps, s.steps);
        assert_eq!(rebuilt.locks, s.locks);
    }

    /// The JS processor must read exactly as many status floats as the engine writes.
    #[test]
    fn worklet_status_length_matches_the_engine() {
        let js = include_str!("audio/worklet.js");
        assert!(js.contains(&format!("const STATUS_LEN = {};", Status::WIRE_LEN)));
    }

    #[test]
    fn machine_steps_suit_the_machine() {
        let mut rng = Noise::new(4);
        let mean = |m: u32, rng: &mut Noise| {
            let mut total = 0.0;
            for _ in 0..50 {
                let s = machine_steps(m, 32, rng);
                let p = seq::periods(s, 32);
                total += p.iter().map(|p| p.1).sum::<usize>() as f32 / p.len().max(1) as f32;
            }
            total / 50.0
        };
        assert!(mean(6, &mut rng) > mean(5, &mut rng));
    }
}
