//! Where Orchard Commander keeps its config: the same directory the Python
//! version gets from `platformdirs.user_config_dir(...)`, so existing users
//! keep their settings. The other Orchard apps read its `color-schemes/`
//! folder from here, so schemes made in its editor show up everywhere.
//!
//! - macOS: `~/Library/Application Support/Orchard Commander`
//! - Windows: `%LOCALAPPDATA%\Orchard Commander` (platformdirs' default is not roaming)
//! - Linux: `$XDG_CONFIG_HOME/orchard-commander` or `~/.config/orchard-commander`
//!   (config dirs there conventionally avoid spaces)
//!
//! platformdirs also honours an absolute `$XDG_CONFIG_HOME` on macOS
//! (`$XDG_CONFIG_HOME/Orchard Commander`), and `WIN_PD_OVERRIDE_LOCAL_APPDATA`
//! on Windows; so does this module.

use std::path::PathBuf;

use crate::ColorScheme;
use crate::schemes;

/// The app's name: the config directory on macOS and Windows, and the keychain service.
pub const APP_NAME: &str = "Orchard Commander";
/// The command name: the config directory on Linux.
pub const COMMAND: &str = "orchard-commander";
/// The name before the rename; its config directory is migrated once.
pub const LEGACY_NAME: &str = "fcmd";

/// Overrides the config directory (tests, portable installs). Not in the Python version.
pub const CONFIG_DIR_ENV: &str = "ORCHARD_COMMANDER_CONFIG_DIR";

/// Orchard Commander's config directory: `$ORCHARD_COMMANDER_CONFIG_DIR` if
/// set (and not empty), else the platform's.
pub fn commander_config_dir() -> PathBuf {
    if let Some(dir) = config_dir_override() {
        return dir;
    }
    let name = if cfg!(target_os = "linux") {
        COMMAND
    } else {
        APP_NAME
    };
    user_config_base().join(name)
}

/// The config directory from before the rename ("fcmd"), next to the
/// platform's; `None` when `$ORCHARD_COMMANDER_CONFIG_DIR` overrides the
/// config directory (there is nothing to migrate from then).
pub fn commander_legacy_dir() -> Option<PathBuf> {
    if config_dir_override().is_some() {
        return None;
    }
    Some(user_config_base().join(LEGACY_NAME))
}

/// The color-schemes folder shared with Orchard Commander (user schemes,
/// one TOML file each).
pub fn schemes_dir() -> PathBuf {
    commander_config_dir().join("color-schemes")
}

/// Every scheme: built-ins, then the user's (blocking: reads the folder).
pub fn all_schemes() -> Vec<ColorScheme> {
    schemes::all_schemes(&schemes_dir())
}

fn config_dir_override() -> Option<PathBuf> {
    std::env::var_os(CONFIG_DIR_ENV)
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
}

/// platformdirs' (>= 4.12, as the Python version requires) `user_config_dir()`
/// without the app name.
fn user_config_base() -> PathBuf {
    if cfg!(windows) {
        // The local (not roaming) AppData, unless platformdirs' override is set.
        let override_dir = env_trimmed("WIN_PD_OVERRIDE_LOCAL_APPDATA")
            .filter(|p| is_fully_qualified_nt(p))
            .map(PathBuf::from);
        return override_dir
            .or_else(::dirs::data_local_dir)
            .unwrap_or_default();
    }
    unix_config_base(
        env_trimmed("XDG_CONFIG_HOME"),
        ::dirs::home_dir,
        cfg!(target_os = "macos"),
    )
}

/// On every Unix, macOS included, an absolute `$XDG_CONFIG_HOME` wins; a
/// relative or blank one is ignored (the XDG spec says so).
fn unix_config_base(
    xdg: Option<String>,
    home: impl FnOnce() -> Option<PathBuf>,
    macos: bool,
) -> PathBuf {
    if let Some(xdg) = xdg.filter(|p| p.starts_with('/')) {
        return PathBuf::from(xdg);
    }
    let home = home().unwrap_or_default();
    if macos {
        home.join("Library/Application Support")
    } else {
        home.join(".config")
    }
}

/// An environment variable with surrounding whitespace removed (Python's `.strip()`).
fn env_trimmed(name: &str) -> Option<String> {
    let value = std::env::var_os(name)?;
    Some(value.to_string_lossy().trim().to_owned())
}

/// platformdirs' `_is_fully_qualified` for Windows paths: a UNC path or a
/// drive with a root ("C:\x"), not "C:x" or "\x".
#[cfg_attr(not(windows), allow(dead_code))]
fn is_fully_qualified_nt(path: &str) -> bool {
    let is_sep = |c: char| c == '\\' || c == '/';
    let mut chars = path.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some(a), Some(b), _) if is_sep(a) && is_sep(b) => true,
        (Some(_), Some(':'), Some(c)) => is_sep(c),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn xdg_config_home_like_platformdirs() {
        let home = || Some(PathBuf::from("/home/me"));
        let base = |xdg: Option<&str>, macos| unix_config_base(xdg.map(String::from), home, macos);
        assert_eq!(base(None, false), Path::new("/home/me/.config"));
        assert_eq!(
            base(None, true),
            Path::new("/home/me/Library/Application Support")
        );
        assert_eq!(base(Some("/xdg"), false), Path::new("/xdg"));
        assert_eq!(base(Some("/xdg"), true), Path::new("/xdg")); // macOS too
        assert_eq!(base(Some("rel/dir"), false), Path::new("/home/me/.config"));
        assert_eq!(
            base(Some(""), true),
            Path::new("/home/me/Library/Application Support")
        );
    }

    #[test]
    fn windows_override_must_be_fully_qualified() {
        assert!(is_fully_qualified_nt("C:\\Data"));
        assert!(is_fully_qualified_nt("d:/data"));
        assert!(is_fully_qualified_nt("\\\\server\\share"));
        assert!(!is_fully_qualified_nt("C:data"));
        assert!(!is_fully_qualified_nt("\\data"));
        assert!(!is_fully_qualified_nt("data"));
        assert!(!is_fully_qualified_nt(""));
    }

    #[test]
    fn schemes_live_in_the_config_dir() {
        assert_eq!(schemes_dir(), commander_config_dir().join("color-schemes"));
        if config_dir_override().is_some() {
            assert_eq!(commander_legacy_dir(), None);
        } else {
            assert_eq!(
                commander_legacy_dir().unwrap().parent(),
                commander_config_dir().parent()
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn platform_dir_matches_platformdirs_on_macos() {
        // platformdirs honours an absolute $XDG_CONFIG_HOME on macOS too.
        let xdg = std::env::var("XDG_CONFIG_HOME").unwrap_or_default();
        if std::env::var_os(CONFIG_DIR_ENV).is_some() || xdg.trim().starts_with('/') {
            return;
        }
        let home = ::dirs::home_dir().unwrap();
        assert_eq!(
            commander_config_dir(),
            home.join("Library/Application Support/Orchard Commander")
        );
        assert_eq!(
            commander_legacy_dir().unwrap(),
            home.join("Library/Application Support/fcmd")
        );
    }
}
