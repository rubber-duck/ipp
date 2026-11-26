//! Host-owned accelerated browser sources and callback-scoped IOSurface capture.

use std::{
    collections::BTreeMap,
    ffi::{CStr, CString, c_char, c_void},
    path::PathBuf,
    ptr,
};

use ipp_core::{
    HostRuntime,
    services::web_content::{WebContentImageId, WebContentRequestId, WebContentSourceId},
};
use ipp_render_metal::{MetalRenderService, MetalWebImage, web_image_private_bytes};
use ipp_server::services::cef::{CefEngine, page_url};

pub(super) struct CefSetup {
    pub(super) engine: PathBuf,
    pub(super) root: PathBuf,
    pub(super) pages: Vec<(String, PathBuf)>,
}

struct Page {
    browser: u64,
    size: [u32; 2],
}

struct Ticket {
    request: WebContentRequestId,
    image: WebContentImageId,
    size: [u32; 2],
    budget: usize,
}

struct Callbacks {
    renderer: *const MetalRenderService,
    tickets: BTreeMap<u64, Ticket>,
    completed: Vec<(Ticket, Result<MetalWebImage, String>)>,
    errors: Vec<(u64, String)>,
}

unsafe extern "C" fn paint(
    context: *mut c_void,
    browser: u64,
    surface: *mut c_void,
    width: u32,
    height: u32,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: The engine calls synchronously on the Host thread. The exclusive
        // context box is pinned in the provider until shutdown completes.
        let callbacks = unsafe { &mut *context.cast::<Callbacks>() };
        if callbacks.renderer.is_null() {
            return;
        }
        let Some(ticket) = callbacks.tickets.remove(&browser) else {
            return;
        };
        let result = if ticket.size == [width, height] {
            // SAFETY: The renderer is lent for this engine poll and cannot be
            // replaced until it returns. CEF retains the IOSurface until this
            // callback returns, after the synchronous GPU conversion is done.
            unsafe { (&*callbacks.renderer).import_web_image(surface, ticket.size, ticket.budget) }
        } else {
            Err("CEF changed the extent of its reserved image".to_owned())
        };
        callbacks.completed.push((ticket, result));
    }));
}

unsafe extern "C" fn event(context: *mut c_void, browser: u64, kind: u32, text: *const c_char) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if kind != 2 || text.is_null() {
            return;
        }
        // SAFETY: Callback context and the terminated message are CEF borrows.
        // Copy diagnostics without retaining or accessing Host state.
        let callbacks = unsafe { &mut *context.cast::<Callbacks>() };
        if callbacks.errors.len() < 64 {
            // SAFETY: The engine lends a terminated message for this callback only.
            let message = unsafe { CStr::from_ptr(text) }.to_string_lossy();
            let message: String = message.chars().take(4096).collect();
            if !callbacks
                .errors
                .iter()
                .any(|entry| entry.0 == browser && entry.1 == message)
            {
                callbacks.errors.push((browser, message));
            }
        }
    }));
}

pub(super) struct CefProvider {
    engine: CefEngine,
    callbacks: Box<Callbacks>,
    sources: BTreeMap<WebContentSourceId, CString>,
    pages: BTreeMap<WebContentRequestId, Page>,
    browser: u64,
}

impl CefProvider {
    pub(super) fn new(host: &mut HostRuntime, setup: &CefSetup) -> Result<Self, String> {
        let root = setup
            .root
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let mut urls = BTreeMap::new();
        for (name, path) in &setup.pages {
            if name.is_empty() || urls.insert(name.as_str(), page_url(&root, path)?).is_some() {
                return Err("CEF page source names must be nonempty and unique".into());
            }
        }
        let mut callbacks = Box::new(Callbacks {
            renderer: ptr::null(),
            tickets: BTreeMap::new(),
            completed: Vec::new(),
            errors: Vec::new(),
        });
        // SAFETY: The callback box remains fixed until shutdown completes. The
        // Host owns both the CEF engine thread and the Metal command queue.
        let engine = unsafe {
            CefEngine::open(
                &setup.engine,
                paint,
                event,
                (&mut *callbacks as *mut Callbacks).cast(),
            )
        }?;
        let sources = urls
            .into_iter()
            .map(|(name, url)| (host.web_content_mut().register_source(name), url))
            .collect();
        Ok(Self {
            engine,
            callbacks,
            sources,
            pages: BTreeMap::new(),
            browser: 0,
        })
    }

