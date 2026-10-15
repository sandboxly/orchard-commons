//! The file and folder picker the Orchard apps share, over the dialog or
//! view that opened it: a places column on the left (home, the standard
//! folders, drives or mounted volumes, then Orchard Commander's bookmarks
//! of local folders), the folder's listing on the right, buttons below.
//!
//! - Pick a folder ([`Options::folder`]): folders only; "Use this folder"
//!   (F2, ⌘Enter) chooses the folder shown.
//! - Pick files ([`Options::files`]): folders and the files the filter
//!   lets through; Enter chooses the file under the cursor, or with
//!   [`Options::multi`] the marked entries (Space / Insert mark, `*` marks
//!   every file).
//!
//! Keys: ↑↓ PgUp PgDn Home End move, Enter / → open, ⌫ / ← go up, typing
//! jumps to a name, `~` goes home, Tab moves between the places column, the
//! list and the buttons, ⌘D (Ctrl+D) opens the bookmark menu, which can also
//! add the folder shown to the bookmarks (in Orchard Commander's file, so
//! every app sees it), Esc cancels. With [`Options::add_folder`], F3 and
//! Shift+Enter hand back the folder under the cursor (or the one shown) as
//! [`Outcome::AddFolder`]. On Windows ".." from a drive root leads to the
//! drives.
//!
//! Folders are listed, and places and bookmarks read, off the UI thread
//! (a slow or network disk never stalls the window); results arrive through
//! [`Tasks`] and the app's waker. Call [`Picker::poll`] when the app's
//! tasks wake it (its `process`), and redraw if it returns `true`. Results
//! for a folder the picker has left are dropped. Bookmarks are read each
//! time a picker opens, so changes made in Orchard Commander show up.
//!
//! ```ignore
//! // Open: e.g. "Choose…" next to a folder field.
//! self.picker = Some(Picker::new(Options::folder(start), app.tasks.waker()));
//! // In `process`: redraw when it has news.
//! changed |= self.picker.as_mut().is_some_and(Picker::poll);
//! // Keys (the mouse alike), then draw it over everything with `draw`.
//! match picker.key(ev, screen) {
//!     Outcome::Keep => {}
//!     Outcome::Cancel => self.picker = None,
//!     Outcome::Folder(dir) => { /* use it */ self.picker = None }
//!     _ => {}
//! }
//! ```

mod add;
mod listing;

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use orchard_grid::{CellRect, Flags, Frame, Style, elide_left};
use orchard_places::bookmarks::{self as store, Item, LocalItem};
use orchard_places::{Bookmark, Place, Spec, Target};

pub use add::{AddBookmark, AddOutcome};
pub use listing::{Entry, Filter, Location, home_relative, io_reason, natural_cmp};

use crate::form::{self, ButtonRow, RowOutcome};
use crate::input::{Key, KeyEvent, MouseEvent, MouseKind, Named, primary_label};
use crate::menu::{self, HotkeyMenu, MenuItem, MenuOutcome};
use crate::tasks::{Tasks, Waker};
use crate::widgets::{DrawCtx, Scrollbar, bottom, contains, fill, put, put_elided, right, width};

/// Type-ahead: keys this close together extend the search.
const TYPE_AHEAD: Duration = Duration::from_millis(900);
/// The places column's width, with the line after it.
const SIDE_W: u16 = 22;
/// Narrower than this, the places column is hidden (⌘D still works).
const MIN_COLS_FOR_SIDE: u16 = 64;

/// What the picker picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// A folder (the one shown).
    Folder,
    /// A file, or with `multi` the marked entries.
    Files { multi: bool },
}

/// How a picker opens and what it shows.
#[derive(Clone)]
pub struct Options {
    pub pick: Pick,
    /// The dialog's title ("Choose a folder").
    pub title: String,
    /// Where it opens (the nearest folder above it that exists); the home
    /// folder if empty.
    pub start: PathBuf,
    /// Files mode: which files show (all without one).
    pub filter: Option<Filter>,
    /// Files mode with `multi`: folders can be marked too (they come back
    /// among the chosen paths, for the app to look into).
    pub mark_folders: bool,
    /// F3 / Shift+Enter hand back a folder as [`Outcome::AddFolder`].
    pub add_folder: bool,
    /// Names starting with "." show too.
    pub hidden: bool,
    /// The main button ("Use this folder", "Open file").
    pub accept: String,
    /// The bookmarks file; `None` hides bookmarks (default: Orchard
    /// Commander's, [`orchard_places::bookmarks_file`]).
    pub bookmarks: Option<PathBuf>,
    /// The places column (home, standard folders, drives, volumes).
    pub places: bool,
}

impl Options {
    /// Pick a folder.
    pub fn folder(start: impl Into<PathBuf>) -> Self {
        Options {
            pick: Pick::Folder,
            title: "Choose a folder".into(),
            start: start.into(),
            filter: None,
            mark_folders: false,
            add_folder: false,
            hidden: false,
            accept: "Use this folder".into(),
            bookmarks: Some(orchard_places::bookmarks_file()),
            places: true,
        }
    }

    /// Pick a file (see [`multi`](Self::multi) for several).
    pub fn files(start: impl Into<PathBuf>) -> Self {
        Options { pick: Pick::Files { multi: false }, title: "Open a file".into(), accept: "Open file".into(), ..Options::folder(start) }
    }

