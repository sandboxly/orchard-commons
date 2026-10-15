//! The picker's logic on temporary folders and bookmark files (never the
//! real config folder: every picker here names its bookmarks file).

use std::fs;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use orchard_places::{Folder, Item};

use super::*;
use crate::input::Mods;

fn screen() -> CellRect {
    CellRect::new(0, 0, 120, 40)
}

/// A picker whose waker ticks a channel, for [`wait`].
fn open(opts: Options) -> (Picker, Receiver<()>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let waker: Waker = Arc::new(move || {
        let _ = tx.lock().unwrap().send(());
    });
    (Picker::new(opts, waker), rx)
}

/// Apply results until `done` holds (5 s at most).
fn wait(p: &mut Picker, rx: &Receiver<()>, done: impl Fn(&Picker) -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        p.poll();
        if done(p) {
            return;
        }
        let left = until.saturating_duration_since(Instant::now());
        assert!(rx.recv_timeout(left).is_ok(), "timed out waiting for the picker");
    }
}

fn listed(p: &mut Picker, rx: &Receiver<()>) {
    wait(p, rx, |p| p.entries().is_some());
}

fn names(p: &Picker) -> Vec<String> {
    p.entries().unwrap().iter().map(|e| e.name.clone()).collect()
}

fn key(p: &mut Picker, k: KeyEvent) -> Outcome {
    p.key(&k, screen())
}

fn named(n: Named) -> KeyEvent {
    KeyEvent::named(n)
}

fn primary(k: KeyEvent) -> KeyEvent {
    k.with(Mods { ctrl: true, logo: true, ..Mods::default() })
}

/// A folder with "A/", "b/", "Music/" (with tracks), "file.txt".
fn tree() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for dir in ["b", "A", "Music"] {
        fs::create_dir(d.path().join(dir)).unwrap();
    }
    fs::write(d.path().join("file.txt"), b"").unwrap();
    for t in ["10 ten.mp3", "2 two.mp3", "cover.jpg"] {
        fs::write(d.path().join("Music").join(t), b"").unwrap();
    }
    d
}

fn folder_opts(start: &Path) -> Options {
    Options::folder(start).no_places().bookmarks(None)
}

#[test]
fn picks_a_folder_going_in_and_back_up() {
    let d = tree();
    let (mut p, rx) = open(folder_opts(d.path()));
    listed(&mut p, &rx);
    assert_eq!(names(&p), ["A", "b", "Music"], "folders only, in natural order");
    key(&mut p, KeyEvent::char('b'));
    assert_eq!(p.cursor(), 2, "\"..\" is row 0");
    key(&mut p, named(Named::Enter));
    assert_eq!(p.location(), &Location::Dir(d.path().join("b")));
    listed(&mut p, &rx);
    key(&mut p, named(Named::Backspace));
    listed(&mut p, &rx);
    assert_eq!(p.cursor(), 2, "back on the folder we came from");
    assert_eq!(key(&mut p, named(Named::F(2))), Outcome::Folder(d.path().to_path_buf()));
    assert_eq!(key(&mut p, primary(named(Named::Enter))), Outcome::Folder(d.path().to_path_buf()));
    // Enter on ".." goes up.
    key(&mut p, named(Named::Home));
    key(&mut p, named(Named::Enter));
    assert_eq!(p.location(), &Location::Dir(d.path().parent().unwrap().to_path_buf()));
    assert_eq!(key(&mut p, named(Named::Escape)), Outcome::Cancel);
}

#[test]
fn a_missing_start_opens_the_nearest_existing_folder() {
    let d = tree();
    let (mut p, rx) = open(folder_opts(&d.path().join("A/not/there")));
    listed(&mut p, &rx);
    assert_eq!(p.location(), &Location::Dir(d.path().join("A")));
    assert_eq!(p.error(), None);
}

#[test]
fn a_file_as_the_start_opens_its_folder_on_it() {
    let d = tree();
    let (mut p, rx) = open(Options::files(d.path().join("file.txt")).no_places().bookmarks(None));
    listed(&mut p, &rx);
    assert_eq!(p.location(), &Location::Dir(d.path().to_path_buf()));
    assert_eq!(p.entry(p.cursor()).unwrap().name, "file.txt");
}

