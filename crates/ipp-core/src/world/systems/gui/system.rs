use crate::systems::{
    System, SystemDependency, SystemFactory, SystemId, SystemInitContext, SystemInitError,
};
use crate::world::systems::SystemCommitContext;
use crate::{ComponentValue, EntityId, ErrorReason};

/// Owner of focus, pointer interaction, overlay modes, native text state,
/// local actions, presentation preferences and skin motion preparation of
/// ordinary GUI entities. Control values and overlays' open state are
/// component fields; this System writes them for actions, input and overlay
/// modes and keeps the eligibility fields current.
#[derive(Default)]
pub struct GuiSystem {
    pub(super) local: super::local::GuiLocalState,
    pub(super) motion: super::motion::GuiMotionState,
    pub(super) preferences: super::GuiPreferences,
}

impl GuiSystem {
    /// Stable system identity.
    pub const ID: SystemId = SystemId("ipp.gui");

    pub(in crate::world::systems) fn local_presentation_revision(&self) -> u64 {
        self.local.presentation_revision
    }

    pub(in crate::world::systems) fn part_interaction(
        &self,
        target: super::local::GuiEntityTarget,
        interaction: super::local::GuiInteractionFlags,
    ) -> super::local::GuiPartInteraction {
        self.local.part_interaction(target, interaction)
    }

    pub(in crate::world::systems) fn interaction_flags(
        &self,
        target: super::local::GuiEntityTarget,
    ) -> super::local::GuiInteractionFlags {
        self.local.interaction_flags(target)
    }

    pub(in crate::world::systems) fn focused(&self, target: super::local::GuiEntityTarget) -> bool {
        self.local
            .focus_record()
            .is_some_and(|focus| focus.target == target)
    }

    /// The focused part of `target` while it shows the focus ring.
    pub(in crate::world::systems) fn focus_ring(
        &self,
        target: super::local::GuiEntityTarget,
    ) -> Option<u32> {
        self.local.focus_ring(target)
    }

    pub(in crate::world::systems) fn native_text_state(
        &self,
        target: super::local::GuiEntityTarget,
    ) -> Option<&super::local::GuiNativeTextState> {
        self.local.native_text(target)
    }

    pub(in crate::world::systems::gui) fn local_text_revision(&self) -> u64 {
        self.local.text_revision
    }

    pub(in crate::world::systems::gui) fn native_text_entity(&self) -> Option<EntityId> {
        self.local.native_text_entity()
    }

    /// The control focus moved to at this frame's mutation boundary, which
    /// layout scrolls into view.
    pub(in crate::world::systems::gui) fn reveal_target(&self) -> Option<EntityId> {
        self.local.reveal
    }

    /// `control`'s place in its group, read from its root-first `ancestry`:
    /// its nearest strict ancestor with a `GuiGroup`, unless the control
    /// cannot be an item or lies inside another item. Finding no group costs
    /// one storage slot check per ancestor, so a canvas without groups pays
    /// nothing measurable.
    pub(in crate::world::systems) fn group_item(
        &self,
        world: &crate::world::WorldSimulationState,
        control: super::local::control::GuiControl,
        ancestry: &[EntityId],
    ) -> Option<super::presentation::GuiGroupItem> {
        if !super::local::group::item_kind(control.kind) {
            return None;
        }
        let ancestors = ancestry.split_last()?.1;
        let (index, value) = ancestors
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, ancestor)| {
                world
                    .components
                    .gui_group(ancestor.index() as usize)
                    .map(|value| (index, value))
            })?;
        if ancestors[index + 1..].iter().any(|&between| {
            super::local::control::entity_control(world, &world.state, between)
                .is_some_and(|between| super::local::group::item_kind(between.kind))
        }) {
            return None;
        }
        let group = ancestors[index];
        Some(super::presentation::GuiGroupItem {
            group,
            axis: value.axis,
            selection: value.selection,
            selected: control.kind == super::local::GuiControlKind::Button
                && world
                    .components
                    .gui_button(control.target.entity.index() as usize)
                    .is_some_and(|button| button.selected),
            active: self.local.is_active(control.target),
        })
    }

    pub(in crate::world::systems) fn motion_changes(&self) -> &[super::motion::GuiMotionOwner] {
        self.motion.changes()
    }

    pub(in crate::world) fn motion_work(&self) -> super::motion::GuiMotionPreparationWork {
        self.motion.statistics
    }
}

