//! The panel: transport, the shift plan (one step row per machine), the
//! machine floor (one card per machine) and the master effects.
//!
//! Every knob, slider and button that RND or MUTATE can change locks with
//! Ctrl-click (a padlock appears); Ctrl-click a track's steps to lock its
//! sequence.

use dioxus::prelude::*;
use ferrodrome_dsp::machines::{MACHINE_COUNT, MACHINE_NAMES, spec};
use ferrodrome_dsp::params::{FILTER_NAMES, KNOB_PARAMS, MAX_TRACKS, STEPS_PER_BEAT};
use ferrodrome_dsp::{GlobalParam, Param, seq};

use crate::audio::AudioStatus;
use crate::state::{Drag, Menu, Synth, TRACK_COUNT_LABEL, Target};

const STYLE: &str = include_str!("../assets/style.css");

/// Pixels of vertical drag for a knob's full travel.
const KNOB_SPAN_PX: f64 = 200.0;
/// Steps per bar.
const BAR: usize = 4 * STEPS_PER_BEAT;

/// Keyboard: hold the n-th machine running.
const HOLD_KEYS: [Code; MAX_TRACKS] = [
    Code::Digit1,
    Code::Digit2,
    Code::Digit3,
    Code::Digit4,
    Code::Digit5,
    Code::Digit6,
    Code::Digit7,
    Code::Digit8,
];

fn lock_click(e: &PointerEvent) -> bool {
    let m = e.modifiers();
    m.ctrl() || m.meta()
}

#[component]
pub fn App() -> Element {
    let synth = use_context_provider(Synth::new);
    let drag = synth.drag;

    let s_move = synth.clone();
    let s_up = synth.clone();
    let s_down = synth.clone();
    let s_key = synth.clone();
    let s_keyup = synth.clone();
    let s_adopt = synth.clone();
    // Follow MUTATE: the engine reports each mutated patch, the knobs move.
    use_effect(move || {
        let status = *s_adopt.audio.meters.status.read();
        s_adopt.adopt(&status);
    });
    let end_gesture = move || {
        let mut drag = s_up.drag;
        let mut paint = s_up.paint;
        if drag.peek().is_some() || paint.peek().is_some() {
            drag.set(None);
            paint.set(None);
            s_up.save();
        }
    };
    let end_leave = end_gesture.clone();
    let end_up = end_gesture;
    let menu = *synth.menu.read();

    rsx! {
        style { {STYLE} }
        div {
            class: if drag.read().is_some() { "root dragging" } else { "root" },
            tabindex: "0",
            autofocus: true,
            onpointerdown: move |_| s_down.audio.user_gesture(),
            onpointermove: move |e: PointerEvent| {
                let Some(d) = *s_move.drag.peek() else { return };
                e.prevent_default();
                let y = e.client_coordinates().y;
                let fine = e.modifiers().shift();
                let mut drag = s_move.drag;
                if fine != d.fine {
                    // Re-anchor so toggling SHIFT mid-drag doesn't jump.
                    drag.set(Some(Drag { start_y: y, start_value: s_move.value(d.target), fine, ..d }));
                    return;
                }
                let scale = if fine { 0.1 } else { 1.0 };
                let v = d.start_value as f64 + (d.start_y - y) / d.span_px * scale;
                s_move.set(d.target, v as f32);
            },
            onpointerup: move |_| end_up(),
            onpointerleave: move |_| end_leave(),
            onkeydown: move |e: KeyboardEvent| on_key(&s_key, e, true),
            onkeyup: move |e: KeyboardEvent| on_key(&s_keyup, e, false),
            Header {}
            main { class: "panel",
                Transport {}
                ShiftPlan {}
                Floor {}
                Master {}
            }
            footer { class: "footer",
                kbd { "Space" }
                span { " play/stop · " }
                kbd { "1" }
                span { "–" }
                kbd { "8" }
                span { " hold a machine running · drag knobs vertically (" }
                kbd { "Shift" }
                span { " = fine), scroll to nudge, double-click to reset · " }
                kbd { "Ctrl" }
                span { "-click any control to lock it against RND, MUTATE and VARIANCE (Ctrl-click a row's steps to lock its sequence) · click and drag across steps to draw or erase active periods; a run over the end of a track continues into its start · click past a track's end to extend it." }
            }
            if let Some(menu) = menu {
                MachineMenu { menu }
            }
        }
    }
}

fn on_key(synth: &Synth, e: KeyboardEvent, down: bool) {
    let m = e.modifiers();
    if m.ctrl() || m.alt() || m.meta() {
        return;
    }
    let code = e.code();
    if let Some(i) = HOLD_KEYS.iter().position(|&c| c == code) {
        e.prevent_default();
        if e.is_auto_repeating() {
            return;
        }
        let track = synth.state.peek().track_list().get(i).copied();
        if let Some(t) = track {
            synth.hold(t, down);
        }
        return;
    }
    if !down {
        return;
    }
    match code {
        Code::Space if !e.is_auto_repeating() => {
            e.prevent_default();
            synth.toggle_play();
        }
        Code::Escape => synth.close_menu(),
        _ => synth.audio.user_gesture(),
    }
}

// --- header ----------------------------------------------------------------------

