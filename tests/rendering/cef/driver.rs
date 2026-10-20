//! Real CEF process driver. Scene assertions live in scenario.rs.
#[path = "../../../crates/ipp-render-gl/examples/smoke/egl.rs"]
#[allow(dead_code)]
mod egl;
mod render;
mod scenario;
mod state_sentinels;

use cef::{args::Args, rc::Rc, *};
use scenario::FrameState;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct State {
    browser: Option<Browser>,
    renderer: Option<render::Renderer>,
    url: String,
    size: [i32; 2],
    painted_size: [i32; 2],
    accelerated: u32,
    cpu: u32,
    loaded: u32,
    loading: bool,
    load_errors: u32,
    closed: bool,
    failure: Option<String>,
}

thread_local! {
    static STATE: RefCell<State> = const {
        RefCell::new(State {
            browser: None,
            renderer: None,
            url: String::new(),
            size: [640, 400],
            painted_size: [0, 0],
            accelerated: 0,
            cpu: 0,
            loaded: 0,
            load_errors: 0,
            loading: false,
            closed: false,
            failure: None,
        })
    };
}

wrap_render_handler! {
    struct PaintHandler {}

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                let size = STATE.with_borrow(|s| s.size);
                rect.width = size[0];
                rect.height = size[1];
            }
        }

        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            _kind: PaintElementType,
            _dirty: Option<&[Rect]>,
            _pixels: *const u8,
            _width: i32,
            _height: i32,
        ) {
            STATE.with_borrow_mut(|s| {
                s.cpu += 1;
                s.failure =
                    Some("accelerated unavailable: CEF requested CPU OnPaint; no fallback".into());
            });
        }

        fn on_accelerated_paint(
            &self,
            _browser: Option<&mut Browser>,
            kind: PaintElementType,
            _dirty: Option<&[Rect]>,
            info: Option<&AcceleratedPaintInfo>,
        ) {
            if kind != PaintElementType::VIEW {
                return;
            }
            STATE.with_borrow_mut(|s| {
                let Some(info) = info else {
                    s.failure = Some("CEF omitted accelerated frame".into());
                    return;
                };
                if s.failure.is_some() {
                    return;
                }
                eprintln!(
                    "CEF shared image: {:?}, {} planes, modifier {:#x}; requested={:?}, coded={}x{}, timestamp={}, capture_counter={} (present={}), source={}x{} (present={})",
                    info.format,
                    info.plane_count,
                    info.modifier,
                    s.size,
                    info.extra.coded_size.width,
                    info.extra.coded_size.height,
                    info.extra.timestamp,
                    info.extra.capture_counter,
                    info.extra.has_capture_counter,
                    info.extra.source_size.width,
                    info.extra.source_size.height,
                    info.extra.has_source_size,
                );
                let Some(renderer) = s.renderer.as_mut() else {
                    s.failure = Some("accelerated paint after renderer release".into());
                    return;
                };

                match renderer.import(info) {
                    Ok(()) => {
                        s.accelerated += 1;
                        s.painted_size = [info.extra.coded_size.width, info.extra.coded_size.height];
                    }
                    Err(e) => s.failure = Some(e.to_string()),
                }
            });
        }
    }
}

wrap_load_handler! {
    struct LoadingHandler {}

    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            _browser: Option<&mut Browser>,
            is_loading: i32,
            _can_go_back: i32,
            _can_go_forward: i32,
        ) {
            STATE.with_borrow_mut(|s| s.loading = is_loading != 0);
        }

        fn on_load_end(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _status: i32,
        ) {
            if frame.is_some_and(|f| f.is_main() != 0) {
                STATE.with_borrow_mut(|s| s.loaded += 1);
            }
        }

        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            code: Errorcode,
            _text: Option<&CefString>,
            _url: Option<&CefString>,
        ) {
            if frame.is_some_and(|f| f.is_main() != 0) {
                eprintln!("CEF navigation error: {code:?}");
                STATE.with_borrow_mut(|s| s.load_errors += 1);
            }
        }
    }
}

wrap_life_span_handler! {
    struct LifeHandler {}

    impl LifeSpanHandler {
        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            STATE.with_borrow_mut(|s| {
                s.closed = true;
                s.browser = None;
            });
        }
    }
}

