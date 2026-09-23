//! Shared failure-injecting render device and World fixtures for renderer
//! integration tests. GL harnesses own image evidence.

#![allow(dead_code)]

use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, MeshAsset, WorldContext,
    components::{Camera, Transform},
    services::asset_management::{AssetLoadStatus, AssetTypeId},
};
use ipp_render_gl::{RenderDevice, RenderError, RenderService};
#[cfg(feature = "surfaces")]
use std::cell::RefCell;
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
pub struct DeviceState {
    pub failed_attempts_remaining: Cell<u32>,
    pub mesh_attempts: Cell<u32>,
    #[cfg(feature = "shadows")]
    pub fail_shadow_allocation: Cell<bool>,
    #[cfg(feature = "shadows")]
    pub shadow_attempts: Cell<u32>,
    pub live_meshes: Cell<u32>,
    pub ended_frames: Cell<u32>,
    #[cfg(feature = "surfaces")]
    pub context_lost: Cell<bool>,
    #[cfg(feature = "surfaces")]
    pub surface_path_attempts: Cell<u32>,
    #[cfg(feature = "surfaces")]
    pub surface_double_sided: Cell<bool>,
    #[cfg(feature = "surfaces")]
    pub surface_state_changes: RefCell<Vec<bool>>,
    #[cfg(feature = "surfaces")]
    pub fail_surface_draw: Cell<bool>,
    #[cfg(feature = "surfaces")]
    pub fail_surface_state_start: Cell<bool>,
    #[cfg(feature = "shadows")]
    pub live_shadow_maps: Cell<u32>,
    #[cfg(feature = "shadows")]
    pub shadow_pass: Cell<bool>,
    #[cfg(feature = "shadows")]
    pub fail_shadow_draw: Cell<bool>,
    #[cfg(feature = "surfaces")]
    pub analytic_glyph_draws: Cell<u32>,
    #[cfg(feature = "gui")]
    pub glyph_batch_uploads: Cell<u32>,
    #[cfg(feature = "gui")]
    pub atlas_populations: Cell<u32>,
    #[cfg(feature = "gui")]
    pub atlas_target_bound: Cell<bool>,
    #[cfg(feature = "gui")]
    pub fail_atlas_begin: RefCell<Option<RenderError>>,
    #[cfg(feature = "gui")]
    pub fail_atlas_end: RefCell<Option<RenderError>>,
    #[cfg(feature = "gui")]
    pub atlas_restores: Cell<u32>,
    #[cfg(feature = "gui")]
    pub atlas_pages_created: Cell<u32>,
    #[cfg(feature = "gui")]
    pub fail_glyph_batch: RefCell<Option<RenderError>>,
    /// Largest cache target dimension; zero (the default) disables caching.
    #[cfg(feature = "surfaces")]
    pub cache_limit: Cell<u32>,
    #[cfg(feature = "surfaces")]
    pub cache_targets_live: Cell<u32>,
    #[cfg(feature = "surfaces")]
    pub cache_creates: Cell<u32>,
    #[cfg(feature = "surfaces")]
    pub cache_resizes: Cell<u32>,
    #[cfg(feature = "surfaces")]
    pub cache_begins: Cell<u32>,
    #[cfg(feature = "surfaces")]
    pub cache_composites: Cell<u32>,
    #[cfg(feature = "surfaces")]
    pub cache_target_bound: Cell<bool>,
    #[cfg(feature = "surfaces")]
    pub fail_cache_create: RefCell<Option<RenderError>>,
    #[cfg(feature = "surfaces")]
    pub fail_cache_begin: RefCell<Option<RenderError>>,
    #[cfg(feature = "surfaces")]
    pub fail_cache_end: RefCell<Option<RenderError>>,
    #[cfg(feature = "surfaces")]
    pub fail_cache_composite: RefCell<Option<RenderError>>,
    /// Curve-path draws, excluding analytic glyph instances.
    #[cfg(feature = "surfaces")]
    pub surface_path_draws: Cell<u32>,
    #[cfg(feature = "gui")]
    pub glyph_batch_draws: Cell<u32>,
    /// Retained GUI box batch draws.
    #[cfg(feature = "gui")]
    pub gui_batch_draws: Cell<u32>,
    /// Ordered Surface work: `B`/`E` begin and end a cache target, `C` composites,
    /// `P` draws paths, `G` analytic glyphs, `T` atlas text, `X` GUI boxes,
    /// `F` begins the frame.
    #[cfg(feature = "surfaces")]
    pub surface_events: RefCell<String>,
}