impl crate::WorldContext<'_> {
    /// `GuiFocus` System query: the World's logical focus, paged by target
    /// entity like the other inspection collections.
    pub fn gui_focus_page(
        &self,
        after: u64,
        target: u64,
        limit: usize,
    ) -> Vec<super::local::GuiFocusRecord> {
        self.system::<GuiSystem>(GuiSystem::ID)
            .and_then(|gui| gui.local.focus_record())
            .filter(|record| {
                let entity = record.target.entity.to_bits();
                if target != 0 {
                    entity == target
                } else {
                    entity > after
                }
            })
            .into_iter()
            .take(limit)
            .collect()
    }

    /// The World's logical focus and its focus part, whether a `GuiAction`
    /// command or the runtime set it, so that no input session owns it, and
    /// whether it shows the focus ring; the input router adopts such focus as
    /// the keyboard target of the context presenting this World, in its
    /// modality.
    pub(crate) fn gui_logical_focus(
        &self,
    ) -> Option<(super::local::GuiEntityTarget, u32, bool, bool)> {
        self.system::<GuiSystem>(GuiSystem::ID)
            .and_then(|gui| gui.local.focus_owner())
    }

    /// `GuiActiveItems` System query: each group's active item, ordered and
    /// paged by group entity.
    pub fn gui_active_item_page(
        &self,
        after: u64,
        group: u64,
        limit: usize,
    ) -> Vec<super::local::GuiActiveItemRecord> {
        self.system::<GuiSystem>(GuiSystem::ID)
            .map(|gui| gui.local.active_records())
            .unwrap_or_default()
            .into_iter()
            .filter(|record| {
                let entity = record.group.to_bits();
                if group != 0 {
                    entity == group
                } else {
                    entity > after
                }
            })
            .take(limit)
            .collect()
    }

    /// `GuiPointers` System query: live pointer feedback records ordered by
    /// target entity, then pointer, paged by target entity. The page holds at
    /// least `limit` records when that many exist, completed with every
    /// remaining record of its last entity, so an entity cursor skips none.
    pub fn gui_pointer_page(
        &self,
        after: u64,
        target: u64,
        limit: usize,
    ) -> Vec<super::local::GuiPointerRecord> {
        let mut page: Vec<super::local::GuiPointerRecord> = Vec::new();
        let records = self
            .system::<GuiSystem>(GuiSystem::ID)
            .map(|gui| gui.local.pointer_records())
            .unwrap_or_default();
        for record in records {
            let entity = record.target.entity.to_bits();
            let selected = if target != 0 {
                entity == target
            } else {
                entity > after
            };
            if !selected {
                continue;
            }
            if page.len() >= limit
                && page
                    .last()
                    .is_none_or(|last| last.target.entity != record.target.entity)
            {
                break;
            }
            page.push(record);
        }
        page
    }
}

/// Factory for GuiSystem.
#[derive(Default)]
pub struct GuiSystemFactory;

impl SystemFactory for GuiSystemFactory {
    fn id(&self) -> SystemId {
        GuiSystem::ID
    }

