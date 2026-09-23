//! RenderSystem: factory configuration and exclusively owned per-world state.

#[cfg(feature = "gui")]
use super::gui_presentation::GuiSkinPresentation;
#[cfg(all(test, feature = "surfaces", feature = "gui"))]
use super::gui_presentation::skin_controller_description;
#[cfg(all(feature = "surfaces", feature = "gui"))]
use super::gui_presentation::skin_paint_for_view;
use super::{RenderReadAccess, RenderSystemState};
use crate::systems::{
    System, SystemDependency, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemTeardownContext, SystemUpdateContext,
};

#[cfg(all(test, feature = "gui"))]
std::thread_local! {
    /// Render preparation passes and skin reconciliations on this thread.
    pub(super) static PREPARATION_COUNTS: std::cell::Cell<(usize, usize)> =
        const { std::cell::Cell::new((0, 0)) };
}

/// Render preparation passes and skin reconciliations on this thread since
/// the previous call.
#[cfg(all(test, feature = "gui"))]
pub(super) fn take_preparation_counts() -> (usize, usize) {
    PREPARATION_COUNTS.with(|counts| counts.replace((0, 0)))
}

/// Fresh runtime state owned by one World.
#[derive(Default)]
pub struct RenderSystem {
    pub(in crate::world) state: RenderSystemState,
    bindings: crate::systems::SystemBindings<Self>,
    prepared_dirty: bool,
    #[cfg(feature = "surfaces")]
    surface_prepared_dirty: bool,
    #[cfg(feature = "surfaces")]
    surface_primitives_dirty: bool,
    /// Consumed GUI paint revisions by root entity. Compared every frame
    /// so retained layout output rebuilds Surface primitives only when
    /// paint actually changed.
    #[cfg(feature = "gui")]
    pub(super) gui_paint_revisions: std::collections::BTreeMap<crate::EntityId, u64>,
    /// Consumed input-owned scroll revision. Compared alongside paint
    /// revisions so a pure scroll refreshes translated primitives without
    /// reflowing layout.
    #[cfg(feature = "gui")]
    pub(super) gui_scroll_revision: u64,
    /// Consumed skin interaction cursors. Compared every frame so hover,
    /// press and focus changes refresh skinned paint without reflowing
    /// layout; paint revisions never observe input-owned cursors.
    #[cfg(feature = "gui")]
    pub(super) gui_skin_cursors: crate::systems::gui::GuiSkinCursors,
    /// Consumed transient caret generation. Compared every frame so caret,
    /// selection and composition paint refreshes without touching committed
    /// state or retained views.
    #[cfg(feature = "gui")]
    pub(super) gui_caret_revision: u64,
    /// Last ready asset selected by each skinned primitive. Pending skin
    /// replacements retain this resource and publish matching RenderSystem
    /// demand until their new source is ready.
    #[cfg(feature = "gui")]
    pub(super) gui_skin_resources: std::collections::BTreeMap<
        (crate::EntityId, crate::systems::surface::GuiPrimitiveId),
        crate::systems::surface::SurfaceRenderResource,
    >,
    /// Last authored destination and ordinary AnimationSystem controller for
    /// each full-fenced GUI primitive.
    #[cfg(feature = "gui")]
    pub(super) gui_skin_presentations: std::collections::BTreeMap<
        (crate::EntityId, crate::systems::surface::GuiPrimitiveId),
        GuiSkinPresentation,
    >,
    /// Effective numeric skin appearance consumed by prepared Surface paint.
    #[cfg(feature = "gui")]
    pub(super) gui_skin_overrides: std::collections::BTreeMap<
        (crate::EntityId, crate::systems::surface::GuiPrimitiveId),
        crate::systems::gui::GuiSkinnedAppearance,
    >,
    /// Aggregate private animation work queued at the next mutation boundary.
    #[cfg(feature = "gui")]
    pub(super) pending_skin_animation_commands:
        Vec<crate::systems::animation::AnimationInternalCommand>,
    /// Monotonic renderer request fence for derived skin controller intents.
    #[cfg(feature = "gui")]
    pub(super) next_skin_animation_request: u64,
    /// Inputs of the latest skin reconciliation; None forces the next one.
    #[cfg(feature = "gui")]
    pub(super) gui_skin_key: Option<super::gui_presentation::GuiSkinReconcileKey>,
}

