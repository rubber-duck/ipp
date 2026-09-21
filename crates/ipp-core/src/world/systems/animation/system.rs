//! AnimationSystem: factory configuration and exclusively owned per-world state.

use super::AnimationSystemState;
use crate::systems::{
    System, SystemAssetContext, SystemCommandContext, SystemCommitContext, SystemDependency,
    SystemFactory, SystemId, SystemInitContext, SystemInitError, SystemTeardownContext,
    SystemUpdateContext,
};

/// Fresh runtime state owned by one World.
#[derive(Default)]
pub struct AnimationSystem {
    pub(in crate::world) state: AnimationSystemState,
    #[cfg(feature = "gui")]
    skin_controllers:
        std::collections::BTreeMap<super::GuiSkinAnimationOwner, GuiSkinAnimationController>,
}

#[cfg(feature = "gui")]
#[derive(Clone, Copy, Debug)]
struct GuiSkinAnimationController {
    id: Option<super::AnimationControllerId>,
    request: u64,
    rejection: Option<crate::ErrorReason>,
}

impl AnimationSystem {
    /// Stable factory and instance identity.
    pub const ID: SystemId = SystemId("ipp.animation");

    #[cfg(feature = "gui")]
    pub(in crate::world) fn skin_controller(
        &self,
        owner: super::GuiSkinAnimationOwner,
    ) -> Option<(
        u64,
        &super::AnimationControllerSnapshot,
        Option<crate::ErrorReason>,
    )> {
        let owned = self.skin_controllers.get(&owner)?;
        let controller = self.state.controllers.get(&owned.id?)?;
        let failure = controller.failure.or_else(|| {
            (controller.snapshot.state == super::AnimationPlaybackStatus::Stopped)
                .then_some(crate::ErrorReason::InvalidValue)
        });
        Some((owned.request, &controller.snapshot, failure))
    }

    #[cfg(feature = "gui")]
    pub(in crate::world) fn skin_controller_rejection(
        &self,
        owner: super::GuiSkinAnimationOwner,
    ) -> Option<(u64, crate::ErrorReason)> {
        let owned = self.skin_controllers.get(&owner)?;
        Some((owned.request, owned.rejection?))
    }

    #[cfg(feature = "gui")]
    pub(super) fn is_derived_skin_controller(&self, id: super::AnimationControllerId) -> bool {
        self.skin_controllers
            .values()
            .any(|owned| owned.id == Some(id))
    }

    #[cfg(not(feature = "gui"))]
    pub(super) fn is_derived_skin_controller(&self, _id: super::AnimationControllerId) -> bool {
        false
    }

    /// Reject an ordinary client operation directed at a private derived controller.
    pub(super) fn ensure_ordinary_controller(
        &self,
        id: super::AnimationControllerId,
    ) -> Result<(), crate::ErrorReason> {
        if self.is_derived_skin_controller(id) {
            return Err(crate::ErrorReason::InvalidValue);
        }
        Ok(())
    }

    #[cfg(feature = "gui")]
    pub(super) fn invalidate_skin_controller_associations(&mut self) {
        self.skin_controllers.clear();
    }

    /// Inspect ordinary controllers in deterministic identity order.
    pub(in crate::world) fn ordinary_controllers(&self) -> Vec<super::AnimationControllerSnapshot> {
        self.state
            .controllers
            .iter()
            .filter(|(id, _)| !self.is_derived_skin_controller(**id))
            .map(|(_, controller)| controller.snapshot.clone())
            .collect()
    }

    /// Read a bounded ordinary-controller page in identity order.
    pub(in crate::world) fn ordinary_controller_page(
        &self,
        after: u64,
        target: u64,
        limit: usize,
    ) -> Vec<super::AnimationControllerSnapshot> {
        if target != 0 {
            return self
                .ordinary_controller(super::AnimationControllerId::from_bits(target))
                .into_iter()
                .take(limit)
                .collect();
        }
        self.state
            .controllers
            .range((
                std::ops::Bound::Excluded(super::AnimationControllerId::from_bits(after)),
                std::ops::Bound::Unbounded,
            ))
            .filter(|(id, _)| !self.is_derived_skin_controller(**id))
            .take(limit)
            .map(|(_, controller)| controller.snapshot.clone())
            .collect()
    }

