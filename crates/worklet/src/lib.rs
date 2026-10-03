//! C ABI over the engine for the AudioWorkletProcessor (`worklet.js`).
//!
//! The module imports nothing, so the worklet can instantiate it with an empty
//! import object. Each worklet node owns one engine handle.

#[cfg(test)]
use ferrodrome_dsp::Status;
use ferrodrome_dsp::{Engine, GlobalParam, Param};

/// Frames per render call; matches the WebAudio render quantum.
pub const BLOCK: usize = 128;

pub struct Worklet {
    engine: Engine,
    left: [f32; BLOCK],
    right: [f32; BLOCK],
    status: Vec<f32>,
}

#[unsafe(no_mangle)]
pub extern "C" fn fd_new(sample_rate: f32) -> *mut Worklet {
    let engine = Engine::new(sample_rate);
    let status = engine.status().to_wire().to_vec();
    Box::into_raw(Box::new(Worklet {
        engine,
        left: [0.0; BLOCK],
        right: [0.0; BLOCK],
        status,
    }))
}

/// # Safety
/// `w` must come from [`fd_new`] and not have been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fd_free(w: *mut Worklet) {
    if !w.is_null() {
        drop(unsafe { Box::from_raw(w) });
    }
}

/// # Safety
/// `w` must come from [`fd_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fd_set_param(w: *mut Worklet, track: u32, id: u32, value: f32) {
    if let Some(p) = Param::from_id(id) {
        unsafe { &mut *w }
            .engine
            .set_param(track as usize, p, value);
    }
}

/// # Safety
/// `w` must come from [`fd_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fd_set_global(w: *mut Worklet, id: u32, value: f32) {
    if let Some(p) = GlobalParam::from_id(id) {
        unsafe { &mut *w }.engine.set_global(p, value);
    }
}

/// # Safety
/// `w` must come from [`fd_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fd_set_lock(w: *mut Worklet, track: u32, id: u32, locked: u32) {
    if let Some(p) = Param::from_id(id) {
        unsafe { &mut *w }
            .engine
            .set_lock(track as usize, p, locked != 0);
    }
}

/// # Safety
/// `w` must come from [`fd_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fd_set_step(w: *mut Worklet, track: u32, step: u32, on: u32) {
    unsafe { &mut *w }
        .engine
        .set_step(track as usize, step as usize, on != 0);
}

/// # Safety
/// `w` must come from [`fd_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fd_hold(w: *mut Worklet, track: u32, on: u32) {
    unsafe { &mut *w }.engine.hold(track as usize, on != 0);
}

/// # Safety
/// `w` must come from [`fd_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fd_play(w: *mut Worklet, on: u32) {
    unsafe { &mut *w }.engine.play(on != 0);
}

/// Jump smoothed parameters to their targets (after the initial sync).
///
/// # Safety
/// `w` must come from [`fd_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fd_snap(w: *mut Worklet) {
    unsafe { &mut *w }.engine.snap_params();
}

/// Render `frames` (≤ [`BLOCK`]) into the buffers returned by [`fd_left`]/[`fd_right`].
///
/// # Safety
/// `w` must come from [`fd_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fd_render(w: *mut Worklet, frames: u32) {
    let w = unsafe { &mut *w };
    let n = (frames as usize).min(BLOCK);
    w.engine.render(&mut w.left[..n], &mut w.right[..n]);
    w.status.copy_from_slice(&w.engine.status().to_wire()[..]);
}

/// # Safety
/// `w` must come from [`fd_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fd_left(w: *mut Worklet) -> *const f32 {
    unsafe { (*w).left.as_ptr() }
}

/// # Safety
/// `w` must come from [`fd_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fd_right(w: *mut Worklet) -> *const f32 {
    unsafe { (*w).right.as_ptr() }
}

/// The engine status after the last render, [`Status::WIRE_LEN`] floats.
///
/// # Safety
/// `w` must come from [`fd_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fd_status(w: *mut Worklet) -> *const f32 {
    unsafe { (*w).status.as_ptr() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_plays_a_sequence() {
        unsafe {
            let w = fd_new(48_000.0);
            let press = ferrodrome_dsp::params::default_patch(5);
            for p in ferrodrome_dsp::params::ALL_PARAMS {
                fd_set_param(w, 0, p.id(), press[p.index()]);
            }
            fd_set_param(w, 0, Param::Enabled.id(), 1.0);
            fd_set_step(w, 0, 0, 1);
            fd_set_global(w, GlobalParam::Tempo.id(), 120.0);
            fd_play(w, 1);
            let mut energy = 0.0;
            for _ in 0..60 {
                fd_render(w, 128);
                let l = std::slice::from_raw_parts(fd_left(w), 128);
                energy += l.iter().map(|v| v * v).sum::<f32>();
            }
            assert!(energy > 0.01, "{energy}");
            let status = std::slice::from_raw_parts(fd_status(w), Status::WIRE_LEN);
            let s = Status::from_wire(status).unwrap();
            assert!(s.playing && s.step >= 0);
            fd_free(w);
        }
    }
}