impl RenderSystem {
    /// Stable factory and instance identity.
    pub const ID: SystemId = SystemId("ipp.render");
}

/// Reusable factory; it retains no mutable world state.
#[derive(Default)]
pub struct RenderSystemFactory;

impl SystemFactory for RenderSystemFactory {
    fn id(&self) -> SystemId {
        RenderSystem::ID
    }

    fn dependencies(&self) -> &[SystemDependency] {
        <RenderSystem as crate::systems::SystemBoundUpdate>::dependencies()
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(RenderSystem {
            state: Default::default(),
            bindings: crate::systems::SystemBindings::resolve(context)?,
            prepared_dirty: false,
            #[cfg(feature = "surfaces")]
            surface_prepared_dirty: true,
            #[cfg(feature = "surfaces")]
            surface_primitives_dirty: true,
            #[cfg(feature = "gui")]
            gui_paint_revisions: Default::default(),
            #[cfg(feature = "gui")]
            gui_scroll_revision: 0,
            #[cfg(feature = "gui")]
            gui_skin_cursors: Default::default(),
            #[cfg(feature = "gui")]
            gui_caret_revision: 0,
            #[cfg(feature = "gui")]
            gui_skin_resources: Default::default(),
            #[cfg(feature = "gui")]
            gui_skin_presentations: Default::default(),
            #[cfg(feature = "gui")]
            gui_skin_overrides: Default::default(),
            #[cfg(feature = "gui")]
            pending_skin_animation_commands: Default::default(),
            #[cfg(feature = "gui")]
            next_skin_animation_request: 1,
            #[cfg(feature = "gui")]
            gui_skin_key: None,
        }))
    }
}

impl System for RenderSystem {
    fn before_numeric_update(&mut self, context: &mut crate::systems::SystemNumericContext<'_>) {
        self.prepared_dirty = true;
        #[cfg(not(feature = "surfaces"))]
        let _ = context;
        #[cfg(feature = "surfaces")]
        {
            self.surface_prepared_dirty |= context
                .changed_components()
                .iter()
                .any(|(_, component)| surface_prepared_component(*component));
            self.surface_primitives_dirty |= context
                .changed_components()
                .iter()
                .any(|(_, component)| *component == crate::ComponentValue::SURFACE);
        }
    }

