//! Shared failure-injecting render device and World fixtures for renderer
//! integration tests. GL harnesses own image evidence.

#![allow(dead_code)]

pub mod canvas;
pub mod selection;

use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, MeshAsset, WorldContext,
    components::{Camera, Transform},
};
use ipp_render_gl::{RenderDevice, RenderError, RenderService};
use std::cell::RefCell;
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
pub struct DeviceState {
    pub program_creates: Cell<u32>,
    pub program_deletes: Cell<u32>,
    pub failed_attempts_remaining: Cell<u32>,
    pub mesh_attempts: Cell<u32>,
    /// Mesh draw submissions.
    pub mesh_draws: Cell<u32>,
    pub fail_shadow_allocation: Cell<bool>,
    pub shadow_attempts: Cell<u32>,
    pub live_meshes: Cell<u32>,
    pub ended_frames: Cell<u32>,
    pub context_lost: Cell<bool>,
    pub surface_path_attempts: Cell<u32>,
    pub surface_double_sided: Cell<bool>,
    pub surface_state_changes: RefCell<Vec<bool>>,
    pub fail_surface_draw: Cell<bool>,
    pub fail_surface_state_start: Cell<bool>,
    pub live_shadow_maps: Cell<u32>,
    pub shadow_pass: Cell<bool>,
    pub fail_shadow_draw: Cell<bool>,
    pub analytic_glyph_draws: Cell<u32>,
    /// Analytic instance streams created or replaced.
    pub analytic_glyph_uploads: Cell<u32>,
    /// Analytic instance streams currently allocated.
    pub live_analytic_streams: Cell<i32>,
    /// Retained GUI storage writes.
    pub gui_batch_writes: Cell<u32>,
    pub atlas_populations: Cell<u32>,
    pub atlas_target_bound: Cell<bool>,
    pub fail_atlas_begin: RefCell<Option<RenderError>>,
    pub fail_atlas_end: RefCell<Option<RenderError>>,
    pub atlas_restores: Cell<u32>,
    pub atlas_pages_created: Cell<u32>,
    pub fail_gui_batch_write: RefCell<Option<RenderError>>,
    /// Largest cache target dimension; zero (the default) disables caching.
    pub cache_limit: Cell<u32>,
    pub cache_targets_live: Cell<u32>,
    pub cache_creates: Cell<u32>,
    pub cache_deletes: Cell<u32>,
    pub cache_resizes: Cell<u32>,
    pub cache_begins: Cell<u32>,
    pub cache_composites: Cell<u32>,
    pub cache_target_bound: Cell<bool>,
    pub bound_cache_target: Cell<Option<u32>>,
    pub camera_target_bound: Cell<bool>,
    pub fail_cache_create: RefCell<Option<RenderError>>,
    pub fail_cache_begin: RefCell<Option<RenderError>>,
    pub fail_cache_end: RefCell<Option<RenderError>>,
    pub fail_cache_composite: RefCell<Option<RenderError>>,
    /// Curve-path draws, excluding analytic glyph instances.
    pub surface_path_draws: Cell<u32>,
    /// Retained GUI draws of glyph records, each sampling an atlas page.
    pub glyph_batch_draws: Cell<u32>,
    /// Retained GUI draws of shape records.
    pub gui_batch_draws: Cell<u32>,
    /// Model-view-projection of every retained GUI draw, in draw order.
    pub gui_draw_mvps: RefCell<Vec<[f32; 16]>>,
    /// Fragment source of every program creation attempt, in order.
    pub program_fragments: RefCell<Vec<String>>,
    /// Program creations whose fragment source contains this text fail.
    pub fail_program_containing: RefCell<Option<String>>,
    /// Canvas paint parameter vectors of every upload, in order.
    pub paint_block_uploads: RefCell<Vec<Vec<[f32; 4]>>>,
    /// Shape records of every retained GUI storage write, in order.
    pub gui_shapes_written: RefCell<Vec<ipp_render_gl::GuiShapeRecord>>,
    /// Glyph records of every retained GUI storage write, in order.
    pub gui_glyphs_written: RefCell<Vec<ipp_render_gl::GuiGlyphRecord>>,
    /// Kind of every retained GUI storage allocation, by handle.
    pub gui_batch_kinds: RefCell<Vec<ipp_render_gl::GuiRecordKind>>,
    /// Ordered Surface work: `B`/`E` begin and end a cache target, `C` composites,
    /// `P` draws paths, `G` analytic glyphs, `T` retained GUI glyphs sampling an
    /// atlas, `X` retained GUI shapes, `F` begins the frame.
    pub surface_events: RefCell<String>,
}