#[component]
fn Header() -> Element {
    let synth = use_context::<Synth>();
    let status = synth.audio.status.read().clone();
    let (class, text) = match status {
        AudioStatus::NeedsGesture => (
            "status warn",
            "Click anywhere or press a key to start audio".to_string(),
        ),
        AudioStatus::Starting => ("status warn", "Starting audio…".to_string()),
        AudioStatus::Running {
            sample_rate,
            detail,
        } => (
            "status ok",
            format!("{detail} · {:.1} kHz", sample_rate as f32 / 1000.0),
        ),
        AudioStatus::Failed(e) => ("status err", format!("Audio unavailable: {e}")),
    };
    rsx! {
        header { class: "header",
            div { class: "brand",
                Logo {}
                span { class: "logo", "FERRODROME" }
                span { class: "model", "FD-1" }
                span { class: "tagline", "Industrial Machine Hall Synthesizer" }
            }
            div { class: "{class}",
                span { class: "status-dot" }
                span { "{text}" }
            }
            div { class: "master-level",
                Knob { target: Target::Global(GlobalParam::Master), size: "sm" }
            }
        }
    }
}

/// The brand mark: a north-light sawtooth roof over a gear.
#[component]
fn Logo() -> Element {
    rsx! {
        svg { class: "brand-mark", view_box: "0 0 44 32",
            path { class: "roof", d: "M2 14L10 4V14L18 4V14L26 4V14L34 4V14L42 4V14" }
            path { class: "hall", d: "M2 14V30H42V14" }
            path { class: "gear", d: "{gear_path(22.0, 23.0, 5.2, 3.8, 8)}" }
            circle { class: "hub", cx: "22", cy: "23", r: "1.6" }
        }
    }
}

/// A gear outline with `teeth` teeth.
fn gear_path(cx: f64, cy: f64, r_out: f64, r_in: f64, teeth: usize) -> String {
    let mut d = String::new();
    let n = teeth * 4;
    for i in 0..=n {
        let a = i as f64 / n as f64 * 360.0;
        let r = if (i / 2) % 2 == 0 { r_out } else { r_in };
        let (x, y) = polar(cx, cy, r, a);
        d += &format!("{}{x:.2} {y:.2}", if i == 0 { 'M' } else { 'L' });
    }
    d + "Z"
}

// --- transport ---------------------------------------------------------------------

#[component]
fn Transport() -> Element {
    let synth = use_context::<Synth>();
    let playing = *synth.playing.read();
    let full = synth.state.read().track_list().len() >= MAX_TRACKS;
    let s_play = synth.clone();
    let s_add = synth.clone();
    let s_rnd = synth.clone();
    rsx! {
        section { class: "sec transport",
            button {
                class: if playing { "play on" } else { "play" },
                title: "Start/stop the shift (Space)",
                onclick: move |_| s_play.toggle_play(),
                svg { view_box: "0 0 20 20",
                    if playing {
                        path { d: "M5 5h10v10H5z" }
                    } else {
                        path { d: "M6 4l10 6-10 6z" }
                    }
                }
                span { if playing { "STOP" } else { "PLAY" } }
            }
            Knob { target: Target::Global(GlobalParam::Tempo) }
            Lcd {}
            div { class: "spacer" }
            button {
                class: "btn add",
                disabled: full,
                title: if full { "The hall is full (eight machines)" } else { "Add a machine to the hall" },
                onpointerdown: move |e| e.stop_propagation(),
                onclick: move |_| {
                    let mut m = s_add.menu;
                    let open = *m.peek() == Some(Menu::Add);
                    m.set(if open { None } else { Some(Menu::Add) });
                },
                "+ MACHINE"
            }
            button {
                class: "btn rnd",
                title: "Randomize everything that isn't locked (Ctrl-click controls to lock them)",
                onclick: move |_| s_rnd.randomize_all(),
                span { "RND" }
            }
        }
    }
}

/// Small read-out: the last touched control and where the shift is.
#[component]
fn Lcd() -> Element {
    let synth = use_context::<Synth>();
    let step = *synth.audio.meters.step.read();
    let playing = *synth.playing.read();
    let state = synth.state.read();
    let machines = state.track_list().len();
    let tempo = GlobalParam::Tempo.display(state.global(GlobalParam::Tempo));
    drop(state);
    let line1 = match *synth.touched.read() {
        Some(t) => format!(
            "{} {}{}",
            synth.name(t).to_uppercase(),
            synth.display(t, synth.value(t)),
            if synth.is_locked(t) { " [LOCKED]" } else { "" }
        ),
        None => format!("{machines} MACHINES · {tempo}"),
    };
    let line2 = if playing && step >= 0 {
        let s = step as usize;
        format!(
            "▶ BAR {:>3} · BEAT {} · STEP {:>2}",
            s / BAR + 1,
            s % BAR / STEPS_PER_BEAT + 1,
            s % BAR + 1
        )
    } else {
        format!("■ SHIFT STOPPED · {tempo}")
    };
    rsx! {
        div { class: "lcd",
            div { class: "lcd-line", "{line1}" }
            div { class: "lcd-line", "{line2}" }
        }
    }
}

// --- machine menu ----------------------------------------------------------------------