    fn before_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        if context.changed_components().next().is_some() {
            self.prepared_dirty = true;
            #[cfg(feature = "surfaces")]
            {
                for (_, component) in context.changed_components() {
                    self.surface_prepared_dirty |= surface_prepared_component(component);
                    self.surface_primitives_dirty |= component == crate::ComponentValue::SURFACE;
                }
            }
            self.state.entries.clear();
            self.state.debug_entries.clear();
            self.state.light_entries.clear();
            self.state.entries_ready = false;
        }
    }

    fn before_asset_release(
        &mut self,
        _context: &mut crate::systems::SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        self.state.entries.clear();
        self.state.entries_ready = false;
        let released = event.key.to_u64();
        let previous = self.state.items.len();
        self.state.items.retain(|item| {
            if item.mesh.asset == released {
                return false;
            }
            #[cfg(feature = "mesh-poses")]
            if item.pose.is_some_and(|(mesh, _)| mesh.asset == released) {
                return false;
            }
            if item
                .texture
                .is_some_and(|texture| texture.asset == released)
            {
                return false;
            }
            true
        });
        // Debug rows own their copied primitive data and use renderer-private assets.
        // Unrelated prepared output remains usable after this Host resource release.
        self.prepared_dirty |= self.state.items.len() != previous;
        #[cfg(feature = "surfaces")]
        {
            let previous = self
                .state
                .surface_items
                .iter()
                .map(|item| item.primitives.len())
                .sum::<usize>();
            for item in &mut self.state.surface_items {
                item.primitives
                    .retain(|primitive| !surface_primitive_uses_asset(primitive, event.key));
            }
            let remaining = self
                .state
                .surface_items
                .iter()
                .map(|item| item.primitives.len())
                .sum::<usize>();
            if remaining != previous {
                self.prepared_dirty = true;
                self.surface_prepared_dirty = true;
                self.surface_primitives_dirty = true;
            }
        }
        #[cfg(feature = "gui")]
        self.gui_skin_resources
            .retain(|_, resource| resource.key != event.key);
    }

    fn asset_lifecycle(
        &mut self,
        _context: &mut crate::systems::SystemAssetContext<'_>,
        _event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        self.state.entries.clear();
        self.state.entries_ready = false;
        self.prepared_dirty = true;
        #[cfg(feature = "surfaces")]
        {
            self.surface_prepared_dirty = true;
            self.surface_primitives_dirty = true;
        }
        #[cfg(feature = "gui")]
        if _event.kind == crate::services::asset_management::AssetLifecycleKind::Removed
            || _event.status == crate::services::asset_management::AssetLoadStatus::Unloaded
        {
            let world = _context.world.id();
            self.update_gui_skin_resource_demand(world, _context.world.asset_acquisition);
        }
    }

    #[cfg(feature = "mesh-poses")]
    fn validate_commit(
        &self,
        context: &crate::systems::SystemCommitContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        RenderReadAccess::new(context.world_data, context.assets, &self.state)
            .validate_mesh_pose_changes(context.staged)
    }

    fn command(
        &mut self,
        _context: &mut crate::systems::SystemCommandContext<'_>,
        _session: u64,
        command: &dyn std::any::Any,
    ) -> Result<(), crate::ErrorReason> {
        let patch = command
            .downcast_ref::<crate::RenderStatePatch>()
            .ok_or(crate::ErrorReason::InvalidValue)?;
        match self.update_render_state(*patch) {
            Ok(changes) => {
                if let Some(changes) = changes {
                    self.state.state_changes.push(changes);
                }
                Ok(())
            }
            Err(reason) => {
                crate::diagnostic!(Warn, "[IPP core] render_state.reject reason={reason}");
                Err(reason)
            }
        }
    }

    fn finish_update(
        &mut self,
        context: &mut SystemUpdateContext<'_, '_>,
        report: &mut crate::WorldUpdateReport,
    ) {
        // The evaluated pass already observed final GUI layout, input and
        // skin controller state; only changes committed after it (deferred
        // removals, later numeric writes) or pending resources prepare again.
        // Skin animation requests queued below are acknowledged at the next
        // mutation boundary, which the next frame's pass observes.
        let needs_update = self.prepared_dirty || !self.state.entries_ready;
        if needs_update {
            <Self as crate::systems::SystemBoundUpdate>::update_bound(
                self,
                self.bindings.get(),
                context,
            );
        }
        #[cfg(feature = "gui")]
        if !self.pending_skin_animation_commands.is_empty() {
            let commands = std::mem::take(&mut self.pending_skin_animation_commands);
            if context
                .world
                .enqueue_internal_system_command(
                    crate::systems::animation::AnimationSystem::ID,
                    crate::systems::animation::AnimationCommand::Internal(commands),
                )
                .is_err()
            {
                // Presentations retain their unacknowledged request and retry
                // at the next boundary without advancing authored state.
                self.gui_skin_key = None;
            }
        }
        report
            .render_state_changes
            .extend(
                self.state
                    .state_changes
                    .drain(..)
                    .map(|changes| crate::RenderStateChange {
                        tick: report.tick,
                        changes,
                    }),
            );
    }

    fn teardown(&mut self, _context: &mut SystemTeardownContext<'_>) {
        self.state = Default::default();

        #[cfg(feature = "gui")]
        {
            let world = _context.world.id();
            _context
                .asset_resources()
                .update_system_users(world, Self::ID.0, Default::default());
            self.gui_skin_cursors = Default::default();
            self.gui_skin_resources.clear();
            self.gui_skin_presentations.clear();
            self.gui_skin_overrides.clear();
            self.pending_skin_animation_commands.clear();
            self.next_skin_animation_request = 1;
            self.gui_skin_key = None;
        }
    }

    crate::system_update!(bindings);
}

