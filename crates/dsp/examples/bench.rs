//! Worst case: eight tracks running all the time with their reverbs, delays,
//! VARIANCE and MUTATE, plus the master effects. Prints the real-time factor.
//! `cargo run --release -p ferrodrome-dsp --example bench`

use std::time::Instant;

use ferrodrome_dsp::Engine;
use ferrodrome_dsp::machines::{MACHINE_COUNT, MACHINE_NAMES};
use ferrodrome_dsp::params::{GlobalParam, MAX_TRACKS, Param, default_patch};

const FS: f32 = 48_000.0;

fn bench(label: &str, setup: impl Fn(&mut Engine)) {
    let mut e = Engine::new(FS);
    setup(&mut e);
    e.set_global(GlobalParam::RevMix, 0.4);
    e.set_global(GlobalParam::DlyMix, 0.4);
    e.snap_params();
    e.play(true);
    let seconds = 20.0;
    let blocks = (seconds * FS / 128.0) as usize;
    let (mut l, mut r) = ([0.0f32; 128], [0.0f32; 128]);
    let t = Instant::now();
    for _ in 0..blocks {
        e.render(&mut l, &mut r);
    }
    let took = t.elapsed().as_secs_f32();
    println!("{label:<34} {:>6.1}× real time", seconds / took);
}

fn main() {
    let fx = if std::env::var_os("NOFX").is_some() {
        0.0
    } else {
        0.5
    };
    for m in 0..MACHINE_COUNT as u32 {
        bench(&format!("8 × {}", MACHINE_NAMES[m as usize]), |e| {
            for t in 0..MAX_TRACKS {
                e.load_patch(t, &default_patch(m));
                e.set_param(t, Param::Enabled, 1.0);
                e.set_param(t, Param::RevMix, fx);
                e.set_param(t, Param::DlyMix, fx);
                e.set_param(t, Param::Variance, 0.5);
                e.set_param(t, Param::FltType, 2.0);
                e.set_param(t, Param::FltReso, 0.6);
                e.set_param(t, Param::FltEnv, 0.8);
                e.set_param(t, Param::EqMid, 0.7);
                for s in 0..16 {
                    e.set_step(t, s, s % 4 != 3);
                }
            }
        });
    }
    bench("mixed hall, MUTATE at RND", |e| {
        for t in 0..MAX_TRACKS {
            e.load_patch(t, &default_patch((t % MACHINE_COUNT) as u32));
            e.set_param(t, Param::Enabled, 1.0);
            e.set_param(t, Param::RevMix, fx);
            e.set_param(t, Param::DlyMix, fx);
            e.set_param(t, Param::Mutate, 1.0);
            e.set_param(t, Param::Variance, 1.0);
            for s in 0..16 {
                e.set_step(t, s, s % 2 == 0);
            }
        }
    });
}
