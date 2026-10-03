//! PRESS: a hydraulic stamping press, one stamp per RATE (tempo synced).
//!
//! The stamp lands on the beat: a heavy pitch-dropping thump (WEIGHT), a hard
//! strike into a 16-mode resonator for the die and workpiece (the XK-1's
//! MODAL bank, its material morphed by MATERIAL and its ring time set by
//! RING) and a short noise burst. Before each stamp the hydraulics build
//! pressure: a rising pump whine and hiss (HYDRAULIC). After it the ram
//! returns: a falling hiss and a latch click (RETURN).

use super::Ctx;
use super::modal::{MATERIALS, Modal, ModalCtl, material_segment};
use super::parts::{Cycle, Decay, Mode, Thump, advance, svf_bp};
use crate::params::map;
use crate::util::{Noise, Svf, SvfCoeffs, sin_turns};

pub fn thump_hz(weight: f32) -> f32 {
    map::expo(weight, 95.0, 34.0)
}

pub fn ring_seconds(v: f32) -> f32 {
    map::expo(v, 0.12, 4.0)
}

/// Fundamental of the die/workpiece ring.
fn ring_hz(weight: f32) -> f32 {
    map::expo(weight, 900.0, 260.0)
}

const MATERIAL_NAMES: [&str; 6] = ["MEMBRANE", "STRING", "MARIMBA", "BAR", "PLATE", "BELL"];

pub fn material_name(v: f32) -> String {
    let (i, f) = material_segment(v);
    if f < 0.05 {
        MATERIAL_NAMES[i].into()
    } else if f > 0.95 {
        MATERIAL_NAMES[(i + 1).min(MATERIALS.len() - 1)].into()
    } else {
        format!(
            "{}>{}",
            &MATERIAL_NAMES[i][..3],
            &MATERIAL_NAMES[i + 1][..3]
        )
    }
}

/// Longest pressure build-up and return, in seconds.
const MAX_BUILD_S: f32 = 1.1;
const MAX_RETURN_S: f32 = 0.7;
/// Mallet contact of the stamp.
const STAMP_SAMPLES: u32 = 20;

#[derive(Clone, Debug)]
pub struct Press {
    cycle: Cycle,
    modal: Modal,
    thump: Thump,
    burst: Decay,
    burst_bp: Svf,
    burst_c: SvfCoeffs,
    stamp_t: u32,
    stamp_amp: f32,
    hiss: Svf,
    hiss_c: SvfCoeffs,
    coef_n: u8,
    whine_phase: f32,
    latch: Mode,
    latched: bool,
    stamped: bool,
    k: [f32; 6],
    d: f32,
    power: f32,
}

