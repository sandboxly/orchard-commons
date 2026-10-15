//! A popup menu with a hotkey column and submenus, in Orchard Commander's
//! look (a port of its bookmark menu, ⌘D): "P  Projects   ~/src/projects".
//!
//! Typing an entry's hotkey chooses it; if several entries share the
//! letter, it moves between them and Enter chooses the highlighted one.
//! ↑/↓ move, Enter / Space / → choose (→ only opens submenus), ← and Esc
//! close the innermost menu. A click chooses or opens a submenu; there are
//! no pointer-move events in the kit, so submenus open on a click, not on
//! hover.
//!
//! Entries carry a value of the host's type `T`, handed back when chosen,
//! so the same menu serves a picker (go to a folder, add one) and Orchard
//! Commander (open, add, remove, organize). [`bookmark_items`] builds the
//! entries for a bookmark tree.

use orchard_grid::{CellRect, Flags, Frame, Style, elide_left};
use orchard_places::bookmarks::{self as store, Bookmark, Item};

use crate::form::cover;
use crate::input::{Key, KeyEvent, MouseEvent, MouseKind, Named};
use crate::widgets::{DrawCtx, Scrollbar, bottom, contains, fill, inset, put, put_elided, right, width};

/// The widest a bookmark's path column gets (cells).
pub const DETAIL_MAX: usize = 56;

/// What an entry is.
#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind<T> {
    /// Chosen: the menu closes and hands back the value.
    Action(T),
    Submenu(Vec<MenuItem<T>>),
    Separator,
    /// Shown, can't be chosen (an empty submenu's "(empty)").
    Note,
}

/// One entry: the hotkey column, the label, a muted detail on the right.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem<T> {
    /// Shown upper case in the hotkey column; typing it chooses the entry.
    /// Built-in entries use symbols ("+", "*"), so every letter stays free
    /// for the user's bookmarks.
    pub key: Option<char>,
    pub label: String,
    /// Right-aligned and muted (a path), shortened at the front to fit.
    pub detail: String,
    pub kind: ItemKind<T>,
}

impl<T> MenuItem<T> {
    pub fn action(key: Option<char>, label: impl Into<String>, detail: impl Into<String>, value: T) -> Self {
        MenuItem { key, label: label.into(), detail: detail.into(), kind: ItemKind::Action(value) }
    }

    pub fn submenu(key: Option<char>, label: impl Into<String>, items: Vec<MenuItem<T>>) -> Self {
        MenuItem { key, label: label.into(), detail: "▸".into(), kind: ItemKind::Submenu(items) }
    }

    pub fn separator() -> Self {
        MenuItem { key: None, label: String::new(), detail: String::new(), kind: ItemKind::Separator }
    }

    pub fn note(label: impl Into<String>) -> Self {
        MenuItem { key: None, label: label.into(), detail: String::new(), kind: ItemKind::Note }
    }

    fn selectable(&self) -> bool {
        matches!(self.kind, ItemKind::Action(_) | ItemKind::Submenu(_))
    }

    /// "P  Projects": the hotkey column (upper case, or a blank) and the label.
    pub fn text(&self) -> String {
        let key = self.key.map_or_else(|| " ".to_owned(), |k| k.to_uppercase().collect());
        format!("{key}  {}", self.label)
    }
}

