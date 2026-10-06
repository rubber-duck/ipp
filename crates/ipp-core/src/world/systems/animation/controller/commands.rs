use crate::systems::animation::system_state::{
    AnimationRuntimeFrozenTransitionValue, AnimationTransitionRuntime, AnimationTransitionSource,
};
use crate::systems::animation::targets::{
    AnimationComponentValues, AnimationTargetIdentity, frozen_transition_target_supported,
    validate_target_support,
};
use crate::systems::animation::{
    ANIMATION_TYPE, AnimationAccess, AnimationController, AnimationControllerCommand,
    AnimationControllerDescription, AnimationControllerId, AnimationControllerOutcome,
    AnimationControllerSnapshot, AnimationControllerState, AnimationControllerTransition,
    AnimationControllerTransitionState, AnimationDriverDescription,
    AnimationPersistentContribution, AnimationPersistentState, AnimationPersistentTransitionSource,
    AnimationPlaybackControl, AnimationPlaybackEvent, AnimationPlaybackEventKind,
    AnimationPlaybackStatus, AnimationTrackTarget, AnimationTransitionStartTime, AnimationValue,
    MAX_CONTROLLERS,
};
use crate::{
    ComponentValue, EntityId, ErrorReason, services::asset_management::AssetDemandSelection,
};
use std::collections::{BTreeMap, BTreeSet};

impl AnimationAccess<'_, '_> {
    pub(in crate::world) fn apply_animation_controller_command(
        &mut self,
        request_id: u64,
        command: AnimationControllerCommand,
    ) -> Result<(), ErrorReason> {
        let result = match command {
            AnimationControllerCommand::Create(description) => {
                self.create_animation_controller(description).map(Some)
            }
            AnimationControllerCommand::Update {
                id,
                description,
            } => self
                .update_animation_controller(id, description)
                .map(|()| None),
            AnimationControllerCommand::Transition {
                id,
                transition,
            } => self
                .transition_animation_controller(id, transition)
                .map(|()| None),
            AnimationControllerCommand::Delete {
                id,
            } => self.remove_animation_controller(id).map(|()| None),
            AnimationControllerCommand::Control {
                id,
                control,
            } => self.control_playback(id, control).map(|()| None),
        };
        let status = result.as_ref().map(|_| ()).map_err(|reason| *reason);
        self.system
            .state
            .controller_outcomes
            .push(AnimationControllerOutcome {
                request_id,
                result,
            });
        status
    }

    pub(in crate::world::systems::animation) fn event(
        &mut self,
        controller: &AnimationController,
        kind: AnimationPlaybackEventKind,
        reason: Option<ErrorReason>,
    ) {
        let snapshot = &controller.snapshot;
        self.system
            .state
            .playback_events
            .push(AnimationPlaybackEvent {
                controller: AnimationControllerState {
                    id: snapshot.id,
                    state: snapshot.state,
                    time: snapshot.time,
                },
                kind,
                reason,
            });
    }

    /// Validate and commit one stopped controller at a mutation boundary.
    pub fn create_animation_controller(
        &mut self,
        description: AnimationControllerDescription,
    ) -> Result<AnimationControllerId, ErrorReason> {
        let incarnations = self
            .read()
            .validate_controller_description(&description, 0.0)?;
        let next_id = self
            .system
            .state
            .next_id
            .checked_add(1)
            .ok_or(ErrorReason::Capacity)?;
        let id = AnimationControllerId::from_bits(self.system.state.next_id);
        let demand = self
            .read()
            .controller_demand(Some((id, &description)), None)?;
        self.system.state.controllers.insert(
            id,
            AnimationController {
                snapshot: AnimationControllerSnapshot {
                    id,
                    description,
                    state: AnimationPlaybackStatus::Stopped,
                    time: 0.0,
                    transition: None,
                },
                drivers: Vec::new(),
                structural_drivers: Vec::new(),
                driver_targets: BTreeMap::new(),
                incarnations,
                sought: false,
                directional_start_pending: false,
                duration: 0.0,
                ready: false,
                numeric_targets: Vec::new(),
                discrete_drivers: Vec::new(),
                numeric_outputs: Vec::new(),
                failure: None,
                transition: None,
                contributions: Default::default(),
            },
        );
        self.system.state.index_controller(id);
        self.system.state.next_id = next_id;
        self.system.state.demand_revision = self.system.state.demand_revision.wrapping_add(1);
        self.system.state.animation_sources = demand;
        Ok(id)
    }

