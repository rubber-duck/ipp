//! Exact root configuration, surface selection and immutable completed capture stamps.

use crate::{
    ProtocolError,
    codec::{Reader, Writer},
    references::OutputReference,
    wire::*,
};
use ipp_core::WorldViewport;

/// Sources fit together with a complete frame/capture reply in one bounded message.
pub const MAX_PRESENTATION_SOURCES: usize = (crate::MAX_MESSAGE_BYTES - 1024) / 65;

/// Included content at or beyond the immutable Host-admission evaluation cut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentedSource {
    /// Exact requested output lifetime.
    pub output: OutputReference,
    /// First evaluation eligible after request admission.
    pub minimum_tick: u64,
    /// Completed source, or an equivalent retained image of that source's content.
    pub publication: PresentationIdentity,
    /// Actual included source evaluation tick.
    pub tick: u64,
}

#[cfg(test)]
#[path = "presentation_tests.rs"]
mod tests;

/// Transport identity pair; never constructs a private core runtime token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentationIdentity {
    /// Runtime Host identity, never a durable World identity.
    pub host: u64,
    /// Monotonic generation or publication revision within that Host.
    pub serial: u64,
}

/// Configuration acknowledgement, not a successful render.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RootBinding {
    /// Exact selected producer lifetime.
    pub output: OutputReference,
    /// Acknowledged drawing and layout extent.
    pub viewport: WorldViewport,
    /// Fresh even on an equal-value explicit bind.
    pub generation: PresentationIdentity,
}

impl From<ipp_core::RootOutputBinding> for RootBinding {
    fn from(binding: ipp_core::RootOutputBinding) -> Self {
        let (host, serial) = binding.generation.identity();
        Self {
            output: binding.output.into(),
            viewport: binding.viewport,
            generation: PresentationIdentity {
                host,
                serial,
            },
        }
    }
}

/// One actual platform drawing surface and its current context lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentationSurface {
    /// Actual platform surface identity.
    pub id: u64,
    /// Fresh context lifetime on loss/replacement.
    pub context: u64,
    /// Maximum supported physical width.
    pub max_width: u32,
    /// Maximum supported physical height.
    pub max_height: u32,
}

/// The surface's explicit selection; a new selection never reuses its serial.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresentationView {
    /// Exact physical context and negotiated device bounds.
    pub surface: PresentationSurface,
    /// Fresh for each explicit selection, including A-to-B-to-A cycles.
    pub selection: u64,
    /// Exact Host root configuration.
    pub binding: RootBinding,
}

/// Successful renderer completion. It does not assert OS compositor scanout.
#[derive(Clone, Debug, PartialEq)]
pub struct PresentedFrame {
    /// Physical selection validated at draw/readback completion.
    pub view: PresentationView,
    /// Actual successful draw count; independent of World evaluation.
    pub sequence: u64,
    /// Actual source publication, not a requested historical replay.
    pub publication: PresentationIdentity,
    /// Completed main-pass draw submissions.
    pub draw_calls: u32,
    /// Submitted triangles.
    pub triangles: u32,
    /// Skipped draw submissions; success does not claim complete resource readiness.
    pub failed_draw_calls: u32,
    /// Deduplicated requested output inclusion witnesses.
    pub sources: Vec<PresentedSource>,
}

/// Presentation failures are distinct from authoring/session failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentationError {
    /// This platform has no rendering surface.
    Unsupported,
    /// The selected publication, surface or transfer is unavailable.
    Unavailable,
    /// A selection, context, root generation or producer lifetime changed.
    StaleView,
    /// The acknowledged extent cannot be drawn exactly.
    InvalidViewport,
    /// An exact publication is no longer the currently authorized draw source.
    ObsoletePublication,
    /// Bounded reliable completion or readback storage is exhausted.
    Capacity,
    /// No eligible completion before the monotonic Host deadline.
    Timeout,
    /// Renderer draw or readback failed; no successful frame is claimed.
    DrawFailed,
}

