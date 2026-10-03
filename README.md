# Ferrodrome FD-1

An industrial machine hall synthesizer and sequencer. Instead of voices and
notes you have **machines** and **shifts**: put up to eight machines on the
floor (a conveyor, a pump, a crankshaft, a transformer, a servo axis, a
stamping press, a turbine, a grinder, a welder, a steam vent), then draw on
each machine's row the steps where it runs. When a period starts the machine
spins up; when it ends, it runs down.

A sibling of [Xenokussion XK-1](https://github.com/mkforsb/xenokussion-xk1-slop),
[Palmkussion PK-1](https://github.com/mkforsb/palmkussion-pk1-slop) and
[Xenopalm XP-1](https://github.com/mkforsb/xenopalm-xp1-slop), with the same
architecture: a dependency-free DSP crate, a C-ABI wasm worklet, and a Dioxus
panel for the web and the Linux desktop.

| Target | Audio | How |
| --- | --- | --- |
| Web (wasm) | WebAudio `AudioWorklet` | `make web` (bundle) or `make serve-web` |
| Native Linux | PulseAudio (also PipeWire's pulse server) | `make desktop` (bundle) or `make run-desktop` |

## Requirements

* Rust (edition 2024) with `rustup target add wasm32-unknown-unknown`
* Dioxus CLI 0.7: `cargo install dioxus-cli --version 0.7.10 --locked`
* Desktop: `libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev libpulse-dev pkg-config`

`FERRODROME_LATENCY_MS` sets the desktop's PulseAudio target latency (default 20).

| Make target | Does |
| --- | --- |
| `make web` | `dx build --release --web` → `target/dx/ferrodrome/release/web/public` |
| `make desktop` | `dx build --release --desktop` → `target/dx/ferrodrome/release/linux/app` |
| `make serve-web`, `make run-desktop` | `dx serve` with hot reload |
| `make test` | 56 unit tests (DSP, worklet ABI, panel state) |
| `make lint` | rustfmt check, clippy on native, desktop and wasm, warnings as errors |
| `make renders` | every machine solo, a filter sweep and four random halls as WAVs in `./renders` |
| `make probe` | loudness of every machine (defaults, hits, random patches) |
| `make bench` | worst case CPU: eight machines with every effect running |

## The machines

Every machine has six knobs of its own and drives them with its **drive**:
the spin-up/spin-down envelope set by RAMP UP and RAMP DN (2 ms … 12 s).
Rotating machines run at a speed proportional to the drive, so you hear them
start and run down; the rest use it as power, current or pressure.

| Machine | What it is | Knobs |
| --- | --- | --- |
| **CONVEYOR** | Motor hum on a harmonic series, roller clicks (four-mode steel tubes), belt whirr and rumble, an occasional belt squeal | SPEED, HUM, RATTLE, ROLLERS (scattered ↔ rigid clatter), WHIRR, SQUEAL |
| **PUMP** | Reciprocating piston: on every stroke a thump, valve clicks and fluid hiss; a drive motor that audibly slows under each stroke's load; all through a pipe resonance | RATE (synced), THUMP, HISS, VALVE, PIPE, MOTOR |
| **CRANKSHAFT** | Firing pulses at `RPM/60 × cylinders/2`, with per-cylinder imbalance, through a Karplus–Strong exhaust and block modes; combustion clatter and valve tick | RPM, CYL (1–12), ROUGH (misfires, slop), EXHAUST, KNOCK, LOAD |
| **TRANSFORMER** | Magnetostriction hum at twice the mains (`tanh(s·B)²`: more even harmonics the harder the core), lamination buzz near the flux peaks, corona crackle at the voltage peaks, inrush on energizing | MAINS (40–70 Hz), SATURATE, BUZZ, ARC, BEAT (a second unit beating), TANK |
| **SERVO** | A move per cycle with a trapezoid velocity profile: motor whine that rises, holds and falls, gear mesh with sidebands and grit, end-stop clunk, PWM whine and hunting at rest | RATE (synced), SPEED, GEAR, TRAVEL, CLUNK, HOLD |
| **PRESS** | Hydraulic pressure build-up into the stamp: a heavy thump, a 16-mode die/workpiece ring (the XK-1's MODAL bank), the return stroke and latch | RATE (synced), WEIGHT, RING, HYDRAULIC, MATERIAL, RETURN |
| **TURBINE** | Blade-pass whine (shaft × blades), roar that opens up with speed, buzz-saw shaft-order tones, imbalance flutter | SPEED, BLADES, WHINE, ROAR, BUZZSAW, FLUTTER |
| **GRINDER** | Gear-mesh tone (shaft × teeth) with wear sidebands and a broken tooth, grit, stick-slip chatter, a resonant housing | SPEED, TEETH, WEAR, GRIT, CHATTER, HOUSING |
| **WELDER** | MIG-style short circuits at an irregular 40–220 Hz ("frying bacon"), arc mains buzz, sizzle, spatter pops and spark tinks, optional pulsed welding | BUZZ, CRACKLE, SIZZLE, HEAT, SPATTER, PULSE |
| **STEAM** | Vent hiss with turbulence, a breathy whistle, tempo-synced chuffing, boiler rumble and sputtering condensate | PRESSURE, VENT, WHISTLE, CHUFF (synced or continuous), RUMBLE, SPUTTER |

The synced machines (PUMP, SERVO, PRESS, STEAM's chuff) restart their cycle
when a period starts, so a one-step period is exactly one stroke, move or
stamp, on that step.

Each machine runs through its own **filter and EQ** before its level and
sends:

* **FILTER**: a Moog-style four-pole ladder (zero-delay feedback, so it tracks
  and stays stable up to the edge of self-oscillation), tapped for **LP12**,
  **LP24**, **BP** and **HP12**/**HP24**. CUTOFF (20 Hz – 20 kHz), RESO, DRIVE
  (up to +24 dB into the ladder's tanh) and **RAMP→**, which moves the cutoff
  up or down by up to five octaves with the machine's spin-up: the filter
  opens as the machine starts and closes as it runs down. Ctrl-click the type
  to lock it.
* **EQ**: low shelf (110 Hz), a sweepable mid peak (80 Hz – 12 kHz) and a
  high shelf (5 kHz), ±12 dB each. Built from state-variable filters (as are the reverb
  return EQs), so VARIANCE and MUTATE can move it every block without clicks.
* A **response curve** under them draws the filter and EQ together, live: it
  moves with the ramp and with VARIANCE.

It also has its own **reverb** (Dattorro plate: MIX, DECAY, TONE,
with LOW/MID/HIGH EQ on its return) and its own **ping-pong delay** (MIX,
tempo-synced TIME from 1/32 to 1/2, FDBK, TONE), LEVEL and PAN. The master
has **the hall** (MIX, DECAY, TONE, PRE-delay, return EQ) and a **master
delay**, after a soft clipper-protected sum.

## The shift plan (sequencer)

* `+ MACHINE` adds a machine on its own row (up to eight); ✕ on its card
  removes it, and its name button changes it.
* Click and drag along a row to draw or erase the steps it runs. Joined steps
  draw as one bar: one **active period**.
* Every row loops its own **length**, 1–128 steps of 1/16 (LEN, or click past
  a row's end to extend it), against one global step counter. Different
  lengths drift against each other.
* A run over a row's end **continues into its start**: with the last two and
  first two of 16 steps on, the machine runs one four-step period (arrows mark
  it, and the tooltip says "15 → 2 across the loop point"). It doesn't spin
  down and up again, and MUTATE acts once.
* M / S mute and solo (effect tails ring out), RUN (or keys `1`–`8`) holds a
  machine running regardless of its row, ⚄ rolls a random sequence, ◀ ▶
  rotate it.

## Randomness

* **RND** (the hazard-striped button) randomizes everything that isn't
  locked: machines, every knob, ramps, effects, VARIANCE and MUTATE, sequence
  lengths and steps, tempo and the master effects. Sequences suit the machine
  (long runs for things that need to spin up, short ones for stroke machines).
  An empty hall gets four to six machines. Each card also has its own RND (its
  sound only). Never touched: mute, solo and the master level.
* **VARIANCE** (per machine) makes every unlocked continuous control wander
  around its setting: an Ornstein–Uhlenbeck process, always pulled back to the
  centre, so it can't drift off to the extremes (at most ±0.28 of the range at
  full VARIANCE, usually much less). It never changes the patch itself, and a
  control at zero stays off. White dots on the knobs show where they are right
  now.
* **MUTATE** (per machine, OFF … RND) acts at the start of each active
  period: every unlocked control moves that fraction of the way towards a
  fresh random patch, `clamp(current + (random − current) × MUTATE)`;
  choices and stepped knobs jump with probability MUTATE, and the machine
  itself with probability MUTATE², bringing its knobs along. It runs on the
  audio thread so the new sound starts exactly on the step; the panel follows.
  Sequence length, level, VARIANCE and MUTATE itself are left alone.
* **Locks**: **Ctrl-click** (⌘-click) any knob or the machine button to lock
  it (a blue padlock appears). Locked controls are skipped by RND, MUTATE and
  VARIANCE. Ctrl-click a row's steps to lock its sequence and length against
  RND.

## How the machines were designed

From procedural audio practice (Farnell, *Designing Sound*), machinery
vibration analysis and a look at real recordings:

* Rotating machines are dominated by orders of the shaft speed: gear-mesh
  frequency = teeth × shaft speed, with wear showing up as sidebands a
  shaft-speed apart ([gear mesh frequency](https://www.fabrico.io/blog/gear-mesh-frequency/)).
  Turbines and fans sing at the blade-pass frequency.
* Transformer hum is at twice the mains frequency from magnetostriction, with
  higher even harmonics from the nonlinear core
  ([TestGuy](https://wiki.testguy.net/t/transformer-humming-noise-explained/88)).
* Engines are trains of exhaust pressure pulses through resonant pipes rather
  than sustained oscillations ([physics-informed engine sound synthesis](https://arxiv.org/html/2603.09391)).

The models were then compared against YouTube recordings of the real things
(a 5 MVA transformer, a diesel idle, conveyors, a piston pump, an industrial
servo, a servo press, a gas turbine, gear whine, MIG welding, steam release)
with `tools/spectro.py`, which plots side-by-side spectrograms and average
spectra. That comparison drove most of the revisions: real machines are much
brighter and noisier than first drafts, transformers keep strong harmonics
out to 1 kHz, pumps drone continuously under their strokes, servos whine
while holding still, MIG welding is a crackle of short circuits rather than
a buzz, steam hiss lives at 3–5 kHz.

```sh
make renders
yt-dlp -x --audio-format wav --download-sections "*0:03-0:33" -o refs/turbine.wav "ytsearch1:gas turbine whine"
python3 tools/spectro.py cmp.png renders/machine-turbine.wav:SIM refs/turbine.wav:REF
```

`tools/shoot.py` drives the web build in headless Chromium (Playwright):
screenshots, clicks, Ctrl-clicks, knob drags, key presses and read-outs.

## Layout

```
crates/dsp          no dependencies, 49 unit tests
  src/machines/       the ten machines, their knob specs and shared parts
    parts.rs            modes, synced cycles, OU wander, envelopes, Chebyshev harmonics
    modal.rs            the XK-1's 16-mode resonator bank (for PRESS)
  src/track.rs        machine + ramps + VARIANCE + MUTATE + filter/EQ + reverb/EQ + delay
  src/filter.rs       ladder filter (LP12/LP24/BP/HP12/HP24), track EQ, response curves
  src/engine.rs       eight tracks, the sequencer, master delay and hall, status for the panel
  src/steps.rs        128-step sequences and their (loop-aware) active periods
  src/mutate.rs       RND, MUTATE and the random sequence generator
  src/delay.rs        ping-pong delay
  src/reverb.rs       Dattorro plate (from the XK-1/PK-1)
  src/params.rs       parameters, ranges and read-outs shared by DSP and panel
crates/worklet      C ABI over the engine as a standalone wasm module
crates/app          Dioxus panel (web + desktop), audio backends, state and persistence
tools/              spectrogram comparison and headless browser driver
```

## What came from where

| Part | From |
| --- | --- |
| Workspace, Makefile, nested worklet build, AudioWorklet and PulseAudio backends, snapshot/replay, persistence | Xenopalm XP-1 |
| Knob, drag/wheel/double-click handling, padlock, LCD | Xenopalm XP-1 |
| Dattorro plate, biquad EQ, SVF, delay line, noise, fast sine/tanh | XK-1 / PK-1 |
| 16-mode MODAL resonator bank | XK-1 (in PRESS) |
| MUTATE's `clamp(current + (random − current) × MUTATE)` | XK-1 |
| Machines, ramps, VARIANCE, locks on everything, loop-aware periods, ladder filter and EQ, per-track effects | new |

## Performance

The worst case, eight machines running through resonant filters and EQs
with every reverb and delay active plus MUTATE and VARIANCE, renders at about
12–17× real time on one desktop core (`make bench`). The effects skip themselves
once their tails have died away, and machines at rest cost almost nothing.
