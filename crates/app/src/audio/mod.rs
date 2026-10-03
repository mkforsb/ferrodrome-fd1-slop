//! Platform audio backends behind one small handle.
//!
//! * `web`: an AudioWorklet running the DSP compiled to a standalone wasm module.
//! * `desktop`: a PulseAudio playback stream fed from a dedicated thread.
//!
//! The UI only ever talks to [`AudioHandle`]; it never blocks and never touches
//! the engine directly. The engine reports back where the sequencer is,
//! which machines run and what MUTATE and VARIANCE are doing ([`Status`])
//! through a channel; [`forward`] splits that into small signals so each
//! part of the panel only redraws for what it shows.

// Without a platform feature only the null backend exists.
#![cfg_attr(not(any(feature = "web", feature = "desktop")), allow(dead_code))]

use dioxus::prelude::*;
use ferrodrome_dsp::params::{
    ALL_GLOBAL_PARAMS, ALL_PARAMS, GlobalPatch, MAX_STEPS, MAX_TRACKS, PARAM_COUNT,
};
use ferrodrome_dsp::{GlobalParam, Param, Status, TrackPatch, seq};
use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures_util::StreamExt;

#[cfg(feature = "desktop")]
mod pulse;
#[cfg(all(feature = "web", not(feature = "desktop")))]
mod web;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Param {
        track: usize,
        param: Param,
        value: f32,
    },
    Global {
        param: GlobalParam,
        value: f32,
    },
    Step {
        track: usize,
        step: usize,
        on: bool,
    },
    Lock {
        track: usize,
        param: Param,
        locked: bool,
    },
    /// Hold a machine running (RUN), independent of the sequencer.
    Hold {
        track: usize,
        on: bool,
    },
    Play(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub enum AudioStatus {
    /// Web only: waiting for a user gesture before the AudioContext may start.
    #[cfg_attr(feature = "desktop", allow(dead_code))]
    NeedsGesture,
    Starting,
    Running {
        sample_rate: u32,
        detail: String,
    },
    Failed(String),
}

/// Everything sent so far, so a backend that comes up late (the web one,
/// after a user gesture) can be brought in sync in one go.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub tracks: [TrackPatch; MAX_TRACKS],
    pub globals: GlobalPatch,
    pub steps: [u128; MAX_TRACKS],
    pub locks: [[bool; PARAM_COUNT]; MAX_TRACKS],
}

impl Snapshot {
    #[cfg_attr(feature = "desktop", allow(dead_code))]
    pub fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::Param {
                track,
                param,
                value,
            } => {
                if let Some(t) = self.tracks.get_mut(track) {
                    t[param.index()] = value;
                }
            }
            Command::Global { param, value } => self.globals[param.index()] = value,
            Command::Step { track, step, on } => {
                if let Some(s) = self.steps.get_mut(track) {
                    seq::set(s, step, on);
                }
            }
            Command::Lock {
                track,
                param,
                locked,
            } => {
                if let Some(l) = self.locks.get_mut(track) {
                    l[param.index()] = locked;
                }
            }
            Command::Hold { .. } | Command::Play(_) => {}
        }
    }

    /// The commands that recreate this state in a fresh engine.
    pub fn commands(&self) -> impl Iterator<Item = Command> + '_ {
        let params = (0..MAX_TRACKS).flat_map(move |track| {
            ALL_PARAMS.iter().map(move |&param| Command::Param {
                track,
                param,
                value: self.tracks[track][param.index()],
            })
        });
        let locks = (0..MAX_TRACKS).flat_map(move |track| {
            ALL_PARAMS
                .iter()
                .filter(move |p| self.locks[track][p.index()])
                .map(move |&param| Command::Lock {
                    track,
                    param,
                    locked: true,
                })
        });
        let globals = ALL_GLOBAL_PARAMS.iter().map(|&param| Command::Global {
            param,
            value: self.globals[param.index()],
        });
        let steps = (0..MAX_TRACKS).flat_map(move |track| {
            (0..MAX_STEPS)
                .filter(move |&s| seq::get(self.steps[track], s))
                .map(move |step| Command::Step {
                    track,
                    step,
                    on: true,
                })
        });
        params.chain(locks).chain(globals).chain(steps)
    }
}

