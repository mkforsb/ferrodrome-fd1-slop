//! RND, MUTATE and the random sequence generator.
//!
//! RND randomizes everything that is not locked; MUTATE moves a track part
//! of the way towards a random patch at the start of each of its active
//! periods: `clamp(current + (random − current) × MUTATE)`, with choices
//! (and stepped knobs) jumping with probability MUTATE instead. Both respect
//! the locks (Ctrl-click on the panel): a locked control keeps its value.
//! These live in the DSP crate because MUTATE runs on the audio thread,
//! where the sequencer starts the periods.

use crate::machines::{MACHINE_COUNT, MACHINE_PRESS, MACHINE_PUMP, MACHINE_SERVO, spec};
use crate::params::{
    ALL_GLOBAL_PARAMS, ALL_PARAMS, DELAY_TIME_NAMES, FILTER_BP, FILTER_HP12, FILTER_HP24,
    FILTER_NAMES, GLOBAL_PARAM_COUNT, GlobalParam, GlobalPatch, MAX_STEPS, PARAM_COUNT, Param,
    TrackPatch,
};
use crate::util::Noise;

pub type Locks = [bool; PARAM_COUNT];
pub type GlobalLocks = [bool; GLOBAL_PARAM_COUNT];

/// Controls nothing random ever touches: whether the track exists, and the
/// performer's mute and solo.
pub fn never_randomized(p: Param) -> bool {
    matches!(p, Param::Enabled | Param::Mute | Param::Solo)
}

/// Controls MUTATE changes (RND changes these and more: the sequence
/// length, VARIANCE, MUTATE itself and the level).
pub fn mutates(p: Param) -> bool {
    !never_randomized(p)
        && !matches!(
            p,
            Param::Length | Param::Variance | Param::Mutate | Param::Level
        )
}

/// Global controls RND leaves alone: the master level.
pub fn global_never_randomized(p: GlobalParam) -> bool {
    p == GlobalParam::Master
}

fn range(rng: &mut Noise, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * rng.uniform()
}

/// Mostly near the centre, sometimes far out.
fn bipolar(rng: &mut Noise, depth: f32) -> f32 {
    0.5 + depth * (2.0 * rng.uniform() - 1.0).powi(3)
}

/// A sequence length: mostly bars and half bars, sometimes odd lengths for
/// polymeter.
pub fn random_length(rng: &mut Noise) -> usize {
    const COMMON: [usize; 12] = [16, 16, 16, 32, 32, 8, 12, 24, 48, 64, 64, 128];
    const ODD: [usize; 8] = [5, 7, 9, 11, 13, 15, 17, 23];
    if rng.chance(0.75) {
        COMMON[rng.below(COMMON.len())]
    } else {
        ODD[rng.below(ODD.len())]
    }
}

/// Random value for `p` (in its own units) on a track running `machine`.
pub fn random_value(p: Param, machine: u32, rng: &mut Noise) -> f32 {
    let event = matches!(machine, MACHINE_PUMP | MACHINE_SERVO | MACHINE_PRESS);
    let v = match p {
        Param::Machine => rng.below(MACHINE_COUNT) as f32,
        Param::Enabled | Param::Mute | Param::Solo => 0.0,
        Param::Length => random_length(rng) as f32,
        Param::K1 | Param::K2 | Param::K3 | Param::K4 | Param::K5 | Param::K6 => {
            let ks = spec(machine).knobs[p.knob_slot().unwrap()];
            let x = range(rng, ks.rnd.0, ks.rnd.1);
            if ks.steps.is_empty() {
                x
            } else {
                let n = (ks.steps.len() - 1) as f32;
                (x * n).round() / n
            }
        }
        Param::RampUp if event => range(rng, 0.0, 0.25),
        Param::RampUp => range(rng, 0.1, 0.85),
        Param::RampDown => range(rng, 0.15, 0.85),
        Param::Level => range(rng, 0.6, 0.85),
        Param::Pan => bipolar(rng, 0.42),
        Param::Variance => {
            if rng.chance(0.5) {
                0.0
            } else {
                range(rng, 0.05, 0.5)
            }
        }
        Param::Mutate => {
            if rng.chance(0.75) {
                0.0
            } else {
                range(rng, 0.05, 0.4)
            }
        }
        // Half the time no filter; otherwise any type, mostly mild resonance.
        Param::FltType => {
            if rng.chance(0.5) {
                0.0
            } else {
                (1 + rng.below(FILTER_NAMES.len() - 1)) as f32
            }
        }
        Param::FltCutoff => range(rng, 0.45, 0.95),
        Param::FltReso => 0.85 * rng.uniform() * rng.uniform(),
        Param::FltDrive => {
            if rng.chance(0.6) {
                0.0
            } else {
                range(rng, 0.1, 0.7)
            }
        }
        Param::FltEnv => bipolar(rng, 0.45),
        Param::EqLow | Param::EqMid | Param::EqHigh => bipolar(rng, 0.25),
        Param::EqFreq => rng.uniform(),
        Param::RevMix => 0.75 * rng.uniform().sqrt(),
        Param::RevDecay => range(rng, 0.1, 0.75),
        Param::RevTone => range(rng, 0.3, 0.85),
        Param::RevLow | Param::RevMid | Param::RevHigh => range(rng, 0.38, 0.62),
        Param::DlyMix => {
            if rng.chance(0.6) {
                0.0
            } else {
                range(rng, 0.25, 0.65)
            }
        }
        Param::DlyTime => rng.below(DELAY_TIME_NAMES.len()) as f32,
        Param::DlyFeedback => range(rng, 0.1, 0.7),
        Param::DlyTone => range(rng, 0.3, 0.85),
    };
    p.sanitize(v)
}

