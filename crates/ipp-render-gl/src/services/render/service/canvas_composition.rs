//! Direct Canvas planes and attachment-authorized nested painting.

use super::super::canvas_scene::{CanvasMapping, CanvasScene};
use super::super::frame_statistics::RenderFrameWork;
use super::super::scene::SceneOutputSurface;
use super::{RenderError, RenderService};
use crate::RenderDevice;
use ipp_core::systems::camera;
use ipp_core::systems::canvas::{CanvasClip, CanvasPaintEntry};
use ipp_core::{HostRuntime, OutputKind, WorldViewport};

pub(super) fn plane_matrix(origin: [f64; 2], scale: [f64; 2]) -> Result<[f32; 16], RenderError> {
    let mut matrix = [0.0; 16];
    matrix[0] = scale[0] as f32;
    matrix[5] = scale[1] as f32;
    matrix[10] = 1.0;
    matrix[12] = origin[0] as f32;
    matrix[13] = origin[1] as f32;
    matrix[15] = 1.0;
    if matrix.iter().any(|value| !value.is_finite()) || matrix[0] == 0.0 || matrix[5] == 0.0 {
        return Err(RenderError::InvalidTransform);
    }
    Ok(matrix)
}

/// Exact provider mapping, with its front normal supplying the offset column
/// used by direct separated layers. Content translation/rotation/shear stay intact.
pub(super) fn affine_matrix(
    geometry: &dyn ipp_core::systems::surface::Surface,
) -> Result<[f32; 16], RenderError> {
    let extent = geometry.physical_extent();
    let mut matrix = geometry
        .exact_affine(0.0)
        .ok_or(RenderError::UnavailableOutput)?
        .map(|v| v as f32);
    let normal = geometry
        .sample(extent.map(|v| v * 0.5), 0.0)
        .map_err(|_| RenderError::UnavailableOutput)?
        .front_normal;
    for (row, value) in normal.into_iter().enumerate() {
        matrix[8 + row] = value as f32;
    }
    if matrix.iter().any(|v| !v.is_finite()) {
        return Err(RenderError::InvalidTransform);
    }
    Ok(matrix)
}

/// Where the camera views a layered Canvas from, in its content space, whose
/// Z is the Surface's local normal axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum CanvasLayerEye {
    /// A perspective camera's position along the normal.
    Point(f64),
    /// The normal component of an orthographic camera's view direction.
    Direction(f64),
}

/// How one Canvas presentation separates completed physical plane positions.
/// Retained geometry stays unchanged while its normal coordinate moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CanvasLayering {
    /// Content Z per resolved normal coordinate; zero keeps one plane.
    pub spacing: f32,
    /// Viewer choosing the order in which layer planes draw.
    pub eye: CanvasLayerEye,
}

impl CanvasLayering {
    /// One plane: layers only order paint, as in a root viewport or a nested slot.
    pub const FLAT: Self = Self {
        spacing: 0.0,
        eye: CanvasLayerEye::Direction(-1.0),
    };

    /// Content model-view-projection at a completed physical coordinate.
    pub fn layer_mvp(&self, mvp: &[f32; 16], coordinate: f64) -> [f32; 16] {
        let offset = (coordinate * f64::from(self.spacing)) as f32;
        let mut placed = *mvp;
        if offset != 0.0 {
            for row in 0..4 {
                placed[12 + row] += offset * mvp[8 + row];
            }
        }
        placed
    }

    /// Distance used for back-to-front translucent drawing. Equal distances
    /// keep the original logical painter order, independently of plane IDs.
    pub fn view_depth(&self, coordinate: f64) -> f64 {
        let depth = coordinate * f64::from(self.spacing);
        match self.eye {
            CanvasLayerEye::Point(eye) => (depth - eye).abs(),
            CanvasLayerEye::Direction(direction) => depth * direction,
        }
    }
}

pub(super) enum CanvasChild<'a> {
    Canvas {
        scene: CanvasScene<'a>,
        mvp: [f32; 16],
        clip: CanvasClip,
        opacity: f32,
    },
    Camera {
        selection: ipp_core::OutputRef,
        mvp: [f32; 16],
        extent: [f32; 2],
        clip: CanvasClip,
        opacity: f32,
    },
}