/// Host-owned time and rendering; no request steps evaluation.
#[derive(Clone, Debug, PartialEq)]
pub enum PresentationRequest {
    /// Discover the actual current surface and device bounds.
    Surface,
    /// Configure one physical surface independently of authoring sessions.
    Select {
        /// Exact discovered surface context.
        surface: PresentationSurface,
        /// Exact current root configuration.
        binding: RootBinding,
    },
    /// Resize the selected view's root in one step: rebind the same output with
    /// `viewport` and select the new binding, so no draw sees nothing selected.
    Resize {
        /// Exact current selection; any other fails as stale.
        view: PresentationView,
        /// New drawing and layout extent, validated as a selection's.
        viewport: WorldViewport,
    },
    /// Compare-and-clear only this exact selection.
    Clear(PresentationView),
    /// Await a future actual draw, bounded by a monotonic Host deadline.
    Frame {
        /// Exact physical/root selection.
        view: PresentationView,
        /// Optional strict lower sequence bound, never simulation time.
        after_sequence: Option<u64>,
        /// Optional exact current source requirement; no replay authority.
        publication: Option<PresentationIdentity>,
        /// Reserve an immutable top-left RGBA8 CPU snapshot.
        capture: bool,
        /// Observe post-admission completed content from these exact outputs.
        after_outputs: Vec<OutputReference>,
    },
    /// Read a bounded chunk of an immutable completed snapshot.
    ReadCapture {
        /// Connection-owned capture identity.
        capture: u64,
        /// Absolute byte offset in the RGBA8 snapshot.
        offset: u64,
    },
    /// Idempotent exact connection-owned snapshot cleanup.
    ReleaseCapture(u64),
    /// Cancel an outstanding request without changing Host configuration.
    CancelFrame {
        /// Original correlation on this connection.
        request: u64,
    },
}

/// Capture chunks refer to an immutable, already stamped CPU snapshot.
#[derive(Clone, Debug, PartialEq)]
pub enum PresentationResponse {
    /// Actual current platform surface/context.
    Surface(PresentationSurface),
    /// Selection acknowledgement, not render completion.
    View(PresentationView),
    /// Actual successful draw.
    Frame(PresentedFrame),
    /// Readback completed and stamped before the drawing buffer was reused.
    Capture {
        /// Immutable source stamp retained across later selection/context changes.
        frame: PresentedFrame,
        /// Connection-owned read/release identity.
        capture: u64,
        /// Exact RGBA8 byte length.
        bytes: u64,
    },
    /// At most one message-sized transfer page.
    Chunk {
        /// Exact connection-owned snapshot identity.
        capture: u64,
        /// Echoed absolute byte offset.
        offset: u64,
        /// Owned snapshot bytes.
        bytes: Vec<u8>,
    },
    /// Cleanup completed; never implies a draw.
    Complete,
    /// Typed operation failure leaving other Worlds and sessions usable.
    Error(PresentationError),
}