    /// Inspect one ordinary controller's descriptions and shared clock.
    pub(in crate::world) fn ordinary_controller(
        &self,
        id: super::AnimationControllerId,
    ) -> Option<super::AnimationControllerSnapshot> {
        if self.is_derived_skin_controller(id) {
            return None;
        }
        Some(self.state.controllers.get(&id)?.snapshot.clone())
    }
}

/// Reusable factory; it retains no mutable world state.
#[derive(Default)]
pub struct AnimationSystemFactory;

impl SystemFactory for AnimationSystemFactory {
    fn id(&self) -> SystemId {
        AnimationSystem::ID
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[]
    }

    fn capacity_hints(&self) -> crate::WorldSystemCapacityHints {
        crate::WorldSystemCapacityHints::new([("controllers", 128)])
    }

    fn create(
        &self,
        _context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(AnimationSystem::default()))
    }
}

impl System for AnimationSystem {
    fn restore_component_input(
        &self,
        entity: crate::EntityId,
        incarnation: u64,
        value: &mut crate::ComponentValue,
    ) {
        self.state.restore_underlying(entity, incarnation, value);
    }

    fn command(
        &mut self,
        context: &mut SystemCommandContext<'_>,
        _session: u64,
        command: &dyn std::any::Any,
    ) -> Result<(), crate::ErrorReason> {
        let command = command
            .downcast_ref::<super::world_api::AnimationCommand>()
            .ok_or(crate::ErrorReason::InvalidValue)?;
        let mut access = super::AnimationAccess {
            system: self,
            context: &mut context.world,
        };
        let result = match command.clone() {
            super::world_api::AnimationCommand::Controller {
                request_id,
                command,
            } => access.apply_animation_controller_command(request_id, command),
            super::world_api::AnimationCommand::Playback {
                id,
                control,
            } => access.control_ordinary_playback(id, control),
            #[cfg(feature = "gui")]
            super::world_api::AnimationCommand::Internal(commands) => {
                for command in commands {
                    // A stale owner or unavailable source is local to one part;
                    // unrelated derived controllers still reconcile.
                    let _ = apply_skin_animation_command(&mut access, command);
                }
                Ok(())
            }
        };
        #[cfg(feature = "gui")]
        access.system.skin_controllers.retain(|_, owned| {
            owned.rejection.is_some()
                || owned
                    .id
                    .is_some_and(|id| access.system.state.controllers.contains_key(&id))
        });
        result
    }

    fn prepare_mutation(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        super::AnimationAccess {
            system: self,
            context: &mut context.world,
        }
        .restore_animation_inputs(true);
    }

    fn before_asset_release(
        &mut self,
        context: &mut SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        use crate::services::asset_management::AssetLifecycleKind;
        if crate::compiled_animation_enabled() && event.kind == AssetLifecycleKind::StatusChanged {
            self.suspend_asset(
                context.world.world,
                context.world.asset_acquisition,
                event.key,
            );
            return;
        }
        if event.kind != AssetLifecycleKind::Removed {
            return;
        }
        let restorations = self.invalidate_asset(
            context.world.world,
            context.world.asset_acquisition,
            event.key,
        );
        for (entity, value) in restorations {
            context.restore_evaluated_component(entity, value);
        }
    }

    fn validate_commit(&self, context: &SystemCommitContext<'_>) -> Result<(), crate::ErrorReason> {
        super::AnimationReadAccess {
            animation: &self.state,
            world: context.world_data,
            state: context.staged,
            asset_acquisition: context.assets,
        }
        .validate_animation_changes(context.staged)?;
        let _ = context;
        Ok(())
    }

