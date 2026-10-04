use super::{CanvasPaintEntry, CanvasPublication, system_state::CanvasSystemState};
use crate::services::asset_management::{AssetLifecycleEvent, AssetLifecycleKind};
use crate::systems::{
    System, SystemAssetContext, SystemCapabilities, SystemCapability, SystemCommitContext,
    SystemDependency, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemNumericContext, SystemUpdateContext,
};
use crate::{AttachmentPlacement, ComponentValue, EntityId, ErrorReason, OutputKind, WorldContext};

/// Ordinary entity Canvas evaluation and immutable output production.
#[derive(Default)]
pub struct CanvasSystem {
    pub(super) plot:
        Option<crate::systems::SystemDependencyBinding<crate::systems::plot::PlotSystem>>,
    pub(super) state: CanvasSystemState,
    pub(super) layout:
        Option<crate::systems::SystemDependencyBinding<crate::systems::gui::GuiLayoutSystem>>,
    pub(super) gui: Option<crate::systems::SystemDependencyBinding<crate::systems::gui::GuiSystem>>,
}

impl CanvasSystem {
    /// Stable compiled evaluator identity.
    pub const ID: SystemId = SystemId("ipp.canvas");

    /// Last locally evaluated canvas output.
    pub fn publication(&self) -> Option<&CanvasPublication> {
        self.state.publication.as_ref()
    }

    /// Classify one changed component. A retained style or leaf value, or a
    /// sampled GUI transition, re-walks its entity's subtree, and a retained
    /// paint's property write re-reads that paint; the next evaluation patches
    /// them unless another change requires the whole walk. Every other input
    /// change walks the whole canvas, and a theme change also repaints every
    /// retained control part.
    fn changed(&mut self, entity: EntityId, component: u16, retained: bool, sample: bool) {
        let patch = &mut self.state.patch;
        match component {
            // Optional bounds may be added to a retained layout-only entity.
            // Populate a new store slot; retained system-owned writes are outputs.
            ComponentValue::CANVAS_BOUNDS if !retained => self.state.dirty = true,
            ComponentValue::GUI_THEME => self.state.gui_dirty = true,
            ComponentValue::GUI_BEHAVIOR if sample => {
                patch.subtrees.insert(entity);
            }
            ComponentValue::CANVAS_STYLE if retained => {
                patch.subtrees.insert(entity);
            }
            ComponentValue::CANVAS_PAINT if retained => {
                patch.paints.insert(entity);
            }
            component if retained && super::walk::LEAF_COMPONENTS.contains(&component) => {
                patch.subtrees.insert(entity);
                self.state.geometry_dirty.insert((entity, component));
            }
            component if input_component(component) => {
                self.state.dirty = true;
                self.state.geometry_dirty.insert((entity, component));
            }
            component if gui_input_component(component) => self.state.dirty = true,
            _ => {}
        }
    }

    fn invalidate_resource(
        &mut self,
        context: &SystemAssetContext<'_>,
        event: &AssetLifecycleEvent,
    ) {
        if event.kind == AssetLifecycleKind::GraphicsInvalidated {
            return;
        }
        self.state.gui_dirty = true;

        for (&(entity, component), leaf) in &self.state.leaves {
            if leaf
                .entry
                .as_ref()
                .is_some_and(|entry| match entry.as_ref() {
                    super::CanvasPaintEntry::Primitive {
                        primitive,
                        ..
                    } => primitive.resource() == Some(event.key),
                    super::CanvasPaintEntry::Attachment(_) => false,
                })
                || super::update::resource_key(
                    context.world.world,
                    context.world.asset_resources(),
                    entity,
                    component,
                ) == Some(event.key)
            {
                self.state.dirty = true;
                self.state.geometry_dirty.insert((entity, component));
            }
        }
    }
}

/// Reusable factory for raw Canvas output without GUI or spatial evaluation.
#[derive(Default)]
pub struct CanvasSystemFactory;

impl SystemFactory for CanvasSystemFactory {
    fn id(&self) -> SystemId {
        CanvasSystem::ID
    }

