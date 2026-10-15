//! The window: winit event loop, GPU surface, input conversion, wake-ups
//! from workers. Everything else is in the app, behind [`ShellApp`].

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use orchard_grid::{FontConfig, Frame, GridRenderer, Rendered, Rgba, WindowRenderer};
use orchard_kit::input::{KeyEvent, MouseEvent, MouseKind};
use orchard_kit::tasks::Waker;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::{Theme, Window, WindowId};

use crate::{AppInfo, winit};

/// The app side of the event loop. The required methods are what every app
/// has; the hooks (default: nothing) are where one app does more, each
/// called at a fixed point of the loop.
pub trait ShellApp: 'static {
    /// Check `quit_requested` once more at the end of `about_to_wait`, after
    /// the app's work and `window_requests` (else only after input and
    /// wake-ups).
    const QUIT_AFTER_WAIT: bool = false;

    /// The font the window should draw with (checked before every frame).
    fn font(&self) -> &FontConfig;
    /// The color a new frame starts out with.
    fn background(&self) -> Rgba;
    /// The grid size the app last got from `resized`: (columns, rows).
    fn grid(&self) -> (u16, u16);
    /// The window's logical size from the settings, if any.
    fn window_size(&self) -> Option<(u32, u32)>;
    /// The window was resized to this logical size (for the settings).
    fn set_window_size(&mut self, logical: (u32, u32));
    /// The app wants to quit.
    fn quit_requested(&self) -> bool;
    /// The system's light or dark mode, for a color scheme following it.
    fn set_system_dark(&mut self, dark: bool);
    /// The grid's size and a cell's size in pixels.
    fn resized(&mut self, cols: u16, rows: u16, cell_px: (f32, f32));
    /// Before each frame: animations, timers.
    fn tick(&mut self, now: Instant);
    fn draw(&self, frame: &mut Frame);
    /// Run what workers sent; `true`: redraw.
    fn process(&mut self) -> bool;
    /// When the next animation frame is due, if one is.
    fn next_frame(&self, now: Instant) -> Option<Instant>;
    fn handle_key(&mut self, ev: &KeyEvent);
    fn handle_mouse(&mut self, ev: &MouseEvent);
    /// On quitting (also ⌘Q from the macOS app menu): save what needs it.
    fn save(&mut self);

    /// A key press as a [`KeyEvent`], `None` for keys the app doesn't use
    /// (default: the kit's conversion, which leaves media keys out).
    fn map_key(&self, ev: &winit::event::KeyEvent, mods: ModifiersState) -> Option<KeyEvent> {
        orchard_kit::winit::convert_key(ev, mods)
    }
    /// Once, when the window is up (sized, before its first frame).
    fn started(&mut self, _window: &Window) {}
    /// Before each frame is drawn, after `tick` (window title, images).
    fn before_draw(&mut self, _window: &Window, _renderer: &mut GridRenderer) {}
    /// After each frame is drawn, before it is rendered (images it uses).
    fn after_draw(&mut self, _frame: &Frame, _renderer: &mut GridRenderer) {}
    /// After the app's work and after input: bring the window forward, ask
    /// for attention. `focused`: the window has the keyboard focus.
    fn window_requests(&mut self, _window: &Window, _focused: bool) {}
    /// The window gained or lost the focus; `true`: treat it like input
    /// (redraw, check for quit).
    fn focus_changed(&mut self, _focused: bool) -> bool {
        false
    }
    /// A file dropped on the window; `true`: treat it like input.
    fn dropped_file(&mut self, _path: PathBuf) -> bool {
        false
    }
    /// Before the app's work (`process`) when the loop is about to wait.
    fn before_process(&mut self) {}
    /// After the app's work when the loop is about to wait.
    fn after_process(&mut self) {}
}

#[derive(Debug, Clone, Copy)]
enum UserEvent {
    Wake,
}

struct Win {
    window: Arc<Window>,
    gpu: WindowRenderer,
    frame: Frame,
    font: FontConfig,
    focused: bool,
}

struct Platform<A: ShellApp> {
    app: A,
    info: &'static AppInfo,
    win: Option<Win>,
    mods: ModifiersState,
    cursor: PhysicalPosition<f64>,
    button_down: bool,
    last_click: Option<(Instant, (u16, u16))>,
    scroll_px: f64,
    /// When the next animation frame is due. Kept across wake-ups, so a
    /// stream of other events (mouse moves) can't keep pushing it back.
    next_frame: Option<Instant>,
    /// `<APP>_EXIT_AFTER_START=1`: quit after the first frame (CI starts
    /// the real app on every platform this way).
    exit_after_start: bool,
}

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

