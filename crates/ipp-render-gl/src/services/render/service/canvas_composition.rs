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
        view_projection: [f32; 16],
        viewport: WorldViewport,
        stats: &mut RenderFrameWork,
    ) -> Result<(), RenderError> {
        let placed = camera::multiply(view_projection, surface.model);
        let [width, height] = surface.extent.map(f64::from);
        match surface.selection.kind() {
            OutputKind::Canvas => {
                let scene = CanvasScene::new(host, surface.selection, surface.publication)?;
                let extent = scene.canvas.logical_extent.map(f64::from);
                let content = plane_matrix(
                    [-width * 0.5, height * 0.5],
                    [width / extent[0], -height / extent[1]],
                )?;
                self.draw_canvas(
                    &scene,
                    camera::multiply(placed, content),
                    scene.root_clip(),
                    1.0,
                    viewport,
                    stats,
                )
            }
            OutputKind::Camera => {
                let content = plane_matrix([-width * 0.5, -height * 0.5], [1.0, 1.0])?;
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
