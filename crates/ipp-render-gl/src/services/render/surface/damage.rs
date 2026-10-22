//! Damage against the entries that actually painted an image, including skipped
//! publications. These immutable references describe derived pixels, not World state.

use super::super::assets::surface_resources::{GlDrawingData, GlFontData};
use super::super::canvas::scene::CanvasScene;
use crate::RenderDevice;
use ipp_core::systems::canvas::{CanvasClip, CanvasPaintEntry, CanvasPrimitive};
use std::sync::Arc;

pub(in crate::services::render) struct CanvasImageDamage {
    entries: Arc<[Arc<CanvasPaintEntry>]>,
    bounds: Vec<CanvasClip>,
    extent: [f32; 2],
    size: [u32; 2],
    clip: CanvasClip,
    opacity: f32,
    resources: u64,
    paints: u64,
}

impl CanvasImageDamage {
    /// Moving content to another existing plane must refresh its cutout even
    /// when ordinary content changes are waiting for a distant refresh interval.
    pub fn ownership_changed(&self, scene: &CanvasScene<'_>) -> bool {
        self.entries.len() != scene.canvas.entries.len()
            || self
                .entries
                .iter()
                .zip(scene.canvas.entries.iter())
                .any(|(old, new)| old.layer() != new.layer())
    }

    pub fn capture<D: RenderDevice>(
        scene: &CanvasScene<'_>,
        size: [u32; 2],
        clip: CanvasClip,
        opacity: f32,
        previous: Option<&Self>,
    ) -> Self {
        let bounds = scene
            .canvas
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                previous
                    .filter(|old| {
                        old.size == size && old.resources == scene.canvas.resource_revision
                    })
                    .and_then(|old| {
                        old.entries
                            .get(index)
                            .filter(|old_entry| Arc::ptr_eq(old_entry, entry))
                            .map(|_| old.bounds[index])
                    })
                    .unwrap_or_else(|| bounds::<D>(scene, entry))
            })
            .collect();
        Self {
            entries: scene.canvas.entries.clone(),
            bounds,
            extent: scene.canvas.logical_extent,
            size,
            clip,
            opacity,
            resources: scene.canvas.resource_revision,
            paints: scene.canvas.paints_revision,
        }
    }

    /// One coalesced raster rectangle. Broad or uncertain damage uses a full
    /// repaint; sparse updates clear and replay intersecting paint in order.
    pub fn region(&self, old: Option<&Self>) -> Option<[u32; 4]> {
        let old = old?;
        if self.extent != old.extent
            || self.size != old.size
            || self.clip != old.clip
            || self.opacity != old.opacity
            || self.resources != old.resources
            || self.paints != old.paints
        {
            return None;
        }
        let mut damage: Option<CanvasClip> = None;
        for index in 0..self.entries.len().max(old.entries.len()) {
            if let (Some(a), Some(b)) = (self.entries.get(index), old.entries.get(index))
                && (Arc::ptr_eq(a, b) || a == b)
            {
                continue;
            }
            for bounds in [self.bounds.get(index), old.bounds.get(index)]
                .into_iter()
                .flatten()
            {
                if bounds[0] < bounds[2] && bounds[1] < bounds[3] {
                    union(&mut damage, *bounds);
                }
            }
        }
        let damage = damage?;
        let scale = [
            self.size[0] as f32 / self.extent[0],
            self.size[1] as f32 / self.extent[1],
        ];
        // Covers clip AA, glyph atlas padding and path/shape pixel footprints.
        let start: [u32; 2] = std::array::from_fn(|i| {
            ((damage[i] * scale[i]).floor() - 4.0).clamp(0.0, self.size[i] as f32) as u32
        });
        let end: [u32; 2] = std::array::from_fn(|i| {
            ((damage[i + 2] * scale[i]).ceil() + 4.0).clamp(start[i] as f32, self.size[i] as f32)
                as u32
        });
        let area = u64::from(end[0] - start[0]) * u64::from(end[1] - start[1]);
        (area * 5 < u64::from(self.size[0]) * u64::from(self.size[1]) * 3).then_some([
            start[0],
            start[1],
            end[0] - start[0],
            end[1] - start[1],
        ])
    }

    pub fn logical_region(&self, rect: [u32; 4]) -> CanvasClip {
        [
            rect[0] as f32 * self.extent[0] / self.size[0] as f32,
            rect[1] as f32 * self.extent[1] / self.size[1] as f32,
            (rect[0] + rect[2]) as f32 * self.extent[0] / self.size[0] as f32,
            (rect[1] + rect[3]) as f32 * self.extent[1] / self.size[1] as f32,
        ]
    }
}

