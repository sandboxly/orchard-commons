//! The app's config folder and its JSON settings files. Each app keeps its
//! own `Settings` struct (`#[serde(default)]`, so missing keys take their
//! defaults and older files keep loading); these are the parts they share.
//!
//! This is the app's own folder, not Orchard Commander's (whose color
//! schemes every app shares: see `orchard_theme::dirs`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// `$env` if set (and not empty), or the platform's config folder (the
/// temporary folder without one) joined with `linux_name` on Linux and
/// `name` elsewhere.
pub fn app_config_dir(env: &str, linux_name: &str, name: &str) -> PathBuf {
    if let Some(d) = std::env::var_os(env).filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    let name = if cfg!(target_os = "linux") { linux_name } else { name };
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join(name)
}

/// Write `path` whole or not at all: into "<name>.tmp" next to it (its
/// folder created first), then renamed over it.
pub fn write_atomic(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, contents)?;
    fs::rename(tmp, path)
}

/// The JSON file at `path`, or `T::default()` if it is missing or doesn't
/// parse (it stays as it is).
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// `value` as pretty-printed JSON, written with [`write_atomic`].
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    write_atomic(path, serde_json::to_string_pretty(value).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(default)]
    struct Settings {
        volume: f32,
        repeat: String,
        window_size: Option<(u32, u32)>,
    }

    impl Default for Settings {
        fn default() -> Self {
            Settings { volume: 0.8, repeat: "off".into(), window_size: None }
        }
    }

    #[test]
    fn writes_through_a_temporary_file_into_a_new_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new").join("x.json");
        write_atomic(&path, "{}").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
        assert!(!dir.path().join("new").join("x.json.tmp").exists(), "renamed away");
        // A temporary file left by a crash is overwritten, not a problem.
        fs::write(dir.path().join("new").join("x.json.tmp"), "half").unwrap();
        write_atomic(&path, "[]").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "[]");
        let names: Vec<_> = fs::read_dir(dir.path().join("new")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["x.json"]);
    }

    #[test]
    fn round_trip_and_defaults_for_missing_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let s = Settings { volume: 0.5, window_size: Some((800, 600)), ..Settings::default() };
        save_json(&path, &s).unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("\n  \"volume\": 0.5"), "pretty-printed");
        assert_eq!(load_json::<Settings>(&path), s);
        fs::write(&path, "{\"volume\": 0.3}").unwrap();
        let s: Settings = load_json(&path);
        assert_eq!(s.volume, 0.3);
        assert_eq!(s.repeat, "off");
    }

    #[test]
    fn missing_or_broken_files_load_as_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(load_json::<Settings>(&path), Settings::default());
        fs::write(&path, "not json").unwrap();
        assert_eq!(load_json::<Settings>(&path), Settings::default());
        assert_eq!(fs::read_to_string(&path).unwrap(), "not json", "left as it is");
    }

    #[test]
    fn the_environment_variable_wins_unless_empty() {
        let dir = tempfile::tempdir().unwrap();
        let var = "ORCHARD_SHELL_TEST_ONLY_CONFIG_DIR";
        // SAFETY: only this test reads or writes this variable.
        unsafe { std::env::set_var(var, dir.path()) };
        assert_eq!(app_config_dir(var, "orchard-test", "Orchard Test"), dir.path());
        unsafe { std::env::set_var(var, "") };
        let default = app_config_dir(var, "orchard-test", "Orchard Test");
        unsafe { std::env::remove_var(var) };
        assert_ne!(default, PathBuf::new());
        let expected = if cfg!(target_os = "linux") { "orchard-test" } else { "Orchard Test" };
        assert!(default.ends_with(expected), "{}", default.display());
    }
}
