//! The command line every app has: `-h`/`--help`, `-V`/`--version`,
//! `--self-test` and `--screenshot`, the crash log, and (on Windows) the
//! console a GUI program prints to. The app's own arguments (files, links)
//! stay with the app.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::AppInfo;

/// Panics also go to `crash.log` in the config folder: on Windows the app
/// has no console, so a crash would leave no trace otherwise.
pub fn log_panics(info: &'static AppInfo, config_dir: fn() -> PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        let backtrace = std::backtrace::Backtrace::force_capture();
        let text = format!("{} {} crashed: {panic}\n{backtrace}\n", info.name, info.version);
        let _ = std::fs::write(dir.join("crash.log"), text);
        default(panic);
    }));
}

/// Windows: started with an option (`--self-test`, `--version`…) from a
/// console and with nothing redirected, print to that console. A GUI
/// program gets no console of its own, and files alone open no console.
#[cfg(windows)]
pub fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE};
    if !std::env::args_os().skip(1).any(|a| a.to_string_lossy().starts_with('-')) {
        return;
    }
    // SAFETY: plain Win32 calls without pointers.
    unsafe {
        if GetStdHandle(STD_OUTPUT_HANDLE).is_null() {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

/// The options every app has, anywhere in `args` (the arguments after the
/// program's name): `-h`/`--help` prints `usage`, `-V`/`--version` the
/// app's name and version, `--self-test` runs `self_test`; a leading
/// `--screenshot` hands the rest to `screenshot`. `None`: none of them, go
/// on (the app's own arguments are left alone).
pub fn common_args(
    info: &AppInfo,
    args: &[String],
    usage: &str,
    self_test: impl FnOnce() -> ExitCode,
    screenshot: impl FnOnce(&[String]) -> ExitCode,
) -> Option<ExitCode> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{usage}");
        return Some(ExitCode::SUCCESS);
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("{} {}", info.name, info.version);
        return Some(ExitCode::SUCCESS);
    }
    if args.iter().any(|a| a == "--self-test") {
        return Some(self_test());
    }
    if args.first().map(String::as_str) == Some("--screenshot") {
        return Some(screenshot(&args[1..]));
    }
    None
}

/// A self-test check: what it tests, and what it did or why it failed.
pub type Check<'a> = (&'a str, Box<dyn FnOnce() -> Result<String, String> + 'a>);

/// `--self-test` (for packaging checks; no window): prints the app's name
/// and version, runs every check in order, then prints one line per check.
/// Fails if any check failed.
pub fn self_test(info: &AppInfo, checks: Vec<Check<'_>>) -> ExitCode {
    println!("{} {}", info.name, info.version);
    let results: Vec<_> = checks.into_iter().map(|(what, check)| (what, check())).collect();
    let mut ok = true;
    for (what, result) in &results {
        ok &= result.is_ok();
        println!("{}", result_line(what, result));
    }
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn result_line(what: &str, result: &Result<String, String>) -> String {
    match result {
        Ok(done) => format!("ok    {what}: {done}"),
        Err(e) => format!("FAIL  {what}: {e}"),
    }
}

/// The self-test's "render" check: `screenshot` (the app's `--screenshot`)
/// into a temporary file, removed again.
pub fn render_check(info: &AppInfo, screenshot: impl FnOnce(&[String]) -> ExitCode) -> Result<String, String> {
    let shot = std::env::temp_dir().join(format!("{}-self-test-{}.png", info.bin, std::process::id()));
    let draw = match screenshot(&[shot.to_string_lossy().into_owned()]) {
        ExitCode::SUCCESS => Ok("the UI drew headlessly".to_string()),
        _ => Err("the UI didn't draw".to_string()),
    };
    let _ = std::fs::remove_file(&shot);
    draw
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: AppInfo = AppInfo {
        name: "Orchard Test",
        bin: "orchard-test",
        version: "1.2.3",
        default_size: (800, 600),
        min_size: (400, 300),
        exit_after_start_env: "ORCHARD_TEST_EXIT_AFTER_START",
    };

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn result_lines_line_up() {
        assert_eq!(result_line("render", &Ok("the UI drew headlessly".into())), "ok    render: the UI drew headlessly");
        assert_eq!(result_line("audio", &Err("no decoder".into())), "FAIL  audio: no decoder");
    }

    #[test]
    fn common_options_and_the_rest() {
        let none = |a: &[&str]| common_args(&INFO, &args(a), "usage", || panic!("self-test"), |_| panic!("screenshot"));
        assert_eq!(none(&["song.mp3", "-psn_0_1"]), None, "the app's own arguments");
        assert_eq!(none(&["a", "--help"]), Some(ExitCode::SUCCESS));
        assert_eq!(none(&["-V"]), Some(ExitCode::SUCCESS));
        let st = common_args(&INFO, &args(&["x", "--self-test"]), "", || ExitCode::FAILURE, |_| panic!());
        assert_eq!(st, Some(ExitCode::FAILURE));
        let mut seen = Vec::new();
        let shot = common_args(&INFO, &args(&["--screenshot", "out.png", "--size", "80x24"]), "", || panic!(), |a| {
            seen = a.to_vec();
            ExitCode::SUCCESS
        });
        assert_eq!(shot, Some(ExitCode::SUCCESS));
        assert_eq!(seen, args(&["out.png", "--size", "80x24"]));
        // Only a leading --screenshot.
        assert_eq!(none(&["x", "--screenshot", "out.png"]), None);
    }

    #[test]
    fn checks_run_in_order_and_any_failure_fails() {
        let order = std::cell::RefCell::new(Vec::new());
        let check = |name: &'static str, ok: bool| -> Check<'_> {
            let order = &order;
            (name, Box::new(move || {
                order.borrow_mut().push(name);
                if ok { Ok("done".into()) } else { Err("broken".into()) }
            }))
        };
        assert_eq!(self_test(&INFO, vec![check("a", true), check("b", true)]), ExitCode::SUCCESS);
        assert_eq!(self_test(&INFO, vec![check("c", false), check("d", true)]), ExitCode::FAILURE);
        assert_eq!(*order.borrow(), ["a", "b", "c", "d"]);
    }

    #[test]
    fn the_render_check_cleans_up() {
        let mut out = None;
        let r = render_check(&INFO, |a| {
            let path = PathBuf::from(&a[0]);
            std::fs::write(&path, b"png").unwrap();
            out = Some(path);
            ExitCode::SUCCESS
        });
        assert_eq!(r, Ok("the UI drew headlessly".into()));
        let out = out.unwrap();
        assert!(out.file_name().unwrap().to_string_lossy().starts_with("orchard-test-self-test-"));
        assert!(!out.exists());
        assert_eq!(render_check(&INFO, |_| ExitCode::FAILURE), Err("the UI didn't draw".into()));
    }
}
