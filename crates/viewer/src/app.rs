//! The eframe adapter around the viewer session (#164): window creation,
//! decorations and the undecorated drag/resize regions, fonts and DPI, the
//! wake threads, input collection and capture, the search box and GUI
//! drawing, screenshots, the bench and evidence. Everything the session
//! decides (actions, reload, mail, canvas input, normal close and restart
//! exit) arrives as an event; the shell carries out the effects it returns.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use edn::Value;
use eframe::egui;
use egui::{FontFamily, FontId, ViewportCommand};
use scene::{Dims, Rect};

use crate::bench;
use crate::driver::{self, Input, Runner, ViewRecord};
use crate::evidence::Evidence;
use crate::gui::{Action, Gui};
use crate::real_input::{
    self, Action as RealAction, Capture, Frame, Modifiers, Pointer, StreamOwner, Wheel,
};
use crate::render_cache::RenderCache;
use crate::search_box::SearchBox;
use crate::shell_session::{
    Action as SessionAction, Canvas, Chrome, Effects, Event, Launch, Now, POLL_INTERVAL, Repaint,
    ViewerSession, WINDOW_MIN, window_size,
};
use crate::theme::Role;
use crate::{cli::Options, fonts, mail_watch, paint, signal, strings};
use crate::{theme, watch};

pub const WINDOW_TITLE: &str = "UML viewer — Rust";

/// `UML_VIEWER_TEST_DPI_SCALE` simulates a display scale in isolated runs
/// (port of `adapters/dpi.lua` `override`). Real displays need nothing:
/// winit reports each monitor's scale factor and egui re-rasterises.
pub fn dpi_override() -> Option<f32> {
    env_nonempty("UML_VIEWER_TEST_DPI_SCALE")
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|v| *v > 0.0)
}
/// Height of the undecorated window's drag strip, in points.
pub const DRAG_STRIP: f32 = 28.0;
/// Resize grip thickness, in points.
pub const RESIZE_GRIP: f32 = 6.0;

/// Window decoration decision (ADR 0008 / #134): decorated X11 when the
/// X11 preflight passes, otherwise an undecorated Wayland window with custom
/// drag and resize regions (winit's client decorations disconnect Weston).
/// macOS windows through AppKit, which always draws its own title bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    X11Decorated,
    WaylandUndecorated,
    NativeDecorated,
}

impl WindowMode {
    pub fn decorated(self) -> bool {
        self != WindowMode::WaylandUndecorated
    }
}

/// `force_undecorated` is the `UML_VIEWER_TEST_UNDECORATED` seam: drivers
/// exercise the libxkbcommon-x11 fallback on a machine that has X11.
pub fn mode_for(x11_available: bool, force_undecorated: bool) -> WindowMode {
    if x11_available && !force_undecorated {
        WindowMode::X11Decorated
    } else {
        WindowMode::WaylandUndecorated
    }
}

/// The decision for a declared host `os` (`std::env::consts::OS`): Windows
/// and macOS always get the system title bar (ADR 0010 story 10); only Linux
/// probes X11, so `x11_available` runs only there.
pub fn window_mode(
    os: &str,
    x11_available: impl FnOnce() -> bool,
    force_undecorated: bool,
) -> WindowMode {
    match os {
        "windows" | "macos" if !force_undecorated => WindowMode::NativeDecorated,
        "windows" | "macos" => WindowMode::WaylandUndecorated,
        _ => mode_for(x11_available(), force_undecorated),
    }
}

/// Side of a window control button, in points.
const CONTROL_W: f32 = 28.0;
const CONTROL_H: f32 = DRAG_STRIP - RESIZE_GRIP - 2.0;

#[derive(Debug, Clone, Copy)]
enum ChromeHit {
    Drag,
    Resize(egui::viewport::ResizeDirection),
    Control,
}

/// The undecorated window's own `[minimise, close]` buttons: at the right
/// of the drag strip, below the north grip and left of the east grip.
pub fn window_controls(screen: egui::Rect) -> [egui::Rect; 2] {
    let top = screen.min.y + RESIZE_GRIP + 1.0;
    let right = screen.max.x - RESIZE_GRIP - 2.0;
    let close = egui::Rect::from_min_max(
        egui::pos2(right - CONTROL_W, top),
        egui::pos2(right, top + CONTROL_H),
    );
    let minimise = close.translate(egui::vec2(-(CONTROL_W + 2.0), 0.0));
    [minimise, close]
}

/// X11 needs libxkbcommon-x11 at runtime; probe it without elevating.
pub fn x11_available() -> bool {
    if std::env::var("DISPLAY").ok().is_none_or(|d| d.is_empty()) {
        return false;
    }
    lib_present("libxkbcommon-x11.so.0")
}

fn lib_present(name: &str) -> bool {
    let paths = [
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/usr/lib",
    ];
    if paths.iter().any(|dir| Path::new(dir).join(name).exists()) {
        return true;
    }
    companion::command_stdout("ldconfig", &["-p"]).is_some_and(|out| out.contains(name))
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

struct Setup {
    project_root: PathBuf,
    /// The resolved diagram and the session snapshot, read once.
    launch: Launch,
    evidence: Option<Evidence>,
    test_seconds: Option<f64>,
    script: Option<driver::Script>,
    /// Protocol-1 output directory (#143); the bench owns the camera.
    bench: Option<PathBuf>,
    bench_only: Option<String>,
    /// Saved window/inspector chrome from a restart session.
    chrome: Chrome,
}

impl Setup {
    fn from_options(options: Options) -> Result<Setup, String> {
        let root = env_nonempty("UML_VIEWER_PROJECT_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let project_root = crate::project_files::canonical(&root);
        // The snapshot is read once: the launch keeps it for the restore.
        let (mut launch, chrome) = Launch::resolve(
            project_root.clone(),
            options.path.clone(),
            options.restart,
            options.backend,
        );
        launch.agent = options.agent;
        let evidence = env_nonempty("UML_VIEWER_EVIDENCE").map(|dir| {
            let dir = PathBuf::from(dir);
            let _ = std::fs::create_dir_all(&dir);
            Evidence::new(dir)
        });
        let test_seconds = env_nonempty("UML_VIEWER_TEST_SECONDS").and_then(|v| v.parse().ok());
        let script = match env_nonempty("UML_VIEWER_TEST_DRIVER") {
            Some(path) => Some(driver::Script::load(Path::new(&path))?),
            None => None,
        };
        Ok(Setup {
            project_root,
            launch,
            evidence,
            test_seconds,
            script,
            bench: env_nonempty("UML_VIEWER_BENCH").map(PathBuf::from),
            bench_only: env_nonempty("UML_VIEWER_BENCH_ONLY"),
            chrome,
        })
    }
}

/// The renderer environment a host `os` (`std::env::consts::OS`) defaults
/// to; a user value always wins. WSLg: d3d12 through Mesa and the GL wgpu
/// backend (also set by the launcher; this keeps direct launches on the
/// GPU). Windows: wgpu's D3D12 backend, which wgpu would otherwise rank
/// below Vulkan. macOS keeps wgpu's native Metal backend.
pub fn render_defaults(os: &str) -> &'static [(&'static str, &'static str)] {
    match os {
        "linux" => &[("GALLIUM_DRIVER", "d3d12"), ("WGPU_BACKEND", "gl")],
        "windows" => &[("WGPU_BACKEND", "dx12")],
        _ => &[],
    }
}

/// eframe entry point.
pub fn run(options: Options) -> ExitCode {
    for &(key, value) in render_defaults(std::env::consts::OS) {
        if std::env::var_os(key).is_none() {
            // SAFETY: called before any thread is spawned (eframe starts the
            // event loop after this point).
            unsafe { std::env::set_var(key, value) };
        }
    }
    signal::install();
    let setup = match Setup::from_options(options) {
        Ok(setup) => setup,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let force_undecorated = env_nonempty("UML_VIEWER_TEST_UNDECORATED").is_some();
    let mode = window_mode(std::env::consts::OS, x11_available, force_undecorated);
    let exit = Arc::new(AtomicU8::new(0));
    // A simulated scale also scales the window, so the canvas stays
    // 1500x920 points (Lua `display.window_size`: view units × ui).
    // 1500x920 is also the protocol-1 bench window (#143).
    let scale = dpi_override().unwrap_or(1.0);
    // A restart restores the saved window; a fresh launch keeps the default.
    let [width, height] = window_size(&setup.chrome, f64::from(scale));
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(WINDOW_TITLE)
        .with_inner_size([width as f32, height as f32])
        .with_min_inner_size([WINDOW_MIN[0] * scale, WINDOW_MIN[1] * scale])
        .with_decorations(mode.decorated());
    if !mode.decorated() {
        viewport = viewport.with_title(format!("{WINDOW_TITLE} (无装饰)"));
    }
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut native = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    #[cfg(target_os = "linux")]
    if mode == WindowMode::X11Decorated {
        native.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            builder.with_x11();
        }));
    }
    let window_mode = mode;
    let exit_handle = Arc::clone(&exit);
    let result = eframe::run_native(
        WINDOW_TITLE,
        native,
        Box::new(move |cc| {
            let app = Viewer::new(cc, setup, window_mode, exit_handle)?;
            Ok(Box::new(app))
        }),
    );
    if let Err(error) = result {
        eprintln!("UML viewer: window failed: {error}");
        return ExitCode::from(1);
    }
    ExitCode::from(exit.load(Ordering::SeqCst))
}