#[cfg(feature = "desktop")]
type Backend = pulse::PulseBackend;
#[cfg(all(feature = "web", not(feature = "desktop")))]
type Backend = web::WebBackend;
#[cfg(not(any(feature = "web", feature = "desktop")))]
type Backend = NullBackend;

/// What the engine reports, split up so components subscribe to only what
/// they draw.
#[derive(Clone, Copy)]
pub struct Meters {
    /// The whole status (for following MUTATE).
    pub status: Signal<Status>,
    /// Global step counter, -1 when stopped.
    pub step: Signal<i32>,
    pub drive: [Signal<f32>; MAX_TRACKS],
    pub active: [Signal<bool>; MAX_TRACKS],
    /// Each track's patch after VARIANCE.
    pub live: [Signal<TrackPatch>; MAX_TRACKS],
}

/// Cheap to clone; shared through the Dioxus context.
#[derive(Clone)]
pub struct AudioHandle {
    backend: std::rc::Rc<Backend>,
    pub status: Signal<AudioStatus>,
    pub meters: Meters,
}

impl AudioHandle {
    /// Must be called inside the Dioxus runtime (e.g. from `use_hook`).
    pub fn new(initial: Snapshot) -> Self {
        let status = Signal::new(AudioStatus::Starting);
        let first = Status::default();
        let meters = Meters {
            status: Signal::new(first),
            step: Signal::new(-1),
            drive: core::array::from_fn(|_| Signal::new(0.0)),
            active: core::array::from_fn(|_| Signal::new(false)),
            live: core::array::from_fn(|i| Signal::new(initial.tracks[i])),
        };
        let (tx, rx) = unbounded::<Status>();
        spawn(forward(rx, meters));
        let backend = std::rc::Rc::new(Backend::new(status, initial, tx));
        Self {
            backend,
            status,
            meters,
        }
    }

    pub fn send(&self, cmd: Command) {
        self.backend.send(cmd);
    }

    /// Call from user-gesture handlers; lets the web backend start or resume.
    pub fn user_gesture(&self) {
        self.backend.user_gesture();
    }
}

fn set_if_changed<T: PartialEq + 'static>(mut s: Signal<T>, v: T) {
    if *s.peek() != v {
        s.set(v);
    }
}

/// Copy engine statuses from the backend into the signals, on the UI thread.
async fn forward(mut rx: UnboundedReceiver<Status>, m: Meters) {
    while let Some(s) = rx.next().await {
        // Several may queue up while the UI is busy; only the newest matters.
        let mut latest = s;
        while let Ok(s) = rx.try_recv() {
            latest = s;
        }
        set_if_changed(m.step, latest.step);
        for t in 0..MAX_TRACKS {
            set_if_changed(m.drive[t], latest.drive[t]);
            set_if_changed(m.active[t], latest.active[t]);
            set_if_changed(m.live[t], latest.live[t]);
        }
        set_if_changed(m.status, latest);
    }
}

pub type StatusSender = UnboundedSender<Status>;

/// Used when building without a platform feature (e.g. `cargo check`).
#[cfg(not(any(feature = "web", feature = "desktop")))]
pub struct NullBackend;

#[cfg(not(any(feature = "web", feature = "desktop")))]
impl NullBackend {
    fn new(mut status: Signal<AudioStatus>, _initial: Snapshot, _tx: StatusSender) -> Self {
        status.set(AudioStatus::Failed("built without an audio backend".into()));
        Self
    }
    fn send(&self, _cmd: Command) {}
    fn user_gesture(&self) {}
}