#[cfg(unix)]
#[test]
fn a_folder_that_cant_be_listed_says_why() {
    use std::os::unix::fs::PermissionsExt;
    let d = tree();
    let locked = d.path().join("A");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_dir(&locked).is_ok() {
        return; // running as root: nothing is locked
    }
    let (mut p, rx) = open(folder_opts(&locked));
    listed(&mut p, &rx);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(p.location(), &Location::Dir(locked.clone()), "it's there: no going up");
    assert_eq!(p.error(), Some("permission denied"));
    assert!(p.entries().unwrap().is_empty());
}

#[test]
fn stale_listings_are_dropped() {
    let d = tree();
    let (mut p, rx) = open(folder_opts(d.path()));
    listed(&mut p, &rx);
    let old = p.generation;
    p.go(Location::Dir(d.path().join("A")), None);
    p.listed(old, Location::Dir(d.path().to_path_buf()), Ok(vec![Entry { name: "late".into(), path: d.path().join("late"), is_dir: true }]));
    assert!(p.entries().is_none(), "still listing A");
    assert_eq!(p.location(), &Location::Dir(d.path().join("A")));
    listed(&mut p, &rx);
    assert!(names(&p).is_empty());
}

#[test]
fn picks_one_file_through_a_filter() {
    let d = tree();
    let opts = Options::files(d.path().join("Music")).extensions(&["MP3"]).no_places().bookmarks(None);
    let (mut p, rx) = open(opts);
    listed(&mut p, &rx);
    assert_eq!(names(&p), ["2 two.mp3", "10 ten.mp3"]);
    key(&mut p, named(Named::Down));
    assert_eq!(key(&mut p, KeyEvent::char(' ')), Outcome::Keep, "no marking with one file");
    assert_eq!(key(&mut p, named(Named::Enter)), Outcome::Files(vec![d.path().join("Music/2 two.mp3")]));
    // The button on ".." doesn't go up; Enter does.
    key(&mut p, named(Named::Home));
    assert_eq!(key(&mut p, named(Named::F(2))), Outcome::Keep);
    assert_eq!(p.location(), &Location::Dir(d.path().join("Music")));
}

#[test]
fn marks_several_files() {
    let d = tree();
    let opts = Options::files(d.path().join("Music")).multi().no_places().bookmarks(None);
    assert_eq!(opts.accept, "Open files");
    let (mut p, rx) = open(opts);
    listed(&mut p, &rx);
    assert_eq!(names(&p), ["2 two.mp3", "10 ten.mp3", "cover.jpg"]);
    key(&mut p, named(Named::Down));
    key(&mut p, KeyEvent::char(' '));
    assert_eq!(p.cursor(), 2, "marking moves down");
    key(&mut p, named(Named::Down));
    key(&mut p, named(Named::Insert));
    let music = d.path().join("Music");
    assert_eq!(p.marked(), [music.join("2 two.mp3"), music.join("cover.jpg")]);
    // Enter chooses the marked ones, wherever the cursor is.
    key(&mut p, named(Named::Up));
    assert_eq!(key(&mut p, named(Named::Enter)), Outcome::Files(p.marked()));
    key(&mut p, KeyEvent::char('*'));
    assert_eq!(p.marked().len(), 3, "every file");
    key(&mut p, KeyEvent::char('*'));
    assert!(p.marked().is_empty(), "again: none");
    // ".." can't be marked; going elsewhere clears the marks.
    key(&mut p, named(Named::Home));
    key(&mut p, KeyEvent::char(' '));
    assert!(p.marked().is_empty());
    key(&mut p, KeyEvent::char(' '));
    assert_eq!(p.marked().len(), 1);
    key(&mut p, named(Named::Backspace));
    assert!(p.marked().is_empty());
}

#[test]
fn folders_can_be_marked_and_added_when_asked() {
    let d = tree();
    let opts = Options::files(d.path()).multi().mark_folders().add_folder().no_places().bookmarks(None);
    let (mut p, rx) = open(opts);
    listed(&mut p, &rx);
    assert_eq!(names(&p), ["A", "b", "Music", "file.txt"]);
    key(&mut p, named(Named::Down));
    key(&mut p, KeyEvent::char(' '));
    assert_eq!(p.marked(), [d.path().join("A")]);
    // F3 / Shift+Enter: the folder under the cursor, else the one shown.
    assert_eq!(key(&mut p, named(Named::F(3))), Outcome::AddFolder(d.path().join("b")));
    key(&mut p, named(Named::End));
    let shift = Mods { shift: true, ..Mods::default() };
    assert_eq!(key(&mut p, named(Named::Enter).with(shift)), Outcome::AddFolder(d.path().to_path_buf()));
    // Without the option, F3 does nothing.
    let (mut q, rx) = open(Options::files(d.path()).no_places().bookmarks(None));
    listed(&mut q, &rx);
    assert_eq!(key(&mut q, named(Named::F(3))), Outcome::Keep);
}

