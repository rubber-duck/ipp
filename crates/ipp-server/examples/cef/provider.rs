//! Host-owned source/request reconciliation and callback-scoped GPU import.

use super::{CefSetup, engine::CefEngine};
use crate::egl::Context;
use ipp_core::HostRuntime;
use ipp_core::services::web_content::{
    WebContentImageDescription, WebContentRequestId, WebContentSourceId,
};
use ipp_render_gl::{
    GlesIOSurfaceImage, GlesIOSurfaceImport, GlesRenderDevice, RenderService, WebImageCompletion,
    WebImageReservation,
};
use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;

type Renderer = RenderService<GlesRenderDevice>;

struct Source {
    authority: WebContentSourceId,
    url: CString,
}

struct Page {
    browser: u64,
    source: u32,
    size: [u32; 2],
}

struct Callbacks {
    importer: GlesIOSurfaceImport,
    renderer: *mut Renderer,
    context: *const Context,
    tickets: BTreeMap<u64, (WebImageReservation, usize)>,
    completed: Vec<WebImageCompletion>,
    events: Vec<(u64, u32, String)>,
}

impl Callbacks {
    fn event(&mut self, browser: u64, kind: u32, text: String) {
        if self.events.len() < 256
            && !self
                .events
                .iter()
                .any(|event| event.0 == browser && event.1 == kind && event.2 == text)
        {
            self.events.push((browser, kind, text));
        }
    }
}

// SAFETY: engine.h guarantees synchronous main-thread callbacks with a stable,
// exclusively borrowed box. Catch panics so no unwind crosses C/Objective-C.
unsafe extern "C" fn paint(
    context: *mut c_void,
    browser: u64,
    surface: *mut c_void,
    width: u32,
    height: u32,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: The engine borrows the stable box until completed shutdown;
        // callback code is the only code accessing it while inside a CEF call.
        let state = unsafe { &mut *context.cast::<Callbacks>() };
        if state.renderer.is_null() || state.context.is_null() {
            return;
        }
        let Some((ticket, budget)) = state.tickets.remove(&browser) else {
            return;
        };
        let imported = (|| {
            // SAFETY: CEF lends stable IOSurface storage until this callback
            // returns. Import completes all GPU reads before dropping the borrow.
            let image = unsafe { GlesIOSurfaceImage::from_raw(surface, width, height) }
                .map_err(|e| e.to_string())?;
            if ticket.pixel_size() != [width, height] || image.allocation_bytes() > budget {
                return Err("CEF frame exceeded its reserved extent or staging budget".into());
            }
            // SAFETY: tick installs these exclusive borrows only around CEF
            // calls. No Host/World reference or renderer borrow overlaps them.
            let context = unsafe { &*state.context };
            context.ensure_current().map_err(|e| e.to_string())?;
            // SAFETY: tick lends the renderer exclusively for this callback.
            let renderer = unsafe { &mut *state.renderer };
            renderer
                .fill_reserved_web_image(ticket, |device, destination| {
                    device.copy_io_surface(&state.importer, &image, destination)
                })
                .map_err(|e| e.to_string())
        })();
        match imported {
            Ok(completion) => state.completed.push(completion),
            Err(error) => state.event(browser, 2, error),
        }
    }));
}

unsafe extern "C" fn event(context: *mut c_void, browser: u64, kind: u32, text: *const c_char) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: CEF lends a terminated string for this callback only. Copy it
        // into the bounded queue, without accessing a Host or retaining pointers.
        let state = unsafe { &mut *context.cast::<Callbacks>() };
        // SAFETY: engine.h guarantees a terminated callback-borrowed string.
        let text = unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .chars()
            .take(4096)
            .collect();
        state.event(browser, kind, text);
    }));
}

pub(crate) struct CefProvider {
    engine: CefEngine,
    callbacks: Box<Callbacks>,
    root: PathBuf,
    sources: BTreeMap<u32, Source>,
    pages: BTreeMap<WebContentRequestId, Page>,
    next_source: u64,
    next_browser: u64,
    events: Vec<(u32, u32, String)>,
}

