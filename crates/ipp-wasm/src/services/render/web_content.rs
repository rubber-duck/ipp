//! DOM capture admission for the presentation canvas's own ElementImages.

use std::collections::{BTreeMap, BTreeSet};

use ipp_core::services::web_content::{
    WebContentImageDescription, WebContentRequestId, WebContentSourceId,
};
use ipp_render_gl::WebImageReservation;

use super::RenderSurfaceService;
use crate::BOUNDARY;

#[derive(Default)]
pub(super) struct BrowserWebContent {
    sources: BTreeMap<u32, WebContentSourceId>,
    served: BTreeSet<WebContentRequestId>,
    pending: BTreeMap<u32, (WebContentRequestId, WebImageReservation)>,
    next_source: u32,
    next_ticket: u32,
    records: Vec<u32>,
    error: String,
}

impl RenderSurfaceService {
    fn html_register(&mut self, host: &mut ipp_core::HostRuntime, id: u32) -> bool {
        if id <= self.web_content.next_source || self.web_content.sources.len() >= 32 {
            return false;
        }
        self.web_content.next_source = id;
        let source = host.web_content_mut().register_source(format!("html:{id}"));
        self.web_content.sources.insert(id, source);
        true
    }

    fn html_poll(&mut self, host: &mut ipp_core::HostRuntime) -> *const u32 {
        let state = &mut self.web_content;
        state.records.clear();
        if !self.active {
            return std::ptr::null();
        }
        let requests: Vec<_> = host.web_content().requests().cloned().collect();
        state
            .served
            .retain(|id| requests.iter().any(|request| request.id == *id));
        for request in requests {
            if state.served.contains(&request.id)
                || state.pending.len() >= 32
                || state.pending.values().any(|(id, _)| *id == request.id)
            {
                continue;
            }
            let Some((&source, _)) = state.sources.iter().find(|(_, id)| **id == request.source)
            else {
                continue;
            };
            let size = request.logical_size.map(|value| value.ceil() as u32);
            if size.contains(&0) || size.iter().any(|value| *value > 2048) {
                let _ = host.web_content_mut().unavailable(request.id);
                state.served.insert(request.id);
                continue;
            }
            let Some(ticket_id) = state.next_ticket.checked_add(1) else {
                break;
            };
            let Ok(ticket) = self.renderer.reserve_web_image(
                host,
                request.id,
                WebContentImageDescription {
                    pixel_size: size,
                },
                size[0] as usize * size[1] as usize * 4,
            ) else {
                continue;
            };
            state.next_ticket = ticket_id;
            state.served.insert(request.id);
            state.pending.insert(ticket_id, (request.id, ticket));
            state.records.extend([ticket_id, source, size[0], size[1]]);
        }
        state.records.as_ptr()
    }
}

/// Register a never-reused local source name; DOM ownership stays on the main thread.
// SAFETY: Unique export, scalar arguments, exclusive Host access; no references escape.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_html_register(id: u32) -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary.presentation_host().map_or(0, |(surface, host)| {
            u32::from(surface.html_register(host, id))
        })
    })
}

/// Request a fresh immutable image for changed DOM; pending imports finish normally.
// SAFETY: Unique export; exclusive Host access, scalar identity only.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_html_changed(id: u32) {
    BOUNDARY.with_borrow_mut(|boundary| {
        if let Some((surface, host)) = boundary.presentation_host()
            && let Some(source) = surface.web_content.sources.get(&id)
        {
            for request in host
                .web_content()
                .requests()
                .filter(|request| request.source == *source)
            {
                surface.web_content.served.remove(&request.id);
            }
        }
    });
}

/// Withdraw the exact registration, including images retained by frozen output.
// SAFETY: Unique export, scalar arguments, exclusive Host access; no references escape.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_html_revoke(id: u32) {
    BOUNDARY.with_borrow_mut(|boundary| {
        if let Some((surface, host)) = boundary.presentation_host()
            && let Some(source) = surface.web_content.sources.remove(&id)
        {
            host.web_content_mut().revoke_source(source);
        }
    });
}

/// Reserve capture capacity and return records of ticket, source, width and height.
// SAFETY: Unique export. Storage stays valid until the next poll or Host destruction;
// JavaScript copies it synchronously before making another runtime call.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_html_requests_ptr() -> *const u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation_host()
            .map_or(std::ptr::null(), |(surface, host)| surface.html_poll(host))
    })
}

/// Number of u32 words in the last poll.
// SAFETY: Unique export; exclusive access returns only a scalar.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_html_requests_len() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |surface| surface.web_content.records.len() as u32)
    })
}

/// Consume one admitted capture. Zero image cancels after the producer releases it.
// SAFETY: Unique export; names stay in the owning renderer, import is synchronous
// and never reenters WASM. The caller releases even an early-rejected stage.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_html_complete(ticket_id: u32, image: u32) -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        let Some((surface, host)) = boundary.presentation_host() else {
            return 0;
        };
        surface.web_content.error.clear();
        let Some((request, ticket)) = surface.web_content.pending.remove(&ticket_id) else {
            surface.web_content.error = "HTML capture ticket is no longer pending".into();
            return 0;
        };
        if !host.web_content().requests().any(|live| live.id == request) {
            drop(ticket);
            let _ = surface.renderer.refresh_web_images(host);
            return 2;
        }
        if image == 0 {
            drop(ticket);
            let _ = surface.renderer.refresh_web_images(host);
            return 0;
        }
        let size = ticket.pixel_size();
        match surface
            .renderer
            .import_reserved_web_image(host, ticket, |device, destination| {
                device.copy_web_image(image, destination, size)
            }) {
            Ok(_) => 1,
            Err(error) => {
                surface.web_content.error = format!("HTML import failed: {error}");
                0
            }
        }
    })
}

/// UTF-8 diagnostic for the last synchronous HTML completion failure.
// SAFETY: Unique export. Storage stays valid until the next completion or Host
// destruction. JavaScript copies it before calling the runtime again.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_html_error_ptr() -> *const u8 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary.presentation().map_or(std::ptr::null(), |surface| {
            surface.web_content.error.as_ptr()
        })
    })
}

/// Byte length of the last HTML completion diagnostic.
// SAFETY: Unique export; exclusive Host access returns only a scalar.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_html_error_len() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |surface| surface.web_content.error.len() as u32)
    })
}