#[test]
fn type_ahead_extends_within_a_moment_and_space_joins_it() {
    let d = tempfile::tempdir().unwrap();
    for f in ["ab", "abba", "b c", "bz"] {
        fs::write(d.path().join(f), b"").unwrap();
    }
    let opts = Options::files(d.path()).multi().no_places().bookmarks(None);
    let (mut p, rx) = open(opts);
    listed(&mut p, &rx);
    let t0 = Instant::now();
    let s = screen();
    p.key_at(&KeyEvent::char('a'), s, t0);
    p.key_at(&KeyEvent::char('b'), s, t0 + Duration::from_millis(300));
    p.key_at(&KeyEvent::char('b'), s, t0 + Duration::from_millis(600));
    assert_eq!(p.entry(p.cursor()).unwrap().name, "abba");
    // Later, a new search; a space right after a letter is part of it.
    let later = t0 + Duration::from_secs(2);
    p.key_at(&KeyEvent::char('b'), s, later);
    p.key_at(&KeyEvent::char(' '), s, later + Duration::from_millis(100));
    assert_eq!(p.entry(p.cursor()).unwrap().name, "b c");
    assert!(p.marked().is_empty(), "the space searched, didn't mark");
    // "~" goes home (unless searching).
    p.key_at(&KeyEvent::char('~'), s, later + Duration::from_secs(5));
    assert_eq!(p.location().dir(), dirs::home_dir().as_deref());
}

#[test]
fn drives_cant_be_chosen_or_marked() {
    let d = tree();
    let opts = Options::files(d.path()).multi().mark_folders().add_folder().no_places().bookmarks(None);
    let (mut p, rx) = open(opts);
    listed(&mut p, &rx);
    // As if gone up from D:\ on Windows.
    p.go(Location::Drives, Some(Location::Dir(PathBuf::from("d:"))));
    let drive = |l: &str| Entry { name: format!("{l}:"), path: PathBuf::from(format!("{l}:\\")), is_dir: true };
    p.listed(p.generation, Location::Drives, Ok(vec![drive("C"), drive("D")]));
    assert_eq!(p.cursor(), 1, "on the drive we came from");
    assert!(!p.has_parent());
    key(&mut p, KeyEvent::char(' '));
    assert!(p.marked().is_empty());
    assert_eq!(key(&mut p, named(Named::F(3))), Outcome::Keep);
    assert_eq!(key(&mut p, named(Named::F(2))), Outcome::Keep);
}

#[test]
fn tab_moves_between_places_list_and_buttons() {
    let d = tree();
    let b = d.path().join("bookmarks.json");
    let (mut p, rx) = open(Options::folder(d.path()).no_places().bookmarks(Some(b)));
    listed(&mut p, &rx);
    assert_eq!(p.focus(), Focus::List);
    key(&mut p, named(Named::Tab));
    assert_eq!(p.focus(), Focus::Buttons);
    assert_eq!(key(&mut p, named(Named::Enter)), Outcome::Folder(d.path().to_path_buf()));
    key(&mut p, named(Named::Right));
    assert_eq!(key(&mut p, KeyEvent::char(' ')), Outcome::Cancel, "the second button");
    key(&mut p, named(Named::Tab));
    assert_eq!(p.focus(), Focus::Places);
    key(&mut p, named(Named::Tab).with(Mods { shift: true, ..Mods::default() }));
    assert_eq!(p.focus(), Focus::Buttons);
    // Too narrow for the column: it is skipped.
    let narrow = CellRect::new(0, 0, 50, 30);
    p.key(&named(Named::Tab), narrow);
    assert_eq!(p.focus(), Focus::List);
    assert!(crate::widgets::is_empty(p.rects(narrow).side));
}

fn bookmarks_file(dir: &Path, items: &[Item]) -> PathBuf {
    let path = dir.join("config").join("bookmarks.json");
    store::save(&path, items).unwrap();
    path
}

