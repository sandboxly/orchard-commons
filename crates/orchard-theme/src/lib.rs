//! The look the Orchard apps share: color schemes (the built-ins plus the
//! user's, in Orchard Commander's `color-schemes/` folder, so a scheme made
//! in its editor shows up in every app), resolved to grid colors, plus the
//! frame style and font.
//!
//! - [`schemes`]: [`ColorScheme`], the built-ins, scheme files, [`Appearance`].
//! - [`colors`]: [`Colors`] (a scheme parsed for the grid) and [`mix`].
//! - [`theme`]: [`Theme`], everything drawing needs.
//! - [`dirs`]: where Orchard Commander keeps its config and color schemes.

pub mod colors;
pub mod dirs;
pub mod schemes;
pub mod theme;

pub use colors::{Colors, mix};
pub use schemes::{Appearance, ColorScheme};
pub use theme::Theme;

/// Errors with a message for the user (e.g. a taken color scheme name) or from the disk.
#[derive(Debug, thiserror::Error)]
pub enum SchemeError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
