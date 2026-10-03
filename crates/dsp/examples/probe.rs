//! Loudness probe: every machine at its defaults, dry, running continuously
//! and as one-step hits, plus a spread of random patches.
//! `cargo run --release -p ferrodrome-dsp --example probe`

use ferrodrome_dsp::machines::{MACHINE_COUNT, MACHINE_NAMES};
use ferrodrome_dsp::mutate::random_patch;
use ferrodrome_dsp::params::{GlobalParam, PARAM_COUNT, Param, default_patch};
use ferrodrome_dsp::util::Noise;
use ferrodrome_dsp::{Engine, TrackPatch};

const FS: f32 = 48_000.0;

fn measure(patch: &TrackPatch, steps: &[usize], seconds: f32) -> (f32, f32) {
    let mut e = Engine::new(FS);
    e.load_patch(0, patch);
    e.set_param(0, Param::Enabled, 1.0);
    e.set_param(0, Param::RevMix, 0.0);
    e.set_param(0, Param::DlyMix, 0.0);
    e.set_param(0, Param::Length, 16.0);
    e.set_global(GlobalParam::RevMix, 0.0);
    e.set_global(GlobalParam::Tempo, 120.0);
    for &s in steps {
        e.set_step(0, s, true);
    }
    e.snap_params();
    e.play(true);
    let n = (seconds * FS) as usize;
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
        e.render(a, b);
    }
    // Skip the first second (spin-up).
    let tail = &l[FS as usize..];
    let rms = (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt();
    let peak = tail.iter().fold(0.0f32, |a, b| a.max(b.abs()));
    (rms, peak)
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

fn main() {
    let all: Vec<usize> = (0..16).collect();
    println!(
        "{:<12} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "machine", "run rms", "run peak", "hit rms", "rnd rms", "rnd max"
    );
    for m in 0..MACHINE_COUNT as u32 {
        let p = default_patch(m);
        let (rr, rp) = measure(&p, &all, 5.0);
        let (hr, _) = measure(&p, &[0, 8], 5.0);
        let mut rng = Noise::new(m + 100);
        let mut locks = [false; PARAM_COUNT];
        locks[Param::Machine.index()] = true;
        let mut rnd = Vec::new();
        for _ in 0..12 {
            let mut q = random_patch(&p, &locks, &mut rng);
            q[Param::Level.index()] = p[Param::Level.index()];
            q[Param::Mutate.index()] = 0.0;
            rnd.push(measure(&q, &all, 4.0).0);
        }
        let mean = rnd.iter().map(|&x| db(x)).sum::<f32>() / rnd.len() as f32;
        let max = rnd.iter().fold(0.0f32, |a, &b| a.max(b));
        println!(
            "{:<12} {:>7.1}dB {:>7.1}dB {:>7.1}dB {:>7.1}dB {:>7.1}dB",
            MACHINE_NAMES[m as usize],
            db(rr),
            db(rp),
            db(hr),
            mean,
            db(max)
        );
    }
}
