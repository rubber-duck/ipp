//! Phase-scoped animation evaluation and sampled component updates.

use super::controller_commands::directional_start;
pub(super) use super::controller_commands::{description_bytes, validate_control};
use super::system_state::AnimationTransitionSource;
use super::*;
use crate::{
    ComponentValue,
    world::{WorldEntityState, WorldSimulationState},
};
use std::collections::BTreeMap;

fn advance_transition_source(
    controller: &mut AnimationController,
    dt: f64,
) -> Result<(), ErrorReason> {
    if controller.snapshot.state == AnimationPlaybackStatus::Stopped {
        return Ok(());
    }
    let advance = dt * f64::from(controller.snapshot.description.speed);
    if !advance.is_finite() {
        return Err(ErrorReason::InvalidValue);
    }
    if controller.snapshot.description.looping {
        controller.snapshot.time = if controller.duration == 0.0 {
            0.0
        } else {
            (controller.snapshot.time + advance.rem_euclid(controller.duration))
                .rem_euclid(controller.duration)
        };
    } else {
        controller.snapshot.time =
            (controller.snapshot.time + advance).clamp(0.0, controller.duration);
    }
    Ok(())
}

pub(in crate::world) struct AnimationAccess<'a, 'world> {
    pub system: &'a mut AnimationSystem,
    pub context: &'a mut crate::systems::SystemRuntimeAccess<'world>,
}

pub(in crate::world) struct AnimationReadAccess<'a> {
    pub animation: &'a AnimationSystemState,
    pub world: &'a WorldSimulationState,
    pub state: &'a WorldEntityState,
    pub asset_acquisition: &'a crate::services::asset_management::AssetManagementService,
}

impl AnimationAccess<'_, '_> {
    pub(super) fn apply_sampled_value(
        &mut self,
        entity: EntityId,
        value: ComponentValue,
    ) -> Result<(), ErrorReason> {
        let before = self.system.state.animation_sources.len();
        value.resource_demand(&mut self.system.state.animation_sources);
        if self.system.state.animation_sources.len() != before {
            self.system.state.demand_revision = self.system.state.demand_revision.wrapping_add(1);
            self.system.state.description_demand_clean = false;
        }
        self.context
            .apply_evaluated_value(self.system, entity, value)
    }

    pub(super) fn apply_component_update(
        &mut self,
        key: (EntityId, u16),
        output: Option<super::numeric_output::AnimationNumericOutput>,
        value: Option<ComponentValue>,
        properties: &super::component_values::PropertyScratch,
    ) -> Result<(), ErrorReason> {
        #[cfg(feature = "instrumentation")]
        let _measurement =
            crate::profiling::Stage::fixed(crate::profiling::FixedStage::AnimationApplyComponent);

        let resource_patch = value.is_none()
            && properties.iter().any(|((target, _), value)| {
                *target == key
                    && matches!(
                        value,
                        crate::components::schema::FieldValue::String(_)
                            | crate::components::schema::FieldValue::Bytes(_)
                            | crate::components::schema::FieldValue::Entity(_)
                            | crate::components::schema::FieldValue::Dynamic(
                                crate::DynamicValue::Asset(_)
                            )
                    )
            });
        if resource_patch {
            let mut component = self
                .context
                .world
                .components
                .get(key.1, key.0.index() as usize)
                .ok_or(ErrorReason::MissingComponent)?;
            for ((target, offset), property) in properties {
                if *target == key {
                    component
                        .set_field(*offset, property.clone())
                        .map_err(|_| ErrorReason::InvalidField)?;
                }
            }
            self.system.state.description_demand_clean = false;
            return self.apply_sampled_value(key.0, component);
        }
        match value {
            Some(value)
                if output.is_some()
                    && !super::numeric_output::AnimationNumericOutput::patch_only(key.1) =>
            {
                output
                    .unwrap()
                    .write(value, &mut self.context.world.components)
            }
            Some(value) => self.apply_sampled_value(key.0, value),
            None if output.is_some() => output.unwrap().write_properties(
                key,
                properties,
                &mut self.context.world.components,
            ),
            None => self.context.apply_evaluated_properties(
                self.system,
                key.0,
                key.1,
                properties
                    .iter()
                    .filter(|((target, _), _)| *target == key)
                    .map(|((_, offset), value)| (*offset, value.clone())),
            ),
        }
    }