impl<A: ShellApp> Platform<A> {
    fn cell_px(&self) -> (f32, f32) {
        self.win.as_ref().map_or((8.0, 16.0), |w| {
            let m = w.gpu.renderer().metrics();
            (m.width as f32, m.height as f32)
        })
    }

    fn cell_at(&self, p: PhysicalPosition<f64>) -> (u16, u16) {
        let (cw, ch) = self.cell_px();
        let col = (p.x.max(0.0) / cw as f64) as u16;
        let row = (p.y.max(0.0) / ch as f64) as u16;
        let (cols, rows) = self.app.grid();
        (col.min(cols.saturating_sub(1)), row.min(rows.saturating_sub(1)))
    }

    fn mouse(&mut self, kind: MouseKind, double: bool) {
        let (col, row) = self.cell_at(self.cursor);
        let ev = MouseEvent { kind, col, row, x: self.cursor.x as f32, y: self.cursor.y as f32, double };
        self.app.handle_mouse(&ev);
    }

    /// Grid size for the window's current pixel size and font.
    fn sync_size(&mut self) {
        let Some(w) = &mut self.win else { return };
        let (cols, rows) = w.gpu.grid_size();
        if (cols, rows) != (w.frame.cols(), w.frame.rows()) {
            w.frame.resize(cols, rows);
        }
        let m = w.gpu.renderer().metrics();
        self.app.resized(cols, rows, (m.width as f32, m.height as f32));
    }

    fn redraw(&mut self) {
        if let Some(w) = &self.win {
            w.window.request_redraw();
        }
    }

    fn draw(&mut self) {
        self.next_frame = None;
        // Font changed (family, style, size, line spacing: settings, ⌘+ / ⌘-)?
        let font = self.app.font().clone();
        if let Some(w) = &mut self.win
            && w.font != font
        {
            if let Err(e) = w.gpu.renderer_mut().set_font(font.clone()) {
                log::warn!("font: {e}");
            }
            w.font = font;
        }
        self.sync_size();
        self.app.tick(Instant::now());
        let Some(w) = &mut self.win else { return };
        self.app.before_draw(&w.window, w.gpu.renderer_mut());
        self.app.draw(&mut w.frame);
        self.app.after_draw(&w.frame, w.gpu.renderer_mut());
        match w.gpu.render(&w.frame) {
            Ok(Rendered::Retry) => w.window.request_redraw(),
            Ok(_) => {}
            Err(e) => log::warn!("render: {e}"),
        }
    }

    /// The app asked to come forward or for attention.
    fn window_requests(&mut self) {
        let Some(w) = &self.win else { return };
        self.app.window_requests(&w.window, w.focused);
    }

    fn quit(&mut self, el: &ActiveEventLoop) {
        self.app.save();
        el.exit();
    }
}

