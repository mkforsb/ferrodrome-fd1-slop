//! Parameter definitions shared by the engine, the wasm worklet and the UI.
//!
//! [`Param`]s exist once per track. Every track runs one machine, whose six
//! machine knobs (`K1`…`K6`) mean something different for every machine (see
//! [`crate::machines::spec`]), plus the controls every machine shares: ramp
//! times, level, pan, VARIANCE, MUTATE, mute/solo, a reverb with a three-band
//! EQ on its return and a delay. [`GlobalParam`]s cover the tempo, the master
//! level and the master reverb and delay.
//!
//! Every parameter is stored as an `f32`. Continuous parameters are normalized
//! to `0.0..=1.0` (think "knob position"); choice parameters hold the index of
//! the selected option and integers the number itself. Machine knobs are
//! always stored as knob positions, even the stepped ones (a stepped knob
//! snaps to the nearest of its positions where it is used), so MUTATE and
//! VARIANCE treat every machine alike. The [`map`] module converts knob
//! positions into physical units and is used by both the DSP and the UI
//! read-outs, so what the panel shows is what the engine does.

use crate::machines::{MACHINE_COUNT, MACHINE_NAMES, spec};

/// Tracks (machines) in the hall.
pub const MAX_TRACKS: usize = 8;
/// Longest track sequence.
pub const MAX_STEPS: usize = 128;
/// Machine-specific knobs per machine.
pub const KNOBS: usize = 6;
/// Steps per beat (the sequencer runs in 16ths).
pub const STEPS_PER_BEAT: usize = 4;

/// Per-track parameters. The discriminant is the stable wire id.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Param {
    /// Which machine this track runs (see [`MACHINE_NAMES`]).
    Machine = 0,
    /// Whether the track exists (tracks are added and removed on the panel).
    Enabled,
    /// Sequence length in steps.
    Length,
    /// The six machine knobs.
    K1,
    K2,
    K3,
    K4,
    K5,
    K6,
    /// Spin-up time when an active period starts.
    RampUp,
    /// Spin-down time when it ends.
    RampDown,
    Level,
    Pan,
    /// Constant, subtle wandering of every unlocked control around its setting.
    Variance,
    /// How far each new active period mutates the track (0 = off … 1 = RND).
    Mutate,
    Mute,
    Solo,
    /// Filter type: OFF, ladder LP 12/24, BP, HP 12/24 (see [`FILTER_NAMES`]).
    FltType,
    FltCutoff,
    /// Resonance, up to the edge of self-oscillation.
    FltReso,
    /// Input drive into the ladder's saturation.
    FltDrive,
    /// How far the cutoff follows the ramp (bipolar, in octaves at full drive).
    FltEnv,
    /// Three-band EQ: low shelf, sweepable mid peak, high shelf.
    EqLow,
    EqMid,
    EqFreq,
    EqHigh,
    /// Reverb send (wet level).
    RevMix,
    RevDecay,
    RevTone,
    RevLow,
    RevMid,
    RevHigh,
    /// Delay send (wet level).
    DlyMix,
    DlyTime,
    DlyFeedback,
    DlyTone,
}

pub const PARAM_COUNT: usize = 36;

pub const ALL_PARAMS: [Param; PARAM_COUNT] = [
    Param::Machine,
    Param::Enabled,
    Param::Length,
    Param::K1,
    Param::K2,
    Param::K3,
    Param::K4,
    Param::K5,
    Param::K6,
    Param::RampUp,
    Param::RampDown,
    Param::Level,
    Param::Pan,
    Param::Variance,
    Param::Mutate,
    Param::Mute,
    Param::Solo,
    Param::FltType,
    Param::FltCutoff,
    Param::FltReso,
    Param::FltDrive,
    Param::FltEnv,
    Param::EqLow,
    Param::EqMid,
    Param::EqFreq,
    Param::EqHigh,
    Param::RevMix,
    Param::RevDecay,
    Param::RevTone,
    Param::RevLow,
    Param::RevMid,
    Param::RevHigh,
    Param::DlyMix,
    Param::DlyTime,
    Param::DlyFeedback,
    Param::DlyTone,
];

