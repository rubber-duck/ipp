use super::*;
use crate::{ComponentValue, services::asset_management::service::AssetDemandSelection};
use std::collections::{BTreeMap, BTreeSet, HashSet};

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
                .update_ordinary_animation_controller(id, description)
                .map(|()| None),
            AnimationControllerCommand::Transition {
                id,
                transition,
            } => self
                .transition_ordinary_animation_controller(id, transition)
                .map(|()| None),
            AnimationControllerCommand::Delete {
                id,
            } => self.remove_ordinary_animation_controller(id).map(|()| None),
            AnimationControllerCommand::Control {
                id,
                control,
            } => self.control_ordinary_playback(id, control).map(|()| None),
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

    pub(in crate::world) fn update_ordinary_animation_controller(
        &mut self,
        id: AnimationControllerId,
        description: AnimationControllerDescription,
    ) -> Result<(), ErrorReason> {
        self.system.ensure_ordinary_controller(id)?;
        self.update_animation_controller(id, description)
    }

    pub(in crate::world) fn transition_ordinary_animation_controller(
        &mut self,
        id: AnimationControllerId,
        transition: AnimationControllerTransition,
    ) -> Result<(), ErrorReason> {
        self.system.ensure_ordinary_controller(id)?;
        self.transition_animation_controller(id, transition)
    }

    pub(in crate::world) fn remove_ordinary_animation_controller(
        &mut self,
        id: AnimationControllerId,
    ) -> Result<(), ErrorReason> {
        self.system.ensure_ordinary_controller(id)?;
        self.remove_animation_controller(id)
    }

    pub(in crate::world) fn control_ordinary_playback(
        &mut self,
        id: AnimationControllerId,
        control: AnimationPlaybackControl,
    ) -> Result<(), ErrorReason> {
        self.system.ensure_ordinary_controller(id)?;
        self.control_playback(id, control)
    }

    pub(super) fn event(
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
                driver_targets: BTreeMap::new(),
                incarnations,
                sought: false,
                directional_start_pending: false,
                duration: 0.0,
                ready: false,
                retain_numeric: false,
                numeric_targets: Vec::new(),
                discrete_drivers: Vec::new(),
                numeric_outputs: Vec::new(),
                failure: None,
                transition: None,
            },
        );
        self.system.state.index_controller(id);
        self.system.state.next_id = next_id;
        self.system.state.demand_revision = self.system.state.demand_revision.wrapping_add(1);
        self.system.state.animation_sources = demand;
        Ok(id)
    }

    /// Atomically update descriptions and settings while retaining clock state.
    pub(super) fn update_animation_controller(
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
        self.restore_controller(id);
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
    pub(super) fn transition_animation_controller(
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
            .map(super::transition::AnimationTransitionProgram::freeze)
            .transpose()?;
        let mut demand = self
            .read()
            .controller_demand(Some((id, &transition.description)), Some(id))?;
        let destination_time = match transition.start_time {
            AnimationTransitionStartTime::Seek(time) => time,
            AnimationTransitionStartTime::Preserve => old.snapshot.time,
            AnimationTransitionStartTime::Restart | AnimationTransitionStartTime::MatchPhase => 0.0,
        };
        let hold_program = {
            let source = self.system.state.controllers.get_mut(&id).unwrap();
            if let Some(previous) = source.transition.as_deref_mut() {
                if previous.program.is_some() {
                    Some(
                        super::transition::AnimationTransitionProgram::bind_frozen_source(
                            interrupted_values.as_deref().unwrap(),
                            &self.context.world.components,
                        )?,
                    )
                } else {
                    match &mut previous.source {
                        super::system_state::AnimationTransitionSource::Live(source)
                            if source.ready
                                && source.snapshot.state != AnimationPlaybackStatus::Stopped =>
                        {
                            let mut program =
                                super::transition::AnimationTransitionProgram::bind_source(
                                    source,
                                    &self.context.world.components,
                                )?;
                            program.prepare_source_hold(source)?;
                            Some(program)
                        }
                        super::system_state::AnimationTransitionSource::Frozen {
                            values,
                            ..
                        } => Some(
                            super::transition::AnimationTransitionProgram::bind_frozen_source(
                                values,
                                &self.context.world.components,
                            )?,
                        ),
                        _ => None,
                    }
                }
            } else if source.ready && source.snapshot.state != AnimationPlaybackStatus::Stopped {
                let mut program = super::transition::AnimationTransitionProgram::bind_source(
                    source,
                    &self.context.world.components,
                )?;
                program.prepare_source_hold(source)?;
                Some(program)
            } else {
                None
            }
        };
        // Ingress keeps restored inputs in storage so later commands at this
        // boundary stage authored values; evaluation publishes the pending hold.
        if let Some(program) = hold_program.as_ref() {
            program.validate_source_hold()?;
        }
        self.system.state.unindex_controller(id);
        let mut source = self.system.state.controllers.remove(&id).unwrap();
        let source_state = source.snapshot.state;
        let source_ready = source.ready;
        let source_retain_numeric = source.retain_numeric;
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
                super::system_state::AnimationTransitionSource::Frozen {
                    values,
                    bindings: Box::new(source),
                    prepared: Some(prepared),
                    reference_time,
                    reference_duration,
                }
            } else {
                match previous.source {
                    super::system_state::AnimationTransitionSource::Live(source) => {
                        super::system_state::AnimationTransitionSource::Live(source)
                    }
                    super::system_state::AnimationTransitionSource::Frozen {
                        values,
                        bindings,
                        prepared,
                        ..
                    } => super::system_state::AnimationTransitionSource::Frozen {
                        values,
                        bindings,
                        prepared,
                        reference_time,
                        reference_duration,
                    },
                }
            }
        } else {
            super::system_state::AnimationTransitionSource::Live(Box::new(source))
        };
        if let super::system_state::AnimationTransitionSource::Live(source) = &transition_source {
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
        let controller = AnimationController {
            snapshot: AnimationControllerSnapshot {
                id,
                description: transition.description,
                state,
                time: destination_time,
                transition: Some(summary),
            },
            drivers: Vec::new(),
            driver_targets: BTreeMap::new(),
            incarnations,
            sought: true,
            directional_start_pending: false,
            duration: 0.0,
            ready: source_ready,
            retain_numeric: source_retain_numeric,
            numeric_targets: Vec::new(),
            discrete_drivers: Vec::new(),
            numeric_outputs: Vec::new(),
            failure: None,
            transition: Some(Box::new(super::system_state::AnimationTransitionRuntime {
                source: transition_source,
                start_time: transition.start_time,
                hold_program,
                program: None,
            })),
        };
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
            let component = self
                .context
                .world
                .state
                .input_value(
                    &self.context.world.components,
                    driver.target,
                    property.component(),
                )
                .ok_or(ErrorReason::MissingComponent)?;
            let value = self.read().read_animation_target(property, &component)?;
            match value {
                AnimationValue::Field(crate::components::schema::FieldValue::F32(_))
                | AnimationValue::Rotation(_) => {}
                AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(value))
                    if !matches!(
                        value,
                        crate::DynamicValue::Bool(_) | crate::DynamicValue::Asset(_)
                    ) => {}
                #[cfg(feature = "skeletal-animation")]
                AnimationValue::Pose(_) => {}
                _ => return Err(ErrorReason::InvalidField),
            }
        }
        Ok(())
    }

    /// Restore contributions and remove a controller without recycling its identity.
    pub(super) fn remove_animation_controller(
        &mut self,
        id: AnimationControllerId,
    ) -> Result<(), ErrorReason> {
        if !self.system.state.controllers.contains_key(&id) {
            return Err(ErrorReason::InvalidValue);
        }
        self.restore_controller(id);
        self.system.state.controllers.remove(&id);
        self.system.state.unindex_controller(id);
        self.system.state.demand_revision = self.system.state.demand_revision.wrapping_add(1);
        self.system.state.animation_sources = self.read().controller_demand(None, None)?;
        Ok(())
    }

    pub(super) fn control_playback(
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
            self.restore_controller(id);
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

    pub(super) fn restore_controller(&mut self, id: AnimationControllerId) {
        self.restore_controller_inputs(id, false);
    }

    pub(super) fn restore_controller_inputs(
        &mut self,
        id: AnimationControllerId,
        retain_outputs: bool,
    ) {
        if retain_outputs
            && self
                .system
                .state
                .controllers
                .get(&id)
                .is_some_and(|controller| {
                    controller.ready && controller.failure.is_none() && controller.retain_numeric
                })
        {
            return;
        }

        #[cfg(feature = "profiling")]
        let measurement =
            crate::profiling::Stage::new(17 * 6, "profile.animation.restore_and_stage");

        let Some(mut controller) = self.system.state.take_controller(id) else {
            return;
        };
        let mut restored_transition_union = false;
        if let Some(transition) = controller.transition.take() {
            if let Some(program) = transition.program.as_ref() {
                self.context
                    .before_numeric_update(program.numeric_targets());
                let _ = program.restore(
                    &mut self.context.world.components,
                    &self.context.world.state,
                );
                restored_transition_union = true;
            } else if let super::system_state::AnimationTransitionSource::Frozen {
                values,
                ..
            } = &transition.source
            {
                self.context
                    .before_numeric_update(&super::transition::frozen_numeric_targets(values));
                let _ = super::transition::restore_frozen_values(
                    values,
                    &mut self.context.world.components,
                    &self.context.world.state,
                );
                restored_transition_union = true;
            }
            controller.transition = Some(transition);
        }
        let mut values = super::component_values::AnimationComponentValues::new(
            std::mem::take(&mut self.system.state.component_scratch),
            std::mem::take(&mut self.system.state.property_scratch),
        );
        self.context
            .before_numeric_update(&controller.numeric_targets);
        for driver in controller
            .restoration_drivers()
            .filter(|_| !restored_transition_union)
        {
            if driver.restore_numeric(&mut self.context.world.components) {
                continue;
            }
            if driver.discrete() {
                let identity = driver.identity();
                let key = (identity.entity, identity.property.component());
                if retain_outputs
                    && controller.ready
                    && controller.failure.is_none()
                    && driver.retain_discrete()
                {
                    continue;
                }
                driver.reset_discrete();
                if let Some(property) = identity.property.property()
                    && let [offset] = property.offsets.as_slice()
                    && let AnimationValue::Field(original) = driver.original()
                {
                    let _ = values.set_numeric(key, *offset, original);
                    continue;
                }
            }
            let identity = driver.identity();
            if !crate::compiled_animation_enabled()
                && !self
                    .read()
                    .animation_target_alive(identity, &self.context.world.state)
            {
                continue;
            }
            #[cfg(feature = "skeletal-animation")]
            if matches!(
                driver.runtime_target(),
                super::driver::AnimationRuntimeTarget::JointLocal { .. }
            ) {
                if crate::allocation_followup_enabled() {
                    if let Some(original) = driver.joint_original() {
                        let _ = driver.runtime_target().write_joint_slice(
                            &mut self.context.world.components,
                            identity.entity,
                            original,
                        );
                    }
                    continue;
                }
                let _ = driver.runtime_target().write_joints(
                    &mut self.context.world.components,
                    identity.entity,
                    driver.original(),
                );
                continue;
            }
            let key = (identity.entity, identity.property.component());
            if crate::allocation_followup_enabled()
                && let Some(property) = identity.property.property()
                && let [offset] = property.offsets.as_slice()
                && values
                    .numeric_current(key, *offset, &self.context.world.components)
                    .is_some()
                && let AnimationValue::Field(original) = driver.original()
            {
                let _ = values.set_numeric(key, *offset, original);
                continue;
            }
            if let Ok(value) = values.get_or_insert(key, || {
                self.context
                    .world
                    .components
                    .get(key.1, key.0.index() as usize)
                    .ok_or(ErrorReason::MissingComponent)
            }) {
                let _ = driver.restore(value);
            }
        }
        // Every bound driver must be visible to synchronous lifecycle callbacks
        // before restoration can replace another component's internal storage.
        #[cfg(feature = "profiling")]
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

    /// Restore inputs required by mutation or composition; eligible replacement
    /// output can survive an empty mutation boundary until its next sample.
    pub(in crate::world) fn restore_animation_inputs(&mut self, retain_outputs: bool) {
        let restorations = std::mem::take(&mut self.system.state.pending_restorations);
        let mut values = BTreeMap::new();
        for (identity, original) in restorations {
            if !self
                .read()
                .animation_target_alive(&identity, &self.context.world.state)
            {
                continue;
            }
            let Some(property) = identity.property.property() else {
                continue;
            };
            let key = (identity.entity, identity.property.component());
            if let std::collections::btree_map::Entry::Vacant(entry) = values.entry(key)
                && let Some(value) = self
                    .context
                    .world
                    .components
                    .get(key.1, key.0.index() as usize)
            {
                entry.insert(value);
            }
            if let Some(value) = values.get_mut(&key) {
                let _ = original.write(property, value);
            }
        }
        for ((entity, _), value) in values {
            let _ = self.apply_sampled_value(entity, value);
        }
        let mut ids = std::mem::take(&mut self.system.state.controller_ids);
        ids.clear();
        ids.extend(self.system.state.controllers.keys().copied());
        let retain_outputs = retain_outputs
            && crate::compiled_animation_enabled()
            && self.context.world.queue.is_empty()
            && !self.context.world.admitting_ingress;
        for &id in &ids {
            self.restore_controller_inputs(id, retain_outputs);
        }
        if crate::allocation_optimizations_enabled() {
            self.system.state.controller_ids = ids;
        }
    }

    /// Install persistent descriptions and frozen clocks, rebuilding runtime bindings on readiness.
    pub fn restore_animation_controllers(
        &mut self,
        persistent: AnimationPersistentState,
    ) -> Result<(), ErrorReason> {
        if persistent.next_id == 0 {
            return Err(ErrorReason::InvalidValue);
        }
        let transitions = persistent.transitions;
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
                    driver_targets: BTreeMap::new(),
                    incarnations,
                    sought: true,
                    directional_start_pending,
                    duration: 0.0,
                    ready: false,
                    retain_numeric: false,
                    numeric_targets: Vec::new(),
                    discrete_drivers: Vec::new(),
                    numeric_outputs: Vec::new(),
                    failure: None,
                    transition: None,
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
                    super::system_state::AnimationTransitionSource::Live(Box::new(
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
                            let incarnation = self
                                .context
                                .world
                                .state
                                .entities
                                .get(&value.target)
                                .and_then(|record| record.input(value.property.component()))
                                .map(|input| input.incarnation)
                                .ok_or(ErrorReason::MissingComponent)?;
                            Ok(super::system_state::AnimationRuntimeFrozenTransitionValue {
                                target: value.target,
                                incarnation,
                                property: value.property,
                                value: value.value,
                                baseline: value.baseline,
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    super::system_state::AnimationTransitionSource::Frozen {
                        values,
                        bindings,
                        prepared: None,
                        reference_time,
                        reference_duration,
                    }
                }
            };
            destination.transition =
                Some(Box::new(super::system_state::AnimationTransitionRuntime {
                    source,
                    start_time: transition.start_time,
                    hold_program: None,
                    program: None,
                }));
            destination.snapshot.transition.as_mut().unwrap().pending = true;
        }
        let mut demand = BTreeSet::new();
        for controller in controllers.values() {
            add_description_demand(&controller.snapshot.description, &mut demand);
        }
        self.read().validate_animation_demand(&demand)?;
        self.restore_animation_inputs(false);
        #[cfg(feature = "gui")]
        self.system.invalidate_skin_controller_associations();
        self.system.state.controllers = controllers;
        self.system.state.rebuild_target_index();
        self.system.state.next_id = persistent.next_id;
        self.system.state.demand_revision = self.system.state.demand_revision.wrapping_add(1);
        self.system.state.animation_sources = demand;
        Ok(())
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
            driver_targets: BTreeMap::new(),
            incarnations,
            sought: true,
            directional_start_pending: false,
            duration: 0.0,
            ready: false,
            retain_numeric: false,
            numeric_targets: Vec::new(),
            discrete_drivers: Vec::new(),
            numeric_outputs: Vec::new(),
            failure: None,
            transition: None,
        })
    }

    /// Refresh sparse restoration values after committed producer/overlay writes.
    pub(in crate::world) fn refresh_animation_originals(&mut self, changed: &[(EntityId, u16)]) {
        #[cfg(feature = "profiling")]
        let _allocation_scope = crate::profiling::AllocationScope::new(196, "animation.originals");

        let changed: HashSet<_> = changed.iter().copied().collect();
        let mut controllers = std::mem::take(&mut self.system.state.controllers);
        for controller in controllers.values_mut() {
            for driver in &mut controller.drivers {
                let identity = driver.identity();
                if !changed.contains(&(identity.entity, identity.property.component())) {
                    continue;
                }
                #[cfg(feature = "skeletal-animation")]
                if crate::allocation_followup_enabled() && driver.skeleton_source().is_some() {
                    let _ = self.read().refresh_joint_original(driver.as_mut());
                    continue;
                }
                if let Some(value) = self.context.world.state.producer_value(
                    &self.context.world.components,
                    identity.entity,
                    identity.property.component(),
                ) && let Ok(original) = self
                    .read()
                    .read_bound_animation_target(driver.as_ref(), &value)
                {
                    let _ = driver.refresh_original(original);
                }
            }
            if let Some(source) = controller.transition_source_mut() {
                for driver in &mut source.drivers {
                    let identity = driver.identity();
                    if !changed.contains(&(identity.entity, identity.property.component())) {
                        continue;
                    }
                    if let Some(value) = self.context.world.state.producer_value(
                        &self.context.world.components,
                        identity.entity,
                        identity.property.component(),
                    ) && let Ok(original) = self
                        .read()
                        .read_bound_animation_target(driver.as_ref(), &value)
                    {
                        let _ = driver.refresh_original(original);
                    }
                }
            }
        }
        self.system.state.controllers = controllers;
    }

    pub(super) fn refresh_all_animation_originals(&mut self) {
        #[cfg(feature = "profiling")]
        let _allocation_scope = crate::profiling::AllocationScope::new(196, "animation.originals");
        let mut controllers = std::mem::take(&mut self.system.state.controllers);
        for controller in controllers.values_mut() {
            for driver in &mut controller.drivers {
                let identity = driver.identity();
                if let Some(property) = identity.property.property() {
                    if let Ok(original) = AnimationValue::read_fields(property, |offset| {
                        self.context.world.state.input_field(
                            &self.context.world.components,
                            identity.entity,
                            property.component,
                            offset,
                        )
                    }) {
                        let _ = driver.refresh_original(original);
                    }
                    continue;
                }
                #[cfg(feature = "skeletal-animation")]
                if crate::allocation_followup_enabled() && driver.skeleton_source().is_some() {
                    let _ = self.read().refresh_joint_original(driver.as_mut());
                    continue;
                }
                if let Some(value) = self.context.world.state.input_value(
                    &self.context.world.components,
                    identity.entity,
                    identity.property.component(),
                ) && let Ok(original) = self
                    .read()
                    .read_bound_animation_target(driver.as_ref(), &value)
                {
                    let _ = driver.refresh_original(original);
                }
            }
        }
        self.system.state.controllers = controllers;
    }

    pub(super) fn sync_description_demand(&mut self) {
        let state = &mut self.system.state;
        if crate::stress_optimizations_enabled() {
            if !state.description_demand_clean {
                let mut demand = BTreeSet::new();
                for controller in state.controllers.values() {
                    add_description_demand(&controller.snapshot.description, &mut demand);
                    if let Some(transition) = controller.transition.as_deref() {
                        let source = match &transition.source {
                            super::system_state::AnimationTransitionSource::Live(source) => {
                                source.as_ref()
                            }
                            super::system_state::AnimationTransitionSource::Frozen {
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
            return;
        }
        state.demand_revision = state.demand_revision.wrapping_add(1);
        state.animation_sources.retain(|selection| {
            selection.kind == ANIMATION_TYPE
                && state.controllers.values().any(|controller| {
                    controller
                        .snapshot
                        .description
                        .drivers
                        .iter()
                        .any(|driver| {
                            selection.source == driver.source && selection.variant == driver.variant
                        })
                })
        });
        for controller in state.controllers.values() {
            add_description_demand(
                &controller.snapshot.description,
                &mut state.animation_sources,
            );
            if let Some(transition) = controller.transition.as_deref() {
                let source = match &transition.source {
                    super::system_state::AnimationTransitionSource::Live(source) => source.as_ref(),
                    super::system_state::AnimationTransitionSource::Frozen {
                        bindings,
                        ..
                    } => bindings.as_ref(),
                };
                add_description_demand(&source.snapshot.description, &mut state.animation_sources);
            }
        }
    }
}

pub(super) fn validate_control(control: AnimationPlaybackControl) -> Result<(), ErrorReason> {
    if matches!(control, AnimationPlaybackControl::Seek(time) if !time.is_finite() || time < 0.0)
        || matches!(control, AnimationPlaybackControl::PlayAtSpeed(speed) if !speed.is_finite())
    {
        return Err(ErrorReason::InvalidValue);
    }
    Ok(())
}

pub(super) fn directional_start(speed: f32, duration: f64) -> f64 {
    if speed < 0.0 {
        duration
    } else {
        0.0
    }
}

pub(super) fn description_bytes(description: &AnimationControllerDescription) -> usize {
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
                    .capacity()
                    .saturating_add(driver.property.owned_bytes())
                    .saturating_mul(2),
            )
        },
    )
}

pub(super) fn add_description_demand(
    description: &AnimationControllerDescription,
    demand: &mut BTreeSet<AssetDemandSelection>,
) {
    for driver in &description.drivers {
        AssetDemandSelection::insert_into(demand, ANIMATION_TYPE, &driver.source, driver.variant);
    }
}