    fn capabilities(&self) -> crate::systems::SystemCapabilities {
        let mut capabilities = crate::systems::SystemCapabilities::new(
            [
                ComponentValue::GUI_BEHAVIOR,
                ComponentValue::GUI_VIRTUAL_ITEM,
                ComponentValue::GUI_GROUP,
            ],
            [crate::systems::WorldOperation::Gui],
        );

        // Every control requires CanvasBounds, which the Canvas System that
        // paints it admits and writes.
        capabilities.components.extend(
            [
                ComponentValue::GUI_BUTTON,
                ComponentValue::GUI_CHECKBOX,
                ComponentValue::GUI_SLIDER,
                ComponentValue::GUI_TEXT_INPUT,
                ComponentValue::GUI_SCROLL_VIEW,
                ComponentValue::GUI_VIRTUAL_LIST,
                ComponentValue::GUI_COLOR,
            ]
            .map(|component| {
                crate::systems::SystemCapability::requiring(
                    component,
                    [crate::systems::canvas::CanvasSystem::ID],
                )
            }),
        );
        capabilities.components.extend(
            [
                ComponentValue::GUI_THEME,
                ComponentValue::GUI_SKIN,
                ComponentValue::GUI_FONT,
            ]
            .map(|component| {
                crate::systems::SystemCapability::requiring(
                    component,
                    [crate::systems::asset_dependencies::AssetDependencySystem::ID],
                )
            }),
        );
        // Motion rows time transitions that only AnimationSystem samples.
        capabilities
            .components
            .push(crate::systems::SystemCapability::requiring(
                ComponentValue::GUI_THEME_MOTION,
                [crate::systems::animation::AnimationSystem::ID],
            ));
        capabilities
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[SystemDependency::After(
            crate::systems::surface::SurfaceSystem::ID,
        )]
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        // Without AnimationSystem, the sole sampler, every part snaps.
        let animated = context
            .world
            .manifest()
            .systems()
            .contains(&crate::systems::animation::AnimationSystem::ID);
        Ok(Box::new(GuiSystem {
            motion: super::motion::GuiMotionState::new(animated),
            ..GuiSystem::default()
        }))
    }
}

impl System for GuiSystem {
    fn prepare_evaluation(&mut self, context: &mut crate::systems::SystemUpdateContext<'_, '_>) {
        self.local.initialize(context.world.world);
        self.local.refresh_pending_eligibility(context.world.world);

        // Overlays move focus in and back before focus inside a closed one is
        // revalidated away; a held step part repeats on the same Host clock.
        self.evaluate_overlays(context);
        self.advance_number_repeat(context);
        let focus = self.local.motion_focus();
        self.local
            .revalidate_focus(context.world.world, &context.world.world.state);
        if focus != self.local.motion_focus()
            && let Some(entity) = focus
        {
            self.motion.dirty_entity(entity);
        }

        // Numeric edits that ended without a commit since the last frame,
        // published with this frame's tick.
        let tick = context
            .world
            .world
            .tick
            .checked_add(1)
            .expect("World tick exhausted");
        self.local
            .publish_number_discards(context.world.world, tick);
        let revision = self.local.presentation_revision;
        let active = self.local.active_entities();
        self.local
            .revalidate_interactions(context.world.world, &context.world.world.state);
        self.local
            .revalidate_active(context.world.world, &context.world.world.state);
        if revision != self.local.presentation_revision {
            self.motion.dirty_watched();
            for entity in active {
                self.motion.dirty_entity(entity);
            }
        }
        self.motion
            .dirty_entities(std::mem::take(&mut self.local.eligibility_changed));
        self.motion.prepare(
            &self.local,
            self.preferences.reduced_motion,
            &mut context.world,
        );
    }

    fn before_numeric_update(&mut self, context: &mut crate::systems::SystemNumericContext<'_>) {
        for &(entity, component) in context.changed_components() {
            // Sampled transition channels are not a policy change.
            if component == ComponentValue::GUI_BEHAVIOR
                && context
                    .world_data
                    .components
                    .gui_behavior(entity.index() as usize)
                    .is_some_and(|behavior| behavior.motion.notifying_sample)
            {
                continue;
            }
            if component == ComponentValue::GUI_BEHAVIOR {
                self.local.policy_changed(entity);
            }
            self.motion.component_changed(entity, component);
        }
    }

