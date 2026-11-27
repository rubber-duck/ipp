//! Renderer-owned images for Canvas paint and authored Surface caching.

use ipp_render_scene::screen_damage::CanvasSurfaceView;
use std::{rc::Rc, sync::Arc};

use ipp_core::{
    HostRuntime, WorldPublicationId,
    systems::canvas::{CanvasPaintEntry, CanvasPrimitive},
};

use crate::{
    canvas::CanvasScene, canvas_damage::CanvasDamage, device::Target, service::asset,
    shaders::MetalShader,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct CanvasContent {
    revisions: [u64; 4],
    extent: [u32; 2],
}

impl CanvasContent {
    pub(super) fn leaf(host: &HostRuntime, scene: CanvasScene<'_>) -> Option<Self> {
        let canvas = scene.canvas;
        if canvas.interaction.requires_direct()
            || canvas
                .entries
                .iter()
                .any(|entry| matches!(entry.as_ref(), CanvasPaintEntry::Attachment(_)))
        {
            return None;
        }
        if canvas.paints.iter().any(|paint| {
            let shader = paint
                .shader
                .and_then(|key| asset::<MetalShader>(host, scene.world.id, key.to_u64()));
            shader.is_some_and(|value| value.definition().recipe.features & 64 != 0)
        }) {
            return None;
        }
        Some(Self::from_scene(scene))
    }

    fn from_scene(scene: CanvasScene<'_>) -> Self {
        let canvas = scene.canvas;
        let revisions = [
            canvas.layout_revision,
            canvas.paint_revision,
            canvas.resource_revision,
            canvas.paints_revision,
        ];
        Self {
            revisions,
            extent: canvas.logical_extent.map(f32::to_bits),
        }
    }
}

/// Screen-space paint is reusable only for the same view, Canvas and target extent.
pub(super) struct ProjectedPaint {
    pub(super) content: CanvasContent,
    pub(super) view: Option<CanvasSurfaceView>,
    pub(super) size: [u32; 2],
    pub(super) frame: u64,
    pub(super) image: Option<PaintImage>,
}

#[derive(Clone)]
pub(super) struct PaintImage {
    pub(super) target: Rc<Target>,
    pub(super) bounds: [f32; 4],
}

impl ProjectedPaint {
    pub(super) fn content(
        scene: CanvasScene<'_>,
        entries: &[Arc<CanvasPaintEntry>],
    ) -> Option<CanvasContent> {
        let live = |entry: &Arc<CanvasPaintEntry>| match entry.as_ref() {
            CanvasPaintEntry::Primitive {
                primitive,
                ..
            } => matches!(primitive, CanvasPrimitive::Html { .. }),
            CanvasPaintEntry::Attachment(_) => true,
        };
        if entries.len() < 2
            || scene.canvas.interaction.requires_direct()
            || entries.iter().any(live)
        {
            return None;
        }

        // The root viewport has no world-space lighting. On a Surface the key
        // also contains the exact presentation transform and lighting used by authored paints.
        Some(CanvasContent::from_scene(scene))
    }
}

pub(super) struct CachedCanvas {
    pub(super) target: Rc<Target>,
    pub(super) content: Option<CanvasContent>,
    pub(super) damage: Option<CanvasDamage>,
    pub(super) publication: WorldPublicationId,
    pub(super) frame: u64,
    pub(super) time: f64,
    pub(super) band: u8,
}

pub(super) use ipp_render_scene::canvas::projected_size;
