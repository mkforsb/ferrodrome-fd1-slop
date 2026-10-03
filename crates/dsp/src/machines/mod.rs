//! The machines.
//!
//! Every machine sees the same small interface. [`Ctx`] carries the block's
//! control values (the six machine knobs after VARIANCE and smoothing, and
//! the tempo), `control` recomputes coefficients once per control block,
//! `start` is called when an active period begins, and `tick` renders one
//! sample given the drive: the track's spin-up/spin-down envelope (0 = at
//! rest, 1 = running at its setting). Rotating machines run at a speed
//! proportional to the drive, so they audibly spin up and down; the others
//! use it as power, pressure or current.
//!
//! The tempo-synced machines (PUMP, SERVO, PRESS, STEAM's chuff) restart
//! their cycle on every active period, so a one-step period gives exactly
//! one stroke, stamp or move, on the step.

pub mod conveyor;
pub mod crankshaft;
pub mod grinder;
pub mod modal;
pub mod parts;
pub mod press;
pub mod pump;
pub mod servo;
pub mod steam;
pub mod transformer;
pub mod turbine;
pub mod welder;

use crate::params::{KNOBS, SYNC_NAMES, fmt_hz, fmt_pct, fmt_secs, map};
use crate::util::Noise;

pub const MACHINE_CONVEYOR: u32 = 0;
pub const MACHINE_PUMP: u32 = 1;
pub const MACHINE_CRANKSHAFT: u32 = 2;
pub const MACHINE_TRANSFORMER: u32 = 3;
pub const MACHINE_SERVO: u32 = 4;
pub const MACHINE_PRESS: u32 = 5;
pub const MACHINE_TURBINE: u32 = 6;
pub const MACHINE_GRINDER: u32 = 7;
pub const MACHINE_WELDER: u32 = 8;
pub const MACHINE_STEAM: u32 = 9;
pub const MACHINE_COUNT: usize = 10;

pub const MACHINE_NAMES: &[&str] = &[
    "CONVEYOR",
    "PUMP",
    "CRANKSHAFT",
    "TRANSFORMER",
    "SERVO",
    "PRESS",
    "TURBINE",
    "GRINDER",
    "WELDER",
    "STEAM",
];

pub const CYL_NAMES: &[&str] = &["1", "2", "3", "4", "5", "6", "8", "12"];
pub const CYL_COUNTS: [u32; 8] = [1, 2, 3, 4, 5, 6, 8, 12];
pub const BLADE_NAMES: &[&str] = &["5", "7", "9", "11", "13", "17", "23", "29", "37", "48"];
pub const BLADE_COUNTS: [u32; 10] = [5, 7, 9, 11, 13, 17, 23, 29, 37, 48];
pub const TEETH_NAMES: &[&str] = &["7", "11", "13", "17", "19", "23", "29", "31", "37", "41"];
pub const TEETH_COUNTS: [u32; 10] = [7, 11, 13, 17, 19, 23, 29, 31, 37, 41];
pub const CHUFF_NAMES: &[&str] = &["CONT", "1 BAR", "1/2", "1/4", "1/8", "1/8T", "1/16"];
/// Chuff cycle in beats (index 0 = a continuous vent).
pub const CHUFF_BEATS: [f32; 7] = [0.0, 4.0, 2.0, 1.0, 0.5, 1.0 / 3.0, 0.25];

#[derive(Clone, Copy, Debug)]
pub struct KnobSpec {
    pub label: &'static str,
    pub name: &'static str,
    /// Knob position.
    pub default: f32,
    /// Position names of a stepped knob (empty = continuous).
    pub steps: &'static [&'static str],
    /// Range RND draws from (knob positions).
    pub rnd: (f32, f32),
}

const fn k(label: &'static str, name: &'static str, default: f32, rnd: (f32, f32)) -> KnobSpec {
    KnobSpec {
        label,
        name,
        default,
        steps: &[],
        rnd,
    }
}