    fn capabilities(&self) -> SystemCapabilities {
        let mut capabilities = SystemCapabilities::new(
            [
                ComponentValue::CANVAS_STYLE,
                ComponentValue::CANVAS_BOX,
                ComponentValue::CANVAS_BOUNDS,
            ],
            [crate::systems::WorldOperation::Canvas],
        );
        capabilities.components.extend(
            [
                ComponentValue::CANVAS_TEXT,
                ComponentValue::CANVAS_GLYPH_RUN,
                ComponentValue::CANVAS_DRAWING,
                ComponentValue::CANVAS_BITMAP,
                ComponentValue::CANVAS_PAINT,
            ]
            .map(|component| {
                SystemCapability::requiring(
                    component,
                    [crate::systems::asset_dependencies::AssetDependencySystem::ID],
                )
            }),
        );

        capabilities
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[
            SystemDependency::After(crate::systems::plot::PlotSystem::ID),
            SystemDependency::After(crate::systems::animation::AnimationSystem::ID),
            SystemDependency::After(crate::systems::asset_dependencies::AssetDependencySystem::ID),
            SystemDependency::After(crate::systems::gui::GuiLayoutSystem::ID),
            SystemDependency::After(crate::systems::gui::GuiSystem::ID),
        ]
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(CanvasSystem {
            plot: context
                .dependency::<crate::systems::plot::PlotSystem>(
                    crate::systems::plot::PlotSystem::ID,
                )
                .ok(),
            gui: context
                .dependency::<crate::systems::gui::GuiSystem>(crate::systems::gui::GuiSystem::ID)
                .ok(),
            layout: context
                .dependency::<crate::systems::gui::GuiLayoutSystem>(
                    crate::systems::gui::GuiLayoutSystem::ID,
                )
                .ok(),
            ..Default::default()
        }))
    }
}

impl System for CanvasSystem {
    fn world_output(&self, kind: OutputKind) -> bool {
        kind == OutputKind::Canvas
    }

    fn command(
        &mut self,
        _context: &mut crate::systems::SystemCommandContext<'_>,
        _session: u64,
        command: &dyn std::any::Any,
    ) -> Result<(), ErrorReason> {
        let update = command
            .downcast_ref::<super::CanvasStateUpdate>()
            .ok_or(ErrorReason::InvalidValue)?;
        match update.applied_to(self.state.canvas) {
            Ok(state) => {
                if state != self.state.canvas {
                    crate::diagnostic!(Debug, "[IPP core] canvas_state.update state={state:?}");
                    self.state.canvas = state;
                }
                Ok(())
            }
            Err(reason) => {
                crate::diagnostic!(Warn, "[IPP core] canvas_state.reject reason={reason}");
                Err(reason)
            }
        }
    }

    fn save_persistent_state(
        &self,
        context: &mut crate::systems::SystemSaveContext<'_>,
    ) -> Result<Option<crate::systems::SystemPersistentState>, String> {
        let state = super::canvas_state::encode_persistent(self.state.canvas);
        *context.bytes = context
            .bytes
            .checked_add(state.len())
            .ok_or("Snapshot size overflow")?;
        if *context.bytes > context.max_bytes {
            return Err("Snapshot byte budget exhausted".into());
        }
        Ok(Some(state))
    }

    fn load_persistent_state(
        &mut self,
        _context: &mut crate::systems::SystemLoadContext<'_, '_>,
        state: Option<&crate::systems::SystemPersistentState>,
    ) -> Result<(), String> {
        self.state.canvas = match state {
            Some(bytes) => super::canvas_state::decode_persistent(bytes)?,
            None => super::CanvasState::default(),
        };
        Ok(())
    }

    fn attachment_placement(
        &self,
        world: &WorldContext<'_>,
        anchor: EntityId,
    ) -> AttachmentPlacement {
        // Every Surface anchor in a canvas World is a canvas slot; spatial
        // attachments keep their spatial placement.
        if world
            .world
            .components
            .world_attachment(anchor.index() as usize)
            .is_none_or(|attachment| attachment.mode == 0)
        {
            return AttachmentPlacement::Unmanaged;
        }
        let owner = world.canvas_output();
        let unavailable = AttachmentPlacement::Unavailable {
            owner,
        };
        let Some(publication) = self
            .state
            .publication
            .as_ref()
            .filter(|publication| publication.selection == owner)
        else {
            return unavailable;
        };
        let Some(entry) = self.state.slots.get(&anchor) else {
            return unavailable;
        };
        let CanvasPaintEntry::Attachment(slot) = entry.as_ref() else {
            return unavailable;
        };
        let token = world
            .topology
            .tokens
            .get(&crate::host::topology::AttachmentAnchor {
                world: world.id(),
                entity: anchor,
            });
        if token != Some(&slot.token) {
            return unavailable;
        }

        AttachmentPlacement::Ready {
            owner,
            affine: slot.parent_affine(publication.logical_extent, publication.units_per_metre),
        }
    }

