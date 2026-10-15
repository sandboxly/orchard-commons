//! The UI kit the Orchard apps share, on top of `orchard-grid`,
//! `orchard-theme` and (for the bookmark
//! menu) `orchard-places`:
//!
//! - [`tasks`]: a worker pool whose results run on the UI thread.
//! - [`input`]: window-system-neutral key and mouse events.
//! - [`winit`]: winit's key events converted to [`input`]'s.
//! - [`widgets`]: the box frame, scrollbar, F-key bar and text field.
//! - [`form`]: dialog boxes and form controls (combo box and its popup, spin
//!   box, checkbox, push buttons, headings) and a laid-out form with one
//!   focused control.
//! - [`menu`]: a popup menu with a hotkey column and submenus (the
//!   bookmark menu).
//! - [`clipboard`]: the clipboard's text, for paste.

pub mod clipboard;
pub mod form;
pub mod input;
pub mod menu;
pub mod tasks;
pub mod widgets;
pub mod winit;