pub const KNOB_PARAMS: [Param; KNOBS] = [
    Param::K1,
    Param::K2,
    Param::K3,
    Param::K4,
    Param::K5,
    Param::K6,
];

pub type TrackPatch = [f32; PARAM_COUNT];

pub const OFF_ON: &[&str] = &["OFF", "ON"];

/// Filter types: a Moog-style ladder tapped for 12 and 24 dB/oct.
pub const FILTER_NAMES: &[&str] = &["OFF", "LP12", "LP24", "BP", "HP12", "HP24"];
pub const FILTER_OFF: u32 = 0;
pub const FILTER_LP12: u32 = 1;
pub const FILTER_LP24: u32 = 2;
pub const FILTER_BP: u32 = 3;
pub const FILTER_HP12: u32 = 4;
pub const FILTER_HP24: u32 = 5;

/// Tempo-synced delay times and their length in beats.
pub const DELAY_TIME_NAMES: &[&str] = &[
    "1/32", "1/16T", "1/16", "1/8T", "1/16.", "1/8", "1/4T", "1/8.", "1/4", "1/4.", "1/2",
];
pub const DELAY_TIME_BEATS: [f32; 11] = [
    0.125,
    1.0 / 6.0,
    0.25,
    1.0 / 3.0,
    0.375,
    0.5,
    2.0 / 3.0,
    0.75,
    1.0,
    1.5,
    2.0,
];

/// Tempo-synced machine cycles (PUMP, SERVO, PRESS) and their length in beats.
pub const SYNC_NAMES: &[&str] = &[
    "4 BAR", "2 BAR", "1 BAR", "1/2", "1/4", "1/4T", "1/8", "1/8T", "1/16", "1/16T", "1/32",
];
pub const SYNC_BEATS: [f32; 11] = [
    16.0,
    8.0,
    4.0,
    2.0,
    1.0,
    2.0 / 3.0,
    0.5,
    1.0 / 3.0,
    0.25,
    1.0 / 6.0,
    0.125,
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamKind {
    /// `0.0..=1.0`.
    Continuous,
    /// Index into the names.
    Choice(&'static [&'static str]),
    /// Whole numbers in `min..=max`.
    Int { min: i32, max: i32 },
}

impl ParamKind {
    pub fn sanitize(self, v: f32, default: f32) -> f32 {
        let v = if v.is_finite() { v } else { default };
        match self {
            ParamKind::Continuous => v.clamp(0.0, 1.0),
            ParamKind::Choice(names) => v.round().clamp(0.0, (names.len() - 1) as f32),
            ParamKind::Int { min, max } => v.round().clamp(min as f32, max as f32),
        }
    }

    /// Value → `0..=1` position for a knob or fader.
    pub fn to_knob(self, v: f32) -> f32 {
        match self {
            ParamKind::Continuous => v,
            ParamKind::Choice(names) => v / (names.len() - 1).max(1) as f32,
            ParamKind::Int { min, max } => (v - min as f32) / (max - min).max(1) as f32,
        }
    }