#[component]
fn MachineMenu(menu: Menu) -> Element {
    let synth = use_context::<Synth>();
    let current = match menu {
        Menu::Change(t) => Some(synth.state.read().machine(t)),
        Menu::Add => None,
    };
    let title = match menu {
        Menu::Add => "ADD A MACHINE".to_string(),
        Menu::Change(t) => format!("MACHINE FOR TRACK {}", TRACK_COUNT_LABEL[t]),
    };
    let s_close = synth.clone();
    rsx! {
        div {
            class: "menu-backdrop",
            onpointerdown: move |e| {
                e.stop_propagation();
                s_close.close_menu();
            },
            div {
                class: "menu",
                onpointerdown: move |e| e.stop_propagation(),
                div { class: "menu-title", "{title}" }
                div { class: "menu-grid",
                    for m in 0..MACHINE_COUNT as u32 {
                        MenuItem { key: "{m}", menu, machine: m, current: current == Some(m) }
                    }
                }
            }
        }
    }
}

#[component]
fn MenuItem(menu: Menu, machine: u32, current: bool) -> Element {
    let synth = use_context::<Synth>();
    let s = spec(machine);
    rsx! {
        button {
            class: if current { "menu-item sel" } else { "menu-item" },
            onclick: move |_| {
                match menu {
                    Menu::Add => {
                        synth.add_track(machine);
                    }
                    Menu::Change(t) => synth.set_machine(t, machine),
                }
                synth.close_menu();
            },
            MachineGlyph { machine, spin: false }
            div { class: "menu-text",
                div { class: "menu-name", "{s.name}" }
                div { class: "menu-blurb", "{s.blurb}" }
            }
        }
    }
}

/// The machine's name and glyph as a button: click to change machine,
/// Ctrl-click to lock it.
#[component]
fn MachineButton(track: usize, #[props(default)] big: bool) -> Element {
    let synth = use_context::<Synth>();
    let machine = synth.state.read().machine(track);
    let target = Target::Param(track, Param::Machine);
    let locked = synth.is_locked(target);
    let active = *synth.audio.meters.active[track].read();
    let s_down = synth.clone();
    let s_click = synth.clone();
    rsx! {
        button {
            class: if big { "machine-btn big" } else { "machine-btn" },
            class: if locked { "locked" },
            title: "{spec(machine).blurb}. Click to change machine; Ctrl-click to lock it against RND and MUTATE.",
            oncontextmenu: move |e| e.prevent_default(),
            onpointerdown: move |e| {
                e.stop_propagation();
                if lock_click(&e) {
                    e.prevent_default();
                    s_down.toggle_lock(target);
                }
            },
            onclick: move |e| {
                if e.modifiers().ctrl() || e.modifiers().meta() {
                    return;
                }
                let mut m = s_click.menu;
                m.set(Some(Menu::Change(track)));
            },
            MachineGlyph { machine, spin: active }
            span { class: "machine-name", "{MACHINE_NAMES[machine as usize]}" }
            if locked {
                Padlock {}
            }
        }
    }
}

/// Line-art glyph per machine in a 24×24 box.
#[component]
fn MachineGlyph(machine: u32, spin: bool) -> Element {
    rsx! {
        svg {
            class: if spin { "glyph spin" } else { "glyph" },
            class: "g{machine}",
            view_box: "0 0 24 24",
            path { d: "{machine_glyph(machine)}" }
        }
    }
}

fn machine_glyph(machine: u32) -> String {
    match machine {
        // CONVEYOR: a belt over three rollers.
        0 => "M3 9h18M3 15h18M3 9a3 3 0 0 0 0 6M21 9a3 3 0 0 1 0 6M7 12a1.4 1.4 0 1 0 .01 0M12 12a1.4 1.4 0 1 0 .01 0M17 12a1.4 1.4 0 1 0 .01 0".into(),
        // PUMP: cylinder, piston and rod.
        1 => "M6 3h12v11H6zM6 9h12M12 9v12M8 21h8M3 6h3M18 6h3".into(),
        // CRANKSHAFT: a crank throw.
        2 => "M2 12h4V6h5v12h5V9h4v3h2M8 6v-2M13.5 18v2".into(),
        // TRANSFORMER: core and windings.
        3 => "M4 4h16v16H4zM9 4v16M15 4v16M4 8h5M4 11h5M4 14h5M4 17h5M15 8h5M15 11h5M15 14h5M15 17h5".into(),
        // SERVO: a two-link arm.
        4 => "M4 21h8M8 21v-3M8 18a2 2 0 1 0 .01 0M9.5 16.5L15 9M15 9a2 2 0 1 0 .01 0M16.5 7.5L20 4M18 3l3 3".into(),
        // PRESS: frame, ram and die.
        5 => "M3 3h18M5 3v18M19 3v18M12 3v5M8 8h8v3H8zM8 17h8v2H8zM3 21h18".into(),
        // TURBINE: a fan.
        6 => "M12 12a1.5 1.5 0 1 0 .01 0M12 10.5C12 5 15 3 17 4S16 9 13.3 11M13.5 12.6C18 15 18.5 18.5 16.5 19.5S12.5 16 12 13.5M10.6 12.8C6 15.5 3.5 14 3.5 12S7 9 10.6 11.2".into(),
        // GRINDER: a gear.
        7 => gear_path(12.0, 12.0, 9.0, 6.5, 9) + "M12 9a3 3 0 1 0 .01 0",
        // WELDER: an arc spark.
        8 => "M14 2L7 13h5l-2 9 8-12h-5zM3 20l3-2M21 18l-3-1M20 6l-2 1".into(),
        // STEAM: rising plumes.
        _ => "M7 21c-2-3 2-5 0-8s2-5 0-8M12 21c-2-3 2-5 0-8s2-5 0-8M17 21c-2-3 2-5 0-8s2-5 0-8".into(),
    }
}