    fn publish_output(
        &self,
        world: &crate::WorldContext<'_>,
        output: &mut crate::host::WorldOutputBuilder<'_>,
    ) -> Result<(), ErrorReason> {
        if self.gui.is_some() {
            output.chunk(Self::ID, self.state.gui.publication.clone());
        }
        if let Some(publication) = self
            .state
            .publication
            .as_ref()
            .filter(|publication| publication.selection == world.canvas_output())
        {
            for resource in publication.resources() {
                output.retain(resource);
            }
            output.output(publication.selection, publication.clone());
        }
        Ok(())
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        self.evaluate(&context.world);
        for (entity, [x, y, width, height]) in std::mem::take(&mut self.state.bounds) {
            let Some(current) = context
                .world
                .world
                .components
                .canvas_bounds_mut(entity.index() as usize)
            else {
                continue;
            };
            let bounds = super::CanvasBounds {
                x,
                y,
                width,
                height,
            };
            if *current != bounds {
                *current = bounds;
            }
        }
    }

    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        for (entity, component) in context.changed_components() {
            let retained = context.retains_component(entity, component);
            self.changed(entity, component, retained, false);
            if input_component(component) && !retained {
                self.state.leaves.remove(&(entity, component));
            }
        }
        if context.changed_entity_links().next().is_some() {
            self.state.tree_dirty = true;
        }
    }

    fn before_numeric_update(&mut self, context: &mut SystemNumericContext<'_>) {
        for &(entity, component) in context.changed_components() {
            // GUI writes a sampled transition into its control's GuiBehavior,
            // which changes only that control's paint.
            let sample = component == ComponentValue::GUI_BEHAVIOR
                && context
                    .world_data
                    .components
                    .gui_behavior(entity.index() as usize)
                    .is_some_and(|behavior| behavior.motion.notifying_sample);
            self.changed(entity, component, true, sample);
        }
    }

    fn before_asset_release(
        &mut self,
        context: &mut SystemAssetContext<'_>,
        event: &AssetLifecycleEvent,
    ) {
        if event.kind != AssetLifecycleKind::GraphicsInvalidated {
            self.invalidate_resource(context, event);
            self.state.gui.invalidate_resource(event.key);
            if self.state.publication.as_ref().is_some_and(|publication| {
                publication
                    .resources()
                    .any(|resource| resource == event.key)
            }) {
                self.state.publication = None;
            }
        }
    }

    fn asset_lifecycle(
        &mut self,
        context: &mut SystemAssetContext<'_>,
        event: &AssetLifecycleEvent,
    ) {
        self.invalidate_resource(context, event);
    }
}

fn input_component(component: u16) -> bool {
    crate::systems::surface::is_provider(component)
        || matches!(
            component,
            ComponentValue::CANVAS_STYLE
                | ComponentValue::CANVAS_TEXT
                | ComponentValue::CANVAS_GLYPH_RUN
                | ComponentValue::CANVAS_DRAWING
                | ComponentValue::CANVAS_BITMAP
                | ComponentValue::CANVAS_BOX
                | ComponentValue::CANVAS_PAINT
                | ComponentValue::WORLD_ATTACHMENT
        )
}

fn gui_input_component(component: u16) -> bool {
    matches!(
        component,
        ComponentValue::GUI_BEHAVIOR
            | ComponentValue::GUI_BUTTON
            | ComponentValue::GUI_CHECKBOX
            | ComponentValue::GUI_SLIDER
            | ComponentValue::GUI_TEXT_INPUT
            | ComponentValue::GUI_SCROLL_VIEW
            | ComponentValue::GUI_VIRTUAL_LIST
            | ComponentValue::GUI_COLOR
            | ComponentValue::GUI_GROUP
            | ComponentValue::GUI_THEME
            | ComponentValue::GUI_SKIN
            | ComponentValue::GUI_FONT
    )
}