impl<D: RenderDevice> RenderService<D> {
    pub(super) fn draw_output_surface(
        &mut self,
        host: &HostRuntime,
        surface: &SceneOutputSurface,
        view: &camera::CameraPublication,
        view_projection: [f32; 16],
        viewport: WorldViewport,
        stats: &mut RenderFrameWork,
    ) -> Result<(), RenderError> {
        #[cfg(feature = "instrumentation")]
        let _gpu_surface = self.gpu_scope(
            super::RenderGpuScope::Surface,
            Some(surface.entity.world),
            Some(surface.entity.entity),
        );
        let placed = camera::multiply(view_projection, surface.model);
        let [width, height] = surface.extent.map(f64::from);
        let affine = affine_matrix(&*surface.geometry)?;
        let normal = surface
            .geometry
            .sample([width * 0.5, height * 0.5], 0.0)
            .map_err(|_| RenderError::UnavailableOutput)?
            .front_normal;
        match surface.selection.kind() {
            OutputKind::Canvas => {
                let scene = CanvasScene::new(host, surface.selection, surface.publication)?;
                let extent = scene.canvas.logical_extent.map(f64::from);
                let content = camera::multiply(
                    affine,
                    plane_matrix([0.0; 2], [width / extent[0], height / extent[1]])?,
                );
                let layering = if surface.layered() {
                    CanvasLayering {
                        spacing: surface.layer_spacing,
                        eye: {
                            let centre = surface
                                .geometry
                                .sample([width * 0.5, height * 0.5], 0.0)
                                .map_err(|_| RenderError::UnavailableOutput)?
                                .position;
                            if view.projection.projection == 0 {
                                let eye =
                                    surface.placement.inverse_point(view.pose.point([0.0; 3]));
                                CanvasLayerEye::Point(
                                    (0..3).map(|i| (eye[i] - centre[i]) * normal[i]).sum(),
                                )
                            } else {
                                let direction = surface
                                    .placement
                                    .inverse_vector(view.pose.vector([0.0, 0.0, -1.0]));
                                CanvasLayerEye::Direction(
                                    (0..3).map(|i| direction[i] * normal[i]).sum(),
                                )
                            }
                        },
                    }
                } else {
                    CanvasLayering::FLAT
                };
                self.draw_canvas(
                    &scene,
                    camera::multiply(placed, content),
                    scene.root_clip(),
                    1.0,
                    layering,
                    viewport,
                    stats,
                )
            }
            OutputKind::Camera => {
                let content = camera::multiply(affine, plane_matrix([0.0, height], [1.0, -1.0])?);
                self.draw_camera_image(
                    surface.selection,
                    camera::multiply(placed, content),
                    surface.extent,
                    [0.0, 0.0, surface.extent[0], surface.extent[1]],
                    1.0,
                    stats,
                )
            }
        }
    }
}

pub(super) fn canvas_attachment<'a>(
    scene: &CanvasScene<'a>,
    index: usize,
    mvp: [f32; 16],
    clip: CanvasClip,
    opacity: f32,
) -> Result<Option<CanvasChild<'a>>, RenderError> {
    let Some(CanvasPaintEntry::Attachment(slot)) =
        scene.canvas.entries.get(index).map(|entry| entry.as_ref())
    else {
        return Ok(None);
    };

    let opacity = opacity * slot.opacity;
    if opacity <= 0.0 {
        return Ok(None);
    }

    let attachment = scene.attachment(slot);
    let Some((selection, publication)) = attachment.output() else {
        return Ok(None);
    };

    let clip = [
        clip[0].max(slot.clip[0]),
        clip[1].max(slot.clip[1]),
        clip[2].min(slot.clip[2]),
        clip[3].min(slot.clip[3]),
    ];
    if clip[0] >= clip[2] || clip[1] >= clip[3] {
        return Ok(None);
    }

    match selection.kind() {
        OutputKind::Canvas => {
            let child = CanvasScene::new(scene.host, selection, publication)?;
            let extent = child.canvas.logical_extent;
            let Some(mapping) = attachment.child_mapping(extent, clip) else {
                return Err(RenderError::InvalidTransform);
            };

            if mapping.clip[0] >= mapping.clip[2] || mapping.clip[1] >= mapping.clip[3] {
                return Ok(None);
            }

            let content = plane_matrix(mapping.origin, mapping.scale)?;
            Ok(Some(CanvasChild::Canvas {
                scene: child,
                mvp: camera::multiply(mvp, content),
                clip: mapping.clip,
                opacity,
            }))
        }
        OutputKind::Camera => {
            let extent = slot.physical_extent;
            let origin = slot.to_canvas([-extent[0] * 0.5, -extent[1] * 0.5]);
            let extent = extent.map(|value| value as f32);
            let Some(mapping) = CanvasMapping::new(origin, slot.scale, clip, extent) else {
                return Err(RenderError::InvalidTransform);
            };

            let content = plane_matrix(mapping.origin, mapping.scale)?;
            Ok(Some(CanvasChild::Camera {
                selection,
                mvp: camera::multiply(mvp, content),
                extent,
                clip: mapping.clip,
                opacity,
            }))
        }
    }
}

#[cfg(test)]
#[path = "canvas_composition_tests.rs"]
mod tests;