    /// Several files: marking with Space / Insert / `*`.
    pub fn multi(mut self) -> Self {
        self.pick = Pick::Files { multi: true };
        if self.accept == "Open file" {
            self.accept = "Open files".into();
        }
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// The main button's label: an action and its object ("Add to playlist").
    pub fn accept(mut self, label: impl Into<String>) -> Self {
        self.accept = label.into();
        self
    }

    pub fn filter(mut self, f: impl Fn(&Path) -> bool + Send + Sync + 'static) -> Self {
        self.filter = Some(std::sync::Arc::new(f));
        self
    }

    /// Only files with one of these extensions (any case, without the dot).
    pub fn extensions(self, exts: &[&str]) -> Self {
        let exts: Vec<String> = exts.iter().map(|e| e.to_lowercase()).collect();
        self.filter(move |p| p.extension().is_some_and(|e| exts.contains(&e.to_string_lossy().to_lowercase())))
    }

    pub fn mark_folders(mut self) -> Self {
        self.mark_folders = true;
        self
    }

    pub fn add_folder(mut self) -> Self {
        self.add_folder = true;
        self
    }

    pub fn hidden(mut self, yes: bool) -> Self {
        self.hidden = yes;
        self
    }

    /// Another bookmarks file (tests), or `None` for none.
    pub fn bookmarks(mut self, file: Option<PathBuf>) -> Self {
        self.bookmarks = file;
        self
    }

    /// Without the standard places (tests, or a picker that wants none).
    pub fn no_places(mut self) -> Self {
        self.places = false;
        self
    }

    fn multi_pick(&self) -> bool {
        self.pick == Pick::Files { multi: true }
    }

    fn files_mode(&self) -> bool {
        matches!(self.pick, Pick::Files { .. })
    }
}

/// What the picker asks of the app that opened it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Keep,
    Cancel,
    /// Folder mode: this folder.
    Folder(PathBuf),
    /// Files mode: these files (with [`Options::mark_folders`], marked
    /// folders too), in the listing's order.
    Files(Vec<PathBuf>),
    /// [`Options::add_folder`]: F3 / Shift+Enter on this folder. The picker
    /// stays open unless the app closes it.
    AddFolder(PathBuf),
}

/// Where the keyboard is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Places,
    List,
    Buttons,
}

/// A row of the places column.
#[derive(Clone, Debug, PartialEq)]
enum Side {
    Heading(&'static str),
    Place(Place),
    Bookmark { title: String, key: Option<char>, path: PathBuf, depth: u16 },
    Folder { title: String, key: Option<char>, id: String, open: bool, depth: u16 },
    Note(String),
    Blank,
}

impl Side {
    fn selectable(&self) -> bool {
        matches!(self, Side::Place(_) | Side::Bookmark { .. } | Side::Folder { .. })
    }

    fn key(&self) -> Option<char> {
        match self {
            Side::Bookmark { key, .. } | Side::Folder { key, .. } => *key,
            _ => None,
        }
    }
}

/// What a bookmark menu entry does.
#[derive(Clone, Debug, PartialEq, Eq)]
enum MenuAct {
    Go(PathBuf),
    Add,
}

/// A note under the list: what happened to a bookmark being added.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Note {
    Done(String),
    Failed(String),
}

/// Where everything goes, for a screen.
#[derive(Clone, Copy, Debug)]
struct Rects {
    outer: CellRect,
    /// The places column (empty when hidden).
    side: CellRect,
    /// The current folder's path.
    header: CellRect,
    list: CellRect,
    hint: CellRect,
    buttons: CellRect,
}

pub struct Picker {
    opts: Options,
    location: Location,
    /// `None` while listing.
    entries: Option<Vec<Entry>>,
    error: Option<String>,
    cursor: usize,
    top: usize,
    /// Marked rows (multi).
    marked: BTreeSet<usize>,
    /// Put the cursor on this entry when the listing arrives (the folder we came from).
    select: Option<Location>,
    /// The listing asked for last; older results are dropped.
    generation: u64,
    search: String,
    search_at: Option<Instant>,
    focus: Focus,
    buttons: ButtonRow,
    places: Option<Vec<Place>>,
    /// The whole tree (for "Put in" and "already bookmarked"); `None` until read.
    bookmarks: Option<Vec<Item>>,
    local: Vec<LocalItem>,
    /// Bookmark folders open in the places column.
    expanded: HashSet<String>,
    side_cursor: usize,
    side_top: usize,
    menu: Option<HotkeyMenu<MenuAct>>,
    add: Option<AddBookmark>,
    note: Option<Note>,
    tasks: Tasks<Picker>,
}