/// A stepped knob; `default` and `rnd` are step indices.
const fn ks(
    label: &'static str,
    name: &'static str,
    steps: &'static [&'static str],
    default: usize,
    rnd: (usize, usize),
) -> KnobSpec {
    let n = (steps.len() - 1) as f32;
    KnobSpec {
        label,
        name,
        default: default as f32 / n,
        steps,
        rnd: (rnd.0 as f32 / n, rnd.1 as f32 / n),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MachineSpec {
    pub name: &'static str,
    pub blurb: &'static str,
    pub knobs: [KnobSpec; KNOBS],
    /// Default ramp up/down knob positions.
    pub ramp: (f32, f32),
    /// Output trim so the machines sit at similar loudness.
    pub trim: f32,
}

pub const SPECS: [MachineSpec; MACHINE_COUNT] = [
    MachineSpec {
        name: "CONVEYOR",
        blurb: "Belt over steel rollers: motor hum, roller clatter, belt whirr and the odd squeal",
        knobs: [
            k("SPEED", "Belt speed (motor hum pitch)", 0.45, (0.1, 0.9)),
            k("HUM", "Motor hum level and brightness", 0.55, (0.1, 0.9)),
            k("RATTLE", "Roller rattle", 0.5, (0.0, 1.0)),
            k(
                "ROLLERS",
                "Roller regularity: scattered … rigid clatter",
                0.6,
                (0.0, 1.0),
            ),
            k("WHIRR", "Belt friction whirr", 0.45, (0.0, 0.9)),
            k("SQUEAL", "Belt squeal", 0.1, (0.0, 0.6)),
        ],
        ramp: (0.62, 0.7),
        trim: 1.19,
    },
    MachineSpec {
        name: "PUMP",
        blurb: "Reciprocating piston pump: thump, valve clicks and fluid hiss on every stroke",
        knobs: [
            ks("RATE", "Stroke cycle (tempo synced)", SYNC_NAMES, 4, (2, 9)),
            k("THUMP", "Piston thump", 0.6, (0.1, 1.0)),
            k("HISS", "Fluid hiss", 0.5, (0.0, 1.0)),
            k("VALVE", "Check valve clicks", 0.5, (0.0, 1.0)),
            k("PIPE", "Pipe resonance", 0.4, (0.0, 1.0)),
            k("MOTOR", "Drive motor drone", 0.45, (0.0, 1.0)),
        ],
        ramp: (0.05, 0.45),
        trim: 1.41,
    },
    MachineSpec {
        name: "CRANKSHAFT",
        blurb: "Combustion engine: firing pulses through an exhaust pipe, block resonance and valve tick",
        knobs: [
            k("RPM", "Engine speed", 0.3, (0.05, 0.75)),
            ks("CYL", "Cylinders", CYL_NAMES, 3, (0, 7)),
            k(
                "ROUGH",
                "Roughness: uneven firing and misfires",
                0.3,
                (0.0, 0.9),
            ),
            k("EXHAUST", "Exhaust pipe resonance", 0.35, (0.0, 1.0)),
            k(
                "KNOCK",
                "Valve train tick and diesel knock",
                0.35,
                (0.0, 1.0),
            ),
            k(
                "LOAD",
                "Throttle load: sharper, louder firing",
                0.45,
                (0.1, 1.0),
            ),
        ],
        ramp: (0.7, 0.72),
        trim: 1.58,
    },
    MachineSpec {
        name: "TRANSFORMER",
        blurb: "Mains transformer: magnetostriction hum at twice the mains, lamination buzz and corona crackle",
        knobs: [
            k(
                "MAINS",
                "Mains frequency (hum at twice this)",
                1.0 / 3.0,
                (0.0, 1.0),
            ),
            k(
                "SATURATE",
                "Core saturation: more even harmonics",
                0.45,
                (0.0, 1.0),
            ),
            k("BUZZ", "Loose lamination buzz", 0.25, (0.0, 0.9)),
            k(
                "ARC",
                "Corona crackle at the voltage peaks",
                0.1,
                (0.0, 0.8),
            ),
            k(
                "BEAT",
                "A second transformer beating against it",
                0.3,
                (0.0, 1.0),
            ),
            k("TANK", "Tank resonance", 0.4, (0.0, 1.0)),
        ],
        ramp: (0.3, 0.4),
        trim: 1.68,
    },
    MachineSpec {
        name: "SERVO",
        blurb: "Servo axis: whining moves on the beat, gear mesh, an end-stop clunk, PWM whine and hunting at rest",
        knobs: [
            ks("RATE", "Move cycle (tempo synced)", SYNC_NAMES, 3, (1, 8)),
            k("SPEED", "Motor whine pitch at full speed", 0.5, (0.1, 1.0)),
            k("GEAR", "Gear mesh whine and grit", 0.4, (0.0, 1.0)),
            k(
                "TRAVEL",
                "Move length (share of the cycle)",
                0.55,
                (0.15, 0.95),
            ),
            k("CLUNK", "End-stop clunk", 0.55, (0.0, 1.0)),
            k(
                "HOLD",
                "Holding: PWM whine at rest and hunting after a stop",
                0.35,
                (0.0, 1.0),
            ),
        ],
        ramp: (0.1, 0.3),
        trim: 2.0,
    },
    MachineSpec {
        name: "PRESS",
        blurb: "Hydraulic stamping press: pressure build-up, the stamp, the ringing die and the return",
        knobs: [
            ks("RATE", "Stamp cycle (tempo synced)", SYNC_NAMES, 3, (1, 6)),
            k("WEIGHT", "Weight of the ram", 0.55, (0.1, 1.0)),
            k("RING", "Die and workpiece ring time", 0.4, (0.0, 1.0)),
            k(
                "HYDRAULIC",
                "Hydraulic whine and hiss before the stamp",
                0.45,
                (0.0, 1.0),
            ),
            k(
                "MATERIAL",
                "Workpiece: membrane … bar … plate … bell",
                0.7,
                (0.3, 1.0),
            ),
            k("RETURN", "Return stroke and latch", 0.35, (0.0, 1.0)),
        ],
        ramp: (0.05, 0.3),
        trim: 2.0,
    },
    MachineSpec {
        name: "TURBINE",
        blurb: "Turbine or big fan: blade-pass whine, roar, buzz-saw shaft tones and imbalance flutter",
        knobs: [
            k("SPEED", "Shaft speed", 0.55, (0.15, 0.95)),
            ks(
                "BLADES",
                "Blade count (blade-pass tone)",
                BLADE_NAMES,
                5,
                (0, 9),
            ),
            k("WHINE", "Blade-pass whine", 0.4, (0.05, 0.9)),
            k("ROAR", "Broadband roar", 0.55, (0.1, 1.0)),
            k("BUZZSAW", "Buzz-saw shaft-order tones", 0.2, (0.0, 0.8)),
            k("FLUTTER", "Imbalance flutter", 0.2, (0.0, 0.9)),
        ],
        ramp: (0.82, 0.85),
        trim: 2.24,
    },
    MachineSpec {
        name: "GRINDER",
        blurb: "Gearbox and mill: gear-mesh tone with wear sidebands, grit, chatter and a resonant housing",
        knobs: [
            k("SPEED", "Shaft speed", 0.5, (0.1, 0.95)),
            ks(
                "TEETH",
                "Teeth (gear-mesh frequency)",
                TEETH_NAMES,
                5,
                (0, 9),
            ),
            k(
                "WEAR",
                "Wear: sidebands and a broken tooth",
                0.35,
                (0.0, 1.0),
            ),
            k("GRIT", "Grinding grit", 0.4, (0.0, 1.0)),
            k("CHATTER", "Stick-slip chatter", 0.2, (0.0, 0.9)),
            k("HOUSING", "Housing resonance", 0.45, (0.0, 1.0)),
        ],
        ramp: (0.6, 0.68),
        trim: 1.19,
    },
    MachineSpec {
        name: "WELDER",
        blurb: "Arc welder: mains buzz, crackling arc, sizzle and spatter, optionally pulsed",
        knobs: [
            k("BUZZ", "Arc mains buzz", 0.25, (0.0, 0.8)),
            k("CRACKLE", "Short-circuit crackle rate", 0.55, (0.1, 1.0)),
            k("SIZZLE", "Sizzle", 0.4, (0.0, 1.0)),
            k("HEAT", "Heat: brightness of the arc", 0.5, (0.0, 1.0)),
            k("SPATTER", "Spatter pops and sparks", 0.3, (0.0, 1.0)),
            k("PULSE", "Pulsed welding rate", 0.0, (0.0, 1.0)),
        ],
        ramp: (0.12, 0.2),
        trim: 3.55,
    },
    MachineSpec {
        name: "STEAM",
        blurb: "Steam vent and boiler: hiss, a breathy whistle, chuffing, rumble and sputtering condensate",
        knobs: [
            k("PRESSURE", "Steam pressure", 0.6, (0.2, 1.0)),
            k("VENT", "Vent tone", 0.75, (0.2, 1.0)),
            k("WHISTLE", "Whistle", 0.15, (0.0, 0.8)),
            ks(
                "CHUFF",
                "Chuffing (tempo synced) or a continuous vent",
                CHUFF_NAMES,
                0,
                (0, 6),
            ),
            k("RUMBLE", "Boiler rumble", 0.15, (0.0, 0.8)),
            k("SPUTTER", "Sputtering condensate", 0.2, (0.0, 1.0)),
        ],
        ramp: (0.35, 0.5),
        trim: 2.51,
    },
];

pub fn spec(machine: u32) -> &'static MachineSpec {
    &SPECS[(machine as usize).min(MACHINE_COUNT - 1)]
}

/// Stepped knob position → its index.
pub fn step_of(machine: u32, slot: usize, v: f32) -> usize {
    let n = spec(machine).knobs[slot].steps.len().max(2);
    map::step_index(v, n)
}

/// Read-out of a machine knob.
pub fn knob_display(machine: u32, slot: usize, v: f32) -> String {
    let ks = &spec(machine).knobs[slot];
    if !ks.steps.is_empty() {
        let name = ks.steps[step_of(machine, slot, v)];
        return match (machine, slot) {
            (MACHINE_CRANKSHAFT, 1) => format!("{name} CYL"),
            (MACHINE_TURBINE, 1) => format!("{name} BLADES"),
            (MACHINE_GRINDER, 1) => format!("{name} TEETH"),
            _ => name.to_string(),
        };
    }
    match (machine, slot) {
        (MACHINE_CONVEYOR, 0) => fmt_hz(conveyor::hum_hz(v)),
        (MACHINE_PUMP, 4) => fmt_hz(pump::pipe_hz(v)),
        (MACHINE_CRANKSHAFT, 0) => format!("{:.0} RPM", crankshaft::rpm(v)),
        (MACHINE_CRANKSHAFT, 3) => fmt_hz(crankshaft::exhaust_hz(v)),
        (MACHINE_TRANSFORMER, 0) => format!("{:.1} Hz", transformer::mains_hz(v)),
        (MACHINE_TRANSFORMER, 4) => {
            if v <= 0.0 {
                "OFF".into()
            } else {
                format!("{:.2} Hz", transformer::beat_hz(v))
            }
        }
        (MACHINE_SERVO, 1) => fmt_hz(servo::whine_hz(v)),
        (MACHINE_SERVO, 3) => format!("{:.0}%", servo::travel(v) * 100.0),
        (MACHINE_PRESS, 1) => fmt_hz(press::thump_hz(v)),
        (MACHINE_PRESS, 2) => fmt_secs(press::ring_seconds(v)),
        (MACHINE_PRESS, 4) => press::material_name(v),
        (MACHINE_TURBINE, 0) => format!("{:.0} RPM", turbine::shaft_hz(v) * 60.0),
        (MACHINE_GRINDER, 0) => format!("{:.0} RPM", grinder::shaft_hz(v) * 60.0),
        (MACHINE_GRINDER, 5) => fmt_hz(grinder::housing_hz(v)),
        (MACHINE_WELDER, 3) => fmt_hz(welder::heat_hz(v)),
        (MACHINE_WELDER, 5) => match welder::pulse_hz(v) {
            Some(hz) => fmt_hz(hz),
            None => "OFF".into(),
        },
        (MACHINE_STEAM, 1) => fmt_hz(steam::vent_hz(v)),
        _ => fmt_pct(v),
    }
}

/// Control values for one block.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub fs: f32,
    /// Machine knob positions after VARIANCE and smoothing.
    pub k: [f32; KNOBS],
    /// Seconds per beat.
    pub beat_s: f32,
    /// Whether an active period is running (the sequencer or a held RUN).
    pub gate: bool,
}