struct Viewer {
    /// View state, hot reload, mail and the companion link (#161).
    session: ViewerSession,
    project_root: PathBuf,
    evidence: Option<Evidence>,
    test_seconds: Option<f64>,
    driver: Option<Runner>,
    frames: u64,
    started: Instant,
    last_frame: Instant,
    frame_ms: f64,
    pending: Vec<Input>,
    show_performance: bool,
    exit: Arc<AtomicU8>,
    published: u64,
    render_shot: bool,
    shots_requested: Vec<String>,
    decorated: bool,
    driver_failed: bool,
    quit: bool,
    ctx: egui::Context,
    window_w: f64,
    window_h: f64,
    shots_sent: Vec<String>,
    /// Resolved CJK font and its source (`env`/`sarasa`/`system`).
    font: Option<(PathBuf, &'static str)>,
    search: SearchBox,
    /// Text and paste events for the search field, delivered to egui right
    /// before the field is drawn so they take egui's own input path.
    text_events: Vec<egui::Event>,
    /// Last pointer position of replayed input (view units); a wheel over the
    /// open search box belongs to it even though the canvas saw a leave.
    driver_pointer: Option<(f64, f64)>,
    bench: Option<bench::Bench>,
    bench_geo: Option<bench::Geo>,
    /// Adapter name, backend, driver, driver info (protocol-1 environment).
    adapter: [String; 4],
    /// Class/edge geometry, rebuilt only when the scene, selection, hover or
    /// LOD tier changes (#143).
    render_cache: RenderCache,
    /// Canvas text layouts across frames (#143).
    text_cache: paint::TextCache,
    /// Stops the idle wake thread when the viewer is dropped.
    wake_stop: Arc<AtomicBool>,
    /// Inspector sidebar, class card and source windows (#141).
    gui: Gui,
    /// Driver GUI events, one batch per frame through `raw_input_hook`.
    gui_batches: std::collections::VecDeque<Vec<egui::Event>>,
    /// Where replayed GUI pointer events are (screen points).
    gui_pointer: egui::Pos2,
    /// A batch reached egui this frame and the GUI has not drawn it yet.
    gui_inflight: bool,
    /// The change-only mailbox waker thread.
    waker: Option<mail_watch::Waker>,
    /// A window close request (`WM_CLOSE`: the title bar's close button, or
    /// another process posting it) takes the normal close: the window stays
    /// until the companion is released. This is Windows' polite termination,
    /// where SIGTERM does not exist; elsewhere eframe closes at once and
    /// `on_exit` asks the helper to stop (unchanged).
    close_request_is_normal_close: bool,
}

impl Drop for Viewer {
    fn drop(&mut self) {
        self.wake_stop.store(true, Ordering::SeqCst);
    }
}

/// Idle wake-ups (#143): a stat-only watcher repaints only when the diagram,
/// metrics or mailbox stamp changes, so an idle viewer does not redraw on the
/// poll cadence. The woken frame's `poll` does the actual reload. A pending
/// SIGTERM/SIGINT also wakes it: the signal handler only sets a flag, and an
/// idle viewer would otherwise not run a frame to act on it.
fn spawn_wake(ctx: egui::Context, diagram: PathBuf, root: PathBuf) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let spawned = std::thread::Builder::new()
        .name("uml-viewer-wake".into())
        .spawn(move || {
            let inbox = companion::mailbox::Mailbox::inbox(&root)
                .path()
                .to_path_buf();
            let mut wake = watch::Wake::new(&diagram, &root, &[&inbox]);
            while !flag.load(Ordering::SeqCst) {
                std::thread::sleep(POLL_INTERVAL);
                if wake.poll() || signal::terminated() {
                    ctx.request_repaint();
                }
            }
        });
    if let Err(error) = spawned {
        eprintln!("uml viewer: could not start the idle watcher: {error}");
    }
    stop
}

impl Viewer {
    fn new(
        cc: &eframe::CreationContext<'_>,
        setup: Setup,
        mode: WindowMode,
        exit: Arc<AtomicU8>,
    ) -> Result<Viewer, String> {
        let font = fonts::install(&cc.egui_ctx, &setup.project_root);
        if let Some(scale) = dpi_override() {
            cc.egui_ctx.set_zoom_factor(scale);
        }
        // Ctrl± / Ctrl0 zoom the canvas, not egui's UI scale.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let adapter = cc
            .wgpu_render_state
            .as_ref()
            .map(|state| {
                let info = state.adapter.get_info();
                [
                    info.name,
                    format!("{:?}", info.backend),
                    info.driver,
                    info.driver_info,
                ]
            })
            .unwrap_or_default();
        let renderer = cc.wgpu_render_state.as_ref().map(|state| {
            let info = state.adapter.get_info();
            format!(
                "renderer: {}\nbackend: {:?}\napi: {:?}\ndriver: {}\n",
                info.name, info.backend, info.device_type, info.driver
            )
        });
        // The first frames measure the window before the content rect is
        // known: start from the requested (or restored) size.
        let [start_w, start_h] =
            window_size(&setup.chrome, dpi_override().map(f64::from).unwrap_or(1.0));
        // The idle watcher takes its baseline before the first load, so a
        // diagram written meanwhile still wakes the viewer.
        let wake_stop = spawn_wake(
            cc.egui_ctx.clone(),
            setup.launch.diagram.clone(),
            setup.project_root.clone(),
        );
        if let Some(evidence) = &setup.evidence {
            if let Some(info) = &renderer {
                let _ = evidence.write_text("renderer.txt", info);
            }
            // The backend identity is the D3D12-versus-llvmpipe evidence.
            eprintln!(
                "UML viewer: {}",
                info_lines(renderer.as_deref().unwrap_or("renderer: unknown\n"))
            );
        }
        // The queue is consumed whether or not a companion backend was asked
        // for (LOVE's `mail` exists in both cases); only the helper process is
        // backend-specific and starts after the first scene, as in LOVE.
        let opened = ViewerSession::open_with_companion(setup.launch);
        let session = match opened {
            Ok(session) => session,
            Err(error) => {
                wake_stop.store(true, Ordering::SeqCst);
                return Err(error);
            }
        };
        let mut app = Viewer {
            session,
            project_root: setup.project_root.clone(),
            evidence: setup.evidence.clone(),
            test_seconds: setup.test_seconds,
            driver: setup.script.clone().map(Runner::new),
            frames: 0,
            started: Instant::now(),
            last_frame: Instant::now(),
            frame_ms: 0.0,
            pending: Vec::new(),
            show_performance: false,
            exit,
            published: 0,
            render_shot: false,
            shots_requested: Vec::new(),
            decorated: mode.decorated(),
            driver_failed: false,
            quit: false,
            ctx: cc.egui_ctx.clone(),
            window_w: start_w,
            window_h: start_h,
            shots_sent: Vec::new(),
            font,
            search: SearchBox::default(),
            text_events: Vec::new(),
            driver_pointer: None,
            bench: None,
            bench_geo: None,
            adapter,
            render_cache: RenderCache::default(),
            text_cache: paint::TextCache::default(),
            wake_stop,
            gui: Gui::default(),
            gui_batches: Default::default(),
            gui_pointer: egui::Pos2::ZERO,
            gui_inflight: false,
            waker: None,
            close_request_is_normal_close: cfg!(windows),
        };
        app.publish();
        // Mail must be noticed even when nothing else asks for a frame; the
        // waker requests one only when a watched file actually changed.
        let ctx = cc.egui_ctx.clone();
        app.waker = Some(mail_watch::spawn(
            setup.project_root.clone(),
            POLL_INTERVAL,
            move || ctx.request_repaint(),
        ));
        if let Some(width) = setup.chrome.inspector_w {
            app.gui.width = Some(width);
        }
        if let Some(out) = setup.bench.clone() {
            let (classes, _, _) = bench::scene_counts(&app.session.state().scene);
            let provenance = bench::Provenance::from_env(bench::load_one_minute())?;
            let runner = bench::Bench::new(
                out,
                setup.bench_only.clone(),
                app.adapter[0].clone(),
                classes,
                provenance,
                bench::load_one_minute,
            )?;
            app.bench = Some(runner);
            // The bench sets the unclamped protocol camera; no viewer fit.
            let dims = Dims::default();
            app.canvas(Canvas::HoldCamera, dims);
        }
        // Only now is the startup sure to succeed: a failed bench start must
        // not overwrite an existing identity.
        app.session.process(Event::Started, Now::read());
        Ok(app)
    }