impl CefProvider {
    pub fn new(setup: CefSetup, context: &Context) -> Result<Self, String> {
        let root = setup.root.canonicalize().map_err(|e| e.to_string())?;
        // SAFETY: The Host retains its exact current EGL context and libraries
        // until after this provider shuts down on the same main thread.
        let importer = unsafe { GlesIOSurfaceImport::from_loader(|name| context.gl(name)) }
            .map_err(|e| e.to_string())?;
        let mut callbacks = Box::new(Callbacks {
            importer,
            renderer: std::ptr::null_mut(),
            context: std::ptr::null(),
            tickets: BTreeMap::new(),
            completed: Vec::new(),
            events: Vec::new(),
        });
        // SAFETY: The box never moves; Drop pumps shutdown before releasing it.
        let engine = unsafe {
            CefEngine::open(
                &setup.engine,
                paint,
                event,
                (&mut *callbacks as *mut Callbacks).cast(),
            )
        }?;
        Ok(Self {
            engine,
            callbacks,
            root,
            sources: BTreeMap::new(),
            pages: BTreeMap::new(),
            next_source: 0,
            next_browser: 0,
            events: Vec::new(),
        })
    }

    pub fn disconnect(&mut self, host: &mut HostRuntime) {
        for source in std::mem::take(&mut self.sources).into_values() {
            host.web_content_mut().revoke_source(source.authority);
        }
        self.events.clear();
    }