    pub(super) fn read(&self) -> AnimationReadAccess<'_> {
        AnimationReadAccess {
            animation: &self.system.state,
            world: self.context.world,
            state: &self.context.world.state,
            asset_acquisition: self.context.asset_acquisition,
        }
    }
}

impl AnimationAccess<'_, '_> {
    pub(in crate::world) fn evaluate_animation(&mut self, dt: f64) {
        if self
            .system
            .state
            .controllers
            .values()
            .all(|controller| controller.snapshot.state == AnimationPlaybackStatus::Stopped)
        {
            // Stopped controllers withdrew their contributions. Withdraw evaluated
            // source demand without allocating target lookups.
            self.sync_description_demand();
            return;
        }
        let mut controllers = std::mem::take(&mut self.system.state.controllers);

        let mut advance_failures = BTreeMap::new();
        let mut invalidated = Vec::new();
        let mut ready_controllers = std::mem::take(&mut self.system.state.ready_controllers);
        ready_controllers.clear();
        for (&id, controller) in &mut controllers {
            if controller.snapshot.state == AnimationPlaybackStatus::Stopped {
                continue;
            }
            let result = (|| {
                if let Some(transition) = controller.transition.as_mut() {
                    let source = match &mut transition.source {
                        AnimationTransitionSource::Live(source)
                            if source.snapshot.state != AnimationPlaybackStatus::Stopped =>
                        {
                            Some(source.as_mut())
                        }
                        AnimationTransitionSource::Frozen {
                            ..
                        } => None,
                        _ => None,
                    };
                    if let Some(source) = source {
                        if source.drivers.is_empty() {
                            let Some(drivers) = self.read().bind_controller(source)? else {
                                return Ok(());
                            };
                            source.drivers = drivers;
                            source.reindex_drivers();
                            for driver in &mut source.drivers {
                                if !self.read().clip_ready(driver.clip())? {
                                    return Ok(());
                                }
                                driver.resolve_track(
                                    self.read()
                                        .clip_by_key(driver.clip())
                                        .ok_or(ErrorReason::InvalidAsset)?,
                                )?;
                            }
                            source.ready = true;
                        }
                        if !source.ready {
                            for driver in &mut source.drivers {
                                if !self.read().clip_ready(driver.clip())? {
                                    return Ok(());
                                }
                                driver.resolve_track(
                                    self.read()
                                        .clip_by_key(driver.clip())
                                        .ok_or(ErrorReason::InvalidAsset)?,
                                )?;
                            }
                            source.ready = true;
                        }
                    }
                }
                if controller.drivers.is_empty() && controller.structural_drivers.is_empty() {
                    if !self.structural_sources_ready(controller)? {
                        return Ok(());
                    }
                    let Some(drivers) = self.read().bind_controller(controller)? else {
                        return Ok(());
                    };
                    self.bind_structural_drivers(controller)?;
                    controller.drivers = drivers;
                    controller.ready = false;
                    controller.reindex_drivers();
                }
                if !controller.ready {
                    if !self.structural_sources_ready(controller)? {
                        return Ok(());
                    }
                    self.prepare_structural_drivers(controller)?;
                    if controller.drivers.iter().any(|driver| {
                        !self
                            .read()
                            .animation_binding_alive(driver.as_ref(), &self.context.world.state)
                    }) {
                        return Err(ErrorReason::MissingComponent);
                    }
                    let mut ready_clip = None;
                    for driver in &mut controller.drivers {
                        if driver.identity().property.component_target()
                            == crate::ComponentValue::PARTICLE_PLAYBACK
                        {
                            let playback = self
                                .context
                                .world
                                .components
                                .particle_playback(driver.identity().entity.index() as usize)
                                .ok_or(ErrorReason::MissingComponent)?;
                            let key = crate::systems::asset_dependencies::source_key_from_fields(
                                self.context.asset_acquisition,
                                self.context.world.id,
                                None,
                                crate::systems::particles::PARTICLE_CACHE_TYPE,
                                &playback.source,
                                playback.variant,
                            );
                            if key
                                .and_then(|key| self.context.asset_acquisition.get(key)?.data())
                                .is_none()
                            {
                                return Ok(());
                            }
                        }
                        if ready_clip != Some(driver.clip()) {
                            if !self.read().clip_ready(driver.clip())? {
                                return Ok(());
                            }
                            ready_clip = Some(driver.clip());
                        }
                        driver.resolve_track(
                            self.read()
                                .clip_by_key(driver.clip())
                                .ok_or(ErrorReason::InvalidAsset)?,
                        )?;
                        if let Some(source) = driver.skeleton_source()
                            && !self
                                .context
                                .world
                                .components
                                .skeleton(driver.identity().entity.index() as usize)
                                .and_then(|value| value.runtime.pose.as_ref())
                                .is_some_and(|pose| pose.valid && pose.source == source)
                        {
                            return Ok(());
                        }
                    }
                    controller.bind_numeric_targets(&self.context.world.components);
                    controller.ready = true;
                }
                ready_controllers.push(id);
                let duration = controller.duration;
                if controller.directional_start_pending {
                    controller.snapshot.time =
                        directional_start(controller.snapshot.description.speed, duration);
                    controller.directional_start_pending = false;
                    controller.sought = true;
                }
                if controller.snapshot.time > duration {
                    if controller.snapshot.transition.is_none() {
                        return Err(ErrorReason::InvalidValue);
                    }
                    controller.snapshot.time = duration;
                }
                if controller
                    .snapshot
                    .transition
                    .is_some_and(|summary| summary.pending)
                {
                    let mut runtime = controller.transition.take().expect("transition runtime");
                    if let AnimationTransitionSource::Live(source) = &runtime.source
                        && source.snapshot.state != AnimationPlaybackStatus::Stopped
                        && (source.drivers.is_empty() || !source.ready)
                    {
                        controller.transition = Some(runtime);
                        return Ok(());
                    }
                    let prepare = (|| -> Result<_, ErrorReason> {
                        let program = match &mut runtime.source {
                            AnimationTransitionSource::Frozen {
                                values,
                                bindings,
                                prepared,
                                ..
                            } => {
                                if let Some(program) = prepared.take() {
                                    program.retarget_frozen(
                                        controller,
                                        values,
                                        &self.context.world.components,
                                    )?
                                } else {
                                    super::transition::AnimationTransitionProgram::bind_frozen(
                                        bindings,
                                        controller,
                                        values,
                                        &self.context.world.components,
                                    )?
                                }
                            }
                            AnimationTransitionSource::Live(source) => {
                                let source =
                                    if source.snapshot.state == AnimationPlaybackStatus::Stopped {
                                        None
                                    } else {
                                        Some(source.as_mut())
                                    };
                                super::transition::AnimationTransitionProgram::bind(
                                    source,
                                    controller,
                                    &self.context.world.components,
                                )?
                            }
                        };
                        let (source_time, source_duration) = match &runtime.source {
                            AnimationTransitionSource::Live(source) => {
                                (source.snapshot.time, source.duration)
                            }
                            AnimationTransitionSource::Frozen {
                                reference_time,
                                reference_duration,
                                ..
                            } => (*reference_time, *reference_duration),
                        };
                        let time = match runtime.start_time {
                            AnimationTransitionStartTime::Restart => {
                                directional_start(controller.snapshot.description.speed, duration)
                            }
                            AnimationTransitionStartTime::Preserve => {
                                source_time.clamp(0.0, duration)
                            }
                            AnimationTransitionStartTime::MatchPhase => {
                                if source_duration == 0.0 {
                                    directional_start(
                                        controller.snapshot.description.speed,
                                        duration,
                                    )
                                } else {
                                    (source_time / source_duration).clamp(0.0, 1.0) * duration
                                }
                            }
                            AnimationTransitionStartTime::Seek(time) => {
                                if time > duration {
                                    return Err(ErrorReason::InvalidValue);
                                }
                                time
                            }
                        };
                        Ok((program, time))
                    })();
                    controller.transition = Some(runtime);
                    let (program, time) = prepare?;
                    let runtime = controller.transition.as_mut().unwrap();
                    runtime.program = Some(program);
                    controller.snapshot.time = time;
                    let summary = controller.snapshot.transition.as_mut().unwrap();
                    summary.pending = false;
                    controller.sought = true;
                }
                if controller.snapshot.state == AnimationPlaybackStatus::Playing
                    && !controller.sought
                {
                    let advance = dt * f64::from(controller.snapshot.description.speed);
                    if !advance.is_finite() {
                        return Err(ErrorReason::InvalidValue);
                    }
                    if controller.snapshot.description.looping && advance != 0.0 {
                        let crossed_boundary = if advance > 0.0 {
                            controller.snapshot.time + advance >= duration
                        } else {
                            controller.snapshot.time + advance < 0.0
                        };
                        if crossed_boundary {
                            for driver in &mut controller.structural_drivers {
                                driver.reset_selection();
                            }
                        }
                        controller.snapshot.time = if duration == 0.0 {
                            0.0
                        } else {
                            (controller.snapshot.time + advance.rem_euclid(duration))
                                .rem_euclid(duration)
                        };
                    } else if !controller.snapshot.description.looping {
                        controller.snapshot.time =
                            (controller.snapshot.time + advance).clamp(0.0, duration);
                        if controller.snapshot.transition.is_none()
                            && ((advance > 0.0 && controller.snapshot.time == duration)
                                || (advance < 0.0 && controller.snapshot.time == 0.0))
                        {
                            controller.snapshot.state = AnimationPlaybackStatus::Completed;
                            self.event(controller, AnimationPlaybackEventKind::Completed, None);
                        }
                    }
                    if let Some(runtime) = &mut controller.transition {
                        if let AnimationTransitionSource::Live(source) = &mut runtime.source {
                            advance_transition_source(source, dt)?;
                        }
                        let summary = controller.snapshot.transition.as_mut().unwrap();
                        summary.elapsed = (summary.elapsed + dt).min(summary.duration);
                    }
                }
                if controller.sought {
                    for driver in &mut controller.structural_drivers {
                        driver.reset_selection();
                    }
                }
                controller.sought = false;
                Ok(())
            })();
            if let Err(reason) = result {
                advance_failures.insert(id, reason);
                if reason == ErrorReason::MissingComponent {
                    // The controller withdraws its contributions once every
                    // controller is installed for lifecycle callbacks.
                    invalidated.push(id);
                    controller.snapshot.transition = None;
                    controller.snapshot.state = AnimationPlaybackStatus::Stopped;
                    self.event(
                        controller,
                        AnimationPlaybackEventKind::Invalidated,
                        Some(reason),
                    );
                }
            }
        }

        // Resource-free drivers retain the known clip set until final synchronization.
        let mut failures = advance_failures;
        let mut controller_ids = std::mem::take(&mut self.system.state.controller_ids);
        controller_ids.clear();
        controller_ids.extend(controllers.keys().copied());
        self.system.state.controllers = controllers;
        for id in invalidated {
            self.withdraw_controller(id);
            let controller = self.system.state.controllers.get_mut(&id).unwrap();
            let structural = std::mem::take(&mut controller.structural_drivers);
            controller.clear_drivers();
            controller.transition = None;
            self.release_structural_drivers(structural);
        }
        for &id in &controller_ids {
            let mut controller = self.system.state.take_controller(id).unwrap();
            // A pending crossfade writes nothing while its clocks and fade
            // progress are frozen; its contributions stay in their fields.
            if controller
                .snapshot
                .transition
                .is_some_and(|transition| transition.pending)
            {
                self.system.state.controllers.insert(id, controller);
                continue;
            }
            // A failing or pending controller stops contributing; what it applied
            // stays in its fields until it stops.
            if controller.snapshot.state == AnimationPlaybackStatus::Stopped
                || (controller.drivers.is_empty() && controller.structural_drivers.is_empty())
                || ready_controllers.binary_search(&id).is_err()
                || failures.contains_key(&id)
            {
                self.system.state.controllers.insert(id, controller);
                continue;
            }
            if controller.transition.is_some() {
                let mut runtime = controller.transition.take().unwrap();
                let summary = controller.snapshot.transition.unwrap();
                let raw_progress = if summary.duration == 0.0 {
                    1.0
                } else {
                    (summary.elapsed / summary.duration).clamp(0.0, 1.0)
                };
                let progress = match summary.easing {
                    AnimationTransitionEasing::Linear => raw_progress,
                    AnimationTransitionEasing::Smoothstep => {
                        raw_progress * raw_progress * (3.0 - 2.0 * raw_progress)
                    }
                };
                let source = match &runtime.source {
                    AnimationTransitionSource::Live(source)
                        if source.snapshot.state != AnimationPlaybackStatus::Stopped =>
                    {
                        Some(source.as_ref())
                    }
                    AnimationTransitionSource::Live(_) => None,
                    AnimationTransitionSource::Frozen {
                        bindings,
                        ..
                    } => Some(bindings.as_ref()),
                };
                let program = runtime.program.as_mut().expect("ready transition program");
                self.context
                    .before_numeric_update(program.numeric_targets());
                let result = program.evaluate(
                    source,
                    &controller.drivers,
                    controller.snapshot.time,
                    &mut controller.contributions,
                    progress,
                    &mut self.context.world.components,
                );
                let finished = result.is_ok() && summary.elapsed >= summary.duration;
                if finished {
                    // Fields only the outgoing side drove hold nothing of it now.
                    controller.contributions.prune_empty();
                    controller.snapshot.transition = None;
                    let at_endpoint = if controller.snapshot.description.speed < 0.0 {
                        controller.snapshot.time == 0.0
                    } else {
                        controller.snapshot.time == controller.duration
                    };
                    if !controller.snapshot.description.looping && at_endpoint {
                        controller.snapshot.state = AnimationPlaybackStatus::Completed;
                        self.event(&controller, AnimationPlaybackEventKind::Completed, None);
                    }
                } else {
                    controller.transition = Some(runtime);
                }
                self.system.state.controllers.insert(id, controller);
                if finished {
                    self.system.state.index_controller(id);
                    self.system.state.description_demand_clean = false;
                }
                if let Err(reason) = result {
                    failures.insert(id, reason);
                }
                continue;
            }
            if !controller.contributions.prepared() {
                controller.contributions.prepare(&controller.drivers);
            }
            // Absolute writers go first: structural placements, then this
            // controller's absolute fields and its contributions.
            let mut result = self.sample_structural_drivers(&mut controller);
            let (mut values, sampled) = self.sample_controller(&mut controller);
            result = result.and(sampled);

            // Sampling only writes numeric joint data or temporary public values.
            // Reinstall the current controller before those values can release
            // storage, so lifecycle callbacks see all affected bindings.
            self.system.state.controllers.insert(id, controller);
            let (properties, updates) = values.drain();
            for (key, value) in updates {
                let output = self.system.state.controllers[&id].numeric_output(key);
                let written = self.apply_component_update(key, output, value, properties);
                // A contribution counts as applied only once its write landed; a
                // rejected write is retried with the same change next frame.
                if written.is_ok()
                    && let Some(controller) = self.system.state.controllers.get_mut(&id)
                {
                    controller.contributions.commit(key);
                }
                result = result.and(written);
            }
            (
                self.system.state.component_scratch,
                self.system.state.property_scratch,
            ) = values.into_scratch();
            if result.is_ok() {
                let controller = &self.system.state.controllers[&id];
                for &index in &controller.discrete_drivers {
                    controller.drivers[index].mark_discrete(controller.snapshot.time);
                }
            }
            if let Err(reason) = result {
                failures.insert(id, reason);
            }
        }

        self.system.state.controller_ids = controller_ids;
        self.system.state.ready_controllers = ready_controllers;
        let mut controllers = std::mem::take(&mut self.system.state.controllers);
        for (&id, controller) in &mut controllers {
            let failure = failures.get(&id).copied();
            if let Some(reason) = failure
                && controller.failure != failure
                && controller.snapshot.state != AnimationPlaybackStatus::Stopped
            {
                self.event(controller, AnimationPlaybackEventKind::Failed, Some(reason));
            }
            controller.failure = failure;
        }
        self.system.state.controllers = controllers;
        self.sync_description_demand();
    }
}

