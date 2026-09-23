//! Phase-scoped animation evaluation and sampled component updates.

use super::controller_commands::directional_start;
pub(super) use super::controller_commands::{description_bytes, validate_control};
use super::system_state::AnimationTransitionSource;
use super::{driver::AnimationTargetIdentity, *};
use crate::{
    ComponentValue,
    world::{WorldEntityState, WorldSimulationState},
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

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
        #[cfg(feature = "profiling")]
        let _measurement =
            crate::profiling::Stage::new(18 * 6, "profile.animation.apply_component");

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
        let reuse_demand = crate::allocation_optimizations_enabled();
        if self
            .system
            .state
            .controllers
            .values()
            .all(|controller| controller.snapshot.state == AnimationPlaybackStatus::Stopped)
        {
            // Inputs were restored before ingress. Withdraw evaluated source demand
            // without allocating target lookups or cloning transaction metadata.
            if reuse_demand {
                self.sync_description_demand();
                return;
            }
            self.system.state.description_demand_clean = false;
            self.system.state.demand_revision = self.system.state.demand_revision.wrapping_add(1);
            self.system.state.animation_sources = if self.system.state.controllers.is_empty() {
                BTreeSet::new()
            } else {
                self.read()
                    .controller_demand(None, None)
                    .unwrap_or_default()
            };
            return;
        }
        // Producer commits refresh sparse originals at their boundary. Joint
        // preparation below refreshes only bindings whose inputs changed.
        if !crate::animation_update_reuse_enabled() {
            if crate::allocation_optimizations_enabled() {
                self.refresh_all_animation_originals();
            } else {
                let changed: Vec<_> = self
                    .system
                    .state
                    .controllers
                    .values()
                    .flat_map(|controller| {
                        controller.drivers.iter().map(|driver| {
                            (
                                driver.identity().entity,
                                driver.identity().property.component(),
                            )
                        })
                    })
                    .collect();
                self.refresh_animation_originals(&changed);
            }
        }
        let mut controllers = std::mem::take(&mut self.system.state.controllers);

        // One temporary target lookup and one scan of existing active drivers.
        let mut baselines: HashMap<AnimationTargetIdentity, Option<AnimationValue>> =
            HashMap::new();
        let mut baseline_order = Vec::new();
        for controller in controllers.values() {
            if controller.snapshot.state != AnimationPlaybackStatus::Stopped
                && controller.drivers.is_empty()
            {
                for (description, (incarnation, property)) in controller
                    .snapshot
                    .description
                    .drivers
                    .iter()
                    .zip(&controller.incarnations)
                {
                    // Binding stops at the first unavailable source. Preserve the
                    // same ordered validation prefix without allocating originals
                    // for the rest of a controller on every loading frame.
                    if crate::stress_optimizations_enabled()
                        && !self
                            .read()
                            .source_key(description)
                            .is_some_and(|key| self.read().clip_ready(key) == Ok(true))
                    {
                        break;
                    }
                    let identity = AnimationTargetIdentity {
                        entity: description.target,
                        incarnation: *incarnation,
                        property: property.clone(),
                    };
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        baselines.entry(identity.clone())
                    {
                        baseline_order.push(identity);
                        entry.insert(None);
                    }
                }
            }
            if let Some(transition) = controller.transition.as_deref() {
                let source = match &transition.source {
                    AnimationTransitionSource::Live(source)
                        if source.snapshot.state != AnimationPlaybackStatus::Stopped =>
                    {
                        Some(source.as_ref())
                    }
                    AnimationTransitionSource::Frozen {
                        bindings,
                        ..
                    } => Some(bindings.as_ref()),
                    _ => None,
                };
                if let Some(source) = source
                    && source.drivers.is_empty()
                {
                    for (description, (incarnation, property)) in source
                        .snapshot
                        .description
                        .drivers
                        .iter()
                        .zip(&source.incarnations)
                    {
                        let identity = AnimationTargetIdentity {
                            entity: description.target,
                            incarnation: *incarnation,
                            property: property.clone(),
                        };
                        if let std::collections::hash_map::Entry::Vacant(entry) =
                            baselines.entry(identity.clone())
                        {
                            baseline_order.push(identity);
                            entry.insert(None);
                        }
                    }
                }
            }
        }
        let needs_baselines = !baselines.is_empty();
        for controller in controllers.values().filter(|_| needs_baselines) {
            for driver in &controller.drivers {
                if let Some(baseline) = baselines.get_mut(driver.identity())
                    && baseline.is_none()
                {
                    *baseline = Some(driver.original());
                }
            }
        }

        // A pending transition publishes its held source into component
        // storage, so a destination sharing those targets takes restoration
        // values from the source instead of capturing the held sample.
        for controller in controllers.values().filter(|_| needs_baselines) {
            let Some(transition) = controller.transition.as_deref() else {
                continue;
            };
            match &transition.source {
                AnimationTransitionSource::Live(source) => {
                    for driver in &source.drivers {
                        if let Some(baseline) = baselines.get_mut(driver.identity())
                            && baseline.is_none()
                        {
                            *baseline = Some(driver.original());
                        }
                    }
                }
                AnimationTransitionSource::Frozen {
                    values,
                    ..
                } => {
                    for value in values {
                        let identity = AnimationTargetIdentity {
                            entity: value.target,
                            incarnation: value.incarnation,
                            property: value.property.clone(),
                        };
                        if let Some(baseline) = baselines.get_mut(&identity)
                            && baseline.is_none()
                        {
                            *baseline = Some(value.baseline.clone());
                        }
                    }
                }
            }
        }

        for identity in &baseline_order {
            let baseline = baselines.get_mut(identity).unwrap();
            if baseline.is_none()
                && self
                    .read()
                    .animation_target_alive(identity, &self.context.world.state)
                && let Some(value) = self.context.world.state.input_value(
                    &self.context.world.components,
                    identity.entity,
                    identity.property.component(),
                )
            {
                *baseline = self
                    .read()
                    .read_animation_target(&identity.property, &value)
                    .ok();
            }
        }

        let mut advance_failures = BTreeMap::new();
        let mut ready = BTreeSet::new();
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
                            let Some(drivers) = self.read().bind_controller(source, &baselines)?
                            else {
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
                if controller.drivers.is_empty() {
                    let Some(drivers) = self.read().bind_controller(controller, &baselines)? else {
                        return Ok(());
                    };
                    controller.drivers = drivers;
                    controller.ready = false;
                    controller.reindex_drivers();
                }
                if !crate::compiled_animation_enabled() || !controller.ready {
                    if controller.drivers.iter().any(|driver| {
                        !self
                            .read()
                            .animation_binding_alive(driver.as_ref(), &self.context.world.state)
                    }) {
                        return Err(ErrorReason::MissingComponent);
                    }
                    let mut ready_clip = None;
                    for driver in &mut controller.drivers {
                        #[cfg(feature = "particles")]
                        if driver.identity().property.component()
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
                        if !crate::animation_update_reuse_enabled()
                            || ready_clip != Some(driver.clip())
                        {
                            if !self.read().clip_ready(driver.clip())? {
                                return Ok(());
                            }
                            ready_clip = Some(driver.clip());
                        }
                        if crate::compiled_animation_enabled() {
                            driver.resolve_track(
                                self.read()
                                    .clip_by_key(driver.clip())
                                    .ok_or(ErrorReason::InvalidAsset)?,
                            )?;
                        }
                        #[cfg(feature = "skeletal-animation")]
                        if let Some(source) = driver.skeleton_source() {
                            if !self
                                .context
                                .world
                                .components
                                .skeleton(driver.identity().entity.index() as usize)
                                .and_then(|value| value.runtime.pose.as_ref())
                                .is_some_and(|pose| pose.valid && pose.source == source)
                            {
                                return Ok(());
                            }
                            if crate::animation_update_reuse_enabled() {
                                self.read().refresh_joint_original(driver.as_mut())?;
                            }
                        }
                    }
                    controller.bind_numeric_targets(
                        &self.context.world.components,
                        &self.system.state.target_controllers,
                    );
                    controller.ready = true;
                }
                if crate::allocation_optimizations_enabled() {
                    ready_controllers.push(id);
                } else {
                    ready.insert(id);
                }
                let duration = if crate::animation_update_reuse_enabled() {
                    controller.duration
                } else {
                    controller
                        .drivers
                        .iter()
                        .filter_map(|driver| {
                            self.read()
                                .clip_by_key(driver.clip())
                                .map(AnimationClip::duration)
                        })
                        .fold(0.0, f64::max)
                };
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
                    runtime.hold_program = None;
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
                controller.sought = false;
                Ok(())
            })();
            if let Err(reason) = result {
                advance_failures.insert(id, reason);
                if reason == ErrorReason::MissingComponent {
                    controller.clear_drivers();
                    controller.transition = None;
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
        if !reuse_demand {
            self.system.state.description_demand_clean = false;
            self.system.state.demand_revision = self.system.state.demand_revision.wrapping_add(1);
            self.system.state.animation_sources = BTreeSet::new();
        }
        let mut failures = advance_failures;
        let mut controller_ids = std::mem::take(&mut self.system.state.controller_ids);
        controller_ids.clear();
        controller_ids.extend(controllers.keys().copied());
        self.system.state.controllers = controllers;
        #[cfg(feature = "skeletal-animation")]
        let mut sampled_joints = BTreeMap::<EntityId, BTreeSet<u32>>::new();
        for &id in &controller_ids {
            let mut controller = self.system.state.take_controller(id).unwrap();
            if controller
                .snapshot
                .transition
                .is_some_and(|transition| transition.pending)
            {
                let result = (|| {
                    let runtime = controller.transition.as_deref_mut().unwrap();
                    if let Some(program) = runtime.program.as_ref() {
                        self.context
                            .before_numeric_update(program.numeric_targets());
                        return program.write_composite_hold(&mut self.context.world.components);
                    }
                    if runtime.hold_program.is_none() {
                        runtime.hold_program = Some(match &mut runtime.source {
                            AnimationTransitionSource::Live(source) => {
                                let mut program =
                                    super::transition::AnimationTransitionProgram::bind_source(
                                        source,
                                        &self.context.world.components,
                                    )?;
                                program.prepare_source_hold(source)?;
                                program
                            }
                            AnimationTransitionSource::Frozen {
                                values,
                                ..
                            } => super::transition::AnimationTransitionProgram::bind_frozen_source(
                                values,
                                &self.context.world.components,
                            )?,
                        });
                    }
                    let program = runtime.hold_program.as_ref().unwrap();
                    self.context
                        .before_numeric_update(program.numeric_targets());
                    program.write_source_hold(&mut self.context.world.components)
                })();
                // A pending crossfade republishes its prepared held source after mutation
                // preparation, while all clocks and fade elapsed remain frozen.
                self.system.state.controllers.insert(id, controller);
                if let Err(reason) = result {
                    failures.insert(id, reason);
                }
                continue;
            }
            if controller.snapshot.state == AnimationPlaybackStatus::Stopped
                || controller.drivers.is_empty()
                || !(if crate::allocation_optimizations_enabled() {
                    ready_controllers.binary_search(&id).is_ok()
                } else {
                    ready.contains(&id)
                })
                || failures.contains_key(&id)
            {
                let restore = controller.retain_numeric;
                self.system.state.controllers.insert(id, controller);
                if restore {
                    // An advance failure or newly pending binding must withdraw
                    // the retained preceding sample before downstream evaluation.
                    self.restore_controller(id);
                }
                continue;
            }
            if !crate::compiled_animation_enabled()
                && controller.drivers.iter().any(|driver| {
                    !self
                        .read()
                        .animation_binding_alive(driver.as_ref(), &self.context.world.state)
                })
            {
                controller.clear_drivers();
                controller.transition = None;
                controller.snapshot.transition = None;
                controller.snapshot.state = AnimationPlaybackStatus::Stopped;
                self.event(
                    &controller,
                    AnimationPlaybackEventKind::Invalidated,
                    Some(ErrorReason::InvalidAsset),
                );
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
                    &controller,
                    progress,
                    &mut self.context.world.components,
                );
                let finished = result.is_ok() && summary.elapsed >= summary.duration;
                if finished {
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
            let (mut values, mut result) = self.sample_controller(
                &controller,
                #[cfg(feature = "skeletal-animation")]
                &mut sampled_joints,
            );

            // Sampling only writes numeric joint data or temporary public values.
            // Reinstall the current controller before those values can release
            // storage, so lifecycle callbacks see all affected bindings.
            self.system.state.controllers.insert(id, controller);
            let (properties, updates) = values.drain();
            for (key, value) in updates {
                let output = self.system.state.controllers[&id].numeric_output(key);
                result = result.and(self.apply_component_update(key, output, value, properties));
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

        if crate::allocation_optimizations_enabled() {
            self.system.state.controller_ids = controller_ids;
            self.system.state.ready_controllers = ready_controllers;
        }
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
        if reuse_demand {
            self.sync_description_demand();
        } else {
            let demand = self
                .read()
                .controller_demand(None, None)
                .unwrap_or_default();
            self.system.state.demand_revision = self.system.state.demand_revision.wrapping_add(1);
            self.system.state.animation_sources.extend(demand);
        }
    }
}

impl AnimationAccess<'_, '_> {
    pub(super) fn sample_controller(
        &mut self,
        controller: &AnimationController,
        #[cfg(feature = "skeletal-animation")] sampled_joints: &mut BTreeMap<
            EntityId,
            BTreeSet<u32>,
        >,
    ) -> (
        super::component_values::AnimationComponentValues,
        Result<(), ErrorReason>,
    ) {
        #[cfg(feature = "profiling")]
        let _allocation_scope = crate::profiling::AllocationScope::new(197, "animation.sample");
        #[cfg(feature = "profiling")]
        let _measurement =
            crate::profiling::Stage::new(16 * 6, "profile.animation.sample_and_stage");

        let mut values = super::component_values::AnimationComponentValues::new(
            std::mem::take(&mut self.system.state.component_scratch),
            std::mem::take(&mut self.system.state.property_scratch),
        );
        self.context
            .before_numeric_update(&controller.numeric_targets);
        let assets = &*self.context.asset_acquisition;
        let mut cached_clip = None;
        let mut resolve_clip = |key| -> Result<&AnimationClip, ErrorReason> {
            if crate::animation_update_reuse_enabled()
                && let Some((previous, clip)) = cached_clip
                && previous == key
            {
                return Ok(clip);
            }
            let clip = assets
                .get_typed::<AnimationClip>(key)
                .ok_or(ErrorReason::InvalidAsset)?;
            cached_clip = Some((key, clip));
            Ok(clip)
        };
        let mut sample = |driver: &dyn super::driver::AnimationDriverBinding, time, current| {
            if crate::compiled_animation_enabled() {
                driver.sample_bound(time, current)
            } else {
                driver.sample(resolve_clip(driver.clip())?, time, current)
            }
        };
        let result = (|| {
            for driver in &controller.drivers {
                if driver
                    .sample_numeric(controller.snapshot.time, &mut self.context.world.components)
                {
                    continue;
                }
                if crate::compiled_animation_enabled()
                    && driver.unchanged_discrete(controller.snapshot.time)
                {
                    continue;
                }
                let description = driver.description();
                if description.weight == 0.0 {
                    continue;
                }
                #[cfg(feature = "skeletal-animation")]
                if matches!(
                    driver.runtime_target(),
                    super::driver::AnimationRuntimeTarget::JointLocal { .. }
                ) {
                    if crate::allocation_followup_enabled() {
                        let (track, duration) = if crate::compiled_animation_enabled() {
                            (driver.bound_pose_track(), driver.duration())
                        } else {
                            let clip = assets
                                .get_typed::<AnimationClip>(driver.clip())
                                .ok_or(ErrorReason::InvalidAsset)?;
                            (
                                clip.typed_track::<Vec<crate::components::Transform>>(
                                    driver.description().track as usize,
                                )
                                .ok_or(ErrorReason::InvalidField)?,
                                clip.duration(),
                            )
                        };
                        super::pose::sample_joints(
                            driver.as_ref(),
                            track,
                            duration,
                            controller.snapshot.time,
                            &mut self.context.world.components,
                        )?;
                        continue;
                    }
                    let current = driver
                        .runtime_target()
                        .read_joints(&self.context.world.components, description.target)?;
                    let result = sample(driver.as_ref(), controller.snapshot.time, current)?;
                    driver.runtime_target().write_joints(
                        &mut self.context.world.components,
                        description.target,
                        result,
                    )?;
                    if let AnimationTrackTarget::Joints(joints) = &description.property {
                        sampled_joints
                            .entry(description.target)
                            .or_default()
                            .extend(joints);
                    }
                    continue;
                }
                let component = description.property.component();
                let key = (description.target, component);
                if crate::allocation_followup_enabled()
                    && let Some(property) = driver.identity().property.property()
                    && let [offset] = property.offsets.as_slice()
                    && let Some(current) =
                        values.numeric_current(key, *offset, &self.context.world.components)
                {
                    let result = sample(
                        driver.as_ref(),
                        controller.snapshot.time,
                        AnimationValue::Field(current),
                    )?;
                    let AnimationValue::Field(result) = result else {
                        return Err(ErrorReason::InvalidField);
                    };
                    values.set_numeric(key, *offset, result)?;
                    continue;
                }
                let current = if driver.discrete() {
                    driver.original()
                } else {
                    let value = values.get_or_insert(key, || {
                        self.context
                            .world
                            .components
                            .get(key.1, key.0.index() as usize)
                            .ok_or(ErrorReason::MissingComponent)
                    })?;
                    (AnimationReadAccess {
                        animation: &self.system.state,
                        world: self.context.world,
                        state: &self.context.world.state,
                        asset_acquisition: assets,
                    })
                    .read_bound_animation_target(driver.as_ref(), value)?
                };
                let result = sample(driver.as_ref(), controller.snapshot.time, current)?;
                if let AnimationValue::Field(crate::components::schema::FieldValue::Entity(entity)) =
                    &result
                    && !(entity.to_bits() == 0
                        && description.property.property().is_some_and(|property| {
                            property.offsets.len() == 1
                                && ComponentValue::accepts_null_entity(
                                    component,
                                    property.offsets[0],
                                )
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
                    && !(AnimationReadAccess {
                        animation: &self.system.state,
                        world: self.context.world,
                        state: &self.context.world.state,
                        asset_acquisition: assets,
                    })
                    .sampled_source_is_authorized(source, &description.source)
                {
                    return Err(ErrorReason::InvalidAsset);
                }
                if let AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(
                    crate::DynamicValue::Asset(asset),
                )) = &result
                    && !(AnimationReadAccess {
                        animation: &self.system.state,
                        world: self.context.world,
                        state: &self.context.world.state,
                        asset_acquisition: assets,
                    })
                    .sampled_source_is_authorized(&asset.uri, &description.source)
                {
                    return Err(ErrorReason::InvalidAsset);
                }
                if driver.discrete()
                    && let Some(property) = driver.identity().property.property()
                    && let [offset] = property.offsets.as_slice()
                    && let AnimationValue::Field(result) = result
                {
                    values.set_numeric(key, *offset, result)?;
                    continue;
                }
                let value = values.get_or_insert(key, || {
                    self.context
                        .world
                        .components
                        .get(key.1, key.0.index() as usize)
                        .ok_or(ErrorReason::MissingComponent)
                })?;
                result.write(
                    driver
                        .identity()
                        .property
                        .property()
                        .ok_or(ErrorReason::InvalidField)?,
                    value,
                )?;
                #[cfg(feature = "skeletal-animation")]
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
                        sampled_joints.get(&description.target),
                    )?;
                }
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
}