    fn publish(&mut self) {
        if let Some(evidence) = &self.evidence
            && self.session.state().revision != self.published
        {
            if let Err(error) =
                evidence.publish(&self.session.state().scene, self.session.state().revision)
            {
                eprintln!("uml viewer: could not publish evidence: {error}");
            }
            self.published = self.session.state().revision;
        }
    }

    fn dims(&self, ctx: &egui::Context) -> Dims {
        let (control, shift) = ctx.input(|i| (i.modifiers.ctrl, i.modifiers.shift));
        let screen = ctx.content_rect();
        Dims {
            window_w: screen.width() as f64,
            window_h: screen.height() as f64,
            view_w: Some(self.gui.canvas_width(screen.width() as f64)),
            control,
            shift,
            ..Dims::default()
        }
    }

    /// Apply one input. Coordinates are view units (points from the canvas
    /// origin); what the search box and the GUI do not capture goes to the
    /// viewer session, exactly as real input does.
    fn apply_input(&mut self, input: Input, ctx: &egui::Context) {
        let mut dims = self.dims(ctx);
        match &input {
            Input::Click { x, y } | Input::DoubleClick { x, y } | Input::MouseMove { x, y } => {
                self.driver_pointer = Some((*x, *y))
            }
            Input::Drag { to, .. } => self.driver_pointer = Some((to[0], to[1])),
            _ => {}
        }
        let reload = matches!(&input, Input::Key { key, .. } if key == "r");
        let canvas = match input {
            Input::Click { x, y } => {
                if self.chrome_press(x, y) {
                    return;
                }
                Canvas::Click(x, y)
            }
            Input::DoubleClick { x, y } => {
                if self.chrome_press(x, y) {
                    return;
                }
                Canvas::DoublePress(x, y)
            }
            Input::MouseMove { x, y } => {
                if self.search.captures(x, y) {
                    Canvas::Leave
                } else if self.session.state().drag.is_some() {
                    Canvas::Drag(x, y)
                } else {
                    Canvas::Move(x, y)
                }
            }
            Input::Drag { from, to } => {
                if self.chrome_press(from[0], from[1]) {
                    return;
                }
                Canvas::DragTo { from, to }
            }
            Input::Wheel {
                dx,
                dy,
                ctrl,
                shift,
            } => {
                if self.gui.card_hovered() {
                    let rows = self.gui.card_rows(self.session.state());
                    // LOVE wheelmoved(0, -4) scrolls the card down four notches.
                    Canvas::ScrollCard {
                        rows,
                        delta: -dy,
                        view_h: self.gui.card.view_h,
                    }
                } else {
                    if self
                        .driver_pointer
                        .or(self.session.state().pointer)
                        .is_some_and(|(x, y)| self.search.captures(x, y))
                    {
                        return;
                    }
                    dims.control |= ctrl;
                    dims.shift |= shift;
                    Canvas::Wheel { dx, dy }
                }
            }
            Input::Key { key, ctrl, shift } => {
                dims.control |= ctrl;
                dims.shift |= shift;
                if key == "f" && dims.control {
                    self.search.open();
                    return;
                }
                if self.search.active {
                    // The open search box owns the keyboard (Lua: canvas
                    // keys only when not inspector.search_active()).
                    self.search_key(&key, &dims);
                    return;
                }
                Canvas::Key(key)
            }
            Input::Text(text) => {
                if self.search.active {
                    self.text_events.push(egui::Event::Text(text));
                } else {
                    // The focused egui field (proposal rename) takes it.
                    self.gui_batches.push_back(vec![egui::Event::Text(text)]);
                }
                return;
            }
            Input::Paste(text) => {
                if self.search.active {
                    self.text_events.push(egui::Event::Paste(text));
                } else {
                    self.gui_batches.push_back(vec![egui::Event::Paste(text)]);
                }
                return;
            }
        };
        self.canvas(canvas, dims);
        if reload {
            // `logic` runs before this input, so its next pass does the reload.
            ctx.request_repaint();
        }
    }

    /// Hand uncaptured canvas input to the viewer session.
    fn canvas(&mut self, input: Canvas, dims: Dims) {
        let effects = self
            .session
            .process(Event::Canvas(input, dims), Now::read());
        let ctx = self.ctx.clone();
        self.carry_out(effects, &ctx);
    }

    /// The window as the canvas geometry, for driver and bench cameras.
    fn window_dims(&self, w: f64, h: f64) -> Dims {
        Dims {
            window_w: w,
            window_h: h,
            ..Dims::default()
        }
    }

    /// A press on screen chrome or the open search box; true when consumed.
    /// A press elsewhere closes the search box (it loses focus) and goes on
    /// to the canvas.
    fn chrome_press(&mut self, x: f64, y: f64) -> bool {
        if self.search.active {
            if let Some(id) = self.search.row_at(x, y) {
                self.search.selection = Some(id.clone());
                self.locate(&id);
                return true;
            }
            if self.search.captures(x, y) {
                return true;
            }
            self.search.close();
        }
        // The sidebar, the class card and source windows are egui's.
        self.gui.captures(x, y, self.window_w)
    }

    fn search_key(&mut self, key: &str, dims: &Dims) {
        let results = self.search.results(self.session.state().document.as_ref());
        match key {
            "down" => self.search.step(true, &results),
            "up" => self.search.step(false, &results),
            "enter" => {
                if let Some(id) = self.search.target(&results) {
                    let action = Event::Action(SessionAction::Locate(id), dims.clone());
                    let effects = self.session.process(action, Now::read());
                    let ctx = self.ctx.clone();
                    self.carry_out(effects, &ctx);
                }
            }
            "escape" => self.search.close(),
            _ => {}
        }
    }

    /// Enter the target's namespace, select it, open its card and centre it.
    fn locate(&mut self, id: &Value) {
        let ctx = self.ctx.clone();
        let dims = self.dims(&ctx);
        let action = Event::Action(SessionAction::Locate(id.clone()), dims);
        let effects = self.session.process(action, Now::read());
        self.carry_out(effects, &ctx);
    }

