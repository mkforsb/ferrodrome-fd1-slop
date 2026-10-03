//! Ferrodrome FD-1: an industrial machine hall synthesizer and sequencer.
//!
//! * `dx serve --platform web` (feature `web`): WebAudio AudioWorklet.
//! * `dx serve --platform desktop` (feature `desktop`): native Linux window,
//!   PulseAudio output.

mod audio;
mod state;
mod ui;

fn main() {
    #[cfg(feature = "desktop")]
    {
        use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
        dioxus::LaunchBuilder::desktop()
            .with_cfg(
                Config::new()
                    .with_menu(None)
                    .with_background_color((14, 15, 17, 255))
                    .with_window(
                        WindowBuilder::new()
                            .with_title("Ferrodrome FD-1")
                            .with_inner_size(LogicalSize::new(1480.0, 980.0))
                            .with_min_inner_size(LogicalSize::new(760.0, 600.0)),
                    ),
            )
            .launch(ui::App);
    }

    #[cfg(not(feature = "desktop"))]
    dioxus::launch(ui::App);
}
