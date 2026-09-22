//! Service construction, resource registration and context recovery.

use super::{RenderError, RenderService};
use crate::RenderDevice;
use ipp_core::WorldContext;
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
            debug: crate::services::render::debug_geometry::DebugGeometryRenderCache::new(
                device.clone(),
            ),
            #[cfg(feature = "surfaces")]
            surface_program: None,
            #[cfg(feature = "surfaces")]
            surface_instance_program: None,
            #[cfg(feature = "surfaces")]
            surface_bitmap_program: None,
            #[cfg(feature = "gui")]
            surface_box_program: None,
            #[cfg(feature = "gui")]
            gui_batch_cache: super::super::gui_batch::GuiBatchRenderCache::new(device.clone()),
            #[cfg(feature = "gui")]
            surface_text_program: None,
            #[cfg(feature = "gui")]
            glyph_atlas: super::super::glyph_atlas::GlyphAtlas::new(device.clone()),
            #[cfg(feature = "gui")]
            glyph_batch_cache: super::super::glyph_atlas::GlyphBatchRenderCache::new(
                device.clone(),
            ),
            device,
            asset_context_active: Rc::new(Cell::new(true)),
            #[cfg(feature = "particles")]
            particle_quad: None,
            custom_program_count: Rc::new(Cell::new(0)),
            recipe_scratch: Vec::new(),
            program_keys: Vec::new(),
            program_lookup: vec![None; super::super::shader::PROGRAM_RECIPE_COUNT],
            custom_materials: BTreeMap::new(),
            custom_diagnostics: BTreeMap::new(),
            uploaded: Rc::new(Cell::new(0)),
            frame_scratch: Default::default(),
            #[cfg(feature = "particles")]
            particle_quad_metadata: None,
            light_selections: Default::default(),
            #[cfg(feature = "shadows")]
            shadow_capacity_limit: usize::MAX,
            #[cfg(feature = "shadows")]
            shadow_map: None,
            #[cfg(feature = "shadows")]
            shadow_map_size: 0,
        })
    }
    /// Bind renderer-owned resources before this world acquires asset data.
    pub fn install(&self, world: &mut ipp_core::HostRuntime) -> Result<(), RenderError> {
        {
            let device = self.device.clone();
            let uploaded = self.uploaded.clone();
            let context_active = self.asset_context_active.clone();
            // Demanded meshes keep CPU metadata available to picking and layout
            // while detached; the loader itself gates only device creation.
            world
                .asset_resources_mut()
                .register_loader(ipp_core::MESH_TYPE, move || {
                    crate::services::render::assets::mesh_asset_loader(
                        device.clone(),
                        uploaded.clone(),
                        context_active.clone(),
                    )
                })
                .map_err(RenderError::RenderDevice)?;
        }
        #[cfg(feature = "surfaces")]
        {
            let device = self.device.clone();
            let uploaded = self.uploaded.clone();
            world
                .asset_resources_mut()
                .register_graphics_loader(
                    ipp_core::services::asset_management::font::FONT_TYPE,
                    move || {
                        super::super::surface_assets::font_loader(device.clone(), uploaded.clone())
                    },
                )
                .map_err(RenderError::RenderDevice)?;
        }
        #[cfg(feature = "surfaces")]
        {
            let device = self.device.clone();
            let uploaded = self.uploaded.clone();
            world
                .asset_resources_mut()
                .register_graphics_loader(
                    ipp_core::services::asset_management::drawing::DRAWING_TYPE,
                    move || {
                        super::super::surface_assets::drawing_loader(
                            device.clone(),
                            uploaded.clone(),
                        )
                    },
                )
                .map_err(RenderError::RenderDevice)?;
        }
        {
            let device = self.device.clone();
            let uploaded = self.uploaded.clone();
            world
                .asset_resources_mut()
                .register_graphics_loader(ipp_core::TEXTURE_TYPE, move || {
                    crate::services::render::assets::texture_asset_loader(
                        device.clone(),
                        uploaded.clone(),
                    )
                })
                .map_err(RenderError::RenderDevice)?;
        }
        let device = self.device.clone();
        let count = self.custom_program_count.clone();
        world
            .asset_resources_mut()
            .register_graphics_loader(
                ipp_core::services::asset_management::shader::SHADER_TYPE,
                move || super::super::shader_asset::loader(device.clone(), count.clone()),
            )
            .map_err(RenderError::RenderDevice)?;
        let device = self.device.clone();
        let count = self.custom_program_count.clone();
        world
            .asset_resources_mut()
            .register_graphics_loader(super::super::program_assets::PROGRAM_TYPE, move || {
                super::super::program_assets::loader(device.clone(), count.clone())
            })
            .map_err(RenderError::RenderDevice)?;
        world.set_renderer_asset_loading(true);
        Ok(())
    }

    /// Gate loader device calls while the owning presentation context is detached.
    pub fn set_asset_context_active(&self, active: bool) {
        self.asset_context_active.set(active);
    }

    /// Request GPU payload release; the owning Host completes the lifecycle barrier.
    pub fn unload(&mut self, world: &mut WorldContext<'_>) {
        self.unload_resources(world.asset_resources_mut());
    }

    /// Request release of this context's GPU payloads, including with no attached World.
    /// The Host must finish its lifecycle barrier before the graphics device is replaced.
    pub fn unload_host(&mut self, host: &mut ipp_core::HostRuntime) {
        self.unload_resources(host.asset_resources_mut());
    }

    fn unload_resources(
        &mut self,
        resources: &mut ipp_core::services::asset_management::AssetManagementService,
    ) {
        #[cfg(feature = "shadows")]
        self.clear_shadows();
        self.debug.clear();
        #[cfg(feature = "surfaces")]
        if let Some(program) = self.surface_program.take() {
            self.device.borrow_mut().delete_program(program);
        }
        #[cfg(feature = "surfaces")]
        if let Some(program) = self.surface_instance_program.take() {
            self.device.borrow_mut().delete_program(program);
        }
        #[cfg(feature = "surfaces")]
        if let Some(program) = self.surface_bitmap_program.take() {
            self.device.borrow_mut().delete_program(program);
        }
        #[cfg(feature = "gui")]
        if let Some(program) = self.surface_box_program.take() {
            self.device.borrow_mut().delete_program(program);
        }
        #[cfg(feature = "gui")]
        self.gui_batch_cache.clear();
        #[cfg(feature = "gui")]
        if let Some(program) = self.surface_text_program.take() {
            self.device.borrow_mut().delete_program(program);
        }
        #[cfg(feature = "gui")]
        self.glyph_atlas.clear();
        #[cfg(feature = "gui")]
        self.glyph_batch_cache.clear();
        #[cfg(feature = "particles")]
        {
            self.particle_quad = None;
        }
        let keys: Vec<_> = resources
            .iter()
            .filter(|resource| {
                let graphics = resource.source().kind == ipp_core::MESH_TYPE;
                let graphics = graphics
                    || resource.source().kind == ipp_core::TEXTURE_TYPE
                    || resource.source().kind
                        == ipp_core::services::asset_management::shader::SHADER_TYPE
                    || resource.source().kind == super::super::program_assets::PROGRAM_TYPE;
                #[cfg(feature = "surfaces")]
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
        self.custom_diagnostics.clear();
        self.light_selections.clear();
        #[cfg(feature = "shadows")]
        {
            self.shadow_capacity_limit = usize::MAX;
        }
    }

    /// Restore a host context while keeping existing resources and factory bindings.
    /// The caller keeps the old native graphics context alive and current until this
    /// returns, then makes the replacement context current before the next render.
    pub fn replace_device(
        &mut self,
        host: &mut ipp_core::HostRuntime,
        device: D,
    ) -> Result<(), RenderError> {
        if host.has_pending_world_updates() {
            return Err(RenderError::RenderDevice(
                "Device replacement requires completed World updates".into(),
            ));
        }
        self.unload_host(host);
        host.flush_resource_lifecycle();
        *self.device.borrow_mut() = device;
        Ok(())
    }

    /// Diagnostic mode; normal submissions validate at pass boundaries.
    pub fn set_exhaustive_draw_checks(&mut self, enabled: bool) {
        self.device.borrow_mut().set_exhaustive_draw_checks(enabled);
    }

    /// Drop renderer-only history when a World is destroyed or detached from presentation.
    pub fn forget_world(&mut self, world: ipp_core::WorldId) {
        self.light_selections.remove(&world);
    }

    /// Linked programs actually demanded in this graphics context.
    pub fn cached_program_count(&self) -> usize {
        self.custom_program_count.get()
    }

    /// Begin frame-local accounting and return uploads completed between frames.
    pub fn begin_frame(&mut self) -> u32 {
        self.uploaded.replace(0)
    }
}

impl<D: RenderDevice> Drop for RenderService<D> {
    fn drop(&mut self) {
        #[cfg(feature = "gui")]
        self.gui_batch_cache.clear();
        #[cfg(feature = "shadows")]
        self.clear_shadows();
    }
}