    /// `0..=1` knob position → value (unrounded; sanitize afterwards).
    pub fn from_knob(self, k: f32) -> f32 {
        match self {
            ParamKind::Continuous => k,
            ParamKind::Choice(names) => k * (names.len() - 1) as f32,
            ParamKind::Int { min, max } => min as f32 + k * (max - min) as f32,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ParamInfo {
    /// Descriptive name.
    pub name: &'static str,
    /// Panel label.
    pub label: &'static str,
    pub default: f32,
    pub kind: ParamKind,
}

impl Param {
    pub fn id(self) -> u32 {
        self as u32
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_id(id: u32) -> Option<Param> {
        ALL_PARAMS.get(id as usize).copied()
    }

    pub fn info(self) -> ParamInfo {
        use ParamKind::*;
        let (name, label, default, kind) = match self {
            Param::Machine => ("Machine", "MACHINE", 0.0, Choice(MACHINE_NAMES)),
            Param::Enabled => ("Track in use", "ON", 0.0, Choice(OFF_ON)),
            Param::Length => (
                "Sequence length",
                "LEN",
                16.0,
                Int {
                    min: 1,
                    max: MAX_STEPS as i32,
                },
            ),
            Param::K1 => ("Machine knob 1", "K1", 0.5, Continuous),
            Param::K2 => ("Machine knob 2", "K2", 0.5, Continuous),
            Param::K3 => ("Machine knob 3", "K3", 0.5, Continuous),
            Param::K4 => ("Machine knob 4", "K4", 0.5, Continuous),
            Param::K5 => ("Machine knob 5", "K5", 0.5, Continuous),
            Param::K6 => ("Machine knob 6", "K6", 0.5, Continuous),
            Param::RampUp => ("Ramp up (spin-up time)", "RAMP UP", 0.62, Continuous),
            Param::RampDown => ("Ramp down (spin-down time)", "RAMP DN", 0.7, Continuous),
            Param::Level => ("Level", "LEVEL", 0.75, Continuous),
            Param::Pan => ("Pan", "PAN", 0.5, Continuous),
            Param::Variance => (
                "Variance (constant subtle wandering)",
                "VARIANCE",
                0.0,
                Continuous,
            ),
            Param::Mutate => ("Mutate on every active period", "MUTATE", 0.0, Continuous),
            Param::Mute => ("Mute", "M", 0.0, Choice(OFF_ON)),
            Param::Solo => ("Solo", "S", 0.0, Choice(OFF_ON)),
            Param::FltType => ("Filter type", "TYPE", 0.0, Choice(FILTER_NAMES)),
            Param::FltCutoff => ("Filter cutoff", "CUTOFF", 0.78, Continuous),
            Param::FltReso => ("Filter resonance", "RESO", 0.25, Continuous),
            Param::FltDrive => ("Filter drive", "DRIVE", 0.0, Continuous),
            Param::FltEnv => ("Ramp → cutoff", "RAMP→", 0.5, Continuous),
            Param::EqLow => ("EQ low shelf", "LOW", 0.5, Continuous),
            Param::EqMid => ("EQ mid peak", "MID", 0.5, Continuous),
            Param::EqFreq => ("EQ mid frequency", "FREQ", 0.5, Continuous),
            Param::EqHigh => ("EQ high shelf", "HIGH", 0.5, Continuous),
            Param::RevMix => ("Reverb send", "MIX", 0.25, Continuous),
            Param::RevDecay => ("Reverb decay", "DECAY", 0.35, Continuous),
            Param::RevTone => ("Reverb tone", "TONE", 0.55, Continuous),
            Param::RevLow => ("Reverb EQ low", "LOW", 0.5, Continuous),
            Param::RevMid => ("Reverb EQ mid", "MID", 0.5, Continuous),
            Param::RevHigh => ("Reverb EQ high", "HIGH", 0.5, Continuous),
            Param::DlyMix => ("Delay send", "MIX", 0.0, Continuous),
            Param::DlyTime => ("Delay time", "TIME", 5.0, Choice(DELAY_TIME_NAMES)),
            Param::DlyFeedback => ("Delay feedback", "FDBK", 0.35, Continuous),
            Param::DlyTone => ("Delay tone", "TONE", 0.6, Continuous),
        };
        ParamInfo {
            name,
            label,
            default,
            kind,
        }
    }

    pub fn default_value(self) -> f32 {
        self.info().default
    }

    /// Clamp/quantize a raw value to what this parameter accepts.
    pub fn sanitize(self, v: f32) -> f32 {
        let info = self.info();
        info.kind.sanitize(v, info.default)
    }

    /// Bipolar controls are centred at 0.5 (the panel lights them from the middle).
    pub fn is_bipolar(self) -> bool {
        matches!(
            self,
            Param::Pan
                | Param::RevLow
                | Param::RevMid
                | Param::RevHigh
                | Param::FltEnv
                | Param::EqLow
                | Param::EqMid
                | Param::EqHigh
        )
    }

    /// The machine knob slot (0..6) of a machine knob.
    pub fn knob_slot(self) -> Option<usize> {
        KNOB_PARAMS.iter().position(|&p| p == self)
    }

    /// Panel label, resolving machine knobs for the given machine.
    pub fn label_for(self, machine: u32) -> &'static str {
        match self.knob_slot() {
            Some(slot) => spec(machine).knobs[slot].label,
            None => self.info().label,
        }
    }

    /// Descriptive name, resolving machine knobs for the given machine.
    pub fn name_for(self, machine: u32) -> &'static str {
        match self.knob_slot() {
            Some(slot) => spec(machine).knobs[slot].name,
            None => self.info().name,
        }
    }

    /// Stepped machine knobs: their position names.
    pub fn steps_for(self, machine: u32) -> Option<&'static [&'static str]> {
        let slot = self.knob_slot()?;
        let steps = spec(machine).knobs[slot].steps;
        (!steps.is_empty()).then_some(steps)
    }

    /// Whether VARIANCE may move this control (if it is not locked).
    pub fn varies(self, machine: u32) -> bool {
        match self {
            Param::Machine
            | Param::Enabled
            | Param::Length
            | Param::Variance
            | Param::Mutate
            | Param::Mute
            | Param::Solo
            | Param::FltType
            | Param::DlyTime => false,
            p => p.steps_for(machine).is_none(),
        }
    }

    /// Human readable value, resolving machine knobs for the given machine.
    pub fn display_for(self, machine: u32, v: f32) -> String {
        if let Some(slot) = self.knob_slot() {
            return crate::machines::knob_display(machine, slot, v);
        }
        match self {
            Param::Machine
            | Param::Enabled
            | Param::Mute
            | Param::Solo
            | Param::DlyTime
            | Param::FltType => match self.info().kind {
                ParamKind::Choice(names) => names[self.sanitize(v) as usize].to_string(),
                _ => unreachable!(),
            },
            Param::Length => format!("{} st", self.sanitize(v) as u32),
            Param::RampUp | Param::RampDown => fmt_secs(map::ramp_seconds(v)),
            Param::Level => fmt_db(map::level_gain(v)),
            Param::Pan => fmt_pan(v),
            Param::Variance | Param::Mutate => {
                if v <= 0.0 {
                    "OFF".into()
                } else if v >= 1.0 && self == Param::Mutate {
                    "RND".into()
                } else {
                    format!("{:.0}%", v * 100.0)
                }
            }
            Param::RevMix | Param::DlyMix => fmt_db(map::send_gain(v)),
            Param::FltCutoff => fmt_hz(map::cutoff_hz(v)),
            Param::FltReso => format!("{:.0}%", v * 100.0),
            Param::FltDrive => {
                if v <= 0.0 {
                    "CLEAN".into()
                } else {
                    format!("+{:.1} dB", 20.0 * map::filter_drive(v).log10())
                }
            }
            Param::FltEnv => {
                let o = map::env_octaves(v);
                if o.abs() < 0.05 {
                    "OFF".into()
                } else {
                    format!("{o:+.1} oct")
                }
            }
            Param::EqLow | Param::EqMid | Param::EqHigh => fmt_eq(v),
            Param::EqFreq => fmt_hz(map::eq_freq_hz(v)),
            Param::RevDecay => fmt_secs(map::reverb_seconds(v)),
            Param::RevTone | Param::DlyTone => fmt_hz(map::tone_hz(v)),
            Param::RevLow | Param::RevMid | Param::RevHigh => fmt_eq(v),
            Param::DlyFeedback => format!("{:.0}%", map::feedback(v) * 100.0),
            Param::K1 | Param::K2 | Param::K3 | Param::K4 | Param::K5 | Param::K6 => {
                unreachable!()
            }
        }
    }
}

/// A fresh track running `machine` with that machine's default knobs.
pub fn default_patch(machine: u32) -> TrackPatch {
    let mut p = [0.0; PARAM_COUNT];
    for q in ALL_PARAMS {
        p[q.index()] = q.default_value();
    }
    let machine = (machine as usize).min(MACHINE_COUNT - 1) as u32;
    p[Param::Machine.index()] = machine as f32;
    apply_machine_defaults(&mut p, machine);
    p
}

/// Load a machine's default knobs and ramps into a patch.
pub fn apply_machine_defaults(p: &mut TrackPatch, machine: u32) {
    let s = spec(machine);
    for (slot, k) in KNOB_PARAMS.iter().enumerate() {
        p[k.index()] = s.knobs[slot].default;
    }
    p[Param::RampUp.index()] = s.ramp.0;
    p[Param::RampDown.index()] = s.ramp.1;
}

pub fn fmt_hz(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.2} kHz", hz / 1000.0)
    } else if hz >= 10.0 {
        format!("{:.0} Hz", hz)
    } else {
        format!("{:.2} Hz", hz)
    }
}