    /// Bounded development bridge: source registration, withdrawal, input and
    /// event delivery. World authoring still uses the common generated client.
    pub fn command(&mut self, host: &mut HostRuntime, bytes: &[u8]) -> Result<Vec<u8>, String> {
        if bytes.len() < 5 {
            return Err("Truncated CEF command".into());
        }
        let operation = bytes[0];
        let id = u32::from_le_bytes(bytes[1..5].try_into().unwrap());
        let data = &bytes[5..];
        match operation {
            1 => {
                if id == 0 || self.sources.contains_key(&id) || self.sources.len() >= 32 {
                    return Err("CEF source capacity or identity conflict".into());
                }
                let name = std::str::from_utf8(data).map_err(|e| e.to_string())?;
                let path = self
                    .root
                    .join(name)
                    .canonicalize()
                    .map_err(|e| e.to_string())?;
                if !path.starts_with(&self.root)
                    || !path.is_file()
                    || path.extension().is_none_or(|ext| ext != "html")
                {
                    return Err("CEF page must be HTML within the configured local root".into());
                }
                let encoded: String = path
                    .to_string_lossy()
                    .bytes()
                    .map(|byte| {
                        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
                            char::from(byte).to_string()
                        } else {
                            format!("%{byte:02X}")
                        }
                    })
                    .collect();
                let url = CString::new(format!("file://{encoded}")).map_err(|e| e.to_string())?;
                self.next_source = self
                    .next_source
                    .checked_add(1)
                    .ok_or("CEF source identity exhausted")?;
                let name = format!("cef:{}", self.next_source);
                let authority = host.web_content_mut().register_source(name.as_str());
                self.sources.insert(
                    id,
                    Source {
                        authority,
                        url,
                    },
                );
                Ok(name.into_bytes())
            }
            2 if data.is_empty() => {
                if let Some(source) = self.sources.remove(&id) {
                    host.web_content_mut().revoke_source(source.authority);
                }
                Ok(Vec::new())
            }
            3 => {
                if data.len() < 41 {
                    return Err("Truncated CEF input".into());
                }
                let kind = data[0];
                let a = u32::from_le_bytes(data[1..5].try_into().unwrap());
                let b = u32::from_le_bytes(data[5..9].try_into().unwrap());
                let values: [f64; 4] = std::array::from_fn(|i| {
                    f64::from_le_bytes(data[9 + i * 8..17 + i * 8].try_into().unwrap())
                });
                if values
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
                {
                    return Err("Invalid CEF input coordinates".into());
                }
                let text = CString::new(&data[41..]).map_err(|e| e.to_string())?;
                // A source binds one input consumer. Ambiguous shared sources
                // cannot guess which request has authority for an input event.
                let mut pages = self.pages.values().filter(|page| page.source == id);
                let Some(page) = pages.next() else {
                    return Ok(Vec::new());
                };
                if pages.next().is_some() {
                    return Ok(Vec::new());
                }
                let browser = page.browser;
                // SAFETY: Engine owns the live browser; all borrowed text is
                // copied synchronously and callbacks cannot access a World.
                unsafe {
                    match kind {
                        1..=4 => (self.engine.pointer)(
                            browser,
                            u32::from(kind),
                            a,
                            values[0],
                            values[1],
                            values[2],
                            values[3],
                        ),
                        5 => (self.engine.key)(browser, a, b),
                        6 => (self.engine.text)(browser, text.as_ptr()),
                        7 => (self.engine.focus)(browser, a as i32),
                        _ => return Err("Unknown CEF input kind".into()),
                    }
                }
                Ok(Vec::new())
            }
            4 if data.is_empty() => {
                let mut bytes = Vec::new();
                for (source, kind, text) in self.events.drain(..) {
                    bytes.extend_from_slice(&source.to_le_bytes());
                    bytes.extend_from_slice(&kind.to_le_bytes());
                    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
                    bytes.extend_from_slice(text.as_bytes());
                }
                Ok(bytes)
            }
            _ => Err("Unknown CEF command".into()),
        }
    }

    pub fn tick(
        &mut self,
        host: &mut HostRuntime,
        renderer: &mut Renderer,
        context: &Context,
    ) -> Result<(), String> {
        #[cfg(feature = "instrumentation")]
        let _allocation_scope = ipp_core::profiling::AllocationScope::new(228, "native.cef-poll");
        context.ensure_current().map_err(|e| e.to_string())?;
        for completion in self.callbacks.completed.drain(..) {
            let _ = renderer.complete_web_image(host, completion);
        }
        renderer
            .refresh_web_images(host)
            .map_err(|e| e.to_string())?;
        // Request identities are retained by the service. Reconcile borrowed
        // entries; an unchanged tick needs no cloned request list or tree nodes.
        self.pages.retain(|request, page| {
            if host.web_content().request(*request).is_some() {
                return true;
            }
            self.callbacks.tickets.remove(&page.browser);
            // SAFETY: Main-thread engine call; callbacks only enqueue owned data.
            unsafe { (self.engine.close)(page.browser) };
            false
        });
        for request in host.web_content().requests() {
            if self.pages.contains_key(&request.id) {
                continue;
            }
            let Some((&source, registration)) = self
                .sources
                .iter()
                .find(|(_, source)| source.authority == request.source)
            else {
                continue;
            };
            let size = request.logical_size.map(|v| v.ceil() as u32);
            if size.contains(&0) || size.iter().any(|v| *v > 4096) {
                continue;
            }
            self.next_browser = self
                .next_browser
                .checked_add(1)
                .ok_or("CEF browser identity exhausted")?;
            let browser = self.next_browser;
            // SAFETY: Engine copies the URL and owns browser lifetime. No GPU
            // callback is admitted before its request has reserved storage.
            if unsafe { (self.engine.create)(browser, registration.url.as_ptr(), size[0], size[1]) }
                != 0
            {
                self.pages.insert(
                    request.id,
                    Page {
                        browser,
                        source,
                        size,
                    },
                );
            }
        }
        context.ensure_current().map_err(|e| e.to_string())?;
        for (request, page) in &self.pages {
            if self.callbacks.tickets.contains_key(&page.browser) {
                continue;
            }
            // Account for padded IOSurface rows and allocation alignment before
            // requesting a frame; validate actual metadata inside the callback.
            let bytes = ((page.size[0] as usize * 4 + 255) & !255) * page.size[1] as usize + 16384;
            if let Ok(ticket) = renderer.reserve_web_image(
                host,
                *request,
                WebContentImageDescription {
                    pixel_size: page.size,
                },
                bytes,
            ) {
                self.callbacks.tickets.insert(page.browser, (ticket, bytes));
            }
        }
        self.callbacks.renderer = renderer;
        self.callbacks.context = context;
        // SAFETY: Scoped exclusive borrows are available only to callbacks. No
        // renderer, context or World method runs concurrently with CEF calls.
        unsafe {
            for page in self.pages.values() {
                (self.engine.begin_frame)(page.browser);
            }
            (self.engine.pump)();
        }
        self.callbacks.renderer = std::ptr::null_mut();
        self.callbacks.context = std::ptr::null();
        context.ensure_current().map_err(|e| e.to_string())?;
        for (browser, kind, text) in self.callbacks.events.drain(..) {
            if let Some(page) = self.pages.values().find(|page| page.browser == browser) {
                if kind == 2 {
                    eprintln!("gles_host: CEF {}: {text}", page.source);
                }
                if self.events.len() < 256 {
                    self.events.push((page.source, kind, text));
                }
            }
        }
        Ok(())
    }
}

impl Drop for CefProvider {
    fn drop(&mut self) {
        self.callbacks.tickets.clear();
        self.callbacks.completed.clear();
        // SAFETY: Callback box and engine remain alive until every browser has
        // closed and CefShutdown has finished. No World or GPU borrow is active.
        unsafe {
            while (self.engine.shutdown)() == 0 {
                (self.engine.pump)();
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    }
}
