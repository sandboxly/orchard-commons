//! What the picker lists: a folder's entries (blocking, run off the UI
//! thread), where ".." leads, and the order.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use orchard_places::places;

/// Which files a picker shows (folders always show).
pub type Filter = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

/// What the picker shows: a folder, or (Windows) the drives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    Dir(PathBuf),
    Drives,
}

impl Location {
    /// Where ".." leads: the parent folder; on Windows, Drives from a
    /// drive root (or a share's root).
    pub fn parent(&self) -> Option<Location> {
        self.parent_on(cfg!(windows))
    }

    pub(crate) fn parent_on(&self, windows: bool) -> Option<Location> {
        match self {
            Location::Drives => None,
            Location::Dir(d) if windows && places::drive_root_letter(d).is_some() => Some(Location::Drives),
            Location::Dir(d) => match d.parent() {
                Some(p) => Some(Location::Dir(p.to_path_buf())),
                None if windows => Some(Location::Drives),
                None => None,
            },
        }
    }

    /// "Drives", or the folder with the home folder as "~".
    pub fn title(&self) -> String {
        match self {
            Location::Drives => "Drives".into(),
            Location::Dir(d) => home_relative(d),
        }
    }

    pub fn dir(&self) -> Option<&Path> {
        match self {
            Location::Dir(d) => Some(d),
            Location::Drives => None,
        }
    }

    /// The same place (drive roots written "C:", "c:\" or "C:/" are one).
    pub(crate) fn same(&self, other: &Location) -> bool {
        match (self, other) {
            (Location::Dir(a), Location::Dir(b)) => {
                a == b || places::drive_root_letter(a).is_some_and(|l| places::drive_root_letter(b) == Some(l))
            }
            (a, b) => a == b,
        }
    }
}

/// `~/Music` for a folder in the home folder, else the path.
pub fn home_relative(p: &Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(rest) = p.strip_prefix(&home)
    {
        if rest.as_os_str().is_empty() {
            return "~".into();
        }
        return format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display());
    }
    p.display().to_string()
}

/// One row of a listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// What listing a folder needs to know.
#[derive(Clone)]
pub(crate) struct ListSpec {
    pub files: bool,
    pub filter: Option<Filter>,
    pub hidden: bool,
}

/// The entries of `location`, folders first, in natural order (blocking).
pub(crate) fn list(location: &Location, spec: &ListSpec) -> Result<Vec<Entry>, String> {
    let dir = match location {
        Location::Drives => {
            return Ok(places::drives().into_iter().map(|p| Entry { name: p.title, path: p.path, is_dir: true }).collect());
        }
        Location::Dir(d) => d,
    };
    let rd = std::fs::read_dir(dir).map_err(|e| io_reason(&e))?;
    let mut entries: Vec<Entry> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !spec.hidden && name.starts_with('.') {
                return None;
            }
            let path = e.path();
            // Only links need a second look: a linked folder is a folder.
            let is_dir = match e.file_type() {
                Ok(t) if t.is_symlink() => path.is_dir(),
                Ok(t) => t.is_dir(),
                Err(_) => path.is_dir(),
            };
            let shown = is_dir || (spec.files && spec.filter.as_ref().is_none_or(|f| f(&path)));
            shown.then_some(Entry { name, path, is_dir })
        })
        .collect();
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| natural_cmp(&a.name, &b.name)));
    Ok(entries)
}

/// The nearest folder at or above `start` that can be listed, and its
/// entries; the home folder (or the root) if none can (blocking).
pub(crate) fn list_nearest(start: &Path, spec: &ListSpec) -> (Location, Result<Vec<Entry>, String>) {
    let mut dir = start.to_path_buf();
    loop {
        let here = Location::Dir(dir.clone());
        match list(&here, spec) {
            Ok(entries) => return (here, Ok(entries)),
            Err(e) => match dir.parent() {
                Some(p) if !dir.is_dir() => dir = p.to_path_buf(),
                // It's there but can't be listed: say why.
                _ => return (here, Err(e)),
            },
        }
    }
}

/// Why a folder can't be listed, in words ("Can't open this folder: …").
pub fn io_reason(e: &std::io::Error) -> String {
    use std::io::ErrorKind::*;
    match e.kind() {
        NotFound => "it doesn't exist".into(),
        PermissionDenied => "permission denied".into(),
        NotADirectory => "it isn't a folder".into(),
        _ => e.to_string(),
    }
}

/// Case-insensitive, with digit runs compared as numbers ("2 b" < "10 a").
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let (mt, nt) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let o = mt.len().cmp(&nt.len()).then_with(|| mt.cmp(nt));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(p), Some(q)) => {
                let o = p.to_lowercase().cmp(q.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn spec(files: bool) -> ListSpec {
        ListSpec { files, filter: None, hidden: false }
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["10 b.mp3", "2 a.mp3", "1 c.mp3", "B.mp3", "a.mp3"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["1 c.mp3", "2 a.mp3", "10 b.mp3", "a.mp3", "B.mp3"]);
    }

    #[test]
    fn folders_first_files_filtered_hidden_skipped() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("CD2")).unwrap();
        fs::create_dir(d.path().join("cd10")).unwrap();
        fs::create_dir(d.path().join(".git")).unwrap();
        for f in ["10.mp3", "02.MP3", "notes.txt", ".hidden.mp3"] {
            fs::write(d.path().join(f), b"").unwrap();
        }
        let here = Location::Dir(d.path().to_path_buf());
        let names = |s: &ListSpec| list(&here, s).unwrap().into_iter().map(|e| e.name).collect::<Vec<_>>();
        assert_eq!(names(&spec(false)), ["CD2", "cd10"]);
        assert_eq!(names(&spec(true)), ["CD2", "cd10", "02.MP3", "10.mp3", "notes.txt"]);
        let mp3: Filter = Arc::new(|p: &Path| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp3")));
        assert_eq!(names(&ListSpec { files: true, filter: Some(mp3), hidden: false }), ["CD2", "cd10", "02.MP3", "10.mp3"]);
        assert_eq!(names(&ListSpec { files: false, filter: None, hidden: true }), [".git", "CD2", "cd10"]);
        let err = list(&Location::Dir(d.path().join("missing")), &spec(true)).unwrap_err();
        assert_eq!(err, "it doesn't exist");
    }

    #[test]
    fn the_nearest_folder_that_exists() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("a")).unwrap();
        let (at, entries) = list_nearest(&d.path().join("a/not/there"), &spec(false));
        assert_eq!(at, Location::Dir(d.path().join("a")));
        assert!(entries.unwrap().is_empty());
    }

    #[test]
    fn up_from_a_drive_root_leads_to_drives_on_windows() {
        for root in ["C:\\", "c:", "D:/"] {
            assert_eq!(Location::Dir(PathBuf::from(root)).parent_on(true), Some(Location::Drives), "{root}");
        }
        assert_eq!(Location::Drives.parent_on(true), None);
        assert_eq!(Location::Dir(PathBuf::from("/")).parent_on(false), None);
        assert_eq!(Location::Dir(PathBuf::from("/Users/me")).parent_on(false), Some(Location::Dir(PathBuf::from("/Users"))));
        assert!(Location::Dir(PathBuf::from("C:\\")).same(&Location::Dir(PathBuf::from("c:"))));
        assert_eq!(Location::Drives.title(), "Drives");
    }

    #[test]
    fn home_relative_titles() {
        let Some(home) = dirs::home_dir() else { return };
        assert_eq!(home_relative(&home), "~");
        assert_eq!(home_relative(&home.join("Music")), format!("~{}Music", std::path::MAIN_SEPARATOR));
    }
}
