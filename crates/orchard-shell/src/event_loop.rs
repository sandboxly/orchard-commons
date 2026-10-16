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

    /// A second window the app wants open, or `None` (checked with
    /// `window_requests`). See [`Popup`].
    fn popup(&self) -> Option<Popup> {
        None
    }
    /// The popup's grid size: (columns, rows).
    fn popup_resized(&mut self, _cols: u16, _rows: u16) {}
    fn draw_popup(&self, _frame: &mut Frame) {}
    fn popup_key(&mut self, _ev: &KeyEvent) {}
    fn popup_mouse(&mut self, _ev: &MouseEvent) {}
    /// The user closed the popup's window (its close button, the window
    /// manager): the popup should go away, like on Esc.
    fn popup_closed(&mut self) {}
}

/// A second window, for a question that should be answered where the user
/// is rather than in the app's window (which may be behind others, minimized
/// or on another workspace): it opens like a new window does, so the window
/// manager places it next to the focused window and gives it the focus.
/// Keys, clicks and drawing for it go to the `popup_*` hooks; the main
/// window stays as it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Popup {
    /// Which question: a different one opens a new window (which comes
    /// forward and takes the focus again).
    pub id: u64,
    pub title: String,
    /// The size it asks for, in cells (a tiling window manager may give it
    /// another).
    pub cells: (u16, u16),
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
    cursor: PhysicalPosition<f64>,
    button_down: bool,
    last_click: Option<(Instant, (u16, u16))>,
    scroll_px: f64,
}

impl Win {
    fn new(window: Arc<Window>, gpu: WindowRenderer, background: Rgba, font: FontConfig) -> Win {
        let (cols, rows) = gpu.grid_size();
        Win {
            window,
            gpu,
            frame: Frame::new(cols, rows, background),
            font,
            focused: true,
            cursor: PhysicalPosition::new(0.0, 0.0),
            button_down: false,
            last_click: None,
            scroll_px: 0.0,
        }
    }

    fn cell_px(&self) -> (f32, f32) {
        let m = self.gpu.renderer().metrics();
        (m.width as f32, m.height as f32)
    }

    /// The cell under the pointer, within a grid of this size.
    fn cell_at(&self, (cols, rows): (u16, u16)) -> (u16, u16) {
        let (cw, ch) = self.cell_px();
        let col = (self.cursor.x.max(0.0) / cw as f64) as u16;
        let row = (self.cursor.y.max(0.0) / ch as f64) as u16;
        (
            col.min(cols.saturating_sub(1)),
            row.min(rows.saturating_sub(1)),
        )
    }

    /// Follow a font change (family, style, size, line spacing).
    fn set_font(&mut self, font: &FontConfig) {
        if self.font != *font {
            if let Err(e) = self.gpu.renderer_mut().set_font(font.clone()) {
                log::warn!("font: {e}");
            }
            self.font = font.clone();
        }
    }

    /// Resize the frame to the window; returns the grid size.
    fn sync_frame(&mut self) -> (u16, u16) {
        let (cols, rows) = self.gpu.grid_size();
        if (cols, rows) != (self.frame.cols(), self.frame.rows()) {
            self.frame.resize(cols, rows);
        }
        (cols, rows)
    }

    fn render(&mut self) {
        match self.gpu.render(&self.frame) {
            Ok(Rendered::Retry) => self.window.request_redraw(),
            Ok(_) => {}
            Err(e) => log::warn!("render: {e}"),
        }
    }
}

/// Which window an event is for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Which {
    Main,
    Popup,
}

struct Platform<A: ShellApp> {
    app: A,
    info: &'static AppInfo,
    win: Option<Win>,
    /// The app's popup and its window.
    popup: Option<(Popup, Win)>,
    mods: ModifiersState,
    /// When the next animation frame is due. Kept across wake-ups, so a
    /// stream of other events (mouse moves) can't keep pushing it back.
    next_frame: Option<Instant>,
    /// `<APP>_EXIT_AFTER_START=1`: quit after the first frame (CI starts
    /// the real app on every platform this way).
    exit_after_start: bool,
}

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

impl<A: ShellApp> Platform<A> {
    fn win_mut(&mut self, which: Which) -> Option<&mut Win> {
        match which {
            Which::Main => self.win.as_mut(),
            Which::Popup => self.popup.as_mut().map(|(_, w)| w),
        }
    }

