//! The clipboard, read for paste (`TextField`'s ⌘V / Ctrl+V).

/// The clipboard's text, if any.
#[cfg(feature = "clipboard")]
pub fn text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok().filter(|t| !t.is_empty())
}

/// The clipboard's text: always `None`, built without the `clipboard` feature.
#[cfg(not(feature = "clipboard"))]
pub fn text() -> Option<String> {
    None
}