#[component]
fn Padlock() -> Element {
    rsx! {
        svg { class: "padlock", view_box: "0 0 12 14", "aria-label": "locked",
            path { class: "lock-shackle", d: "M3.5 6V4a2.5 2.5 0 0 1 5 0v2" }
            rect { class: "lock-body", x: "2", y: "6", width: "8", height: "6.5", rx: "1.2" }
        }
    }
}

// --- shift plan (sequencer) ---------------------------------------------------------------

#[component]
fn ShiftPlan() -> Element {
    let synth = use_context::<Synth>();
    let state = synth.state.read();
    let tracks = state.track_list();
    let view = state.view_steps();
    drop(state);
    rsx! {
        section { class: "sec plan",
            div { class: "plan-head",
                div { class: "sec-title", "SHIFT PLAN" }
                span { class: "hint",
                    "Draw when each machine runs. Every row loops its own length; a run across a row's end continues into its start."
                }
            }
            if tracks.is_empty() {
                div { class: "empty-hall",
                    "The hall is empty. Add a machine with + MACHINE, or press RND for a random hall."
                }
            } else {
                div { class: "plan-grid", style: "--view: {view}",
                    div { class: "ruler-head" }
                    div { class: "ruler",
                        for s in 0..view {
                            div {
                                key: "{s}",
                                class: if s % BAR == 0 { "tick bar" } else if s % STEPS_PER_BEAT == 0 { "tick beat" } else { "tick" },
                                if s % BAR == 0 { "{s / BAR + 1}" }
                            }
                        }
                    }
                    for t in tracks {
                        SeqRow { key: "{t}", track: t, view }
                    }
                }
            }
        }
    }
}

