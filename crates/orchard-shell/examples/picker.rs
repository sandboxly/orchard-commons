//! The shared file and folder picker in a window: places, Orchard
//! Commander's bookmarks (⌘D / Ctrl+D), the listing. Choosing prints the
//! choice and opens the picker again; Esc quits.
//!
//! ```sh
//! cargo run -p orchard-shell --example picker            # pick a folder
//! cargo run -p orchard-shell --example picker -- files   # pick files (marking)
//! cargo run -p orchard-shell --example picker -- --screenshot /tmp/picker.png --size 110x34
//! ```
//!
//! It reads (and "Add current folder…" writes) Orchard Commander's real
//! bookmarks file; set `ORCHARD_COMMANDER_CONFIG_DIR` to try it on another.

use std::process::ExitCode;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use orchard_grid::{CellRect, FontConfig, Frame, Rgba};
use orchard_kit::input::{KeyEvent, MouseEvent};
use orchard_kit::picker::{Options, Outcome, Picker};
use orchard_kit::tasks::Waker;
use orchard_kit::widgets::DrawCtx;
use orchard_shell::{AppInfo, ShellApp, cli, shot};
use orchard_theme::Theme;

const INFO: AppInfo = AppInfo {
    name: "Orchard Picker",
    bin: "orchard-picker",
    version: env!("CARGO_PKG_VERSION"),
    default_size: (960, 600),
    min_size: (320, 200),
    exit_after_start_env: "ORCHARD_PICKER_EXIT_AFTER_START",
};

struct Demo {
    theme: Theme,
    cols: u16,
    rows: u16,
    cell_px: (f32, f32),
    files: bool,
    waker: Waker,
    picker: Picker,
    quit: bool,
    window_size: Option<(u32, u32)>,
}

fn options(files: bool) -> Options {
    let home = dirs::home_dir().unwrap_or_default();
    if files { Options::files(home).multi().mark_folders().add_folder().accept("Add files") } else { Options::folder(home) }
}

impl Demo {
    fn new(files: bool, waker: Waker) -> Self {
        Demo {
            theme: Theme::default_dark(),
            cols: 100,
            rows: 30,
            cell_px: (8.0, 16.0),
            files,
            picker: Picker::new(options(files), waker.clone()),
            waker,
            quit: false,
            window_size: None,
        }
    }

    fn screen(&self) -> CellRect {
        CellRect::new(0, 0, self.cols, self.rows)
    }

    fn outcome(&mut self, out: Outcome) {
        match &out {
            Outcome::Keep => {}
            Outcome::Cancel => self.quit = true,
            Outcome::Folder(d) => println!("folder: {}", d.display()),
            Outcome::Files(f) => f.iter().for_each(|p| println!("file: {}", p.display())),
            Outcome::AddFolder(d) => println!("add folder: {}", d.display()),
        }
        if !matches!(out, Outcome::Keep | Outcome::Cancel | Outcome::AddFolder(_)) {
            self.picker = Picker::new(options(self.files), self.waker.clone());
        }
    }
}

impl ShellApp for Demo {
    fn font(&self) -> &FontConfig {
        &self.theme.font
    }
    fn background(&self) -> Rgba {
        self.theme.colors.background
    }
    fn grid(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }
    fn window_size(&self) -> Option<(u32, u32)> {
        self.window_size
    }
    fn set_window_size(&mut self, logical: (u32, u32)) {
        self.window_size = Some(logical);
    }
    fn quit_requested(&self) -> bool {
        self.quit
    }
    fn set_system_dark(&mut self, _dark: bool) {}
    fn resized(&mut self, cols: u16, rows: u16, cell_px: (f32, f32)) {
        (self.cols, self.rows, self.cell_px) = (cols, rows, cell_px);
    }
    fn tick(&mut self, _now: Instant) {}
    fn draw(&self, frame: &mut Frame) {
        frame.clear(self.theme.colors.background);
        let ctx = DrawCtx { theme: &self.theme, cell_px: self.cell_px };
        self.picker.draw(frame, self.screen(), &ctx);
    }
    fn process(&mut self) -> bool {
        self.picker.poll()
    }
    fn next_frame(&self, _now: Instant) -> Option<Instant> {
        None
    }
    fn handle_key(&mut self, ev: &KeyEvent) {
        let out = self.picker.key(ev, self.screen());
        self.outcome(out);
    }
    fn handle_mouse(&mut self, ev: &MouseEvent) {
        let out = self.picker.mouse(ev, self.screen());
        self.outcome(out);
    }
    fn save(&mut self) {}
}

fn screenshot(args: &[String]) -> ExitCode {
    let mut files = false;
    let o = match shot::parse(args, (110, 34), |a, _| files |= a == "files") {
        Ok(o) => o,
        Err(code) => return code,
    };
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let mut app = Demo::new(files, Arc::new(move || {
        let _ = tx.lock().unwrap().send(());
    }));
    // Wait for the listing, places and bookmarks (a few seconds at most).
    let until = Instant::now() + Duration::from_secs(5);
    while app.picker.loading() && Instant::now() < until {
        let _ = rx.recv_timeout(until.saturating_duration_since(Instant::now()));
        app.picker.poll();
    }
    let mut r = match shot::renderer(app.theme.font.clone()) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let (cols, rows) = o.size;
    app.resized(cols, rows, shot::cell_px(&mut r));
    if o.dialog.as_deref() == Some("bookmarks") {
        let d = orchard_kit::input::KeyEvent::char('d').with(orchard_kit::input::Mods { ctrl: true, logo: true, ..Default::default() });
        app.picker.key(&d, app.screen());
    }
    let mut frame = Frame::new(cols, rows, app.background());
    app.draw(&mut frame);
    shot::finish(&mut r, &frame, &o.out)
}

fn main() -> ExitCode {
    #[cfg(windows)]
    cli::attach_parent_console();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: picker [files]\n       picker --screenshot OUT.png|OUT.webp [--size COLSxROWS] [files] [--dialog bookmarks]";
    let self_test = || cli::self_test(&INFO, vec![("render", Box::new(|| cli::render_check(&INFO, screenshot)))]);
    if let Some(code) = cli::common_args(&INFO, &args, usage, self_test, screenshot) {
        return code;
    }
    let files = args.iter().any(|a| a == "files");
    orchard_shell::run(&INFO, move |wake| Demo::new(files, wake))
}