impl AnimationAccess<'_, '_> {
    /// Stage one frame of a ready controller: absolute fields take their sample,
    /// each contributed field moves by the change of this controller's total, and
    /// joint contributions apply onto the Skeleton's rebuilt pose.
    pub(super) fn sample_controller(
        &mut self,
        controller: &mut AnimationController,
    ) -> (
        super::component_values::AnimationComponentValues,
        Result<(), ErrorReason>,
    ) {
        #[cfg(feature = "instrumentation")]
        let _allocation_scope = crate::profiling::AllocationScope::new(197, "animation.sample");
        #[cfg(feature = "instrumentation")]
        let _measurement =
            crate::profiling::Stage::fixed(crate::profiling::FixedStage::AnimationSampleAndStage);

        let mut values = super::component_values::AnimationComponentValues::new(
            std::mem::take(&mut self.system.state.component_scratch),
            std::mem::take(&mut self.system.state.property_scratch),
        );
        self.context
            .before_numeric_update(&controller.numeric_targets);
        let time = controller.snapshot.time;
        let result = (|| {
            controller.contributions.begin();
            for (index, driver) in controller.drivers.iter().enumerate() {
                if matches!(
                    driver.runtime_target(),
                    super::driver::AnimationRuntimeTarget::JointLocal { .. }
                ) {
                    super::pose::sample_joints(
                        driver.as_ref(),
                        driver.bound_pose_track(),
                        driver.duration(),
                        time,
                        &mut self.context.world.components,
                    )?;
                    continue;
                }
                if driver.contributes() {
                    controller
                        .contributions
                        .add(index, &driver.contribution(time)?)?;
                    continue;
                }
                if driver.unchanged_discrete(time) || driver.description().weight == 0.0 {
                    continue;
                }
                self.stage_absolute(driver.as_ref(), time, &mut values)?;
            }
            for index in 0..controller.contributions.entries().len() {
                if !controller.contributions.moves(index) {
                    continue;
                }
                let (before, after) = {
                    let contributions = &controller.contributions;
                    let identity = contributions.identity(index);
                    let property = identity
                        .property
                        .property()
                        .ok_or(ErrorReason::InvalidField)?;
                    let key = (identity.entity, property.component);
                    if let [offset] = property.offsets.as_slice()
                        && let Some(current) =
                            values.numeric_current(key, *offset, &self.context.world.components)
                    {
                        let current = AnimationValue::Field(current);
                        let next = contributions.moved(index, &current)?;
                        let AnimationValue::Field(field) = next.clone() else {
                            return Err(ErrorReason::InvalidField);
                        };
                        values.set_numeric(key, *offset, field)?;
                        (current, next)
                    } else {
                        let value = values.get_or_insert(key, || {
                            self.context
                                .world
                                .components
                                .get(key.1, key.0.index() as usize)
                                .ok_or(ErrorReason::MissingComponent)
                        })?;
                        let current = AnimationValue::read(property, value)?;
                        let next = contributions.moved(index, &current)?;
                        next.write(property, value)?;
                        (current, next)
                    }
                };
                controller.contributions.stage(index, before, after);
            }
            Ok(())
        })();
        for value in values.values_mut() {
            if let ComponentValue::Transform(transform) = value {
                let q = [transform.qx, transform.qy, transform.qz, transform.qw];
                [transform.qx, transform.qy, transform.qz, transform.qw] = normalize(q);
            }
        }
        (values, result)
    }