impl Ctx {
    pub fn new(fs: f32) -> Self {
        Self {
            fs,
            k: [0.5; KNOBS],
            beat_s: 0.5,
            gate: false,
        }
    }
}

/// All machines, each with its own state; only the selected one runs. Kept
/// together so a track can switch machines (MUTATE does, on the audio
/// thread) without allocating.
#[derive(Clone, Debug)]
pub struct Machines {
    pub conveyor: conveyor::Conveyor,
    pub pump: pump::Pump,
    pub crankshaft: crankshaft::Crankshaft,
    pub transformer: transformer::Transformer,
    pub servo: servo::Servo,
    pub press: press::Press,
    pub turbine: turbine::Turbine,
    pub grinder: grinder::Grinder,
    pub welder: welder::Welder,
    pub steam: steam::Steam,
}

impl Machines {
    pub fn new(fs: f32) -> Self {
        Self {
            conveyor: conveyor::Conveyor::new(),
            pump: pump::Pump::new(fs),
            crankshaft: crankshaft::Crankshaft::new(fs),
            transformer: transformer::Transformer::new(),
            servo: servo::Servo::new(),
            press: press::Press::new(),
            turbine: turbine::Turbine::new(),
            grinder: grinder::Grinder::new(),
            welder: welder::Welder::new(),
            steam: steam::Steam::new(),
        }
    }

