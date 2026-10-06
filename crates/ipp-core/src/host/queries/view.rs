//! Explicit completed-view CPU queries, independent of input and presentation authority.

use crate::host::*;
use crate::{WorldPlane, systems::camera::CameraPublication};

/// Either a current selected root or an explicitly retained historical CPU view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewQueryTarget {
    /// Current or exact CPU source fenced by an exact root generation, including equal rebinds.
    BoundView {
        /// Exact previously acknowledged root configuration.
        binding: RootOutputBinding,
        /// None selects current completed state at execution; Some requires this exact source.
        publication: Option<WorldPublicationId>,
    },
    /// The current root must still select this exact output and viewport.
    RootView {
        /// Exact producer, never an active-camera fallback.
        output: OutputRef,
        /// Caller-observed dimensions and display scale.
        expected_viewport: WorldViewport,
    },
    /// Historical CPU access only; this does not assert completed presentation.
    PublicationView {
        /// Exact live producer of the retained output.
        output: OutputRef,
        /// Exact still-available completed publication.
        publication: WorldPublicationId,
        /// Explicit projection dimensions and display scale.
        viewport: WorldViewport,
    },
}

/// Exact inputs used by a completed-view CPU query, not a presentation fence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewDescriptor {
    /// Exact selected output lifetime.
    pub output: OutputRef,
    /// Exact publication read by the query.
    pub publication: WorldPublicationId,
    /// Physical dimensions used to project frozen camera inputs.
    pub viewport: WorldViewport,
}

/// A World-qualified hit and its containing camera-domain position.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewPickHit {
    /// Exact producing World, entity, component incarnation and publication.
    pub identity: PublishedSceneHit,
    /// Position in the root camera domain, including composed placement.
    pub position: [f32; 3],
    /// Optional root-domain camera-facing drag plane.
    pub view_plane: Option<WorldPlane>,
}

impl WorldPublicationId {
    /// Host-qualified transport identity; constructing a query requires Host resolution.
    pub fn identity(self) -> (u64, u64) {
        (self.host, self.revision)
    }
}

impl HostRuntime {
    /// Resolve untrusted publication syntax only while the exact CPU history is available.
    pub fn resolve_publication_ref(
        &self,
        host: u64,
        revision: u64,
    ) -> Result<WorldPublicationId, ErrorReason> {
        let identity = WorldPublicationId {
            host,
            revision,
        };
        self.publication(identity)
            .map(|value| value.id)
            .ok_or(ErrorReason::InvalidEntity)
    }

    /// Resolve a view after ordered admission and the Host frame, without reading live components.
    pub fn resolve_view(&self, target: ViewQueryTarget) -> Result<ViewDescriptor, ErrorReason> {
        let (output, publication, viewport) = match target {
            ViewQueryTarget::BoundView {
                binding,
                publication,
            } => {
                if self.root_output_binding(binding.output.world())? != Some(binding) {
                    return Err(ErrorReason::InvalidEntity);
                }
                let publication = publication
                    .or_else(|| {
                        self.root_output(binding.output.world().id())
                            .map(|(_, _, publication)| publication)
                    })
                    .ok_or(ErrorReason::InvalidEntity)?;
                (binding.output, publication, binding.viewport)
            }
            ViewQueryTarget::RootView {
                output,
                expected_viewport,
            } => {
                let (current, viewport, publication) = self
                    .root_output(output.world().id())
                    .ok_or(ErrorReason::InvalidEntity)?;
                if current != output {
                    return Err(ErrorReason::InvalidEntity);
                }
                if viewport != expected_viewport {
                    return Err(ErrorReason::InvalidViewport);
                }
                (output, publication, viewport)
            }
            ViewQueryTarget::PublicationView {
                output,
                publication,
                viewport,
            } => (output, publication, viewport),
        };
        viewport
            .validate()
            .map_err(|_| ErrorReason::InvalidViewport)?;
        self.output(publication, output)
            .ok_or(ErrorReason::InvalidEntity)?;
        Ok(ViewDescriptor {
            output,
            publication,
            viewport,
        })
    }

    fn camera_publication(
        &self,
        output: OutputRef,
        publication: WorldPublicationId,
    ) -> Result<&CameraPublication, ErrorReason> {
        if output.kind() != OutputKind::Camera {
            return Err(ErrorReason::UnsupportedDependency);
        }
        self.output(publication, output)
            .and_then(|chunk| chunk.data::<CameraPublication>())
            .ok_or(ErrorReason::InvalidEntity)
    }

    /// Pick composed frozen geometry with the frozen camera's affine clipping interval.
    pub fn pick_view(
        &self,
        target: ViewQueryTarget,
        point: [f32; 2],
        include_view_plane: bool,
    ) -> Result<(ViewDescriptor, Option<ViewPickHit>), ErrorReason> {
        if !point
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        {
            return Err(ErrorReason::InvalidViewport);
        }
        let view = self.resolve_view(target)?;
        if view.output.kind() == OutputKind::Canvas {
            let canvas = self
                .output(view.publication, view.output)
                .and_then(|chunk| chunk.data::<crate::systems::canvas::CanvasPublication>())
                .ok_or(ErrorReason::InvalidEntity)?;
            let logical = [
                point[0] * canvas.logical_extent[0],
                point[1] * canvas.logical_extent[1],
            ];
            let hit = self
                .pick_canvas_plot(view.publication, view.output, logical, Vec::new(), None)?
                .map(|identity| ViewPickHit {
                    identity,
                    position: [logical[0], logical[1], 0.0],
                    view_plane: None,
                });
            return Ok((view, hit));
        }
        let camera = self.camera_publication(view.output, view.publication)?;
        let ray = camera.ray(point, view.viewport.width, view.viewport.height)?;
        let hit =
            self.pick_publication(view.publication, view.output, &ray.ray, ray.near, ray.far)?;
        let hit = hit
            .map(|identity| {
                let position = std::array::from_fn(|index| {
                    (ray.ray.origin[index] + identity.hit.distance * ray.ray.direction[index])
                        as f32
                });
                if !position.iter().all(|value| value.is_finite()) {
                    return Err(ErrorReason::InvalidGeometry);
                }
                let view_plane = if include_view_plane {
                    let forward = camera.pose.vector([0.0, 0.0, -1.0]);
                    let length = forward
                        .iter()
                        .map(|value| value * value)
                        .sum::<f64>()
                        .sqrt();
                    Some(WorldPlane {
                        point: position,
                        normal: forward.map(|value| (value / length) as f32),
                    })
                } else {
                    None
                };
                Ok(ViewPickHit {
                    identity,
                    position,
                    view_plane,
                })
            })
            .transpose()?;
        Ok((view, hit))
    }

    /// Intersect the frozen camera ray with an explicit plane in its camera domain.
    pub fn project_view(
        &self,
        target: ViewQueryTarget,
        point: [f32; 2],
        plane: WorldPlane,
    ) -> Result<(ViewDescriptor, Option<[f32; 3]>), ErrorReason> {
        let view = self.resolve_view(target)?;
        let camera = self.camera_publication(view.output, view.publication)?;
        let position = camera.project(point, view.viewport.width, view.viewport.height, plane)?;
        Ok((view, position))
    }
}
