//! Click hunt: a conveyor through LP24 and a +12 dB low EQ (a user report),
//! rendered with parts switched off one at a time; counts high-frequency
//! clicks in the output.
//! `cargo run --release -p ferrodrome-dsp --example clicks`

use ferrodrome_dsp::Engine;
use ferrodrome_dsp::params::{GlobalParam, Param, default_patch};

const FS: f32 = 48_000.0;

fn patch() -> ferrodrome_dsp::TrackPatch {
    let mut p = default_patch(0);
    let mut set = |q: Param, v: f32| p[q.index()] = v;
    set(Param::Enabled, 1.0);
    set(Param::Length, 16.0);
    set(Param::K1, 1.0);
    set(Param::K2, 0.30);
    set(Param::K3, 0.0);
    set(Param::K4, 0.34);
    set(Param::K5, 0.27);
    set(Param::K6, 0.48);
    set(Param::RampUp, 0.7155);
    set(Param::RampDown, 0.1703);
    set(Param::Variance, 0.07);
    set(Param::Level, 0.6457);
    set(Param::FltType, 2.0);
    set(Param::FltCutoff, 0.4352);
    set(Param::FltReso, 0.0);
    set(Param::EqMid, 1.0);
    set(Param::EqFreq, 0.0);
    set(Param::DlyMix, 1.0);
    p
}

/// Clicks: samples where the 2nd difference (a crude high-pass) jumps far
/// above its typical level, grouped into events.
fn clicks(x: &[f32]) -> (usize, f32, f32) {
    let d2: Vec<f32> = x
        .windows(3)
        .map(|w| (w[0] - 2.0 * w[1] + w[2]).abs())
        .collect();
    let mut sorted = d2.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = sorted[sorted.len() / 2].max(1e-9);
    let mut events = 0;
    let mut last = 0usize;
    let mut worst = 0.0f32;
    let peak = d2.iter().fold(0.0f32, |a, &b| a.max(b));
    for (i, &v) in d2.iter().enumerate() {
        if v > 40.0 * median {
            if events == 0 || i - last > 96 {
                events += 1;
            }
            last = i;
            worst = worst.max(v / median);
        }
    }
    (events, worst, peak)
}

fn run(label: &str, tweak: impl Fn(&mut Engine)) {
    let mut e = Engine::new(FS);
    e.load_patch(0, &patch());
    e.set_global(GlobalParam::Tempo, 128.0);
    for s in 0..8 {
        e.set_step(0, s, true);
    }
    tweak(&mut e);
    e.snap_params();
    e.play(true);
    let n = (FS * 8.0) as usize;
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
        e.render(a, b);
    }
    let (events, worst, peak) = clicks(&l);
    println!(
        "{label:<44} {events:>4} clicks, worst {worst:>6.0}× median, largest 2nd difference {peak:.5}"
    );
}

fn main() {
    run("as reported", |_| {});
    run("no VARIANCE", |e| e.set_param(0, Param::Variance, 0.0));
    run("EQ flat", |e| e.set_param(0, Param::EqMid, 0.5));
    run("filter off", |e| e.set_param(0, Param::FltType, 0.0));
    run("filter off, EQ flat", |e| {
        e.set_param(0, Param::FltType, 0.0);
        e.set_param(0, Param::EqMid, 0.5);
    });
    run("no delay", |e| e.set_param(0, Param::DlyMix, 0.0));
    run("no delay, no VARIANCE", |e| {
        e.set_param(0, Param::DlyMix, 0.0);
        e.set_param(0, Param::Variance, 0.0);
    });
    run("no squeal", |e| e.set_param(0, Param::K6, 0.0));
    run("baseline: no VARIANCE, filter off, EQ flat", |e| {
        e.set_param(0, Param::Variance, 0.0);
        e.set_param(0, Param::FltType, 0.0);
        e.set_param(0, Param::EqMid, 0.5);
    });
    for only in ferrodrome_dsp::params::ALL_PARAMS {
        if !only.varies(0) {
            continue;
        }
        run(&format!("VARIANCE only on {only:?}"), |e| {
            for p in ferrodrome_dsp::params::ALL_PARAMS {
                e.set_lock(0, p, p != only);
            }
        });
    }
    run("no reverbs", |e| {
        e.set_param(0, Param::RevMix, 0.0);
        e.set_global(GlobalParam::RevMix, 0.0);
    });
}