    fn mouse(&mut self, which: Which, kind: MouseKind, double: bool) {
        let grid = match which {
            Which::Main => self.app.grid(),
            Which::Popup => self
                .popup
                .as_ref()
                .map_or((1, 1), |(_, w)| (w.frame.cols(), w.frame.rows())),
        };
        let Some(w) = self.win_mut(which) else { return };
        let (col, row) = w.cell_at(grid);
        let ev = MouseEvent {
            kind,
            col,
            row,
            x: w.cursor.x as f32,
            y: w.cursor.y as f32,
            double,
        };
        match which {
            Which::Main => self.app.handle_mouse(&ev),
            Which::Popup => self.app.popup_mouse(&ev),
        }
    }

    /// Grid size for the window's current pixel size and font.
    fn sync_size(&mut self) {
        let Some(w) = &mut self.win else { return };
        let (cols, rows) = w.sync_frame();
        let (cw, ch) = w.cell_px();
        self.app.resized(cols, rows, (cw, ch));
    }

    fn sync_popup_size(&mut self) {
        let Some((_, w)) = &mut self.popup else {
            return;
        };
        let (cols, rows) = w.sync_frame();
        self.app.popup_resized(cols, rows);
    }

    fn redraw(&mut self) {
        if let Some(w) = &self.win {
            w.window.request_redraw();
        }
        if let Some((_, w)) = &self.popup {
            w.window.request_redraw();
        }
    }

    fn draw(&mut self) {
        self.next_frame = None;
        // Font changed (family, style, size, line spacing: settings, ⌘+ / ⌘-)?
        let font = self.app.font().clone();
        if let Some(w) = &mut self.win {
            w.set_font(&font);
        }
        self.sync_size();
        self.app.tick(Instant::now());
        let Some(w) = &mut self.win else { return };
        self.app.before_draw(&w.window, w.gpu.renderer_mut());
        self.app.draw(&mut w.frame);
        self.app.after_draw(&w.frame, w.gpu.renderer_mut());
        w.render();
    }

    fn draw_popup(&mut self) {
        let font = self.app.font().clone();
        if let Some((_, w)) = &mut self.popup {
            w.set_font(&font);
        }
        self.sync_popup_size();
        let Some((_, w)) = &mut self.popup else {
            return;
        };
        self.app.draw_popup(&mut w.frame);
        w.render();
    }

    /// The app asked to come forward or for attention, or for its popup
    /// (opened, changed or closed here).
    fn window_requests(&mut self, el: &ActiveEventLoop) {
        if let Some(w) = &self.win {
            self.app.window_requests(&w.window, w.focused);
        }
        let want = self.app.popup();
        let px = want.as_ref().map(|p| self.popup_px(p.cells));
        match (&mut self.popup, want) {
            (None, None) => {}
            (Some(_), None) => self.popup = None,
            (Some((have, w)), Some(want)) if have.id == want.id => {
                if have.title != want.title {
                    w.window.set_title(&want.title);
                }
                if have.cells != want.cells
                    && let Some(px) = px
                {
                    let _ = w.window.request_inner_size(px);
                }
                *have = want;
            }
            (_, Some(want)) => {
                self.popup = None;
                self.open_popup(el, want);
            }
        }
    }

    /// A popup's size in pixels, by the main window's cells.
    fn popup_px(&self, (cols, rows): (u16, u16)) -> PhysicalSize<u32> {
        let (cw, ch) = self.win.as_ref().map_or((8.0, 16.0), Win::cell_px);
        PhysicalSize::new(
            (cols as f32 * cw).ceil() as u32,
            (rows as f32 * ch).ceil() as u32,
        )
    }

