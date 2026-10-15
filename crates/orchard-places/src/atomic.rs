//! Writing a shared file whole or not at all.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Replace `path`'s contents without a reader ever seeing a half-written
/// file: other apps (and other instances of the same app) re-read the
/// file before each change, and a torn file would read as "nothing saved"
/// and be written back that way. Writes go through `<name>.tmp` next to it
/// (its folder created first), and through a symlinked file (dotfile
/// setups) to its target. If staging is refused (a read-only directory; on
/// Windows, a file another process has open), the file is written in place
/// as Orchard Commander's Python version does.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let target = match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => fs::canonicalize(path)?,
        _ => path.to_path_buf(),
    };
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut tmp = target.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let staged = fs::write(&tmp, contents).and_then(|()| fs::rename(&tmp, &target));
    if staged.is_err() {
        let _ = fs::remove_file(&tmp);
        fs::write(&target, contents)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_through_a_temporary_file_into_a_new_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new").join("x.json");
        write_atomic(&path, b"{}").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
        // A temporary file left by a crash is overwritten, not a problem.
        fs::write(dir.path().join("new").join("x.json.tmp"), "half").unwrap();
        write_atomic(&path, b"[]").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "[]");
        let names: Vec<_> = fs::read_dir(dir.path().join("new")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["x.json"]);
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_a_symlink_to_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.json");
        let link = dir.path().join("link.json");
        fs::write(&real, "old").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomic(&link, b"new").unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "still a link");
        assert_eq!(fs::read_to_string(&real).unwrap(), "new");
    }
}