    pub fn reset(&mut self, m: u32) {
        match m {
            MACHINE_CONVEYOR => self.conveyor.reset(),
            MACHINE_PUMP => self.pump.reset(),
            MACHINE_CRANKSHAFT => self.crankshaft.reset(),
            MACHINE_TRANSFORMER => self.transformer.reset(),
            MACHINE_SERVO => self.servo.reset(),
            MACHINE_PRESS => self.press.reset(),
            MACHINE_TURBINE => self.turbine.reset(),
            MACHINE_GRINDER => self.grinder.reset(),
            MACHINE_WELDER => self.welder.reset(),
            _ => self.steam.reset(),
        }
    }

    pub fn control(&mut self, m: u32, c: &Ctx) {
        match m {
            MACHINE_CONVEYOR => self.conveyor.control(c),
            MACHINE_PUMP => self.pump.control(c),
            MACHINE_CRANKSHAFT => self.crankshaft.control(c),
            MACHINE_TRANSFORMER => self.transformer.control(c),
            MACHINE_SERVO => self.servo.control(c),
            MACHINE_PRESS => self.press.control(c),
            MACHINE_TURBINE => self.turbine.control(c),
            MACHINE_GRINDER => self.grinder.control(c),
            MACHINE_WELDER => self.welder.control(c),
            _ => self.steam.control(c),
        }
    }

