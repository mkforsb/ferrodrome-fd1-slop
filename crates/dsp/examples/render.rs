//! Render every machine solo and a few random halls to WAV files.
//! `cargo run --release -p ferrodrome-dsp --example render -- [out_dir] [machine]`

use ferrodrome_dsp::machines::{MACHINE_COUNT, MACHINE_NAMES};
use ferrodrome_dsp::mutate::{random_globals, random_length, random_patch, random_steps};
use ferrodrome_dsp::params::{
    GLOBAL_PARAM_COUNT, GlobalParam, MAX_TRACKS, PARAM_COUNT, Param, default_global_patch,
    default_patch,
};
use ferrodrome_dsp::util::Noise;
use ferrodrome_dsp::{Engine, steps};

const FS: u32 = 48_000;

fn write_wav(path: &std::path::Path, l: &[f32], r: &[f32]) -> std::io::Result<()> {
    let mut b = Vec::with_capacity(44 + l.len() * 4);
    let data_len = (l.len() * 4) as u32;
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&FS.to_le_bytes());
    b.extend_from_slice(&(FS * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for (a, c) in l.iter().zip(r) {
        for s in [a, c] {
            b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes());
        }
    }
    std::fs::write(path, b)
}

fn render(e: &mut Engine, seconds: f32) -> (Vec<f32>, Vec<f32>) {
    let n = (seconds * FS as f32) as usize;
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
        e.render(a, b);
    }
    (l, r)
}

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().unwrap_or_else(|| "renders".into()));
    let only = args.next();
    std::fs::create_dir_all(&dir)?;

    // Each machine alone: running for 12 of 16 steps, so the ramps show.
    for m in 0..MACHINE_COUNT as u32 {
        let name = MACHINE_NAMES[m as usize].to_lowercase();
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let mut e = Engine::new(FS as f32);
        e.load_patch(0, &default_patch(m));
        e.set_param(0, Param::Enabled, 1.0);
        e.set_param(0, Param::Length, 32.0);
        e.set_global(GlobalParam::Tempo, 120.0);
        for s in 0..24 {
            e.set_step(0, s, true);
        }
        e.snap_params();
        e.play(true);
        let (l, r) = render(&mut e, 10.0);
        let path = dir.join(format!("machine-{name}.wav"));
        write_wav(&path, &l, &r)?;
        println!("{}", path.display());
    }
    if only.is_some() {
        return Ok(());
    }

    // The filter following the ramp: a conveyor through a resonant LP24 that
    // opens three octaves as it spins up and closes as it runs down.
    let mut e = Engine::new(FS as f32);
    let mut p = default_patch(0);
    p[Param::Enabled.index()] = 1.0;
    p[Param::Length.index()] = 32.0;
    p[Param::RampUp.index()] = 0.75;
    p[Param::RampDown.index()] = 0.75;
    p[Param::FltType.index()] = ferrodrome_dsp::params::FILTER_LP24 as f32;
    p[Param::FltCutoff.index()] = 0.5;
    p[Param::FltReso.index()] = 0.75;
    p[Param::FltEnv.index()] = 0.89;
    e.load_patch(0, &p);
    e.set_global(GlobalParam::Tempo, 120.0);
    for s in 0..16 {
        e.set_step(0, s, true);
    }
    e.snap_params();
    e.play(true);
    let (l, r) = render(&mut e, 8.0);
    let path = dir.join("filter-ramp-sweep.wav");
    write_wav(&path, &l, &r)?;
    println!("{}", path.display());

    // Random halls, as RND would make them.
    let mut rng = Noise::new(2026);
    for hall in 0..4 {
        let mut e = Engine::new(FS as f32);
        let g = random_globals(
            &default_global_patch(),
            &[false; GLOBAL_PARAM_COUNT],
            &mut rng,
        );
        for p in ferrodrome_dsp::params::ALL_GLOBAL_PARAMS {
            e.set_global(p, g[p.index()]);
        }
        let tracks = 4 + rng.below(MAX_TRACKS - 3);
        for t in 0..tracks {
            let mut p = random_patch(&default_patch(0), &[false; PARAM_COUNT], &mut rng);
            p[Param::Enabled.index()] = 1.0;
            p[Param::Length.index()] = random_length(&mut rng) as f32;
            e.load_patch(t, &p);
            e.set_steps(t, random_steps(p[Param::Length.index()] as usize, &mut rng));
            println!(
                "  hall {hall} track {t}: {} len {} {:?}",
                MACHINE_NAMES[p[0] as usize],
                p[Param::Length.index()],
                steps::periods(e.steps(t), p[Param::Length.index()] as usize)
            );
        }
        e.snap_params();
        e.play(true);
        let (l, r) = render(&mut e, 24.0);
        let path = dir.join(format!("hall-{hall}.wav"));
        write_wav(&path, &l, &r)?;
        println!("{}", path.display());
    }
    Ok(())
}