impl Reader<'_> {
    pub(crate) fn presentation_identity(&mut self) -> Result<PresentationIdentity, ProtocolError> {
        Ok(PresentationIdentity {
            host: self.u64()?,
            serial: self.u64()?,
        })
    }

    pub(crate) fn root_binding(&mut self) -> Result<RootBinding, ProtocolError> {
        Ok(RootBinding {
            output: self.output_reference()?,
            viewport: WorldViewport {
                width: self.u32()?,
                height: self.u32()?,
                device_pixel_ratio: self.f64()?,
            },
            generation: self.presentation_identity()?,
        })
    }

    fn presentation_surface(&mut self) -> Result<PresentationSurface, ProtocolError> {
        Ok(PresentationSurface {
            id: self.u64()?,
            context: self.u64()?,
            max_width: self.u32()?,
            max_height: self.u32()?,
        })
    }

    pub(crate) fn presentation_view(&mut self) -> Result<PresentationView, ProtocolError> {
        Ok(PresentationView {
            surface: self.presentation_surface()?,
            selection: self.u64()?,
            binding: self.root_binding()?,
        })
    }

    fn presented_frame(&mut self) -> Result<PresentedFrame, ProtocolError> {
        Ok(PresentedFrame {
            view: self.presentation_view()?,
            sequence: self.u64()?,
            publication: self.presentation_identity()?,
            draw_calls: self.u32()?,
            triangles: self.u32()?,
            failed_draw_calls: self.u32()?,
            sources: {
                let count = self.count(MAX_PRESENTATION_SOURCES)?;
                (0..count)
                    .map(|_| {
                        Ok(PresentedSource {
                            output: self.output_reference()?,
                            minimum_tick: self.u64()?,
                            publication: self.presentation_identity()?,
                            tick: self.u64()?,
                        })
                    })
                    .collect::<Result<_, ProtocolError>>()?
            },
        })
    }

    pub(crate) fn presentation_request(&mut self) -> Result<PresentationRequest, ProtocolError> {
        Ok(match self.u8()? {
            PRESENTATION_REQUEST_SURFACE => PresentationRequest::Surface,
            PRESENTATION_REQUEST_SELECT => PresentationRequest::Select {
                surface: self.presentation_surface()?,
                binding: self.root_binding()?,
            },
            PRESENTATION_REQUEST_RESIZE => PresentationRequest::Resize {
                view: self.presentation_view()?,
                viewport: WorldViewport {
                    width: self.u32()?,
                    height: self.u32()?,
                    device_pixel_ratio: self.f64()?,
                },
            },
            PRESENTATION_REQUEST_CLEAR => PresentationRequest::Clear(self.presentation_view()?),
            PRESENTATION_REQUEST_FRAME => PresentationRequest::Frame {
                view: self.presentation_view()?,
                after_sequence: if self.boolean()? {
                    Some(self.u64()?)
                } else {
                    None
                },
                publication: if self.boolean()? {
                    Some(self.presentation_identity()?)
                } else {
                    None
                },
                capture: self.boolean()?,
                after_outputs: {
                    let count = self.count(MAX_PRESENTATION_SOURCES)?;
                    (0..count)
                        .map(|_| self.output_reference())
                        .collect::<Result<_, _>>()?
                },
            },
            PRESENTATION_REQUEST_READ_CAPTURE => PresentationRequest::ReadCapture {
                capture: self.u64()?,
                offset: self.u64()?,
            },
            PRESENTATION_REQUEST_RELEASE_CAPTURE => {
                PresentationRequest::ReleaseCapture(self.u64()?)
            }
            PRESENTATION_REQUEST_CANCEL_FRAME => PresentationRequest::CancelFrame {
                request: self.u64()?,
            },
            tag => return Err(ProtocolError::Unsupported(tag)),
        })
    }

    pub(crate) fn presentation_response(&mut self) -> Result<PresentationResponse, ProtocolError> {
        Ok(match self.u8()? {
            PRESENTATION_RESPONSE_SURFACE => {
                PresentationResponse::Surface(self.presentation_surface()?)
            }
            PRESENTATION_RESPONSE_VIEW => PresentationResponse::View(self.presentation_view()?),
            PRESENTATION_RESPONSE_FRAME => PresentationResponse::Frame(self.presented_frame()?),
            PRESENTATION_RESPONSE_CAPTURE => PresentationResponse::Capture {
                frame: self.presented_frame()?,
                capture: self.u64()?,
                bytes: self.u64()?,
            },
            PRESENTATION_RESPONSE_CHUNK => PresentationResponse::Chunk {
                capture: self.u64()?,
                offset: self.u64()?,
                bytes: self.bytes()?.to_vec(),
            },
            PRESENTATION_RESPONSE_COMPLETE => PresentationResponse::Complete,
            PRESENTATION_RESPONSE_ERROR => PresentationResponse::Error(match self.u8()? {
                PRESENTATION_ERROR_UNSUPPORTED => PresentationError::Unsupported,
                PRESENTATION_ERROR_UNAVAILABLE => PresentationError::Unavailable,
                PRESENTATION_ERROR_STALE_VIEW => PresentationError::StaleView,
                PRESENTATION_ERROR_INVALID_VIEWPORT => PresentationError::InvalidViewport,
                PRESENTATION_ERROR_OBSOLETE_PUBLICATION => PresentationError::ObsoletePublication,
                PRESENTATION_ERROR_CAPACITY => PresentationError::Capacity,
                PRESENTATION_ERROR_TIMEOUT => PresentationError::Timeout,
                PRESENTATION_ERROR_DRAW_FAILED => PresentationError::DrawFailed,
                tag => return Err(ProtocolError::Unsupported(tag)),
            }),
            tag => return Err(ProtocolError::Unsupported(tag)),
        })
    }
}

impl Writer {
    fn presentation_identity(
        &mut self,
        identity: PresentationIdentity,
    ) -> Result<(), ProtocolError> {
        self.u64(identity.host)?;
        self.u64(identity.serial)
    }

    pub(crate) fn root_binding(&mut self, binding: RootBinding) -> Result<(), ProtocolError> {
        self.output_reference(binding.output)?;
        self.u32(binding.viewport.width)?;
        self.u32(binding.viewport.height)?;
        self.f64(binding.viewport.device_pixel_ratio)?;
        self.presentation_identity(binding.generation)
    }

    fn presentation_surface(&mut self, surface: PresentationSurface) -> Result<(), ProtocolError> {
        self.u64(surface.id)?;
        self.u64(surface.context)?;
        self.u32(surface.max_width)?;
        self.u32(surface.max_height)
    }

    pub(crate) fn presentation_view(
        &mut self,
        view: PresentationView,
    ) -> Result<(), ProtocolError> {
        self.presentation_surface(view.surface)?;
        self.u64(view.selection)?;
        self.root_binding(view.binding)
    }

    fn presented_frame(&mut self, frame: &PresentedFrame) -> Result<(), ProtocolError> {
        self.presentation_view(frame.view)?;
        self.u64(frame.sequence)?;
        self.presentation_identity(frame.publication)?;
        self.u32(frame.draw_calls)?;
        self.u32(frame.triangles)?;
        self.u32(frame.failed_draw_calls)?;
        self.count(frame.sources.len(), MAX_PRESENTATION_SOURCES)?;
        for source in &frame.sources {
            self.output_reference(source.output)?;
            self.u64(source.minimum_tick)?;
            self.presentation_identity(source.publication)?;
            self.u64(source.tick)?;
        }
        Ok(())
    }

