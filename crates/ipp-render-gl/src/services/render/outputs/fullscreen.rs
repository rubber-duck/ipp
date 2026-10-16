//! Exact fullscreen Camera backgrounds bypass intermediate image composition.

use super::super::canvas::layering::{CanvasChild, canvas_attachment, plane_matrix};
use super::super::canvas::scene::CanvasScene;
use crate::RenderError;
use ipp_core::{HostRuntime, OutputKind, OutputRef, WorldPublicationId};

#[derive(Clone, Copy)]
pub(in crate::services::render) struct FullscreenCamera {
    pub root: OutputRef,
    pub selection: OutputRef,
    pub publication: WorldPublicationId,
    pub extent: [f64; 2],
}

pub(in crate::services::render) fn select(
    host: &HostRuntime,
    root: OutputRef,
    publication: WorldPublicationId,
) -> Result<Option<FullscreenCamera>, RenderError> {
    if root.kind() != OutputKind::Canvas {
        return Ok(None);
    }

    let scene = CanvasScene::new(host, root, publication)?;
    // Only the first paint can be hoisted ahead of the GUI. Anything already
    // underneath it, including another Camera during transitions, keeps RTT.
    let Some(ipp_core::systems::canvas::CanvasPaintEntry::Attachment(slot)) =
        scene.canvas.entries.first().map(|entry| entry.as_ref())
    else {
        return Ok(None);
    };
    let Some(CanvasChild::Camera {
        selection,
        mvp,
        extent,
        clip,
        opacity,
    }) = canvas_attachment(
        &scene,
        0,
        plane_matrix([0.0; 2], [1.0; 2])?,
        scene.root_clip(),
        1.0,
    )?
    else {
        return Ok(None);
    };

    let close = |left: f32, right: f32| (left - right).abs() <= 0.00001 * right.abs().max(1.0);
    if opacity != 1.0
        || !close(mvp[12], 0.0)
        || !close(mvp[13], scene.canvas.logical_extent[1])
        || mvp[0] <= 0.0
        || mvp[5] >= 0.0
        || !close(mvp[0], -mvp[5])
        || !close(mvp[0] * extent[0], scene.canvas.logical_extent[0])
        || !close(-mvp[5] * extent[1], scene.canvas.logical_extent[1])
        || clip[0] > 0.0
        || clip[1] > 0.0
        || clip[2] < extent[0]
        || clip[3] < extent[1]
    {
        return Ok(None);
    }

    let Some((_, publication)) = scene.attachment(slot).output() else {
        return Ok(None);
    };
    Ok(Some(FullscreenCamera {
        root,
        selection,
        publication,
        extent: extent.map(f64::from),
    }))
}
