//! "Add bookmark" over the picker: the title (an `&` sets the hotkey), the
//! folder, and the bookmark folder to put it in (a port of Orchard
//! Commander's dialog, without its other-panel option).

use std::path::{Path, PathBuf};

use orchard_grid::{CellRect, Frame, elide_left};
use orchard_places::Spec;

use crate::form::{self, Act, Choice, Ctl, Form, Kind, Popup, PopupOutcome, TextKind};
use crate::input::{KeyEvent, MouseEvent, MouseKind};
use crate::widgets::{DrawCtx, TextField, contains};

pub const TIP: &str = "Tip: an & in the title sets its hotkey, e.g. “&Projects” → P";
const WIDTH: u16 = 70;
const LABEL_W: u16 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Id {
    Title,
    PutIn,
    Add,
    Cancel,
}

/// What a key or click did to the dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddOutcome {
    Keep,
    Cancel,
    /// Add a bookmark with this title (hotkey marker included) to this
    /// bookmark folder (`None`: the top level).
    Add { title: String, folder: Option<String> },
}

#[derive(Clone, Debug)]
pub struct AddBookmark {
    pub title: TextField,
    pub dir: PathBuf,
    /// (label, folder id) for "Put in", from `bookmarks::folder_choices`.
    choices: Vec<(String, Option<String>)>,
    pub current: usize,
    focus: Option<Id>,
    popup: Option<Popup>,
}

impl AddBookmark {
    /// For the folder `dir`; `choices` from `orchard_places::bookmarks::folder_choices`.
    pub fn new(dir: &Path, choices: Vec<(String, Option<String>)>) -> Self {
        let name = Spec::local(dir.to_string_lossy()).name();
        AddBookmark { title: TextField::new(name), dir: dir.to_path_buf(), choices, current: 0, focus: Some(Id::Title), popup: None }
    }

    fn items(&self) -> Vec<Choice> {
        self.choices.iter().map(|(l, _)| Some(l.clone())).collect()
    }

    /// (outer box, form).
    fn layout(&self, screen: CellRect) -> (CellRect, Form<Id>) {
        let inner_w = WIDTH.min(screen.cols.saturating_sub(4)).saturating_sub(4);
        let tip = form::wrap(TIP, inner_w.max(1) as usize);
        let (outer, c) = form::dialog_rects(screen, WIDTH, 3 + 1 + tip.len() as u16 + 2);
        let mut f = Form::default();
        let line = |row: u16| CellRect::new(c.col, c.row + row, LABEL_W, 1);
        let field = |row: u16| CellRect::new(c.col + LABEL_W, c.row + row, c.cols.saturating_sub(LABEL_W), 1);
        f.label(line(0), "Title", TextKind::Label);
        f.label(line(1), "Folder", TextKind::Label);
        f.label(line(2), "Put in", TextKind::Label);
        let folder = field(1);
        f.label(folder, elide_left(&crate::picker::home_relative(&self.dir), folder.cols as usize).into_owned(), TextKind::Label);
        for (i, t) in tip.iter().enumerate() {
            f.label(CellRect::new(c.col, c.row + 4 + i as u16, c.cols, 1), t.clone(), TextKind::Muted);
        }
        f.ctls.push(Ctl::new(Id::Title, field(0), Kind::Text(self.title.clone(), String::new())));
        f.ctls.push(Ctl::new(Id::PutIn, field(2), Kind::Combo { items: self.items(), current: self.current }));
        let buttons = form::ButtonRow::new(&["Add bookmark", "Cancel"]);
        let row = CellRect::new(c.col, c.row + c.rows.saturating_sub(1), c.cols, 1);
        for (id, r) in [Id::Add, Id::Cancel].into_iter().zip(buttons.rects(row)) {
            let label = if id == Id::Add { "Add bookmark" } else { "Cancel" };
            f.ctls.push(Ctl::new(id, r, Kind::Button(label.into())));
        }
        (outer, f)
    }

    fn finish(&self) -> AddOutcome {
        let title = self.title.text.trim();
        let title = if title.is_empty() { "Bookmark".to_owned() } else { title.to_owned() };
        AddOutcome::Add { title, folder: self.choices.get(self.current).and_then(|(_, id)| id.clone()) }
    }

    fn act(&mut self, act: Act<Id>, screen: CellRect) -> AddOutcome {
        match act {
            Act::Escape | Act::Press(Id::Cancel) => AddOutcome::Cancel,
            Act::Submit | Act::Press(Id::Add) => self.finish(),
            Act::Step(Id::PutIn, d) => {
                self.current = (self.current as i64 + d as i64).clamp(0, self.choices.len().saturating_sub(1) as i64) as usize;
                AddOutcome::Keep
            }
            Act::Open(Id::PutIn) => {
                let (_, f) = self.layout(screen);
                if let Some(ctl) = f.ctl(Id::PutIn) {
                    self.popup = Some(Popup::open(ctl.rect, &self.items(), self.current));
                }
                AddOutcome::Keep
            }
            Act::Caret(Id::Title, r, col) => {
                self.title.click(r, col);
                AddOutcome::Keep
            }
            _ => AddOutcome::Keep,
        }
    }