impl Press {
    pub fn new() -> Self {
        Self {
            cycle: Cycle::default(),
            modal: Modal::new(),
            thump: Thump::default(),
            burst: Decay::default(),
            burst_bp: Svf::default(),
            burst_c: SvfCoeffs::new(1800.0, 0.8, 48_000.0),
            stamp_t: STAMP_SAMPLES,
            stamp_amp: 0.0,
            hiss: Svf::default(),
            hiss_c: SvfCoeffs::new(800.0, 1.0, 48_000.0),
            coef_n: 0,
            whine_phase: 0.0,
            latch: Mode::default(),
            latched: true,
            stamped: false,
            k: [0.5; 6],
            d: 0.0,
            power: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.cycle.reset();
        self.modal.reset();
        self.thump.reset();
        self.burst.reset();
        self.latch.reset();
        self.hiss.reset();
        self.stamp_t = STAMP_SAMPLES;
        self.stamped = false;
    }

    pub fn control(&mut self, c: &Ctx) {
        self.k = c.k;
        self.cycle.set(map::sync_beats(c.k[0]), c.beat_s, c.fs);
        self.modal.control(&ModalCtl {
            fs: c.fs,
            hz: ring_hz(c.k[1]),
            x: c.k[4],
            y: 0.33,
            z: 0.35,
            decay_s: ring_seconds(c.k[2]),
        });
        self.latch.set(2600.0, 0.02, c.fs);
        self.burst_c = SvfCoeffs::new(1800.0, 0.8, c.fs);
    }

    pub fn start(&mut self, c: &Ctx) {
        self.cycle.start(c.fs);
        self.stamped = false;
    }

    fn stamp(&mut self, fs: f32, rng: &mut Noise) {
        let p = self.power;
        let w = self.k[1];
        let v = p * (0.88 + 0.12 * rng.uniform());
        self.thump.trigger(
            v * (0.6 + 0.4 * w),
            thump_hz(w),
            3.0,
            0.03,
            0.25 + 0.3 * w,
            fs,
        );
        self.stamp_t = 0;
        self.stamp_amp = v;
        self.burst.trigger(v, 0.09, fs);
        self.latched = false;
        self.stamped = true;
    }

    #[inline]
    pub fn tick(&mut self, c: &Ctx, d: f32, rng: &mut Noise) -> f32 {
        let fs = c.fs;
        self.d = d;
        self.power = d;
        if self.cycle.tick(c.gate) {
            self.stamp(fs, rng);
        }
        let period = self.cycle.period(fs);
        let phase = self.cycle.phase();

        // The strike into the die: a short raised-cosine pulse.
        let mut force = 0.0;
        if self.stamp_t < STAMP_SAMPLES {
            let x = self.stamp_t as f32 / STAMP_SAMPLES as f32;
            force = (0.5 - 0.5 * crate::util::cos_turns(x))
                * self.stamp_amp
                * (2.0 / STAMP_SAMPLES as f32);
            self.stamp_t += 1;
        }
        let ring = self.modal.tick(force);
        let burst =
            svf_bp(&mut self.burst_bp, &self.burst_c, rng.sample(), 0.8) * self.burst.tick();

        // Hydraulics: pressure builds over the end of the cycle (only while
        // the press is running, so it leads into the next stamp).
        let build_from = 1.0 - (MAX_BUILD_S / period).min(0.45);
        let mut hyd = 0.0;
        if c.gate && self.stamped && phase >= build_from && self.k[3] > 0.0 {
            let x = (phase - build_from) / (1.0 - build_from);
            let hz = 140.0 + 140.0 * x;
            advance(&mut self.whine_phase, hz / fs);
            if self.coef_n == 0 {
                self.hiss_c = SvfCoeffs::new(450.0 + 1200.0 * x, 1.2, fs);
                self.coef_n = 32;
            }
            self.coef_n -= 1;
            let hiss = svf_bp(&mut self.hiss, &self.hiss_c, rng.sample(), 1.2);
            let env = x * x * (3.0 - 2.0 * x);
            hyd = (0.4 * sin_turns(self.whine_phase)
                + 0.25 * sin_turns(2.0 * self.whine_phase)
                + hiss)
                * env
                * self.k[3]
                * d;
        }
        // Return stroke: a falling hiss after the stamp, then the latch.
        let ret_len = (MAX_RETURN_S / period).min(0.3);
        let (ret_from, ret_to) = (0.06, 0.06 + ret_len);
        let mut ret = 0.0;
        if self.stamped && self.k[5] > 0.0 && phase >= ret_from && phase < ret_to && !self.latched {
            let x = (phase - ret_from) / ret_len;
            if self.coef_n == 0 {
                self.hiss_c = SvfCoeffs::new(900.0 - 500.0 * x, 1.0, fs);
                self.coef_n = 32;
            }
            self.coef_n -= 1;
            ret = svf_bp(&mut self.hiss, &self.hiss_c, rng.sample(), 1.0)
                * (1.0 - x)
                * x.sqrt()
                * self.k[5]
                * 0.7;
        }
        if !self.latched && phase >= ret_to {
            self.latched = true;
            self.latch.tick(self.k[5] * self.power);
        }
        let latch = self.latch.tick(0.0);

        0.45 * self.thump.tick(fs)
            + 0.6 * ring
            + 0.5 * burst
            + 0.18 * hyd
            + 0.3 * ret
            + 0.15 * latch
    }

    pub fn is_quiet(&self) -> bool {
        self.d <= 0.0 && self.modal.energy() < 1e-9 && self.burst.value() < 1e-6
    }
}

impl Default for Press {
    fn default() -> Self {
        Self::new()
    }
}