impl Picker {
    /// Open a picker; `waker` is the app's (`Tasks::waker`), so results
    /// wake its event loop.
    pub fn new(opts: Options, waker: Waker) -> Picker {
        let buttons = ButtonRow::new(&[opts.accept.as_str(), "Cancel"]);
        let start = match &opts.start {
            s if s.as_os_str().is_empty() => dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")),
            s if s.is_relative() => dirs::home_dir().unwrap_or_default().join(s),
            s => s.clone(),
        };
        let mut p = Picker {
            location: Location::Dir(start.clone()),
            entries: None,
            error: None,
            cursor: 0,
            top: 0,
            marked: BTreeSet::new(),
            select: None,
            generation: 0,
            search: String::new(),
            search_at: None,
            focus: Focus::List,
            buttons,
            places: None,
            bookmarks: None,
            local: Vec::new(),
            expanded: HashSet::new(),
            side_cursor: 0,
            side_top: 0,
            menu: None,
            add: None,
            note: None,
            tasks: Tasks::new(1, waker),
            opts,
        };
        p.list_start(start);
        if p.opts.places {
            let tasks = p.tasks.clone();
            // Its own thread: a network mount can take a while to answer.
            let _ = std::thread::Builder::new().name("picker-places".into()).spawn(move || {
                let places = orchard_places::places();
                tasks.post(move |p| p.places = Some(places));
            });
        }
        p.reload_bookmarks();
        p
    }

    /// Apply what arrived from the background; `true`: redraw.
    pub fn poll(&mut self) -> bool {
        let done = self.tasks.take();
        let any = !done.is_empty();
        for f in done {
            f(self);
        }
        any
    }

    pub fn location(&self) -> &Location {
        &self.location
    }

    /// The listing (`None` while it is read; empty when it failed).
    pub fn entries(&self) -> Option<&[Entry]> {
        self.entries.as_deref()
    }

    /// Why the folder couldn't be listed.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// The marked entries' paths, in the listing's order.
    pub fn marked(&self) -> Vec<PathBuf> {
        self.marked.iter().filter_map(|&r| self.entry(r)).map(|e| e.path.clone()).collect()
    }

    /// The places (`None` until read).
    pub fn places(&self) -> Option<&[Place]> {
        self.places.as_deref()
    }

    /// The local bookmarks the picker shows (empty until read).
    pub fn bookmarks(&self) -> &[LocalItem] {
        &self.local
    }

    /// Still reading the listing, the places or the bookmarks (headless
    /// screenshots wait for this).
    pub fn loading(&self) -> bool {
        self.entries.is_none() || (self.opts.places && self.places.is_none()) || (self.opts.bookmarks.is_some() && self.bookmarks.is_none())
    }

    /// Whether the bookmark menu is open.
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// Whether the "Add bookmark" dialog is open.
    pub fn adding(&self) -> bool {
        self.add.is_some()
    }

    // -- Background work --------------------------------------------------

    fn list_spec(&self) -> listing::ListSpec {
        listing::ListSpec { files: self.opts.files_mode(), filter: self.opts.filter.clone(), hidden: self.opts.hidden }
    }

    /// Start a listing; `f` makes it (blocking) on its own thread, as a
    /// slow disk would keep a pooled worker busy.
    fn start_listing(&mut self, f: impl FnOnce() -> (Location, Result<Vec<Entry>, String>) + Send + 'static) {
        self.generation += 1;
        let generation = self.generation;
        let tasks = self.tasks.clone();
        let _ = std::thread::Builder::new().name("picker-list".into()).spawn(move || {
            let (location, result) = f();
            tasks.post(move |p| p.listed(generation, location, result));
        });
    }

    /// The first listing: `start` or the nearest folder above it; when
    /// `start` is a file, the cursor goes on it.
    fn list_start(&mut self, start: PathBuf) {
        self.select = Some(Location::Dir(start.clone()));
        let spec = self.list_spec();
        self.start_listing(move || listing::list_nearest(&start, &spec));
    }

    /// Show `location`; `select`: the entry to put the cursor on once listed.
    fn go(&mut self, location: Location, select: Option<Location>) {
        self.location = location.clone();
        self.select = select;
        self.entries = None;
        self.error = None;
        self.marked.clear();
        self.cursor = 0;
        self.top = 0;
        self.search.clear();
        let spec = self.list_spec();
        self.start_listing(move || {
            let result = listing::list(&location, &spec);
            (location, result)
        });
    }

    fn listed(&mut self, generation: u64, location: Location, result: Result<Vec<Entry>, String>) {
        if generation != self.generation {
            return; // the picker has moved on
        }
        self.location = location;
        let entries = match result {
            Ok(e) => e,
            Err(e) => {
                self.error = Some(e);
                Vec::new()
            }
        };
        let skip = usize::from(self.has_parent());
        if let Some(want) = self.select.take() {
            let found = entries.iter().position(|e| want.same(&Location::Dir(e.path.clone())));
            self.cursor = found.map_or(0, |i| i + skip);
        }
        self.entries = Some(entries);
    }

    /// Read the bookmarks again (after a change, or to catch up with
    /// Orchard Commander's).
    pub fn reload_bookmarks(&mut self) {
        let Some(path) = self.opts.bookmarks.clone() else { return };
        self.tasks.spawn(
            move || store::load(&path),
            |p, items| {
                p.local = store::local_tree(&items);
                p.bookmarks = Some(items);
                p.clamp_side();
            },
        );
    }

    fn add_bookmark(&mut self, title: String, folder: Option<String>) {
        let (Some(path), Some(dir)) = (self.opts.bookmarks.clone(), self.location.dir().map(Path::to_path_buf)) else { return };
        let shown = store::plain(&title);
        self.tasks.spawn(
            move || store::add_bookmark(&path, Bookmark::local(title, &dir), folder.as_deref()),
            move |p, result| {
                p.note = Some(match result {
                    Ok(()) => Note::Done(format!("Bookmark “{shown}” added")),
                    Err(e) => Note::Failed(format!("Can't add the bookmark: {e}")),
                });
                p.reload_bookmarks();
            },
        );
    }