    pub fn key(&mut self, ev: &KeyEvent, screen: CellRect) -> AddOutcome {
        if let Some(mut p) = self.popup.take() {
            match p.handle_key(ev, &self.items()) {
                PopupOutcome::Consumed => self.popup = Some(p),
                PopupOutcome::Chosen(i) => self.current = i,
                PopupOutcome::Closed => {}
            }
            return AddOutcome::Keep;
        }
        let (_, f) = self.layout(screen);
        let field = (self.focus == Some(Id::Title)).then_some(&mut self.title);
        let act = f.handle_key(&mut self.focus, ev, field);
        self.act(act, screen)
    }

    pub fn mouse(&mut self, ev: &MouseEvent, screen: CellRect) -> AddOutcome {
        if let Some(mut p) = self.popup.take() {
            match p.handle_mouse(ev, &self.items(), screen) {
                PopupOutcome::Consumed => self.popup = Some(p),
                PopupOutcome::Chosen(i) => self.current = i,
                PopupOutcome::Closed => {}
            }
            return AddOutcome::Keep;
        }
        let (outer, f) = self.layout(screen);
        if ev.kind == MouseKind::Down && !contains(outer, ev.col, ev.row) {
            return AddOutcome::Keep;
        }
        let act = f.handle_mouse(&mut self.focus, ev);
        self.act(act, screen)
    }

    pub fn draw(&self, f: &mut Frame, screen: CellRect, ctx: &DrawCtx) {
        let (outer, form) = self.layout(screen);
        form::dialog_box(f, outer, "Add bookmark", ctx);
        let mut focus = self.focus;
        form.fix_focus(&mut focus);
        form.draw(f, focus, ctx);
        if let Some(p) = &self.popup {
            p.draw(f, &self.items(), screen, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Named;

    fn choices() -> Vec<(String, Option<String>)> {
        vec![("Top level".into(), None), ("Work".into(), Some("w1".into())), ("Work › Old".into(), Some("o1".into()))]
    }

    #[test]
    fn title_from_the_folder_and_a_folder_to_put_it_in() {
        let screen = CellRect::new(0, 0, 100, 30);
        let mut a = AddBookmark::new(Path::new("/src/projects"), choices());
        assert_eq!(a.title.text, "projects");
        a.title.set_text("");
        for c in "&Proj".chars() {
            a.key(&KeyEvent::char(c), screen);
        }
        a.key(&KeyEvent::named(Named::Tab), screen);
        a.key(&KeyEvent::named(Named::Right), screen);
        assert_eq!(a.current, 1);
        // The list: Enter opens it, ↓ and Enter choose.
        a.key(&KeyEvent::named(Named::Enter), screen);
        assert!(a.popup.is_some());
        a.key(&KeyEvent::named(Named::Down), screen);
        a.key(&KeyEvent::named(Named::Enter), screen);
        assert_eq!((a.current, a.popup.is_none()), (2, true));
        a.key(&KeyEvent::named(Named::Tab), screen);
        assert_eq!(
            a.key(&KeyEvent::named(Named::Enter), screen),
            AddOutcome::Add { title: "&Proj".into(), folder: Some("o1".into()) }
        );
        assert_eq!(a.key(&KeyEvent::named(Named::Escape), screen), AddOutcome::Cancel);
    }

    #[test]
    fn an_empty_title_is_named_bookmark() {
        let screen = CellRect::new(0, 0, 100, 30);
        let mut a = AddBookmark::new(Path::new("/"), choices());
        assert_eq!(a.title.text, "/");
        a.title.set_text("  ");
        assert_eq!(a.key(&KeyEvent::named(Named::Enter), screen), AddOutcome::Add { title: "Bookmark".into(), folder: None });
    }

    #[test]
    fn draws_its_labels_and_buttons() {
        let theme = orchard_theme::Theme::default_dark();
        let ctx = DrawCtx { theme: &theme, cell_px: (8.0, 16.0) };
        let screen = CellRect::new(0, 0, 90, 24);
        let a = AddBookmark::new(Path::new("/src/projects"), choices());
        let mut f = Frame::new(90, 24, theme.colors.background);
        a.draw(&mut f, screen, &ctx);
        let text: Vec<String> = (0..24).map(|r| f.row_text(r)).collect();
        let all = text.join("\n");
        for want in ["Add bookmark", "Title", "projects", "Put in", "Top level", "Tip: an & in the title", "Cancel"] {
            assert!(all.contains(want), "{want} missing:\n{all}");
        }
    }
}