impl<A: ShellApp> ApplicationHandler<UserEvent> for Platform<A> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_some() {
            return;
        }
        let info = self.info;
        let (w, h) = self.app.window_size().unwrap_or(info.default_size);
        let (min_w, min_h) = info.min_size;
        let attrs = Window::default_attributes()
            .with_title(info.name)
            .with_inner_size(LogicalSize::new(w.max(min_w), h.max(min_h)))
            .with_min_inner_size(LogicalSize::new(min_w, min_h));
        let window = match el.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("{}: can't open a window: {e}", info.bin);
                el.exit();
                return;
            }
        };
        // Light or dark mode, for the color scheme when following the system.
        self.app.set_system_dark(window.theme() != Some(Theme::Light));
        let font = self.app.font().clone();
        let gpu = match WindowRenderer::new(window.clone(), font.clone()) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("{}: {e}", info.bin);
                el.exit();
                return;
            }
        };
        let (cols, rows) = gpu.grid_size();
        let frame = Frame::new(cols, rows, self.app.background());
        self.win = Some(Win { window: window.clone(), gpu, frame, font, focused: true });
        self.sync_size();
        self.app.started(&window);
        self.redraw();
    }

    fn new_events(&mut self, _: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            self.redraw();
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, _: UserEvent) {
        if self.app.process() {
            self.redraw();
        }
        self.window_requests();
        if self.app.quit_requested() {
            self.quit(el);
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let mut input = true;
        match event {
            WindowEvent::CloseRequested => {
                self.quit(el);
                return;
            }
            WindowEvent::RedrawRequested => {
                self.draw();
                input = false;
                if std::mem::take(&mut self.exit_after_start) {
                    // Packaging checks: the window opened and drew (with
                    // what the app set up when it started).
                    println!("opened, drew a frame and closed");
                    self.quit(el);
                    return;
                }
            }
            WindowEvent::Resized(PhysicalSize { width, height }) => {
                if let Some(w) = &mut self.win {
                    w.gpu.resize(width, height);
                    let s = PhysicalSize::new(width, height).to_logical::<u32>(w.window.scale_factor());
                    self.app.set_window_size((s.width, s.height));
                }
                self.sync_size();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(w) = &mut self.win {
                    let _ = w.gpu.set_scale_factor(scale_factor);
                }
                self.sync_size();
            }
            WindowEvent::ThemeChanged(t) => self.app.set_system_dark(t == Theme::Dark),
            // Shown again: frames drawn while it was hidden were dropped
            // (on macOS also the very first one).
            WindowEvent::Occluded(false) => {}
            WindowEvent::Focused(f) => {
                if let Some(w) = &mut self.win {
                    w.focused = f;
                }
                input = self.app.focus_changed(f);
            }
            WindowEvent::ModifiersChanged(m) => {
                self.mods = m.state();
                input = false;
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(k) = self.app.map_key(&event, self.mods) {
                    self.app.handle_key(&k);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = position;
                if self.button_down {
                    self.mouse(MouseKind::Drag, false);
                } else {
                    input = false;
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                if state == ElementState::Pressed {
                    self.button_down = true;
                    let cell = self.cell_at(self.cursor);
                    let now = Instant::now();
                    let double = self
                        .last_click
                        .is_some_and(|(t, c)| c == cell && now.duration_since(t) < DOUBLE_CLICK);
                    self.last_click = if double { None } else { Some((now, cell)) };
                    self.mouse(MouseKind::Down, double);
                } else {
                    self.button_down = false;
                    self.mouse(MouseKind::Up, false);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let rows = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => {
                        self.scroll_px += p.y;
                        let ch = self.cell_px().1 as f64;
                        let whole = (self.scroll_px / ch).trunc();
                        self.scroll_px -= whole * ch;
                        whole as f32
                    }
                };
                if rows != 0.0 {
                    self.mouse(MouseKind::Scroll(rows), false);
                }
            }
            WindowEvent::DroppedFile(path) => input = self.app.dropped_file(path),
            _ => input = false,
        }
        if input {
            self.window_requests();
            if self.app.quit_requested() {
                self.quit(el);
                return;
            }
            self.redraw();
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.app.before_process();
        if self.app.process() {
            self.redraw();
        }
        self.app.after_process();
        self.window_requests();
        if A::QUIT_AFTER_WAIT && self.app.quit_requested() {
            self.quit(el);
            return;
        }
        let now = Instant::now();
        if self.next_frame.is_some_and(|t| now >= t) {
            self.next_frame = None;
            self.redraw();
        }
        if self.next_frame.is_none() {
            self.next_frame = self.app.next_frame(now);
        }
        el.set_control_flow(match self.next_frame {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        });
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        // ⌘Q from the macOS app menu ends the loop without CloseRequested.
        self.app.save();
    }
}

/// Run the app in a window until it quits: builds the event loop, then the
/// app with `make` (given the waker its workers call to wake the loop).
pub fn run<A: ShellApp>(info: &'static AppInfo, make: impl FnOnce(Waker) -> A) -> ExitCode {
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("{}: {e}", info.bin);
            return ExitCode::FAILURE;
        }
    };
    let proxy = Mutex::new(event_loop.create_proxy());
    let wake: Waker = Arc::new(move || {
        let _ = proxy.lock().unwrap().send_event(UserEvent::Wake);
    });
    let app = make(wake);
    let mut platform = Platform {
        app,
        info,
        win: None,
        mods: ModifiersState::empty(),
        cursor: PhysicalPosition::new(0.0, 0.0),
        button_down: false,
        last_click: None,
        scroll_px: 0.0,
        next_frame: None,
        exit_after_start: std::env::var_os(info.exit_after_start_env).is_some_and(|v| v == "1"),
    };
    match event_loop.run_app(&mut platform) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}: {e}", info.bin);
            ExitCode::FAILURE
        }
    }
}
