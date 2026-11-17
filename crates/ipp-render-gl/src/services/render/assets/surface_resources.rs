//! Renderer-owned GPU providers for immutable Surface resources.

use std::any::Any;

use ipp_core::services::asset_management::{
    Asset, AssetLoadFailure, AssetLoader, AsyncAssetLoader, formats::drawing::DrawingAsset,
    formats::font::FontAsset,
};

use super::super::statistics::RenderUploadCounter;
use super::context::{RenderAssetContext, RenderAssetLease};
use super::{loaders::SharedRenderDevice, paths};
use crate::RenderDevice;

pub(in crate::services::render) struct GlFontData<D: RenderDevice> {
    pub font: std::rc::Rc<FontAsset>,
    pub curves: std::rc::Rc<std::cell::RefCell<super::font_curves::FontCurves<D>>>,
    pub(super) graphics_prepared: bool,
}

impl<D: RenderDevice> GlFontData<D> {
    pub fn glyph_bounds(&self, id: u32) -> Option<[f32; 4]> {
        let [left, bottom, right, top] = self.font.glyph(id)?.bounds;
        Some([left, -top, right, -bottom])
    }
}

impl<D: RenderDevice> Asset for GlFontData<D> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        self.font.as_ref()
    }

    fn graphics_ready(&self) -> Option<bool> {
        Some(self.graphics_prepared)
    }

    fn graphics_bytes(&self) -> Option<usize> {
        Some(self.curves.borrow().bytes())
    }

    fn resident_bytes(&self) -> usize {
        self.font.resident_bytes() + self.curves.borrow().bytes()
    }

    fn invalidate_graphics(&mut self) {
        self.curves.borrow_mut().invalidate();
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
        let font = std::rc::Rc::new(FontAsset::decode_reader(&mut *reader).await?);
        let has_curves = font.glyphs().iter().any(|glyph| !glyph.contours.is_empty());
        let empty = super::font_curves::empty_texels();
        loop {
            let lease = if has_curves {
                context.wait().await
            } else {
                context.lease()
            };
            let result = if has_curves {
                device.borrow_mut().create_surface_path(&empty).map(Some)
            } else {
                Ok(None)
            };
            if matches!(result, Err(crate::RenderError::ContextLost)) {
                context.set_active(false);
                continue;
            }
            let (path, failure) = match result {
                Ok(path) => {
                    if path.is_some() {
                        uploads.add(empty.byte_len());
                    }
                    (path, None)
                }
                Err(error) => (None, Some(error)),
            };
            let data = GlFontData {
                font: font.clone(),
                graphics_prepared: failure.is_none(),
                curves: std::rc::Rc::new(std::cell::RefCell::new(
                    super::font_curves::FontCurves::new(
                        std::rc::Rc::downgrade(&font),
                        device.clone(),
                        lease,
                        uploads.clone(),
                        path,
                    ),
                )),
            };
            return match failure {
                Some(error) => Err(AssetLoadFailure::with_decoded(error.to_string(), data)),
                None => Ok(data),
            };
        }
    })
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