    /// A new active period begins.
    pub fn start(&mut self, m: u32, c: &Ctx) {
        match m {
            MACHINE_CONVEYOR => {}
            MACHINE_PUMP => self.pump.start(c),
            MACHINE_CRANKSHAFT => {}
            MACHINE_TRANSFORMER => self.transformer.start(c),
            MACHINE_SERVO => self.servo.start(c),
            MACHINE_PRESS => self.press.start(c),
            MACHINE_TURBINE => {}
            MACHINE_GRINDER => {}
            MACHINE_WELDER => self.welder.start(c),
            _ => self.steam.start(c),
        }
    }

    /// One sample at drive `d`.
    #[inline]
    pub fn tick(&mut self, m: u32, c: &Ctx, d: f32, rng: &mut Noise) -> f32 {
        let y = match m {
            MACHINE_CONVEYOR => self.conveyor.tick(c, d, rng),
            MACHINE_PUMP => self.pump.tick(c, d, rng),
            MACHINE_CRANKSHAFT => self.crankshaft.tick(c, d, rng),
            MACHINE_TRANSFORMER => self.transformer.tick(c, d, rng),
            MACHINE_SERVO => self.servo.tick(c, d, rng),
            MACHINE_PRESS => self.press.tick(c, d, rng),
            MACHINE_TURBINE => self.turbine.tick(c, d, rng),
            MACHINE_GRINDER => self.grinder.tick(c, d, rng),
            MACHINE_WELDER => self.welder.tick(c, d, rng),
            _ => self.steam.tick(c, d, rng),
        };
        y * spec(m).trim
    }