    // -- The listing -------------------------------------------------------

    fn has_parent(&self) -> bool {
        self.location.parent().is_some()
    }

    /// Rows: ".." (unless at the top), then the entries.
    fn rows(&self) -> usize {
        self.entries.as_ref().map_or(0, Vec::len) + usize::from(self.has_parent())
    }

    /// The entry at row `r` (`None` for "..").
    fn entry(&self, r: usize) -> Option<&Entry> {
        let i = r.checked_sub(usize::from(self.has_parent()))?;
        self.entries.as_ref()?.get(i)
    }

    fn move_cursor(&mut self, delta: isize) {
        let n = self.rows();
        if n > 0 {
            self.cursor = (self.cursor as isize + delta).clamp(0, n as isize - 1) as usize;
        }
    }

    /// The first row shown for `h` rows, keeping `cursor` in view.
    fn top_for(&self, top: usize, cursor: usize, h: usize) -> usize {
        let h = h.max(1);
        let top = if cursor < top {
            cursor
        } else if cursor >= top + h {
            cursor + 1 - h
        } else {
            top
        };
        top.min(self.rows().saturating_sub(h))
    }

    fn up(&mut self) {
        if let Some(p) = self.location.parent() {
            let from = self.location.clone();
            self.go(p, Some(from));
        }
    }

    fn home(&mut self) {
        if let Some(h) = dirs::home_dir() {
            self.go(Location::Dir(h), None);
        }
    }

    /// Whether a row can be marked: files, and folders if asked for;
    /// never ".." or a drive.
    fn markable(&self, r: usize) -> bool {
        self.location != Location::Drives && self.entry(r).is_some_and(|e| !e.is_dir || self.opts.mark_folders)
    }

    fn toggle_mark(&mut self) {
        if self.markable(self.cursor) && !self.marked.remove(&self.cursor) {
            self.marked.insert(self.cursor);
        }
        self.move_cursor(1);
    }

    /// Mark every file (not the folders); again to unmark all.
    fn mark_all(&mut self) {
        let files: BTreeSet<usize> = (0..self.rows()).filter(|&r| self.entry(r).is_some_and(|e| !e.is_dir)).collect();
        self.marked = if self.marked == files { BTreeSet::new() } else { files };
    }

    /// Enter on the list: ".." goes up, folders open, files are chosen
    /// (with marks: the marked ones).
    fn activate(&mut self) -> Outcome {
        if self.cursor == 0 && self.has_parent() {
            self.up();
            return Outcome::Keep;
        }
        if !self.marked.is_empty() {
            return Outcome::Files(self.marked());
        }
        let Some(e) = self.entry(self.cursor).cloned() else { return Outcome::Keep };
        if e.is_dir {
            self.go(Location::Dir(e.path), None);
            return Outcome::Keep;
        }
        match self.opts.pick {
            Pick::Files { .. } => Outcome::Files(vec![e.path]),
            Pick::Folder => Outcome::Keep,
        }
    }

    /// The main button: the folder shown, or the files (as Enter, except
    /// that it never goes up).
    fn accept(&mut self) -> Outcome {
        match (self.opts.pick, &self.location) {
            (_, Location::Drives) => Outcome::Keep,
            (Pick::Folder, Location::Dir(d)) => Outcome::Folder(d.clone()),
            (Pick::Files { .. }, _) if self.cursor == 0 && self.has_parent() && self.marked.is_empty() => Outcome::Keep,
            (Pick::Files { .. }, _) => self.activate(),
        }
    }

    /// The folder under the cursor, or the one shown.
    fn add_folder(&self) -> Outcome {
        if !self.opts.add_folder {
            return Outcome::Keep;
        }
        match (self.entry(self.cursor), &self.location) {
            (_, Location::Drives) => Outcome::Keep,
            (Some(e), _) if e.is_dir => Outcome::AddFolder(e.path.clone()),
            (_, Location::Dir(d)) => Outcome::AddFolder(d.clone()),
        }
    }

    /// Typing jumps to the first entry starting with what was typed.
    fn type_ahead(&mut self, text: &str, now: Instant) {
        if self.search_at.is_none_or(|t| now.duration_since(t) > TYPE_AHEAD) {
            self.search.clear();
        }
        self.search_at = Some(now);
        self.search.push_str(&text.to_lowercase());
        let found = (0..self.rows()).find(|&r| self.entry(r).is_some_and(|e| e.name.to_lowercase().starts_with(&self.search)));
        if let Some(r) = found {
            self.cursor = r;
        }
    }

    fn searching(&self, now: Instant) -> bool {
        !self.search.is_empty() && self.search_at.is_some_and(|t| now.duration_since(t) <= TYPE_AHEAD)
    }

    // -- The places column --------------------------------------------------

