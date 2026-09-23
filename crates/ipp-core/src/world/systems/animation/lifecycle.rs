//! Synchronous animation cleanup before component or source storage is released.

use super::*;
use crate::{ComponentValue, systems::SystemCommitContext};
use std::collections::BTreeMap;

impl AnimationSystem {
    pub(super) fn suspend_asset(
        &mut self,
        world: &crate::world::WorldSimulationState,
        assets: &crate::services::asset_management::AssetManagementService,
        key: crate::services::asset_management::AssetKey,
    ) {
        let mut controllers = std::mem::take(&mut self.state.controllers);
        let read = AnimationReadAccess {
            animation: &self.state,
            world,
            state: &world.state,
            asset_acquisition: assets,
        };
        for controller in controllers.values_mut() {
            let destination_uses = controller
                .drivers
                .iter()
                .any(|driver| read.binding_uses_asset(driver.as_ref(), key));
            let source_uses = controller.transition_source().is_some_and(|source| {
                source
                    .drivers
                    .iter()
                    .any(|driver| read.binding_uses_asset(driver.as_ref(), key))
            });
            if destination_uses || source_uses {
                if let Some(summary) = &mut controller.snapshot.transition {
                    summary.pending = true;
                }
                if destination_uses {
                    controller.ready = false;
                }
                for driver in &mut controller.drivers {
                    if driver.clip() == key {
                        driver.suspend_track();
                    }
                }
                if let Some(source) = controller.transition_source_mut() {
                    source.ready = false;
                    for driver in &mut source.drivers {
                        if driver.clip() == key {
                            driver.suspend_track();
                        }
                    }
                }
            }
        }
        self.state.controllers = controllers;
    }

    pub(in crate::world) fn invalidate_asset(
        &mut self,
        world: &mut crate::world::WorldSimulationState,
        assets: &crate::services::asset_management::AssetManagementService,
        key: crate::services::asset_management::AssetKey,
    ) -> Vec<(EntityId, ComponentValue)> {
        let invalid: Vec<_> = {
            let read = AnimationReadAccess {
                animation: &self.state,
                world,
                state: &world.state,
                asset_acquisition: assets,
            };
            self.state
                .controllers
                .iter()
                .filter_map(|(&id, controller)| {
                    (controller
                        .drivers
                        .iter()
                        .any(|driver| read.binding_uses_asset(driver.as_ref(), key))
                        || controller.transition_source().is_some_and(|source| {
                            source
                                .drivers
                                .iter()
                                .any(|driver| read.binding_uses_asset(driver.as_ref(), key))
                        }))
                    .then_some(id)
                })
                .collect()
        };
        let mut restorations = BTreeMap::new();
        for id in invalid {
            let mut controller = self.state.controllers.remove(&id).unwrap();
            if let Some(program) = controller
                .transition
                .as_deref()
                .and_then(|transition| transition.program.as_ref())
            {
                let _ = program.restore(&mut world.components, &world.state);
            }
            for driver in controller.restoration_drivers() {
                let identity = driver.identity();
                let key = (identity.entity, identity.property.component());
                if !world
                    .state
                    .entities
                    .get(&identity.entity)
                    .and_then(|record| record.input(key.1))
                    .is_some_and(|input| input.incarnation == identity.incarnation)
                {
                    continue;
                }
                #[cfg(feature = "skeletal-animation")]
                if matches!(
                    driver.runtime_target(),
                    super::driver::AnimationRuntimeTarget::JointLocal { .. }
                ) {
                    let _ = driver.runtime_target().write_joints(
                        &mut world.components,
                        identity.entity,
                        driver.original(),
                    );
                    continue;
                }
                if let std::collections::btree_map::Entry::Vacant(entry) = restorations.entry(key)
                    && let Some(value) = world.components.get(key.1, key.0.index() as usize)
                {
                    entry.insert(value);
                }
                if let Some(value) = restorations.get_mut(&key) {
                    let _ = driver.restore(value);
                }
            }
            controller.clear_drivers();
            controller.transition = None;
            controller.snapshot.transition = None;
            controller.snapshot.state = AnimationPlaybackStatus::Stopped;
            self.state.playback_events.push(AnimationPlaybackEvent {
                controller: AnimationControllerState {
                    id,
                    state: controller.snapshot.state,
                    time: controller.snapshot.time,
                },
                kind: AnimationPlaybackEventKind::Invalidated,
                reason: Some(ErrorReason::InvalidAsset),
            });
            self.state.controllers.insert(id, controller);
        }
        restorations
            .into_iter()
            .map(|((entity, _), value)| (entity, value))
            .collect()
    }