    /// Atomically update descriptions and settings while retaining clock state.
    pub(in crate::world::systems::animation) fn update_animation_controller(
        &mut self,
        id: AnimationControllerId,
        description: AnimationControllerDescription,
    ) -> Result<(), ErrorReason> {
        let old = self
            .system
            .state
            .controllers
            .get(&id)
            .ok_or(ErrorReason::InvalidValue)?;
        let incarnations = self
            .read()
            .validate_controller_description(&description, old.snapshot.time)?;
        let demand = self
            .read()
            .controller_demand(Some((id, &description)), None)?;
        if old.snapshot.description.drivers == description.drivers {
            let controller = self.system.state.controllers.get_mut(&id).unwrap();
            controller.snapshot.description.speed = description.speed;
            controller.snapshot.description.looping = description.looping;
            self.system.state.animation_sources = demand;
            return Ok(());
        }
        self.withdraw_controller(id);
        let structural = std::mem::take(
            &mut self
                .system
                .state
                .controllers
                .get_mut(&id)
                .unwrap()
                .structural_drivers,
        );
        self.release_structural_drivers(structural);
        let controller = self.system.state.controllers.get_mut(&id).unwrap();
        controller.snapshot.description = description;
        controller.clear_drivers();
        controller.incarnations = incarnations;
        controller.failure = None;
        controller.transition = None;
        controller.snapshot.transition = None;
        self.system.state.index_controller(id);
        self.system.state.demand_revision = self.system.state.demand_revision.wrapping_add(1);
        self.system.state.animation_sources = demand;
        Ok(())
    }

    /// Begin an interruptible crossfade to a replacement controller description.
    pub(in crate::world::systems::animation) fn transition_animation_controller(
        &mut self,
        id: AnimationControllerId,
        transition: AnimationControllerTransition,
    ) -> Result<(), ErrorReason> {
        if !transition.duration.is_finite() || transition.duration < 0.0 {
            return Err(ErrorReason::InvalidValue);
        }
        if matches!(transition.start_time, AnimationTransitionStartTime::Seek(time) if !time.is_finite() || time < 0.0)
        {
            return Err(ErrorReason::InvalidValue);
        }
        let incarnations = self
            .read()
            .validate_controller_description(&transition.description, 0.0)?;
        self.validate_transition_targets(&transition.description, &incarnations)?;
        let old = self
            .system
            .state
            .controllers
            .get(&id)
            .ok_or(ErrorReason::InvalidValue)?;
        self.validate_transition_targets(&old.snapshot.description, &old.incarnations)?;
        // Capture before mutating indexes or demand so any validation failure leaves the
        // existing controller fully installed.
        let interrupted_values = old
            .transition
            .as_ref()
            .and_then(|runtime| runtime.program.as_ref())
            .map(|program| program.freeze(Some(&old.contributions)))
            .transpose()?;
        let mut demand = self
            .read()
            .controller_demand(Some((id, &transition.description)), Some(id))?;
        let destination_time = match transition.start_time {
            AnimationTransitionStartTime::Seek(time) => time,
            AnimationTransitionStartTime::Preserve => old.snapshot.time,
            AnimationTransitionStartTime::Restart | AnimationTransitionStartTime::MatchPhase => 0.0,
        };
        self.system.state.unindex_controller(id);
        let mut source = self.system.state.controllers.remove(&id).unwrap();
        // The destination takes over what the outgoing side has in its fields.
        let contributions = source.contributions.take();
        let source_state = source.snapshot.state;
        let source_ready = source.ready;
        source.snapshot.transition = None;
        let transition_source = if let Some(previous) = source.transition.take() {
            let reference_time = source.snapshot.time;
            let reference_duration = source.duration;
            if let Some(prepared) = previous.program {
                let values = interrupted_values.expect("prepared transition was captured");
                source.snapshot.description.drivers.clear();
                source.snapshot.time = 0.0;
                source.snapshot.state = AnimationPlaybackStatus::Stopped;
                source.incarnations.clear();
                source.clear_drivers();
                source.duration = 0.0;
                AnimationTransitionSource::Frozen {
                    values,
                    bindings: Box::new(source),
                    prepared: Some(prepared),
                    reference_time,
                    reference_duration,
                }
            } else {
                match previous.source {
                    AnimationTransitionSource::Live(source) => {
                        AnimationTransitionSource::Live(source)
                    }
                    AnimationTransitionSource::Frozen {
                        values,
                        bindings,
                        prepared,
                        ..
                    } => AnimationTransitionSource::Frozen {
                        values,
                        bindings,
                        prepared,
                        reference_time,
                        reference_duration,
                    },
                }
            }
        } else {
            AnimationTransitionSource::Live(Box::new(source))
        };
        if let AnimationTransitionSource::Live(source) = &transition_source {
            add_description_demand(&source.snapshot.description, &mut demand);
        }
        let state = if source_state == AnimationPlaybackStatus::Paused {
            AnimationPlaybackStatus::Paused
        } else {
            AnimationPlaybackStatus::Playing
        };
        let summary = AnimationControllerTransitionState {
            duration: transition.duration,
            elapsed: 0.0,
            easing: transition.easing,
            pending: true,
        };
        let mut controller = AnimationController {
            snapshot: AnimationControllerSnapshot {
                id,
                description: transition.description,
                state,
                time: destination_time,
                transition: Some(summary),
            },
            drivers: Vec::new(),
            structural_drivers: Vec::new(),
            driver_targets: BTreeMap::new(),
            incarnations,
            sought: true,
            directional_start_pending: false,
            duration: 0.0,
            ready: source_ready,
            numeric_targets: Vec::new(),
            discrete_drivers: Vec::new(),
            numeric_outputs: Vec::new(),
            failure: None,
            transition: Some(Box::new(AnimationTransitionRuntime {
                source: transition_source,
                start_time: transition.start_time,
                program: None,
            })),
            contributions: Default::default(),
        };
        controller.contributions.replace(contributions);
        self.system.state.controllers.insert(id, controller);
        self.system.state.index_controller(id);
        self.system.state.demand_revision = self.system.state.demand_revision.wrapping_add(1);
        self.system.state.animation_sources = demand;
        Ok(())
    }