pub struct TestDevice(pub Rc<DeviceState>);

impl RenderDevice for TestDevice {
    #[cfg(feature = "surfaces")]
    type SurfacePath = ();
    #[cfg(feature = "surfaces")]
    type SurfaceCacheTarget = ();
    type Program = ();
    type Mesh = ();
    type Texture = ();

    #[cfg(feature = "shadows")]
    type ShadowMap = ();
    #[cfg(feature = "gui")]
    type GuiBatch = ();
    #[cfg(feature = "gui")]
    type GlyphBatch = ();
    #[cfg(feature = "gui")]
    type GlyphAtlasPage = ();

    #[cfg(feature = "gui")]
    fn glyph_atlas_texture(page: &Self::GlyphAtlasPage) -> &Self::Texture {
        page
    }

    fn set_lighting(
        &mut self,
        _: &(),
        _: &[f32; 16],
        _: &[f32; 16],
        _: &[f32; 3],
        _: &ipp_render_gl::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "surfaces")]
    fn set_surface_double_sided(&mut self, enabled: bool) -> Result<(), RenderError> {
        self.0.surface_double_sided.set(enabled);
        self.0.surface_state_changes.borrow_mut().push(enabled);
        if enabled && self.0.fail_surface_state_start.get() {
            Err(RenderError::RenderDevice(
                "injected Surface state failure".into(),
            ))
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "shadows")]
    fn shadow_map_limit(&self) -> u32 {
        4096
    }

    #[cfg(feature = "shadows")]
    fn create_shadow_map(&mut self, _: u32) -> Result<(), RenderError> {
        self.0.shadow_attempts.set(self.0.shadow_attempts.get() + 1);
        if self.0.fail_shadow_allocation.get() {
            return Err(RenderError::RenderDevice(
                "injected atlas allocation failure".into(),
            ));
        }
        self.0
            .live_shadow_maps
            .set(self.0.live_shadow_maps.get() + 1);
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn begin_shadow(&mut self, _: &(), _: u32, _: u32) -> Result<(), RenderError> {
        assert!(!self.0.shadow_pass.replace(true));
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn end_shadow(&mut self) -> Result<(), RenderError> {
        assert!(self.0.shadow_pass.replace(false));
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn bind_shadow(
        &mut self,
        _: &(),
        _: &(),
        _: &ipp_render_gl::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn delete_shadow_map(&mut self, _: ()) {
        self.0
            .live_shadow_maps
            .set(self.0.live_shadow_maps.get() - 1);
    }

    fn create_program(&mut self, _vertex: &str, _fragment: &str) -> Result<(), RenderError> {
        Ok(())
    }

    fn create_mesh(&mut self, _asset: &MeshAsset) -> Result<(), RenderError> {
        self.0.mesh_attempts.set(self.0.mesh_attempts.get() + 1);
        if self.0.failed_attempts_remaining.get() != 0 {
            self.0
                .failed_attempts_remaining
                .set(self.0.failed_attempts_remaining.get() - 1);
            return Err(RenderError::RenderDevice(
                "injected GPU allocation failure".into(),
            ));
        }
        self.0.live_meshes.set(self.0.live_meshes.get() + 1);
        Ok(())
    }

    fn create_texture(
        &mut self,
        _width: u32,
        _height: u32,
        _pixels: &[u8],
    ) -> Result<(), RenderError> {
        panic!("mesh-only scenarios never create textures");
    }

    fn allocate_texture(&mut self, _width: u32, _height: u32) -> Result<(), RenderError> {
        panic!("mesh-only scenarios never allocate textures");
    }

    fn upload_texture_rows(
        &mut self,
        _texture: &(),
        _width: u32,
        _first_row: u32,
        _rows: u32,
        _pixels: &[u8],
    ) -> Result<(), RenderError> {
        panic!("mesh-only scenarios never upload textures");
    }

    #[cfg(feature = "surfaces")]
    fn create_surface_path(
        &mut self,
        _: &[f32; 4],
        _: &[[f32; 8]],
        _: &[[u32; 2]],
    ) -> Result<(), RenderError> {
        self.0
            .surface_path_attempts
            .set(self.0.surface_path_attempts.get() + 1);
        if self.0.context_lost.get() {
            Err(RenderError::ContextLost)
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "surfaces")]
    fn draw_surface_path(
        &mut self,
        _: &(),
        _: &(),
        _: &[f32; 4],
        _: ipp_render_gl::SurfacePathDescriptor,
        _: &[f32; 16],
        _: &[f32; 4],
        _: &[f32; 4],
        _: &[f32; 4],
        _: u32,
    ) -> Result<(), RenderError> {
        assert!(
            self.0.surface_double_sided.get(),
            "Surface draw must run with back-face culling suspended"
        );
        self.0
            .surface_path_draws
            .set(self.0.surface_path_draws.get() + 1);
        #[cfg(feature = "gui")]
        if !self.0.atlas_target_bound.get() {
            self.0.surface_events.borrow_mut().push('P');
        }
        #[cfg(not(feature = "gui"))]
        self.0.surface_events.borrow_mut().push('P');
        if self.0.fail_surface_draw.get() {
            Err(RenderError::RenderDevice(
                "injected Surface draw failure".into(),
            ))
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "surfaces")]
    fn draw_surface_path_instances(
        &mut self,
        _: &(),
        _: &(),
        _: &[ipp_render_gl::SurfacePathInstance],
        _: &[f32; 16],
        _: &[f32; 4],
        _: u32,
    ) -> Result<(), RenderError> {
        #[cfg(feature = "gui")]
        assert!(
            !self.0.atlas_target_bound.get(),
            "the main pass never draws into an atlas page"
        );
        self.0
            .analytic_glyph_draws
            .set(self.0.analytic_glyph_draws.get() + 1);
        self.0.surface_events.borrow_mut().push('G');
        Ok(())
    }

    #[cfg(feature = "surfaces")]
    fn surface_cache_limit(&self) -> u32 {
        self.0.cache_limit.get()
    }

    #[cfg(feature = "surfaces")]
    fn create_surface_cache_target(&mut self, width: u32, height: u32) -> Result<(), RenderError> {
        let limit = self.0.cache_limit.get();
        assert!(
            (1..=limit).contains(&width) && (1..=limit).contains(&height),
            "cache targets stay within the device limit"
        );
        self.0.cache_creates.set(self.0.cache_creates.get() + 1);
        if let Some(error) = self.0.fail_cache_create.borrow().clone() {
            return Err(error);
        }

        self.0
            .cache_targets_live
            .set(self.0.cache_targets_live.get() + 1);
        Ok(())
    }

    #[cfg(feature = "surfaces")]
    fn resize_surface_cache_target(
        &mut self,
        _: &mut (),
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        let limit = self.0.cache_limit.get();
        assert!(
            (1..=limit).contains(&width) && (1..=limit).contains(&height),
            "cache targets stay within the device limit"
        );
        self.0.cache_resizes.set(self.0.cache_resizes.get() + 1);
        Ok(())
    }

    /// Cache targets never nest or start inside atlas population; atlas
    /// population may nest inside one.
    #[cfg(feature = "surfaces")]
    fn begin_surface_cache_target(&mut self, _: &()) -> Result<(), RenderError> {
        assert!(
            !self.0.cache_target_bound.get(),
            "Surface cache targets never nest"
        );
        #[cfg(feature = "gui")]
        assert!(
            !self.0.atlas_target_bound.get(),
            "Surface cache targets never begin inside atlas population"
        );
        assert!(
            self.0.surface_double_sided.get(),
            "repaints run with back-face culling suspended"
        );
        self.0.cache_begins.set(self.0.cache_begins.get() + 1);
        self.0.surface_events.borrow_mut().push('B');
        if let Some(error) = self.0.fail_cache_begin.borrow().clone() {
            return Err(error);
        }

        self.0.cache_target_bound.set(true);
        Ok(())
    }

    #[cfg(feature = "surfaces")]
    fn end_surface_cache_target(&mut self) -> Result<(), RenderError> {
        self.0.cache_target_bound.set(false);
        self.0.surface_events.borrow_mut().push('E');
        match self.0.fail_cache_end.borrow().clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    #[cfg(feature = "surfaces")]
    fn draw_surface_cache(
        &mut self,
        _: &(),
        _: &(),
        _: &[f32; 16],
        _: &[f32; 2],
    ) -> Result<(), RenderError> {
        assert!(
            self.0.surface_double_sided.get(),
            "Surface cache composites run with back-face culling suspended"
        );
        assert!(
            !self.0.cache_target_bound.get(),
            "the main pass never composites into a cache target"
        );
        #[cfg(feature = "gui")]
        assert!(
            !self.0.atlas_target_bound.get(),
            "the main pass never composites into an atlas page"
        );
        self.0
            .cache_composites
            .set(self.0.cache_composites.get() + 1);
        self.0.surface_events.borrow_mut().push('C');
        match self.0.fail_cache_composite.borrow().clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    #[cfg(feature = "surfaces")]
    fn delete_surface_cache_target(&mut self, _: ()) {
        self.0
            .cache_targets_live
            .set(self.0.cache_targets_live.get() - 1);
    }

    #[cfg(feature = "gui")]
    fn create_gui_batch(&mut self, _: &[ipp_render_gl::GuiBoxVertex]) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "gui")]
    fn update_gui_batch(
        &mut self,
        _: &mut (),
        _: &[ipp_render_gl::GuiBoxVertex],
    ) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "gui")]
    fn draw_gui_batch(
        &mut self,
        _: &(),
        _: &(),
        _: &[f32; 16],
        _: &[f32; 4],
    ) -> Result<(), RenderError> {
        self.0.gui_batch_draws.set(self.0.gui_batch_draws.get() + 1);
        self.0.surface_events.borrow_mut().push('X');
        Ok(())
    }

    #[cfg(feature = "gui")]
    fn create_glyph_batch(&mut self, _: &[ipp_render_gl::GlyphVertex]) -> Result<(), RenderError> {
        self.0
            .glyph_batch_uploads
            .set(self.0.glyph_batch_uploads.get() + 1);
        match self.0.fail_glyph_batch.borrow().clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    #[cfg(feature = "gui")]
    fn update_glyph_batch(
        &mut self,
        _: &mut (),
        _: &[ipp_render_gl::GlyphVertex],
    ) -> Result<(), RenderError> {
        self.0
            .glyph_batch_uploads
            .set(self.0.glyph_batch_uploads.get() + 1);
        match self.0.fail_glyph_batch.borrow().clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    #[cfg(feature = "gui")]
    fn draw_glyph_batch(
        &mut self,
        _: &(),
        _: &(),
        _: &(),
        _: &[f32; 16],
        _: &[f32; 4],
    ) -> Result<(), RenderError> {
        assert!(
            !self.0.atlas_target_bound.get(),
            "the main pass never draws into an atlas page"
        );
        self.0
            .glyph_batch_draws
            .set(self.0.glyph_batch_draws.get() + 1);
        self.0.surface_events.borrow_mut().push('T');
        Ok(())
    }

    #[cfg(feature = "gui")]
    fn create_glyph_atlas_page(&mut self, _: u32, _: u32) -> Result<(), RenderError> {
        self.0
            .atlas_pages_created
            .set(self.0.atlas_pages_created.get() + 1);
        Ok(())
    }

    /// Consecutive begins switch pages; one end restores the host target.
    #[cfg(feature = "gui")]
    fn begin_glyph_atlas_page(&mut self, _: &()) -> Result<(), RenderError> {
        self.0
            .atlas_populations
            .set(self.0.atlas_populations.get() + 1);
        if let Some(error) = self.0.fail_atlas_begin.borrow().clone() {
            return Err(error);
        }

        self.0.atlas_target_bound.set(true);
        Ok(())
    }

    #[cfg(feature = "gui")]
    fn end_glyph_atlas_page(&mut self) -> Result<(), RenderError> {
        self.0.atlas_restores.set(self.0.atlas_restores.get() + 1);
        self.0.atlas_target_bound.set(false);
        match self.0.fail_atlas_end.borrow().clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn begin_frame(
        &mut self,
        _width: u32,
        _height: u32,
        _clear: &[f32; 4],
    ) -> Result<(), RenderError> {
        #[cfg(feature = "surfaces")]
        {
            assert!(
                !self.0.cache_target_bound.get(),
                "the frame begins outside cache repaints"
            );
            self.0.surface_events.borrow_mut().push('F');
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        _program: &(),
        _mesh: &(),
        _mvp: &[f32; 16],
        _material: &[f32; 3],
        #[cfg(feature = "mesh-poses")] _pose: Option<(&Self::Mesh, f32)>,
        _texture: Option<&()>,
    ) -> Result<(), RenderError> {
        #[cfg(feature = "surfaces")]
        assert!(
            !self.0.surface_double_sided.get(),
            "Surface rasterization state leaked into a mesh draw"
        );
        #[cfg(feature = "shadows")]
        if self.0.shadow_pass.get() && self.0.fail_shadow_draw.get() {
            return Err(RenderError::RenderDevice(
                "injected shadow draw error".into(),
            ));
        }
        Ok(())
    }

    fn end_frame(&mut self) -> Result<(), RenderError> {
        self.0.ended_frames.set(self.0.ended_frames.get() + 1);
        Ok(())
    }

    fn delete_mesh(&mut self, _mesh: ()) {
        self.0.live_meshes.set(self.0.live_meshes.get() - 1);
    }

    fn delete_texture(&mut self, _texture: ()) {
        panic!("mesh-only scenarios never delete textures");
    }

    fn delete_program(&mut self, _program: ()) {}
}

pub fn create(world: &mut WorldContext<'_>, values: Vec<ComponentValue>) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 0,
        metadata: Default::default(),
    }];
    operations.extend(
        values
            .into_iter()
            .map(|value| Command::InsertComponentValue {
                entity: EntityRef::Alias(0),
                value,
            }),
    );
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    update(world).unwrap().outcomes[0].result.as_ref().unwrap()[0].1
}

pub fn setup(
    host: &mut ipp_core::HostRuntime,
) -> (WorldContext<'_>, RenderService<TestDevice>, Rc<DeviceState>) {
    let state = Rc::new(DeviceState::default());
    let renderer = RenderService::new(TestDevice(state.clone())).unwrap();
    renderer.install(host).unwrap();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let entity = create(
        &mut world,
        vec![
            ComponentValue::Camera(Camera::default()),
            ComponentValue::Transform(Transform {
                z: 5.0,
                ..Transform::default()
            }),
        ],
    );
    world.enqueue_camera_activate(entity).unwrap();
    update(&mut world).unwrap();
    (world, renderer, state)
}

#[cfg(feature = "surfaces")]
pub fn triangle_contour(bytes: &mut Vec<u8>) {
    for value in [0.0_f32, 0.0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(3_u32.to_le_bytes());
    for point in [[1.0_f32, 0.0], [0.0, 1.0], [0.0, 0.0]] {
        bytes.extend([0, 0, 0, 0]);
        for value in point {
            bytes.extend(value.to_le_bytes());
        }
    }
}

/// An IPPD drawing of one opaque white triangle layer over the unit square.
#[cfg(feature = "surfaces")]
pub fn surface_drawing() -> Vec<u8> {
    let mut bytes = b"IPPD".to_vec();
    bytes.extend(1_u32.to_le_bytes());
    for value in [0.0_f32, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.01] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend([255, 255, 255, 255, 0, 0, 0, 0]);
    bytes.extend(1_u32.to_le_bytes());
    triangle_contour(&mut bytes);
    bytes
}

#[cfg(feature = "surfaces")]
pub fn surface_font() -> Vec<u8> {
    let mut bytes = b"IPPF".to_vec();
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend(1_000_u32.to_le_bytes());
    for value in [800.0_f32, -200.0, 0.0] {
        bytes.extend(value.to_le_bytes());
    }
    for value in [1_u32, 0, 0] {
        bytes.extend(value.to_le_bytes());
    }
    for value in [600.0_f32, 0.0, 0.0, 0.0, 1.0, 1.0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(1_u32.to_le_bytes());
    triangle_contour(&mut bytes);
    bytes
}

pub fn update(
    world: &mut WorldContext<'_>,
) -> Result<ipp_core::WorldUpdateReport, ipp_core::ErrorReason> {
    world.prepare_update(0.0)?;
    world.poll_all_assets();
    world.step(0.0)
}

/// Complete one World update after `dt` seconds of Host time.
pub fn advance(
    world: &mut WorldContext<'_>,
    dt: f64,
) -> Result<ipp_core::WorldUpdateReport, ipp_core::ErrorReason> {
    world.prepare_update(dt)?;
    world.poll_all_assets();
    world.step(dt)
}

pub fn render_frame<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    world: &mut WorldContext<'_>,
    width: u32,
    height: u32,
) -> Result<ipp_render_gl::RenderStats, String> {
    renderer.begin_frame();
    update(world).map_err(|error| error.to_string())?;
    let result = renderer
        .render(world, width, height)
        .map_err(|error| error.to_string())?;
    if world.asset_resources().iter().any(|resource| {
        resource.source().kind == AssetTypeId(14) && *resource.status() == AssetLoadStatus::Unloaded
    }) {
        update(world).map_err(|error| error.to_string())?;
        return renderer
            .render(world, width, height)
            .map_err(|error| error.to_string());
    }
    Ok(result)
}

/// One text Surface in front of the default camera, with its font resolved.
#[cfg(feature = "surfaces")]
pub fn text_surface_scene(
    host: &mut ipp_core::HostRuntime,
) -> (
    RenderService<TestDevice>,
    Rc<DeviceState>,
    ipp_core::WorldId,
    EntityId,
) {
    text_run_scene(host, surface_font(), &[0])
}

/// A glyph run of `glyph_ids`, one centimetre apart, in the given font.
#[cfg(feature = "surfaces")]
pub fn text_run_scene(
    host: &mut ipp_core::HostRuntime,
    font: Vec<u8>,
    glyph_ids: &[u32],
) -> (
    RenderService<TestDevice>,
    Rc<DeviceState>,
    ipp_core::WorldId,
    EntityId,
) {
    use ipp_core::services::asset_management::{AssetSource, font::FONT_TYPE};
    use ipp_core::{PositionedGlyph, Surface, SurfaceItemContent, SurfaceItemStyle};

    host.data_sources_mut()
        .register_stream("fixture://")
        .unwrap();
    let (mut world, renderer, state) = setup(host);
    let world_id = world.id();

    // A 1 m em five metres away projects to about 24 px: an atlas band, not analytic.
    let mut surface = Surface::default();
    surface
        .insert_item(
            0,
            SurfaceItemContent::GlyphRun(
                glyph_ids
                    .iter()
                    .enumerate()
                    .map(|(index, &glyph_id)| PositionedGlyph {
                        glyph_id,
                        position: [0.01 * index as f32, 0.0],
                        color: None,
                    })
                    .collect(),
            ),
            SurfaceItemStyle {
                position: [0.5, 0.5],
                font_size: 1.0,
                asset: Some(AssetSource {
                    kind: FONT_TYPE,
                    uri: "fixture:///font.ippf".into(),
                    variant: 0,
                }),
                ..Default::default()
            },
        )
        .unwrap();
    let entity = create(
        &mut world,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::Surface(surface),
            ComponentValue::BoundingGeometry(Default::default()),
        ],
    );
    drop(world);

    resolve_text(host, world_id, &font);
    (renderer, state, world_id, entity)
}

/// Complete font requests until the text Surface prepares its glyph run.
#[cfg(feature = "surfaces")]
pub fn resolve_text(host: &mut ipp_core::HostRuntime, world_id: ipp_core::WorldId, font: &[u8]) {
    for _ in 0..16 {
        host.progress_assets();
        for request in host.take_resource_requests() {
            host.complete_resource(request.id, Ok(font.to_vec()))
                .unwrap();
        }
        let mut world = host.world_mut(world_id).unwrap();
        update(&mut world).unwrap();
        if world
            .surface_render_items()
            .first()
            .is_some_and(|item| item.primitives.len() == 1)
        {
            return;
        }
    }
    panic!("text Surface font did not resolve");
}

#[cfg(feature = "surfaces")]
pub fn place(world: &mut WorldContext<'_>, entity: EntityId, z: f32) {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations: vec![Command::InsertComponentValue {
                entity: EntityRef::Handle(entity),
                value: ComponentValue::Transform(Transform {
                    z,
                    ..Transform::default()
                }),
            }],
        })
        .unwrap();
    update(world).unwrap();
}

/// An IPPF font of `count` identical triangle glyphs whose bounds span `extent` units.
#[cfg(feature = "gui")]
pub fn glyph_font(count: u32, units_per_em: u32, extent: f32) -> Vec<u8> {
    let mut bytes = b"IPPF".to_vec();
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend(units_per_em.to_le_bytes());
    for value in [800.0_f32, -200.0, 0.0] {
        bytes.extend(value.to_le_bytes());
    }
    for value in [count, 0, 0] {
        bytes.extend(value.to_le_bytes());
    }
    for _ in 0..count {
        for value in [600.0_f32, 0.0, 0.0, 0.0, extent, extent] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(1_u32.to_le_bytes());
        triangle_contour(&mut bytes);
    }
    bytes
}

/// Host recovery after context loss: release context state, then restore resources.
#[cfg(feature = "surfaces")]
pub fn recover_context(
    renderer: &mut RenderService<TestDevice>,
    host: &mut ipp_core::HostRuntime,
    world_id: ipp_core::WorldId,
    font: &[u8],
) {
    renderer.set_asset_context_active(false);
    renderer.unload_host(host);
    host.flush_resource_lifecycle();
    renderer.set_asset_context_active(true);
    resolve_text(host, world_id, font);
}
