//! Renderer-owned images for authored optional Surface caching.

use std::rc::Rc;

use ipp_core::{
    HostRuntime, WorldPublicationId,
    systems::{
        canvas::CanvasPaintEntry,
        geometry::GeometryShape,
        surface::{SurfaceCachePolicy, SurfaceGeometry},
    },
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
        let revisions = [
            canvas.layout_revision,
            canvas.paint_revision,
            canvas.resource_revision,
            canvas.paints_revision,
        ];
        Some(Self {
            revisions,
            extent: canvas.logical_extent.map(f32::to_bits),
        })
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

pub(super) fn projected_size(
    geometry: &SurfaceGeometry,
    matrix: [f32; 16],
    viewport: [u32; 2],
    policy: SurfaceCachePolicy,
) -> Option<[u32; 2]> {
    let GeometryShape::Box {
        min,
        max,
    } = geometry.bounds([0.0; 2]).ok()?
    else {
        return None;
    };
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    let mut in_front = false;
    let mut behind = false;
    for corner in 0..8 {
        let p: [f32; 3] = std::array::from_fn(|i| {
            if corner & (1 << i) == 0 {
                min[i] as f32
            } else {
                max[i] as f32
            }
        });
        let clip: [f32; 4] = std::array::from_fn(|i| {
            matrix[12 + i] + matrix[i] * p[0] + matrix[4 + i] * p[1] + matrix[8 + i] * p[2]
        });
        if clip[3] <= 1e-6 {
            behind = true;
            continue;
        }
        in_front = true;
        for i in 0..2 {
            let ndc = clip[i] / clip[3];
            low[i] = low[i].min(ndc);
            high[i] = high[i].max(ndc);
        }
    }
    if !in_front || (0..2).any(|i| low[i] > 1.0 || high[i] < -1.0) {
        return None;
    }

    let limit = policy.texture_limit(2048);
    Some(std::array::from_fn(|i| {
        let span = if behind {
            2.0
        } else {
            high[i] - low[i]
        };
        let pixels = (viewport[i] as f32 * span * 0.5 * policy.resolution_scale)
            .ceil()
            .clamp(1.0, limit as f32) as u32;
        // A modest allocation step absorbs small changes in projected support.
        pixels.next_multiple_of(32).min(limit)
    }))
}