pub fn fmt_db(gain: f32) -> String {
    if gain <= 1e-4 {
        "-inf dB".to_string()
    } else {
        format!("{:.1} dB", 20.0 * gain.log10())
    }
}

pub fn fmt_secs(s: f32) -> String {
    if s >= 1.0 {
        format!("{:.2} s", s)
    } else if s >= 0.01 {
        format!("{:.0} ms", s * 1000.0)
    } else {
        format!("{:.1} ms", s * 1000.0)
    }
}

pub fn fmt_pct(v: f32) -> String {
    if v <= 0.0 {
        "OFF".into()
    } else {
        format!("{:.0}%", v * 100.0)
    }
}

pub fn fmt_pan(v: f32) -> String {
    let p = (v - 0.5) * 200.0;
    if p.abs() < 1.0 {
        "C".to_string()
    } else if p < 0.0 {
        format!("L{:.0}", -p)
    } else {
        format!("R{:.0}", p)
    }
}

pub fn fmt_eq(v: f32) -> String {
    let db = map::eq_db(v);
    if db.abs() < 0.05 {
        "0 dB".into()
    } else {
        format!("{db:+.1} dB")
    }
}

// --- global parameters ---------------------------------------------------------

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GlobalParam {
    Tempo = 0,
    Master,
    RevMix,
    RevDecay,
    RevTone,
    RevPredelay,
    RevLow,
    RevMid,
    RevHigh,
    DlyMix,
    DlyTime,
    DlyFeedback,
    DlyTone,
}