/// RND on one track: every control that is not locked (and not mute, solo or
/// the track's existence) gets a new random value. The machine is drawn
/// first, so its knobs are drawn from the new machine's ranges.
pub fn random_patch(current: &TrackPatch, locks: &Locks, rng: &mut Noise) -> TrackPatch {
    let mut patch = *current;
    let mi = Param::Machine.index();
    if !locks[mi] {
        patch[mi] = random_value(Param::Machine, 0, rng);
    }
    let machine = patch[mi] as u32;
    for p in ALL_PARAMS {
        if p == Param::Machine || never_randomized(p) || locks[p.index()] {
            continue;
        }
        patch[p.index()] = random_value(p, machine, rng);
    }
    tame_filter(&mut patch, locks, rng);
    patch
}

/// A highpass or bandpass up near the top would leave almost nothing of
/// most machines: pull its cutoff down into the body of the sound.
fn tame_filter(patch: &mut TrackPatch, locks: &Locks, rng: &mut Noise) {
    let c = Param::FltCutoff.index();
    if locks[c] {
        return;
    }
    match patch[Param::FltType.index()] as u32 {
        FILTER_HP12 | FILTER_HP24 => patch[c] = range(rng, 0.2, 0.6),
        FILTER_BP => patch[c] = range(rng, 0.35, 0.75),
        _ => {}
    }
}

/// MUTATE: move every unlocked, mutable control `amount` (0..=1) of the way
/// towards a fresh random patch. Continuous controls glide part way; choices
/// and stepped knobs jump with probability `amount` (the machine itself with
/// `amount²`, so it changes rarely until MUTATE is high). When the machine
/// changes, its knobs come with it.
pub fn mutate_patch(
    current: &TrackPatch,
    locks: &Locks,
    amount: f32,
    rng: &mut Noise,
) -> TrackPatch {
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.0 {
        return *current;
    }
    let mut keep = *locks;
    for p in ALL_PARAMS {
        if !mutates(p) {
            keep[p.index()] = true;
        }
    }
    let mi = Param::Machine.index();
    let mut patch = *current;
    let old_machine = current[mi] as u32;
    if !keep[mi] && rng.chance(amount * amount) {
        patch[mi] = random_value(Param::Machine, 0, rng);
    }
    let machine = patch[mi] as u32;
    let switched = machine != old_machine;
    for p in ALL_PARAMS {
        if p == Param::Machine || keep[p.index()] {
            continue;
        }
        let i = p.index();
        let target = random_value(p, machine, rng);
        let knob = p.knob_slot().is_some();
        let stepped = p.steps_for(machine).is_some()
            || matches!(p.info().kind, crate::params::ParamKind::Choice(_));
        patch[i] = if knob && switched {
            target
        } else if stepped {
            if rng.chance(amount) {
                target
            } else {
                current[i]
            }
        } else {
            p.sanitize(current[i] + (target - current[i]) * amount)
        };
    }
    patch
}

/// A random sequence of `len` steps: runs of activity with gaps, from a
/// two-state Markov chain with a random density and mean run length.
pub fn random_steps(len: usize, rng: &mut Noise) -> u128 {
    let len = len.clamp(1, MAX_STEPS);
    let density = range(rng, 0.25, 0.75);
    let run = range(rng, 1.0, 9.0);
    let p_stop = 1.0 / run;
    let p_start = (density * p_stop / (1.0 - density)).min(1.0);
    let mut on = rng.chance(density);
    let mut steps = 0u128;
    for s in 0..len {
        if on {
            steps |= 1 << s;
        }
        on = if on {
            !rng.chance(p_stop)
        } else {
            rng.chance(p_start)
        };
    }
    if steps == 0 {
        steps = 1;
    }
    steps
}

