//! Plot-owned physical planes through shared Canvas shape, glyph and path machinery.

use super::super::{
    outputs::scene::{RenderScene, ScenePlotPlane},
    retained::{
        analytic_glyphs::AnalyticGlyphCache,
        shape_batches::GuiBatchRenderCache,
        surface_paint::{CANVAS_SURFACE, SurfacePaint},
    },
    statistics::RenderFrameWork,
};
use crate::{RenderDevice, RenderError, RenderService};
use ipp_core::systems::{
    canvas::{CanvasPrimitive, CanvasTarget},
    plot::PlotPreparedGeometry,
};
use std::sync::{Arc, Weak};

pub(in crate::services::render) type PlotPlaneKey = (ipp_core::WorldRef, CanvasTarget, u32);

pub(in crate::services::render) struct PlotPlaneCache<D: RenderDevice> {
    pub source: Weak<PlotPreparedGeometry>,
    pub boxes: GuiBatchRenderCache<D>,
    pub glyphs: AnalyticGlyphCache<D>,
}

impl<D: RenderDevice> RenderService<D> {
    pub(in crate::services::render) fn draw_plot_plane(
        &mut self,
        scene: &RenderScene<'_>,
        plane: &ScenePlotPlane<'_>,
        view_projection: [f32; 16],
        stats: &mut RenderFrameWork,
    ) -> Result<(), RenderError> {
        let key = (plane.entity.world, plane.target, plane.plane.part);
        let device = &self.device;
        let cache = self
            .plot_plane_caches
            .entry(key)
            .or_insert_with(|| PlotPlaneCache {
                source: Weak::new(),
                boxes: GuiBatchRenderCache::new(device.clone()),
                glyphs: AnalyticGlyphCache::new(device.clone()),
            });
        let paint = SurfacePaint {
            revision: 1,
            opacity: 1.0,
            patched: 0,
            kept: 0,
            reusable: cache
                .source
                .upgrade()
                .is_some_and(|source| Arc::ptr_eq(&source, plane.geometry)),
        };
        cache.source = Arc::downgrade(plane.geometry);
        cache.boxes.begin_surface(CANVAS_SURFACE);
        let clip = plane.plane.clip;
        let mut ranges = Vec::with_capacity(plane.plane.primitives.len());
        for primitive in plane.plane.primitives.iter() {
            let start = cache.boxes.piece_count();
            if matches!(primitive, CanvasPrimitive::Box { .. }) {
                cache.boxes.push_boxes(&[(primitive, clip, paint)], stats);
            }
            ranges.push(start..cache.boxes.piece_count());
        }
        let ready = cache.boxes.commit_surface(|_, _| &[], stats)?;
        self.prepare_surface_program()?;
        if self.gui_glyph_program.is_none() {
            self.gui_glyph_program = Some(self.device.borrow_mut().create_program(
                crate::services::render::embedded_shader!("shaders/surface_glyph.vert"),
                crate::services::render::embedded_shader!("shaders/surface_glyph.frag"),
            )?);
        }
        let mvp = ipp_core::math::multiply(view_projection, plane.model);
        let primitive_scene = super::super::canvas::draw::PrimitiveScene {
            host: scene.host,
            publication: plane.publication,
            world: plane.entity.world,
            selection: None,
            plane: Some(key),
        };
        self.device.borrow_mut().set_surface_double_sided(true)?;
        let mut instances = Vec::new();
        let result = (|| {
            for (primitive, range) in plane.plane.primitives.iter().zip(ranges) {
                if matches!(primitive, CanvasPrimitive::Box { .. }) {
                    if !ready {
                        continue;
                    }
                    let cache = self
                        .plot_plane_caches
                        .get_mut(&key)
                        .expect("prepared plane cache");
                    let shapes = self.canvas_paints.prepare_draw(
                        &mut self.device.borrow_mut(),
                        scene.selection,
                        &cache.boxes.paints,
                    )?;
                    cache.boxes.draw_pieces(
                        shapes,
                        self.gui_glyph_program
                            .as_ref()
                            .expect("shared glyph program"),
                        range,
                        clip,
                        |_| None,
                        &mvp,
                        stats,
                    )?;
                } else {
                    self.draw_surface_primitive(
                        &primitive_scene,
                        primitive,
                        clip,
                        paint,
                        &mvp,
                        stats,
                        &mut instances,
                    )?;
                }
            }
            Ok(())
        })();
        let restored = self.device.borrow_mut().set_surface_double_sided(false);
        result.and(restored)
    }
}
