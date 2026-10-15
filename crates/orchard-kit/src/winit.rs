//! winit's keyboard events converted to [`crate::input`]'s (for the
//! binary's event loop).

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
        WKey::Named(n) => Key::Named(match n {
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
            NamedKey::Space => return Some(KeyEvent { key: Key::Char(' '), mods: m, text: Some(" ".into()) }),
            n => {
                let f = format!("{n:?}");
                Named::F(f.strip_prefix('F').and_then(|d| d.parse::<u8>().ok())?)
            }
        }),
        WKey::Character(s) => Key::Char(s.chars().next()?.to_lowercase().next()?),
        _ => return None,
    };
    Some(KeyEvent { key, mods: m, text: ev.text.as_ref().map(|t| t.to_string()) })
}