fn sample_bookmarks(d: &Path) -> Vec<Item> {
    vec![
        Bookmark::local("&Music", &d.join("Music")).into(),
        Bookmark::new("&Web", Spec::server("sftp://root@web", "/var/www"), None).into(),
        Folder::new("&Work", vec![Bookmark::local("&A folder", &d.join("A")).into(), Bookmark::local("&B folder", &d.join("b")).into()]).into(),
    ]
}

#[test]
fn the_column_lists_local_bookmarks_and_goes_there() {
    let d = tree();
    let file = bookmarks_file(d.path(), &sample_bookmarks(d.path()));
    let (mut p, rx) = open(Options::folder(d.path()).no_places().bookmarks(Some(file)));
    wait(&mut p, &rx, |p| !p.bookmarks().is_empty() && p.entries().is_some());
    let rows = p.side_rows();
    assert_eq!(rows[0], Side::Heading("Bookmarks"));
    assert!(matches!(&rows[1], Side::Bookmark { title, key: Some('m'), .. } if title == "Music"));
    assert!(matches!(&rows[2], Side::Folder { title, open: false, .. } if title == "Work"));
    assert_eq!(rows.len(), 3, "the server bookmark is left out");
    // Into the column; "w" opens the folder (the only "W"), "a" goes to its bookmark.
    key(&mut p, named(Named::Tab));
    key(&mut p, named(Named::Tab));
    assert_eq!(p.focus(), Focus::Places);
    key(&mut p, KeyEvent::char('w'));
    assert_eq!(p.side_rows().len(), 5, "open");
    key(&mut p, KeyEvent::char('a'));
    assert_eq!(p.location(), &Location::Dir(d.path().join("A")));
    assert_eq!(p.focus(), Focus::List, "on to the list");
    // A click on "Music" goes there.
    let r = p.rects(screen());
    let click = MouseEvent { kind: MouseKind::Down, col: r.side.col + 2, row: r.side.row + 1, x: 0.0, y: 0.0, double: false };
    p.mouse(&click, screen());
    assert_eq!(p.location(), &Location::Dir(d.path().join("Music")));
}

#[test]
fn the_bookmark_menu_goes_to_a_bookmark() {
    let d = tree();
    let file = bookmarks_file(d.path(), &sample_bookmarks(d.path()));
    let (mut p, rx) = open(Options::folder(d.path()).no_places().bookmarks(Some(file)));
    wait(&mut p, &rx, |p| p.bookmarks.is_some());
    key(&mut p, primary(KeyEvent::char('d')));
    let labels = p.menu.as_ref().unwrap().labels();
    assert_eq!(labels, ["M  Music", "W  Work", "+  Add current folder…"]);
    key(&mut p, KeyEvent::char('w'));
    key(&mut p, KeyEvent::char('b'));
    assert!(!p.menu_open());
    assert_eq!(p.location(), &Location::Dir(d.path().join("b")));
    // A bookmarked folder isn't offered again.
    key(&mut p, primary(KeyEvent::char('d')));
    key(&mut p, KeyEvent::char('m'));
    listed(&mut p, &rx);
    key(&mut p, primary(KeyEvent::char('d')));
    assert_eq!(p.menu.as_ref().unwrap().labels(), ["M  Music", "W  Work"]);
    key(&mut p, named(Named::Escape));
    assert!(!p.menu_open(), "Esc closes the menu, not the picker");
}

#[test]
fn adding_a_bookmark_writes_the_shared_file() {
    let d = tree();
    let work = Folder::new("&Work", vec![]);
    let work_id = work.id.clone();
    let file = bookmarks_file(d.path(), &[work.into()]);
    let (mut p, rx) = open(Options::folder(d.path().join("Music")).no_places().bookmarks(Some(file.clone())));
    wait(&mut p, &rx, |p| p.bookmarks.is_some() && p.entries().is_some());
    // Meanwhile Orchard Commander adds one; it must survive.
    store::add_bookmark(&file, Bookmark::local("&Elsewhere", &d.path().join("A")), None).unwrap();
    key(&mut p, primary(KeyEvent::char('d')));
    key(&mut p, KeyEvent::char('+'));
    assert!(p.adding());
    assert_eq!(p.add.as_ref().unwrap().title.text, "Music");
    p.add.as_mut().unwrap().title.set_text("&Tunes");
    key(&mut p, named(Named::Tab));
    key(&mut p, named(Named::Right)); // Put in: Work
    key(&mut p, named(Named::Tab));
    assert_eq!(key(&mut p, named(Named::Enter)), Outcome::Keep);
    assert!(!p.adding());
    wait(&mut p, &rx, |p| p.note.is_some() && p.local.len() == 2);
    assert_eq!(p.note, Some(Note::Done("Bookmark “Tunes” added".into())));
    let items = store::load(&file);
    let all: Vec<_> = store::bookmarks(&items).into_iter().map(|(t, b)| (t, b.title.clone())).collect();
    assert_eq!(all, [(vec!["Work".to_string()], "&Tunes".to_string()), (vec![], "&Elsewhere".to_string())]);
    let Some(Item::Folder(f)) = store::find(&items, &work_id) else { panic!() };
    assert_eq!(f.items.len(), 1);
    // The next key clears the note.
    key(&mut p, named(Named::Down));
    assert_eq!(p.note, None);
}

