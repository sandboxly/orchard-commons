//! The window, event loop and command line the Orchard apps share, on top of
//! `orchard-grid` and `orchard-kit`. Everything the app itself does stays in
//! the app, behind [`ShellApp`]:
//!
//! - [`run`]: the winit event loop, the window and its GPU surface, redraws
//!   and animation frames, font changes, key and mouse conversion, wake-ups
//!   from workers, and `<APP>_EXIT_AFTER_START=1` for packaging checks;
//!   a second window for a [`Popup`] (a question that should come up where
//!   the user is and take the focus).
//! - [`cli`]: `--help`, `--version`, `--self-test` and `--screenshot`, the
//!   crash log, and (on Windows) printing to the console that started the
//!   app.
//! - [`shot`]: `--screenshot`'s common options and its headless render to
//!   PNG or WebP.
//! - [`config`]: the app's config folder and its JSON settings files.

pub use orchard_grid::winit;

pub mod cli;
pub mod config;
mod event_loop;
pub mod shot;

pub use event_loop::{Popup, ShellApp, run};

/// What the shell needs to know about an app; one `const` per app.
#[derive(Clone, Copy, Debug)]
pub struct AppInfo {
    /// "Orchard Music Player": the window title, `--version`, the crash log
    /// and the self-test's first line.
    pub name: &'static str,
    /// "orchard-music-player": the prefix of error messages and the
    /// self-test's temporary screenshot.
    pub bin: &'static str,
    /// The app's version (its `env!("CARGO_PKG_VERSION")`).
    pub version: &'static str,
    /// The window's logical size unless the settings have one.
    pub default_size: (u32, u32),
    /// The window's smallest logical size.
    pub min_size: (u32, u32),
    /// Set to `1`: quit after the first frame (CI starts the real app on
    /// every platform this way), e.g. `ORCHARD_MUSIC_PLAYER_EXIT_AFTER_START`.
    pub exit_after_start_env: &'static str,
}