wrap_client! {
    struct PageClient {
        paint: RenderHandler,
        loading: LoadHandler,
        life: LifeSpanHandler,
    }

    impl Client {
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(self.paint.clone())
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(self.loading.clone())
        }

        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(self.life.clone())
        }
    }
}

wrap_browser_process_handler! {
    struct ProcessHandler {}

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            let window = WindowInfo {
                windowless_rendering_enabled: 1,
                shared_texture_enabled: 1,
                external_begin_frame_enabled: 1,
                ..Default::default()
            };
            let settings = BrowserSettings {
                windowless_frame_rate: 60,
                background_color: 0,
                ..Default::default()
            };
            let url = STATE.with_borrow(|s| s.url.clone());
            let mut client = PageClient::new(
                PaintHandler::new(),
                LoadingHandler::new(),
                LifeHandler::new(),
            );
            let browser = browser_host_create_browser_sync(
                Some(&window),
                Some(&mut client),
                Some(&url.as_str().into()),
                Some(&settings),
                None,
                None,
            );
            STATE.with_borrow_mut(|s| {
                if browser.is_none() {
                    s.failure = Some("CEF browser creation failed".into());
                }
                s.browser = browser;
            });
        }
    }
}

wrap_app! {
    struct PrototypeApp {}

    impl App {
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(ProcessHandler::new())
        }
    }
}

fn host() -> Option<BrowserHost> {
    STATE.with_borrow(|s| s.browser.as_ref().and_then(|b| b.host()))
}

fn pump() {
    do_message_loop_work();
    if let Some(host) = host() {
        host.send_external_begin_frame();
    }
    std::thread::sleep(Duration::from_millis(8));
}

fn navigate(url: &str) -> Result<()> {
    let frame = STATE.with_borrow(|s| s.browser.as_ref().and_then(|b| b.main_frame()));
    frame
        .ok_or("missing browser frame")?
        .load_url(Some(&url.into()));
    Ok(())
}

fn wait_frame(
    evidence: &Path,
    name: &str,
    state: FrameState,
    minimum_loads: u32,
) -> Result<Vec<u8>> {
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut observed = 0;
    let mut mismatch = String::from("no accelerated CEF frame received");
    while Instant::now() < deadline {
        pump();
        let frame = STATE.with_borrow_mut(|s| -> Result<Option<Vec<u8>>> {
            if let Some(e) = &s.failure {
                return Err(e.clone().into());
            }
            if s.accelerated == observed || s.painted_size != s.size || s.loaded < minimum_loads {
                return Ok(None);
            }
            observed = s.accelerated;
            let renderer = s
                .renderer
                .as_mut()
                .ok_or("renderer unavailable while waiting for frame")?;
            Ok(Some(renderer.capture()?))
        })?;
        if let Some(frame) = frame {
            std::fs::write(evidence.join(format!("{name}.rgba")), &frame)?;
            match scenario::assert_frame(&frame, state) {
                Ok(()) => {
                    eprintln!("PASS {name}");
                    return Ok(frame);
                }
                Err(e) => mismatch = e,
            }
        }
    }
    Err(format!("{name}: accelerated unavailable or invalid pixels: {mismatch}").into())
}

fn run(evidence: &Path) -> Result<()> {
    let initial = wait_frame(evidence, "initial", FrameState::Initial, 1)?;
    let event = MouseEvent {
        x: 150,
        y: 280,
        modifiers: 0,
    };
    let host = host().ok_or("missing browser host")?;
    host.set_focus(1);
    host.send_mouse_click_event(Some(&event), MouseButtonType::LEFT, 0, 1);
    host.send_mouse_click_event(Some(&event), MouseButtonType::LEFT, 1, 1);
    let updated = wait_frame(evidence, "clicked", FrameState::Updated, 1)?;
    scenario::assert_canvas_changed(&initial, &updated)?;
    STATE.with_borrow_mut(|s| s.size = [800, 500]);
    host.was_resized();
    wait_frame(evidence, "resized", FrameState::Updated, 1)?;

    // OnLoadError precedes completion of the failed navigation. Starting the
    // recovery there races CEF's error-document commit; wait for loading=false.
    navigate("file:///ipp-cef-prototype-does-not-exist/page.html")?;
    let deadline = Instant::now() + Duration::from_secs(8);
    while STATE.with_borrow(|s| s.load_errors == 0 || s.loading) && Instant::now() < deadline {
        pump();
    }
    if STATE.with_borrow(|s| s.load_errors == 0 || s.loading) {
        return Err("missing navigation error".into());
    }
    let (url, loads) = STATE.with_borrow(|s| (s.url.clone(), s.loaded));
    navigate(&url)?;
    wait_frame(evidence, "recovered", FrameState::Initial, loads + 1)?;
    let frame = STATE
        .with_borrow(|s| s.browser.as_ref().and_then(|b| b.main_frame()))
        .ok_or("missing main frame")?;
    frame.execute_java_script(
        Some(&include_str!("fixtures/transparent.js").into()),
        Some(&url.as_str().into()),
        1,
    );
    wait_frame(evidence, "transparent", FrameState::Transparent, loads + 1)?;
    Ok(())
}