    /// Draw the Ctrl+F search box and record its area and rows in view
    /// units, so driver and real pointer input see the same capture.
    fn draw_search(&mut self, ctx: &egui::Context) {
        if !self.search.active {
            return;
        }
        self.search.sync(self.session.state().revision);
        let origin = ctx.content_rect().min;
        let field = egui::Id::unique("uml-search-field");
        // Driver text/paste go through egui's own event path into the field.
        let events = std::mem::take(&mut self.text_events);
        if !events.is_empty() {
            ctx.input_mut(|i| i.events.extend(events));
        }
        let results = self.search.results(self.session.state().document.as_ref());
        let mut rows = Vec::new();
        let mut chosen = None;
        let response = egui::Area::new(egui::Id::unique("uml-search"))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 44.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(440.0);
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.search.query)
                            .id(field)
                            .hint_text(strings::SEARCH)
                            .font(FontId::new(theme::fonts::BODY, FontFamily::Monospace))
                            .desired_width(f32::INFINITY),
                    );
                    if self.search.focus_request {
                        edit.request_focus();
                        self.search.focus_request = false;
                    }
                    let target = self.search.target(&results);
                    for m in results.iter().take(12) {
                        let selected = target.as_ref() == Some(&m.id);
                        let row = ui.selectable_label(selected, &m.label);
                        if row.clicked() {
                            chosen = Some(m.id.clone());
                        }
                        rows.push((view_rect(row.rect, origin), m.id.clone()));
                    }
                    if results.is_empty() && !self.search.query.trim().is_empty() {
                        ui.weak(strings::SEARCH_EMPTY);
                    } else if !results.is_empty() {
                        ui.weak(strings::SEARCH_OPEN);
                    }
                });
            });
        self.search.area = Some(view_rect(response.response.rect, origin));
        self.search.rows = rows;
        if let Some(id) = chosen {
            self.search.selection = Some(id.clone());
            self.locate(&id);
        }
    }

    /// Apply what the inspector, card and source windows asked for.
    /// The viewer session applies them; a source window is GUI state.
    fn apply_actions(&mut self, actions: Vec<Action>, ctx: &egui::Context) {
        if actions.is_empty() {
            return;
        }
        for action in actions {
            let action = match action {
                Action::OpenSource(ident) => {
                    if let Err(reason) = self.gui.open_source(&ident, &self.project_root) {
                        let line = strings::SOURCE_NOT_OPENED.replace("%s", &reason);
                        let effects = self.session.process(Event::Notice(line), Now::read());
                        self.carry_out(effects, ctx);
                    }
                    continue;
                }
                Action::Choose(id) => SessionAction::Choose(id),
                Action::NewProposal => SessionAction::NewProposal,
                Action::Declutter => SessionAction::Declutter,
                Action::Regen => SessionAction::Regen,
                Action::Rename(id, name) => SessionAction::Rename(id, name),
                Action::Delete(id) => SessionAction::Delete(id),
                Action::CloseCard => SessionAction::CloseCard,
                Action::Locate(id) => SessionAction::Locate(id),
                Action::Mail(op) => SessionAction::Mail(op),
            };
            let dims = self.dims(ctx);
            let effects = self
                .session
                .process(Event::Action(action, dims), Now::read());
            self.carry_out(effects, ctx);
        }
        self.publish();
        // Every action redraws, a source window included.
        ctx.request_repaint();
    }

    /// Do what the viewer session asked for.
    fn carry_out(&mut self, effects: Effects, ctx: &egui::Context) {
        if effects.invalidate_gui {
            self.gui.invalidate();
        }
        match effects.repaint {
            Repaint::None => {}
            Repaint::Now => ctx.request_repaint(),
            Repaint::After(delay) => ctx.request_repaint_after(delay),
        }
        // The first close wins: a failed driver's exit code stays.
        if let Some(code) = effects.close
            && !self.quit
        {
            self.exit.store(code, Ordering::SeqCst);
            self.quit = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// The window chrome a session snapshot saves.
    fn chrome(&self) -> Chrome {
        Chrome {
            window_w: Some(self.window_w),
            window_h: Some(self.window_h),
            inspector_w: Some(self.gui.inspector_width()),
        }
    }

    /// The point a GUI step targets, in screen points.
    fn gui_target(&self, target: &driver::GuiTarget) -> Result<egui::Pos2, String> {
        let origin = self.ctx.content_rect().min;
        let (x, y) = match (&target.at, target.xy) {
            (Some(key), _) => {
                let r = self
                    .gui
                    .bounds
                    .get(key)
                    .ok_or_else(|| format!("missing GUI widget {key}"))?;
                match &target.prefix {
                    Some(prefix) => {
                        let w = paint::measure(&self.ctx, theme::fonts::CODE, prefix);
                        (r.x + w, r.y + r.h / 2.0)
                    }
                    None => (r.x + r.w / 2.0, r.y + r.h / 2.0),
                }
            }
            (None, Some([x, y])) => (x, y),
            (None, None) => return Err("GUI step without a target".into()),
        };
        Ok(egui::Pos2::new(
            origin.x + (x + target.offset[0]) as f32,
            origin.y + (y + target.offset[1]) as f32,
        ))
    }

    fn gui_button_event(&self, secondary: bool, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: self.gui_pointer,
            button: if secondary {
                egui::PointerButton::Secondary
            } else {
                egui::PointerButton::Primary
            },
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Font source, role sizes and glyph coverage of the UI table plus
    /// `UML_FONT_TEST_EXTRA` and an optional extra probe.
    fn check_fonts(&mut self, extra: Option<&str>) -> Result<(), String> {
        let source = self.font.as_ref().map(|(_, s)| *s).unwrap_or("none");
        if let Some(expect) = env_nonempty("UML_FONT_TEST_EXPECT")
            && expect != source
        {
            return Err(format!("font source {source} expected {expect}"));
        }
        let roles = [
            ("body", theme::fonts::BODY, 18.0),
            ("title", theme::fonts::TITLE, 18.0),
            ("badge", theme::fonts::BADGE, 9.0),
            ("level", theme::fonts::LEVEL, 11.0),
            ("marker", theme::fonts::MARKER, 12.0),
            ("code", theme::fonts::CODE, 13.0),
        ];
        let probe = strings::glyph_text()
            + &env_nonempty("UML_FONT_TEST_EXTRA").unwrap_or_default()
            + extra.unwrap_or_default();
        let mut seen = Vec::new();
        for c in probe.chars() {
            if (c as u32) > 32 && !c.is_whitespace() && !seen.contains(&c) {
                seen.push(c);
            }
        }
        let mut missing = Vec::new();
        for (role, size, expected) in roles {
            if (size - expected).abs() > 0.01 {
                return Err(format!("{role} font size {size}"));
            }
            let font = FontId::new(size, FontFamily::Monospace);
            self.ctx.fonts_mut(|f| {
                for &c in &seen {
                    if !f.has_glyph(&font, c) {
                        missing.push(format!("{role}:U+{:04X}", c as u32));
                    }
                }
            });
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!("missing glyphs {}", missing.join(" ")))
        }
    }

    /// A normal close: the session stops the helper without keeping it. The
    /// next pass's `logic` runs the checkpoint that closes once it settled.
    fn begin_normal_close(&mut self, ctx: &egui::Context) {
        let effects = self.session.process(Event::NormalClose, Now::read());
        self.carry_out(effects, ctx);
    }

    fn request_screenshot(&self, ctx: &egui::Context, name: &str) {
        let Some(evidence) = &self.evidence else {
            return;
        };
        let target = evidence.path(name);
        let ctx = ctx.clone();
        ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::ScreenshotCallback::new(
            move |image| {
                if let Err(error) = write_png(&target, &image) {
                    eprintln!("uml viewer: screenshot failed: {error}");
                }
            },
        )));
    }

    /// Undecorated fallback: no title bar, so the drag strip carries
    /// minimise and close. Close is the normal close a window-manager close
    /// takes (companion stop without keep, exit 0).
    fn draw_window_controls(&mut self, ctx: &egui::Context) {
        if self.decorated {
            return;
        }
        let screen = ctx.content_rect();
        let [minimise, close] = window_controls(screen);
        let quiet = |ui: &mut egui::Ui, rect: egui::Rect, label: &str, hot: Role| {
            let response = ui.interact(rect, ui.scope_id().with(label), egui::Sense::click());
            let painter = ui.painter();
            if response.hovered() {
                painter.rect_filled(rect, 3.0, crate::color::color(Role::Line));
            }
            let ink = if response.hovered() { hot } else { Role::Muted };
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(14.0),
                crate::color::color(ink),
            );
            response.clicked()
        };
        let (minimised, closed) = egui::Area::new(egui::Id::unique("uml-window-controls"))
            .fixed_pos(screen.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                (
                    quiet(ui, minimise, "—", Role::Ink),
                    quiet(ui, close, "×", Role::Danger),
                )
            })
            .inner;
        self.gui
            .record_bound("window-minimize", minimise, screen.min);
        self.gui.record_bound("window-close", close, screen.min);
        if minimised {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        }
        if closed && !self.quit {
            self.begin_normal_close(ctx);
            // `logic` already ran its checkpoint for this frame.
            ctx.request_repaint();
        }
    }

    /// Geometry remains host-owned; the same hit is reused by action execution.
    fn window_chrome_hit(
        &self,
        screen: egui::Rect,
        pointer: Option<egui::Pos2>,
    ) -> Option<ChromeHit> {
        if self.decorated || self.driver.is_some() {
            return None;
        }
        let pointer = pointer?;
        let direction = if pointer.y <= screen.min.y + RESIZE_GRIP {
            if pointer.x <= screen.min.x + RESIZE_GRIP * 3.0 {
                egui::viewport::ResizeDirection::NorthWest
            } else if pointer.x >= screen.max.x - RESIZE_GRIP * 3.0 {
                egui::viewport::ResizeDirection::NorthEast
            } else {
                egui::viewport::ResizeDirection::North
            }
        } else if pointer.y >= screen.max.y - RESIZE_GRIP {
            if pointer.x <= screen.min.x + RESIZE_GRIP * 3.0 {
                egui::viewport::ResizeDirection::SouthWest
            } else if pointer.x >= screen.max.x - RESIZE_GRIP * 3.0 {
                egui::viewport::ResizeDirection::SouthEast
            } else {
                egui::viewport::ResizeDirection::South
            }
        } else if pointer.x <= screen.min.x + RESIZE_GRIP {
            egui::viewport::ResizeDirection::West
        } else if pointer.x >= screen.max.x - RESIZE_GRIP {
            egui::viewport::ResizeDirection::East
        } else {
            if window_controls(screen).iter().any(|r| r.contains(pointer)) {
                // The minimise/close buttons take the press (egui sees it); the
                // canvas and the drag must not.
                return Some(ChromeHit::Control);
            }
            if pointer.y <= screen.min.y + DRAG_STRIP {
                return Some(ChromeHit::Drag);
            }
            return None;
        };
        Some(ChromeHit::Resize(direction))
    }

    fn window_chrome_input(ctx: &egui::Context, hit: ChromeHit, pressed: bool) {
        if !matches!(hit, ChromeHit::Control) && pressed {
            match hit {
                ChromeHit::Resize(direction) => {
                    ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
                }
                ChromeHit::Drag => ctx.send_viewport_cmd(ViewportCommand::StartDrag),
                ChromeHit::Control => unreachable!(),
            }
        }
    }

    fn collect_real_input(&mut self, ctx: &egui::Context) {
        let screen = ctx.content_rect();
        let (pos, pressed, released, double, down) = ctx.input(|i| {
            (
                i.pointer.hover_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_released(),
                i.pointer
                    .button_double_clicked(egui::PointerButton::Primary),
                i.pointer.primary_down(),
            )
        });
        let position = pos.map(|p| [p.x as f64, p.y as f64]);
        let origin = [screen.min.x as f64, screen.min.y as f64];
        let view_position = position.map(|[x, y]| [x - origin[0], y - origin[1]]);
        let over_search = view_position.is_some_and(|[x, y]| self.search.captures(x, y));
        let over_gui =
            view_position.is_some_and(|[x, y]| self.gui.captures(x, y, screen.width() as f64));
        // Geometry stays with the existing interface owners. Drag capture is
        // independent: an active canvas drag bypasses ordinary UI capture.
        let ordinary = over_search || over_gui;
        let chrome_hit = self.window_chrome_hit(screen, pos);
        let owner = if self.driver.is_some() {
            StreamOwner::Driver
        } else if self.bench.is_some() {
            StreamOwner::Bench
        } else {
            StreamOwner::Real
        };
        let (ctrl, shift) = ctx.input(|i| (i.modifiers.ctrl, i.modifiers.shift));
        let frame = Frame {
            owner,
            origin,
            size: [screen.width() as f64, screen.height() as f64],
            pointer: Pointer {
                position,
                pressed,
                double,
                released,
                down,
            },
            modifiers: Modifiers { ctrl, shift },
            wheel: ctx.input(|i| Wheel {
                zoom: i.zoom_delta() as f64,
                scroll: [
                    i.smooth_scroll_delta.x as f64,
                    i.smooth_scroll_delta.y as f64,
                ],
                over_search,
                over_gui,
                over_card: self.gui.card_hovered(),
            }),
            keys: ctx.input(|i| {
                i.events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::Key {
                            key, pressed: true, ..
                        } => Some(if *key == egui::Key::F3 {
                            real_input::Key::PerformanceToggle
                        } else {
                            match key_name(*key) {
                                "" => real_input::Key::Unknown,
                                name => real_input::Key::Named(name),
                            }
                        }),
                        _ => None,
                    })
                    .collect()
            }),
            text_edit_focused: ctx.text_edit_focused(),
            search_active: self.search.active,
            capture: Capture {
                ordinary,
                dragging: self.session.state().drag.is_some(),
                window_chrome: chrome_hit.is_some(),
            },
        };
        let converted = real_input::convert(&frame);
        [self.window_w, self.window_h] = converted.size;
        let mut dims = self.dims(ctx);
        dims.control = converted.modifiers.ctrl;
        dims.shift = converted.modifiers.shift;
        for action in converted.actions {
            self.execute_real_action(action, &dims, chrome_hit, pressed);
        }
    }

    /// A right press on the canvas selects what it hits, real or replayed, so
    /// the element menu opening on the release targets that element.
    fn secondary_press(&mut self, ctx: &egui::Context) {
        let (pressed, pos) = ctx.input(|i| {
            (
                i.pointer.button_pressed(egui::PointerButton::Secondary),
                i.pointer.interact_pos(),
            )
        });
        let Some(pos) = pos.filter(|_| pressed) else {
            return;
        };
        let origin = ctx.content_rect().min;
        let (x, y) = ((pos.x - origin.x) as f64, (pos.y - origin.y) as f64);
        if self.search.captures(x, y) || self.gui.captures(x, y, self.window_w) {
            return;
        }
        let dims = self.dims(ctx);
        self.canvas(Canvas::SecondaryPress(x, y), dims);
    }

    /// Execute in order; release observes state after the preceding actions.
    fn execute_real_action(
        &mut self,
        action: RealAction,
        dims: &Dims,
        chrome_hit: Option<ChromeHit>,
        pointer_pressed: bool,
    ) {
        let input = match action {
            RealAction::Key { key, ctrl, shift } => {
                self.pending.push(Input::Key {
                    key: key.into(),
                    ctrl,
                    shift,
                });
                return;
            }
            RealAction::TogglePerformance => {
                self.show_performance = !self.show_performance;
                return;
            }
            RealAction::Wheel {
                dx,
                dy,
                ctrl,
                shift,
            } => {
                self.pending.push(Input::Wheel {
                    dx,
                    dy,
                    ctrl,
                    shift,
                });
                return;
            }
            RealAction::Press(x, y) => Canvas::Press(x, y),
            RealAction::DoublePress(x, y) => Canvas::DoublePress(x, y),
            RealAction::Drag(x, y) => Canvas::Drag(x, y),
            RealAction::ChromePress(x, y) => {
                let ctx = self.ctx.clone();
                if let Some(hit) = chrome_hit {
                    Self::window_chrome_input(&ctx, hit, pointer_pressed);
                } else {
                    self.chrome_press(x, y);
                }
                return;
            }
            RealAction::Move(x, y) => Canvas::Move(x, y),
            RealAction::Leave => Canvas::Leave,
            RealAction::ReleaseCandidate(x, y) => {
                if self.session.state().drag.is_none() {
                    return;
                }
                Canvas::Release(x, y)
            }
        };
        self.canvas(input, dims.clone());
    }
}