pub const GLOBAL_PARAM_COUNT: usize = 13;

pub const ALL_GLOBAL_PARAMS: [GlobalParam; GLOBAL_PARAM_COUNT] = [
    GlobalParam::Tempo,
    GlobalParam::Master,
    GlobalParam::RevMix,
    GlobalParam::RevDecay,
    GlobalParam::RevTone,
    GlobalParam::RevPredelay,
    GlobalParam::RevLow,
    GlobalParam::RevMid,
    GlobalParam::RevHigh,
    GlobalParam::DlyMix,
    GlobalParam::DlyTime,
    GlobalParam::DlyFeedback,
    GlobalParam::DlyTone,
];

pub type GlobalPatch = [f32; GLOBAL_PARAM_COUNT];

impl GlobalParam {
    pub fn id(self) -> u32 {
        self as u32
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_id(id: u32) -> Option<Self> {
        ALL_GLOBAL_PARAMS.get(id as usize).copied()
    }

    pub fn info(self) -> ParamInfo {
        use ParamKind::*;
        let (name, label, default, kind) = match self {
            GlobalParam::Tempo => ("Tempo", "TEMPO", 116.0, Int { min: 40, max: 240 }),
            GlobalParam::Master => ("Master level", "MASTER", 0.75, Continuous),
            GlobalParam::RevMix => ("Master reverb mix", "MIX", 0.22, Continuous),
            GlobalParam::RevDecay => ("Master reverb decay", "DECAY", 0.6, Continuous),
            GlobalParam::RevTone => ("Master reverb tone", "TONE", 0.45, Continuous),
            GlobalParam::RevPredelay => ("Master reverb pre-delay", "PRE", 0.25, Continuous),
            GlobalParam::RevLow => ("Master reverb EQ low", "LOW", 0.5, Continuous),
            GlobalParam::RevMid => ("Master reverb EQ mid", "MID", 0.5, Continuous),
            GlobalParam::RevHigh => ("Master reverb EQ high", "HIGH", 0.45, Continuous),
            GlobalParam::DlyMix => ("Master delay mix", "MIX", 0.0, Continuous),
            GlobalParam::DlyTime => ("Master delay time", "TIME", 7.0, Choice(DELAY_TIME_NAMES)),
            GlobalParam::DlyFeedback => ("Master delay feedback", "FDBK", 0.4, Continuous),
            GlobalParam::DlyTone => ("Master delay tone", "TONE", 0.5, Continuous),
        };
        ParamInfo {
            name,
            label,
            default,
            kind,
        }
    }

    pub fn default_value(self) -> f32 {
        self.info().default
    }

    pub fn sanitize(self, v: f32) -> f32 {
        let info = self.info();
        info.kind.sanitize(v, info.default)
    }

    pub fn is_bipolar(self) -> bool {
        matches!(
            self,
            GlobalParam::RevLow | GlobalParam::RevMid | GlobalParam::RevHigh
        )
    }

    pub fn display(self, v: f32) -> String {
        match self {
            GlobalParam::Tempo => format!("{} BPM", self.sanitize(v) as u32),
            GlobalParam::Master => fmt_db(map::level_gain(v)),
            GlobalParam::RevMix | GlobalParam::DlyMix => fmt_db(map::send_gain(v)),
            GlobalParam::RevDecay => fmt_secs(map::reverb_seconds(v)),
            GlobalParam::RevTone | GlobalParam::DlyTone => fmt_hz(map::tone_hz(v)),
            GlobalParam::RevPredelay => fmt_secs(map::reverb_predelay_seconds(v)),
            GlobalParam::RevLow | GlobalParam::RevMid | GlobalParam::RevHigh => fmt_eq(v),
            GlobalParam::DlyTime => DELAY_TIME_NAMES[self.sanitize(v) as usize].into(),
            GlobalParam::DlyFeedback => format!("{:.0}%", map::feedback(v) * 100.0),
        }
    }
}

pub fn default_global_patch() -> GlobalPatch {
    ALL_GLOBAL_PARAMS.map(|p| p.default_value())
}

/// Knob positions → physical units, shared by the DSP and the panel.
pub mod map {
    use super::{DELAY_TIME_BEATS, SYNC_BEATS};

