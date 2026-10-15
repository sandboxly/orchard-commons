//! The places a file picker offers besides bookmarks: the home folder, the
//! user's standard folders, drives (Windows) and mounted volumes (macOS,
//! Linux).
//!
//! [`places`] blocks: it checks which folders exist and lists the mount
//! folders, and a network mount can take a while to answer. Call it off the
//! UI thread.

use std::path::{Path, PathBuf};

/// What a place is (for an icon, or to group them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlaceKind {
    Home,
    Desktop,
    Documents,
    Downloads,
    Music,
    Pictures,
    Videos,
    /// The file system's root ("/").
    Root,
    /// A drive letter (Windows).
    Drive,
    /// A mounted volume: a USB stick, a network share, a disk image.
    Volume,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub title: String,
    pub path: PathBuf,
    pub kind: PlaceKind,
}

impl Place {
    pub fn new(title: impl Into<String>, path: impl Into<PathBuf>, kind: PlaceKind) -> Self {
        Place { title: title.into(), path: path.into(), kind }
    }
}

/// Home, the standard folders that exist (Desktop, Documents, Downloads,
/// Music, Pictures, Videos), then the drives on Windows, or "/" and the
/// mounted volumes elsewhere (blocking, see the module docs).
pub fn places() -> Vec<Place> {
    let mut out = Vec::new();
    let home = dirs::home_dir();
    if let Some(h) = &home {
        out.push(Place::new("Home", h, PlaceKind::Home));
    }
    let standard = [
        ("Desktop", dirs::desktop_dir(), PlaceKind::Desktop),
        ("Documents", dirs::document_dir(), PlaceKind::Documents),
        ("Downloads", dirs::download_dir(), PlaceKind::Downloads),
        ("Music", dirs::audio_dir(), PlaceKind::Music),
        ("Pictures", dirs::picture_dir(), PlaceKind::Pictures),
        ("Videos", dirs::video_dir(), PlaceKind::Videos),
    ];
    for (title, dir, kind) in standard {
        // Unset XDG folders can point at home itself.
        let Some(dir) = dir.filter(|d| Some(d) != home.as_ref() && d.is_dir()) else { continue };
        if !out.iter().any(|p| p.path == dir) {
            out.push(Place::new(title, dir, kind));
        }
    }
    #[cfg(windows)]
    out.extend(drives());
    #[cfg(not(windows))]
    {
        out.push(Place::new("/", "/", PlaceKind::Root));
        out.extend(mount_roots().iter().flat_map(|root| mounts_in(root)));
    }
    out
}

/// Where volumes get mounted: `/Volumes` on macOS; `/media/$USER`,
/// `/run/media/$USER` and `/mnt` on Linux.
#[cfg_attr(windows, allow(dead_code))]
fn mount_roots() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        return vec![PathBuf::from("/Volumes")];
    }
    let user = std::env::var_os("USER").unwrap_or_default();
    if user.is_empty() {
        return vec![PathBuf::from("/mnt")];
    }
    vec![Path::new("/media").join(&user), Path::new("/run/media").join(&user), PathBuf::from("/mnt")]
}

/// The folders in a mount root (e.g. `/Volumes`), sorted by path.
pub fn mounts_in(root: &Path) -> Vec<Place> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut children: Vec<PathBuf> = rd.filter_map(|de| de.ok().map(|de| de.path())).collect();
    children.sort();
    children
        .into_iter()
        .filter(|p| p.is_dir())
        .map(|p| Place::new(p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), p, PlaceKind::Volume))
        .collect()
}

/// The drives, "C:" at `C:\` and so on (Windows; none elsewhere). The
/// drives aren't touched: a look at an empty card reader can hang.
pub fn drives() -> Vec<Place> {
    #[cfg(windows)]
    {
        // SAFETY: no arguments; returns a bitmask of the drive letters in use.
        let mask = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
        drive_letters(mask).into_iter().map(|l| Place::new(format!("{l}:"), format!("{l}:\\"), PlaceKind::Drive)).collect()
    }
    #[cfg(not(windows))]
    Vec::new()
}

/// The letters in a drive bitmask: bit 0 is A, bit 1 B, ….
pub fn drive_letters(mask: u32) -> Vec<char> {
    (0..26u8).filter(|i| mask & (1 << i) != 0).map(|i| (b'A' + i) as char).collect()
}

/// "C" for a drive root (`C:\`, `C:`, `C:/`, any case); `None` for
/// anything else. Pure text: works the same on every platform.
pub fn drive_root_letter(path: &Path) -> Option<char> {
    let s = path.to_str()?;
    let mut chars = s.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    (chars.next() == Some(':') && matches!(chars.as_str(), "" | "\\" | "/")).then(|| letter.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_comes_first_and_standard_folders_exist() {
        let all = places();
        if dirs::home_dir().is_some() {
            assert_eq!(all[0].kind, PlaceKind::Home);
        }
        for p in all.iter().filter(|p| !matches!(p.kind, PlaceKind::Drive | PlaceKind::Volume | PlaceKind::Root)) {
            assert!(p.path.is_dir(), "{p:?}");
        }
        #[cfg(not(windows))]
        assert!(all.iter().any(|p| p.kind == PlaceKind::Root && p.path == Path::new("/")));
    }

    #[test]
    fn mount_roots_list_sorted_subdirectories_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Zeta")).unwrap();
        std::fs::create_dir(dir.path().join("Alpha")).unwrap();
        std::fs::write(dir.path().join("file"), b"").unwrap();
        let found = mounts_in(dir.path());
        let names: Vec<&str> = found.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(names, ["Alpha", "Zeta"]);
        assert_eq!(found[0].kind, PlaceKind::Volume);
        assert_eq!(found[0].path, dir.path().join("Alpha"));
        assert!(mounts_in(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn drive_letters_from_the_mask() {
        assert_eq!(drive_letters(0b1101), ['A', 'C', 'D']);
        assert!(drive_letters(0).is_empty());
        for root in ["C:\\", "c:", "D:/"] {
            assert!(drive_root_letter(Path::new(root)).is_some(), "{root}");
        }
        assert_eq!(drive_root_letter(Path::new("c:\\")), Some('C'));
        assert_eq!(drive_root_letter(Path::new("C:\\Music")), None);
        assert_eq!(drive_root_letter(Path::new("/Users")), None);
        assert_eq!(drive_root_letter(Path::new("")), None);
    }
}
