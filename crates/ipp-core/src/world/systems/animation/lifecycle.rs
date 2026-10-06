//! Synchronous animation cleanup before component or source storage is released.

use super::*;
use crate::{ComponentValue, systems::SystemCommitContext};
use std::collections::BTreeMap;

impl AnimationSystem {
    /// Another System overwrites these fields absolutely: contributions to them
    /// are gone, so each controller applies its full total again next frame.
    pub(super) fn forget_overwritten(&mut self, fields: &[(EntityId, u16, u32)]) {
        for &(entity, component, offset) in fields {
            let Some(ids) = self.state.target_controllers.get(&(entity, component)) else {
                continue;
            };
            for id in ids {
                if let Some(controller) = self.state.controllers.get_mut(id) {
                    controller.contributions.forget(entity, component, offset);
                }
            }
        }
    }

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
                .any(|driver| read.binding_uses_asset(driver.as_ref(), key))
                || controller
                    .structural_drivers
                    .iter()
                    .any(|driver| driver.clip() == key);
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
                for driver in &mut controller.structural_drivers {
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
            for (identity, applied) in controller.contributions.take() {
                let key = (identity.entity, identity.property.component_target());
                if applied.is_empty()
                    || !world
                        .state
                        .entities
                        .get(&identity.entity)
                        .and_then(|record| record.input(key.1))
                        .is_some_and(|input| input.incarnation == identity.incarnation)
                {
                    continue;
                }
                if let std::collections::btree_map::Entry::Vacant(entry) = restorations.entry(key)
                    && let Some(value) = world.components.get(key.1, key.0.index() as usize)
                {
                    entry.insert(value);
                }
                if let Some(value) = restorations.get_mut(&key) {
                    let _ = withdraw_from(value, &identity, &applied);
                }
            }
            for driver in std::mem::take(&mut controller.structural_drivers) {
                self.state.resample_structural_target(driver.target());
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

    pub(in crate::world) fn invalidate_changes(&mut self, context: &mut SystemCommitContext<'_>) {
        #[cfg(feature = "instrumentation")]
        let _allocation_scope = crate::profiling::AllocationScope::new(199, "animation.invalidate");
        #[cfg(feature = "instrumentation")]
        let _measurement =
            crate::profiling::Stage::fixed(crate::profiling::FixedStage::AnimationInvalidate);

        // Dynamic metadata can rebuild byte offsets while preserving property
        // keys and the component incarnation. Revoke compiled descriptors before
        // that storage changes, then prepare them against committed storage.
        // Applied contributions and controller clocks remain semantic state.
        for key in context
            .staged
            .changed
            .keys()
            .filter(|key| ComponentValue::supports_dynamic_properties(key.1))
        {
            if let Some(ids) = self.state.target_controllers.get(key) {
                for id in ids {
                    let controller = self.state.controllers.get_mut(id).unwrap();
                    controller.numeric_outputs.clear();
                    controller.numeric_targets.clear();
                    controller.ready = false;
                    if let Some(transition) = &mut controller.transition {
                        transition.program = None;
                        let source = match &mut transition.source {
                            super::system_state::AnimationTransitionSource::Live(source) => source,
                            super::system_state::AnimationTransitionSource::Frozen {
                                bindings,
                                ..
                            } => bindings,
                        };
                        source.numeric_outputs.clear();
                        source.numeric_targets.clear();
                        source.ready = false;
                    }
                }
            }
        }

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
        // Numeric playback time edits preserve readiness. Source replacement
        // must prepare the new particle cache even when the component survives.
        for (&key, value) in context.staged.prepared.iter() {
            if let ComponentValue::ParticlePlayback(next) = value
                && context
                    .world_data
                    .components
                    .particle_playback(key.0.index() as usize)
                    .is_some_and(|old| {
                        !crate::components::schema::same_text(&old.source, &next.source)
                            || old.variant != next.variant
                    })
                && let Some(ids) = self.state.target_controllers.get(&key)
            {
                for id in ids {
                    self.state.controllers.get_mut(id).unwrap().ready = false;
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
            && context.staged.operation_deleted.is_empty()
        {
            return;
        }
        let affected = self.state.affected_by(context.staged);
        if context
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
                    context.staged.changed.contains_key(&(
                        driver.identity().entity,
                        driver.identity().property.component_target(),
                    )) && departs_individually(&driver.description().property)
                        && !read.animation_binding_alive(*driver, context.staged)
                })
                .map(|driver| driver.description().clone())
                .collect();
            if removed.is_empty() {
                continue;
            }
            let departed: Vec<_> = controller
                .drivers
                .iter()
                .filter(|driver| removed.contains(driver.description()))
                .map(|driver| driver.identity().clone())
                .collect();
            controller
                .contributions
                .retain(|identity| !departed.contains(identity));
            controller
                .drivers
                .retain(|driver| !removed.contains(driver.description()));
            controller.reindex_drivers();
            if controller.drivers.is_empty() && controller.structural_drivers.is_empty() {
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
        let invalid: Vec<_> =
            {
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
                        (controller.structural_drivers.iter().any(|driver| {
                            context.staged.operation_deleted.contains(&driver.target())
                        }) || controller.changed_drivers(context.staged).any(|driver| {
                            context.staged.changed.contains_key(&(
                                driver.identity().entity,
                                driver.identity().property.component_target(),
                            )) && !read.animation_binding_alive(driver, context.staged)
                        }) || controller.transition.as_deref().is_some_and(|transition| {
                            transition.program.as_ref().is_some_and(|program| {
                                program
                                    .invalidated_by(context.staged, &context.world_data.components)
                            }) || matches!(
                                &transition.source,
                                super::system_state::AnimationTransitionSource::Frozen {
                                    values,
                                    ..
                                } if super::controller::frozen_values_invalidated(
                                    values,
                                    context.staged,
                                    &context.world_data.components,
                                )
                            )
                        }))
                        .then_some(id)
                    })
                    .collect()
            };
        // Contributions leave through commit cleanup onto each component's
        // staged value, so the subtraction combines with this batch's writes.
        let mut restorations = BTreeMap::new();
        for id in invalid {
            let mut controller = self.state.controllers.remove(&id).unwrap();
            for (identity, applied) in controller.contributions.take() {
                if !applied.is_empty() {
                    stage_withdrawal(&mut restorations, context, &identity, &applied);
                }
            }
            for driver in std::mem::take(&mut controller.structural_drivers) {
                self.state.resample_structural_target(driver.target());
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

fn target_retained(
    context: &SystemCommitContext<'_>,
    identity: &super::targets::AnimationTargetIdentity,
) -> bool {
    context
        .staged
        .entities
        .get(&identity.entity)
        .and_then(|record| record.input(identity.property.component_target()))
        .is_some_and(|input| input.incarnation == identity.incarnation)
}

/// Subtract one contribution from its component's staged value; a replaced or
/// removed component keeps nothing of the departing controller.
fn stage_withdrawal(
    restorations: &mut BTreeMap<(EntityId, u16), ComponentValue>,
    context: &SystemCommitContext<'_>,
    identity: &super::targets::AnimationTargetIdentity,
    applied: &super::controller::contribution::AnimationApplied,
) {
    let Some(property) = identity.property.property() else {
        return;
    };
    if !target_retained(context, identity) {
        return;
    }
    let key = (identity.entity, property.component);
    if let std::collections::btree_map::Entry::Vacant(entry) = restorations.entry(key)
        && let Some(value) =
            context
                .staged
                .input_value(&context.world_data.components, key.0, key.1)
    {
        entry.insert(value);
    }
    if let Some(value) = restorations.get_mut(&key) {
        let _ = withdraw_from(value, identity, applied);
    }
}

/// Subtract `applied` from its property of `value`. A result the field rejects
/// leaves the value as it is.
fn withdraw_from(
    value: &mut ComponentValue,
    identity: &super::targets::AnimationTargetIdentity,
    applied: &super::controller::contribution::AnimationApplied,
) -> Result<(), ErrorReason> {
    let property = identity
        .property
        .property()
        .ok_or(ErrorReason::InvalidField)?;
    let current = AnimationValue::read(property, value)?;
    let next = applied.withdrawn(&current)?;
    let mut candidate = value.clone();
    next.write(property, &mut candidate)?;
    candidate.validate_lifecycle()?;
    *value = candidate;
    Ok(())
}

/// A departed dynamic property, removed row or cleared optional row property
/// drops only its own drivers; other targets invalidate the whole controller.
fn departs_individually(target: &AnimationTrackTarget) -> bool {
    matches!(target, AnimationTrackTarget::DynamicProperty { .. })
        || target.property().is_some_and(|property| {
            property
                .offsets
                .iter()
                .any(|offset| crate::components::rows::row_region(*offset).is_some())
        })
}