    /// Stage the sample of a driver of a field without a delta.
    fn stage_absolute(
        &mut self,
        driver: &dyn super::driver::AnimationDriverBinding,
        time: f64,
        values: &mut super::component_values::AnimationComponentValues,
    ) -> Result<(), ErrorReason> {
        let description = driver.description();
        let component = description.property.component_target();
        let key = (description.target, component);
        let result = driver.sample(time);
        if let AnimationValue::Field(crate::components::schema::FieldValue::Entity(entity)) =
            &result
            && !(entity.to_bits() == 0
                && description.property.property().is_some_and(|property| {
                    property.offsets.len() == 1
                        && ComponentValue::accepts_null_entity(component, property.offsets[0])
                }))
            && !self.context.world.state.entities.contains_key(entity)
        {
            return Err(ErrorReason::InvalidEntity);
        }
        // Source hints belong to interchange only. Check the actual destination.
        // A restored producer clip preserves its original namespace; external clips
        // can introduce producer references only within the current World.
        if let AnimationValue::Field(crate::components::schema::FieldValue::String(source)) =
            &result
            && let Some(property) = description.property.property()
            && ComponentValue::asset_references(component)
                .iter()
                .any(|reference| property.offsets == [reference.source_offset])
            && !self
                .read()
                .sampled_source_is_authorized(source, &description.source)
        {
            return Err(ErrorReason::InvalidAsset);
        }
        if let AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(
            crate::DynamicValue::Asset(asset),
        )) = &result
            && !self
                .read()
                .sampled_source_is_authorized(&asset.uri, &description.source)
        {
            return Err(ErrorReason::InvalidAsset);
        }
        let property = driver
            .identity()
            .property
            .property()
            .ok_or(ErrorReason::InvalidField)?;
        if driver.discrete()
            && let [offset] = property.offsets.as_slice()
            && let AnimationValue::Field(result) = result
        {
            return values.set_numeric(key, *offset, result);
        }
        let value = values.get_or_insert(key, || {
            self.context
                .world
                .components
                .get(key.1, key.0.index() as usize)
                .ok_or(ErrorReason::MissingComponent)
        })?;
        result.write(property, value)?;
        if let ComponentValue::Skeleton(sampled) = value {
            let current = self
                .context
                .world
                .components
                .skeleton_mut(description.target.index() as usize)
                .ok_or(ErrorReason::MissingComponent)?;
            crate::systems::skeleton::rebase_sampled_inputs(
                current,
                sampled,
                self.context.asset_acquisition,
                self.context.world.id,
            )?;
        }
        Ok(())
    }
}
