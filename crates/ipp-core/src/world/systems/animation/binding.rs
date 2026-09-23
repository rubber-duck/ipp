use super::controller_commands::add_description_demand;
use super::driver::{AnimationTargetIdentity, make_driver};
use super::*;
use crate::world::WorldMutationState;
use crate::{
    ComponentValue,
    services::asset_management::{AssetKey, service::AssetDemandSelection},
    world::WorldEntityState,
};
use std::collections::{BTreeSet, HashMap};

impl<'a> AnimationReadAccess<'a> {
    pub(super) fn asset_resources(
        &self,
    ) -> &'a crate::services::asset_management::AssetManagementService {
        self.asset_acquisition
    }

    #[cfg(feature = "skeletal-animation")]
    pub(super) fn source_data<T: 'static>(
        &self,
        kind: crate::services::asset_management::AssetTypeId,
        source: &str,
        variant: u32,
    ) -> Option<(AssetKey, &'a T)> {
        self.asset_acquisition
            .source_data(self.world.id, kind, source, variant)
    }

    pub(super) fn sampled_source_is_authorized(&self, source: &str, clip_source: &str) -> bool {
        fn producer_namespace(source: &str) -> Option<&str> {
            source
                .strip_prefix("producer://")?
                .split_once('/')
                .map(|(world, _)| world)
        }

        if !source.starts_with("producer://") {
            return true;
        }
        producer_namespace(source).is_some_and(|world| {
            !world.is_empty()
                && (world.parse::<u64>() == Ok(self.world.id.0)
                    || producer_namespace(clip_source) == Some(world))
        })
    }

    pub(super) fn source_key(&self, description: &AnimationDriverDescription) -> Option<AssetKey> {
        if crate::allocation_optimizations_enabled() {
            return self.asset_resources().find_source(
                self.world.id,
                ANIMATION_TYPE,
                &description.source,
                description.variant,
            );
        }
        let source =
            crate::services::asset_management::service::AssetManagementService::scoped_selection(
                self.world.id,
                &AssetDemandSelection::new(
                    ANIMATION_TYPE,
                    &description.source,
                    description.variant,
                ),
            )
            .descriptor();
        self.asset_resources().find(&source)
    }

    pub(super) fn clip_by_key(&self, key: AssetKey) -> Option<&'a AnimationClip> {
        self.asset_resources().get_typed::<AnimationClip>(key)
    }

    pub(super) fn clip_ready(&self, key: AssetKey) -> Result<bool, ErrorReason> {
        let provider = self
            .asset_resources()
            .get(key)
            .ok_or(ErrorReason::MissingComponent)?;
        if matches!(
            provider.status(),
            crate::services::asset_management::AssetLoadStatus::Failed(_)
        ) {
            return Err(ErrorReason::InvalidAsset);
        }
        if provider.data().is_some() && self.clip_by_key(key).is_none() {
            return Err(ErrorReason::InvalidAsset);
        }
        Ok(self.clip_by_key(key).is_some())
    }

    pub(super) fn validate_animation_demand(
        &self,
        demand: &BTreeSet<AssetDemandSelection>,
    ) -> Result<(), ErrorReason> {
        self.asset_acquisition
            .validate_additional_users(self.world.id, demand.iter().cloned())
            .map_err(|_| ErrorReason::Capacity)
    }

    pub(super) fn controller_demand(
        &self,
        replace: Option<(AnimationControllerId, &AnimationControllerDescription)>,
        remove: Option<AnimationControllerId>,
    ) -> Result<BTreeSet<AssetDemandSelection>, ErrorReason> {
        #[cfg(feature = "profiling")]
        let _allocation_scope = crate::profiling::AllocationScope::new(198, "animation.demand");

        let mut demand = BTreeSet::new();
        let mut count = usize::from(replace.is_some());
        for (&id, controller) in &self.animation.controllers {
            if Some(id) != remove && replace.is_none_or(|(other, _)| other != id) {
                count += 1;
                add_description_demand(&controller.snapshot.description, &mut demand);
            }
        }
        if count > MAX_CONTROLLERS {
            return Err(ErrorReason::Capacity);
        }
        if let Some((_, description)) = replace {
            add_description_demand(description, &mut demand);
        }
        #[cfg(debug_assertions)]
        self.validate_animation_demand(&demand)?;
        Ok(demand)
    }

    pub(super) fn resolve_animation_property(
        &self,
        description: &AnimationDriverDescription,
    ) -> Result<AnimationTrackTarget, ErrorReason> {
        match &description.property {
            AnimationTrackTarget::DynamicProperty {
                component,
                name,
            } => {
                let value = self
                    .state
                    .input_value(&self.world.components, description.target, *component)
                    .ok_or(ErrorReason::MissingComponent)?;
                let offset = value
                    .dynamic_properties()
                    .and_then(|p| p.key(name))
                    .ok_or(ErrorReason::InvalidField)?;
                Ok(AnimationTrackTarget::AnimationProperty(AnimationProperty {
                    component: *component,
                    offsets: vec![offset],
                }))
            }
            property => Ok(property.clone()),
        }
    }

    pub(super) fn validate_controller_description(
        &self,
        description: &AnimationControllerDescription,
        time: f64,
    ) -> Result<Vec<(u64, AnimationTrackTarget)>, ErrorReason> {
        if description.drivers.is_empty() || !description.speed.is_finite() {
            return Err(ErrorReason::InvalidValue);
        }
        let mut incarnations = Vec::with_capacity(description.drivers.len());
        let mut duration: f64 = 0.0;
        let mut all_ready = true;
        for driver in &description.drivers {
            crate::services::asset_management::service::validate_source(&driver.source)?;
            if driver.source.is_empty()
                || !driver.weight.is_finite()
                || !(0.0..=1.0).contains(&driver.weight)
                || !driver.reference_time.is_finite()
                || driver.reference_time < 0.0
            {
                return Err(ErrorReason::InvalidValue);
            }
            let record = self
                .state
                .entities
                .get(&driver.target)
                .ok_or(ErrorReason::InvalidEntity)?;
            let input = record
                .input(driver.property.component())
                .ok_or(ErrorReason::MissingComponent)?;
            let value = self
                .state
                .input_value(
                    &self.world.components,
                    driver.target,
                    driver.property.component(),
                )
                .ok_or(ErrorReason::MissingComponent)?;
            let current = match self.read_animation_target(&driver.property, &value) {
                Ok(value) => Some(value),
                #[cfg(feature = "skeletal-animation")]
                Err(ErrorReason::InvalidAsset)
                    if matches!(driver.property, AnimationTrackTarget::Joints(_)) =>
                {
                    None
                }
                Err(reason) => return Err(reason),
            };
            if driver.additive && current.as_ref().is_some_and(|current| !current.numeric()) {
                return Err(ErrorReason::InvalidField);
            }
            if let Some(clip) = self
                .source_key(driver)
                .and_then(|key| self.clip_by_key(key))
            {
                let track = clip
                    .tracks()
                    .get(driver.track as usize)
                    .ok_or(ErrorReason::InvalidField)?;
                if current
                    .as_ref()
                    .is_some_and(|v| !v.same_type(&track.sample_value(0.0)))
                    || driver.property.indices().len() != track.target().indices().len()
                {
                    return Err(ErrorReason::InvalidField);
                }
                if f64::from(driver.reference_time) > clip.duration() {
                    return Err(ErrorReason::InvalidValue);
                }
                duration = duration.max(clip.duration());
            } else {
                all_ready = false;
            }
            incarnations.push((input.incarnation, self.resolve_animation_property(driver)?));
        }
        if all_ready && time > duration {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(incarnations)
    }

    pub(super) fn animation_target_alive(
        &self,
        identity: &AnimationTargetIdentity,
        state: &crate::world::WorldEntityState,
    ) -> bool {
        if crate::allocation_optimizations_enabled()
            && let [offset] = identity.property.indices()
            && crate::components::dynamic_properties::is_dynamic_field(*offset)
        {
            return state
                .entities
                .get(&identity.entity)
                .and_then(|record| record.input(identity.property.component()))
                .is_some_and(|input| input.incarnation == identity.incarnation)
                && state
                    .input_field(
                        &self.world.components,
                        identity.entity,
                        identity.property.component(),
                        *offset,
                    )
                    .is_some();
        }
        if identity
            .property
            .indices()
            .iter()
            .any(|key| crate::components::dynamic_properties::is_dynamic_field(*key))
            && !state
                .input_value(
                    &self.world.components,
                    identity.entity,
                    identity.property.component(),
                )
                .is_some_and(|v| self.read_animation_target(&identity.property, &v).is_ok())
        {
            return false;
        }
        state
            .entities
            .get(&identity.entity)
            .and_then(|record| record.input(identity.property.component()))
            .is_some_and(|input| input.incarnation == identity.incarnation)
    }

    pub(super) fn animation_binding_alive(
        &self,
        driver: &dyn super::driver::AnimationDriverBinding,
        state: &WorldEntityState,
    ) -> bool {
        if !self.animation_target_alive(driver.identity(), state) {
            return false;
        }
        if !crate::allocation_optimizations_enabled()
            && driver
                .identity()
                .property
                .indices()
                .iter()
                .any(|offset| crate::components::dynamic_properties::is_dynamic_field(*offset))
        {
            let identity = driver.identity();
            let Some(value) = state.input_value(
                &self.world.components,
                identity.entity,
                identity.property.component(),
            ) else {
                return false;
            };
            if self.read_bound_animation_target(driver, &value).is_err() {
                return false;
            }
        }
        #[cfg(feature = "skeletal-animation")]
        if let Some(source) = driver.skeleton_source() {
            let Some(skeleton) =
                state.input_skeleton(&self.world.components, driver.identity().entity)
            else {
                return false;
            };
            if !self.bound_skeleton_matches(source, &skeleton) {
                return false;
            }
            let Some(asset) = self
                .asset_resources()
                .get_typed::<crate::SkeletonAsset>(source)
            else {
                return true;
            };
            if !skeleton.pose_source.is_empty() {
                let Some((_, pose)) = self.source_data::<crate::PoseAsset>(
                    crate::POSE_TYPE,
                    &skeleton.pose_source,
                    skeleton.pose_variant,
                ) else {
                    return true;
                };
                if pose.joints().len() != asset.joints().len() {
                    return false;
                }
            }
            return skeleton.joints.as_chunks::<44>().0.iter().all(|bytes| {
                (crate::services::asset_management::skeleton::u32_at(bytes, 0) as usize)
                    < asset.joints().len()
            });
        }
        true
    }

    pub(super) fn binding_uses_asset(
        &self,
        driver: &dyn super::driver::AnimationDriverBinding,
        key: AssetKey,
    ) -> bool {
        if driver.clip() == key {
            return true;
        }
        #[cfg(feature = "particles")]
        if driver.identity().property.component() == ComponentValue::PARTICLE_PLAYBACK
            && let Some(playback) = self
                .world
                .components
                .particle_playback(driver.identity().entity.index() as usize)
            && self.asset_resources().find_source(
                self.world.id,
                crate::systems::particles::PARTICLE_CACHE_TYPE,
                &playback.source,
                playback.variant,
            ) == Some(key)
        {
            return true;
        }
        #[cfg(feature = "skeletal-animation")]
        if let Some(source) = driver.skeleton_source() {
            if source == key {
                return true;
            }
            if let Some(value) = self
                .world
                .components
                .skeleton(driver.identity().entity.index() as usize)
            {
                return self
                    .source_data::<crate::PoseAsset>(
                        crate::POSE_TYPE,
                        &value.pose_source,
                        value.pose_variant,
                    )
                    .is_some_and(|(source, _)| source == key);
            }
        }
        false
    }

    pub(super) fn bind_controller(
        &self,
        controller: &AnimationController,
        baselines: &HashMap<AnimationTargetIdentity, Option<AnimationValue>>,
    ) -> Result<Option<Vec<Box<dyn super::driver::AnimationDriverBinding>>>, ErrorReason> {
        let mut drivers = Vec::new();
        for (description, (incarnation, property)) in controller
            .snapshot
            .description
            .drivers
            .iter()
            .zip(&controller.incarnations)
        {
            let identity = AnimationTargetIdentity {
                entity: description.target,
                incarnation: *incarnation,
                property: property.clone(),
            };
            if !self.animation_target_alive(&identity, self.state) {
                return Err(ErrorReason::MissingComponent);
            }
            let Some(key) = self.source_key(description) else {
                return Ok(None);
            };
            if !self.clip_ready(key)? {
                return Ok(None);
            }
            let clip = self.clip_by_key(key).ok_or(ErrorReason::InvalidAsset)?;
            let track = clip
                .tracks()
                .get(description.track as usize)
                .ok_or(ErrorReason::InvalidField)?;
            let Some(original) = baselines.get(&identity).and_then(Option::as_ref).cloned() else {
                #[cfg(feature = "skeletal-animation")]
                if matches!(description.property, AnimationTrackTarget::Joints(_)) {
                    return Ok(None);
                }
                return Err(ErrorReason::InvalidField);
            };
            if !original.same_type(&track.sample_value(0.0))
                || description.additive && !original.numeric()
                || description.property.indices().len() != track.target().indices().len()
            {
                return Err(ErrorReason::InvalidField);
            }
            if f64::from(description.reference_time) > clip.duration() {
                return Err(ErrorReason::InvalidValue);
            }
            #[cfg(feature = "skeletal-animation")]
            let skeleton_source = if let AnimationTrackTarget::Joints(_) = &description.property {
                let Some(ComponentValue::Skeleton(skeleton)) = self.state.input_value(
                    &self.world.components,
                    description.target,
                    ComponentValue::SKELETON,
                ) else {
                    return Err(ErrorReason::MissingComponent);
                };
                Some(
                    self.source_data::<crate::SkeletonAsset>(
                        crate::SKELETON_TYPE,
                        &skeleton.source,
                        skeleton.variant,
                    )
                    .ok_or(ErrorReason::InvalidAsset)?
                    .0,
                )
            } else {
                None
            };
            drivers.push(make_driver(
                description.clone(),
                *incarnation,
                property.clone(),
                key,
                clip.duration(),
                original,
                #[cfg(feature = "skeletal-animation")]
                skeleton_source,
            )?);
        }
        Ok(Some(drivers))
    }

    #[cfg(feature = "skeletal-animation")]
    pub(super) fn bound_skeleton_matches(
        &self,
        key: AssetKey,
        value: &crate::components::Skeleton,
    ) -> bool {
        let Some(provider) = self.asset_resources().get(key) else {
            return false;
        };
        let source = provider.source();
        let matches = if let Some(local) = value.source.strip_prefix("asset://") {
            source
                .uri
                .strip_prefix("producer://")
                .and_then(|path| path.split_once('/'))
                .is_some_and(|(world, path)| {
                    world.parse::<u64>() == Ok(self.world.id.0) && path == local
                })
        } else {
            source.uri == value.source
        };
        matches && source.kind == crate::SKELETON_TYPE && source.variant == value.variant
    }

    #[cfg(feature = "skeletal-animation")]
    pub(super) fn bound_skeleton_data(
        &self,
        key: AssetKey,
        value: &crate::components::Skeleton,
    ) -> Option<&'a crate::SkeletonAsset> {
        self.bound_skeleton_matches(key, value)
            .then(|| self.asset_resources().get_typed(key))
            .flatten()
    }

    pub(super) fn read_bound_animation_target(
        &self,
        driver: &dyn super::driver::AnimationDriverBinding,
        value: &ComponentValue,
    ) -> Result<AnimationValue, ErrorReason> {
        self.read_animation_target_from_source(
            &driver.identity().property,
            value,
            #[cfg(feature = "skeletal-animation")]
            driver.skeleton_source(),
        )
    }

    pub(in crate::world) fn read_animation_target(
        &self,
        target: &AnimationTrackTarget,
        value: &ComponentValue,
    ) -> Result<AnimationValue, ErrorReason> {
        self.read_animation_target_from_source(
            target,
            value,
            #[cfg(feature = "skeletal-animation")]
            None,
        )
    }

    pub(super) fn read_animation_target_from_source(
        &self,
        target: &AnimationTrackTarget,
        value: &ComponentValue,
        #[cfg(feature = "skeletal-animation")] skeleton_source: Option<AssetKey>,
    ) -> Result<AnimationValue, ErrorReason> {
        match target {
            AnimationTrackTarget::DynamicProperty {
                component,
                name,
            } => {
                if *component != value.type_id() {
                    return Err(ErrorReason::InvalidField);
                }
                value
                    .dynamic_properties()
                    .and_then(|p| p.get(name))
                    .map(|v| {
                        AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(v))
                    })
                    .ok_or(ErrorReason::InvalidField)
            }
            AnimationTrackTarget::AnimationProperty(property) => {
                AnimationValue::read(property, value)
            }
            #[cfg(feature = "skeletal-animation")]
            AnimationTrackTarget::Joints(joints) => {
                if joints.is_empty()
                    || joints.len() > crate::MAX_JOINTS
                    || joints.windows(2).any(|pair| pair[0] >= pair[1])
                {
                    return Err(ErrorReason::InvalidField);
                }
                let ComponentValue::Skeleton(skeleton) = value else {
                    return Err(ErrorReason::MissingComponent);
                };
                let asset = if let Some(key) = skeleton_source {
                    self.bound_skeleton_data(key, skeleton)
                } else {
                    self.source_data::<crate::SkeletonAsset>(
                        crate::SKELETON_TYPE,
                        &skeleton.source,
                        skeleton.variant,
                    )
                    .map(|(_, asset)| asset)
                }
                .ok_or(ErrorReason::InvalidAsset)?;
                let pose = if skeleton.pose_source.is_empty() {
                    None
                } else {
                    let (_, pose) = self
                        .source_data::<crate::PoseAsset>(
                            crate::POSE_TYPE,
                            &skeleton.pose_source,
                            skeleton.pose_variant,
                        )
                        .ok_or(ErrorReason::InvalidAsset)?;
                    if pose.joints().len() != asset.joints().len() {
                        return Err(ErrorReason::InvalidAsset);
                    }
                    Some(pose)
                };
                let mut selected = joints
                    .iter()
                    .map(|&joint| {
                        if let Some(pose) = pose {
                            pose.joints().get(joint as usize).copied()
                        } else {
                            asset.joints().get(joint as usize).map(|joint| joint.rest)
                        }
                        .ok_or(ErrorReason::InvalidField)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                for (joint, transform) in
                    crate::services::asset_management::skeleton::overrides(&skeleton.joints)?
                {
                    if joint >= asset.joints().len() {
                        return Err(ErrorReason::InvalidValue);
                    }
                    if let Ok(index) = joints.binary_search(&(joint as u32)) {
                        selected[index] = transform;
                    }
                }
                Ok(AnimationValue::Pose(selected))
            }
        }
    }

    /// Fixed public fields cannot change type or disappear within an incarnation.
    /// Dynamic fields and joint bindings still require their full lifetime checks.
    pub(super) fn unchanged_static_target(
        &self,
        key: (EntityId, u16),
        staged: &WorldMutationState,
    ) -> bool {
        if !crate::animation_update_reuse_enabled()
            || ComponentValue::supports_dynamic_properties(key.1)
        {
            return false;
        }
        #[cfg(feature = "skeletal-animation")]
        if key.1 == ComponentValue::SKELETON {
            return false;
        }
        let previous = staged.changed.get(&key).copied().flatten();
        previous.is_some()
            && previous
                == staged
                    .entities
                    .get(&key.0)
                    .and_then(|record| record.input(key.1))
                    .map(|input| input.incarnation)
    }

    pub(in crate::world) fn validate_animation_changes(
        &self,
        staged: &WorldMutationState,
    ) -> Result<(), ErrorReason> {
        #[cfg(feature = "profiling")]
        let _allocation_scope = crate::profiling::AllocationScope::new(195, "animation.validate");
        #[cfg(feature = "profiling")]
        let _measurement =
            crate::profiling::Stage::fixed(crate::profiling::FixedStage::AnimationValidate);

        if crate::stress_optimizations_enabled() {
            for key in staged.changed.keys() {
                if self.unchanged_static_target(*key, staged) {
                    continue;
                }
                if let Some(ids) = self.animation.target_controllers.get(key) {
                    for id in ids {
                        for driver in self.animation.controllers[id].drivers_for(*key) {
                            let identity = driver.identity();
                            if (identity.entity, identity.property.component()) == *key {
                                self.validate_animation_driver(driver, staged)?;
                            }
                        }
                    }
                }
            }
        } else {
            for controller in self.animation.controllers.values() {
                for driver in &controller.drivers {
                    self.validate_animation_driver(driver.as_ref(), staged)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn validate_animation_driver(
        &self,
        driver: &dyn super::driver::AnimationDriverBinding,
        staged: &WorldMutationState,
    ) -> Result<(), ErrorReason> {
        let identity = driver.identity();
        if crate::allocation_optimizations_enabled()
            && !staged
                .changed
                .contains_key(&(identity.entity, identity.property.component()))
        {
            return Ok(());
        }
        if !self.animation_binding_alive(driver, staged) {
            return Ok(());
        }
        if crate::allocation_optimizations_enabled()
            && let Some(property) = identity.property.property()
        {
            AnimationValue::read_fields(property, |offset| {
                staged.input_field(
                    &self.world.components,
                    identity.entity,
                    property.component,
                    offset,
                )
            })?;
            return Ok(());
        }
        let value = staged
            .input_value(
                &self.world.components,
                identity.entity,
                identity.property.component(),
            )
            .ok_or(ErrorReason::MissingComponent)?;
        #[cfg(feature = "skeletal-animation")]
        if let Some(source) = driver.skeleton_source()
            && let ComponentValue::Skeleton(skeleton) = &value
            && (self.bound_skeleton_data(source, skeleton).is_none()
                || (!skeleton.pose_source.is_empty()
                    && self
                        .source_data::<crate::PoseAsset>(
                            crate::POSE_TYPE,
                            &skeleton.pose_source,
                            skeleton.pose_variant,
                        )
                        .is_none()))
        {
            return Ok(());
        }
        self.read_bound_animation_target(driver, &value)?;
        Ok(())
    }
}
