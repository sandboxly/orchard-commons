//! The smallest app on the shared shell: a dialog form in a window, with the
//! command line every app has.
//!
//! Keys: Tab / ↑↓ move the focus, Space toggles, Enter presses, Esc or
//! "Close" quits. A packaging-style check: `ORCHARD_HELLO_EXIT_AFTER_START=1`.
//!
//! ```sh
//! cargo run -p orchard-shell --example hello
//! cargo run -p orchard-shell --example hello -- --self-test
//! cargo run -p orchard-shell --example hello -- --screenshot /tmp/hello.png --size 60x16
//! ```

use std::process::ExitCode;
use std::time::Instant;

use orchard_grid::{CellRect, FontConfig, Frame, Rgba};
use orchard_kit::form::{self, Act, Ctl, Form, Kind, TextKind};
use orchard_kit::input::{KeyEvent, MouseEvent};
use orchard_kit::widgets::DrawCtx;
use orchard_shell::{AppInfo, ShellApp, cli, shot};
use orchard_theme::Theme;

const INFO: AppInfo = AppInfo {
    name: "Orchard Hello",
    bin: "orchard-hello",
    version: env!("CARGO_PKG_VERSION"),
    default_size: (640, 360),
    min_size: (320, 200),
    exit_after_start_env: "ORCHARD_HELLO_EXIT_AFTER_START",
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Id {
    Greet,
    Count,
    Close,
}

struct Hello {
    theme: Theme,
    cols: u16,
    rows: u16,
    cell_px: (f32, f32),
    greet: bool,
    count: u32,
    focus: Option<Id>,
    quit: bool,
    window_size: Option<(u32, u32)>,
}

impl Hello {
    fn new() -> Self {
        Hello {
            theme: Theme::default_dark(),
            cols: 80,
            rows: 24,
            cell_px: (8.0, 16.0),
            greet: true,
            count: 0,
            focus: None,
            quit: false,
            window_size: None,
        }
    }

    /// The form, laid out for the current grid: (outer box, form).
    fn layout(&self) -> (CellRect, Form<Id>) {
        let screen = CellRect::new(0, 0, self.cols, self.rows);
        let (outer, inner) = form::dialog_rects(screen, 44, 7);
        let mut f = Form::default();
        let line = |r: u16, w: u16| CellRect::new(inner.col, inner.row + r, w.min(inner.cols), 1);
        f.label(line(0, inner.cols), "The shared shell, kit and form", TextKind::Heading);
        let text = if self.greet { format!("Hello! Pressed {} times", self.count) } else { String::new() };
        f.label(line(2, inner.cols), text, TextKind::Label);
        f.ctls.push(Ctl::new(Id::Greet, line(3, 20), Kind::Check("Say hello".into(), self.greet)));
        let count = form::button_w("Count");
        f.ctls.push(Ctl::new(Id::Count, line(5, count), Kind::Button("Count".into())));
        let close = CellRect::new(inner.col + count + 1, inner.row + 5, form::button_w("Close"), 1);
        f.ctls.push(Ctl::new(Id::Close, close, Kind::Button("Close".into())));
        (outer, f)
    }

    fn act(&mut self, act: Act<Id>) {
        match act {
            Act::Toggle(Id::Greet) => self.greet = !self.greet,
            Act::Press(Id::Count) => self.count += 1,
            Act::Press(Id::Close) | Act::Escape => self.quit = true,
            _ => {}
        }
    }
}

impl ShellApp for Hello {
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
        let (outer, f) = self.layout();
        form::dialog_box(frame, outer, "Hello", &ctx);
        let mut focus = self.focus;
        f.fix_focus(&mut focus);
        f.draw(frame, focus, &ctx);
    }
    fn process(&mut self) -> bool {
        false
    }
    fn next_frame(&self, _now: Instant) -> Option<Instant> {
        None
    }
    fn handle_key(&mut self, ev: &KeyEvent) {
        let (_, f) = self.layout();
        let act = f.handle_key(&mut self.focus, ev, None);
        self.act(act);
    }
    fn handle_mouse(&mut self, ev: &MouseEvent) {
        let (_, f) = self.layout();
        let act = f.handle_mouse(&mut self.focus, ev);
        self.act(act);
    }
    fn save(&mut self) {}
}

fn screenshot(args: &[String]) -> ExitCode {
    let o = match shot::parse(args, (60, 16), |_, _| {}) {
        Ok(o) => o,
        Err(code) => return code,
    };
    let mut app = Hello::new();
    if o.dialog.is_some() {
        app.count = 3;
    }
    let mut r = match shot::renderer(app.theme.font.clone()) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let (cols, rows) = o.size;
    app.resized(cols, rows, shot::cell_px(&mut r));
    let mut frame = Frame::new(cols, rows, app.background());
    app.draw(&mut frame);
    shot::finish(&mut r, &frame, &o.out)
}

fn main() -> ExitCode {
    #[cfg(windows)]
    cli::attach_parent_console();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: hello\n       hello --version | --self-test\n       hello --screenshot OUT.png|OUT.webp [--size COLSxROWS]";
    let self_test = || cli::self_test(&INFO, vec![("render", Box::new(|| cli::render_check(&INFO, screenshot)))]);
    if let Some(code) = cli::common_args(&INFO, &args, usage, self_test, screenshot) {
        return code;
    }
    orchard_shell::run(&INFO, |_wake| Hello::new())
}
