//! Service construction, resource registration and context recovery.

use super::{RenderError, RenderService};
use crate::RenderDevice;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

impl<D: RenderDevice> RenderService<D> {
    /// Initialize empty context caches. Programs are compiled on first demand.
    pub fn new(device: D) -> Result<Self, RenderError> {
        let device = Rc::new(RefCell::new(device));
        Ok(Self {
            inclusions: Default::default(),
            #[cfg(feature = "instrumentation")]
            gpu_capture: Default::default(),
            generated_paths: super::super::generated_paths::GeneratedPathCache::new(device.clone()),
            generated_meshes: super::super::generated_meshes::GeneratedMeshCache::new(
                device.clone(),
            ),
            plot_plane_caches: BTreeMap::new(),
            plot_label_layouts: BTreeMap::new(),
            debug: crate::services::render::debug_geometry::DebugGeometryRenderCache::new(
                device.clone(),
            ),
            surface_program: None,
            surface_instance_program: None,
            surface_bitmap_program: None,
            surface_cache_program: None,
            surface_cache: Default::default(),
            canvas_caches: Default::default(),
            canvas_cache_frame: Default::default(),
            surface_missing: Vec::new(),
            surface_analytic_text: false,
            surface_gui_unretained: false,
            canvas_paints: Default::default(),
            gui_glyph_program: None,
            canvas_paint_fallbacks: BTreeMap::new(),
            gui_batch_cache: BTreeMap::new(),
            surface_ops: Vec::new(),
            glyph_atlas: super::super::glyph_atlas::GlyphAtlas::new(device.clone()),
            glyph_batch_cache: BTreeMap::new(),
            glyph_frame: Default::default(),
            glyph_population: Default::default(),
            surface_paint: BTreeMap::new(),
            analytic_glyphs: BTreeMap::new(),
            device,
            asset_context_active: Rc::new(Cell::new(true)),
            particle_quad: None,
            recipe_scratch: Vec::new(),
            program_demand: None,
            prepared_output: None,
            program_lookup: vec![None; super::super::shader::PROGRAM_RECIPE_COUNT],
            custom_materials: BTreeMap::new(),
            custom_fallbacks: BTreeMap::new(),
            uploads: Default::default(),
            statistics: Default::default(),
            frame_scratch: Default::default(),
            light_selections: Default::default(),
            camera_targets: Default::default(),
            camera_completed: Default::default(),
            shadow_capacity_limit: usize::MAX,
            shadow_map: None,
            shadow_map_size: 0,
        })
    }

    /// Bind renderer-owned resources before this world acquires asset data.
    pub fn install(&self, world: &mut ipp_core::HostRuntime) -> Result<(), RenderError> {
        {
            let device = self.device.clone();
            let uploads = self.uploads.clone();
            let context_active = self.asset_context_active.clone();
            // Demanded meshes keep CPU metadata available to picking and layout
            // while detached; the loader itself gates only device creation.
            world
                .asset_resources_mut()
                .register_loader(ipp_core::MESH_TYPE, move || {
                    crate::services::render::assets::mesh_asset_loader(
                        device.clone(),
                        uploads.clone(),
                        context_active.clone(),
                    )
                })
                .map_err(RenderError::RenderDevice)?;
        }
        {
            let device = self.device.clone();
            let uploads = self.uploads.clone();
            world
                .asset_resources_mut()
                .register_graphics_loader(
                    ipp_core::services::asset_management::font::FONT_TYPE,
                    move || {
                        super::super::surface_assets::font_loader(device.clone(), uploads.clone())
                    },
                )
                .map_err(RenderError::RenderDevice)?;
        }
        {
            let device = self.device.clone();
            let uploads = self.uploads.clone();
            world
                .asset_resources_mut()
                .register_graphics_loader(
                    ipp_core::services::asset_management::drawing::DRAWING_TYPE,
                    move || {
                        super::super::surface_assets::drawing_loader(
                            device.clone(),
                            uploads.clone(),
                        )
                    },
                )
                .map_err(RenderError::RenderDevice)?;
        }
        {
            let device = self.device.clone();
            let uploads = self.uploads.clone();
            world
                .asset_resources_mut()
                .register_graphics_loader(ipp_core::TEXTURE_TYPE, move || {
                    crate::services::render::assets::texture_asset_loader(
                        device.clone(),
                        uploads.clone(),
                    )
                })
                .map_err(RenderError::RenderDevice)?;
        }
        let device = self.device.clone();
        world
            .asset_resources_mut()
            .register_graphics_loader(
                ipp_core::services::asset_management::shader::SHADER_TYPE,
                move || super::super::shader_asset::loader(device.clone()),
            )
            .map_err(RenderError::RenderDevice)?;
        let device = self.device.clone();
        world
            .asset_resources_mut()
            .register_graphics_loader(super::super::program_assets::PROGRAM_TYPE, move || {
                super::super::program_assets::loader(device.clone())
            })
            .map_err(RenderError::RenderDevice)?;
        world.set_renderer_asset_loading(true);
        Ok(())
    }