#[component]
fn SeqRow(track: usize, view: usize) -> Element {
    let synth = use_context::<Synth>();
    let state = synth.state.read();
    let len = state.length(track);
    let steps = state.steps[track];
    let muted = state.tracks[track][Param::Mute.index()] >= 0.5;
    let soloed = state.tracks[track][Param::Solo.index()] >= 0.5;
    let pattern_locked = state.pattern_locks[track];
    drop(state);
    let step = *synth.audio.meters.step.read();
    let playing = *synth.playing.read();
    let pos = (playing && step >= 0).then(|| step as usize % len);
    let periods = seq::periods(steps, len);
    let mut period_of = vec![(0usize, 0usize); len];
    for &(start, n) in &periods {
        for i in 0..n {
            period_of[(start + i) % len] = (start, n);
        }
    }
    let wraps = len > 1 && seq::get(steps, 0) && seq::get(steps, len - 1) && periods.len() > 1
        || periods.len() == 1 && periods[0].1 == len && len > 1;
    let active_steps: usize = periods.iter().map(|p| p.1).sum();
    let info = match periods.len() {
        0 => "idle".to_string(),
        1 if periods[0].1 == len => "always running".to_string(),
        1 => format!("1 period · {active_steps} st"),
        n => format!("{n} periods · {active_steps} st"),
    };
    let s_m = synth.clone();
    let s_s = synth.clone();
    let s_dice = synth.clone();
    let s_clear = synth.clone();
    let s_left = synth.clone();
    let s_right = synth.clone();
    rsx! {
        div {
            class: "row-head t{track}",
            class: if muted { "muted" },
            span { class: "row-num", "{TRACK_COUNT_LABEL[track]}" }
            MachineButton { track }
            div { class: "row-tools",
                button {
                    class: if muted { "tog on" } else { "tog" },
                    title: "Mute",
                    onclick: move |_| {
                        s_m.set_param(track, Param::Mute, if muted { 0.0 } else { 1.0 });
                        s_m.save();
                    },
                    "M"
                }
                button {
                    class: if soloed { "tog solo on" } else { "tog solo" },
                    title: "Solo",
                    onclick: move |_| {
                        s_s.set_param(track, Param::Solo, if soloed { 0.0 } else { 1.0 });
                        s_s.save();
                    },
                    "S"
                }
                RunButton { track }
                Knob { target: Target::Param(track, Param::Length), size: "xs" }
                div { class: "row-mini",
                    button { class: "mini", title: "Random sequence", onclick: move |_| s_dice.random_steps(track), "⚄" }
                    button { class: "mini", title: "Clear the sequence", onclick: move |_| s_clear.clear_steps(track), "✕" }
                    button { class: "mini", title: "Shift the sequence one step earlier", onclick: move |_| s_left.rotate_steps(track, -1), "◀" }
                    button { class: "mini", title: "Shift the sequence one step later", onclick: move |_| s_right.rotate_steps(track, 1), "▶" }
                }
            }
            div { class: "row-info",
                if pattern_locked {
                    Padlock {}
                }
                span { "{info}" }
                DriveBar { track }
            }
        }
        div {
            class: "cells t{track}",
            class: if muted { "muted" },
            class: if pattern_locked { "locked" },
            for s in 0..view {
                Cell {
                    key: "{s}",
                    track,
                    step: s,
                    on: s < len && seq::get(steps, s),
                    joins: if s < len { seq::joins(steps, len, s) } else { (false, false) },
                    beyond: s >= len,
                    beat: s % STEPS_PER_BEAT == 0,
                    bar: s % BAR == 0,
                    now: pos == Some(s),
                    wrap: wraps && s < len && (s == 0 || s == len - 1) && seq::get(steps, s),
                    period: if s < len { period_of[s] } else { (0, 0) },
                    len,
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[component]
fn Cell(
    track: usize,
    step: usize,
    on: bool,
    joins: (bool, bool),
    beyond: bool,
    beat: bool,
    bar: bool,
    now: bool,
    wrap: bool,
    period: (usize, usize),
    len: usize,
) -> Element {
    let synth = use_context::<Synth>();
    let mut class = String::from("cell");
    for (cond, name) in [
        (on, " on"),
        (joins.0, " jl"),
        (joins.1, " jr"),
        (beyond, " beyond"),
        (beat, " beat"),
        (bar, " bar"),
        (now, " now"),
        (wrap && step == 0, " wrap-in"),
        (wrap && step + 1 == len, " wrap-out"),
    ] {
        if cond {
            class += name;
        }
    }
    let title = if beyond {
        format!(
            "Past the end (track is {len} steps): click to extend it to {}",
            step + 1
        )
    } else if on {
        let (start, n) = period;
        let end = (start + n - 1) % len + 1;
        if n == len {
            format!("Step {}: always running", step + 1)
        } else if start + n > len {
            format!(
                "Step {}: one {n}-step period, {} → {} across the loop point",
                step + 1,
                start + 1,
                end
            )
        } else {
            format!(
                "Step {}: one {n}-step period, {} → {}",
                step + 1,
                start + 1,
                end
            )
        }
    } else {
        format!("Step {}: idle. Click or drag to draw.", step + 1)
    };
    let s_down = synth.clone();
    let s_enter = synth.clone();
    rsx! {
        div {
            class: "{class}",
            title: "{title}",
            oncontextmenu: move |e| e.prevent_default(),
            onpointerdown: move |e: PointerEvent| {
                e.prevent_default();
                let synth = &s_down;
                synth.audio.user_gesture();
                if lock_click(&e) {
                    synth.toggle_pattern_lock(track);
                    return;
                }
                if beyond {
                    synth.set_param(track, Param::Length, (step + 1) as f32);
                    synth.save();
                    return;
                }
                let value = !on;
                synth.set_step(track, step, value);
                let mut paint = synth.paint;
                paint.set(Some((track, value)));
            },
            onpointerenter: move |_| {
                let Some((t, value)) = *s_enter.paint.peek() else { return };
                if t == track && !beyond {
                    s_enter.set_step(track, step, value);
                }
            },
        }
    }
}

/// Hold to run a machine regardless of the sequence.
#[component]
fn RunButton(track: usize) -> Element {
    let synth = use_context::<Synth>();
    let held = synth.held.read()[track];
    let s_down = synth.clone();
    let s_up = synth.clone();
    let s_leave = synth.clone();
    rsx! {
        button {
            class: if held { "tog run on" } else { "tog run" },
            title: "Hold to run this machine now (or keys 1–8)",
            onpointerdown: move |e| {
                e.prevent_default();
                s_down.hold(track, true);
            },
            onpointerup: move |_| s_up.hold(track, false),
            onpointerleave: move |_| s_leave.hold(track, false),
            "RUN"
        }
    }
}

/// The drive (spin-up) as a small bar.
#[component]
fn DriveBar(track: usize) -> Element {
    let synth = use_context::<Synth>();
    let d = *synth.audio.meters.drive[track].read();
    rsx! {
        div { class: "drive-bar", title: "Drive {d * 100.0:.0}%",
            div { class: "drive-fill", style: "width: {d * 100.0:.1}%" }
        }
    }
}

// --- machine floor ------------------------------------------------------------------------

#[component]
fn Floor() -> Element {
    let synth = use_context::<Synth>();
    let tracks = synth.state.read().track_list();
    rsx! {
        div { class: "floor",
            for t in tracks {
                MachineCard { key: "{t}", track: t }
            }
        }
    }
}

#[component]
fn MachineCard(track: usize) -> Element {
    let synth = use_context::<Synth>();
    let machine = synth.state.read().machine(track);
    let s = spec(machine);
    let p = |param: Param| Target::Param(track, param);
    let s_rnd = synth.clone();
    let s_del = synth.clone();
    rsx! {
        section { class: "sec card t{track}",
            div { class: "card-head",
                span { class: "row-num", "{TRACK_COUNT_LABEL[track]}" }
                MachineButton { track, big: true }
                button {
                    class: "btn",
                    title: "Randomize this machine's sound (not its sequence; locked controls stay)",
                    onclick: move |_| s_rnd.randomize_track(track, false),
                    "RND"
                }
                button {
                    class: "btn danger",
                    title: "Remove this machine from the hall",
                    onclick: move |_| s_del.remove_track(track),
                    "✕"
                }
            }
            div { class: "card-blurb", "{s.blurb}" }
            div { class: "card-body",
                Gauge { track }
                div { class: "machine-knobs",
                    for k in KNOB_PARAMS {
                        Knob { key: "{k:?}", target: p(k) }
                    }
                }
            }
            div { class: "card-row",
                div { class: "group",
                    div { class: "sec-title", "RAMP" }
                    div { class: "row",
                        Knob { target: p(Param::RampUp), size: "sm" }
                        Knob { target: p(Param::RampDown), size: "sm" }
                    }
                }
                div { class: "group chaos",
                    div { class: "sec-title", "RANDOM" }
                    div { class: "row",
                        Knob { target: p(Param::Variance), size: "sm" }
                        Knob { target: p(Param::Mutate), size: "sm" }
                    }
                }
                div { class: "group",
                    div { class: "sec-title", "OUT" }
                    div { class: "row",
                        Knob { target: p(Param::Level), size: "sm" }
                        Knob { target: p(Param::Pan), size: "sm" }
                    }
                }
            }
            div { class: "card-row",
                div { class: "group filter",
                    div { class: "sec-title", "FILTER" }
                    div { class: "row",
                        Choice { target: p(Param::FltType), names: FILTER_NAMES.to_vec() }
                        Knob { target: p(Param::FltCutoff), size: "sm" }
                        Knob { target: p(Param::FltReso), size: "sm" }
                        Knob { target: p(Param::FltDrive), size: "sm" }
                        Knob { target: p(Param::FltEnv), size: "sm" }
                    }
                }
                div { class: "group eqg",
                    div { class: "sec-title", "EQ" }
                    div { class: "row",
                        Knob { target: p(Param::EqLow), size: "sm" }
                        Knob { target: p(Param::EqMid), size: "sm" }
                        Knob { target: p(Param::EqFreq), size: "sm" }
                        Knob { target: p(Param::EqHigh), size: "sm" }
                    }
                }
                ResponseCurve { track }
            }
            div { class: "card-row",
                div { class: "group wide",
                    div { class: "sec-title", "REVERB" }
                    div { class: "row",
                        Knob { target: p(Param::RevMix), size: "sm" }
                        Knob { target: p(Param::RevDecay), size: "sm" }
                        Knob { target: p(Param::RevTone), size: "sm" }
                        div { class: "eq",
                            Knob { target: p(Param::RevLow), size: "xs" }
                            Knob { target: p(Param::RevMid), size: "xs" }
                            Knob { target: p(Param::RevHigh), size: "xs" }
                        }
                    }
                }
                div { class: "group wide",
                    div { class: "sec-title", "DELAY" }
                    div { class: "row",
                        Knob { target: p(Param::DlyMix), size: "sm" }
                        Knob { target: p(Param::DlyTime), size: "sm" }
                        Knob { target: p(Param::DlyFeedback), size: "sm" }
                        Knob { target: p(Param::DlyTone), size: "sm" }
                    }
                }
            }
        }
    }
}

/// The filter and EQ response, drawn live: it follows the ramp (RAMP→) and
/// VARIANCE.
#[component]
fn ResponseCurve(track: usize) -> Element {
    const W: f64 = 200.0;
    const H: f64 = 74.0;
    const TOP_DB: f32 = 18.0;
    const BOTTOM_DB: f32 = -36.0;
    const POINTS: usize = 90;
    let synth = use_context::<Synth>();
    let base = synth.state.read().tracks[track];
    let patch = if base[Param::Variance.index()] > 0.0 {
        let mut live = *synth.audio.meters.live[track].read();
        // VARIANCE never moves the type; keep what the panel has.
        live[Param::FltType.index()] = base[Param::FltType.index()];
        live
    } else {
        base
    };
    let drive = *synth.audio.meters.drive[track].read();
    let v = |p: Param| patch[p.index()];
    let fs = 48_000.0;
    let x_of = |f: f32| (f / 20.0).log10() as f64 / 3.0 * W;
    let y_of = |db: f32| {
        let db = db.clamp(BOTTOM_DB, TOP_DB);
        ((TOP_DB - db) / (TOP_DB - BOTTOM_DB)) as f64 * H
    };
    let mut d = String::with_capacity(POINTS * 16);
    for i in 0..POINTS {
        let f = 20.0 * 1000f32.powf(i as f32 / (POINTS - 1) as f32);
        let db = ferrodrome_dsp::filter::response_db(
            v(Param::FltType) as u32,
            v(Param::FltCutoff),
            v(Param::FltReso),
            v(Param::FltEnv),
            drive,
            [
                v(Param::EqLow),
                v(Param::EqMid),
                v(Param::EqFreq),
                v(Param::EqHigh),
            ],
            f,
            fs,
        );
        d += &format!(
            "{}{:.1} {:.1}",
            if i == 0 { 'M' } else { 'L' },
            x_of(f),
            y_of(db)
        );
    }
    let fill = format!("{d}L{W} {H}L0 {H}Z");
    let zero = y_of(0.0);
    rsx! {
        svg {
            class: "response",
            view_box: "0 0 {W} {H}",
            preserve_aspect_ratio: "none",
            for f in [100.0f32, 1_000.0, 10_000.0] {
                line { key: "{f}", class: "resp-grid", x1: "{x_of(f):.1}", y1: "0", x2: "{x_of(f):.1}", y2: "{H}" }
            }
            line { class: "resp-zero", x1: "0", y1: "{zero:.1}", x2: "{W}", y2: "{zero:.1}" }
            path { class: "resp-fill", d: "{fill}" }
            path { class: "resp-line", d: "{d}" }
        }
    }
}

/// Segmented choice (one button per option). Ctrl-click locks it.
#[component]
fn Choice(target: Target, names: Vec<&'static str>) -> Element {
    let synth = use_context::<Synth>();
    let n = names.len();
    let current = (synth.value(target) * (n - 1) as f32).round() as usize;
    let locked = synth.is_locked(target);
    let label = synth.label(target);
    let name = synth.name(target);
    rsx! {
        div { class: "choice", class: if locked { "locked" },
            div { class: "ctl-label", "{label}" }
            div { class: "knob-wrap",
                div { class: "segments", title: "{name} · Ctrl-click to lock",
                    for (i, label) in names.into_iter().enumerate() {
                        Segment { key: "{i}", target, index: i, count: n, label, selected: i == current }
                    }
                }
                if locked {
                    Padlock {}
                }
            }
        }
    }
}

#[component]
fn Segment(
    target: Target,
    index: usize,
    count: usize,
    label: &'static str,
    selected: bool,
) -> Element {
    let synth = use_context::<Synth>();
    let s_down = synth.clone();
    rsx! {
        button {
            class: if selected { "segment sel" } else { "segment" },
            oncontextmenu: move |e| e.prevent_default(),
            onpointerdown: move |e| {
                if lock_click(&e) {
                    e.prevent_default();
                    s_down.toggle_lock(target);
                }
            },
            onclick: move |e| {
                if e.modifiers().ctrl() || e.modifiers().meta() {
                    return;
                }
                synth.set(target, index as f32 / (count - 1).max(1) as f32);
                synth.save();
            },
            "{label}"
        }
    }
}

/// Tachometer: the machine's drive, with a RUN button in it.
#[component]
fn Gauge(track: usize) -> Element {
    let synth = use_context::<Synth>();
    let d = *synth.audio.meters.drive[track].read() as f64;
    let active = *synth.audio.meters.active[track].read();
    let angle = -120.0 + 240.0 * d;
    let (px, py) = polar(40.0, 40.0, 26.0, angle);
    let track_arc = arc_path(40.0, 40.0, 32.0, -120.0, 120.0);
    let lit = if d > 0.004 {
        arc_path(40.0, 40.0, 32.0, -120.0, angle)
    } else {
        String::new()
    };
    rsx! {
        div { class: "gauge",
            svg { view_box: "0 0 80 64",
                path { class: "gauge-track", d: "{track_arc}" }
                for i in 0..=8 {
                    {
                        let a = -120.0 + 30.0 * i as f64;
                        let (x0, y0) = polar(40.0, 40.0, 36.0, a);
                        let (x1, y1) = polar(40.0, 40.0, 39.0, a);
                        rsx! { line { key: "{i}", class: "gauge-tick", x1: "{x0:.1}", y1: "{y0:.1}", x2: "{x1:.1}", y2: "{y1:.1}" } }
                    }
                }
                if !lit.is_empty() {
                    path { class: "gauge-lit", d: "{lit}" }
                }
                line { class: "gauge-needle", x1: "40", y1: "40", x2: "{px:.1}", y2: "{py:.1}" }
                circle { class: "gauge-hub", cx: "40", cy: "40", r: "3" }
                text { class: "gauge-text", x: "40", y: "60", "{d * 100.0:.0}%" }
            }
            div { class: if active { "gauge-led on" } else { "gauge-led" } }
            RunButton { track }
        }
    }
}

// --- master ----------------------------------------------------------------------------------

#[component]
fn Master() -> Element {
    let g = Target::Global;
    rsx! {
        section { class: "sec master",
            div { class: "group wide",
                div { class: "sec-title", "MASTER REVERB · THE HALL" }
                div { class: "row",
                    Knob { target: g(GlobalParam::RevMix) }
                    Knob { target: g(GlobalParam::RevDecay) }
                    Knob { target: g(GlobalParam::RevTone) }
                    Knob { target: g(GlobalParam::RevPredelay) }
                    div { class: "eq",
                        Knob { target: g(GlobalParam::RevLow), size: "sm" }
                        Knob { target: g(GlobalParam::RevMid), size: "sm" }
                        Knob { target: g(GlobalParam::RevHigh), size: "sm" }
                    }
                }
            }
            div { class: "group wide",
                div { class: "sec-title", "MASTER DELAY" }
                div { class: "row",
                    Knob { target: g(GlobalParam::DlyMix) }
                    Knob { target: g(GlobalParam::DlyTime) }
                    Knob { target: g(GlobalParam::DlyFeedback) }
                    Knob { target: g(GlobalParam::DlyTone) }
                }
            }
        }
    }
}

// --- controls ----------------------------------------------------------------------------------

fn polar(cx: f64, cy: f64, r: f64, deg: f64) -> (f64, f64) {
    let a = deg.to_radians();
    (cx + r * a.sin(), cy - r * a.cos())
}

fn arc_path(cx: f64, cy: f64, r: f64, from_deg: f64, to_deg: f64) -> String {
    let (x0, y0) = polar(cx, cy, r, from_deg);
    let (x1, y1) = polar(cx, cy, r, to_deg);
    let large = if (to_deg - from_deg).abs() > 180.0 {
        1
    } else {
        0
    };
    format!("M {x0:.2} {y0:.2} A {r} {r} 0 {large} 1 {x1:.2} {y1:.2}")
}

fn begin_drag(synth: &Synth, target: Target, e: &PointerEvent, span_px: f64) {
    e.prevent_default();
    synth.audio.user_gesture();
    let mut touched = synth.touched;
    touched.set(Some(target));
    let mut drag = synth.drag;
    drag.set(Some(Drag {
        target,
        start_y: e.client_coordinates().y,
        start_value: synth.value(target),
        span_px,
        fine: e.modifiers().shift(),
    }));
}

fn nudge(synth: &Synth, target: Target, e: &WheelEvent) {
    e.prevent_default();
    let dy = e.delta().strip_units().y;
    if dy == 0.0 {
        return;
    }
    let step = match synth.steps(target) {
        Some(n) => 1.0 / (n - 1).max(1) as f32,
        None if e.modifiers().shift() => 0.002,
        None => 0.02,
    };
    let v = synth.value(target) - (dy.signum() as f32) * step;
    synth.set(target, v);
    synth.save();
}

/// The VARIANCE ghost: where a varied control is right now, if it wanders.
fn ghost(synth: &Synth, target: Target) -> Option<f32> {
    let Target::Param(t, p) = target else {
        return None;
    };
    let s = synth.state.read();
    let machine = s.machine(t);
    if s.tracks[t][Param::Variance.index()] <= 0.0 || !p.varies(machine) || s.locks[t][p.index()] {
        return None;
    }
    drop(s);
    let live = synth.audio.meters.live[t].read()[p.index()];
    Some(synth.knob_of(target, live))
}

#[component]
fn Knob(target: Target, #[props(default = "lg")] size: &'static str) -> Element {
    let synth = use_context::<Synth>();
    let value = synth.value(target);
    let label = synth.label(target);
    let name = synth.name(target);
    let locked = synth.is_locked(target);
    let active = matches!(*synth.drag.read(), Some(d) if d.target == target);
    let angle = -135.0 + 270.0 * value as f64;
    let (px, py) = polar(30.0, 30.0, 15.0, angle);
    let track = arc_path(30.0, 30.0, 26.0, -135.0, 135.0);
    // Bipolar controls light the arc from the centre.
    let (a0, a1) = if target.is_bipolar() {
        (angle.min(0.0), angle.max(0.0))
    } else {
        (-135.0, angle)
    };
    let lit = if (a1 - a0).abs() > 0.5 {
        arc_path(30.0, 30.0, 26.0, a0, a1)
    } else {
        String::new()
    };
    let ghost = ghost(&synth, target).map(|g| polar(30.0, 30.0, 26.0, -135.0 + 270.0 * g as f64));
    let text = synth.display(target, value);
    let lock_hint = if !target.lockable() {
        ""
    } else if locked {
        " · LOCKED (Ctrl-click to unlock)"
    } else {
        " · Ctrl-click to lock"
    };
    let s_down = synth.clone();
    let s_dbl = synth.clone();
    let s_wheel = synth.clone();
    rsx! {
        div {
            class: "knob knob-{size}",
            class: if active { "active" },
            class: if locked { "locked" },
            div { class: "ctl-label", "{label}" }
            div { class: "knob-wrap",
                svg {
                    class: "knob-svg",
                    view_box: "0 0 60 60",
                    role: "slider",
                    "aria-label": "{name}",
                    "aria-valuetext": "{text}",
                    oncontextmenu: move |e| e.prevent_default(),
                    onpointerdown: move |e| {
                        if lock_click(&e) {
                            e.prevent_default();
                            s_down.toggle_lock(target);
                        } else {
                            begin_drag(&s_down, target, &e, KNOB_SPAN_PX);
                        }
                    },
                    ondoubleclick: move |_| s_dbl.reset(target),
                    onwheel: move |e| nudge(&s_wheel, target, &e),
                    title { "{name}: {text}{lock_hint}" }
                    path { class: "knob-track", d: "{track}" }
                    if !lit.is_empty() {
                        path { class: "knob-lit", d: "{lit}" }
                    }
                    if let Some((gx, gy)) = ghost {
                        circle { class: "knob-ghost", cx: "{gx:.2}", cy: "{gy:.2}", r: "3.2" }
                    }
                    circle { class: "knob-skirt", cx: "30", cy: "30", r: "21" }
                    circle { class: "knob-body", cx: "30", cy: "30", r: "17" }
                    line { class: "knob-pointer", x1: "30", y1: "30", x2: "{px:.2}", y2: "{py:.2}" }
                }
                if locked {
                    Padlock {}
                }
            }
            div { class: "ctl-value", "{text}" }
        }
    }
}