    /// Ramp up/down: 2 ms … 12 s.
    pub fn ramp_seconds(v: f32) -> f32 {
        0.002 * 6000f32.powf(v.clamp(0.0, 1.0))
    }

    pub fn level_gain(level: f32) -> f32 {
        level * level
    }

    /// Reverb and delay sends.
    pub fn send_gain(amount: f32) -> f32 {
        amount * amount
    }

    /// Approximate RT60 of the plate.
    pub fn reverb_seconds(decay: f32) -> f32 {
        0.3 * 50f32.powf(decay)
    }

    /// Damping lowpass of the reverbs and delays.
    pub fn tone_hz(tone: f32) -> f32 {
        600.0 * 30f32.powf(tone)
    }

    /// Kept for the plate, which calls it by this name.
    pub fn reverb_tone_hz(tone: f32) -> f32 {
        tone_hz(tone)
    }

    pub fn reverb_predelay_seconds(predelay: f32) -> f32 {
        0.15 * predelay
    }

    /// Reverb return EQ: ±12 dB.
    pub fn eq_db(v: f32) -> f32 {
        (v - 0.5) * 24.0
    }

    /// Filter cutoff: 20 Hz … 20 kHz.
    pub fn cutoff_hz(v: f32) -> f32 {
        20.0 * 1000f32.powf(v.clamp(0.0, 1.0))
    }

