//! GRINDER: a gearbox driving a mill.
//!
//! Every time a pair of teeth meets, the gears ring, so a gearbox sings at
//! the gear-mesh frequency, shaft speed × TEETH, and its harmonics. Wear makes
//! the meshing uneven once per revolution, which modulates the mesh tone and
//! puts sidebands a shaft-speed apart around it; a badly worn gear also
//! clicks once a turn on a broken tooth. GRIT is the grinding: noise made
//! rough by fast random gating. CHATTER is stick-slip: irregular bursts of
//! micro-impacts. Everything rings the gearbox HOUSING. The drive is the
//! shaft speed.

use super::parts::{Mode, Pow, Wander, advance, harmonics, svf_bp};
use super::{Ctx, MACHINE_GRINDER, TEETH_COUNTS, step_of};
use crate::params::map;
use crate::util::{Noise, Svf, SvfCoeffs, sin_turns};

pub fn shaft_hz(v: f32) -> f32 {
    map::expo(v, 1.5, 32.0)
}

pub fn housing_hz(v: f32) -> f32 {
    map::expo(v, 160.0, 3200.0)
}

const MESH_AMPS: [f32; 8] = [1.0, 0.55, 0.4, 0.3, 0.22, 0.18, 0.12, 0.1];

#[derive(Clone, Debug)]
pub struct Grinder {
    pw: Pow,
    shaft_phase: f32,
    mesh_phase: f32,
    teeth: f32,
    housing: [Mode; 2],
    housing_bp: Svf,
    housing_c: SvfCoeffs,
    grit_bp: Svf,
    grit_c: SvfCoeffs,
    floor: Svf,
    floor_c: SvfCoeffs,
    rough: f32,
    rough_target: f32,
    rough_phase: f32,
    chatter_phase: f32,
    chatter_left: u32,
    chatter_gap: u32,
    jitter: Wander,
    k: [f32; 6],
    d: f32,
}

impl Grinder {
    pub fn new() -> Self {
        let fs = 48_000.0;
        Self {
            pw: Pow::default(),
            shaft_phase: 0.0,
            mesh_phase: 0.0,
            teeth: 23.0,
            housing: Default::default(),
            housing_bp: Svf::default(),
            housing_c: SvfCoeffs::new(800.0, 3.0, fs),
            grit_bp: Svf::default(),
            grit_c: SvfCoeffs::new(1200.0, 0.8, fs),
            floor: Svf::default(),
            floor_c: SvfCoeffs::new(5000.0, 0.6, fs),
            rough: 0.0,
            rough_target: 0.0,
            rough_phase: 0.0,
            chatter_phase: 0.0,
            chatter_left: 0,
            chatter_gap: 0,
            jitter: Wander::default(),
            k: [0.5; 6],
            d: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.housing.iter_mut().for_each(Mode::reset);
        self.housing_bp.reset();
        self.grit_bp.reset();
        self.chatter_left = 0;
    }

    pub fn control(&mut self, c: &Ctx) {
        let fs = c.fs;
        self.k = c.k;
        self.teeth = TEETH_COUNTS[step_of(MACHINE_GRINDER, 1, c.k[1])] as f32;
        let h = housing_hz(c.k[5]);
        self.housing[0].set(h, 0.07, fs);
        self.housing[1].set(h * 1.47, 0.05, fs);
        self.housing_c = SvfCoeffs::new(h, 3.0, fs);
        self.grit_c = SvfCoeffs::new((h * 1.6).min(0.4 * fs), 0.8, fs);
        self.floor_c = SvfCoeffs::new((5000.0 * self.d.max(0.1)).min(0.4 * fs), 0.6, fs);
    }

    #[inline]
    pub fn tick(&mut self, c: &Ctx, d: f32, rng: &mut Noise) -> f32 {
        let fs = c.fs;
        self.d = d;
        if d <= 0.0 && self.housing.iter().all(|m| m.energy() < 1e-10) {
            return 0.0;
        }
        let k = self.k;
        let wear = k[2];
        let shaft = shaft_hz(k[0]) * d;
        let sinc = shaft / fs;
        let turned = advance(&mut self.shaft_phase, sinc);
        let s = sin_turns(self.shaft_phase);
        let minc = sinc * self.teeth;
        advance(&mut self.mesh_phase, minc);
        // Phase modulation once a turn: sidebands a shaft-speed apart.
        let mp = self.mesh_phase + 0.12 * wear * s;
        let mesh = harmonics(mp, minc, &MESH_AMPS, 0.45) * (1.0 + 0.8 * wear * s);

        let mut kick = 0.0;
        if turned && wear > 0.4 && d > 0.05 {
            kick += (wear - 0.4) * 1.6;
        }
        // Grit: noise roughened by random gating at about half the mesh rate.
        if advance(&mut self.rough_phase, (0.5 * minc).max(40.0 / fs)) {
            let u = rng.uniform();
            self.rough_target = u * u;
        }
        self.rough += (self.rough_target - self.rough) * 0.05;
        let grit =
            svf_bp(&mut self.grit_bp, &self.grit_c, rng.sample(), 0.8) * (0.3 + 1.4 * self.rough);

        // Chatter: bursts of micro-impacts at an irregular 6–20 Hz.
        let ch = k[4];
        if ch > 0.0 && d > 0.05 {
            let rate = (6.0 + 14.0 * ch) * (1.0 + 0.4 * self.jitter.tick(2.0, fs, rng)) * d;
            if advance(&mut self.chatter_phase, rate / fs) && rng.chance(0.4 + 0.6 * ch) {
                self.chatter_left = 3 + rng.below(4) as u32;
                self.chatter_gap = 0;
            }
            if self.chatter_left > 0 {
                if self.chatter_gap == 0 {
                    kick += ch * (0.4 + 0.6 * rng.uniform());
                    self.chatter_left -= 1;
                    self.chatter_gap = (fs * (0.001 + 0.002 * rng.uniform())) as u32;
                } else {
                    self.chatter_gap -= 1;
                }
            }
        }
        let ring = self.housing[0].tick(kick) + 0.6 * self.housing[1].tick(0.7 * kick);

        // Broadband mechanical noise under it all.
        let floor = self.floor.process(rng.sample(), &self.floor_c).0;
        let src = 0.3 * mesh + 0.25 * grit * k[3] + 0.2 * floor * (0.3 + k[3]);
        let housed = svf_bp(&mut self.housing_bp, &self.housing_c, src, 3.0);
        let power = self.pw.get(d, 0.8);
        (src * 0.6 + housed * 0.6) * power + 0.18 * ring
    }

    pub fn is_quiet(&self) -> bool {
        self.d <= 0.0 && self.housing.iter().all(|m| m.energy() < 1e-10)
    }
}

impl Default for Grinder {
    fn default() -> Self {
        Self::new()
    }
}