/// The entries for a bookmark tree: bookmarks with their location as the
/// detail ("loc  ⇄  other" when they open both panels), folders as
/// submenus, separators. `value` says what choosing a bookmark does;
/// `None` leaves it out (e.g. a picker can only follow local folders), and
/// a folder left without entries that way goes too (an empty folder in the
/// file stays, with "(empty)" in its submenu).
pub fn bookmark_items<T>(items: &[Item], value: &impl Fn(&Bookmark) -> Option<T>) -> Vec<MenuItem<T>> {
    let mut out: Vec<MenuItem<T>> = Vec::new();
    for item in items {
        match item {
            Item::Separator { .. } => {
                if out.last().is_some_and(|i| !matches!(i.kind, ItemKind::Separator)) {
                    out.push(MenuItem::separator());
                }
            }
            Item::Folder(f) => {
                let mut children = bookmark_items(&f.items, value);
                if children.is_empty() && !f.items.is_empty() {
                    continue;
                }
                if children.is_empty() {
                    children.push(MenuItem::note("(empty)"));
                }
                out.push(MenuItem::submenu(store::hotkey_char(&f.title), store::plain(&f.title), children));
            }
            Item::Bookmark(b) => {
                let Some(v) = value(b) else { continue };
                let detail = match &b.target {
                    Some(t) => {
                        let half = (DETAIL_MAX - 5) / 2;
                        format!("{}  ⇄  {}", elide_left(&b.location.describe(), half), elide_left(&t.describe(), half))
                    }
                    None => elide_left(&b.location.describe(), DETAIL_MAX).into_owned(),
                };
                out.push(MenuItem::action(store::hotkey_char(&b.title), store::plain(&b.title), detail, v));
            }
        }
    }
    if out.last().is_some_and(|i| matches!(i.kind, ItemKind::Separator)) {
        out.pop();
    }
    out
}

/// What a key or click did to the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuOutcome<T> {
    Consumed,
    /// This entry was chosen; the menu is done.
    Chosen(T),
    /// Closed without a choice.
    Closed,
}

/// One open menu: the top one or a submenu.
#[derive(Clone, Debug)]
struct Level<T> {
    items: Vec<MenuItem<T>>,
    cursor: usize,
    top: usize,
    /// Where it opens (its top-left corner, moved to stay on the screen).
    col: u16,
    row: u16,
    /// The user has moved the highlight: the first hotkey press goes to
    /// the first entry with that key.
    moved: bool,
}

impl<T> Level<T> {
    fn new(items: Vec<MenuItem<T>>, col: u16, row: u16) -> Self {
        let cursor = items.iter().position(MenuItem::selectable).unwrap_or(0);
        Level { items, cursor, top: 0, col, row, moved: false }
    }

    /// The box, moved left / up to stay on `screen`.
    fn rect(&self, screen: CellRect) -> CellRect {
        let lw = self.items.iter().map(|i| width(&i.text())).max().unwrap_or(0);
        let dw = self.items.iter().map(|i| width(&i.detail)).max().unwrap_or(0);
        // border, space, label, gap (3) and detail, space, border
        let w = (lw + if dw > 0 { dw + 3 } else { 0 } + 4).min(screen.cols);
        let h = (self.items.len() as u16 + 2).min(screen.rows);
        let col = self.col.min(right(screen).saturating_sub(w)).max(screen.col);
        let row = self.row.min(bottom(screen).saturating_sub(h)).max(screen.row);
        CellRect::new(col, row, w, h)
    }

    fn visible(&self, screen: CellRect) -> usize {
        (self.rect(screen).rows.saturating_sub(2) as usize).max(1)
    }

    fn scroll_into_view(&mut self, screen: CellRect) {
        let v = self.visible(screen);
        if self.cursor < self.top {
            self.top = self.cursor;
        } else if self.cursor >= self.top + v {
            self.top = self.cursor + 1 - v;
        }
        self.top = self.top.min(self.items.len().saturating_sub(v));
    }

    fn step(&mut self, delta: isize) {
        let n = self.items.len() as isize;
        let mut i = self.cursor as isize;
        for _ in 0..n {
            i = (i + delta).rem_euclid(n);
            if self.items[i as usize].selectable() {
                self.cursor = i as usize;
                return;
            }
        }
    }

    fn entry_at(&self, screen: CellRect, col: u16, row: u16) -> Option<usize> {
        let inner = inset(self.rect(screen), 1, 1);
        if !contains(inner, col, row) {
            return None;
        }
        let i = self.top + (row - inner.row) as usize;
        (i < self.items.len()).then_some(i)
    }

