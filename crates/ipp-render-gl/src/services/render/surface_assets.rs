//! Renderer-owned GPU providers for immutable Surface resources.

use std::{any::Any, cell::Cell, rc::Rc, task::Poll};

use ipp_core::services::asset_management::{
    Asset, AssetLoader, BufferedAssetLoader, DataReader,
    drawing::DrawingAsset,
    font::FontAsset,
    quadratic::{QuadraticContour, QuadraticSegment},
};

use super::{assets::SharedRenderDevice, surface_path};
use crate::RenderDevice;

pub(super) struct GlFontData<D: RenderDevice> {
    pub font: FontAsset,
    pub path: Option<D::SurfacePath>,
    pub ranges: Vec<super::device::SurfacePathDescriptor>,
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
        if let Some(path) = self.path.take() {
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

pub(super) struct GlDrawingData<D: RenderDevice> {
    pub drawing: DrawingAsset,
    pub path: Option<D::SurfacePath>,
    pub ranges: Vec<super::device::SurfacePathDescriptor>,
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
        if let Some(path) = self.path.take() {
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

struct SurfaceLoader<D: RenderDevice, A: Asset> {
    decoder: BufferedAssetLoader<A>,
    device: SharedRenderDevice<D>,
    uploaded: Rc<Cell<u32>>,
    pending: Option<A>,
    failed_font: Option<GlFontData<D>>,
    failed_drawing: Option<GlDrawingData<D>>,
}

pub(super) fn font_loader<D: RenderDevice>(
    device: SharedRenderDevice<D>,
    uploaded: Rc<Cell<u32>>,
) -> impl AssetLoader<Data = GlFontData<D>> {
    SurfaceLoader {
        decoder: BufferedAssetLoader::new(|bytes| {
            FontAsset::decode(bytes).map_err(|error| error.to_string())
        }),
        device,
        uploaded,
        pending: None,
        failed_font: None,
        failed_drawing: None,
    }
}

impl<D: RenderDevice> AssetLoader for SurfaceLoader<D, FontAsset> {
    type Data = GlFontData<D>;

    fn take_failed_data(&mut self) -> Option<Self::Data> {
        self.failed_font.take()
    }

    fn poll_load(
        &mut self,
        reader: &mut dyn DataReader,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<Self::Data, String>> {
        let font = match self.pending.take() {
            Some(font) => font,
            None => match self.decoder.poll_load(reader, cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Ready(Ok(font)) => font,
            },
        };
        let normalized_contours: Vec<Vec<QuadraticContour>> = font
            .glyphs()
            .iter()
            .map(|glyph| normalize_glyph_contours(&glyph.contours))
            .collect();
        let glyph_bounds: Vec<[f32; 4]> = font
            .glyphs()
            .iter()
            .map(|glyph| {
                let b = glyph.bounds;
                [b[0], -b[3], b[2], -b[1]]
            })
            .collect();
        let atlas = surface_path::atlas(
            glyph_bounds
                .iter()
                .zip(&normalized_contours)
                .map(|(&bounds, contours)| (bounds, contours.as_slice())),
        );
        let bytes = atlas.curves.len() * 32 + atlas.bands.len() * 8;
        let path = if atlas.curves.is_empty() {
            None
        } else {
            match self.device.borrow_mut().create_surface_path(
                &[0.0; 4],
                &atlas.curves,
                &atlas.bands,
            ) {
                Ok(path) => Some(path),
                Err(crate::RenderError::ContextLost) => {
                    self.pending = Some(font);
                    return Poll::Pending;
                }
                Err(error) => {
                    self.failed_font = Some(GlFontData {
                        font,
                        path: None,
                        ranges: atlas.descriptors,
                        glyph_bounds,
                        device: self.device.clone(),
                        gpu_bytes: 0,
                        graphics_prepared: false,
                    });
                    return Poll::Ready(Err(error.to_string()));
                }
            }
        };
        self.uploaded
            .set(self.uploaded.get().saturating_add(bytes as u32));
        Poll::Ready(Ok(GlFontData {
            font,
            path,
            ranges: atlas.descriptors,
            glyph_bounds,
            device: self.device.clone(),
            gpu_bytes: bytes,
            graphics_prepared: true,
        }))
    }
}

fn normalize_glyph_contours(contours: &[QuadraticContour]) -> Vec<QuadraticContour> {
    contours
        .iter()
        .map(|c| QuadraticContour {
            start: [c.start[0], -c.start[1]],
            segments: c
                .segments
                .iter()
                .map(|s| match *s {
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
                })
                .collect(),
        })
        .collect()
}

pub(super) fn drawing_loader<D: RenderDevice>(
    device: SharedRenderDevice<D>,
    uploaded: Rc<Cell<u32>>,
) -> impl AssetLoader<Data = GlDrawingData<D>> {
    SurfaceLoader {
        decoder: BufferedAssetLoader::new(|bytes| {
            DrawingAsset::decode(bytes).map_err(|error| error.to_string())
        }),
        device,
        uploaded,
        pending: None,
        failed_font: None,
        failed_drawing: None,
    }
}

impl<D: RenderDevice> AssetLoader for SurfaceLoader<D, DrawingAsset> {
    type Data = GlDrawingData<D>;

    fn take_failed_data(&mut self) -> Option<Self::Data> {
        self.failed_drawing.take()
    }

    fn poll_load(
        &mut self,
        reader: &mut dyn DataReader,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<Self::Data, String>> {
        let drawing = match self.pending.take() {
            Some(drawing) => drawing,
            None => match self.decoder.poll_load(reader, cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Ready(Ok(drawing)) => drawing,
            },
        };
        let atlas = surface_path::atlas(
            drawing
                .layers()
                .iter()
                .map(|layer| (drawing.bounds(), layer.contours.as_slice())),
        );
        let bytes = atlas.curves.len() * 32 + atlas.bands.len() * 8;
        let path = if atlas.curves.is_empty() {
            None
        } else {
            match self.device.borrow_mut().create_surface_path(
                &[0.0; 4],
                &atlas.curves,
                &atlas.bands,
            ) {
                Ok(path) => Some(path),
                Err(crate::RenderError::ContextLost) => {
                    self.pending = Some(drawing);
                    return Poll::Pending;
                }
                Err(error) => {
                    self.failed_drawing = Some(GlDrawingData {
                        drawing,
                        path: None,
                        ranges: atlas.descriptors,
                        device: self.device.clone(),
                        gpu_bytes: 0,
                        graphics_prepared: false,
                    });
                    return Poll::Ready(Err(error.to_string()));
                }
            }
        };
        self.uploaded
            .set(self.uploaded.get().saturating_add(bytes as u32));
        Poll::Ready(Ok(GlDrawingData {
            drawing,
            path,
            ranges: atlas.descriptors,
            device: self.device.clone(),
            gpu_bytes: bytes,
            graphics_prepared: true,
        }))
    }
}

#[cfg(test)]
#[path = "surface_assets_tests.rs"]
mod tests;