    pub(crate) fn presentation_request(
        &mut self,
        request: &PresentationRequest,
    ) -> Result<(), ProtocolError> {
        match request {
            PresentationRequest::Surface => self.u8(PRESENTATION_REQUEST_SURFACE),
            PresentationRequest::Select {
                surface,
                binding,
            } => {
                self.u8(PRESENTATION_REQUEST_SELECT)?;
                self.presentation_surface(*surface)?;
                self.root_binding(*binding)
            }
            PresentationRequest::Resize {
                view,
                viewport,
            } => {
                self.u8(PRESENTATION_REQUEST_RESIZE)?;
                self.presentation_view(*view)?;
                self.u32(viewport.width)?;
                self.u32(viewport.height)?;
                self.f64(viewport.device_pixel_ratio)
            }
            PresentationRequest::Clear(view) => {
                self.u8(PRESENTATION_REQUEST_CLEAR)?;
                self.presentation_view(*view)
            }
            PresentationRequest::Frame {
                view,
                after_sequence,
                publication,
                capture,
                after_outputs,
            } => {
                self.u8(PRESENTATION_REQUEST_FRAME)?;
                self.presentation_view(*view)?;
                self.u8(u8::from(after_sequence.is_some()))?;
                if let Some(sequence) = after_sequence {
                    self.u64(*sequence)?;
                }
                self.u8(u8::from(publication.is_some()))?;
                if let Some(publication) = publication {
                    self.presentation_identity(*publication)?;
                }
                self.u8(u8::from(*capture))?;
                self.count(after_outputs.len(), MAX_PRESENTATION_SOURCES)?;
                for output in after_outputs {
                    self.output_reference(*output)?;
                }
                Ok(())
            }
            PresentationRequest::ReadCapture {
                capture,
                offset,
            } => {
                self.u8(PRESENTATION_REQUEST_READ_CAPTURE)?;
                self.u64(*capture)?;
                self.u64(*offset)
            }
            PresentationRequest::ReleaseCapture(capture) => {
                self.u8(PRESENTATION_REQUEST_RELEASE_CAPTURE)?;
                self.u64(*capture)
            }
            PresentationRequest::CancelFrame {
                request,
            } => {
                self.u8(PRESENTATION_REQUEST_CANCEL_FRAME)?;
                self.u64(*request)
            }
        }
    }

    pub(crate) fn presentation_response(
        &mut self,
        response: &PresentationResponse,
    ) -> Result<(), ProtocolError> {
        match response {
            PresentationResponse::Surface(surface) => {
                self.u8(PRESENTATION_RESPONSE_SURFACE)?;
                self.presentation_surface(*surface)
            }
            PresentationResponse::View(view) => {
                self.u8(PRESENTATION_RESPONSE_VIEW)?;
                self.presentation_view(*view)
            }
            PresentationResponse::Frame(frame) => {
                self.u8(PRESENTATION_RESPONSE_FRAME)?;
                self.presented_frame(frame)
            }
            PresentationResponse::Capture {
                frame,
                capture,
                bytes,
            } => {
                self.u8(PRESENTATION_RESPONSE_CAPTURE)?;
                self.presented_frame(frame)?;
                self.u64(*capture)?;
                self.u64(*bytes)
            }
            PresentationResponse::Chunk {
                capture,
                offset,
                bytes,
            } => {
                self.u8(PRESENTATION_RESPONSE_CHUNK)?;
                self.u64(*capture)?;
                self.u64(*offset)?;
                self.bytes(bytes)
            }
            PresentationResponse::Complete => self.u8(PRESENTATION_RESPONSE_COMPLETE),
            PresentationResponse::Error(error) => {
                self.u8(PRESENTATION_RESPONSE_ERROR)?;
                self.u8(match error {
                    PresentationError::Unsupported => PRESENTATION_ERROR_UNSUPPORTED,
                    PresentationError::Unavailable => PRESENTATION_ERROR_UNAVAILABLE,
                    PresentationError::StaleView => PRESENTATION_ERROR_STALE_VIEW,
                    PresentationError::InvalidViewport => PRESENTATION_ERROR_INVALID_VIEWPORT,
                    PresentationError::ObsoletePublication => {
                        PRESENTATION_ERROR_OBSOLETE_PUBLICATION
                    }
                    PresentationError::Capacity => PRESENTATION_ERROR_CAPACITY,
                    PresentationError::Timeout => PRESENTATION_ERROR_TIMEOUT,
                    PresentationError::DrawFailed => PRESENTATION_ERROR_DRAW_FAILED,
                })
            }
        }
    }
}