impl Viewer {
    /// One protocol-1 frame: apply the trajectory camera (unclamped, world
    /// centre → top-left cam), keep repainting in interactive phases, and
    /// close with the verdict as the exit code once every phase is done.
    fn bench_frame(
        &mut self,
        ctx: &egui::Context,
        rect: egui::Rect,
        now: Instant,
        cpu_ms: Option<f64>,
    ) {
        let (w, h) = (rect.width() as f64, rect.height() as f64);
        if self.bench_geo.is_none() {
            let geo = bench::Geo::from_scene(&self.session.state().scene, w, h);
            let [name, backend, driver, info] = &self.adapter;
            let meta = bench::environment_meta(
                name,
                backend,
                driver,
                info,
                ctx.pixels_per_point() as f64,
                (w, h),
                &geo,
                bench::scene_counts(&self.session.state().scene),
            );
            eprintln!("UML viewer bench: {meta}");
            if let Some(runner) = &mut self.bench {
                runner.set_meta(meta);
            }
            self.bench_geo = Some(geo);
        }
        let geo = self.bench_geo.expect("set above");
        let Some(runner) = &mut self.bench else {
            return;
        };
        let step = runner.frame(&geo, now, cpu_ms);
        let passed = runner.passed();
        if let Some((cx, cy, zoom)) = step.camera {
            let dims = self.window_dims(w, h);
            self.canvas(Canvas::Camera { cx, cy, zoom }, dims);
        }
        if step.continuous {
            ctx.request_repaint();
        } else if let Some(seconds) = step.wake_s {
            ctx.request_repaint_after(Duration::from_secs_f64(seconds));
        }
        if step.finished {
            let code = if passed == Some(true) { 0 } else { 1 };
            self.exit.store(code, Ordering::SeqCst);
            self.quit = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
}

fn key_name(key: egui::Key) -> &'static str {
    match key {
        egui::Key::R => "r",
        egui::Key::P => "p",
        egui::Key::Escape => "escape",
        egui::Key::ArrowLeft => "left",
        egui::Key::ArrowRight => "right",
        egui::Key::ArrowUp => "up",
        egui::Key::ArrowDown => "down",
        egui::Key::Plus | egui::Key::Equals => "+",
        egui::Key::Minus => "-",
        egui::Key::Num0 => "0",
        egui::Key::F => "f",
        egui::Key::Enter => "enter",
        _ => "",
    }
}

/// An egui screen rect in view units (points from the canvas origin).
fn view_rect(r: egui::Rect, origin: egui::Pos2) -> Rect {
    Rect::new(
        (r.min.x - origin.x) as f64,
        (r.min.y - origin.y) as f64,
        r.width() as f64,
        r.height() as f64,
    )
}

/// EDN number: an integer when it is one (window sizes, scales).
fn edn_number(x: f64) -> Value {
    if (x - x.round()).abs() < 1e-3 {
        Value::Integer(x.round() as i64)
    } else {
        Value::Float(x)
    }
}

fn info_lines(info: &str) -> String {
    info.trim().replace('\n', "; ")
}

fn write_png(target: &Path, image: &egui::ColorImage) -> Result<(), String> {
    let [w, h] = image.size;
    let mut pixels = Vec::with_capacity(w * h * 4);
    for pixel in &image.pixels {
        pixels.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b(), pixel.a()]);
    }
    let buffer = image::RgbaImage::from_raw(w as u32, h as u32, pixels)
        .ok_or_else(|| "screenshot size mismatch".to_string())?;
    buffer.save(target).map_err(|e| e.to_string())
}