    fn finish_update(
        &mut self,
        _context: &mut SystemUpdateContext<'_, '_>,
        report: &mut crate::WorldUpdateReport,
    ) {
        #[cfg(feature = "gui")]
        {
            let derived: std::collections::BTreeSet<_> = self
                .skin_controllers
                .values()
                .filter_map(|owned| owned.id)
                .collect();
            self.state
                .playback_events
                .retain(|event| !derived.contains(&event.controller.id));
        }
        report
            .playback_events
            .append(&mut self.state.playback_events);
        report
            .animation_controller_outcomes
            .append(&mut self.state.controller_outcomes);
    }

    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        self.invalidate_changes(context);
    }

    fn after_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        self.refresh_after_commit(context);
    }

    fn save_persistent_state(
        &self,
        context: &mut crate::systems::SystemSaveContext<'_>,
    ) -> Result<Option<crate::systems::SystemPersistentState>, String> {
        let state = self.save_persistent_state(context.ids, context.bytes, context.max_bytes)?;
        state.validate_entities(context.entities)?;
        state.encode(context.max_bytes).map(Some)
    }

    fn load_persistent_state(
        &mut self,
        context: &mut crate::systems::SystemLoadContext<'_, '_>,
        state: Option<&crate::systems::SystemPersistentState>,
    ) -> Result<(), String> {
        let persistent = match state {
            Some(bytes) => super::AnimationPersistentState::decode(bytes, context.max_bytes)?
                .remap_entities(context.ids)?,
            None => super::AnimationPersistentState::default(),
        };
        super::AnimationAccess {
            system: self,
            context: &mut context.world,
        }
        .restore_animation_controllers(persistent)
        .map_err(|reason| format!("Invalid persistent animation: {reason:?}"))
    }

    fn reserve_capacity(
        &mut self,
        hints: &crate::WorldSystemCapacityHints,
    ) -> Result<(), crate::ErrorReason> {
        let _ = hints;
        Ok(())
    }

    fn teardown(&mut self, _context: &mut SystemTeardownContext<'_>) {
        self.state.controllers.clear();
        self.state.rebuild_target_index();
        self.state.affected_controllers.clear();
        self.state.pending_restorations.clear();
        self.state.controller_outcomes.clear();
        self.state.playback_events.clear();
        self.state.animation_sources.clear();
        #[cfg(feature = "gui")]
        self.skin_controllers.clear();
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        let dt = context.dt();
        super::AnimationAccess {
            system: self,
            context: &mut context.world,
        }
        .evaluate_animation(dt);
    }
}