    fn side_rows(&self) -> Vec<Side> {
        fn tree(items: &[LocalItem], depth: u16, open: &HashSet<String>, out: &mut Vec<Side>) {
            for item in items {
                match item {
                    LocalItem::Bookmark(b) => out.push(Side::Bookmark { title: b.title.clone(), key: b.hotkey, path: b.path.clone(), depth }),
                    LocalItem::Folder(f) => {
                        let is_open = open.contains(&f.id);
                        out.push(Side::Folder { title: f.title.clone(), key: f.hotkey, id: f.id.clone(), open: is_open, depth });
                        if is_open {
                            tree(&f.items, depth + 1, open, out);
                        }
                    }
                    LocalItem::Separator => {}
                }
            }
        }
        let mut out = Vec::new();
        if self.opts.places {
            out.push(Side::Heading("Places"));
            match &self.places {
                None => out.push(Side::Note("Loading…".into())),
                Some(places) => out.extend(places.iter().cloned().map(Side::Place)),
            }
        }
        if self.opts.bookmarks.is_some() {
            if !out.is_empty() {
                out.push(Side::Blank);
            }
            out.push(Side::Heading("Bookmarks"));
            match &self.bookmarks {
                None => out.push(Side::Note("Loading…".into())),
                Some(_) if self.local.is_empty() => {
                    out.push(Side::Note("No bookmarks yet".into()));
                    out.push(Side::Note(primary_label("⌘D adds this folder")));
                }
                Some(_) => tree(&self.local, 0, &self.expanded, &mut out),
            }
        }
        out
    }

    /// Keep the column's cursor on a row that can be chosen.
    fn clamp_side(&mut self) {
        let rows = self.side_rows();
        if rows.get(self.side_cursor).is_some_and(Side::selectable) {
            return;
        }
        self.side_cursor = rows.iter().position(Side::selectable).unwrap_or(0);
    }

    fn side_step(&mut self, delta: isize) {
        let rows = self.side_rows();
        let mut i = self.side_cursor as isize;
        loop {
            i += delta;
            if i < 0 || i >= rows.len() as isize {
                return;
            }
            if rows[i as usize].selectable() {
                self.side_cursor = i as usize;
                return;
            }
        }
    }

    /// Go to the place or bookmark at column row `i`, or open / close the
    /// bookmark folder there.
    fn side_activate(&mut self, i: usize) {
        match self.side_rows().get(i).cloned() {
            Some(Side::Place(p)) => {
                self.side_cursor = i;
                self.go(Location::Dir(p.path), None);
                self.focus = Focus::List;
            }
            Some(Side::Bookmark { path, .. }) => {
                self.side_cursor = i;
                self.go(Location::Dir(path), None);
                self.focus = Focus::List;
            }
            Some(Side::Folder { id, open, .. }) => {
                self.side_cursor = i;
                if open {
                    self.expanded.remove(&id);
                } else {
                    self.expanded.insert(id);
                }
            }
            _ => {}
        }
    }

