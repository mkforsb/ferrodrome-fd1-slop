//! Track sequences: up to 128 on/off steps in a `u128`, and the active
//! periods they make when looped.
//!
//! A sequence loops, so a run of steps at the end continues into a run at the
//! start: with the last two and first two of 16 steps on, the machine runs
//! for one four-step period that starts at step 15, not two short ones.

use crate::params::MAX_STEPS;

#[inline]
pub fn get(steps: u128, s: usize) -> bool {
    s < MAX_STEPS && steps >> s & 1 == 1
}

#[inline]
pub fn set(steps: &mut u128, s: usize, on: bool) {
    if s < MAX_STEPS {
        if on {
            *steps |= 1 << s;
        } else {
            *steps &= !(1 << s);
        }
    }
}

/// The first `len` steps.
pub fn mask(len: usize) -> u128 {
    if len >= MAX_STEPS {
        u128::MAX
    } else {
        (1u128 << len) - 1
    }
}

/// Active periods of the looped sequence as `(start, length)`, in order of
/// their start. A period crossing the loop point starts near the end and
/// wraps. If every step is on, it is one endless period `(0, len)`.
pub fn periods(steps: u128, len: usize) -> Vec<(usize, usize)> {
    let len = len.clamp(1, MAX_STEPS);
    let steps = steps & mask(len);
    if steps == 0 {
        return Vec::new();
    }
    if steps == mask(len) {
        return vec![(0, len)];
    }
    // Start scanning just after an off step so no run is split.
    let off = (0..len).find(|&s| !get(steps, s)).unwrap();
    let mut out = Vec::new();
    let mut run: Option<(usize, usize)> = None;
    for i in 1..=len {
        let s = (off + i) % len;
        if get(steps, s) {
            run = Some(match run {
                Some((start, n)) => (start, n + 1),
                None => (s, 1),
            });
        } else if let Some(r) = run.take() {
            out.push(r);
        }
    }
    if let Some(r) = run {
        out.push(r);
    }
    out.sort_by_key(|&(s, _)| s);
    out
}

/// Whether step `s` is on and its neighbours (looping) are on: for drawing
/// periods as joined bars.
pub fn joins(steps: u128, len: usize, s: usize) -> (bool, bool) {
    let len = len.clamp(1, MAX_STEPS);
    if !get(steps, s) || len == 1 {
        return (false, false);
    }
    let prev = (s + len - 1) % len;
    let next = (s + 1) % len;
    (get(steps, prev), get(steps, next))
}

/// Rotate the first `len` steps by `by` (positive = later).
pub fn rotate(steps: u128, len: usize, by: i32) -> u128 {
    let len = len.clamp(1, MAX_STEPS);
    let mut out = steps & !mask(len);
    for s in 0..len {
        if get(steps, s) {
            let t = (s as i64 + by as i64).rem_euclid(len as i64) as usize;
            out |= 1 << t;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from(bits: &[usize]) -> u128 {
        let mut s = 0;
        for &b in bits {
            set(&mut s, b, true);
        }
        s
    }

    #[test]
    fn a_run_across_the_loop_point_is_one_period() {
        let s = from(&[14, 15, 0, 1, 5]);
        assert_eq!(periods(s, 16), vec![(5, 1), (14, 4)]);
        assert_eq!(joins(s, 16, 15), (true, true));
        assert_eq!(joins(s, 16, 0), (true, true));
        assert_eq!(joins(s, 16, 1), (true, false));
        assert_eq!(joins(s, 16, 14), (false, true));
    }

    #[test]
    fn steps_past_the_length_are_ignored() {
        let s = from(&[0, 1, 20]);
        assert_eq!(periods(s, 16), vec![(0, 2)]);
        assert_eq!(periods(from(&[]), 16), vec![]);
        assert_eq!(periods(mask(16), 16), vec![(0, 16)]);
        assert_eq!(periods(u128::MAX, 128), vec![(0, 128)]);
    }

    #[test]
    fn rotate_wraps_within_the_length() {
        let s = from(&[0, 15]);
        assert_eq!(rotate(s, 16, 1), from(&[1, 0]));
        assert_eq!(rotate(s, 16, -1), from(&[15, 14]));
        assert_eq!(rotate(from(&[127]), 128, 1), from(&[0]));
    }
}