    /// Gate loader device calls while the owning presentation context is detached.
    pub fn set_asset_context_active(&self, active: bool) {
        self.asset_context_active.set(active);
    }

    /// Request release of this context's GPU payloads, including with no attached World.
    /// The Host must finish its lifecycle barrier before the graphics device is replaced.
    pub fn unload_host(&mut self, host: &mut ipp_core::HostRuntime) -> Result<(), RenderError> {
        self.validate_catalog(host)?;
        self.unload_resources(host.asset_resources_mut());
        Ok(())
    }

    fn unload_resources(
        &mut self,
        resources: &mut ipp_core::services::asset_management::AssetManagementService,
    ) {
        self.clear_shadows();
        self.debug.clear();
        self.generated_paths.clear();
        self.generated_meshes.clear();
        self.plot_plane_caches.clear();
        self.plot_label_layouts.clear();
        if let Some(program) = self.surface_program.take() {
            self.device.borrow_mut().delete_program(program);
        }
        if let Some(program) = self.surface_instance_program.take() {
            self.device.borrow_mut().delete_program(program);
        }
        if let Some(program) = self.surface_bitmap_program.take() {
            self.device.borrow_mut().delete_program(program);
        }
        if let Some(program) = self.gui_glyph_program.take() {
            self.device.borrow_mut().delete_program(program);
        }
        // Cache images are rebuilt from current evaluated inputs; the budget survives.
        self.clear_surface_caches();
        for cache in self.analytic_glyphs.values_mut() {
            cache.clear();
        }
        // Paints are admitted again and the canvas program rebuilt on next use.
        self.canvas_paints.clear(&mut self.device.borrow_mut());
        self.canvas_paint_fallbacks.clear();
        self.gui_batch_cache.clear();
        // Glyph atlas layout, demand and run bands survive, so recovered text
        // repopulates its original slots.
        for cache in self.glyph_batch_cache.values_mut() {
            cache.release_context();
        }
        self.glyph_atlas.release_context();
        self.particle_quad = None;
        let keys: Vec<_> = resources
            .iter()
            .filter(|resource| {
                let graphics = resource.source().kind == ipp_core::MESH_TYPE;
                let graphics = graphics
                    || resource.source().kind == ipp_core::TEXTURE_TYPE
                    || resource.source().kind
                        == ipp_core::services::asset_management::shader::SHADER_TYPE
                    || resource.source().kind == super::super::program_assets::PROGRAM_TYPE;
                let graphics = graphics
                    || resource.source().kind
                        == ipp_core::services::asset_management::font::FONT_TYPE
                    || resource.source().kind
                        == ipp_core::services::asset_management::drawing::DRAWING_TYPE;
                graphics
                    && !matches!(
                        resource.status(),
                        ipp_core::services::asset_management::AssetLoadStatus::Unloaded
                    )
            })
            .map(|resource| resource.key())
            .collect();
        for key in keys {
            resources.invalidate_graphics(key);
        }
        self.custom_fallbacks.clear();
        self.light_selections.clear();
        for (_, (target, _)) in std::mem::take(&mut self.camera_targets) {
            self.device.borrow_mut().delete_surface_cache_target(target);
        }
        self.camera_completed.clear();
        self.shadow_capacity_limit = usize::MAX;
    }

    /// Restore a host context while keeping existing resources and factory bindings.
    /// The caller keeps the old native graphics context alive and current until this
    /// returns, then makes the replacement context current before the next render.
    pub fn replace_device(
        &mut self,
        host: &mut ipp_core::HostRuntime,
        device: D,
    ) -> Result<(), RenderError> {
        self.unload_host(host)?;
        host.flush_resource_lifecycle();
        #[cfg(feature = "instrumentation")]
        self.gpu_replace_context();
        *self.device.borrow_mut() = device;
        Ok(())
    }

    /// Testing mode attributing each GL error to its failing call; normal
    /// submissions validate at pass boundaries (see
    /// [`crate::RenderDevice::set_exhaustive_draw_checks`]).
    #[cfg(any(test, feature = "instrumentation"))]
    pub fn set_exhaustive_draw_checks(&mut self, enabled: bool) {
        self.device.borrow_mut().set_exhaustive_draw_checks(enabled);
    }

    /// Drop renderer-only history when a World is destroyed or detached from presentation.
    ///
    /// The World's glyph demand leaves the shared atlas; pages other Worlds still use
    /// stay resident. Its Surface cache images are released.
    pub fn forget_world(&mut self, world: ipp_core::WorldId) {
        self.light_selections
            .retain(|selection, _| selection.world().id() != world);
        self.plot_label_layouts
            .retain(|selection, _| selection.world().id() != world);
        self.forget_surface_caches(world);
        {
            self.surface_paint
                .retain(|output, _| output.world().id() != world);
            self.analytic_glyphs
                .retain(|output, _| output.world().id() != world);
            let retired: Vec<_> = self
                .camera_targets
                .keys()
                .filter(|selection| selection.world().id() == world)
                .copied()
                .collect();
            for selection in retired {
                if let Some((target, _)) = self.camera_targets.remove(&selection) {
                    self.device.borrow_mut().delete_surface_cache_target(target);
                }
            }
            self.camera_completed
                .retain(|selection| selection.world().id() != world);
        }
        self.gui_batch_cache
            .retain(|output, _| output.world().id() != world);
        self.canvas_paint_fallbacks
            .retain(|(output, _), _| output.world().id() != world);
        self.glyph_batch_cache.retain(|output, cache| {
            if output.world().id() == world {
                cache.release_demand(&mut self.glyph_atlas);
                false
            } else {
                true
            }
        });
        self.glyph_atlas.release_if_unused();
    }

