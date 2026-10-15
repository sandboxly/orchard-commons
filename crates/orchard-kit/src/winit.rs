//! winit's keyboard events converted to [`crate::input`]'s (for the
//! binary's event loop). Mouse buttons and scrolling stay with each app.
//!
//! The keyboard's play/pause media key arrives as Space, so it plays and
//! pauses in the Music Player (the Download Manager ignores it).

use orchard_grid::winit::event::KeyEvent as WKeyEvent;
use orchard_grid::winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use orchard_grid::winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

use crate::input::{Key, KeyEvent, Mods, Named};

/// The modifier keys held.
pub fn mods(mods: ModifiersState) -> Mods {
    Mods { shift: mods.shift_key(), ctrl: mods.control_key(), alt: mods.alt_key(), logo: mods.super_key() }
}

/// A key press as a [`KeyEvent`], `None` for keys the apps don't use.
pub fn convert_key(ev: &WKeyEvent, mods: ModifiersState) -> Option<KeyEvent> {
    let m = self::mods(mods);
    let key = match ev.key_without_modifiers() {
        WKey::Named(n) => match named(&n, m) {
            Some(Ok(n)) => Key::Named(n),
            Some(Err(typed)) => return Some(typed),
            None => return None,
        },
        WKey::Character(s) => Key::Char(s.chars().next()?.to_lowercase().next()?),
        _ => return None,
    };
    Some(KeyEvent { key, mods: m, text: ev.text.as_ref().map(|t| t.to_string()) })
}

/// A named key: `Ok` for a [`Named`] key (the caller adds mods and text),
/// `Err` for a key that types a character (the whole event), `None` for keys
/// the apps don't use.
fn named(n: &NamedKey, m: Mods) -> Option<Result<Named, KeyEvent>> {
    Some(Ok(match n {
        NamedKey::ArrowUp => Named::Up,
        NamedKey::ArrowDown => Named::Down,
        NamedKey::ArrowLeft => Named::Left,
        NamedKey::ArrowRight => Named::Right,
        NamedKey::PageUp => Named::PageUp,
        NamedKey::PageDown => Named::PageDown,
        NamedKey::Home => Named::Home,
        NamedKey::End => Named::End,
        NamedKey::Enter => Named::Enter,
        NamedKey::Escape => Named::Escape,
        NamedKey::Backspace => Named::Backspace,
        NamedKey::Delete => Named::Delete,
        NamedKey::Insert => Named::Insert,
        NamedKey::Tab => Named::Tab,
        NamedKey::Space => return Some(Err(KeyEvent { key: Key::Char(' '), mods: m, text: Some(" ".into()) })),
        NamedKey::MediaPlayPause => return Some(Err(KeyEvent::char(' '))),
        n => {
            let f = format!("{n:?}");
            Named::F(f.strip_prefix('F').and_then(|d| d.parse::<u8>().ok())?)
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys() {
        let shift = Mods { shift: true, ..Mods::default() };
        assert_eq!(named(&NamedKey::ArrowUp, shift), Some(Ok(Named::Up)));
        assert_eq!(named(&NamedKey::F5, shift), Some(Ok(Named::F(5))));
        assert_eq!(named(&NamedKey::F12, shift), Some(Ok(Named::F(12))));
        assert_eq!(named(&NamedKey::CapsLock, shift), None);
        assert_eq!(named(&NamedKey::Space, shift), Some(Err(KeyEvent { key: Key::Char(' '), mods: shift, text: Some(" ".into()) })));
    }

    #[test]
    fn play_pause_is_space() {
        // The Music Player plays and pauses with it, whatever modifiers are held.
        let shift = Mods { shift: true, ..Mods::default() };
        assert_eq!(named(&NamedKey::MediaPlayPause, shift), Some(Err(KeyEvent::char(' '))));
        assert_eq!(KeyEvent::char(' ').key, Key::Char(' '));
    }
}