fn main() {
    if let Err(error) = entry() {
        eprintln!("FAIL: {error}");
        std::process::exit(1);
    }
}

fn entry() -> Result<()> {
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let args = Args::new();
    let mut app = PrototypeApp::new();
    let code = execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if code >= 0 {
        std::process::exit(code);
    }
    let library_dir = std::env::var("IPP_EGL_LIBRARY_DIR").unwrap_or_else(|_| "/usr/lib64".into());
    let renderer = render::Renderer::new(Path::new(&library_dir))?;
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/panel.html");
    STATE.with_borrow_mut(|s| {
        s.renderer = Some(renderer);
        s.url = format!("file://{}", fixture.display());
    });
    let evidence = PathBuf::from(std::env::var("IPP_CEF_EVIDENCE")?);
    std::fs::create_dir_all(&evidence)?;
    let cache = evidence.join("profile");
    let cef_dir = std::env::var("IPP_CEF_RUNTIME")?;
    let settings = Settings {
        windowless_rendering_enabled: 1,
        external_message_pump: 1,
        no_sandbox: 1,
        root_cache_path: cache.to_str().ok_or("invalid cache path")?.into(),
        resources_dir_path: cef_dir.as_str().into(),
        locales_dir_path: format!("{cef_dir}/locales").as_str().into(),
        log_file: evidence
            .join("chromium.log")
            .to_str()
            .ok_or("invalid log path")?
            .into(),
        log_severity: LogSeverity::WARNING,
        ..Default::default()
    };
    if initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    ) != 1
    {
        return Err("CEF initialization failed".into());
    }
    let outcome = run(&evidence);
    if let Some(host) = host() {
        host.close_browser(1);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while !STATE.with_borrow(|s| s.closed) && Instant::now() < deadline {
        do_message_loop_work();
        std::thread::sleep(Duration::from_millis(8));
    }
    let closed = STATE.with_borrow(|s| s.closed);
    let (accelerated, cpu) = STATE.with_borrow(|s| (s.accelerated, s.cpu));
    // After browser closure, drop GPU resources with their context current,
    // then shut CEF down. No further paint callbacks can reach the renderer.
    STATE.with_borrow_mut(|s| {
        s.renderer.take();
    });
    if closed {
        shutdown();
    }
    // Cleanup errors must not suppress the observed browser/rendering result.
    let cleanup = if closed && cache.exists() {
        std::fs::remove_dir_all(&cache)
    } else {
        Ok(())
    };
    let profile_removed = cleanup.is_ok() && !cache.exists();
    std::fs::write(
        evidence.join("result.json"),
        format!(
            "{{\"accelerated_frames\":{accelerated},\"cpu_frames\":{cpu},\"closed\":{closed},\"profile_removed\":{profile_removed},\"passed\":{}}}\n",
            outcome.is_ok() && closed && cpu == 0 && profile_removed
        ),
    )?;
    outcome?;
    cleanup?;
    if !closed {
        return Err("CEF browser failed to close before deadline".into());
    }
    if cpu != 0 {
        return Err("unexpected CPU painting".into());
    }
    println!(
        "PASS: real Chromium HTML/canvas → DMA-BUF → IPP GLES Surface; click, resize, load error/recovery, shutdown; {accelerated} GPU frames, {cpu} CPU frames"
    );
    Ok(())
}