#[cfg(feature = "gui")]
fn apply_skin_animation_command(
    access: &mut super::AnimationAccess<'_, '_>,
    command: super::AnimationInternalCommand,
) -> Result<(), crate::ErrorReason> {
    use super::AnimationInternalCommand;

    match command {
        AnimationInternalCommand::EnsureSkinTransition {
            owner,
            request,
            source,
            source_time,
            source_sample,
            transition,
            destination_sample,
        } => {
            if request == 0 || !skin_animation_owner_live(access, owner) {
                delete_skin_animation_controller(access, owner)?;
                return Err(crate::ErrorReason::InvalidValue);
            }
            let current = access.system.skin_controllers.get(&owner).copied();
            if current
                .is_some_and(|current| current.request == request && current.rejection.is_some())
            {
                return Ok(());
            }
            if let Some(current) = current
                && let Some(id) = current.id
                && access.system.state.controllers.contains_key(&id)
            {
                if current.request == request {
                    return Ok(());
                }

                match validate_skin_transition_samples(
                    access,
                    &source,
                    source_time,
                    source_sample,
                    &transition.description,
                    skin_transition_destination_time(&transition),
                    destination_sample,
                ) {
                    Ok(true) => {}
                    Ok(false) => return Err(crate::ErrorReason::MissingAsset),
                    Err(reason) => {
                        reject_skin_animation_controller(access, owner, request, reason)?;
                        return Err(reason);
                    }
                }
                if let Err(reason) = access.transition_animation_controller(id, transition) {
                    if skin_animation_failure_retryable(reason) {
                        return Err(reason);
                    }
                    reject_skin_animation_controller(access, owner, request, reason)?;
                    return Err(reason);
                }
                access.system.skin_controllers.insert(
                    owner,
                    GuiSkinAnimationController {
                        id: Some(id),
                        request,
                        rejection: None,
                    },
                );
                return Ok(());
            }
            access.system.skin_controllers.remove(&owner);

            match validate_skin_transition_samples(
                access,
                &source,
                source_time,
                source_sample,
                &transition.description,
                skin_transition_destination_time(&transition),
                destination_sample,
            ) {
                Ok(true) => {}
                Ok(false) => return Err(crate::ErrorReason::MissingAsset),
                Err(reason) => {
                    reject_skin_animation_controller(access, owner, request, reason)?;
                    return Err(reason);
                }
            }
            let id = match access.create_animation_controller(source) {
                Ok(id) => id,
                Err(reason) => {
                    if !skin_animation_failure_retryable(reason) {
                        reject_skin_animation_controller(access, owner, request, reason)?;
                    }
                    return Err(reason);
                }
            };
            let result = access
                .control_playback(id, super::AnimationPlaybackControl::Seek(source_time))
                .and_then(|()| access.control_playback(id, super::AnimationPlaybackControl::Play))
                .and_then(|()| access.transition_animation_controller(id, transition));
            if let Err(reason) = result {
                let _ = access.remove_animation_controller(id);
                if !skin_animation_failure_retryable(reason) {
                    reject_skin_animation_controller(access, owner, request, reason)?;
                }
                return Err(reason);
            }
            access.system.skin_controllers.insert(
                owner,
                GuiSkinAnimationController {
                    id: Some(id),
                    request,
                    rejection: None,
                },
            );
            Ok(())
        }
        AnimationInternalCommand::DeleteSkin(owner) => {
            delete_skin_animation_controller(access, owner)
        }
    }
}

#[cfg(feature = "gui")]
fn delete_skin_animation_controller(
    access: &mut super::AnimationAccess<'_, '_>,
    owner: super::GuiSkinAnimationOwner,
) -> Result<(), crate::ErrorReason> {
    let Some(owned) = access.system.skin_controllers.remove(&owner) else {
        return Ok(());
    };
    if let Some(id) = owned.id
        && access.system.state.controllers.contains_key(&id)
    {
        access.remove_animation_controller(id)?;
    }
    Ok(())
}

#[cfg(feature = "gui")]
fn reject_skin_animation_controller(
    access: &mut super::AnimationAccess<'_, '_>,
    owner: super::GuiSkinAnimationOwner,
    request: u64,
    reason: crate::ErrorReason,
) -> Result<(), crate::ErrorReason> {
    if let Some(id) = access
        .system
        .skin_controllers
        .get(&owner)
        .and_then(|owned| owned.id)
        && access.system.state.controllers.contains_key(&id)
    {
        access.remove_animation_controller(id)?;
    }
    access.system.skin_controllers.insert(
        owner,
        GuiSkinAnimationController {
            id: None,
            request,
            rejection: Some(reason),
        },
    );
    Ok(())
}

#[cfg(feature = "gui")]
fn skin_animation_failure_retryable(reason: crate::ErrorReason) -> bool {
    matches!(
        reason,
        crate::ErrorReason::Capacity | crate::ErrorReason::MissingAsset
    )
}

#[cfg(feature = "gui")]
fn skin_transition_destination_time(transition: &super::AnimationControllerTransition) -> f64 {
    match transition.start_time {
        super::AnimationTransitionStartTime::Seek(time) => time,
        super::AnimationTransitionStartTime::Restart
        | super::AnimationTransitionStartTime::Preserve
        | super::AnimationTransitionStartTime::MatchPhase => 0.0,
    }
}