#[test]
fn a_damaged_bookmarks_file_isnt_overwritten() {
    let d = tree();
    let file = d.path().join("bookmarks.json");
    fs::write(&file, "{oops").unwrap();
    let (mut p, rx) = open(Options::folder(d.path()).no_places().bookmarks(Some(file.clone())));
    wait(&mut p, &rx, |p| p.bookmarks.is_some());
    key(&mut p, primary(KeyEvent::char('d')));
    key(&mut p, KeyEvent::char('+'));
    key(&mut p, named(Named::Enter));
    wait(&mut p, &rx, |p| p.note.is_some());
    assert!(matches!(&p.note, Some(Note::Failed(t)) if t.starts_with("Can't add the bookmark: ") && t.contains("damaged")));
    assert_eq!(fs::read_to_string(&file).unwrap(), "{oops");
}

#[test]
fn places_load_in_the_background() {
    let d = tree();
    let (mut p, rx) = open(Options::folder(d.path()).bookmarks(None));
    assert!(matches!(p.side_rows()[1], Side::Note(_)), "loading");
    wait(&mut p, &rx, |p| p.places().is_some());
    let rows = p.side_rows();
    assert_eq!(rows[0], Side::Heading("Places"));
    if dirs::home_dir().is_some() {
        assert!(matches!(&rows[1], Side::Place(pl) if pl.title == "Home"));
    }
}

#[test]
fn draws_places_listing_and_buttons() {
    let d = tree();
    let file = bookmarks_file(d.path(), &sample_bookmarks(d.path()));
    let (mut p, rx) = open(Options::folder(d.path()).bookmarks(Some(file)));
    wait(&mut p, &rx, |p| p.places().is_some() && p.bookmarks.is_some() && p.entries().is_some());
    let theme = orchard_theme::Theme::default_dark();
    let ctx = DrawCtx { theme: &theme, cell_px: (8.0, 16.0) };
    let s = screen();
    let text = |p: &Picker| {
        let mut f = Frame::new(s.cols, s.rows, theme.colors.background);
        p.draw(&mut f, s, &ctx);
        (0..s.rows).map(|r| f.row_text(r)).collect::<Vec<_>>().join("\n")
    };
    let all = text(&p);
    let sep = std::path::MAIN_SEPARATOR;
    for want in [
        "Choose a folder".to_string(),
        "Places".into(),
        "Bookmarks".into(),
        "Music".into(),
        "▸ Work".into(),
        format!("A{sep}"),
        format!("Music{sep}"),
        "..".into(),
        "Use this folder".into(),
        "Cancel".into(),
        primary_label("⌘D bookmarks"),
    ] {
        assert!(all.contains(&want), "{want} missing:\n{all}");
    }
    key(&mut p, primary(KeyEvent::char('d')));
    assert!(text(&p).contains("+  Add current folder…"));
    key(&mut p, KeyEvent::char('+'));
    assert!(text(&p).contains("Add bookmark"));
    // Narrow: no column, the listing still shows.
    key(&mut p, named(Named::Escape));
    let narrow = CellRect::new(0, 0, 50, 20);
    let mut f = Frame::new(50, 20, theme.colors.background);
    p.draw(&mut f, narrow, &ctx);
    let all: String = (0..20).map(|r| f.row_text(r)).collect::<Vec<_>>().join("\n");
    assert!(!all.contains("Places") && all.contains(&format!("Music{sep}")), "{all}");
}