    pub(super) fn tick(
        &mut self,
        host: &mut HostRuntime,
        renderer: &mut MetalRenderService,
    ) -> Result<(), String> {
        renderer.prune_web_images(host);
        self.pages.retain(|request, page| {
            if host.web_content().request(*request).is_some() {
                return true;
            }
            self.callbacks.tickets.remove(&page.browser);
            // SAFETY: This provider owns the browser and calls on the engine thread.
            unsafe { (self.engine.close)(page.browser) };
            false
        });
        for request in host.web_content().requests() {
            let Some(url) = self.sources.get(&request.source) else {
                continue;
            };
            let size = request.logical_size.map(|axis| axis.ceil() as u32);
            if self.pages.contains_key(&request.id)
                || size.contains(&0)
                || size
                    .iter()
                    .any(|&axis| axis > MetalRenderService::MAX_VIEWPORT)
                || self.pages.len() >= 16
            {
                continue;
            }
            self.browser = self
                .browser
                .checked_add(1)
                .ok_or("CEF browser identity exhausted")?;
            let browser = self.browser;
            // SAFETY: The engine copies the live URL and owns the new browser.
            if unsafe { (self.engine.create)(browser, url.as_ptr(), size[0], size[1]) } != 0 {
                self.pages.insert(
                    request.id,
                    Page {
                        browser,
                        size,
                    },
                );
            }
        }

        let mut count = self.callbacks.tickets.len();
        let mut bytes = self
            .callbacks
            .tickets
            .values()
            .map(|ticket| web_image_private_bytes(ticket.size) + ticket.budget)
            .sum::<usize>();
        for (&request, page) in &self.pages {
            if self.callbacks.tickets.contains_key(&page.browser) {
                continue;
            }
            let [width, height] = page.size.map(|axis| axis as usize);
            let budget = (width * 4).next_multiple_of(256) * height + 16384;
            if let Ok(image) =
                renderer.reserve_web_image(host, request, page.size, bytes + budget, count)
            {
                self.callbacks.tickets.insert(
                    page.browser,
                    Ticket {
                        request,
                        image,
                        size: page.size,
                        budget,
                    },
                );
                bytes += budget + web_image_private_bytes(page.size);
                count += 1;
            }
        }

        self.callbacks.renderer = ptr::from_ref(renderer);
        // SAFETY: The renderer borrow is visible only while CEF synchronously
        // pumps. Callbacks do not mutate Host state or reenter the engine.
        unsafe {
            for page in self.pages.values() {
                (self.engine.begin_frame)(page.browser);
            }
            (self.engine.pump)();
        }
        self.callbacks.renderer = ptr::null();

        for (ticket, result) in self.callbacks.completed.drain(..) {
            let delivered = result.and_then(|image| {
                renderer.complete_web_image(host, ticket.request, ticket.image, image)
            });
            if let Err(error) = delivered {
                host.web_content_mut().discard_image(ticket.image);
                ipp_core::diagnostic!(Warn, "[IPP Metal] web_image.import_failed error={error}");
            }
        }
        for (browser, error) in self.callbacks.errors.drain(..) {
            ipp_core::diagnostic!(
                Warn,
                "[IPP Metal] cef.error browser={browser} error={error}"
            );
        }
        Ok(())
    }
}

impl Drop for CefProvider {
    fn drop(&mut self) {
        // SAFETY: Storage and mapped engine code outlive CEF's last callback.
        unsafe {
            while (self.engine.shutdown)() == 0 {
                (self.engine.pump)();
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    }
}
