//! Untrusted completed-view queries; resolution belongs after the Host's ordered frame.

use crate::{
    ProtocolError,
    codec::{Reader, Writer},
    contract::wire_manifest::*,
    host::presentation::{PresentationIdentity, RootBinding},
    references::OutputReference,
};
use ipp_core::{ErrorReason, HostRuntime, ViewDescriptor, ViewPickHit, WorldRef, WorldViewport};

/// Explicit current root or retained CPU publication, never implicit presentation selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RootViewQueryTarget {
    /// Current or exact source under an exact current root binding, including generation.
    BoundView {
        /// Acknowledged root identity and viewport.
        binding: RootBinding,
        /// None requests current completed state at execution; Some requires exact available history.
        publication: Option<PresentationIdentity>,
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

impl RootViewQueryTarget {
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
                    .filter(|current| RootBinding::from(*current) == binding)
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

/// A selected root or an explicit Camera domain below that root.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewQueryTarget {
    /// Existing root query semantics.
    Root(RootViewQueryTarget),
    /// Coordinates and geometry belong to the Camera reached by this exact path.
    CameraView {
        /// Nonrecursive root target.
        root: RootViewQueryTarget,
        /// Applied attachment producer identities.
        path: Vec<CameraAttachmentReference>,
    },
}

impl From<RootViewQueryTarget> for ViewQueryTarget {
    fn from(root: RootViewQueryTarget) -> Self {
        Self::Root(root)
    }
}

/// Exact attachment producer metadata copied from an acknowledged receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CameraAttachmentReference {
    /// Parent World lifetime.
    pub parent: crate::references::WorldReference,
    /// Generational attachment entity.
    pub anchor: u64,
    /// Attachment component lifetime.
    pub incarnation: u64,
    /// Host-unique applied producer revision.
    pub revision: u64,
}

/// Bounded wire path; the bound is independent of scene/World count.
pub const MAX_CAMERA_PATH_DEPTH: usize = 64;

fn camera_path(
    host: &HostRuntime,
    root: ipp_core::ViewQueryTarget,
    path: &[CameraAttachmentReference],
) -> Result<Vec<ipp_core::WorldAttachmentToken>, ErrorReason> {
    if path.len() > MAX_CAMERA_PATH_DEPTH {
        return Err(ErrorReason::Capacity);
    }
    let view = host.resolve_view(root)?;
    let mut source = host
        .publication(view.publication)
        .ok_or(ErrorReason::InvalidEntity)?;
    let mut tokens = Vec::with_capacity(path.len());
    for reference in path {
        let edge = source
            .attachments
            .iter()
            .find(|edge| {
                crate::references::WorldReference::from(edge.token.parent()) == reference.parent
                    && edge.token.anchor().to_bits() == reference.anchor
                    && edge.token.incarnation() == reference.incarnation
                    && edge.token.identity().1 == reference.revision
            })
            .ok_or(ErrorReason::InvalidEntity)?;
        tokens.push(edge.token.clone());
        source = host
            .attached_publication(edge)
            .ok_or(ErrorReason::InvalidEntity)?;
    }
    Ok(tokens)
}

impl ViewQueryTarget {
    /// Resolve the explicit root through its authoring session.
    pub fn resolve(
        &self,
        host: &HostRuntime,
        world: WorldRef,
    ) -> Result<ipp_core::ViewQueryTarget, ErrorReason> {
        let root = match self {
            Self::Root(root)
            | Self::CameraView {
                root,
                ..
            } => root,
        };
        root.resolve(host, world)
    }

    /// Query geometry in either the root or explicitly named child Camera domain.
    pub fn pick(
        &self,
        host: &HostRuntime,
        world: WorldRef,
        point: [f32; 2],
        include_view_plane: bool,
    ) -> Result<(ViewDescriptor, Option<ViewPickHit>), ErrorReason> {
        let root = self.resolve(host, world)?;
        match self {
            Self::Root(_) => host.pick_view(root, point, include_view_plane),
            Self::CameraView {
                path,
                ..
            } => {
                let path = camera_path(host, root, path)?;
                host.pick_camera_domain(root, &path, point, include_view_plane)
            }
        }
    }

