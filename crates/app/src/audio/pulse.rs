//! PulseAudio backend (works with PipeWire's pulse server too).
//!
//! A dedicated thread owns the engine and a `pa_simple` playback stream. The UI
//! sends [`Command`]s over a channel; the thread drains it before every block,
//! so parameter changes and live hits land within one block plus the stream's
//! target latency. The sequencer runs inside the engine, so its timing is
//! sample-accurate regardless. After each block the thread reports the engine
//! status back if it changed.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use dioxus::prelude::*;
use ferrodrome_dsp::Engine;
use libpulse_binding::def::BufferAttr;
use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;

use super::{AudioStatus, Command, Snapshot, StatusSender};

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u8 = 2;
const BLOCK: usize = 128;
const BYTES_PER_FRAME: usize = 4 * CHANNELS as usize;
/// Default target latency; override with `FERRODROME_LATENCY_MS`.
const DEFAULT_LATENCY_MS: u32 = 20;

pub struct PulseBackend {
    tx: mpsc::Sender<Command>,
}

impl PulseBackend {
    pub fn new(
        mut status: Signal<AudioStatus>,
        initial: Snapshot,
        status_tx: StatusSender,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<Command>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<String, String>>();
        let latency_ms = std::env::var("FERRODROME_LATENCY_MS")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(DEFAULT_LATENCY_MS)
            .clamp(3, 500);

        let spawned = thread::Builder::new()
            .name("ferrodrome-audio".into())
            .spawn(move || match open_stream(latency_ms) {
                Ok(stream) => {
                    let _ =
                        ready_tx.send(Ok(format!("PulseAudio · {latency_ms} ms target latency")));
                    run(stream, rx, initial, status_tx);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            });

        let result = match spawned {
            Err(e) => Err(format!("could not spawn audio thread: {e}")),
            Ok(_) => ready_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap_or_else(|_| Err("timed out connecting to PulseAudio".into())),
        };
        status.set(match result {
            Ok(detail) => AudioStatus::Running {
                sample_rate: SAMPLE_RATE,
                detail,
            },
            Err(e) => {
                eprintln!("ferrodrome: audio unavailable: {e}");
                AudioStatus::Failed(e)
            }
        });
        Self { tx }
    }

    pub fn send(&self, cmd: Command) {
        // If the audio thread died the error is already shown in the status.
        let _ = self.tx.send(cmd);
    }

    pub fn user_gesture(&self) {}
}

fn open_stream(latency_ms: u32) -> Result<Simple, String> {
    let spec = Spec {
        format: Format::F32le,
        channels: CHANNELS,
        rate: SAMPLE_RATE,
    };
    if !spec.is_valid() {
        return Err("invalid sample spec".into());
    }
    let tlength = (SAMPLE_RATE * latency_ms / 1000) * BYTES_PER_FRAME as u32;
    let attr = BufferAttr {
        maxlength: u32::MAX,
        tlength,
        prebuf: u32::MAX,
        minreq: (BLOCK * BYTES_PER_FRAME) as u32,
        fragsize: u32::MAX,
    };
    Simple::new(
        None,
        "Ferrodrome",
        Direction::Playback,
        None,
        "Industrial machine hall synthesizer",
        &spec,
        None,
        Some(&attr),
    )
    .map_err(|e| format!("PulseAudio: {e}"))
}

fn run(stream: Simple, rx: mpsc::Receiver<Command>, initial: Snapshot, status_tx: StatusSender) {
    let mut engine = Engine::new(SAMPLE_RATE as f32);
    for cmd in initial.commands() {
        apply(&mut engine, cmd);
    }
    // Start on the restored state rather than gliding to it.
    engine.snap_params();
    let mut left = [0.0f32; BLOCK];
    let mut right = [0.0f32; BLOCK];
    let mut bytes = vec![0u8; BLOCK * BYTES_PER_FRAME];
    let mut last_status = engine.status();
    loop {
        loop {
            match rx.try_recv() {
                Ok(cmd) => apply(&mut engine, cmd),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
        engine.render(&mut left, &mut right);
        let status = engine.status();
        if status != last_status {
            last_status = status;
            let _ = status_tx.unbounded_send(status);
        }
        for (i, (l, r)) in left.iter().zip(right.iter()).enumerate() {
            bytes[i * 8..i * 8 + 4].copy_from_slice(&l.to_le_bytes());
            bytes[i * 8 + 4..i * 8 + 8].copy_from_slice(&r.to_le_bytes());
        }
        if let Err(e) = stream.write(&bytes) {
            eprintln!("ferrodrome: PulseAudio write failed: {e}");
            return;
        }
    }
}

fn apply(engine: &mut Engine, cmd: Command) {
    match cmd {
        Command::Param {
            track,
            param,
            value,
        } => engine.set_param(track, param, value),
        Command::Global { param, value } => engine.set_global(param, value),
        Command::Step { track, step, on } => engine.set_step(track, step, on),
        Command::Lock {
            track,
            param,
            locked,
        } => engine.set_lock(track, param, locked),
        Command::Hold { track, on } => engine.hold(track, on),
        Command::Play(on) => engine.play(on),
    }
}
