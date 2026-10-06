//! Renderer-owned GPU providers for immutable Surface resources.

use std::any::Any;

use ipp_core::services::asset_management::{
    Asset, AssetLoadFailure, AssetLoader, AsyncAssetLoader,
    formats::drawing::DrawingAsset,
    formats::font::FontAsset,
    formats::quadratic::{QuadraticContour, QuadraticSegment},
};

use super::super::statistics::RenderUploadCounter;
use super::context::{RenderAssetContext, RenderAssetLease};
use super::{loaders::SharedRenderDevice, paths};
use crate::RenderDevice;

pub(in crate::services::render) struct GlFontData<D: RenderDevice> {
    pub font: FontAsset,
    asset_lease: RenderAssetLease,
    pub path: Option<D::SurfacePath>,
    pub ranges: Vec<super::super::device::SurfacePathDescriptor>,
    pub glyph_bounds: Vec<[f32; 4]>,
    device: SharedRenderDevice<D>,
    gpu_bytes: usize,
    graphics_prepared: bool,
}

impl<D: RenderDevice> Asset for GlFontData<D> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        &self.font
    }

    fn graphics_ready(&self) -> Option<bool> {
        Some(self.graphics_prepared)
    }

    fn graphics_bytes(&self) -> Option<usize> {
        Some(self.gpu_bytes)
    }

    fn resident_bytes(&self) -> usize {
        self.font.resident_bytes() + self.gpu_bytes
    }

    fn invalidate_graphics(&mut self) {
        let mut device = self.device.borrow_mut();
        if let Some(path) = self.path.take()
            && self.asset_lease.is_current()
        {
            device.delete_surface_path(path);
        }
        self.gpu_bytes = 0;
        self.graphics_prepared = false;
    }
}

impl<D: RenderDevice> Drop for GlFontData<D> {
    fn drop(&mut self) {
        self.invalidate_graphics();
    }
}

pub(in crate::services::render) struct GlDrawingData<D: RenderDevice> {
    pub drawing: DrawingAsset,
    asset_lease: RenderAssetLease,
    pub path: Option<D::SurfacePath>,
    pub ranges: Vec<super::super::device::SurfacePathDescriptor>,
    pub layer_bounds: Vec<[f32; 4]>,
    device: SharedRenderDevice<D>,
    gpu_bytes: usize,
    graphics_prepared: bool,
}

impl<D: RenderDevice> Asset for GlDrawingData<D> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        &self.drawing
    }

    fn graphics_ready(&self) -> Option<bool> {
        Some(self.graphics_prepared)
    }

    fn graphics_bytes(&self) -> Option<usize> {
        Some(self.gpu_bytes)
    }

    fn resident_bytes(&self) -> usize {
        self.drawing.resident_bytes() + self.gpu_bytes
    }

    fn invalidate_graphics(&mut self) {
        let mut device = self.device.borrow_mut();
        if let Some(path) = self.path.take()
            && self.asset_lease.is_current()
        {
            device.delete_surface_path(path);
        }
        self.gpu_bytes = 0;
        self.graphics_prepared = false;
    }
}

impl<D: RenderDevice> Drop for GlDrawingData<D> {
    fn drop(&mut self) {
        self.invalidate_graphics();
    }
}

pub(in crate::services::render) fn font_loader<D: RenderDevice>(
    device: SharedRenderDevice<D>,
    uploads: RenderUploadCounter,
    context: RenderAssetContext,
) -> impl AssetLoader<Data = GlFontData<D>> {
    AsyncAssetLoader::new(move |mut reader| async move {
        let font = FontAsset::decode_reader(&mut *reader).await?;
        let mut normalized = Vec::new();
        let mut glyph_bounds = Vec::new();
        let mut budget = ipp_core::services::asset_management::decode::DecodeBudget::default();
        for glyph in font.glyphs() {
            normalized.push(normalize_glyph_contours_async(&glyph.contours).await);
            let bounds = glyph.bounds;
            glyph_bounds.push([bounds[0], -bounds[3], bounds[2], -bounds[1]]);
            budget.advance(0).await;
        }
        let atlas = paths::pack_surface_paths_async(
            glyph_bounds
                .iter()
                .zip(&normalized)
                .map(|(&bounds, contours)| (bounds, contours.as_slice())),
        )
        .await;
        let bytes = atlas.texels.byte_len();
        loop {
            let asset_lease = if atlas.texels.curves.is_empty() {
                context.lease()
            } else {
                context.wait().await
            };
            let result = if atlas.texels.curves.is_empty() {
                Ok(None)
            } else {
                device
                    .borrow_mut()
                    .create_surface_path(&atlas.texels)
                    .map(Some)
            };
            if matches!(result, Err(crate::RenderError::ContextLost)) {
                context.set_active(false);
                continue;
            }
            match result {
                Ok(path) => {
                    uploads.add(bytes);
                    return Ok(GlFontData {
                        font,
                        asset_lease,
                        path,
                        ranges: atlas.descriptors,
                        glyph_bounds,
                        device,
                        gpu_bytes: bytes,
                        graphics_prepared: true,
                    });
                }
                Err(error) => {
                    return Err(AssetLoadFailure::with_decoded(
                        error.to_string(),
                        GlFontData {
                            font,
                            asset_lease,
                            path: None,
                            ranges: atlas.descriptors,
                            glyph_bounds,
                            device,
                            gpu_bytes: 0,
                            graphics_prepared: false,
                        },
                    ));
                }
            }
        }
    })
}