    pub(in crate::world) fn refresh_after_commit(&mut self, context: &SystemCommitContext<'_>) {
        if context.is_evaluated() {
            return;
        }
        let affected = self.state.affected_by(context.staged);
        let mut controllers = std::mem::take(&mut self.state.controllers);
        let read = AnimationReadAccess {
            animation: &self.state,
            world: context.world_data,
            state: context.staged,
            asset_acquisition: context.assets,
        };
        for id in &affected {
            let controller = controllers.get_mut(id).unwrap();
            for driver in &mut controller.drivers {
                let identity = driver.identity();
                let key = (identity.entity, identity.property.component());
                if !context.staged.changed.contains_key(&key) {
                    continue;
                }
                if let Some(value) =
                    context
                        .staged
                        .input_value(&context.world_data.components, key.0, key.1)
                    && let Ok(original) = read.read_bound_animation_target(driver.as_ref(), &value)
                {
                    let _ = driver.refresh_original(original);
                }
            }
            if let Some(source) = controller.transition_source_mut() {
                for driver in &mut source.drivers {
                    let identity = driver.identity();
                    let key = (identity.entity, identity.property.component());
                    if !context.staged.changed.contains_key(&key) {
                        continue;
                    }
                    if let Some(value) =
                        context
                            .staged
                            .producer_value(&context.world_data.components, key.0, key.1)
                        && let Ok(original) =
                            read.read_bound_animation_target(driver.as_ref(), &value)
                    {
                        let _ = driver.refresh_original(original);
                    }
                }
            }
            if let Some(mut transition) = controller.transition.take() {
                match &mut transition.source {
                    super::system_state::AnimationTransitionSource::Live(source) => {
                        if let Some(program) = transition.program.as_mut() {
                            program.refresh_baselines(Some(source.as_ref()), controller);
                        }
                        if let Some(program) = transition.hold_program.as_mut() {
                            program.refresh_baselines(Some(source.as_ref()), controller);
                        }
                    }
                    super::system_state::AnimationTransitionSource::Frozen {
                        values,
                        ..
                    } => {
                        for value in values.iter_mut() {
                            let key = (value.target, value.property.component());
                            if !context.staged.changed.contains_key(&key) {
                                continue;
                            }
                            if let Some(component) = context.staged.producer_value(
                                &context.world_data.components,
                                key.0,
                                key.1,
                            ) && let Ok(baseline) =
                                read.read_animation_target(&value.property, &component)
                            {
                                value.baseline = baseline;
                            }
                        }
                        if let Some(program) = transition.program.as_mut() {
                            program.refresh_baselines(None, controller);
                            program.refresh_frozen_baselines(values);
                        }
                        if let Some(program) = transition.hold_program.as_mut() {
                            program.refresh_frozen_baselines(values);
                        }
                    }
                }
                controller.transition = Some(transition);
            }
        }
        self.state.controllers = controllers;
        self.state.affected_controllers = affected;
    }