impl eframe::App for Viewer {
    fn on_exit(&mut self) {
        if let Some(waker) = &self.waker {
            waker.stop();
        }
        // A window close the frame loop did not see (WM quit): ask the
        // helper to stop without keeping the companion. It then releases the
        // session and ownership on its own.
        self.session.process(Event::WindowClosed, Now::read());
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.take_gui_batch(raw_input);
    }

    /// eframe runs this before each `ui`, and alone while the window is
    /// hidden (minimised or occluded) or its redraw is overdue. Reload, mail,
    /// the restart exit and the normal close live here, so a hidden viewer
    /// still acts on them; nothing here draws. The frame's input reaches the
    /// session after this, so `r` reloads on the next pass.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Hot reload: IR or metrics stamp change, or R.
        let effects = self.session.process(Event::Tick, Now::read());
        self.carry_out(effects, ctx);
        self.publish();
        // Mail is polled only when the waker saw a change (or at startup):
        // an unchanged project does no mailbox work and no repaint. `ui`
        // has not counted this frame yet, so these are the first two.
        let mail_due = self.frames < 2 || self.waker.as_ref().is_some_and(|waker| waker.take());
        if mail_due {
            let effects = self.session.process(Event::MailDue, Now::read());
            self.carry_out(effects, ctx);
            self.publish();
        }
        // A restart request saves the session and exits; a stopped helper
        // keeps being polled so the replacement viewer never binds a ghost.
        let chrome = self.chrome();
        let effects = self
            .session
            .process(Event::RestartExit(chrome), Now::read());
        self.carry_out(effects, ctx);

        // A termination signal is a normal close: release the companion and
        // exit through the same path a window close takes.
        if signal::terminated() && !self.quit {
            self.begin_normal_close(ctx);
        }
        // Settle a pending close before acknowledging another close event;
        // CancelClose and Close in one pass leave the native window open.
        let effects = self
            .session
            .process(Event::NormalCloseCheckpoint, Now::read());
        self.carry_out(effects, ctx);
        if self.close_request_is_normal_close
            && !self.quit
            && ctx.input(|i| i.viewport().close_requested())
        {
            self.begin_normal_close(ctx);
            if !self.quit {
                // The checkpoint below sends the close once it settled.
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            }
        }
        if let Some(seconds) = self.test_seconds
            && self.started.elapsed().as_secs_f64() >= seconds
        {
            // The test duration is the normal close: the companion is
            // stopped here and the normal-close checkpoint closes the window.
            self.begin_normal_close(ctx);
        }
        // A timed run keeps the poll cadence even while hidden, so its
        // duration still runs out.
        if self.bench.is_none() && self.test_seconds.is_some() {
            ctx.request_repaint_after(POLL_INTERVAL);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        self.frames += 1;
        let now = Instant::now();
        self.frame_ms = now.duration_since(self.last_frame).as_secs_f64() * 1000.0;
        self.last_frame = now;

        // Protocol-1 bench (#143): it owns the camera and the frame cadence.
        if self.bench.is_some() {
            let cpu_ms = frame.info().cpu_usage.map(|s| s as f64 * 1000.0);
            self.bench_frame(ctx, ui.max_rect(), now, cpu_ms);
        }

        // Driver replay: take the runner out so it can borrow the app as host.
        if self.bench.is_none() && self.driver.is_some() && !self.driver_failed {
            let mut runner = self.driver.take().expect("checked");
            let evidence = self.evidence.as_ref().map(|e| e.dir.clone());
            let project = self.project_root.clone();
            let outcome = runner.run(self.frames, evidence.as_deref(), &project, self);
            if let Err(message) = &outcome {
                eprintln!("Navigation acceptance failed: {message}");
                self.exit.store(1, Ordering::SeqCst);
                self.driver_failed = true;
                self.quit = true;
            }
            // A finished script's `:quit` is the verdict, exactly like the
            // Lua drivers' `love.event.quit(0)`: the companion's normal stop
            // must not turn a passed driver into a failure.
            let finished = runner.is_done() && !self.driver_failed;
            self.driver = Some(runner);
            if self.quit || finished {
                self.quit = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }

        // Real input (unless a driver owns the stream).
        self.collect_real_input(ctx);
        let pending = std::mem::take(&mut self.pending);
        for input in pending {
            self.apply_input(input, ctx);
        }
        self.secondary_press(ctx);

        let dims = self.dims(ctx);
        self.canvas(Canvas::Fit, dims);

        let full = ui.max_rect();
        let rect = full.with_max_x(full.min.x + self.gui.canvas_width(full.width() as f64) as f32);
        let hint = self.session.hint().map(str::to_owned);
        let status = self.session.status().map(str::to_owned);
        let performance = self
            .show_performance
            .then(|| strings::performance(fps(ctx), self.frame_ms));
        paint::paint(
            ctx,
            ui.painter(),
            self.session.state(),
            rect,
            hint.as_deref(),
            status.as_deref(),
            performance,
            self.show_performance,
            &mut self.render_cache,
            &mut self.text_cache,
        );
        self.draw_search(ctx);
        let actions = self.gui.draw(ctx, self.session.state(), self.frames);
        self.apply_actions(actions, ctx);
        self.draw_window_controls(ctx);
        self.gui_inflight = false;
        if self.driver.is_some() || !self.gui_batches.is_empty() {
            // Replayed input needs frames; a real session repaints on input.
            ctx.request_repaint();
        }

        // Evidence: a PNG at frame 4 plus driver-requested screenshots.
        if self.frames == 4 && self.evidence.is_some() && !self.render_shot {
            self.render_shot = true;
            self.request_screenshot(ctx, "render.png");
        }
        for name in self.shots_requested.clone() {
            let missing = self
                .evidence
                .as_ref()
                .is_some_and(|e| !e.path(&name).exists());
            if missing && !self.shots_sent.iter().any(|n| n == &name) {
                self.shots_sent.push(name.clone());
                self.request_screenshot(ctx, &name);
            }
        }

        // Idle: no cadence; input and the wake watcher repaint. Drivers and
        // pending screenshots keep the poll cadence; `logic` keeps it for a
        // timed run.
        let shots_pending = self.shots_requested.len() > self.shots_sent.len()
            || (self.evidence.is_some() && self.frames < 4);
        if self.bench.is_none() && (self.driver.is_some() || shots_pending) {
            ctx.request_repaint_after(POLL_INTERVAL);
        }
    }
}

fn fps(ctx: &egui::Context) -> u32 {
    let dt = ctx.input(|i| i.stable_dt).max(1e-6);
    (1.0 / dt).round() as u32
}

impl Viewer {
    /// Deliver one batch of replayed GUI events at the start of a frame, so
    /// egui handles them as real input (the Lua drivers fed imgui's IO).
    fn take_gui_batch(&mut self, raw_input: &mut egui::RawInput) {
        if let Some(batch) = self.gui_batches.pop_front() {
            raw_input.events.extend(batch);
            self.gui_inflight = true;
        }
    }
}

impl driver::Host for Viewer {
    fn inject(&mut self, input: Input) {
        self.pending.push(input);
    }

    fn assert_italic_glyphs(&mut self) -> Result<(), String> {
        // The probe runs through this window's live font atlas.
        paint::italic_glyph_probe(&self.ctx)
    }

