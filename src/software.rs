//! egui -> CPU rasterizer -> retained bitmap -> Win32 GDI. No graphics API context.
use crate::{
    repaint::{Repaint, SceneCache},
    ui::Desktop,
};
use anyhow::Result;
use egui_software_backend::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
use std::{
    num::NonZeroU32,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy, OwnedDisplayHandle},
    window::{Window, WindowId},
};

enum Event {
    Repaint { when: Instant, pass: u64 },
    Access(egui_winit::accesskit_winit::Event),
    DiagnosticExit,
    DiagnosticVisibility(bool),
    DiagnosticSettings,
}
impl From<egui_winit::accesskit_winit::Event> for Event {
    fn from(event: egui_winit::accesskit_winit::Event) -> Self {
        Self::Access(event)
    }
}

struct Canvas {
    surface: softbuffer::Surface<OwnedDisplayHandle, Rc<Window>>,
    _context: softbuffer::Context<OwnedDisplayHandle>,
    window: Rc<Window>,
    input: egui_winit::State,
    desktop: Desktop,
    renderer: EguiSoftwareRender,
    cache: SceneCache,
    pixels: Vec<[u8; 4]>,
    bitmap: Vec<u32>,
    size: [u32; 2],
    occluded: bool,
    needs_exposure: bool,
}
impl Canvas {
    fn visible(&self) -> bool {
        let size = self.window.inner_size();
        !self.occluded
            && self.window.is_visible() != Some(false)
            && self.window.is_minimized() != Some(true)
            && size.width > 0
            && size.height > 0
    }
}
#[derive(Default)]
struct Counters {
    ui: AtomicU64,
    raster: AtomicU64,
    presents: AtomicU64,
    cache_presents: AtomicU64,
    equal: AtomicU64,
    wakeups: AtomicU64,
    frame_micros: AtomicU64,
    max_frame_micros: AtomicU64,
    repaint_events: AtomicU64,
    input_events: AtomicU64,
    visible: AtomicBool,
    phase: AtomicU64,
}