    fn draw(&self, f: &mut Frame, screen: CellRect, ctx: &DrawCtx) {
        let c = &ctx.theme.colors;
        let r = self.rect(screen);
        cover(f, r, ctx);
        fill(f, r, c.surface);
        f.draw_box(r, orchard_grid::BoxStyle::Single, Style::new(c.border_active, c.surface));
        let inner = inset(r, 1, 1);
        for k in 0..inner.rows {
            let i = self.top + k as usize;
            let Some(item) = self.items.get(i) else { break };
            let row = CellRect::new(inner.col, inner.row + k, inner.cols, 1);
            if let ItemKind::Separator = item.kind {
                f.fill(row, '─', Style::new(c.border, c.surface));
                f.put_char(r.col, row.row, '├', Style::new(c.border_active, c.surface));
                f.put_char(right(r) - 1, row.row, '┤', Style::new(c.border_active, c.surface));
                continue;
            }
            let on = i == self.cursor && item.selectable();
            let style = if on {
                Style::new(c.cursor_fg, c.cursor_bg)
            } else if item.selectable() {
                Style::new(c.foreground, c.surface)
            } else {
                Style::new(c.muted, c.surface)
            };
            fill(f, row, style.bg);
            let key: String = item.key.map_or_else(|| " ".to_owned(), |k| k.to_uppercase().collect());
            let key_style = if on { style.with(Flags::BOLD) } else { Style::new(c.accent, c.surface).with(Flags::BOLD) };
            let x = put(f, row.col + 1, row.row, &key, key_style, 1) + 2;
            let end = put_elided(f, x, row.row, &item.label, style, right(row).saturating_sub(x + 1));
            if !item.detail.is_empty() {
                let room = right(row).saturating_sub(end + 3 + 1);
                let detail = elide_left(&item.detail, room as usize);
                let dx = right(row).saturating_sub(1 + width(&detail));
                let s = if on { style } else { Style::new(c.muted, c.surface) };
                put(f, dx, row.row, &detail, s, room);
            }
        }
        Scrollbar { total: self.items.len(), visible: inner.rows as usize, top: self.top }.draw(f, inner, ctx);
    }
}

/// An open menu and its open submenus.
#[derive(Clone, Debug)]
pub struct HotkeyMenu<T> {
    levels: Vec<Level<T>>,
}

impl<T: Clone> HotkeyMenu<T> {
    /// The menu for `items`, its top-left corner at (col, row) (moved to
    /// stay on the screen).
    pub fn new(items: Vec<MenuItem<T>>, col: u16, row: u16) -> Self {
        HotkeyMenu { levels: vec![Level::new(items, col, row)] }
    }

    /// The entries of the innermost open menu as shown ("S  Src"), without separators.
    pub fn labels(&self) -> Vec<String> {
        let Some(l) = self.levels.last() else { return Vec::new() };
        l.items.iter().filter(|i| !matches!(i.kind, ItemKind::Separator)).map(MenuItem::text).collect()
    }

    /// The highlighted entry of the innermost open menu.
    pub fn highlighted(&self) -> Option<String> {
        let l = self.levels.last()?;
        l.items.get(l.cursor).filter(|i| i.selectable()).map(MenuItem::text)
    }

    /// How many menus are open (1 = no submenu).
    pub fn depth(&self) -> usize {
        self.levels.len()
    }

    /// The innermost menu's box (for hit tests around it).
    pub fn rect(&self, screen: CellRect) -> CellRect {
        self.levels.last().map_or(CellRect::new(0, 0, 0, 0), |l| l.rect(screen))
    }

