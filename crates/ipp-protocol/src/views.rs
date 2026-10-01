//! Untrusted completed-view queries; resolution belongs after the Host's ordered frame.

use crate::{
    ProtocolError,
    codec::{Reader, Writer},
    references::OutputReference,
    wire::*,
};
use ipp_core::{ErrorReason, HostRuntime, ViewDescriptor, ViewPickHit, WorldRef, WorldViewport};

/// Explicit current root or retained CPU publication, never implicit presentation selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewQueryTarget {
    /// Current or exact source under an exact current root binding, including generation.
    BoundView {
        /// Acknowledged root identity and viewport.
        binding: crate::presentation::RootBinding,
        /// None requests current completed state at execution; Some requires exact available history.
        publication: Option<crate::presentation::PresentationIdentity>,
    },
    /// Require this exact current root and caller-observed viewport.
    RootView {
        /// Exact selected camera producer.
        output: OutputReference,
        /// Caller-observed root dimensions and display scale.
        expected_viewport: WorldViewport,
    },
    /// Read retained CPU history without presentation or input authority.
    PublicationView {
        /// Exact retained camera producer.
        output: OutputReference,
        /// Publication Host identity.
        host: u64,
        /// Exact retained publication revision.
        revision: u64,
        /// Explicit historical projection dimensions and display scale.
        viewport: WorldViewport,
    },
}

impl ViewQueryTarget {
    /// Validate the session's World and every runtime lifetime at query execution.
    pub fn resolve(
        self,
        host: &HostRuntime,
        world: WorldRef,
    ) -> Result<ipp_core::ViewQueryTarget, ErrorReason> {
        let output = match self {
            Self::BoundView {
                binding,
                ..
            } => binding.output,
            Self::RootView {
                output,
                ..
            }
            | Self::PublicationView {
                output,
                ..
            } => output,
        };
        if output.world != world.into() {
            return Err(ErrorReason::InvalidEntity);
        }
        let output = output
            .resolve(host)
            .map_err(|_| ErrorReason::InvalidEntity)?;
        Ok(match self {
            Self::BoundView {
                binding,
                publication,
            } => {
                let current = host
                    .root_output_binding(world)?
                    .filter(|current| crate::presentation::RootBinding::from(*current) == binding)
                    .ok_or(ErrorReason::InvalidEntity)?;
                ipp_core::ViewQueryTarget::BoundView {
                    binding: current,
                    publication: publication
                        .map(|source| host.resolve_publication_ref(source.host, source.serial))
                        .transpose()?,
                }
            }
            Self::RootView {
                expected_viewport,
                ..
            } => ipp_core::ViewQueryTarget::RootView {
                output,
                expected_viewport,
            },
            Self::PublicationView {
                host: identity,
                revision,
                viewport,
                ..
            } => ipp_core::ViewQueryTarget::PublicationView {
                output,
                publication: host.resolve_publication_ref(identity, revision)?,
                viewport,
            },
        })
    }
}

/// Correlated navigation of the exact bound root Camera, never an active-camera fallback.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraNavigateRequest {
    /// Exact root identity and source; historical-only targets cannot authorize mutation.
    pub binding: crate::presentation::RootBinding,
    /// None requests current completed state; Some is strict and never substitutes a new source.
    pub publication: Option<crate::presentation::PresentationIdentity>,
    /// Gesture deltas without raster dimensions.
    pub motion: ipp_core::systems::camera::CameraViewMotion,
}

impl CameraNavigateRequest {
    /// Bind untrusted syntax in its authoring session, before ordinary queue admission.
    pub fn resolve(
        self,
        host: &HostRuntime,
        world: WorldRef,
    ) -> Result<ipp_core::systems::camera::CameraNavigationCommand, ErrorReason> {
        let ipp_core::ViewQueryTarget::BoundView {
            binding,
            publication,
        } = (ViewQueryTarget::BoundView {
            binding: self.binding,
            publication: self.publication,
        })
        .resolve(host, world)?
        else {
            unreachable!("bound view");
        };
        host.camera_navigation(binding, publication, &[], self.motion)
    }
}

/// Normalized viewport picking against an exact completed view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryPickQuery {
    /// Exact view selection.
    pub view: ViewQueryTarget,
    /// Normalized horizontal coordinate.
    pub x: f32,
    /// Normalized vertical coordinate.
    pub y: f32,
    /// Include a camera-facing drag plane in the selected camera domain.
    pub include_view_plane: bool,
}

/// Projection onto an explicit plane in a completed camera domain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraProjectQuery {
    /// Exact view selection.
    pub view: ViewQueryTarget,
    /// Normalized horizontal coordinate, possibly outside the viewport.
    pub x: f32,
    /// Normalized vertical coordinate, possibly outside the viewport.
    pub y: f32,
    /// Explicit camera-domain plane.
    pub plane: ipp_core::WorldPlane,
}

/// Correlated result whose successful descriptor identifies every view input.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewQueryOutcome<T> {
    /// Original session request identity.
    pub request_id: u64,
    /// Evaluated owning World tick, not presentation completion.
    pub tick: u64,
    /// Resolved view and CPU result, or an explicit failure.
    pub result: Result<(ViewDescriptor, T), ErrorReason>,
}

/// Correlated World-qualified pick result.
pub type GeometryPickOutcome = ViewQueryOutcome<Option<ViewPickHit>>;
/// Correlated camera-domain projection result.
pub type CameraProjectOutcome = ViewQueryOutcome<Option<[f32; 3]>>;

impl Reader<'_> {
    pub(crate) fn view_source(
        &mut self,
    ) -> Result<Option<crate::presentation::PresentationIdentity>, ProtocolError> {
        match self.u8()? {
            OPTION_NONE => Ok(None),
            OPTION_SOME => Ok(Some(crate::presentation::PresentationIdentity {
                host: self.u64()?,
                serial: self.u64()?,
            })),
            _ => Err(ProtocolError::Malformed("view source")),
        }
    }

    fn view_viewport(&mut self) -> Result<WorldViewport, ProtocolError> {
        Ok(WorldViewport {
            width: self.u32()?,
            height: self.u32()?,
            device_pixel_ratio: self.f64()?,
        })
    }

    pub(crate) fn view_target(&mut self) -> Result<ViewQueryTarget, ProtocolError> {
        let tag = self.u8()?;
        if tag == VIEW_BOUND {
            return Ok(ViewQueryTarget::BoundView {
                binding: self.root_binding()?,
                publication: self.view_source()?,
            });
        }
        let output = self.output_reference()?;
        match tag {
            VIEW_ROOT => Ok(ViewQueryTarget::RootView {
                output,
                expected_viewport: self.view_viewport()?,
            }),
            VIEW_PUBLICATION => Ok(ViewQueryTarget::PublicationView {
                output,
                host: self.u64()?,
                revision: self.u64()?,
                viewport: self.view_viewport()?,
            }),
            _ => Err(ProtocolError::Malformed("view target")),
        }
    }
}

impl Writer {
    pub(crate) fn view_descriptor(&mut self, view: &ViewDescriptor) -> Result<(), ProtocolError> {
        self.output_reference(view.output.into())?;
        self.publication_reference(view.publication)?;
        self.u32(view.viewport.width)?;
        self.u32(view.viewport.height)?;
        self.f64(view.viewport.device_pixel_ratio)
    }

    pub(crate) fn publication_reference(
        &mut self,
        publication: ipp_core::WorldPublicationId,
    ) -> Result<(), ProtocolError> {
        let (host, revision) = publication.identity();
        self.u64(host)?;
        self.u64(revision)
    }
}