struct Host {
    ctx: egui::Context,
    proxy: EventLoopProxy<Event>,
    canvas: Option<Canvas>,
    repaint: Repaint,
    counters: Arc<Counters>,
    error: Option<String>,
    next_frame: Instant,
    frame_interval: Duration,
    urgent: bool,
    fixture: bool,
}
impl Host {
    fn create(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let icon = crate::branding::window_icon();
        let icon = winit::window::Icon::from_rgba(icon.rgba, icon.width, icon.height)?;
        let window = Rc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("StartupManager")
                    .with_visible(false)
                    .with_active(!self.fixture)
                    .with_inner_size(LogicalSize::new(1120., 760.))
                    .with_min_inner_size(LogicalSize::new(900., 620.))
                    .with_window_icon(Some(icon.clone())),
            )?,
        );
        // winit's generic window icon sets ICON_SMALL; set ICON_BIG for taskbar/Alt-Tab.
        use winit::platform::windows::WindowExtWindows;
        window.set_taskbar_icon(Some(icon));
        let context = softbuffer::Context::new(event_loop.owned_display_handle())
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let surface = softbuffer::Surface::new(&context, window.clone())
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let mut input = egui_winit::State::new(
            self.ctx.clone(),
            egui::ViewportId::ROOT,
            event_loop,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(8192),
        );
        input.init_accesskit(event_loop, &window, self.proxy.clone());
        if self.fixture {
            // The quiet diagnostic must not steal focus or consume unrelated mouse input.
            window
                .set_cursor_hittest(false)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        self.frame_interval = monitor_interval(&window);
        window.set_visible(true);
        let mut desktop = if self.fixture {
            Desktop::preview(&self.ctx, "startup")?
        } else {
            Desktop::new(&self.ctx)
        };
        if self.fixture {
            desktop.use_live_preview_clock();
        }
        self.canvas = Some(Canvas {
            surface,
            _context: context,
            window,
            input,
            desktop,
            renderer: EguiSoftwareRender::new(ColorFieldOrder::Bgra),
            cache: SceneCache::default(),
            pixels: vec![],
            bitmap: vec![],
            size: [0, 0],
            occluded: false,
            needs_exposure: true,
        });
        if !self.fixture {
            crate::updater::cleanup_pending();
        }
        self.repaint.invalidate();
        Ok(())
    }
    fn redraw(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let Some(canvas) = &mut self.canvas else {
            return Ok(());
        };
        if !canvas.visible() {
            self.repaint.queued = false;
            return Ok(());
        }
        let now = Instant::now();
        let requested = self.repaint.queued;
        self.repaint.queued = false;
        let size = canvas.window.inner_size();
        let size = [size.width, size.height];
        let mut changed = false;
        if (self.repaint.needs_ui(now) && (self.urgent || now >= self.next_frame))
            || canvas.bitmap.is_empty()
            || size != canvas.size
        {
            self.urgent = false;
            self.next_frame = now + self.frame_interval;
            self.repaint.begin_frame(now);
            let info = canvas
                .input
                .egui_input_mut()
                .viewports
                .entry(egui::ViewportId::ROOT)
                .or_default();
            egui_winit::update_viewport_info(info, &self.ctx, &canvas.window, false);
            let input = canvas.input.take_egui_input(&canvas.window);
            let output = self.ctx.run(input, |ctx| canvas.desktop.update(ctx));
            self.counters.ui.fetch_add(1, Ordering::Relaxed);
            canvas
                .input
                .handle_platform_output(&canvas.window, output.platform_output);
            if let Some(viewport) = output.viewport_output.get(&egui::ViewportId::ROOT) {
                if viewport
                    .commands
                    .iter()
                    .any(|command| matches!(command, egui::ViewportCommand::Close))
                {
                    event_loop.exit();
                    return Ok(());
                }
                let mut info = egui::ViewportInfo::default();
                let mut actions = vec![];
                egui_winit::process_viewport_commands(
                    &self.ctx,
                    &mut info,
                    viewport.commands.clone(),
                    &canvas.window,
                    &mut actions,
                );
                if let Some(when) = Instant::now().checked_add(viewport.repaint_delay) {
                    self.repaint.request_at(when);
                }
            }
            changed = canvas.cache.changed(
                &output.shapes,
                size,
                output.pixels_per_point,
                !output.textures_delta.is_empty(),
            );
            if changed {
                canvas
                    .cache
                    .store(output.shapes, size, output.pixels_per_point);
                let primitives = self
                    .ctx
                    .tessellate(canvas.cache.shapes().to_vec(), output.pixels_per_point);
                let count = size[0] as usize * size[1] as usize;
                canvas.pixels.resize(count, [0, 0, 0, 255]);
                canvas.renderer.render(
                    &mut BufferMutRef::new(&mut canvas.pixels, size[0] as usize, size[1] as usize),
                    &primitives,
                    &output.textures_delta,
                    output.pixels_per_point,
                );
                canvas.bitmap.resize(count, 0);
                for (target, pixel) in canvas.bitmap.iter_mut().zip(&canvas.pixels) {
                    *target = u32::from_le_bytes([pixel[0], pixel[1], pixel[2], 0]);
                }
                self.counters.raster.fetch_add(1, Ordering::Relaxed);
            } else {
                self.counters.equal.fetch_add(1, Ordering::Relaxed);
            }
        }
        // OS exposure needs the retained bitmap. Unchanged input does not need a blit.
        if changed || !requested || canvas.needs_exposure {
            if canvas.size != size {
                canvas
                    .surface
                    .resize(
                        NonZeroU32::new(size[0]).unwrap(),
                        NonZeroU32::new(size[1]).unwrap(),
                    )
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                canvas.size = size;
            }
            let mut buffer = canvas
                .surface
                .buffer_mut()
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            buffer.copy_from_slice(&canvas.bitmap);
            buffer
                .present()
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            canvas.needs_exposure = false;
            self.counters.presents.fetch_add(1, Ordering::Relaxed);
            if !changed {
                self.counters.cache_presents.fetch_add(1, Ordering::Relaxed);
            }
        }
        let micros = now.elapsed().as_micros() as u64;
        self.counters
            .frame_micros
            .fetch_add(micros, Ordering::Relaxed);
        self.counters
            .max_frame_micros
            .fetch_max(micros, Ordering::Relaxed);
        Ok(())
    }
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.error = Some(format!("{error:#}"));
        event_loop.exit();
    }
}
impl ApplicationHandler<Event> for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.canvas.is_none()
            && let Err(error) = self.create(event_loop)
        {
            self.fail(event_loop, error);
        }
    }
    fn new_events(&mut self, _: &ActiveEventLoop, _: winit::event::StartCause) {
        self.counters.wakeups.fetch_add(1, Ordering::Relaxed);
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        // Drain bounded worker results without constructing a hidden UI. This also
        // keeps downloads/progress and a user-requested updater exit from blocking.
        if let Some(canvas) = &mut self.canvas {
            if canvas.desktop.receive_background(&self.ctx) {
                self.repaint.invalidate();
            }
            if canvas.desktop.exit_requested() {
                event_loop.exit();
                return;
            }
        }
        match event {
            Event::Repaint { when, pass } => {
                self.counters.repaint_events.fetch_add(1, Ordering::Relaxed);
                let current = self.ctx.cumulative_pass_nr();
                if current == pass || current == pass + 1 {
                    self.repaint.request_at(when);
                }
            }
            Event::Access(event) => {
                if let Some(canvas) = &mut self.canvas {
                    match event.window_event {
                        egui_winit::accesskit_winit::WindowEvent::InitialTreeRequested => {
                            self.ctx.enable_accesskit();
                            self.repaint.invalidate();
                        }
                        egui_winit::accesskit_winit::WindowEvent::ActionRequested(request) => {
                            canvas.input.on_accesskit_action_request(request);
                            self.repaint.invalidate();
                        }
                        egui_winit::accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
                    }
                }
            }
            Event::DiagnosticExit => event_loop.exit(),
            Event::DiagnosticVisibility(visible) => {
                if let Some(canvas) = &mut self.canvas {
                    canvas.window.set_visible(visible);
                    canvas.occluded = false;
                    self.repaint.queued = false;
                    self.counters
                        .phase
                        .store(if visible { 3 } else { 2 }, Ordering::Relaxed);
                    if visible {
                        canvas.needs_exposure = true;
                        self.repaint.invalidate();
                    }
                }
            }
            Event::DiagnosticSettings => {
                if let Some(canvas) = &mut self.canvas {
                    canvas.desktop.diagnostic_settings();
                    self.counters.phase.store(1, Ordering::Relaxed);
                    self.repaint.invalidate();
                }
            }
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(canvas) = &mut self.canvas else {
            return;
        };
        if canvas.window.id() != id {
            return;
        }
        let response = canvas.input.on_window_event(&canvas.window, &event);
        if matches!(
            event,
            WindowEvent::CursorMoved { .. }
                | WindowEvent::MouseInput { .. }
                | WindowEvent::KeyboardInput { .. }
                | WindowEvent::MouseWheel { .. }
        ) {
            self.counters.input_events.fetch_add(1, Ordering::Relaxed);
        }
        match event {
            WindowEvent::CloseRequested => {
                if canvas.desktop.request_close() {
                    event_loop.exit();
                } else {
                    self.repaint.invalidate();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.redraw(event_loop) {
                    self.fail(event_loop, error);
                }
            }
            WindowEvent::Occluded(occluded) => {
                canvas.occluded = occluded;
                if !occluded {
                    canvas.needs_exposure = true;
                    self.repaint.invalidate();
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                canvas.needs_exposure = true;
                self.urgent = true;
                self.repaint.invalidate()
            }
            WindowEvent::Moved(_) => {
                self.frame_interval = monitor_interval(&canvas.window);
                if response.repaint {
                    self.repaint.invalidate();
                }
            }
            WindowEvent::MouseInput { .. }
            | WindowEvent::KeyboardInput { .. }
            | WindowEvent::Ime(_) => {
                self.urgent = true;
                if response.repaint {
                    self.repaint.invalidate();
                }
            }
            _ => {
                if response.repaint {
                    self.repaint.invalidate();
                }
            }
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let visible = self.canvas.as_ref().is_some_and(Canvas::visible);
        self.counters.visible.store(visible, Ordering::Relaxed);
        let now = Instant::now();
        let ready = self.urgent || now >= self.next_frame;
        if ready
            && self.repaint.queue(now, visible)
            && let Some(canvas) = &self.canvas
        {
            canvas.window.request_redraw();
        }
        // Never poll. Pending redraws are dispatched by winit before waiting.
        event_loop.set_control_flow(if visible {
            if self.repaint.needs_ui(now) && !ready {
                ControlFlow::WaitUntil(self.next_frame)
            } else {
                self.repaint
                    .deadline
                    .filter(|when| *when > now)
                    .map_or(ControlFlow::Wait, ControlFlow::WaitUntil)
            }
        } else {
            ControlFlow::Wait
        });
    }
    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.ctx.set_request_repaint_callback(|_| {});
        self.canvas = None;
    }
}

pub fn run(diagnostic_path: Option<PathBuf>, duration: Option<u64>) -> Result<()> {
    run_internal(diagnostic_path, duration, false)
}
pub fn verify(path: PathBuf) -> Result<()> {
    run_internal(Some(path), Some(36), true)
}
fn run_internal(
    diagnostic_path: Option<PathBuf>,
    duration: Option<u64>,
    fixture: bool,
) -> Result<()> {
    let event_loop = EventLoop::<Event>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let ctx = egui::Context::default();
    let repaint_proxy = proxy.clone();
    ctx.set_request_repaint_callback(move |info| {
        if let Some(when) = Instant::now().checked_add(info.delay) {
            let _ = repaint_proxy.send_event(Event::Repaint {
                when,
                pass: info.current_cumulative_pass_nr,
            });
        }
    });
    let counters = Arc::new(Counters::default());
    let diagnostics = diagnostic_path
        .map(|path| Diagnostics::start(path, counters.clone(), proxy.clone(), duration, fixture));
    let mut host = Host {
        ctx,
        proxy,
        canvas: None,
        repaint: Repaint::default(),
        counters,
        error: None,
        next_frame: Instant::now(),
        frame_interval: Duration::from_secs_f64(1. / 60.),
        urgent: false,
        fixture,
    };
    event_loop.run_app(&mut host)?;
    drop(diagnostics);
    if let Some(error) = host.error {
        anyhow::bail!(error);
    }
    Ok(())
}
fn monitor_interval(window: &Window) -> Duration {
    let hz = window
        .current_monitor()
        .and_then(|monitor| monitor.refresh_rate_millihertz())
        .map(|rate| rate as f64 / 1000.)
        .unwrap_or(60.)
        .max(1.);
    Duration::from_secs_f64(1. / hz)
}

struct Diagnostics {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Diagnostics {
    fn start(
        path: PathBuf,
        counters: Arc<Counters>,
        proxy: EventLoopProxy<Event>,
        duration: Option<u64>,
        lifecycle: bool,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = std::thread::spawn(move || {
            let start = Instant::now();
            let mut previous_cpu = process_cpu();
            let mut previous_time = Instant::now();
            let mut previous_ui = 0;
            let mut previous_raster = 0;
            let mut previous_presents = 0;
            let mut samples = vec![];
            let mut quiet_samples = 0;
            let mut lifecycle_phase = 0;
            let logical_processors = std::thread::available_parallelism().map_or(1, usize::from);
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let now = Instant::now();
                let cpu = process_cpu();
                let elapsed = now.duration_since(previous_time).as_secs_f64();
                let ui = counters.ui.load(Ordering::Relaxed);
                let raster = counters.raster.load(Ordering::Relaxed);
                let presents = counters.presents.load(Ordering::Relaxed);
                let one_core_percent = cpu
                    .zip(previous_cpu)
                    .map(|(after, before)| 100. * (after - before) / elapsed);
                samples.push(serde_json::json!({"elapsed_seconds":start.elapsed().as_secs_f64(),"phase":counters.phase.load(Ordering::Relaxed),"visible":counters.visible.load(Ordering::Relaxed),"one_core_percent":one_core_percent,"machine_percent":one_core_percent.map(|value|value/logical_processors as f64),"ui_passes":ui-previous_ui,"rasterizations":raster-previous_raster,"presentations":presents-previous_presents}));
                let quiet =
                    ui == previous_ui && raster == previous_raster && presents == previous_presents;
                previous_cpu = cpu;
                previous_time = now;
                previous_ui = ui;
                previous_raster = raster;
                previous_presents = presents;
                let report = serde_json::json!({"backend":"CPU / Win32 GDI","executable":std::env::current_exe().ok(),"pid":std::process::id(),"release_build":!cfg!(debug_assertions),"fixture":lifecycle,"ui_passes":ui,"rasterizations":raster,"presentations":presents,"unchanged_scenes":counters.equal.load(Ordering::Relaxed),"cache_presentations":counters.cache_presents.load(Ordering::Relaxed),"event_loop_wakeups":counters.wakeups.load(Ordering::Relaxed),"repaint_events":counters.repaint_events.load(Ordering::Relaxed),"input_events":counters.input_events.load(Ordering::Relaxed),"max_draw_ms":counters.max_frame_micros.load(Ordering::Relaxed) as f64/1000.,"samples":samples});
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
                    let _ = std::fs::write(&path, bytes);
                }
                if lifecycle {
                    if quiet && counters.phase.load(Ordering::Relaxed) == lifecycle_phase {
                        quiet_samples += 1;
                    } else {
                        quiet_samples = 0;
                    }
                    if quiet_samples >= 5 {
                        quiet_samples = 0;
                        match lifecycle_phase {
                            0 => {
                                let _ = proxy.send_event(Event::DiagnosticSettings);
                            }
                            1 => {
                                let _ = proxy.send_event(Event::DiagnosticVisibility(false));
                            }
                            2 => {
                                let _ = proxy.send_event(Event::DiagnosticVisibility(true));
                            }
                            _ => {
                                let _ = proxy.send_event(Event::DiagnosticExit);
                                break;
                            }
                        }
                        lifecycle_phase += 1;
                    }
                }
                if duration.is_some_and(|seconds| start.elapsed().as_secs() >= seconds) {
                    let _ = proxy.send_event(Event::DiagnosticExit);
                    break;
                }
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Diagnostics {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn process_cpu() -> Option<f64> {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentProcess, GetProcessTimes},
    };
    let mut create = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut create,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return None;
    }
    let ticks = |time: FILETIME| ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64;
    Some((ticks(kernel) + ticks(user)) as f64 / 10_000_000.)
}
