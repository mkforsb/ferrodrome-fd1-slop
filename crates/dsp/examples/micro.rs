//! Per-part cost in nanoseconds per sample.
use ferrodrome_dsp::delay::PingPong;
use ferrodrome_dsp::machines::{Ctx, MACHINE_COUNT, MACHINE_NAMES, Machines};
use ferrodrome_dsp::params::{KNOB_PARAMS, default_patch};
use ferrodrome_dsp::reverb::Plate;
use ferrodrome_dsp::util::Noise;
use std::time::Instant;

const FS: f32 = 48_000.0;
const N: usize = 480_000;

fn main() {
    for m in 0..MACHINE_COUNT as u32 {
        let mut ms = Machines::new(FS);
        let mut c = Ctx::new(FS);
        let p = default_patch(m);
        for (i, k) in KNOB_PARAMS.iter().enumerate() {
            c.k[i] = p[k.index()];
        }
        c.gate = true;
        ms.start(m, &c);
        let mut rng = Noise::new(1);
        let mut acc = 0.0;
        let t = Instant::now();
        for i in 0..N {
            if i % 32 == 0 {
                ms.control(m, &c);
            }
            acc += ms.tick(m, &c, 1.0, &mut rng);
        }
        println!(
            "{:<12} {:>6.1} ns/sample ({acc:.1})",
            MACHINE_NAMES[m as usize],
            t.elapsed().as_nanos() as f64 / N as f64
        );
    }
    let mut p = Plate::new(FS);
    let c = p.controls(0.5, 0.5, 0.1);
    let mut acc = 0.0;
    let t = Instant::now();
    for i in 0..N {
        let (a, b) = p.process(if i % 1000 == 0 { 1.0 } else { 0.0 }, &c);
        acc += a + b;
    }
    println!(
        "plate        {:>6.1} ns/sample ({acc:.1})",
        t.elapsed().as_nanos() as f64 / N as f64
    );
    let mut d = PingPong::new(FS);
    let t = Instant::now();
    for i in 0..N {
        let (a, b) = d.process(if i % 1000 == 0 { 1.0 } else { 0.0 }, 0.3, 0.5, 0.5);
        acc += a + b;
    }
    println!(
        "pingpong     {:>6.1} ns/sample ({acc:.1})",
        t.elapsed().as_nanos() as f64 / N as f64
    );
    for (label, steps, fx) in [
        ("engine 8 idle tracks", false, 0.0),
        ("engine 8 conveyors no fx", true, 0.0),
        ("engine 8 conveyors fx", true, 0.5),
    ] {
        let mut e = ferrodrome_dsp::Engine::new(FS);
        for t in 0..8 {
            e.load_patch(t, &default_patch(0));
            e.set_param(t, ferrodrome_dsp::Param::Enabled, 1.0);
            e.set_param(t, ferrodrome_dsp::Param::RevMix, fx);
            e.set_param(t, ferrodrome_dsp::Param::DlyMix, fx);
            if steps {
                for s in 0..16 {
                    e.set_step(t, s, true);
                }
            }
        }
        e.play(true);
        let (mut l, mut r) = ([0.0f32; 128], [0.0f32; 128]);
        let t = Instant::now();
        for _ in 0..N / 128 {
            e.render(&mut l, &mut r);
            acc += l[0];
        }
        println!(
            "{label:<26} {:>6.1} ns/sample",
            t.elapsed().as_nanos() as f64 / N as f64
        );
    }
    let mut rng = Noise::new(3);
    let t = Instant::now();
    for _ in 0..N {
        acc += rng.gauss();
    }
    println!(
        "gauss        {:>6.1} ns/sample ({acc:.1})",
        t.elapsed().as_nanos() as f64 / N as f64
    );
}

#[allow(dead_code)]
fn engine_cases() {}