async fn normalize_glyph_contours_async(contours: &[QuadraticContour]) -> Vec<QuadraticContour> {
    let mut output = Vec::with_capacity(contours.len());
    let mut budget = ipp_core::services::asset_management::decode::DecodeBudget::default();
    for contour in contours {
        let mut segments = Vec::with_capacity(contour.segments.len());
        for segment in &contour.segments {
            segments.push(match *segment {
                QuadraticSegment::Line {
                    to,
                } => QuadraticSegment::Line {
                    to: [to[0], -to[1]],
                },
                QuadraticSegment::Quadratic {
                    control,
                    to,
                } => QuadraticSegment::Quadratic {
                    control: [control[0], -control[1]],
                    to: [to[0], -to[1]],
                },
            });
            budget.advance(0).await;
        }
        output.push(QuadraticContour {
            start: [contour.start[0], -contour.start[1]],
            segments,
        });
    }
    output
}

pub(in crate::services::render) fn drawing_loader<D: RenderDevice>(
    device: SharedRenderDevice<D>,
    uploads: RenderUploadCounter,
    context: RenderAssetContext,
) -> impl AssetLoader<Data = GlDrawingData<D>> {
    AsyncAssetLoader::new(move |mut reader| async move {
        let drawing = DrawingAsset::decode_reader(&mut *reader).await?;
        let mut layer_bounds = Vec::new();
        let mut budget = ipp_core::services::asset_management::decode::DecodeBudget::default();
        for layer in drawing.layers() {
            layer_bounds.push(compute_layer_bounds_async(drawing.bounds(), layer).await);
            budget.advance(0).await;
        }
        let atlas = paths::pack_surface_paths_async(
            drawing
                .layers()
                .iter()
                .zip(&layer_bounds)
                .map(|(layer, &bounds)| (bounds, layer.contours.as_slice())),
        )
        .await;
        let bytes = atlas.texels.byte_len();
        loop {
            let asset_lease = if atlas.texels.curves.is_empty() {
                context.lease()
            } else {
                context.wait().await
            };
            let result = if atlas.texels.curves.is_empty() {
                Ok(None)
            } else {
                device
                    .borrow_mut()
                    .create_surface_path(&atlas.texels)
                    .map(Some)
            };
            if matches!(result, Err(crate::RenderError::ContextLost)) {
                context.set_active(false);
                continue;
            }
            match result {
                Ok(path) => {
                    uploads.add(bytes);
                    return Ok(GlDrawingData {
                        drawing,
                        asset_lease,
                        path,
                        ranges: atlas.descriptors,
                        layer_bounds,
                        device,
                        gpu_bytes: bytes,
                        graphics_prepared: true,
                    });
                }
                Err(error) => {
                    return Err(AssetLoadFailure::with_decoded(
                        error.to_string(),
                        GlDrawingData {
                            drawing,
                            asset_lease,
                            path: None,
                            ranges: atlas.descriptors,
                            layer_bounds,
                            device,
                            gpu_bytes: 0,
                            graphics_prepared: false,
                        },
                    ));
                }
            }
        }
    })
}

async fn compute_layer_bounds_async(
    drawing_bounds: [f32; 4],
    layer: &ipp_core::services::asset_management::formats::drawing::DrawingLayer,
) -> [f32; 4] {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    let mut budget = ipp_core::services::asset_management::decode::DecodeBudget::default();
    for contour in &layer.contours {
        budget.advance(0).await;
        min_x = min_x.min(contour.start[0]);
        min_y = min_y.min(contour.start[1]);
        max_x = max_x.max(contour.start[0]);
        max_y = max_y.max(contour.start[1]);

        for segment in &contour.segments {
            budget.advance(0).await;
            match segment {
                ipp_core::services::asset_management::formats::quadratic::QuadraticSegment::Line {
                    to,
                } => {
                    min_x = min_x.min(to[0]);
                    min_y = min_y.min(to[1]);
                    max_x = max_x.max(to[0]);
                    max_y = max_y.max(to[1]);
                }
                ipp_core::services::asset_management::formats::quadratic::QuadraticSegment::Quadratic {
                    control,
                    to,
                } => {
                    min_x = min_x.min(control[0]).min(to[0]);
                    min_y = min_y.min(control[1]).min(to[1]);
                    max_x = max_x.max(control[0]).max(to[0]);
                    max_y = max_y.max(control[1]).max(to[1]);
                }
            }
        }
    }

    if min_x >= max_x || min_y >= max_y {
        drawing_bounds
    } else {
        [
            min_x.max(drawing_bounds[0]),
            min_y.max(drawing_bounds[1]),
            max_x.min(drawing_bounds[2]),
            max_y.min(drawing_bounds[3]),
        ]
    }
}

#[cfg(test)]
#[path = "surface_resources_tests.rs"]
mod tests;