    fn screenshot(&mut self, evidence: &str) -> bool {
        if !self.shots_requested.iter().any(|n| n == evidence) {
            self.shots_requested.push(evidence.to_string());
        }
        self.evidence
            .as_ref()
            .is_some_and(|e| e.path(evidence).exists())
    }

    fn set_view(&mut self, zoom: Option<f64>, center: Option<[f64; 2]>) {
        let dims = self.window_dims(self.window_w, self.window_h);
        self.canvas(Canvas::View { zoom, center }, dims);
    }

    fn quit(&mut self, code: u8) {
        self.exit.store(code, Ordering::SeqCst);
        self.quit = true;
    }

    fn view_record(&mut self, anchor: Option<[f64; 2]>) -> ViewRecord {
        let results = self.search.results(self.session.state().document.as_ref());
        ViewRecord {
            search: self.search.record(&results),
            ..self.session.view_record(anchor)
        }
    }

    fn is_fitted(&self) -> bool {
        self.session.is_fitted(&self.dims(&self.ctx))
    }

    fn is_blank(&self, x: f64, y: f64) -> bool {
        self.session.is_blank(x, y) && !self.gui.captures(x, y, self.window_w)
    }

    fn click_class(&mut self, id: &str, double: bool, secondary: bool) -> Result<(), String> {
        let (x, y) = self.session.class_point(id)?;
        if secondary {
            // A right click through egui's own pointer path: the canvas menu
            // opens on the release, where the user's right click lands.
            let origin = self.ctx.content_rect().min;
            let pos = egui::Pos2::new(origin.x + x as f32, origin.y + y as f32);
            self.gui_pointer = pos;
            self.gui_batches
                .push_back(vec![egui::Event::PointerMoved(pos)]);
            self.gui_batches
                .push_back(vec![self.gui_button_event(true, true)]);
            self.gui_batches
                .push_back(vec![self.gui_button_event(true, false)]);
            return Ok(());
        }
        self.pending.push(if double {
            Input::DoubleClick { x, y }
        } else {
            Input::Click { x, y }
        });
        Ok(())
    }

    fn click_declutter(&mut self) {
        // The control lives in the inspector: a real egui click on it.
        let target = driver::GuiTarget {
            at: Some("declutter".into()),
            xy: None,
            offset: [0.0, 0.0],
            prefix: None,
        };
        if let Err(error) = self.gui_click(&target, false) {
            eprintln!("uml viewer: {error}");
        }
    }

    fn assert_fonts(&mut self) -> Result<(), String> {
        self.check_fonts(None)
    }

    fn assert_fonts_with(&mut self, extra: Option<&str>) -> Result<(), String> {
        self.check_fonts(extra)
    }

    fn busy(&self) -> bool {
        // Until the GUI has drawn the last batch its effect is not visible.
        !self.gui_batches.is_empty() || self.gui_inflight
    }

    fn gui_point(&mut self, target: &driver::GuiTarget) -> Result<(), String> {
        let pos = self.gui_target(target)?;
        self.gui_pointer = pos;
        self.gui_batches
            .push_back(vec![egui::Event::PointerMoved(pos)]);
        Ok(())
    }

    fn gui_button(&mut self, secondary: bool, pressed: bool) {
        let event = self.gui_button_event(secondary, pressed);
        self.gui_batches.push_back(vec![event]);
    }

    fn gui_click(&mut self, target: &driver::GuiTarget, secondary: bool) -> Result<(), String> {
        let pos = self.gui_target(target)?;
        self.gui_pointer = pos;
        self.gui_batches
            .push_back(vec![egui::Event::PointerMoved(pos)]);
        let press = self.gui_button_event(secondary, true);
        let release = self.gui_button_event(secondary, false);
        self.gui_batches.push_back(vec![press]);
        self.gui_batches.push_back(vec![release]);
        Ok(())
    }