fn union(result: &mut Option<CanvasClip>, rect: CanvasClip) {
    *result = Some(result.map_or(rect, |old| {
        [
            old[0].min(rect[0]),
            old[1].min(rect[1]),
            old[2].max(rect[2]),
            old[3].max(rect[3]),
        ]
    }));
}

fn bounds<D: RenderDevice>(scene: &CanvasScene<'_>, entry: &CanvasPaintEntry) -> CanvasClip {
    let CanvasPaintEntry::Primitive {
        primitive,
        ..
    } = entry
    else {
        return scene.root_clip();
    };
    let style = primitive.style();
    let place = |rect: CanvasClip| {
        let a = [
            style.position[0] + rect[0] * style.scale[0],
            style.position[1] + rect[1] * style.scale[1],
        ];
        let b = [
            style.position[0] + rect[2] * style.scale[0],
            style.position[1] + rect[3] * style.scale[1],
        ];
        [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[0].max(b[0]),
            a[1].max(b[1]),
        ]
    };
    let rect = match primitive {
        CanvasPrimitive::Box {
            style,
            size,
            corner_radius,
            border_width,
            border_color,
            fill,
            glow,
            shape,
        } => {
            let records = super::super::retained::box_records::generate_box_records(
                style,
                size,
                corner_radius,
                *border_width,
                border_color,
                fill,
                glow.as_ref(),
                shape,
                style.clip,
            );
            let mut rect = None;
            for record in records {
                union(&mut rect, record.rect);
            }
            rect.unwrap_or([0.0; 4])
        }
        CanvasPrimitive::Bitmap {
            size,
            ..
        }
        | CanvasPrimitive::Html {
            size,
            ..
        } => place([0.0, 0.0, size[0], size[1]]),
        CanvasPrimitive::Path {
            path,
            ..
        } => place(path.bounds),
        CanvasPrimitive::Drawing {
            drawing,
            ..
        } => {
            let Some(data) = scene
                .resource(*drawing)
                .and_then(|resource| resource.data())
                .and_then(|asset| asset.as_any().downcast_ref::<GlDrawingData<D>>())
            else {
                return scene.root_clip();
            };
            place(data.drawing.bounds())
        }
        CanvasPrimitive::Glyphs {
            font,
            font_size,
            glyphs,
            ..
        } => {
            let Some(data) = scene
                .resource(*font)
                .and_then(|resource| resource.data())
                .and_then(|asset| asset.as_any().downcast_ref::<GlFontData<D>>())
            else {
                return scene.root_clip();
            };
            let unit = *font_size / data.font.units_per_em() as f32;
            let mut rect = None;
            for glyph in glyphs.iter() {
                let Some(b) = data.glyph_bounds.get(glyph.glyph_id as usize) else {
                    return scene.root_clip();
                };
                union(
                    &mut rect,
                    place([
                        glyph.position[0] + b[0] * unit,
                        glyph.position[1] + b[1] * unit,
                        glyph.position[0] + b[2] * unit,
                        glyph.position[1] + b[3] * unit,
                    ]),
                );
            }
            rect.unwrap_or([0.0; 4])
        }
    };
    [
        rect[0].max(style.clip[0]),
        rect[1].max(style.clip[1]),
        rect[2].min(style.clip[2]),
        rect[3].min(style.clip[3]),
    ]
}