pub struct TestDevice(pub Rc<DeviceState>);

impl RenderDevice for TestDevice {
    fn viewport_limits(&self) -> Option<ipp_render_gl::ViewportLimits> {
        Some(ipp_render_gl::ViewportLimits {
            max_width: 4096,
            max_height: 4096,
        })
    }

    type SurfacePath = ();
    type SurfaceCacheTarget = u32;
    type SurfaceInstances = ();
    type Program = ();
    type Mesh = ();
    type Texture = ();

    type ShadowMap = ();
    /// Index of the allocation in [`DeviceState::gui_batch_kinds`].
    type GuiBatch = usize;
    type GlyphAtlasPage = ();

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

    fn shadow_map_limit(&self) -> u32 {
        4096
    }

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

    fn begin_shadow(&mut self, _: &(), _: u32, _: u32) -> Result<(), RenderError> {
        assert!(!self.0.shadow_pass.replace(true));
        Ok(())
    }

    fn end_shadow(&mut self) -> Result<(), RenderError> {
        assert!(self.0.shadow_pass.replace(false));
        Ok(())
    }

    fn bind_shadow(
        &mut self,
        _: &(),
        _: &(),
        _: &ipp_render_gl::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    fn delete_shadow_map(&mut self, _: ()) {
        self.0
            .live_shadow_maps
            .set(self.0.live_shadow_maps.get() - 1);
    }

    fn create_program(&mut self, _vertex: &str, fragment: &str) -> Result<(), RenderError> {
        self.0.program_fragments.borrow_mut().push(fragment.into());
        if self
            .0
            .fail_program_containing
            .borrow()
            .as_ref()
            .is_some_and(|marker| fragment.contains(marker.as_str()))
        {
            return Err(RenderError::RenderDevice("injected compile failure".into()));
        }
        self.0.program_creates.set(self.0.program_creates.get() + 1);
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

    fn create_surface_path(
        &mut self,
        _: &ipp_render_gl::SurfacePathTexels,
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
        if !self.0.atlas_target_bound.get() {
            self.0.surface_events.borrow_mut().push('P');
        }
        if self.0.fail_surface_draw.get() {
            Err(RenderError::RenderDevice(
                "injected Surface draw failure".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn create_surface_instances(
        &mut self,
        _: &(),
        _: &[ipp_render_gl::SurfacePathInstance],
    ) -> Result<(), RenderError> {
        self.0
            .analytic_glyph_uploads
            .set(self.0.analytic_glyph_uploads.get() + 1);
        self.0
            .live_analytic_streams
            .set(self.0.live_analytic_streams.get() + 1);
        Ok(())
    }

    fn update_surface_instances(
        &mut self,
        _: &mut (),
        _: &(),
        _: &[ipp_render_gl::SurfacePathInstance],
    ) -> Result<(), RenderError> {
        self.0
            .analytic_glyph_uploads
            .set(self.0.analytic_glyph_uploads.get() + 1);
        Ok(())
    }

    fn delete_surface_instances(&mut self, _: ()) {
        self.0
            .live_analytic_streams
            .set(self.0.live_analytic_streams.get() - 1);
    }

    fn draw_surface_instances(
        &mut self,
        _: &(),
        _: &(),
        _: &(),
        _: &[f32; 16],
        _: &[f32; 4],
        _: u32,
    ) -> Result<(), RenderError> {
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

    fn surface_cache_limit(&self) -> u32 {
        self.0.cache_limit.get()
    }

    fn create_surface_cache_target(&mut self, width: u32, height: u32) -> Result<u32, RenderError> {
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
        Ok(self.0.cache_creates.get())
    }

    fn resize_surface_cache_target(
        &mut self,
        _: &mut u32,
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
    fn begin_camera_target(&mut self, target: &mut u32, _: &[f32; 4]) -> Result<(), RenderError> {
        assert!(!self.0.cache_target_bound.replace(true));
        self.0.bound_cache_target.set(Some(*target));
        self.0.camera_target_bound.set(true);
        self.0.cache_begins.set(self.0.cache_begins.get() + 1);
        Ok(())
    }

    fn begin_surface_cache_target(&mut self, target: &u32) -> Result<(), RenderError> {
        assert!(
            !self.0.cache_target_bound.get(),
            "Surface cache targets never nest"
        );
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
        self.0.bound_cache_target.set(Some(*target));
        Ok(())
    }

    fn end_surface_cache_target(&mut self) -> Result<(), RenderError> {
        self.0.cache_target_bound.set(false);
        self.0.bound_cache_target.set(None);
        self.0.camera_target_bound.set(false);
        self.0.surface_events.borrow_mut().push('E');
        match self.0.fail_cache_end.borrow().clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn draw_surface_cache(
        &mut self,
        _: &(),
        target: &u32,
        _: &[f32; 16],
        _: &[f32; 2],
        _clip: &[f32; 4],
        _opacity: f32,
    ) -> Result<(), RenderError> {
        assert!(
            self.0.surface_double_sided.get(),
            "Surface cache composites run with back-face culling suspended"
        );
        assert_ne!(
            self.0.bound_cache_target.get(),
            Some(*target),
            "a composite cannot sample its bound target"
        );
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

    fn delete_surface_cache_target(&mut self, _: u32) {
        self.0.cache_deletes.set(self.0.cache_deletes.get() + 1);
        self.0
            .cache_targets_live
            .set(self.0.cache_targets_live.get() - 1);
    }

    fn create_gui_batch(
        &mut self,
        kind: ipp_render_gl::GuiRecordKind,
        _: usize,
    ) -> Result<usize, RenderError> {
        let mut kinds = self.0.gui_batch_kinds.borrow_mut();
        kinds.push(kind);
        Ok(kinds.len() - 1)
    }

    fn write_gui_batch<R: ipp_render_gl::GuiRecord>(
        &mut self,
        batch: &mut usize,
        _: usize,
        records: &[R],
    ) -> Result<(), RenderError> {
        assert_eq!(self.0.gui_batch_kinds.borrow()[*batch], R::KIND);
        self.0
            .gui_batch_writes
            .set(self.0.gui_batch_writes.get() + 1);
        let records: Box<dyn std::any::Any> = Box::new(records.to_vec());
        if let Some(shapes) = records.downcast_ref::<Vec<ipp_render_gl::GuiShapeRecord>>() {
            self.0
                .gui_shapes_written
                .borrow_mut()
                .extend_from_slice(shapes);
        }
        if let Some(glyphs) = records.downcast_ref::<Vec<ipp_render_gl::GuiGlyphRecord>>() {
            self.0
                .gui_glyphs_written
                .borrow_mut()
                .extend_from_slice(glyphs);
        }
        match self.0.fail_gui_batch_write.borrow().clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn set_gui_paint_blocks(&mut self, _: &(), blocks: &[[f32; 4]]) -> Result<(), RenderError> {
        self.0
            .paint_block_uploads
            .borrow_mut()
            .push(blocks.to_vec());
        Ok(())
    }

    fn draw_gui_batch(
        &mut self,
        _: &(),
        batch: &usize,
        atlas: Option<&()>,
        mvp: &[f32; 16],
        _: usize,
        _: usize,
    ) -> Result<(), RenderError> {
        assert!(
            !self.0.atlas_target_bound.get(),
            "the main pass never draws into an atlas page"
        );
        let kind = self.0.gui_batch_kinds.borrow()[*batch];
        assert_eq!(
            atlas.is_some(),
            kind == ipp_render_gl::GuiRecordKind::Glyph,
            "glyph storage samples an atlas and shape storage none"
        );
        self.0.gui_draw_mvps.borrow_mut().push(*mvp);
        if atlas.is_some() {
            self.0
                .glyph_batch_draws
                .set(self.0.glyph_batch_draws.get() + 1);
            self.0.surface_events.borrow_mut().push('T');
        } else {
            self.0.gui_batch_draws.set(self.0.gui_batch_draws.get() + 1);
            self.0.surface_events.borrow_mut().push('X');
        }
        Ok(())
    }

    fn create_glyph_atlas_page(&mut self, _: u32, _: u32) -> Result<(), RenderError> {
        self.0
            .atlas_pages_created
            .set(self.0.atlas_pages_created.get() + 1);
        Ok(())
    }

    /// Consecutive begins switch pages; one end restores the host target.
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
        assert!(
            !self.0.cache_target_bound.get(),
            "the frame begins outside cache repaints"
        );
        self.0.surface_events.borrow_mut().push('F');
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        _program: &(),
        _mesh: &(),
        _mvp: &[f32; 16],
        _material: &[f32; 3],
        _pose: Option<(&Self::Mesh, f32)>,
        _texture: Option<&()>,
    ) -> Result<(), RenderError> {
        self.0.mesh_draws.set(self.0.mesh_draws.get() + 1);
        assert!(
            !self.0.surface_double_sided.get(),
            "Surface rasterization state leaked into a mesh draw"
        );
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

    fn delete_program(&mut self, _program: ()) {
        self.0.program_deletes.set(self.0.program_deletes.get() + 1);
    }
}

/// Apply one queued batch and evaluate its World, returning the batch outcome.
pub fn apply_batch(world: &mut WorldContext<'_>, batch: Batch) -> ipp_core::BatchOutcome {
    world.enqueue(batch).unwrap();
    update(world).unwrap().outcomes.remove(0)
}

pub fn create(world: &mut WorldContext<'_>, values: Vec<ComponentValue>) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 0,
        metadata: Default::default(),
        adopt: false,
    }];
    operations.extend(
        values
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(0), value)),
    );
    let id = world.tick() + 1;
    let outcome = apply_batch(
        world,
        Batch {
            id,
            operations,
        },
    );

    outcome.result.unwrap()[0].1
}

/// The presenting scene World: a camera over rendered content, which with
/// Surfaces also presents attached child Worlds on Surface anchors. Fixture
/// Systems a test registered on this Host beyond the compiled ones are selected
/// too, so their hooks observe the scene.
pub fn scene_systems(host: &ipp_core::HostRuntime) -> Vec<ipp_core::systems::SystemId> {
    let parts = [
        selection::ATTACHMENTS,
        selection::CAMERA,
        selection::RENDER,
        selection::SURFACE,
    ];
    let compiled: Vec<_> = ipp_core::systems::compiled_system_factories()
        .iter()
        .map(|factory| factory.id())
        .collect();
    let mut selected = selection::select(&parts);
    selected.extend(host.system_ids().filter(|id| !compiled.contains(id)));
    selected
}

pub fn setup(
    host: &mut ipp_core::HostRuntime,
) -> (WorldContext<'_>, RenderService<TestDevice>, Rc<DeviceState>) {
    let systems = scene_systems(host);
    setup_with(host, &systems)
}

/// [`setup`] with an explicit scene World selection.
pub fn setup_with<'a>(
    host: &'a mut ipp_core::HostRuntime,
    systems: &[ipp_core::systems::SystemId],
) -> (WorldContext<'a>, RenderService<TestDevice>, Rc<DeviceState>) {
    let state = Rc::new(DeviceState::default());
    let renderer = RenderService::new(TestDevice(state.clone())).unwrap();
    renderer.install(host).unwrap();
    let id = host.create_world(Default::default(), systems).unwrap();
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
    drop(world);
    let selection = host
        .bind_output(
            host.world_ref(id).unwrap(),
            entity,
            ipp_core::OutputKind::Camera,
        )
        .unwrap();
    host.set_root_output(
        selection,
        ipp_core::WorldViewport {
            width: 100,
            height: 100,
            device_pixel_ratio: 1.0,
        },
    )
    .unwrap();
    (host.world_mut(id).unwrap(), renderer, state)
}

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

/// A completed render's summary together with its diagnostics statistics.
///
/// Statistics fields are reached through `Deref`, so fixtures read every counter of
/// one frame from one value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    pub draw_calls: u32,
    pub triangles: u32,
    pub failed_draw_calls: u32,
    pub invalid_camera: bool,
    statistics: ipp_render_gl::RenderStatistics,
}

impl FrameStats {
    /// Combine a render's summary with the statistics the renderer kept for it.
    pub fn new(
        summary: ipp_render_gl::RenderFrameSummary,
        statistics: ipp_render_gl::RenderStatistics,
    ) -> Self {
        Self {
            draw_calls: summary.draw_calls,
            triangles: summary.triangles,
            failed_draw_calls: summary.failed_draw_calls,
            invalid_camera: summary.invalid_camera,
            statistics,
        }
    }
}

impl std::ops::Deref for FrameStats {
    type Target = ipp_render_gl::RenderStatistics;

    fn deref(&self) -> &Self::Target {
        &self.statistics
    }
}

impl std::ops::DerefMut for FrameStats {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.statistics
    }
}

/// Render and collect the completed frame's summary and statistics.
pub trait RenderFrameStats {
    fn draw_stats(
        &mut self,
        host: &ipp_core::HostRuntime,
        world: ipp_core::WorldId,
        width: u32,
        height: u32,
    ) -> Result<FrameStats, ipp_render_gl::RenderError>;
}

impl<D: ipp_render_gl::RenderDevice> RenderFrameStats for ipp_render_gl::RenderService<D> {
    fn draw_stats(
        &mut self,
        host: &ipp_core::HostRuntime,
        world: ipp_core::WorldId,
        width: u32,
        height: u32,
    ) -> Result<FrameStats, ipp_render_gl::RenderError> {
        let viewport = ipp_core::WorldViewport {
            width,
            height,
            device_pixel_ratio: 1.0,
        };
        let summary = if let Some((selection, _, publication)) = host.root_output(world) {
            let time = host
                .publication(publication)
                .expect("selected publication")
                .time;
            self.draw(host, selection, publication, viewport, time)?
        } else {
            self.clear(viewport)?
        };
        Ok(FrameStats::new(summary, *self.statistics()))
    }
}

pub fn render_frame<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    host: &mut ipp_core::HostRuntime,
    world: ipp_core::WorldId,
    width: u32,
    height: u32,
) -> Result<FrameStats, String> {
    host.frame(0.0).map_err(|error| error.to_string())?;
    renderer
        .prepare(
            host,
            host.root_output(world)
                .map(|(output, _, publication)| (output, publication)),
        )
        .map_err(|error| error.to_string())?;
    host.progress_assets();
    renderer
        .draw_stats(host, world, width, height)
        .map_err(|error| error.to_string())
}

/// One text Surface in front of the default camera, with its font resolved.
pub fn text_surface_scene(
    host: &mut ipp_core::HostRuntime,
) -> (
    RenderService<TestDevice>,
    Rc<DeviceState>,
    ipp_core::WorldId,
    canvas::CanvasSurface,
) {
    text_run_scene(host, surface_font(), &[0])
}

/// A glyph run of `glyph_ids`, one centimetre apart, in the given font.
pub fn text_run_scene(
    host: &mut ipp_core::HostRuntime,
    font: Vec<u8>,
    glyph_ids: &[u32],
) -> (
    RenderService<TestDevice>,
    Rc<DeviceState>,
    ipp_core::WorldId,
    canvas::CanvasSurface,
) {
    let systems = scene_systems(host);
    text_run_scene_with(host, font, glyph_ids, &systems)
}

/// [`text_run_scene`] with an explicit scene World selection.
pub fn text_run_scene_with(
    host: &mut ipp_core::HostRuntime,
    font: Vec<u8>,
    glyph_ids: &[u32],
    systems: &[ipp_core::systems::SystemId],
) -> (
    RenderService<TestDevice>,
    Rc<DeviceState>,
    ipp_core::WorldId,
    canvas::CanvasSurface,
) {
    host.data_sources_mut()
        .register_stream("fixture://")
        .unwrap();
    let (world, renderer, state) = setup_with(host, systems);
    let world_id = world.id();
    drop(world);
    let surface = canvas::CanvasSurface::new(
        host,
        world_id,
        0.0,
        canvas::glyph_run(glyph_ids, [0.5, 0.5]),
    );

    resolve_text(host, world_id, &font);
    assert_eq!(surface.publication(host).entries.len(), 1);
    (renderer, state, world_id, surface)
}

/// Complete font requests until the text Surface prepares its glyph run.
pub fn resolve_text(host: &mut ipp_core::HostRuntime, world_id: ipp_core::WorldId, font: &[u8]) {
    assert!(host.world_ref(world_id).is_some());
    for _ in 0..16 {
        host.progress_assets();
        for request in host.take_resource_requests() {
            host.complete_resource(request.id, Ok(font.to_vec()))
                .unwrap();
        }
        let report = host.frame(0.0).unwrap();
        assert!(report.worlds.values().all(Result::is_ok));
        assert!(report.publication_errors.is_empty());
        if host.asset_resources().iter().any(|resource| {
            &*resource.source().uri == "fixture:///font.ippf"
                && resource.status()
                    == &ipp_core::services::asset_management::AssetLoadStatus::Loaded
        }) {
            return;
        }
    }
    panic!("text Surface font did not resolve");
}

pub fn place(world: &mut WorldContext<'_>, entity: EntityId, z: f32) {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations: vec![Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::Transform(Transform {
                    z,
                    ..Transform::default()
                }),
            )],
        })
        .unwrap();
    update(world).unwrap();
}

/// An IPPF font of `count` identical triangle glyphs whose bounds span `extent` units.
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
pub fn recover_context(
    renderer: &mut RenderService<TestDevice>,
    host: &mut ipp_core::HostRuntime,
    world_id: ipp_core::WorldId,
    font: &[u8],
) {
    renderer.set_asset_context_active(false);
    renderer.unload_host(host).unwrap();
    host.flush_resource_lifecycle();
    renderer.set_asset_context_active(true);
    resolve_text(host, world_id, font);
}
