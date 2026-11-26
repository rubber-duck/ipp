//! Damage relative to the entries that actually painted an eligible Canvas image.

use std::sync::Arc;

use ipp_core::{
    HostRuntime,
    services::asset_management::formats::{drawing::DrawingAsset, font::FontAsset},
    systems::{
        canvas::{CanvasClip, CanvasPaintEntry, CanvasPrimitive},
        plot::PlotPathGeometry,
    },
};

use crate::canvas::{CanvasScene, decoded};

#[derive(Clone)]
pub(super) struct CanvasDamage {
    entries: Arc<[Arc<CanvasPaintEntry>]>,
    bounds: Arc<[CanvasClip]>,
    extent: [f32; 2],
    size: [u32; 2],
    resources: u64,
    paints: u64,
}

impl CanvasDamage {
    pub(super) fn capture(
        host: &HostRuntime,
        scene: CanvasScene<'_>,
        size: [u32; 2],
        old: Option<&Self>,
    ) -> Self {
        let entries = &scene.canvas.entries;
        let resources = scene.canvas.resource_revision;
        let previous = old.filter(|item| item.resources == resources);
        let bounds = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                preceding_bound(previous, i, entry)
                    .unwrap_or_else(|| paint_bounds(host, scene, entry))
            })
            .collect();

        Self {
            entries: entries.clone(),
            bounds,
            extent: scene.canvas.logical_extent,
            size,
            resources,
            paints: scene.canvas.paints_revision,
        }
    }

    pub(super) fn region(&self, old: Option<&Self>) -> Option<[u32; 4]> {
        let old = old?;
        if self.size != old.size
            || self.extent != old.extent
            || self.resources != old.resources
            || self.paints != old.paints
        {
            return None;
        }

        let mut damage = None;
        for i in 0..self.entries.len().max(old.entries.len()) {
            if let (Some(next), Some(previous)) = (self.entries.get(i), old.entries.get(i))
                && Arc::ptr_eq(next, previous)
            {
                continue;
            }
            for bounds in [old.bounds.get(i), self.bounds.get(i)]
                .into_iter()
                .flatten()
            {
                extend(&mut damage, *bounds);
            }
        }

        let damage = damage?;
        let scale =
            std::array::from_fn::<_, 2, _>(|axis| self.size[axis] as f32 / self.extent[axis]);
        // Keep derivative-based antialiasing at the scissor edge outside
        // the changing paint. The Canvas transforms and clips do not change.
        let start = std::array::from_fn::<_, 2, _>(|axis| {
            (damage[axis]
                .mul_add(scale[axis], -4.0)
                .floor()
                .clamp(0.0, self.size[axis] as f32)) as u32
        });
        let end = std::array::from_fn::<_, 2, _>(|axis| {
            (damage[axis + 2]
                .mul_add(scale[axis], 4.0)
                .ceil()
                .clamp(start[axis] as f32, self.size[axis] as f32)) as u32
        });
        let [width, height] = [end[0] - start[0], end[1] - start[1]];
        let area = u64::from(width) * u64::from(height);
        let full = u64::from(self.size[0]) * u64::from(self.size[1]);
        (area > 0 && area * 5 < full * 3).then_some([start[0], start[1], width, height])
    }

    pub(super) fn entries(
        &self,
        [x, y, width, height]: [u32; 4],
    ) -> impl Iterator<Item = &CanvasPaintEntry> {
        let [sx, sy] = std::array::from_fn(|axis| self.extent[axis] / self.size[axis] as f32);
        let left = (x as f32 - 4.0) * sx;
        let top = (y as f32 - 4.0) * sy;
        let right = ((x + width) as f32 + 4.0) * sx;
        let bottom = ((y + height) as f32 + 4.0) * sy;
        self.entries
            .iter()
            .zip(self.bounds.iter())
            .filter_map(move |(entry, bounds)| {
                (bounds[0] < right && bounds[1] < bottom && bounds[2] > left && bounds[3] > top)
                    .then_some(entry.as_ref())
            })
    }
}