    /// Choose entry `i` of the innermost menu.
    fn activate(&mut self, i: usize, screen: CellRect) -> MenuOutcome<T> {
        let Some(level) = self.levels.last() else { return MenuOutcome::Closed };
        let Some(item) = level.items.get(i) else { return MenuOutcome::Consumed };
        match &item.kind {
            ItemKind::Action(v) => MenuOutcome::Chosen(v.clone()),
            ItemKind::Submenu(children) => {
                let r = level.rect(screen);
                let row = r.row + (i - level.top) as u16;
                let mut sub = Level::new(children.clone(), right(r), row);
                // No room on the right: open on the left of the parent.
                let w = sub.rect(screen).cols;
                if right(r) + w > right(screen) && r.col >= w {
                    sub.col = r.col - w;
                }
                self.levels.push(sub);
                MenuOutcome::Consumed
            }
            ItemKind::Separator | ItemKind::Note => MenuOutcome::Consumed,
        }
    }

    /// A typed hotkey: the only entry with it is chosen; several: move between them.
    fn hotkey(&mut self, ch: char, screen: CellRect) -> MenuOutcome<T> {
        let Some(level) = self.levels.last_mut() else { return MenuOutcome::Closed };
        let matches: Vec<usize> = (0..level.items.len()).filter(|&i| level.items[i].key == Some(ch) && level.items[i].selectable()).collect();
        match matches.as_slice() {
            [] => MenuOutcome::Consumed,
            [one] => {
                level.cursor = *one;
                self.activate(*one, screen)
            }
            several => {
                let next = match several.iter().position(|&i| i == level.cursor) {
                    Some(p) if level.moved => several[(p + 1) % several.len()],
                    _ => several[0],
                };
                level.cursor = next;
                level.moved = true;
                level.scroll_into_view(screen);
                MenuOutcome::Consumed
            }
        }
    }

    fn close_level(&mut self) -> MenuOutcome<T> {
        if self.levels.len() > 1 {
            self.levels.pop();
            MenuOutcome::Consumed
        } else {
            MenuOutcome::Closed
        }
    }

    pub fn handle_key(&mut self, ev: &KeyEvent, screen: CellRect) -> MenuOutcome<T> {
        if let Some(text) = ev.typed() {
            let mut chars = text.chars();
            if let (Some(c), None) = (chars.next(), chars.next())
                && !c.is_whitespace()
            {
                return self.hotkey(c.to_lowercase().next().unwrap_or(c), screen);
            }
        }
        let plain = ev.mods.none() || ev.mods == crate::input::Mods { shift: true, ..Default::default() };
        let Some(level) = self.levels.last_mut() else { return MenuOutcome::Closed };
        let before = level.cursor;
        match &ev.key {
            Key::Named(Named::Escape) => return self.close_level(),
            Key::Named(Named::Left) if plain => return self.close_level(),
            Key::Named(Named::Right) if plain => {
                let i = level.cursor;
                if matches!(level.items.get(i).map(|e| &e.kind), Some(ItemKind::Submenu(_))) {
                    return self.activate(i, screen);
                }
            }
            Key::Named(Named::Enter) | Key::Char(' ') if plain => {
                let i = level.cursor;
                return self.activate(i, screen);
            }
            Key::Named(Named::Up) if plain => level.step(-1),
            Key::Named(Named::Down) if plain => level.step(1),
            Key::Named(Named::Home | Named::PageUp) if plain => {
                level.cursor = level.items.len().saturating_sub(1);
                level.step(1);
            }
            Key::Named(Named::End | Named::PageDown) if plain => {
                level.cursor = 0;
                level.step(-1);
            }
            _ => {}
        }
        level.moved |= level.cursor != before;
        level.scroll_into_view(screen);
        MenuOutcome::Consumed
    }