/// RND for the master section: tempo, reverb and delay (not the master level).
pub fn random_globals(current: &GlobalPatch, locks: &GlobalLocks, rng: &mut Noise) -> GlobalPatch {
    let mut g = *current;
    for p in ALL_GLOBAL_PARAMS {
        if locks[p.index()] || global_never_randomized(p) {
            continue;
        }
        let v = match p {
            GlobalParam::Tempo => range(rng, 72.0, 152.0),
            GlobalParam::Master => current[p.index()],
            GlobalParam::RevMix => range(rng, 0.1, 0.5),
            GlobalParam::RevDecay => range(rng, 0.35, 0.85),
            GlobalParam::RevTone => range(rng, 0.25, 0.7),
            GlobalParam::RevPredelay => range(rng, 0.05, 0.6),
            GlobalParam::RevLow | GlobalParam::RevMid | GlobalParam::RevHigh => {
                range(rng, 0.4, 0.6)
            }
            GlobalParam::DlyMix => {
                if rng.chance(0.5) {
                    0.0
                } else {
                    range(rng, 0.15, 0.5)
                }
            }
            GlobalParam::DlyTime => rng.below(DELAY_TIME_NAMES.len()) as f32,
            GlobalParam::DlyFeedback => range(rng, 0.15, 0.65),
            GlobalParam::DlyTone => range(rng, 0.25, 0.75),
        };
        g[p.index()] = p.sanitize(v);
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{ParamKind, default_patch};

    fn sanitized(p: &TrackPatch) -> bool {
        ALL_PARAMS
            .iter()
            .all(|q| q.sanitize(p[q.index()]) == p[q.index()])
    }

    #[test]
    fn mutate_off_changes_nothing() {
        let c = default_patch(0);
        assert_eq!(
            mutate_patch(&c, &[false; PARAM_COUNT], 0.0, &mut Noise::new(1)),
            c
        );
    }

    #[test]
    fn locks_hold_under_rnd_and_mutate() {
        let mut rng = Noise::new(9);
        let mut c = default_patch(3);
        c[Param::Enabled.index()] = 1.0;
        let mut locks = [false; PARAM_COUNT];
        for p in [
            Param::Machine,
            Param::K2,
            Param::RevMix,
            Param::DlyTime,
            Param::RampUp,
        ] {
            locks[p.index()] = true;
        }
        for _ in 0..200 {
            let r = random_patch(&c, &locks, &mut rng);
            let m = mutate_patch(&c, &locks, 1.0, &mut rng);
            for p in ALL_PARAMS {
                if locks[p.index()] || never_randomized(p) {
                    assert_eq!(r[p.index()], c[p.index()], "{p:?}");
                    assert_eq!(m[p.index()], c[p.index()], "{p:?}");
                }
            }
            assert!(sanitized(&r) && sanitized(&m));
        }
    }

    #[test]
    fn mutate_leaves_structure_and_level_alone() {
        let mut rng = Noise::new(4);
        let mut c = default_patch(0);
        c[Param::Length.index()] = 23.0;
        c[Param::Mutate.index()] = 0.6;
        c[Param::Level.index()] = 0.4;
        let locks = [false; PARAM_COUNT];
        for _ in 0..100 {
            let m = mutate_patch(&c, &locks, 1.0, &mut rng);
            for p in [Param::Length, Param::Mutate, Param::Variance, Param::Level] {
                assert_eq!(m[p.index()], c[p.index()]);
            }
        }
    }

    #[test]
    fn partial_mutation_glides_continuous_controls() {
        let mut locks = [false; PARAM_COUNT];
        locks[Param::Machine.index()] = true;
        let c = default_patch(2);
        let m = mutate_patch(&c, &locks, 0.25, &mut Noise::new(3));
        for p in ALL_PARAMS {
            if mutates(p) && p.info().kind == ParamKind::Continuous && p.steps_for(2).is_none() {
                assert!((m[p.index()] - c[p.index()]).abs() <= 0.25 + 1e-6, "{p:?}");
            }
        }
    }

    #[test]
    fn random_steps_respect_length_and_are_not_empty() {
        let mut rng = Noise::new(5);
        for _ in 0..500 {
            let len = 1 + rng.below(MAX_STEPS);
            let s = random_steps(len, &mut rng);
            assert!(s != 0);
            if len < 128 {
                assert_eq!(s >> len, 0);
            }
        }
    }

    #[test]
    fn rnd_keeps_master_level() {
        let mut g = crate::params::default_global_patch();
        g[GlobalParam::Master.index()] = 0.31;
        let r = random_globals(&g, &[false; GLOBAL_PARAM_COUNT], &mut Noise::new(2));
        assert_eq!(r[GlobalParam::Master.index()], 0.31);
    }
}