    /// Whether the machine has gone quiet (no drive and nothing ringing), so
    /// the track can skip it.
    pub fn is_quiet(&self, m: u32) -> bool {
        match m {
            MACHINE_PUMP => self.pump.is_quiet(),
            MACHINE_CRANKSHAFT => self.crankshaft.is_quiet(),
            MACHINE_SERVO => self.servo.is_quiet(),
            MACHINE_PRESS => self.press.is_quiet(),
            MACHINE_WELDER => self.welder.is_quiet(),
            MACHINE_STEAM => self.steam.is_quiet(),
            MACHINE_GRINDER => self.grinder.is_quiet(),
            MACHINE_CONVEYOR => self.conveyor.is_quiet(),
            MACHINE_TRANSFORMER => self.transformer.is_quiet(),
            _ => self.turbine.is_quiet(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::default_patch;

    #[test]
    fn specs_are_consistent() {
        assert_eq!(MACHINE_NAMES.len(), MACHINE_COUNT);
        for (i, s) in SPECS.iter().enumerate() {
            assert_eq!(s.name, MACHINE_NAMES[i]);
            for k in s.knobs {
                assert!((0.0..=1.0).contains(&k.default), "{} {}", s.name, k.label);
                assert!(k.rnd.0 <= k.rnd.1 && k.rnd.0 >= 0.0 && k.rnd.1 <= 1.0);
            }
        }
    }

    /// Every machine at its defaults, fully driven, makes a sane sound, and
    /// goes quiet when the drive is gone.
    #[test]
    fn every_machine_sounds_and_settles() {
        let fs = 48_000.0;
        for m in 0..MACHINE_COUNT as u32 {
            let mut ms = Machines::new(fs);
            let mut rng = Noise::new(m + 1);
            let patch = default_patch(m);
            let mut c = Ctx::new(fs);
            for (slot, p) in crate::params::KNOB_PARAMS.iter().enumerate() {
                c.k[slot] = patch[p.index()];
            }
            c.gate = true;
            ms.start(m, &c);
            let mut sum = 0.0f64;
            let mut peak = 0.0f32;
            let n = (fs * 2.0) as usize;
            for i in 0..n {
                if i % 32 == 0 {
                    ms.control(m, &c);
                }
                let y = ms.tick(m, &c, 1.0, &mut rng);
                assert!(y.is_finite(), "{}", MACHINE_NAMES[m as usize]);
                sum += (y * y) as f64;
                peak = peak.max(y.abs());
            }
            let rms = (sum / n as f64).sqrt();
            assert!(
                rms > 0.01 && peak < 4.0,
                "{}: rms {rms} peak {peak}",
                MACHINE_NAMES[m as usize]
            );
            c.gate = false;
            let mut late = 0.0f32;
            for i in 0..(fs * 8.0) as usize {
                if i % 32 == 0 {
                    ms.control(m, &c);
                }
                let y = ms.tick(m, &c, 0.0, &mut rng);
                if i > (fs * 6.0) as usize {
                    late = late.max(y.abs());
                }
            }
            assert!(
                late < 1e-3,
                "{} keeps sounding: {late}",
                MACHINE_NAMES[m as usize]
            );
            assert!(
                ms.is_quiet(m),
                "{} never reports quiet",
                MACHINE_NAMES[m as usize]
            );
        }
    }
}
