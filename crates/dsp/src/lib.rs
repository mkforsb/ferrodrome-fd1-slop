//! Ferrodrome DSP: an industrial machine hall. Ten machine models
//! (conveyor, pump, crankshaft, transformer, servo, press, turbine, grinder,
//! welder, steam), each on a track with its own spin-up/down ramps, VARIANCE,
//! MUTATE, reverb and delay, played by a step sequencer where every track
//! loops its own length.
//!
//! The crate has no dependencies and no platform code so it can be compiled to
//! a standalone wasm module for an AudioWorklet as well as used natively.

// The machines loop over several parallel arrays at once; index loops read
// better there than zipped iterators.
#![allow(clippy::needless_range_loop)]

pub mod delay;
pub mod engine;
pub mod filter;
pub mod machines;
pub mod mutate;
pub mod params;
pub mod reverb;
pub mod steps;
pub mod track;
pub mod util;

pub use engine::{Engine, Status};
pub use params::{GlobalParam, MAX_STEPS, MAX_TRACKS, Param, TrackPatch};
pub use steps as seq;