    fn update(&mut self, context: &mut crate::systems::SystemUpdateContext<'_, '_>) {
        self.local.observations.prune();
        self.local.initialize(context.world.world);
        self.local
            .revalidate_focus(context.world.world, &context.world.world.state);
        self.local
            .revalidate_interactions(context.world.world, &context.world.world.state);
        self.local
            .revalidate_active(context.world.world, &context.world.world.state);
    }

    fn finish_update(
        &mut self,
        context: &mut crate::systems::SystemUpdateContext<'_, '_>,
        _report: &mut crate::WorldUpdateReport,
    ) {
        // Animated policy is sampled after this System's update.
        self.local.refresh_pending_eligibility(context.world.world);

        // Numeric edits that ended without a commit during evaluation, with
        // the tick of the frame just evaluated.
        let tick = context.world.world.tick;
        self.local
            .publish_number_discards(context.world.world, tick);

        // Layout revealed the newly focused control in this frame's pass.
        self.local.reveal = None;
    }

    fn apply_operation(
        &mut self,
        context: &mut crate::systems::SystemOperationContext<'_>,
    ) -> Option<Result<(), ErrorReason>> {
        let crate::Command::GuiAction {
            target,
            action,
        } = context.command
        else {
            return None;
        };
        Some(self.apply_action(context, target, action))
    }

    fn after_operation(
        &mut self,
        context: &mut crate::systems::SystemOperationContext<'_>,
    ) -> Result<(), ErrorReason> {
        super::local::group::keep_exclusive_selection(context)
    }

    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        self.motion.before_commit(context);
        self.local.before_commit(context);
    }

    fn command_world_references(
        &self,
        command: &dyn std::any::Any,
        visit: &mut dyn FnMut(crate::WorldRef),
    ) {
        if let Some(command) = command.downcast_ref::<super::local::GuiLocalCommand>() {
            command.world_references(visit);
        }
    }

    fn command_ready(&self, command: &dyn std::any::Any) -> bool {
        command
            .downcast_ref::<super::local::GuiLocalCommand>()
            .is_none_or(super::local::GuiLocalCommand::ready)
    }

    fn command(
        &mut self,
        context: &mut crate::systems::SystemCommandContext<'_>,
        _session: u64,
        command: &dyn std::any::Any,
    ) -> Result<(), ErrorReason> {
        if let Some(command) = command.downcast_ref::<super::observations::GuiObservationCommand>()
        {
            self.local.observations.command(
                crate::WorldRef {
                    id: context.world.world.id,
                    incarnation: context.world.world.identity,
                },
                command,
            );
            return Ok(());
        }
        if let Some(command) = command.downcast_ref::<super::local::GuiLocalCommand>() {
            return self.local_command(context, command);
        }
        if let Some(command) = command.downcast_ref::<super::local::GuiOverlayCommand>() {
            return self.close_overlay(context, command);
        }
        if let Some(update) = command.downcast_ref::<super::GuiPreferencesUpdate>() {
            let preferences = update.applied_to(self.preferences);
            if preferences != self.preferences {
                crate::diagnostic!(
                    Debug,
                    "[IPP core] gui_preferences.update preferences={preferences:?}"
                );
                self.preferences = preferences;
            }
            return Ok(());
        }
        Err(ErrorReason::InvalidValue)
    }

    fn save_persistent_state(
        &self,
        context: &mut crate::systems::SystemSaveContext<'_>,
    ) -> Result<Option<crate::systems::SystemPersistentState>, String> {
        let Some(state) = self.preferences.encode_persistent() else {
            return Ok(None);
        };
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
        self.preferences = match state {
            Some(bytes) => super::GuiPreferences::decode_persistent(bytes)?,
            None => super::GuiPreferences::default(),
        };
        Ok(())
    }

    fn after_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        let revision = self.local.presentation_revision;
        self.local.after_commit(context);
        if revision != self.local.presentation_revision {
            self.motion.dirty_watched();
        }
    }
}

#[cfg(test)]
#[path = "motion_lifetime_tests.rs"]
mod motion_lifetime_tests;