#[cfg(feature = "gui")]
fn validate_skin_transition_samples(
    access: &super::AnimationAccess<'_, '_>,
    source: &super::AnimationControllerDescription,
    source_time: f64,
    source_sample: super::GuiSkinAnimationSample,
    destination: &super::AnimationControllerDescription,
    destination_time: f64,
    destination_sample: super::GuiSkinAnimationSample,
) -> Result<bool, crate::ErrorReason> {
    Ok(
        validate_skin_sample(access, source, source_time, source_sample)?
            && validate_skin_sample(access, destination, destination_time, destination_sample)?,
    )
}

#[cfg(feature = "gui")]
fn validate_skin_sample(
    access: &super::AnimationAccess<'_, '_>,
    description: &super::AnimationControllerDescription,
    time: f64,
    expected: super::GuiSkinAnimationSample,
) -> Result<bool, crate::ErrorReason> {
    use crate::DynamicValue;
    use crate::components::schema::FieldValue;

    if description.drivers.len() != 3 || !time.is_finite() || time < 0.0 {
        return Err(crate::ErrorReason::InvalidValue);
    }
    let expected = [
        DynamicValue::Vec4(expected.color),
        DynamicValue::F32(expected.opacity),
        DynamicValue::Vec2(expected.scale),
    ];
    for (driver, expected) in description.drivers.iter().zip(expected) {
        let read = access.read();
        let Some(key) = read.source_key(driver) else {
            return Ok(false);
        };
        if !read.clip_ready(key)? {
            return Ok(false);
        }
        let clip = read
            .clip_by_key(key)
            .ok_or(crate::ErrorReason::InvalidAsset)?;
        if time > clip.duration() {
            return Err(crate::ErrorReason::InvalidValue);
        }
        let track = driver.track as usize;
        if clip.tracks().get(track).is_none() {
            return Err(crate::ErrorReason::InvalidField);
        }
        let sampled = clip.sample(track, time);
        let super::AnimationValue::Field(FieldValue::Dynamic(sampled)) = sampled else {
            return Err(crate::ErrorReason::InvalidField);
        };
        if !skin_samples_match(&sampled, &expected) {
            return Err(crate::ErrorReason::InvalidValue);
        }
    }
    Ok(true)
}

#[cfg(feature = "gui")]
fn skin_samples_match(actual: &crate::DynamicValue, expected: &crate::DynamicValue) -> bool {
    const EPSILON: f32 = 1.0e-5;

    let lanes = |actual: &[f32], expected: &[f32]| {
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| (*actual - *expected).abs() <= EPSILON)
    };
    match (actual, expected) {
        (crate::DynamicValue::F32(actual), crate::DynamicValue::F32(expected)) => {
            (*actual - *expected).abs() <= EPSILON
        }
        (crate::DynamicValue::Vec2(actual), crate::DynamicValue::Vec2(expected)) => {
            lanes(actual, expected)
        }
        (crate::DynamicValue::Vec4(actual), crate::DynamicValue::Vec4(expected)) => {
            lanes(actual, expected)
        }
        _ => false,
    }
}

#[cfg(feature = "gui")]
fn skin_animation_owner_live(
    access: &super::AnimationAccess<'_, '_>,
    owner: super::GuiSkinAnimationOwner,
) -> bool {
    let Some(record) = access.context.world.state.entities.get(&owner.entity) else {
        return false;
    };
    if record
        .input(crate::ComponentValue::GUI_ROOT)
        .is_none_or(|input| input.incarnation != owner.primitive.root_incarnation)
    {
        return false;
    }
    access
        .context
        .world
        .components
        .gui_root(owner.entity.index() as usize)
        .and_then(|root| root.nodes().node(owner.primitive.node))
        .is_some_and(|node| node.lifetime == owner.primitive.lifetime)
}