    fn open_popup(&mut self, el: &ActiveEventLoop, popup: Popup) {
        let attrs = Window::default_attributes()
            .with_title(&popup.title)
            .with_inner_size(self.popup_px(popup.cells));
        let window = match el.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::warn!("can't open a window: {e}");
                return;
            }
        };
        let font = self.app.font().clone();
        // On the main window's GPU: a GPU context of its own would break
        // the main window when it goes (see `orchard_grid::Gpu`).
        let library = orchard_grid::FontLibrary::system();
        let shared = self.win.as_ref().map(|main| WindowRenderer::with_gpu(window.clone(), main.gpu.gpu(), library.clone(), font.clone()));
        let made = match shared {
            Some(Ok(g)) => Ok(g),
            // Better than a dialog that can't be seen.
            other => {
                if let Some(Err(e)) = other {
                    log::warn!("popup on the main window's GPU: {e}");
                }
                WindowRenderer::with_library(window.clone(), library, font.clone())
            }
        };
        let gpu = match made {
            Ok(g) => g,
            Err(e) => {
                log::warn!("{e}");
                return;
            }
        };
        self.popup = Some((
            popup,
            Win::new(window.clone(), gpu, self.app.background(), font),
        ));
        self.sync_popup_size();
        window.focus_window();
        window.request_redraw();
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
        self.app
            .set_system_dark(window.theme() != Some(Theme::Light));
        let font = self.app.font().clone();
        let gpu = match WindowRenderer::new(window.clone(), font.clone()) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("{}: {e}", info.bin);
                el.exit();
                return;
            }
        };
        self.win = Some(Win::new(window.clone(), gpu, self.app.background(), font));
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
        self.window_requests(el);
        if self.app.quit_requested() {
            self.quit(el);
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let which = match &self.popup {
            Some((_, w)) if w.window.id() == id => Which::Popup,
            _ => Which::Main,
        };
        let mut input = true;
        match event {
            WindowEvent::CloseRequested if which == Which::Popup => {
                self.popup = None;
                self.app.popup_closed();
            }
            WindowEvent::CloseRequested => {
                self.quit(el);
                return;
            }
            WindowEvent::RedrawRequested if which == Which::Popup => {
                self.draw_popup();
                input = false;
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
                if let Some(w) = self.win_mut(which) {
                    w.gpu.resize(width, height);
                }
                match which {
                    Which::Main => {
                        if let Some(w) = &self.win {
                            let s = PhysicalSize::new(width, height)
                                .to_logical::<u32>(w.window.scale_factor());
                            self.app.set_window_size((s.width, s.height));
                        }
                        self.sync_size();
                    }
                    Which::Popup => self.sync_popup_size(),
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(w) = self.win_mut(which) {
                    let _ = w.gpu.set_scale_factor(scale_factor);
                }
                match which {
                    Which::Main => self.sync_size(),
                    Which::Popup => self.sync_popup_size(),
                }
            }
            WindowEvent::ThemeChanged(t) => self.app.set_system_dark(t == Theme::Dark),
            // Shown again: frames drawn while it was hidden were dropped
            // (on macOS also the very first one).
            WindowEvent::Occluded(false) => {}
            WindowEvent::Focused(f) => {
                if let Some(w) = self.win_mut(which) {
                    w.focused = f;
                }
                if which == Which::Main {
                    input = self.app.focus_changed(f);
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                self.mods = m.state();
                input = false;
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(k) = self.app.map_key(&event, self.mods) {
                    match which {
                        Which::Main => self.app.handle_key(&k),
                        Which::Popup => self.app.popup_key(&k),
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let Some(w) = self.win_mut(which) else { return };
                w.cursor = position;
                if w.button_down {
                    self.mouse(which, MouseKind::Drag, false);
                } else {
                    input = false;
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let Some(w) = self.win_mut(which) else { return };
                if state == ElementState::Pressed {
                    w.button_down = true;
                    let cell = w.cell_at((w.frame.cols(), w.frame.rows()));
                    let now = Instant::now();
                    let double = w
                        .last_click
                        .is_some_and(|(t, c)| c == cell && now.duration_since(t) < DOUBLE_CLICK);
                    w.last_click = if double { None } else { Some((now, cell)) };
                    self.mouse(which, MouseKind::Down, double);
                } else {
                    w.button_down = false;
                    self.mouse(which, MouseKind::Up, false);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let Some(w) = self.win_mut(which) else { return };
                let rows = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => {
                        w.scroll_px += p.y;
                        let ch = w.cell_px().1 as f64;
                        let whole = (w.scroll_px / ch).trunc();
                        w.scroll_px -= whole * ch;
                        whole as f32
                    }
                };
                if rows != 0.0 {
                    self.mouse(which, MouseKind::Scroll(rows), false);
                }
            }
            WindowEvent::DroppedFile(path) if which == Which::Main => {
                input = self.app.dropped_file(path)
            }
            _ => input = false,
        }
        if input {
            self.window_requests(el);
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
        self.window_requests(el);
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
        popup: None,
        mods: ModifiersState::empty(),
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
