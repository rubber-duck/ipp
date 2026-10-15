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
        let mut paint = SurfacePaint {
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
        let programs = &mut self.canvas_paints;
        let fallbacks = &mut self.canvas_paint_fallbacks;
        cache.boxes.paints.prepare(
            plane.callout_paint.map_or(&[], std::slice::from_ref),
            |instance| {
                super::super::canvas::draw::admit_paint::<D>(programs, instance, |key| {
                    scene.resource(key)
                })
            },
            |instance, reason| {
                super::super::canvas::draw::record_paint_fallback(
                    fallbacks,
                    scene.selection,
                    instance,
                    reason,
                )
            },
        );
        if cache.boxes.paints.lanes_changed() {
            paint.invalidate();
        }
        let clip = plane.plane.clip;
        let painted = plane.callout_paint.map(|paint| {
            plane
                .plane
                .primitives
                .iter()
                .map(|primitive| {
                    let mut primitive = primitive.clone();
                    if let CanvasPrimitive::Box {
                        fill,
                        ..
                    } = &mut primitive
                    {
                        let color = match fill {
                            ipp_core::systems::canvas::CanvasShapeFill::Solid(color) => *color,
                            _ => [1.0; 4],
                        };
                        *fill = ipp_core::systems::canvas::CanvasShapeFill::Paint {
                            color,
                            paint: paint.target,
                            data: [0.0; 4],
                            bounds: None,
                        };
                    }
                    primitive
                })
                .collect::<Vec<_>>()
        });
        // Ordinary axes and labels borrow retained primitives without copies.
        let primitives = painted.as_deref().unwrap_or(&plane.plane.primitives);
        let mut ranges = Vec::with_capacity(primitives.len());
        for primitive in primitives {
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
        let features = plane
            .callout_paint
            .and_then(|instance| instance.shader)
            .and_then(|key| scene.resource(key))
            .and_then(|resource| resource.data())
            .and_then(|asset| {
                asset
                    .as_any()
                    .downcast_ref::<super::super::materials::shader_asset::GlShaderData<D>>()
            })
            .and_then(|asset| asset.paint.as_ref())
            .map_or(0, |source| source.features);
        let presentation = if features & 64 != 0 {
            let (lighting, selected) = super::super::lighting::selection::entity_lighting(
                scene,
                plane.entity,
                plane.model,
                false,
                features & 8 != 0,
                &[],
            )?;
            let mut model = plane.model;
            // Native plane coordinates are Y-down; the paint's optical basis is Y-up.
            for axis in 0..3 {
                model[4 + axis] = -model[4 + axis];
                model[8 + axis] = -model[8 + axis];
            }
            Some(super::super::canvas::presentation::CanvasPresentation {
                model,
                content: super::super::canvas::layering::plane_matrix([0.0; 2], [1.0, -1.0])?,
                mapping: [0.0; 3],
                spacing: 0.0,
                lighting: Arc::new(lighting),
                selected: selected.into(),
                features,
            })
        } else {
            None
        };
        let primitive_scene = super::super::canvas::draw::PrimitiveScene {
            host: scene.host,
            publication: plane.publication,
            world: plane.entity.world,
            selection: None,
            plane: Some(key),
            presentation: presentation.as_ref(),
        };
        self.device.borrow_mut().set_surface_double_sided(true)?;
        let mut instances = Vec::new();
        let result = (|| {
            for (primitive, range) in primitives.iter().zip(ranges) {
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
                    if let Some(presentation) = &presentation {
                        presentation.upload(&mut *self.device.borrow_mut(), shapes)?;
                    }
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