#[crate::systems::system_update(
    SystemDependency::Required(crate::systems::geometry::GeometrySystem::ID),
    SystemDependency::After(crate::systems::camera::CameraSystem::ID),
    SystemDependency::After(SystemId("ipp.particles"))
)]
impl RenderSystem {
    #[allow(clippy::too_many_arguments)] // The generated hook keeps each dependency explicit.
    fn update(
        &mut self,
        ecs: crate::systems::SystemEcsAccess<'_>,
        assets: &mut crate::services::asset_management::AssetManagementService,
        #[cfg(feature = "skeletal-animation")] skeleton: Option<
            &crate::systems::skeleton::SkeletonSystem,
        >,
        #[cfg(feature = "skeletal-animation")] skinning: Option<
            &crate::systems::skinning::SkinningSystem,
        >,
        #[cfg(feature = "gui")] animation: &crate::systems::animation::AnimationSystem,
        #[cfg(feature = "gui")] gui_layout: Option<&crate::systems::gui::GuiLayoutSystem>,
        #[cfg(feature = "gui")] gui_input: Option<&crate::systems::gui::GuiInputSystem>,
        _dt: f64,
    ) {
        #[cfg(all(test, feature = "gui"))]
        PREPARATION_COUNTS.with(|counts| {
            let (passes, reconciliations) = counts.get();
            counts.set((passes + 1, reconciliations));
        });

        if !self.state.entries_ready {
            let mut entries = std::mem::take(&mut self.state.entries);
            let mut debug_entries = std::mem::take(&mut self.state.debug_entries);
            let mut light_entries = std::mem::take(&mut self.state.light_entries);
            let mut diagnostics = std::mem::take(&mut self.state.compatibility_diagnostics);
            diagnostics.clear();
            let read = RenderReadAccess::new(ecs.world, assets, &self.state);
            let pending_resources = read.compile_entries(&mut entries);
            read.compile_auxiliary_entries(&mut debug_entries, &mut light_entries);
            diagnostics = read.prepare_render_diagnostics(diagnostics);
            self.state.entries = entries;
            self.state.debug_entries = debug_entries;
            self.state.light_entries = light_entries;
            self.state.compatibility_diagnostics = diagnostics;
            self.state.entries_ready = !pending_resources;
            self.state.items.clear();
        }
        let (mut items, debug_items) = if crate::allocation_optimizations_enabled() {
            (
                std::mem::take(&mut self.state.items),
                std::mem::take(&mut self.state.debug_items),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        let mut diagnostics = if crate::render_buffer_reuse_enabled() {
            std::mem::take(&mut self.state.diagnostics)
        } else {
            Vec::new()
        };
        diagnostics.clear();
        #[cfg(feature = "skeletal-animation")]
        if let Some(skeleton) = skeleton {
            diagnostics.extend(skeleton.state.skeleton_diagnostics.iter().cloned());
        }
        #[cfg(feature = "skeletal-animation")]
        if let Some(skinning) = skinning {
            diagnostics.extend(skinning.state.diagnostics.iter().cloned());
        }
        diagnostics.extend(self.state.compatibility_diagnostics.iter().cloned());
        let mut written = 0;
        for entry in &self.state.entries {
            entry.append(ecs.world, &mut items, &mut written);
        }
        items.truncate(written);
        let debug_items = {
            let world = RenderReadAccess::new(ecs.world, assets, &self.state);
            world.prepare_debug_render_items(debug_items)
        };
        #[cfg(all(feature = "surfaces", feature = "gui"))]
        if let Some(gui_layout) = gui_layout {
            // Retained GUI paint invalidates Surface primitives only when
            // a paint revision moved, covering layout edits, paint-only
            // edits, font readiness and root removal alike. Input-owned
            // scroll offsets never reflow layout, so the scroll revision
            // joins the compare: a pure scroll refreshes primitives with
            // translated geometry while paint revisions hold still.
            let current: std::collections::BTreeMap<crate::EntityId, u64> =
                gui_layout.paint_states().into_iter().collect();
            let scroll_revision = gui_input.map(|input| input.scroll_revision()).unwrap_or(0);
            if current != self.gui_paint_revisions || scroll_revision != self.gui_scroll_revision {
                self.surface_prepared_dirty = true;
                self.surface_primitives_dirty = true;
            }
            // Skin cursors are input-owned like scroll offsets: a hover,
            // press or focus change refreshes skinned paint while every
            // revision above holds still.
            let skin_cursors = gui_input
                .map(|input| input.skin_cursors())
                .unwrap_or_default();
            if skin_cursors != self.gui_skin_cursors {
                self.surface_prepared_dirty = true;
                self.surface_primitives_dirty = true;
                self.gui_skin_cursors = skin_cursors;
            }
            // Caret paint is transient like skin cursors: caret, selection
            // and composition changes refresh derived paint while committed
            // revisions hold still.
            let caret_revision = gui_input.map(|input| input.caret_revision()).unwrap_or(0);
            if caret_revision != self.gui_caret_revision {
                self.surface_prepared_dirty = true;
                self.surface_primitives_dirty = true;
                self.gui_caret_revision = caret_revision;
            }

            if self.update_gui_skin_presentations(ecs.world, assets, animation, gui_layout) {
                self.surface_prepared_dirty = true;
                self.surface_primitives_dirty = true;
            }
        }
        #[cfg(feature = "gui")]
        let surface_prepared = self.surface_prepared_dirty;
        #[cfg(feature = "surfaces")]
        if self.surface_prepared_dirty {
            let mut surface_items = std::mem::take(&mut self.state.surface_items);
            let mut surface_layout_cache = std::mem::take(&mut self.state.surface_layout_cache);
            crate::systems::surface::rendering::prepare_surface_render_items(
                ecs.world,
                assets,
                &mut surface_layout_cache,
                &mut surface_items,
                self.surface_primitives_dirty,
            );
            #[cfg(feature = "gui")]
            if let Some(gui_layout) = gui_layout {
                let world_id = ecs.id();
                let world = &*ecs.world;
                for item in &mut surface_items {
                    if let Some(view) = gui_layout.view(item.entity) {
                        // Skin FIRST: resolve interaction-driven control
                        // styling into prepared primitives. Idle views and
                        // nodes without skin parts keep base paint; only
                        // style fields, swappable drawing/bitmap resources
                        // and focus borders change, never order, identity,
                        // geometry or clips.
                        let primitives = skin_paint_for_view(
                            world_id,
                            world,
                            assets,
                            &self.gui_skin_cursors,
                            view,
                            item.entity,
                            &mut self.gui_skin_resources,
                            &self.gui_skin_overrides,
                        );
                        // Scrolled paint translates retained content-metre
                        // geometry by each node's ancestor shift; viewport
                        // clips stay fixed while descendants shift beneath
                        // them. Without a scroll revision nothing moved, so
                        // retained bytes pass through untouched. Translation
                        // preserves the skinned style and color.
                        let mut translated = match gui_input {
                            Some(input) if input.scroll_revision() != 0 => {
                                let units = view.units_per_metre;
                                let shifts =
                                    input.scroll_shifts_logical(gui_layout, ecs.world, item.entity);
                                if shifts.is_empty() || !units.is_finite() || units <= 0.0 {
                                    primitives
                                } else {
                                    let metres: std::collections::BTreeMap<_, _> = shifts
                                        .into_iter()
                                        .map(|(id, shift)| {
                                            (id, [shift[0] / units, shift[1] / units])
                                        })
                                        .collect();
                                    crate::systems::surface::rendering::translate_gui_primitives_for_scroll(
                                        primitives, &metres,
                                    )
                                }
                            }
                            _ => primitives,
                        };
                        // Caret composition: transient text paint over the
                        // skinned, scrolled primitives. Overlays arrive
                        // scroll-shifted in content metres with the retained
                        // node identity and clip, so they track scrolled
                        // content exactly like the text they annotate.
                        if let Some(input) = gui_input {
                            translated.extend(input.text_caret_primitives(
                                gui_layout,
                                world,
                                item.entity,
                                world_id,
                                assets,
                            ));
                        }
                        crate::systems::surface::rendering::append_gui_surface_primitives(
                            item,
                            translated,
                            item.clip_size,
                        );
                    }
                }
                self.gui_paint_revisions = gui_layout.paint_states().into_iter().collect();
                self.gui_scroll_revision =
                    gui_input.map(|input| input.scroll_revision()).unwrap_or(0);
                let live: std::collections::BTreeSet<_> = surface_items
                    .iter()
                    .flat_map(|item| {
                        item.primitives.iter().filter_map(move |primitive| {
                            let crate::systems::surface::SurfacePrimitiveIdentity::Gui(id) =
                                primitive.style().identity
                            else {
                                return None;
                            };
                            Some((item.entity, id))
                        })
                    })
                    .collect();
                self.gui_skin_resources
                    .retain(|identity, _| live.contains(identity));
                self.update_gui_skin_resource_demand(ecs.id(), assets);
            }
            self.state
                .surface_cache_inputs
                .publish(ecs.world, &mut surface_items);
            self.state.surface_layout_cache = surface_layout_cache;
            self.state.surface_items = surface_items;
            self.surface_prepared_dirty = false;
            self.surface_primitives_dirty = false;
        }

        // Interaction priority follows live input cursors every frame without
        // re-preparing primitives; skin paint for the same cursors is handled
        // by the preparation above.
        #[cfg(feature = "gui")]
        self.state.surface_cache_inputs.publish_interaction(
            gui_input
                .map(|input| input.interaction_roots())
                .unwrap_or_default(),
            &mut self.state.surface_items,
            surface_prepared,
        );
        self.prepared_dirty = false;
        self.state.items = items;
        self.state.debug_items = debug_items;
        self.state.diagnostics = diagnostics;
    }
}

#[cfg(feature = "surfaces")]
fn surface_primitive_uses_asset(
    primitive: &crate::systems::surface::SurfaceRenderPrimitive,
    key: crate::services::asset_management::AssetKey,
) -> bool {
    match primitive {
        crate::systems::surface::SurfaceRenderPrimitive::Glyphs {
            font,
            ..
        } => font.key == key,
        crate::systems::surface::SurfaceRenderPrimitive::Drawing {
            drawing,
            ..
        } => drawing.key == key,
        crate::systems::surface::SurfaceRenderPrimitive::Bitmap {
            bitmap,
            ..
        } => bitmap.key == key,
        #[cfg(feature = "gui")]
        crate::systems::surface::SurfaceRenderPrimitive::Box {
            ..
        } => false,
    }
}

/// Component changes that require Surface re-preparation: placement inputs,
/// the Surface itself and its optional cache policy.
#[cfg(feature = "surfaces")]
fn surface_prepared_component(component: u16) -> bool {
    surface_model_component(component)
        || component == crate::ComponentValue::SURFACE
        || component == crate::ComponentValue::SURFACE_CACHE
}

#[cfg(feature = "surfaces")]
fn surface_model_component(component: u16) -> bool {
    let relevant = matches!(
        component,
        crate::ComponentValue::TRANSFORM
            | crate::ComponentValue::HIERARCHY
            | crate::ComponentValue::LOOK_AT
    );
    #[cfg(feature = "skeletal-animation")]
    let relevant = relevant || component == crate::ComponentValue::SKELETON;
    relevant
}

#[cfg(all(test, feature = "surfaces"))]
#[path = "surface_preparation_tests.rs"]
mod tests;