    pub(in crate::world) fn invalidate_changes(&mut self, context: &mut SystemCommitContext<'_>) {
        #[cfg(feature = "profiling")]
        let _allocation_scope = crate::profiling::AllocationScope::new(199, "animation.invalidate");
        #[cfg(feature = "profiling")]
        let _measurement =
            crate::profiling::Stage::fixed(crate::profiling::FixedStage::AnimationInvalidate);

        // Every real mutation invalidates held discrete output, including writes
        // from another System. The sampling controller acknowledges its own
        // successful publication only after all lifecycle callbacks complete.
        for key in context.staged.changed.keys() {
            if let Some(ids) = self.state.target_controllers.get(key) {
                for id in ids {
                    if let Some(controller) = self.state.controllers.get(id) {
                        for driver in controller.drivers_for(*key) {
                            driver.reset_discrete();
                        }
                    }
                }
            }
        }
        self.state.pending_restorations.retain(|identity, _| {
            let key = (identity.entity, identity.property.component());
            (context.is_evaluated() || !context.staged.changed.contains_key(&key))
                && context
                    .staged
                    .entities
                    .get(&identity.entity)
                    .and_then(|record| record.input(key.1))
                    .is_some_and(|input| input.incarnation == identity.incarnation)
        });
        // Numeric playback time edits preserve readiness. Source replacement
        // must prepare the new particle cache even when the component survives.
        #[cfg(feature = "particles")]
        if crate::compiled_animation_enabled() {
            for (&key, value) in context.staged.prepared.iter() {
                if let ComponentValue::ParticlePlayback(next) = value
                    && context
                        .world_data
                        .components
                        .particle_playback(key.0.index() as usize)
                        .is_some_and(|old| old.source != next.source || old.variant != next.variant)
                    && let Some(ids) = self.state.target_controllers.get(&key)
                {
                    for id in ids {
                        self.state.controllers.get_mut(id).unwrap().ready = false;
                    }
                }
            }
        }
        let read = AnimationReadAccess {
            animation: &self.state,
            world: context.world_data,
            state: context.staged,
            asset_acquisition: context.assets,
        };
        if context
            .staged
            .changed
            .keys()
            .all(|key| read.unchanged_static_target(*key, context.staged))
        {
            return;
        }
        let affected = self.state.affected_by(context.staged);
        #[cfg(feature = "skeletal-animation")]
        if crate::compiled_animation_enabled()
            && context
                .staged
                .changed
                .keys()
                .any(|key| key.1 == ComponentValue::SKELETON)
        {
            // Changing pose inputs can suspend internal pose preparation even
            // when the skeleton identity and its joint ordinals stay valid.
            for id in &affected {
                self.state.controllers.get_mut(id).unwrap().ready = false;
            }
        }
        let mut controllers = std::mem::take(&mut self.state.controllers);
        let read = AnimationReadAccess {
            animation: &self.state,
            world: context.world_data,
            state: context.staged,
            asset_acquisition: context.assets,
        };
        let mut events = Vec::new();
        for &id in &affected {
            let controller = controllers.get_mut(&id).unwrap();
            let removed: Vec<_> = controller
                .changed_drivers(context.staged)
                .filter(|driver| {
                    (!crate::allocation_optimizations_enabled()
                        || context.staged.changed.contains_key(&(
                            driver.identity().entity,
                            driver.identity().property.component(),
                        )))
                        && matches!(
                            driver.description().property,
                            AnimationTrackTarget::DynamicProperty { .. }
                        )
                        && !read.animation_binding_alive(*driver, context.staged)
                })
                .map(|driver| driver.description().clone())
                .collect();
            if removed.is_empty() {
                continue;
            }
            controller
                .drivers
                .retain(|driver| !removed.contains(driver.description()));
            controller.reindex_drivers();
            if controller.drivers.is_empty() {
                controller.snapshot.state = AnimationPlaybackStatus::Stopped;
            }
            events.push(AnimationPlaybackEvent {
                controller: AnimationControllerState {
                    id,
                    state: controller.snapshot.state,
                    time: controller.snapshot.time,
                },
                kind: AnimationPlaybackEventKind::Invalidated,
                reason: Some(ErrorReason::MissingComponent),
            });
            let mut index = 0;
            controller.incarnations.retain(|_| {
                let keep = !removed.contains(&controller.snapshot.description.drivers[index]);
                index += 1;
                keep
            });
            controller
                .snapshot
                .description
                .drivers
                .retain(|description| !removed.contains(description));
        }
        if !events.is_empty() {
            self.state.description_demand_clean = false;
        }
        self.state.playback_events.extend(events);
        self.state.controllers = controllers;
        let invalid: Vec<_> = {
            let read = AnimationReadAccess {
                animation: &self.state,
                world: context.world_data,
                state: context.staged,
                asset_acquisition: context.assets,
            };
            affected
                .iter()
                .filter_map(|&id| {
                    let controller = &self.state.controllers[&id];
                    (controller.changed_drivers(context.staged).any(|driver| {
                        (!crate::allocation_optimizations_enabled()
                            || context.staged.changed.contains_key(&(
                                driver.identity().entity,
                                driver.identity().property.component(),
                            )))
                            && !read.animation_binding_alive(driver, context.staged)
                    }) || controller.transition.as_deref().is_some_and(|transition| {
                        transition
                            .program
                            .as_ref()
                            .or(transition.hold_program.as_ref())
                            .is_some_and(|program| {
                                program
                                    .invalidated_by(context.staged, &context.world_data.components)
                            })
                    }))
                    .then_some(id)
                })
                .collect()
        };
        let mut restorations = BTreeMap::new();
        for id in invalid {
            let mut controller = self.state.controllers.remove(&id).unwrap();
            if let Some(program) = controller.transition.as_deref().and_then(|transition| {
                transition
                    .program
                    .as_ref()
                    .or(transition.hold_program.as_ref())
            }) {
                let _ = program.restore(&mut context.world_data.components, context.staged);
            }
            for driver in controller.restoration_drivers() {
                let identity = driver.identity();
                let key = (identity.entity, identity.property.component());
                // An explicit pending value owns this component's resulting inputs.
                if context.staged.changed.contains_key(&key)
                    || !context
                        .staged
                        .entities
                        .get(&identity.entity)
                        .and_then(|record| record.input(key.1))
                        .is_some_and(|input| input.incarnation == identity.incarnation)
                {
                    continue;
                }
                #[cfg(feature = "skeletal-animation")]
                if matches!(
                    driver.runtime_target(),
                    super::driver::AnimationRuntimeTarget::JointLocal { .. }
                ) {
                    let _ = driver.runtime_target().write_joints(
                        &mut context.world_data.components,
                        identity.entity,
                        driver.original(),
                    );
                    continue;
                }
                if context.is_evaluated() {
                    self.state
                        .pending_restorations
                        .entry(identity.clone())
                        .or_insert_with(|| driver.original());
                    continue;
                }
                if let std::collections::btree_map::Entry::Vacant(entry) = restorations.entry(key)
                    && let Some(value) = context
                        .world_data
                        .components
                        .get(key.1, key.0.index() as usize)
                {
                    entry.insert(value);
                }
                if let Some(value) = restorations.get_mut(&key) {
                    let _ = driver.restore(value);
                }
            }
            controller.clear_drivers();
            controller.transition = None;
            controller.snapshot.transition = None;
            controller.snapshot.state = AnimationPlaybackStatus::Stopped;
            self.state.playback_events.push(AnimationPlaybackEvent {
                controller: AnimationControllerState {
                    id,
                    state: controller.snapshot.state,
                    time: controller.snapshot.time,
                },
                kind: AnimationPlaybackEventKind::Invalidated,
                reason: Some(ErrorReason::MissingComponent),
            });
            self.state.controllers.insert(id, controller);
        }
        self.state.affected_controllers = affected;
        for ((entity, _), value) in restorations {
            context.restore_evaluated_component(entity, value);
        }
    }
}