    fn gui_key(&mut self, key: &str, ctrl: bool, shift: bool) {
        let Some(code) =
            egui::Key::from_name(&key.to_uppercase()).or_else(|| egui::Key::from_name(key))
        else {
            eprintln!("uml viewer: unknown driver key {key}");
            return;
        };
        let modifiers = egui::Modifiers {
            ctrl,
            command: ctrl,
            shift,
            ..egui::Modifiers::NONE
        };
        let event = |pressed| egui::Event::Key {
            key: code,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        let mut batch = vec![egui::Event::ModifiersChanged(modifiers), event(true)];
        if ctrl && code == egui::Key::C {
            batch.push(egui::Event::Copy);
        }
        batch.push(event(false));
        batch.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        self.gui_batches.push_back(batch);
    }

    fn gui_record(&self) -> Value {
        let mut record = self.gui.record(self.session.state());
        record.assoc(
            Value::keyword("status"),
            self.session
                .status()
                .map_or(Value::Nil, |s| Value::String(s.into())),
        );
        record.assoc(
            Value::keyword("window"),
            Value::Map(vec![
                (Value::keyword("w"), edn_number(self.window_w)),
                (Value::keyword("h"), edn_number(self.window_h)),
            ]),
        );
        record.assoc(Value::keyword("mail"), self.session.mail_record());
        record.assoc(Value::keyword("session"), self.session_record());
        record
    }

    fn open_source(&mut self, lang: &str, file: &str, name: Option<&str>) -> Result<(), String> {
        self.gui
            .open_source_file(lang, file, name, &self.project_root)
    }

    fn resize_card(&mut self, w: f64, h: f64) {
        self.gui.card.force = Some((egui::vec2(w as f32, h as f32), 3));
    }

    fn widget_pixels(&self, key: &str) -> Option<[f64; 4]> {
        let r = self.gui.bounds.get(key)?;
        let origin = self.ctx.content_rect().min;
        let ppp = self.ctx.pixels_per_point() as f64;
        let (x, y) = (origin.x as f64 + r.x, origin.y as f64 + r.y);
        Some([x * ppp, y * ppp, (x + r.w) * ppp, (y + r.h) * ppp])
    }

    fn assert_text_fits(&mut self) -> Result<(), String> {
        paint::assert_text_fits(&self.ctx, self.session.state())
    }

    fn session_record(&self) -> Value {
        self.session.session_record(&self.chrome())
    }

    fn display_record(&self) -> Value {
        let screen = self.ctx.content_rect();
        let kw = Value::keyword;
        Value::Map(vec![
            (kw("ui"), edn_number(self.ctx.zoom_factor() as f64)),
            (
                kw("pixels-per-point"),
                edn_number(self.ctx.pixels_per_point() as f64),
            ),
            (
                kw("window"),
                Value::Map(vec![
                    (kw("w"), edn_number(screen.width() as f64)),
                    (kw("h"), edn_number(screen.height() as f64)),
                ]),
            ),
            (
                kw("fonts"),
                Value::Map(vec![
                    (kw("body"), edn_number(theme::fonts::BODY as f64)),
                    (kw("title"), edn_number(theme::fonts::TITLE as f64)),
                    (kw("badge"), edn_number(theme::fonts::BADGE as f64)),
                    (kw("level"), edn_number(theme::fonts::LEVEL as f64)),
                    (kw("marker"), edn_number(theme::fonts::MARKER as f64)),
                ]),
            ),
            (
                kw("font-source"),
                kw(self.font.as_ref().map(|(_, s)| *s).unwrap_or("none")),
            ),
            (kw("decorated"), Value::Bool(self.decorated)),
        ])
    }

    fn quality_line(&self, id: &str) -> Option<String> {
        self.session.quality_line(id)
    }

    fn resize_window(&mut self, w: f64, h: f64) {
        self.ctx
            .send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(w as f32, h as f32)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    fn viewer() -> Viewer {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("uml viewer app input {} {n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".uml-viewer")).unwrap();
        let session = ViewerSession::open(
            Launch {
                project_root: root.clone(),
                diagram: root.join("diagram.edn"),
                restart: false,
                backend: false,
                agent: None,
                snapshot: None,
            },
            || Box::new(crate::mail::Companion::mail_only()),
        )
        .unwrap();
        let ctx = egui::Context::default();
        Viewer {
            session,
            project_root: root,
            evidence: None,
            test_seconds: None,
            driver: None,
            frames: 0,
            started: Instant::now(),
            last_frame: Instant::now(),
            frame_ms: 0.0,
            pending: Vec::new(),
            show_performance: false,
            exit: Arc::new(AtomicU8::new(0)),
            published: 0,
            render_shot: false,
            shots_requested: Vec::new(),
            decorated: false,
            driver_failed: false,
            quit: false,
            ctx: ctx.clone(),
            window_w: 800.0,
            window_h: 600.0,
            shots_sent: Vec::new(),
            font: None,
            search: SearchBox::default(),
            text_events: Vec::new(),
            driver_pointer: None,
            bench: None,
            bench_geo: None,
            adapter: Default::default(),
            render_cache: RenderCache::default(),
            text_cache: paint::TextCache::default(),
            wake_stop: Arc::new(AtomicBool::new(false)),
            gui: Gui::default(),
            gui_batches: Default::default(),
            gui_pointer: egui::Pos2::ZERO,
            gui_inflight: false,
            waker: None,
            close_request_is_normal_close: false,
        }
    }

    /// One pass for a hidden window, as eframe runs it: no egui pass, only
    /// `App::logic`. Returns what it asked of the root viewport.
    fn hidden_pass(app: &mut Viewer) -> Vec<ViewportCommand> {
        let ctx = app.ctx.clone();
        let mut frame = eframe::Frame::_new_kittest();
        let mut output = ctx.run_logic(&egui::RawInput::default(), |ctx| {
            eframe::App::logic(app, ctx, &mut frame)
        });
        output
            .viewport_commands
            .remove(&egui::ViewportId::ROOT)
            .unwrap_or_default()
    }

    fn inbox(app: &Viewer) -> PathBuf {
        app.project_root.join(".uml-viewer/to-viewer.edn")
    }

    const DIAGRAM: &str = "{:title \"Hidden\" :packages [{:label \"P\" :classes [{:id :target :ns \"app.target\"}]}]}";

    #[test]
    fn a_hidden_window_reloads_and_takes_mail_without_a_frame() {
        let mut app = viewer();
        assert!(app.session.hint().is_some(), "no diagram yet");
        std::fs::write(app.project_root.join("diagram.edn"), DIAGRAM).unwrap();
        std::fs::write(
            inbox(&app),
            "{:next-id 2 :queue [{:id 1 :op :context :context :real}]}",
        )
        .unwrap();
        assert!(hidden_pass(&mut app).is_empty(), "nothing to close");
        assert!(app.session.hint().is_none(), "the diagram was reloaded");
        assert!(app.session.state().document.is_some());
        assert_eq!(1, app.session.state().mail_seen, "the mail was applied");
        assert_eq!(
            "{:next-id 2 :queue []}",
            std::fs::read_to_string(inbox(&app)).unwrap(),
            "and popped"
        );
        assert_eq!(0, app.frames, "no frame was counted");
    }

    #[test]
    fn a_hidden_window_takes_the_restart_exit() {
        let mut app = viewer();
        std::fs::write(
            inbox(&app),
            "{:next-id 2 :queue [{:id 1 :op :quit-for-restart}]}",
        )
        .unwrap();
        assert_eq!(vec![ViewportCommand::Close], hidden_pass(&mut app));
        assert!(app.quit);
        assert_eq!(0, app.exit.load(Ordering::SeqCst));
        assert!(
            app.project_root.join(".uml-viewer/session.edn").exists(),
            "the restart snapshot was saved"
        );
    }

    #[test]
    fn a_hidden_window_takes_the_normal_close() {
        let mut app = viewer();
        // The test duration is a normal close, like a termination signal.
        app.test_seconds = Some(0.0);
        assert_eq!(vec![ViewportCommand::Close], hidden_pass(&mut app));
        assert!(app.quit);
        assert!(
            !app.project_root.join(".uml-viewer/session.edn").exists(),
            "a normal close saves no snapshot"
        );
        assert!(hidden_pass(&mut app).is_empty(), "the close is sent once");
    }

    /// A pass whose input carries a window close request (`WM_CLOSE`, the
    /// title bar's close button), as eframe delivers it.
    fn close_request_pass(app: &mut Viewer) -> Vec<ViewportCommand> {
        let ctx = app.ctx.clone();
        let mut frame = eframe::Frame::_new_kittest();
        let mut input = egui::RawInput::default();
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .events
            .push(egui::ViewportEvent::Close);
        let mut output = ctx.run_logic(&input, |ctx| eframe::App::logic(app, ctx, &mut frame));
        output
            .viewport_commands
            .remove(&egui::ViewportId::ROOT)
            .unwrap_or_default()
    }

    /// A helper that settles only when told to; it records its stops.
    struct HeldLink(std::rc::Rc<std::cell::RefCell<(bool, Vec<crate::shell_session::Stop>)>>);

    impl crate::shell_session::CompanionLink for HeldLink {
        fn launch_status(&self) -> Option<String> {
            None
        }
        fn poll(&mut self) -> crate::shell_session::LinkPoll {
            Default::default()
        }
        fn stop(&mut self, stop: crate::shell_session::Stop) -> Result<(), String> {
            self.0.borrow_mut().1.push(stop);
            Ok(())
        }
        fn wake(&mut self, _id: i64) -> Result<(), String> {
            Ok(())
        }
        fn settled(&self) -> bool {
            self.0.borrow().0
        }
    }

    #[test]
    fn a_windows_close_request_waits_for_the_normal_close() {
        let mut app = viewer();
        let link = std::rc::Rc::new(std::cell::RefCell::new((false, Vec::new())));
        let held = link.clone();
        app.session = ViewerSession::open(
            Launch {
                project_root: app.project_root.clone(),
                diagram: app.project_root.join("diagram.edn"),
                restart: false,
                backend: false,
                agent: None,
                snapshot: None,
            },
            move || Box::new(HeldLink(held)),
        )
        .unwrap();
        app.close_request_is_normal_close = true;
        assert_eq!(
            vec![ViewportCommand::CancelClose],
            close_request_pass(&mut app),
            "the window stays while the companion is released"
        );
        assert!(!app.quit);
        assert_eq!(
            vec![crate::shell_session::Stop::ReleaseCompanion],
            link.borrow().1,
            "the normal close releases the companion"
        );
        link.borrow_mut().0 = true;
        // eframe can still deliver the window close event while the helper
        // settles. Never queue CancelClose and Close in that same pass: the
        // cancellation wins and the window needs a second WM_CLOSE.
        assert_eq!(vec![ViewportCommand::Close], close_request_pass(&mut app));
        assert_eq!(0, app.exit.load(Ordering::SeqCst));
    }

    #[test]
    fn a_close_request_without_a_helper_to_wait_for_closes_at_once() {
        let mut app = viewer();
        app.close_request_is_normal_close = true;
        assert_eq!(vec![ViewportCommand::Close], close_request_pass(&mut app));
        assert!(app.quit);
    }

    #[test]
    fn elsewhere_the_window_manager_close_is_left_to_eframe() {
        let mut app = viewer();
        app.close_request_is_normal_close = false;
        assert!(close_request_pass(&mut app).is_empty());
        assert!(!app.quit);
    }

    #[test]
    fn a_hidden_timed_run_keeps_its_cadence() {
        // A new context owes one pass; the startup passes serve it.
        let asks_again = |app: &mut Viewer| {
            for _ in 0..3 {
                assert!(hidden_pass(app).is_empty(), "nothing to close");
            }
            app.ctx.has_requested_repaint()
        };
        assert!(!asks_again(&mut viewer()), "an idle viewer waits");
        let mut app = viewer();
        app.test_seconds = Some(3600.0);
        assert!(asks_again(&mut app), "a timed run asks for its next pass");
    }

    #[test]
    fn the_r_key_requests_the_logic_pass_that_reloads() {
        let mut app = viewer();
        let ctx = app.ctx.clone();
        for _ in 0..3 {
            let _ = ctx.run_logic(&egui::RawInput::default(), |_| {});
        }
        assert!(!ctx.has_requested_repaint(), "an idle viewer waits");
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| {
            app.apply_input(
                Input::Key {
                    key: "r".into(),
                    ctrl: false,
                    shift: false,
                },
                ctx,
            );
        });
        assert!(
            ctx.has_requested_repaint(),
            "the next logic pass consumes the refresh"
        );
    }

    #[test]
    fn committed_text_and_paste_follow_the_current_text_focus() {
        for search_active in [false, true] {
            let mut app = viewer();
            app.search.active = search_active;
            let ctx = app.ctx.clone();
            app.apply_input(Input::Text("中文".into()), &ctx);
            app.apply_input(Input::Paste(" pasted ".into()), &ctx);
            let expected = [
                egui::Event::Text("中文".into()),
                egui::Event::Paste(" pasted ".into()),
            ];
            if search_active {
                assert_eq!(app.text_events, expected);
                assert!(app.gui_batches.is_empty());
            } else {
                assert!(app.text_events.is_empty());
                assert_eq!(
                    app.gui_batches
                        .iter()
                        .flatten()
                        .cloned()
                        .collect::<Vec<_>>(),
                    expected
                );
            }
        }
    }
}