    /// A click chooses (or opens a submenu); the wheel scrolls a long
    /// menu; a click outside every open menu closes them all.
    pub fn handle_mouse(&mut self, ev: &MouseEvent, screen: CellRect) -> MenuOutcome<T> {
        // The innermost menu under the pointer gets it.
        let Some(j) = self.levels.iter().rposition(|l| contains(l.rect(screen), ev.col, ev.row)) else {
            return if ev.kind == MouseKind::Down { MenuOutcome::Closed } else { MenuOutcome::Consumed };
        };
        match ev.kind {
            MouseKind::Scroll(dy) => {
                let level = &mut self.levels[j];
                let max = level.items.len().saturating_sub(level.visible(screen));
                level.top = (level.top as f32 - dy * 3.0).round().clamp(0.0, max as f32) as usize;
                MenuOutcome::Consumed
            }
            MouseKind::Down => {
                // Submenus below the one clicked close.
                self.levels.truncate(j + 1);
                let level = &mut self.levels[j];
                let Some(i) = level.entry_at(screen, ev.col, ev.row).filter(|&i| level.items[i].selectable()) else {
                    return MenuOutcome::Consumed;
                };
                level.moved |= level.cursor != i;
                level.cursor = i;
                self.activate(i, screen)
            }
            _ => MenuOutcome::Consumed,
        }
    }