    pub(in crate::services::render) fn retain_canvas_outputs(
        &mut self,
        outputs: &std::collections::BTreeSet<ipp_core::OutputRef>,
    ) {
        self.retain_canvas_cache_outputs(outputs);
        self.surface_paint
            .retain(|selection, _| outputs.contains(selection));
        self.analytic_glyphs
            .retain(|selection, _| outputs.contains(selection));
        self.gui_batch_cache
            .retain(|selection, _| outputs.contains(selection));
        self.canvas_paint_fallbacks
            .retain(|(selection, _), _| outputs.contains(selection));
        self.glyph_batch_cache.retain(|selection, cache| {
            let live = outputs.contains(selection);
            if !live {
                cache.release_demand(&mut self.glyph_atlas);
            }
            live
        });
        self.glyph_atlas.release_if_unused();
    }

    pub(in crate::services::render) fn retain_prepared_outputs(
        &mut self,
        selection: Option<ipp_core::OutputRef>,
        outputs: &std::collections::BTreeSet<ipp_core::OutputRef>,
    ) {
        if selection.is_none() {
            self.frame_scratch.clear();
            self.clear_shadows();
            self.particle_quad = None;
            self.canvas_cache_frame = Default::default();
            self.glyph_frame.clear();
            self.surface_missing.clear();
            self.surface_ops.clear();
        }
        if selection.is_none() || self.prepared_output != selection {
            self.debug.clear();
            self.generated_paths.clear();
            self.generated_meshes.clear();
            self.plot_plane_caches.clear();
            self.plot_label_layouts.clear();
            self.custom_materials.clear();
            self.custom_fallbacks.clear();
        }
        self.prepared_output = selection;
        {
            let retired: Vec<_> = self
                .camera_targets
                .keys()
                .filter(|selection| !outputs.contains(selection))
                .copied()
                .collect();
            self.retain_canvas_outputs(outputs);
            for selection in retired {
                if let Some((target, _)) = self.camera_targets.remove(&selection) {
                    self.device.borrow_mut().delete_surface_cache_target(target);
                }
                self.camera_completed.remove(&selection);
            }
        }
        self.light_selections
            .retain(|selection, _| outputs.contains(selection));
        self.plot_label_layouts
            .retain(|selection, _| outputs.contains(selection));
    }

    /// Testing override of the renderer-owned glyph atlas bounds of this context: its
    /// resident page budget and how many demand publications a page without demand
    /// stays resident.
    ///
    /// A lowered budget retires pages at the next publication; zero pages is treated
    /// as one.
    #[cfg(any(test, feature = "instrumentation"))]
    pub fn set_glyph_atlas_limits(&mut self, limits: super::super::glyph_atlas::GlyphAtlasLimits) {
        self.glyph_atlas.set_limits(limits);
    }

    /// Bound the time one frame spends populating glyph atlas entries beyond
    /// [`super::super::glyph_atlas::MIN_POPULATES_PER_FRAME`], in milliseconds.
    ///
    /// A testing override: the renderer default is
    /// [`super::super::glyph_atlas::DEFAULT_POPULATE_BUDGET_MS`]. Zero populates only
    /// that floor per frame; an infinite budget populates up to
    /// [`super::super::glyph_atlas::MAX_POPULATES_PER_FRAME`].
    #[cfg(any(test, feature = "instrumentation"))]
    pub fn set_glyph_population_budget_ms(&mut self, budget_ms: f64) {
        self.glyph_population.set_budget_ms(budget_ms);
    }

    /// Largest drawing-buffer size the attached device accepts; `None` until a
    /// device context can report it, such as while the context is lost.
    pub fn viewport_limits(&self) -> Option<crate::ViewportLimits> {
        self.device.borrow().viewport_limits()
    }

    /// Statistics of the last completed render; reset when a render starts.
    pub fn statistics(&self) -> &crate::RenderStatistics {
        &self.statistics
    }
}

impl<D: RenderDevice> Drop for RenderService<D> {
    fn drop(&mut self) {
        {
            let mut device = self.device.borrow_mut();
            self.surface_cache
                .clear(&mut super::surface_cache::DeviceCacheTargets(&mut *device));
            for (_, (target, _)) in std::mem::take(&mut self.camera_targets) {
                device.delete_surface_cache_target(target);
            }
        }
        self.gui_batch_cache.clear();
        self.analytic_glyphs.clear();
        self.clear_shadows();
    }
}