    /// A letter in the column: the only bookmark with that hotkey opens;
    /// several: move between them.
    fn side_hotkey(&mut self, ch: char) {
        let rows = self.side_rows();
        let hits: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].key() == Some(ch)).collect();
        match hits.as_slice() {
            [] => {}
            [one] => self.side_activate(*one),
            several => {
                let at = several.iter().position(|&i| i == self.side_cursor);
                self.side_cursor = at.map_or(several[0], |p| several[(p + 1) % several.len()]);
            }
        }
    }

    fn side_shown(&self) -> bool {
        self.opts.places || self.opts.bookmarks.is_some()
    }

    // -- The bookmark menu ----------------------------------------------------

    fn open_menu(&mut self, screen: CellRect) {
        let Some(items) = &self.bookmarks else { return };
        let go = |b: &Bookmark| match b.location.target() {
            Some(Target::Local { path }) => Some(MenuAct::Go(PathBuf::from(path))),
            _ => None,
        };
        let mut entries = menu::bookmark_items(items, &go);
        let here = self.location.dir().map(|d| Spec::local(d.to_string_lossy()));
        let known = here.as_ref().is_some_and(|s| store::find_location(items, s.as_ref()).is_some());
        if here.is_some() && !known {
            if !entries.is_empty() {
                entries.push(MenuItem::separator());
            }
            entries.push(MenuItem::action(Some('+'), "Add current folder…", "", MenuAct::Add));
        }
        if entries.is_empty() {
            entries.push(MenuItem::note("No bookmarks to open here"));
        }
        let r = self.rects(screen);
        self.menu = Some(HotkeyMenu::new(entries, r.header.col, r.header.row + 1));
    }

    fn menu_chose(&mut self, act: MenuAct) {
        self.menu = None;
        match act {
            MenuAct::Go(path) => {
                self.go(Location::Dir(path), None);
                self.focus = Focus::List;
            }
            MenuAct::Add => {
                if let (Some(items), Some(dir)) = (&self.bookmarks, self.location.dir()) {
                    self.add = Some(AddBookmark::new(dir, store::folder_choices(items)));
                }
            }
        }
    }

    fn add_done(&mut self, out: AddOutcome) {
        match out {
            AddOutcome::Keep => {}
            AddOutcome::Cancel => self.add = None,
            AddOutcome::Add { title, folder } => {
                self.add = None;
                self.add_bookmark(title, folder);
            }
        }
    }

    // -- Layout --------------------------------------------------------------

    fn rects(&self, screen: CellRect) -> Rects {
        let h = screen.rows.saturating_sub(4).clamp(10, 34);
        let (outer, c) = form::dialog_rects(screen, 100, h);
        let body_h = c.rows.saturating_sub(3);
        let side_w = if self.side_shown() && c.cols >= MIN_COLS_FOR_SIDE { SIDE_W } else { 0 };
        let side = CellRect::new(c.col, c.row, side_w.saturating_sub(2), body_h);
        let lx = c.col + side_w;
        let lw = c.cols.saturating_sub(side_w);
        Rects {
            outer,
            side,
            header: CellRect::new(lx, c.row, lw, 1.min(body_h)),
            list: CellRect::new(lx, c.row + 1, lw, body_h.saturating_sub(1)),
            hint: CellRect::new(c.col, c.row + c.rows.saturating_sub(2), c.cols, 1),
            buttons: CellRect::new(c.col, c.row + c.rows.saturating_sub(1), c.cols, 1),
        }
    }

    fn cycle_focus(&mut self, back: bool, side: bool) {
        let order: &[Focus] = if side { &[Focus::Places, Focus::List, Focus::Buttons] } else { &[Focus::List, Focus::Buttons] };
        let at = order.iter().position(|&f| f == self.focus).unwrap_or(0);
        let n = order.len();
        self.focus = order[if back { (at + n - 1) % n } else { (at + 1) % n }];
        if self.focus == Focus::Places {
            self.clamp_side();
        }
    }

    // -- Input ---------------------------------------------------------------

    pub fn key(&mut self, ev: &KeyEvent, screen: CellRect) -> Outcome {
        self.key_at(ev, screen, Instant::now())
    }

    fn key_at(&mut self, ev: &KeyEvent, screen: CellRect, now: Instant) -> Outcome {
        self.poll();
        if let Some(a) = &mut self.add {
            let out = a.key(ev, screen);
            self.add_done(out);
            return Outcome::Keep;
        }
        if let Some(m) = &mut self.menu {
            match m.handle_key(ev, screen) {
                MenuOutcome::Consumed => {}
                MenuOutcome::Closed => self.menu = None,
                MenuOutcome::Chosen(act) => self.menu_chose(act),
            }
            return Outcome::Keep;
        }
        self.note = None;
        let r = self.rects(screen);
        let side = !crate::widgets::is_empty(r.side);
        if self.focus == Focus::Places && !side {
            self.focus = Focus::List;
        }
        // Keys that work wherever the focus is.
        let primary = ev.mods.primary();
        match &ev.key {
            Key::Named(Named::Escape | Named::F(10)) => return Outcome::Cancel,
            Key::Char('d') if primary => {
                self.open_menu(screen);
                return Outcome::Keep;
            }
            Key::Named(Named::F(2)) => return self.accept(),
            Key::Named(Named::Enter) if primary => return self.accept(),
            Key::Named(Named::F(3)) => return self.add_folder(),
            Key::Named(Named::Enter) if ev.mods.shift && self.opts.add_folder => return self.add_folder(),
            Key::Named(Named::Tab) => {
                self.cycle_focus(ev.mods.shift, side);
                return Outcome::Keep;
            }
            _ => {}
        }
        if ev.typed() == Some("~") && !self.searching(now) {
            self.home();
            return Outcome::Keep;
        }
        match self.focus {
            Focus::Buttons => match self.buttons.handle_key(ev) {
                RowOutcome::Pressed(0) => self.accept(),
                RowOutcome::Pressed(_) => Outcome::Cancel,
                _ => Outcome::Keep,
            },
            Focus::Places => {
                self.side_key(ev, r);
                Outcome::Keep
            }
            Focus::List => {
                let out = self.list_key(ev, r, now);
                self.top = self.top_for(self.top, self.cursor, r.list.rows as usize);
                out
            }
        }
    }

    fn side_key(&mut self, ev: &KeyEvent, r: Rects) {
        let rows = self.side_rows();
        let open = |i: usize| matches!(rows.get(i), Some(Side::Folder { open: true, .. }));
        match &ev.key {
            Key::Named(Named::Up) => self.side_step(-1),
            Key::Named(Named::Down) => self.side_step(1),
            Key::Named(Named::Home | Named::PageUp) => {
                self.side_cursor = 0;
                self.clamp_side();
            }
            Key::Named(Named::End | Named::PageDown) => {
                self.side_cursor = rows.len();
                self.side_step(-1);
            }
            Key::Named(Named::Left) if open(self.side_cursor) => self.side_activate(self.side_cursor),
            Key::Named(Named::Right) if open(self.side_cursor) => {}
            Key::Named(Named::Enter | Named::Right) | Key::Char(' ') => self.side_activate(self.side_cursor),
            _ => {
                if let Some(t) = ev.typed() {
                    let mut chars = t.chars();
                    if let (Some(c), None) = (chars.next(), chars.next()) {
                        self.side_hotkey(c.to_lowercase().next().unwrap_or(c));
                    }
                }
            }
        }
        let h = r.side.rows.max(1) as usize;
        if self.side_cursor < self.side_top {
            self.side_top = self.side_cursor;
        } else if self.side_cursor >= self.side_top + h {
            self.side_top = self.side_cursor + 1 - h;
        }
    }

    fn list_key(&mut self, ev: &KeyEvent, r: Rects, now: Instant) -> Outcome {
        let page = r.list.rows.max(2) as isize - 1;
        let multi = self.opts.multi_pick();
        match &ev.key {
            Key::Named(Named::Up) => self.move_cursor(-1),
            Key::Named(Named::Down) => self.move_cursor(1),
            Key::Named(Named::PageUp) => self.move_cursor(-page),
            Key::Named(Named::PageDown) => self.move_cursor(page),
            Key::Named(Named::Home) => self.cursor = 0,
            Key::Named(Named::End) => self.cursor = self.rows().saturating_sub(1),
            Key::Named(Named::Enter) => return self.activate(),
            Key::Named(Named::Right) => {
                if let Some(e) = self.entry(self.cursor).filter(|e| e.is_dir).cloned() {
                    self.go(Location::Dir(e.path), None);
                }
            }
            Key::Named(Named::Backspace | Named::Left) => self.up(),
            Key::Named(Named::Insert) if multi => self.toggle_mark(),
            _ => match ev.typed() {
                Some(" ") if multi && !self.searching(now) => self.toggle_mark(),
                Some("*") if multi => self.mark_all(),
                Some(t) => self.type_ahead(t, now),
                None => {}
            },
        }
        Outcome::Keep
    }

    pub fn mouse(&mut self, ev: &MouseEvent, screen: CellRect) -> Outcome {
        self.poll();
        if let Some(a) = &mut self.add {
            let out = a.mouse(ev, screen);
            self.add_done(out);
            return Outcome::Keep;
        }
        if let Some(m) = &mut self.menu {
            match m.handle_mouse(ev, screen) {
                MenuOutcome::Consumed => {}
                MenuOutcome::Closed => self.menu = None,
                MenuOutcome::Chosen(act) => self.menu_chose(act),
            }
            return Outcome::Keep;
        }
        let r = self.rects(screen);
        let at = |rect: CellRect| contains(rect, ev.col, ev.row);
        match ev.kind {
            MouseKind::Scroll(dy) if at(r.list) => {
                let max = self.rows().saturating_sub(r.list.rows as usize);
                self.top = (self.top as f32 - dy * 3.0).round().clamp(0.0, max as f32) as usize;
            }
            MouseKind::Scroll(dy) if at(r.side) => {
                let max = self.side_rows().len().saturating_sub(r.side.rows as usize);
                self.side_top = (self.side_top as f32 - dy * 3.0).round().clamp(0.0, max as f32) as usize;
            }
            MouseKind::Down if at(r.side) => {
                self.note = None;
                self.focus = Focus::Places;
                self.side_activate(self.side_top + (ev.row - r.side.row) as usize);
            }
            MouseKind::Down if at(r.list) => {
                self.note = None;
                self.focus = Focus::List;
                let row = self.top + (ev.row - r.list.row) as usize;
                if row < self.rows() {
                    self.cursor = row;
                    if ev.double {
                        let out = self.activate();
                        self.top = self.top_for(self.top, self.cursor, r.list.rows as usize);
                        return out;
                    }
                }
            }
            MouseKind::Down => match self.buttons.hit(r.buttons, ev.col, ev.row) {
                Some(0) => return self.accept(),
                Some(_) => return Outcome::Cancel,
                None if !at(r.outer) => return Outcome::Cancel,
                None => {}
            },
            _ => {}
        }
        Outcome::Keep
    }

    // -- Drawing -------------------------------------------------------------

    pub fn draw(&self, f: &mut Frame, screen: CellRect, ctx: &DrawCtx) {
        let c = &ctx.theme.colors;
        let r = self.rects(screen);
        let subtitle = if self.marked.is_empty() { String::new() } else { format!("{} marked", self.marked.len()) };
        form::cover(f, r.outer, ctx);
        crate::widgets::BoxFrame { title: &self.opts.title, subtitle: &subtitle, active: true }.draw(f, r.outer, ctx);
        if !crate::widgets::is_empty(r.side) {
            self.draw_side(f, r.side, ctx);
            let line = Style::new(c.border, c.surface);
            for row in r.side.row..bottom(r.side) {
                f.put_char(right(r.side) + 1, row, '│', line);
            }
        }
        self.draw_list(f, r, ctx);
        let (text, fg) = match &self.note {
            Some(Note::Done(t)) => (t.clone(), c.success),
            Some(Note::Failed(t)) => (t.clone(), c.error),
            None => (self.hint(), c.muted),
        };
        put_elided(f, r.hint.col, r.hint.row, &text, Style::new(fg, c.surface), r.hint.cols);
        let can_accept = self.location != Location::Drives;
        self.buttons.draw(f, r.buttons, ctx, self.focus == Focus::Buttons && can_accept);
        if let Some(m) = &self.menu {
            m.draw(f, screen, ctx);
        }
        if let Some(a) = &self.add {
            a.draw(f, screen, ctx);
        }
    }

    fn hint(&self) -> String {
        let mut parts: Vec<&str> = vec!["Enter open", "⌫ up", "~ home"];
        if self.opts.multi_pick() {
            parts.extend(["Space mark", "* all"]);
        }
        if self.opts.add_folder {
            parts.push("F3 add folder");
        }
        if self.opts.bookmarks.is_some() {
            parts.push("⌘D bookmarks");
        }
        let accept = format!("F2 {}", self.opts.accept.to_lowercase());
        parts.push(&accept);
        parts.push("Esc cancel");
        primary_label(&parts.join(" · "))
    }

    fn draw_side(&self, f: &mut Frame, area: CellRect, ctx: &DrawCtx) {
        let c = &ctx.theme.colors;
        let rows = self.side_rows();
        let focused = self.focus == Focus::Places;
        let top = self.side_top.min(rows.len().saturating_sub(area.rows as usize));
        for k in 0..area.rows {
            let i = top + k as usize;
            let Some(row) = rows.get(i) else { break };
            let line = CellRect::new(area.col, area.row + k, area.cols, 1);
            let here = match row {
                Side::Place(p) => self.location.same(&Location::Dir(p.path.clone())),
                Side::Bookmark { path, .. } => self.location.same(&Location::Dir(path.clone())),
                _ => false,
            };
            let on = focused && i == self.side_cursor && row.selectable();
            let base = if on {
                Style::new(c.cursor_fg, c.cursor_bg)
            } else if here {
                Style::new(c.accent, c.surface).with(Flags::BOLD)
            } else {
                Style::new(c.foreground, c.surface)
            };
            match row {
                Side::Heading(t) => form::heading(f, line, t, ctx),
                Side::Note(t) => {
                    put_elided(f, line.col + 1, line.row, t, Style::new(c.muted, c.surface), line.cols.saturating_sub(1));
                }
                Side::Blank => {}
                Side::Place(p) => {
                    fill(f, line, base.bg);
                    put_elided(f, line.col + 1, line.row, &p.title, base, line.cols.saturating_sub(2));
                }
                Side::Bookmark { title, key, depth, .. } | Side::Folder { title, key, depth, .. } => {
                    fill(f, line, base.bg);
                    let x = line.col + 1 + depth * 2;
                    let mark = match row {
                        Side::Folder { open: true, .. } => "▾ ",
                        Side::Folder { .. } => "▸ ",
                        _ => "",
                    };
                    let keyw = 2;
                    let room = right(line).saturating_sub(x + keyw);
                    let x = put(f, x, line.row, mark, base, room);
                    put_elided(f, x, line.row, title, base, right(line).saturating_sub(x + keyw));
                    if let Some(k) = key {
                        let ks = if on { base.with(Flags::BOLD) } else { Style::new(c.accent, c.surface).with(Flags::BOLD) };
                        let k: String = k.to_uppercase().collect();
                        put(f, right(line).saturating_sub(1 + width(&k)), line.row, &k, ks, 1);
                    }
                }
            }
        }
        Scrollbar { total: rows.len(), visible: area.rows as usize, top }.draw(f, area, ctx);
    }

    fn draw_list(&self, f: &mut Frame, r: Rects, ctx: &DrawCtx) {
        let c = &ctx.theme.colors;
        let title = self.location.title();
        let header = elide_left(&title, r.header.cols as usize);
        put(f, r.header.col, r.header.row, &header, Style::new(c.accent, c.surface).with(Flags::BOLD), r.header.cols);
        let list = r.list;
        let Some(entries) = &self.entries else {
            put_elided(f, list.col + 1, list.row, "Listing…", Style::new(c.muted, c.surface), list.cols.saturating_sub(1));
            return;
        };
        let multi = self.opts.multi_pick();
        let focused = self.focus == Focus::List;
        let rows = self.rows();
        let top = self.top_for(self.top, self.cursor, list.rows as usize);
        for k in 0..list.rows {
            let i = top + k as usize;
            if i >= rows {
                break;
            }
            let line = CellRect::new(list.col, list.row + k, list.cols, 1);
            let marked = self.marked.contains(&i);
            let (name, fg) = match self.entry(i) {
                None => ("..".to_string(), c.dir),
                Some(e) if self.location == Location::Drives => (e.name.clone(), c.dir),
                Some(e) if e.is_dir => (format!("{}{}", e.name, std::path::MAIN_SEPARATOR), c.dir),
                Some(e) => (e.name.clone(), c.foreground),
            };
            let bg = match (i == self.cursor, focused) {
                (true, true) => c.cursor_bg,
                (true, false) => c.cursor_inactive_bg,
                _ => c.surface,
            };
            let fg = if i == self.cursor && focused {
                c.cursor_fg
            } else if marked {
                c.marked
            } else {
                fg
            };
            fill(f, line, bg);
            let mut x = line.col + 1;
            if multi {
                let mark_fg = if i == self.cursor && focused { c.cursor_fg } else { c.marked };
                put(f, line.col, line.row, if marked { "✓" } else { " " }, Style::new(mark_fg, bg), 1);
                x = line.col + 2;
            }
            put_elided(f, x, line.row, &name, Style::new(fg, bg), right(line).saturating_sub(x + 1));
        }
        let msg = |f: &mut Frame, text: &str, fg| {
            let row = list.row + u16::from(self.has_parent());
            if row < bottom(list) {
                put_elided(f, list.col + 1, row, text, Style::new(fg, c.surface), list.cols.saturating_sub(2));
            }
        };
        if let Some(e) = &self.error {
            msg(f, &format!("Can't open this folder: {e}"), c.error);
        } else if entries.is_empty() {
            let text = match self.opts.pick {
                Pick::Folder => "No folders in here — use this one or go up",
                Pick::Files { .. } if self.opts.filter.is_some() => "No matching files in here — go up or pick a place",
                Pick::Files { .. } => "Nothing in here — go up or pick a place",
            };
            msg(f, text, c.muted);
        }
        Scrollbar { total: rows, visible: list.rows as usize, top }.draw(f, list, ctx);
    }
}

#[cfg(test)]
mod tests;