fn preceding_bound(
    old: Option<&CanvasDamage>,
    index: usize,
    entry: &Arc<CanvasPaintEntry>,
) -> Option<CanvasClip> {
    let old = old?;
    old.entries
        .get(index)
        .filter(|&value| Arc::ptr_eq(value, entry))?;
    old.bounds.get(index).copied()
}

fn extend(result: &mut Option<CanvasClip>, rect: CanvasClip) {
    if rect[0] < rect[2] && rect[1] < rect[3] {
        *result = Some(result.map_or(rect, |prior| {
            [
                prior[0].min(rect[0]),
                prior[1].min(rect[1]),
                prior[2].max(rect[2]),
                prior[3].max(rect[3]),
            ]
        }));
    }
}

fn paint_bounds(
    host: &HostRuntime,
    scene: CanvasScene<'_>,
    entry: &CanvasPaintEntry,
) -> CanvasClip {
    let full = [
        0.0,
        0.0,
        scene.canvas.logical_extent[0],
        scene.canvas.logical_extent[1],
    ];
    let CanvasPaintEntry::Primitive {
        primitive,
        space,
        ..
    } = entry
    else {
        return full;
    };
    let style = primitive.style();
    let place = |rect: CanvasClip| {
        let a = std::array::from_fn::<_, 2, _>(|i| style.position[i] + rect[i] * style.scale[i]);
        let b =
            std::array::from_fn::<_, 2, _>(|i| style.position[i] + rect[i + 2] * style.scale[i]);
        [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[0].max(b[0]),
            a[1].max(b[1]),
        ]
    };
    let [left, top, right, bottom] = match primitive {
        CanvasPrimitive::Box {
            size,
            glow,
            ..
        } => {
            let [l, t, r, b] = place([0.0, 0.0, size[0], size[1]]);
            let scale = style.scale[0].abs().max(style.scale[1].abs());
            let radius = glow.map_or(0.0, |glow| glow.radius * scale) + 2.0;
            [l - radius, t - radius, r + radius, b + radius]
        }
        CanvasPrimitive::Path {
            path,
            ..
        } => {
            let margin = match path.geometry {
                PlotPathGeometry::Stroke {
                    radius,
                    ..
                } => radius,
                _ => 0.0,
            };
            let [l, t, r, b] = path.bounds;
            place([l - margin, t - margin, r + margin, b + margin])
        }
        CanvasPrimitive::Bitmap {
            size,
            ..
        } => place([0.0, 0.0, size[0], size[1]]),
        CanvasPrimitive::Html {
            ..
        } => return full,
        CanvasPrimitive::Drawing {
            drawing,
            ..
        } => {
            let Some(asset) = decoded::<DrawingAsset>(host, scene.world.id, *drawing) else {
                return full;
            };
            let [l, t, r, b] = asset.bounds();
            place([l, -b, r, -t])
        }
        CanvasPrimitive::Glyphs {
            font,
            font_size,
            glyphs,
            ..
        } => {
            let Some(asset) = decoded::<FontAsset>(host, scene.world.id, *font) else {
                return full;
            };
            let scale = *font_size / asset.units_per_em() as f32;
            let mut rect = None;
            for glyph in glyphs.iter() {
                if let Some(outline) = asset.glyph(glyph.glyph_id) {
                    let [l, t, r, b] = outline.bounds;
                    let [x, y] = glyph.position;
                    extend(
                        &mut rect,
                        place([x + l * scale, y - b * scale, x + r * scale, y - t * scale]),
                    );
                }
            }
            rect.unwrap_or([0.0; 4])
        }
    };

    let local = [
        left.max(style.clip[0]),
        top.max(style.clip[1]),
        right.min(style.clip[2]),
        bottom.min(style.clip[3]),
    ];
    if local[0] >= local[2] || local[1] >= local[3] {
        return [0.0; 4];
    }
    space
        .as_ref()
        .map_or(local, |space| space.affine.bounds(local))
}
