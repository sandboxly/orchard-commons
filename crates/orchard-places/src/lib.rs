//! Places for the Orchard apps' file and folder pickers: Orchard
//! Commander's bookmarks, so a folder bookmarked there shows up in every
//! app's picker (and one added in a picker shows up in Orchard Commander),
//! plus the home folder, the user's standard folders, drives and mounted
//! volumes. No UI here: the picker itself is `orchard_kit::picker`.
//!
//! - [`bookmarks`]: the bookmark tree, its file (Orchard Commander's
//!   `bookmarks.json`, byte-compatible with its Python version), changing
//!   it safely while other apps use it ([`bookmarks::update`],
//!   [`bookmarks::add_bookmark`], [`bookmarks::Stamp`],
//!   [`bookmarks::merge_external`]) and its local part for pickers
//!   ([`bookmarks::local_tree`]).
//! - [`locations`]: [`Spec`], a bookmark's saved location (a local folder,
//!   a folder on a server, a folder in an archive).
//! - [`mod@places`]: home, standard folders, drives, mounted volumes.
//! - [`pyjson`]: JSON written like Python's `json.dumps(indent=2)`.
//!
//! Organizing bookmarks (reordering, folders, deleting) stays in Orchard
//! Commander; the other apps read them and add new ones.

pub mod bookmarks;
pub mod locations;
pub mod places;
pub mod pyjson;

mod atomic;

use std::path::PathBuf;

pub use atomic::write_atomic;
pub use bookmarks::{Bookmark, Folder, Item, LocalBookmark, LocalFolder, LocalItem, Stamp};
pub use locations::{Spec, SpecRef, Target};
pub use places::{Place, PlaceKind, places};

/// Orchard Commander's bookmarks file: `bookmarks.json` in its config
/// directory (`$ORCHARD_COMMANDER_CONFIG_DIR` if set, see
/// `orchard_theme::dirs`).
pub fn bookmarks_file() -> PathBuf {
    orchard_theme::dirs::commander_config_dir().join("bookmarks.json")
}

#[cfg(test)]
mod tests {
    #[test]
    fn bookmarks_live_in_the_commander_config_dir() {
        assert_eq!(super::bookmarks_file(), orchard_theme::dirs::commander_config_dir().join("bookmarks.json"));
    }
}
