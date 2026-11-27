//! Retention and projection of optional Surface images.
use ipp_core::{HostRuntime, OutputRef, WorldPublicationId, systems::canvas::CanvasPaintEntry};
use ipp_render_scene::canvas::{CanvasProjection, CanvasScene};
use std::collections::BTreeMap;

use crate::{device::Target, scene::resource, shaders::WebGpuShader};

#[derive(Copy, Clone, PartialEq)]
pub(super) struct Content {
    revisions: [u64; 4],
    extent: [f32; 2],
}

impl Content {
    pub fn leaf(host: &HostRuntime, scene: CanvasScene<'_>) -> Option<Self> {
        let canvas = scene.canvas;
        if canvas.interaction.requires_direct()
            || canvas
                .entries
                .iter()
                .any(|e| matches!(e.as_ref(), CanvasPaintEntry::Attachment(_)))
        {
            return None;
        }
        if canvas.paints.iter().any(|p| {
            p.shader
                .and_then(|key| resource::<WebGpuShader>(host, scene.world.id, key.to_u64()))
                .is_some_and(|shader| shader.definition.recipe.features & 64 != 0)
        }) {
            return None;
        }
        Some(Self {
            revisions: [
                canvas.layout_revision,
                canvas.paint_revision,
                canvas.resource_revision,
                canvas.paints_revision,
            ],
            extent: canvas.logical_extent,
        })
    }
}

pub(super) struct CachedCanvas {
    pub target: Target,
    pub content: Option<Content>,
    pub included: BTreeMap<OutputRef, WorldPublicationId>,
    pub time: f64,
    pub frame: u64,
    pub band: u8,
}

pub(super) fn image_projection(
    scene: CanvasProjection<'_>,
    extent: [f32; 2],
) -> CanvasProjection<'_> {
    let CanvasProjection::Plane {
        matrix: image_matrix,
        ..
    } = CanvasProjection::viewport(extent)
    else {
        unreachable!()
    };
    match scene {
        CanvasProjection::Plane {
            world,
            ..
        } => CanvasProjection::Plane {
            matrix: image_matrix,
            world,
        },
        CanvasProjection::Surface {
            matrix,
            world,
            geometry,
            scale,
            offset,
            ..
        } => CanvasProjection::Surface {
            matrix,
            world,
            geometry,
            scale,
            offset,
            image_projection: Some(image_matrix),
        },
        CanvasProjection::Analytic(_) => unreachable!(),
    }
}