    pub fn draw(&self, f: &mut Frame, screen: CellRect, ctx: &DrawCtx) {
        for level in &self.levels {
            level.draw(f, screen, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orchard_places::{Folder, Spec};

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Act {
        Open(String),
        Add,
    }

    fn local(p: &str) -> Spec {
        Spec::local(p)
    }

    fn sample() -> Vec<Item> {
        vec![
            Bookmark::new("&Src", local("/t/src"), None).into(),
            Bookmark::new("&Dst", local("/t/dst"), Some(local("/t/src"))).into(),
            Bookmark::new("&Sub", local("/t/src/sub"), None).into(),
            Item::separator(),
            Folder::new("&Work", vec![Bookmark::new("&Inside", local("/t"), None).into()]).into(),
            Folder::new("&Empty", vec![]).into(),
            Folder::new("&Remote", vec![Bookmark::new("&Web", Spec::server("sftp://web", "/"), None).into()]).into(),
            Bookmark::new("Fish && Chips", local("/t/f"), None).into(),
        ]
    }

    fn menu() -> HotkeyMenu<Act> {
        let open = |b: &Bookmark| match b.location.target() {
            Some(orchard_places::Target::Local { path }) => Some(Act::Open(path.to_owned())),
            _ => None,
        };
        let mut items = bookmark_items(&sample(), &open);
        items.push(MenuItem::separator());
        items.push(MenuItem::action(Some('+'), "Add current folder…", "", Act::Add));
        HotkeyMenu::new(items, 2, 1)
    }

    fn screen() -> CellRect {
        CellRect::new(0, 0, 100, 30)
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::char(c)
    }

    #[test]
    fn entries_from_bookmarks() {
        let m = menu();
        assert_eq!(m.labels(), ["S  Src", "D  Dst", "S  Sub", "W  Work", "E  Empty", "   Fish & Chips", "+  Add current folder…"]);
        assert_eq!(m.highlighted().as_deref(), Some("S  Src"));
        let dst = &m.levels[0].items[1];
        assert!(dst.detail.contains("  ⇄  "), "{}", dst.detail);
    }

    #[test]
    fn a_shared_hotkey_cycles_and_a_unique_one_chooses() {
        let mut m = menu();
        let s = screen();
        // Unmoved: the first press goes to the first "S".
        assert_eq!(m.handle_key(&key('s'), s), MenuOutcome::Consumed);
        assert_eq!(m.highlighted().as_deref(), Some("S  Src"));
        assert_eq!(m.handle_key(&key('S'), s), MenuOutcome::Consumed);
        assert_eq!(m.highlighted().as_deref(), Some("S  Sub"));
        m.handle_key(&key('s'), s);
        assert_eq!(m.highlighted().as_deref(), Some("S  Src"), "wraps");
        assert_eq!(m.handle_key(&KeyEvent::named(Named::Enter), s), MenuOutcome::Chosen(Act::Open("/t/src".into())));
        assert_eq!(m.handle_key(&key('d'), s), MenuOutcome::Chosen(Act::Open("/t/dst".into())));
        assert_eq!(m.handle_key(&key('+'), s), MenuOutcome::Chosen(Act::Add));
        assert_eq!(m.handle_key(&key('q'), s), MenuOutcome::Consumed, "no such key");
    }

    #[test]
    fn submenus_open_and_close() {
        let mut m = menu();
        let s = screen();
        assert_eq!(m.handle_key(&key('w'), s), MenuOutcome::Consumed);
        assert_eq!((m.depth(), m.labels()), (2, vec!["I  Inside".to_string()]));
        assert_eq!(m.handle_key(&KeyEvent::named(Named::Left), s), MenuOutcome::Consumed);
        assert_eq!(m.depth(), 1);
        m.handle_key(&key('e'), s);
        assert_eq!(m.labels(), ["   (empty)"]);
        assert_eq!(m.highlighted(), None, "nothing to choose");
        assert_eq!(m.handle_key(&KeyEvent::named(Named::Enter), s), MenuOutcome::Consumed);
        assert_eq!(m.handle_key(&KeyEvent::named(Named::Escape), s), MenuOutcome::Consumed);
        assert_eq!(m.handle_key(&KeyEvent::named(Named::Escape), s), MenuOutcome::Closed);
        // → opens a submenu, but doesn't choose a bookmark.
        let mut m = menu();
        assert_eq!(m.handle_key(&KeyEvent::named(Named::Right), s), MenuOutcome::Consumed);
        assert_eq!(m.depth(), 1);
        for _ in 0..3 {
            m.handle_key(&KeyEvent::named(Named::Down), s);
        }
        assert_eq!(m.highlighted().as_deref(), Some("W  Work"), "over the separator");
        m.handle_key(&KeyEvent::named(Named::Right), s);
        assert_eq!(m.depth(), 2);
    }

    #[test]
    fn clicks_choose_and_open() {
        let mut m = menu();
        let s = screen();
        let r = m.rect(s);
        assert_eq!((r.col, r.row), (2, 1));
        let click = |col, row| MouseEvent { kind: MouseKind::Down, col, row, x: 0.0, y: 0.0, double: false };
        // Entry 4: "W  Work".
        assert_eq!(m.handle_mouse(&click(5, r.row + 5), s), MenuOutcome::Consumed);
        assert_eq!(m.depth(), 2);
        let sub = m.rect(s);
        assert_eq!(sub.col, right(r), "beside its parent");
        assert_eq!(m.handle_mouse(&click(sub.col + 2, sub.row + 1), s), MenuOutcome::Chosen(Act::Open("/t".into())));
        // A click on the parent closes the submenu; outside closes all.
        m.handle_mouse(&click(5, r.row + 1), s);
        assert_eq!(m.depth(), 1);
        assert_eq!(m.handle_mouse(&click(90, 28), s), MenuOutcome::Closed);
    }

    #[test]
    fn stays_on_screen_and_scrolls() {
        let items: Vec<MenuItem<usize>> = (0..40).map(|i| MenuItem::action(None, format!("Item {i}"), "", i)).collect();
        let mut m = HotkeyMenu::new(items, 95, 3);
        let s = CellRect::new(0, 0, 100, 20);
        let r = m.rect(s);
        assert!(right(r) <= 100 && r.rows == 20);
        m.handle_key(&KeyEvent::named(Named::End), s);
        assert_eq!(m.highlighted().as_deref(), Some("   Item 39"));
        assert_eq!(m.levels[0].top, 40 - 18);
        let theme = orchard_theme::Theme::default_dark();
        let ctx = DrawCtx { theme: &theme, cell_px: (8.0, 16.0) };
        let mut f = Frame::new(100, 20, theme.colors.background);
        m.draw(&mut f, s, &ctx);
        assert!(f.row_text(18).contains("Item 39"));
    }
}