    /// Ladder feedback `k`: 4 is self-oscillation; stop just short of it.
    pub fn reso_k(v: f32) -> f32 {
        3.9 * v.clamp(0.0, 1.0).powf(0.8)
    }

    /// Gain into the ladder's saturation: ×1 … ×16 (+24 dB).
    pub fn filter_drive(v: f32) -> f32 {
        16f32.powf(v.clamp(0.0, 1.0))
    }

    /// RAMP→: cutoff offset in octaves at full drive, ±5.
    pub fn env_octaves(v: f32) -> f32 {
        let x = (v - 0.5) * 2.0;
        5.0 * x * x.abs()
    }

    /// EQ mid peak: 80 Hz … 12 kHz.
    pub fn eq_freq_hz(v: f32) -> f32 {
        80.0 * 150f32.powf(v.clamp(0.0, 1.0))
    }

    pub fn feedback(v: f32) -> f32 {
        0.92 * v
    }

    pub fn delay_beats(choice: f32) -> f32 {
        DELAY_TIME_BEATS[(choice.max(0.0) as usize).min(DELAY_TIME_BEATS.len() - 1)]
    }

    /// Position of a stepped knob, `0..n`.
    pub fn step_index(v: f32, n: usize) -> usize {
        ((v.clamp(0.0, 1.0) * (n - 1) as f32).round() as usize).min(n - 1)
    }

    /// A tempo-synced cycle knob, in beats.
    pub fn sync_beats(v: f32) -> f32 {
        SYNC_BEATS[step_index(v, SYNC_BEATS.len())]
    }

    /// Exponential map of a knob onto `lo..hi`.
    pub fn expo(v: f32, lo: f32, hi: f32) -> f32 {
        lo * (hi / lo).powf(v.clamp(0.0, 1.0))
    }

    pub fn seconds_per_beat(bpm: f32) -> f32 {
        60.0 / bpm.max(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_roundtrip() {
        for (i, p) in ALL_PARAMS.iter().enumerate() {
            assert_eq!(p.index(), i);
            assert_eq!(Param::from_id(p.id()), Some(*p));
        }
        for (i, p) in ALL_GLOBAL_PARAMS.iter().enumerate() {
            assert_eq!(p.index(), i);
            assert_eq!(GlobalParam::from_id(p.id()), Some(*p));
        }
    }

    #[test]
    fn defaults_are_sanitized() {
        for m in 0..MACHINE_COUNT as u32 {
            let p = default_patch(m);
            for q in ALL_PARAMS {
                assert_eq!(q.sanitize(p[q.index()]), p[q.index()], "{q:?}");
            }
        }
        for q in ALL_GLOBAL_PARAMS {
            assert_eq!(q.sanitize(q.default_value()), q.default_value());
        }
    }

    #[test]
    fn every_value_displays() {
        for m in 0..MACHINE_COUNT as u32 {
            for p in ALL_PARAMS {
                for k in [0.0, 0.37, 1.0] {
                    let v = p.sanitize(p.info().kind.from_knob(k));
                    assert!(!p.display_for(m, v).is_empty());
                }
            }
        }
    }

    #[test]
    fn ramp_range() {
        assert!((map::ramp_seconds(0.0) - 0.002).abs() < 1e-6);
        assert!((map::ramp_seconds(1.0) - 12.0).abs() < 1e-3);
    }
}
