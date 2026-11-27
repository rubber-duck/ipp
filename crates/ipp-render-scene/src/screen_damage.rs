//! Conservative screen-space damage for a persistent target containing a single Canvas.
//! A changed view, attachment lifetime, shader property or resource invalidates the target.
use crate::{
    analytic_surface::AnalyticSurface,
    canvas::{CanvasProjection, CanvasScene, corners, surface_projection},
    canvas_damage::paint_bounds,
    lighting::Lighting,
};
use ipp_core::{
    HostRuntime, OutputRef,
    systems::{
        canvas::{CanvasAffine, CanvasClip, CanvasPaintEntry, CanvasPrimitive},
        surface::SurfaceGeometry,
    },
};
use std::sync::Arc;

#[derive(Clone, PartialEq)]
/// Complete camera and surface inputs that can move or shade the Canvas.
pub struct CanvasSurfaceView {
    /// Selected Camera output.
    pub camera: OutputRef,
    /// Camera view-projection matrix.
    pub projection: [f32; 16],
    /// World placement of the Surface.
    pub model: [f32; 16],
    /// Projection provider and physical extent.
    pub geometry: SurfaceGeometry,
    /// Resolved lighting for this Surface.
    pub lighting: Lighting,
}

type Rect = [u32; 4];

/// Last complete contents of a renderer-owned presentation target. Discard when replacing the target.
pub struct ScreenDamage {
    output: OutputRef,
    size: [u32; 2],
    extent: [f32; 2],
    view: Option<CanvasSurfaceView>,
    resources: u64,
    paints: u64,
    entries: Arc<[Arc<CanvasPaintEntry>]>,
    bounds: Vec<Rect>,
}

/// A framebuffer scissor and the current painters that overlap it in publication order.
pub struct Repaint {
    /// Top-left pixel origin followed by width and height.
    pub scissor: Rect,
    /// Canvas entries to replay within the scissor.
    pub indices: Vec<usize>,
}

impl ScreenDamage {
    /// Compare the new Canvas and view with the contents of the same target.
    pub fn prepare(
        old: Option<Self>,
        host: &HostRuntime,
        scene: CanvasScene<'_>,
        size: [u32; 2],
        view: Option<CanvasSurfaceView>,
    ) -> Option<(Self, Option<Repaint>)> {
        let canvas = scene.canvas;
        let extent = canvas.logical_extent;
        let stable = old.filter(|old| {
            old.output == canvas.selection
                && old.size == size
                && old.extent == extent
                && old.view == view
                && old.resources == canvas.resource_revision
                && old.paints == canvas.paints_revision
        });
        if stable
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(&old.entries, &canvas.entries))
        {
            return stable.map(|old| (old, None));
        }
        if scene.canvas.entries.iter().any(|e| {
            matches!(
                e.as_ref(),
                CanvasPaintEntry::Attachment(_)
                    | CanvasPaintEntry::Primitive {
                        primitive: CanvasPrimitive::Html { .. },
                        ..
                    }
            )
        }) {
            return None;
        }
        let stable = stable.as_ref();
        let mut changed = None;
        let mut bounds = Vec::with_capacity(canvas.entries.len());
        let scale = view.as_ref().map_or([1.; 2], |v| {
            std::array::from_fn(|i| v.geometry.physical_extent()[i] / f64::from(extent[i]))
        });
        let analytic = view.as_ref().and_then(|v| {
            AnalyticSurface::prepare(&v.geometry, v.projection, v.model, scale, size)
        });
        let projection = view.as_ref().map_or_else(
            || CanvasProjection::viewport(extent),
            |v| surface_projection(v.projection, v.model, &v.geometry, scale, 0.),
        );
        // A curved provider without an analytic enclosure can extend past its projected corners.
        if analytic.is_none() && matches!(projection, CanvasProjection::Surface { .. }) {
            return None;
        }
        for (i, entry) in canvas.entries.iter().enumerate() {
            if let Some(old) = stable
                && old.entries.get(i).is_some_and(|e| Arc::ptr_eq(e, entry))
            {
                bounds.push(old.bounds[i]);
                continue;
            }
            let mut logical = paint_bounds(host, scene, entry.as_ref());
            for i in 0..2 {
                logical[i] = logical[i].clamp(0., extent[i]);
                logical[i + 2] = logical[i + 2].clamp(0., extent[i]);
            }
            let rect = screen_rect(logical, size, &projection, analytic.as_ref());
            extend(&mut changed, rect);
            if let Some(rect) = stable.and_then(|old| old.bounds.get(i)) {
                extend(&mut changed, *rect);
            }
            bounds.push(rect);
        }
        if let Some(old) = stable {
            for rect in old.bounds.iter().skip(bounds.len()) {
                extend(&mut changed, *rect);
            }
        }
        let repaint = stable.and_then(|_| {
            let [l, t, r, b] = changed?;
            if u64::from(r - l) * u64::from(b - t) * 5 > u64::from(size[0]) * u64::from(size[1]) * 3
            {
                return None;
            }
            let indices = bounds
                .iter()
                .enumerate()
                .filter_map(|(i, &[x, y, z, w])| (x < r && z > l && y < b && w > t).then_some(i))
                .collect();
            Some(Repaint {
                scissor: [l, t, r - l, b - t],
                indices,
            })
        });
        Some((
            Self {
                output: canvas.selection,
                size,
                extent,
                view,
                resources: canvas.resource_revision,
                paints: canvas.paints_revision,
                entries: canvas.entries.clone(),
                bounds,
            },
            repaint,
        ))
    }
}

fn extend(rect: &mut Option<Rect>, [l, t, r, b]: Rect) {
    if r > l && b > t {
        *rect = Some(rect.map_or([l, t, r, b], |v| {
            [v[0].min(l), v[1].min(t), v[2].max(r), v[3].max(b)]
        }));
    }
}

fn screen_rect(
    rect: CanvasClip,
    size: [u32; 2],
    projection: &CanvasProjection<'_>,
    analytic: Option<&AnalyticSurface<'_>>,
) -> Rect {
    if rect[0] >= rect[2] || rect[1] >= rect[3] {
        return [0; 4];
    }
    let points = corners(rect);
    let whole = [0, 0, size[0], size[1]];
    let cover = if let Some(surface) = analytic {
        let Some(primitive) = surface.draw(&points, rect, CanvasAffine::IDENTITY) else {
            return whole;
        };
        primitive.draw.cover()
    } else {
        let mut cover = [[0.; 2]; 4];
        for (point, out) in points.into_iter().zip(&mut cover) {
            let Some(v) = projection.vertex(point, point) else {
                return whole;
            };
            if v.clip[3] <= 0. {
                return whole;
            }
            *out = [v.clip[0] / v.clip[3], v.clip[1] / v.clip[3]];
        }
        cover
    };
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    for [x, y] in cover {
        let p = [
            (x + 1.) * 0.5 * size[0] as f32,
            (1. - y) * 0.5 * size[1] as f32,
        ];
        for i in 0..2 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    let axis = |i: usize, v: f32| v.clamp(0., size[i] as f32) as u32;
    [
        axis(0, lo[0].floor() - 4.),
        axis(1, lo[1].floor() - 4.),
        axis(0, hi[0].ceil() + 4.),
        axis(1, hi[1].ceil() + 4.),
    ]
}