    /// Project an explicit plane in the selected Camera domain.
    pub fn project(
        &self,
        host: &HostRuntime,
        world: WorldRef,
        point: [f32; 2],
        plane: ipp_core::WorldPlane,
    ) -> Result<(ViewDescriptor, Option<[f32; 3]>), ErrorReason> {
        let root = self.resolve(host, world)?;
        match self {
            Self::Root(_) => host.project_view(root, point, plane),
            Self::CameraView {
                path,
                ..
            } => {
                let path = camera_path(host, root, path)?;
                host.project_camera_view(root, &path, point, plane)
                    .map(|(view, point)| (view.root, point))
            }
        }
    }
}

/// Correlated navigation of an explicit composed Camera, without target fallback.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraNavigateRequest {
    /// Exact root-to-Camera attachment producer path.
    pub path: Vec<CameraAttachmentReference>,
    /// Exact root identity and source; historical-only targets cannot authorize mutation.
    pub binding: RootBinding,
    /// None requests current completed state; Some is strict and never substitutes a new source.
    pub publication: Option<PresentationIdentity>,
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
        } = (RootViewQueryTarget::BoundView {
            binding: self.binding,
            publication: self.publication,
        })
        .resolve(host, world)?
        else {
            unreachable!("bound view");
        };
        let root = ipp_core::ViewQueryTarget::BoundView {
            binding,
            publication,
        };
        let path = camera_path(host, root, &self.path)?;
        host.camera_navigation(binding, publication, &path, self.motion)
    }
}

/// Normalized viewport picking against an exact completed view.
#[derive(Clone, Debug, PartialEq)]
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
#[derive(Clone, Debug, PartialEq)]
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
    pub(crate) fn view_source(&mut self) -> Result<Option<PresentationIdentity>, ProtocolError> {
        match self.u8()? {
            OPTION_NONE => Ok(None),
            OPTION_SOME => Ok(Some(PresentationIdentity {
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

    fn root_view_target(&mut self) -> Result<RootViewQueryTarget, ProtocolError> {
        let tag = self.u8()?;
        self.root_view_tag(tag)
    }

    fn root_view_tag(&mut self, tag: u8) -> Result<RootViewQueryTarget, ProtocolError> {
        if tag == VIEW_BOUND {
            return Ok(RootViewQueryTarget::BoundView {
                binding: self.root_binding()?,
                publication: self.view_source()?,
            });
        }
        if !matches!(tag, VIEW_ROOT | VIEW_PUBLICATION) {
            return Err(ProtocolError::Malformed("nonrecursive root view target"));
        }
        let output = self.output_reference()?;
        match tag {
            VIEW_ROOT => Ok(RootViewQueryTarget::RootView {
                output,
                expected_viewport: self.view_viewport()?,
            }),
            VIEW_PUBLICATION => Ok(RootViewQueryTarget::PublicationView {
                output,
                host: self.u64()?,
                revision: self.u64()?,
                viewport: self.view_viewport()?,
            }),
            _ => Err(ProtocolError::Malformed("view target")),
        }
    }

    pub(crate) fn view_target(&mut self) -> Result<ViewQueryTarget, ProtocolError> {
        let tag = self.u8()?;
        if tag == VIEW_CAMERA {
            Ok(ViewQueryTarget::CameraView {
                root: self.root_view_target()?,
                path: self.camera_path()?,
            })
        } else {
            self.root_view_tag(tag).map(Into::into)
        }
    }

    pub(crate) fn camera_path(&mut self) -> Result<Vec<CameraAttachmentReference>, ProtocolError> {
        let count = self.u32()? as usize;
        if count > MAX_CAMERA_PATH_DEPTH {
            return Err(ProtocolError::Malformed("camera path depth"));
        }
        (0..count)
            .map(|_| {
                Ok(CameraAttachmentReference {
                    parent: self.world_reference()?,
                    anchor: self.u64()?,
                    incarnation: self.u64()?,
                    revision: self.u64()?,
                })
            })
            .collect()
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