    fn validate_transition_targets(
        &self,
        description: &AnimationControllerDescription,
        incarnations: &[(u64, AnimationTrackTarget)],
    ) -> Result<(), ErrorReason> {
        for (driver, (_, property)) in description.drivers.iter().zip(incarnations) {
            if matches!(property, AnimationTrackTarget::EntityLink) {
                return Err(ErrorReason::InvalidField);
            }
            let component = self
                .context
                .world
                .state
                .input_value(
                    &self.context.world.components,
                    driver.target,
                    property.component_target(),
                )
                .ok_or(ErrorReason::MissingComponent)?;
            let value = self.read().read_animation_target(property, &component)?;
            match value {
                AnimationValue::Field(crate::components::schema::FieldValue::F32(_)) => {
                    if !frozen_transition_target_supported(property, &value) {
                        return Err(ErrorReason::InvalidField);
                    }
                }
                AnimationValue::Rotation(_) => {}
                AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(value))
                    if !matches!(
                        value,
                        crate::DynamicValue::Bool(_) | crate::DynamicValue::Asset(_)
                    ) => {}
                AnimationValue::Pose(_) => {}
                _ => return Err(ErrorReason::InvalidField),
            }
        }
        Ok(())
    }

    /// Restore contributions and remove a controller without recycling its identity.
    pub(in crate::world::systems::animation) fn remove_animation_controller(
        &mut self,
        id: AnimationControllerId,
    ) -> Result<(), ErrorReason> {
        if !self.system.state.controllers.contains_key(&id) {
            return Err(ErrorReason::InvalidValue);
        }
        self.withdraw_controller(id);
        let structural = std::mem::take(
            &mut self
                .system
                .state
                .controllers
                .get_mut(&id)
                .unwrap()
                .structural_drivers,
        );
        self.release_structural_drivers(structural);
        self.system.state.controllers.remove(&id);
        self.system.state.unindex_controller(id);
        self.system.state.demand_revision = self.system.state.demand_revision.wrapping_add(1);
        self.system.state.animation_sources = self.read().controller_demand(None, None)?;
        Ok(())
    }

    pub(in crate::world::systems::animation) fn control_playback(
        &mut self,
        id: AnimationControllerId,
        control: AnimationPlaybackControl,
    ) -> Result<(), ErrorReason> {
        validate_control(control)?;
        let controller = self
            .system
            .state
            .controllers
            .get(&id)
            .ok_or(ErrorReason::InvalidValue)?;
        let mut incarnations = None;
        if matches!(
            control,
            AnimationPlaybackControl::Play
                | AnimationPlaybackControl::PlayAtSpeed(_)
                | AnimationPlaybackControl::Restart
        ) && controller.drivers.is_empty()
            && controller.structural_drivers.is_empty()
        {
            incarnations = Some(self.read().validate_controller_description(
                &controller.snapshot.description,
                controller.snapshot.time,
            )?);
        }
        if let AnimationPlaybackControl::Seek(time) = control {
            self.read()
                .validate_controller_description(&controller.snapshot.description, time)?;
        }
        let known_duration =
            controller
                .snapshot
                .description
                .drivers
                .iter()
                .try_fold(0.0_f64, |duration, driver| {
                    let clip = self
                        .read()
                        .source_key(driver)
                        .and_then(|key| self.read().clip_by_key(key))?;
                    Some(duration.max(clip.duration()))
                });
        let replay_endpoint = control == AnimationPlaybackControl::Play
            && matches!(
                controller.snapshot.state,
                AnimationPlaybackStatus::Stopped | AnimationPlaybackStatus::Completed
            )
            && !controller.sought
            && known_duration.is_some_and(|duration| {
                if controller.snapshot.description.speed < 0.0 {
                    controller.snapshot.time == 0.0
                } else {
                    controller.snapshot.time == duration
                }
            });
        if control == AnimationPlaybackControl::Stop {
            self.withdraw_controller(id);
            let structural = std::mem::take(
                &mut self
                    .system
                    .state
                    .controllers
                    .get_mut(&id)
                    .unwrap()
                    .structural_drivers,
            );
            self.release_structural_drivers(structural);
        }
        let mut controller = self.system.state.take_controller(id).unwrap();
        let old = controller.snapshot.state;
        let mut event = None;
        match control {
            AnimationPlaybackControl::Play
            | AnimationPlaybackControl::PlayAtSpeed(_)
            | AnimationPlaybackControl::Restart => {
                if control == AnimationPlaybackControl::Restart || replay_endpoint {
                    controller.snapshot.time = directional_start(
                        controller.snapshot.description.speed,
                        known_duration.unwrap_or(controller.duration),
                    );
                    controller.sought = true;
                    controller.directional_start_pending = known_duration.is_none();
                }
                if let AnimationPlaybackControl::PlayAtSpeed(speed) = control {
                    controller.snapshot.description.speed = speed;
                    controller.directional_start_pending = false;
                }
                controller.snapshot.state = AnimationPlaybackStatus::Playing;
                if let Some(incarnations) = incarnations {
                    controller.incarnations = incarnations;
                }
                if old != AnimationPlaybackStatus::Playing {
                    event = Some(AnimationPlaybackEventKind::Started);
                }
            }
            AnimationPlaybackControl::Pause => {
                if old == AnimationPlaybackStatus::Playing {
                    controller.snapshot.state = AnimationPlaybackStatus::Paused;
                    event = Some(AnimationPlaybackEventKind::Paused);
                }
            }
            AnimationPlaybackControl::Stop => {
                controller.snapshot.state = AnimationPlaybackStatus::Stopped;
                controller.snapshot.transition = None;
                controller.transition = None;
                controller.clear_drivers();
                controller.failure = None;
                controller.directional_start_pending = false;
                if old != AnimationPlaybackStatus::Stopped {
                    event = Some(AnimationPlaybackEventKind::Stopped);
                }
            }
            AnimationPlaybackControl::Seek(time) => {
                controller.snapshot.time = time;
                controller.sought = true;
                controller.directional_start_pending = false;
                if old == AnimationPlaybackStatus::Completed {
                    controller.snapshot.state = AnimationPlaybackStatus::Paused;
                }
            }
        }
        if let Some(event) = event {
            self.event(&controller, event, None);
        }
        self.system.state.controllers.insert(id, controller);
        Ok(())
    }

    /// Subtract everything a controller has applied from its still-live fields:
    /// on stop, removal, description change and invalidation. A subtraction the
    /// field rejects leaves the field as it is.
    pub(in crate::world::systems::animation) fn withdraw_controller(
        &mut self,
        id: AnimationControllerId,
    ) {
        #[cfg(feature = "instrumentation")]
        let measurement =
            crate::profiling::Stage::fixed(crate::profiling::FixedStage::AnimationRestoreAndStage);

        let Some(mut controller) = self.system.state.take_controller(id) else {
            return;
        };
        let contributions = controller.contributions.take();
        let mut values = AnimationComponentValues::new(
            std::mem::take(&mut self.system.state.component_scratch),
            std::mem::take(&mut self.system.state.property_scratch),
        );
        self.context
            .before_numeric_update(&controller.numeric_targets);
        for (identity, applied) in &contributions {
            if applied.is_empty()
                || !self
                    .read()
                    .animation_target_alive(identity, &self.context.world.state)
            {
                continue;
            }
            let _ = withdraw(
                &mut values,
                &self.context.world.components,
                identity,
                applied,
            );
        }
        // Every bound driver must be visible to synchronous lifecycle callbacks
        // before the writes can replace another component's internal storage.
        #[cfg(feature = "instrumentation")]
        drop(measurement);

        self.system.state.controllers.insert(id, controller);
        let (properties, updates) = values.drain();
        for (key, value) in updates {
            let output = self.system.state.controllers[&id].numeric_output(key);
            let _ = self.apply_component_update(key, output, value, properties);
        }
        (
            self.system.state.component_scratch,
            self.system.state.property_scratch,
        ) = values.into_scratch();
    }

    /// Install persistent descriptions and frozen clocks, rebuilding runtime bindings on readiness.
    pub fn restore_animation_controllers(
        &mut self,
        persistent: AnimationPersistentState,
    ) -> Result<(), ErrorReason> {
        if !self
            .context
            .world
            .manifest
            .supports_operation(crate::systems::WorldOperation::Animation)
        {
            return Err(ErrorReason::UnsupportedDependency);
        }
        if persistent.next_id == 0 {
            return Err(ErrorReason::InvalidValue);
        }
        let transitions = persistent.transitions;
        let contributions = persistent.contributions;
        let directional_count = persistent.directional_starts.len();
        let directional_starts: BTreeSet<_> = persistent.directional_starts.into_iter().collect();
        if directional_starts.len() != directional_count {
            return Err(ErrorReason::InvalidValue);
        }
        let mut controllers = BTreeMap::new();
        for snapshot in persistent.controllers {
            if snapshot.id.to_bits() == 0
                || snapshot.id.to_bits() >= persistent.next_id
                || controllers.contains_key(&snapshot.id)
                || !snapshot.time.is_finite()
                || snapshot.time < 0.0
            {
                return Err(ErrorReason::InvalidValue);
            }
            let incarnations = if snapshot.description.drivers.is_empty() {
                if snapshot.state != AnimationPlaybackStatus::Stopped
                    || !snapshot.description.speed.is_finite()
                {
                    return Err(ErrorReason::InvalidValue);
                }
                Vec::new()
            } else {
                self.read()
                    .validate_controller_description(&snapshot.description, snapshot.time)?
            };
            let directional_start_pending = directional_starts.contains(&snapshot.id);
            controllers.insert(
                snapshot.id,
                AnimationController {
                    snapshot,
                    drivers: Vec::new(),
                    structural_drivers: Vec::new(),
                    driver_targets: BTreeMap::new(),
                    incarnations,
                    sought: true,
                    directional_start_pending,
                    duration: 0.0,
                    ready: false,
                    numeric_targets: Vec::new(),
                    discrete_drivers: Vec::new(),
                    numeric_outputs: Vec::new(),
                    failure: None,
                    transition: None,
                    contributions: Default::default(),
                },
            );
        }
        if controllers.len() > MAX_CONTROLLERS {
            return Err(ErrorReason::Capacity);
        }
        if directional_starts.iter().any(|id| {
            !controllers.get(id).is_some_and(|controller| {
                matches!(
                    controller.snapshot.state,
                    AnimationPlaybackStatus::Playing | AnimationPlaybackStatus::Paused
                )
            })
        }) {
            return Err(ErrorReason::InvalidValue);
        }
        for transition in transitions {
            let destination = controllers
                .get_mut(&transition.id)
                .ok_or(ErrorReason::InvalidValue)?;
            if destination.snapshot.transition.is_none() {
                return Err(ErrorReason::InvalidValue);
            }
            let source = match transition.source {
                AnimationPersistentTransitionSource::Live(snapshot) => {
                    AnimationTransitionSource::Live(Box::new(
                        self.restored_transition_controller(snapshot)?,
                    ))
                }
                AnimationPersistentTransitionSource::Frozen {
                    values,
                    bindings,
                    reference_time,
                    reference_duration,
                } => {
                    let bindings = Box::new(self.restored_transition_controller(bindings)?);
                    let values = values
                        .into_iter()
                        .map(|value| {
                            validate_target_support(&self.context.world.manifest, &value.property)?;
                            let component = value
                                .property
                                .component()
                                .ok_or(ErrorReason::InvalidField)?;
                            let incarnation = self
                                .context
                                .world
                                .state
                                .entities
                                .get(&value.target)
                                .and_then(|record| record.input(component))
                                .map(|input| input.incarnation)
                                .ok_or(ErrorReason::MissingComponent)?;
                            Ok(AnimationRuntimeFrozenTransitionValue {
                                target: value.target,
                                incarnation,
                                property: value.property,
                                baseline: super::contribution::identity_value(&value.value),
                                value: value.value,
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    AnimationTransitionSource::Frozen {
                        values,
                        bindings,
                        prepared: None,
                        reference_time,
                        reference_duration,
                    }
                }
            };
            destination.transition = Some(Box::new(AnimationTransitionRuntime {
                source,
                start_time: transition.start_time,
                program: None,
            }));
            destination.snapshot.transition.as_mut().unwrap().pending = true;
        }
        let mut demand = BTreeSet::new();
        for controller in controllers.values() {
            add_description_demand(&controller.snapshot.description, &mut demand);
        }
        self.read().validate_animation_demand(&demand)?;
        // Fields hold what was saved, contributions included; each controller
        // resumes knowing what it applied.
        for contribution in contributions {
            let controller = controllers
                .get_mut(&contribution.controller)
                .ok_or(ErrorReason::InvalidValue)?;
            let identity = self.saved_contribution_target(&contribution)?;
            if controller.contributions.get(&identity).is_some() {
                return Err(ErrorReason::InvalidValue);
            }
            controller.contributions.set(&identity, contribution.value);
        }
        // Replaced controllers leave their fields as they are: restoring pairs the
        // saved contributions with the fields they were saved with.
        let mut structural = Vec::new();
        for controller in self.system.state.controllers.values_mut() {
            structural.append(&mut controller.structural_drivers);
        }
        self.release_structural_drivers(structural);
        self.system.state.controllers = controllers;
        self.system.state.rebuild_target_index();
        self.system.state.next_id = persistent.next_id;
        self.system.state.demand_revision = self.system.state.demand_revision.wrapping_add(1);
        self.system.state.animation_sources = demand;
        Ok(())
    }

    /// Resolve a saved contribution's field and check that it can hold it.
    fn saved_contribution_target(
        &self,
        contribution: &AnimationPersistentContribution,
    ) -> Result<AnimationTargetIdentity, ErrorReason> {
        validate_target_support(&self.context.world.manifest, &contribution.property)?;
        let property = contribution
            .property
            .property()
            .ok_or(ErrorReason::InvalidField)?;
        let incarnation = self
            .context
            .world
            .state
            .entities
            .get(&contribution.target)
            .and_then(|record| record.input(property.component))
            .map(|input| input.incarnation)
            .ok_or(ErrorReason::MissingComponent)?;
        let value = self
            .context
            .world
            .state
            .input_value(
                &self.context.world.components,
                contribution.target,
                property.component,
            )
            .ok_or(ErrorReason::MissingComponent)?;
        let current = self
            .read()
            .read_animation_target(&contribution.property, &value)?;
        if !super::contribution::contributes(&current) || !current.same_type(&contribution.value) {
            return Err(ErrorReason::InvalidField);
        }
        Ok(AnimationTargetIdentity {
            entity: contribution.target,
            incarnation,
            property: contribution.property.clone(),
        })
    }

    fn restored_transition_controller(
        &self,
        snapshot: AnimationControllerSnapshot,
    ) -> Result<AnimationController, ErrorReason> {
        let incarnations = if snapshot.description.drivers.is_empty() {
            if snapshot.state != AnimationPlaybackStatus::Stopped
                || !snapshot.description.speed.is_finite()
            {
                return Err(ErrorReason::InvalidValue);
            }
            Vec::new()
        } else {
            self.read()
                .validate_controller_description(&snapshot.description, snapshot.time)?
        };
        Ok(AnimationController {
            snapshot,
            drivers: Vec::new(),
            structural_drivers: Vec::new(),
            driver_targets: BTreeMap::new(),
            incarnations,
            sought: true,
            directional_start_pending: false,
            duration: 0.0,
            ready: false,
            numeric_targets: Vec::new(),
            discrete_drivers: Vec::new(),
            numeric_outputs: Vec::new(),
            failure: None,
            transition: None,
            contributions: Default::default(),
        })
    }

    pub(in crate::world::systems::animation) fn sync_description_demand(&mut self) {
        let state = &mut self.system.state;
        if !state.description_demand_clean {
            let mut demand = BTreeSet::new();
            for controller in state.controllers.values() {
                add_description_demand(&controller.snapshot.description, &mut demand);
                if let Some(transition) = controller.transition.as_deref() {
                    let source = match &transition.source {
                        AnimationTransitionSource::Live(source) => source.as_ref(),
                        AnimationTransitionSource::Frozen {
                            bindings,
                            ..
                        } => bindings.as_ref(),
                    };
                    add_description_demand(&source.snapshot.description, &mut demand);
                }
                if controller.snapshot.state != AnimationPlaybackStatus::Stopped {
                    for &key in controller.driver_targets.keys() {
                        if (!ComponentValue::asset_references(key.1).is_empty()
                            || ComponentValue::supports_dynamic_properties(key.1))
                            && let Some(value) = self
                                .context
                                .world
                                .components
                                .get(key.1, key.0.index() as usize)
                        {
                            value.resource_demand(&mut demand);
                        }
                    }
                }
            }
            state.demand_revision = state.demand_revision.wrapping_add(1);
            state.animation_sources = demand;
            state.description_demand_clean = true;
        }
    }
}

pub(in crate::world::systems::animation) fn validate_control(
    control: AnimationPlaybackControl,
) -> Result<(), ErrorReason> {
    if matches!(control, AnimationPlaybackControl::Seek(time) if !time.is_finite() || time < 0.0)
        || matches!(control, AnimationPlaybackControl::PlayAtSpeed(speed) if !speed.is_finite())
    {
        return Err(ErrorReason::InvalidValue);
    }
    Ok(())
}

pub(in crate::world::systems::animation) fn directional_start(speed: f32, duration: f64) -> f64 {
    if speed < 0.0 {
        duration
    } else {
        0.0
    }
}

pub(in crate::world::systems::animation) fn description_bytes(
    description: &AnimationControllerDescription,
) -> usize {
    let headers = description
        .drivers
        .capacity()
        .saturating_mul(std::mem::size_of::<AnimationDriverDescription>() * 2 + 64);
    description.drivers.iter().fold(
        headers.saturating_add(std::mem::size_of::<AnimationController>()),
        |bytes, driver| {
            bytes.saturating_add(
                driver
                    .source
                    .len()
                    .saturating_add(driver.property.owned_bytes())
                    .saturating_add(
                        driver
                            .entity_bindings
                            .capacity()
                            .saturating_mul(std::mem::size_of::<EntityId>()),
                    )
                    .saturating_mul(2),
            )
        },
    )
}

pub(in crate::world::systems::animation) fn add_description_demand(
    description: &AnimationControllerDescription,
    demand: &mut BTreeSet<AssetDemandSelection>,
) {
    for driver in &description.drivers {
        AssetDemandSelection::insert_into(demand, ANIMATION_TYPE, &driver.source, driver.variant);
    }
}

/// Stage the subtraction of `applied` from its field.
fn withdraw(
    values: &mut AnimationComponentValues,
    storage: &crate::components::registry::ComponentStorage,
    identity: &AnimationTargetIdentity,
    applied: &super::contribution::AnimationApplied,
) -> Result<(), ErrorReason> {
    let property = identity
        .property
        .property()
        .ok_or(ErrorReason::InvalidField)?;
    let key = (identity.entity, property.component);
    if let [offset] = property.offsets.as_slice()
        && let Some(current) = values.numeric_current(key, *offset, storage)
    {
        let AnimationValue::Field(next) = applied.withdrawn(&AnimationValue::Field(current))?
        else {
            return Err(ErrorReason::InvalidField);
        };
        return values.set_numeric(key, *offset, next);
    }
    let value = values.get_or_insert(key, || {
        storage
            .get(key.1, key.0.index() as usize)
            .ok_or(ErrorReason::MissingComponent)
    })?;
    let current = AnimationValue::read(property, value)?;
    applied.withdrawn(&current)?.write(property, value)
}
