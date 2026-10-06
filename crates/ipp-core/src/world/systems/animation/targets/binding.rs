use super::driver::{AnimationTargetIdentity, make_driver};
use crate::systems::animation::controller::{add_description_demand, contribution};
use crate::systems::animation::{
    ANIMATION_TYPE, AnimationClip, AnimationController, AnimationControllerDescription,
    AnimationControllerId, AnimationDriverDescription, AnimationEntityPlacementKey,
    AnimationProperty, AnimationReadAccess, AnimationTrackTarget, AnimationValue, MAX_CONTROLLERS,
};
use crate::world::WorldMutationState;
use crate::{
    ComponentValue, EntityId, ErrorReason,
    services::asset_management::{AssetDemandSelection, AssetKey},
    world::WorldEntityState,
};
use std::collections::BTreeSet;

pub(in crate::world::systems::animation) fn validate_target_support(
    manifest: &crate::systems::WorldManifest,
    target: &AnimationTrackTarget,
) -> Result<(), ErrorReason> {
    use crate::systems::WorldOperation;

    if !manifest.supports_operation(WorldOperation::Animation) {
        return Err(ErrorReason::UnsupportedDependency);
    }
    match target {
        AnimationTrackTarget::EntityLink => {
            if !manifest.supports_operation(WorldOperation::EntityLinks) {
                return Err(ErrorReason::UnsupportedDependency);
            }
        }
        _ => {
            if !manifest.supports_component(target.component_target()) {
                return Err(ErrorReason::UnsupportedDependency);
            }
            if matches!(target, AnimationTrackTarget::Joints(_))
                && !manifest.supports_operation(WorldOperation::JointAnimation)
            {
                return Err(ErrorReason::UnsupportedDependency);
            }
        }
    }
    Ok(())
}

/// Whether an offset names a property that may depart within a component
/// incarnation: a dynamic property (removal) or a row property (row removal,
/// or clearing an optional property). Its bindings check presence, not only
/// the incarnation.
pub(in crate::world::systems::animation) fn removable_field(offset: u32) -> bool {
    crate::components::dynamic_properties::is_dynamic_field(offset)
        || crate::components::rows::row_region(offset).is_some()
}

/// A removable property is live while it reads a present value; an absent
/// optional row property reads `Unset` and a dead slot reads nothing.
pub(in crate::world::systems::animation) fn present_field(
    value: Option<crate::components::schema::FieldValue>,
) -> bool {
    value.is_some_and(|value| !matches!(value, crate::components::schema::FieldValue::Unset))
}

impl<'a> AnimationReadAccess<'a> {
    pub(in crate::world::systems::animation) fn asset_resources(
        &self,
    ) -> &'a crate::services::asset_management::AssetManagementService {
        self.asset_acquisition
    }

    pub(in crate::world::systems::animation) fn source_data<T: 'static>(
        &self,
        kind: crate::services::asset_management::AssetTypeId,
        source: &str,
        variant: u32,
    ) -> Option<(AssetKey, &'a T)> {
        self.asset_acquisition
            .source_data(self.world.id, kind, source, variant)
    }

    pub(in crate::world::systems::animation) fn sampled_source_is_authorized(
        &self,
        source: &str,
        clip_source: &str,
    ) -> bool {
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

    pub(in crate::world::systems::animation) fn source_key(
        &self,
        description: &AnimationDriverDescription,
    ) -> Option<AssetKey> {
        self.asset_resources().find_source(
            self.world.id,
            ANIMATION_TYPE,
            &description.source,
            description.variant,
        )
    }

    pub(in crate::world::systems::animation) fn clip_by_key(
        &self,
        key: AssetKey,
    ) -> Option<&'a AnimationClip> {
        self.asset_resources().get_typed::<AnimationClip>(key)
    }

    pub(in crate::world::systems::animation) fn clip_ready(
        &self,
        key: AssetKey,
    ) -> Result<bool, ErrorReason> {
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

    pub(in crate::world::systems::animation) fn validate_animation_demand(
        &self,
        demand: &BTreeSet<AssetDemandSelection>,
    ) -> Result<(), ErrorReason> {
        self.asset_acquisition
            .validate_additional_users(self.world.id, demand.iter().cloned())
            .map_err(|_| ErrorReason::Capacity)
    }

    pub(in crate::world::systems::animation) fn controller_demand(
        &self,
        replace: Option<(AnimationControllerId, &AnimationControllerDescription)>,
        remove: Option<AnimationControllerId>,
    ) -> Result<BTreeSet<AssetDemandSelection>, ErrorReason> {
        #[cfg(feature = "instrumentation")]
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
            // Retained controllers were validated when admitted; check only the
            // demand this description adds, proportionally to the controller.
            let mut added = BTreeSet::new();
            add_description_demand(description, &mut added);
            self.validate_animation_demand(&added)?;
            demand.extend(added);
        }
        Ok(demand)
    }

    pub(in crate::world::systems::animation) fn resolve_animation_property(
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

    pub(in crate::world::systems::animation) fn validate_controller_description(
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
            validate_target_support(&self.world.manifest, &driver.property)?;
            crate::services::asset_management::validate_source(&driver.source)?;
            if driver.source.is_empty()
                || !driver.weight.is_finite()
                || !(0.0..=1.0).contains(&driver.weight)
                || !driver.reference_time.is_finite()
                || driver.reference_time < 0.0
            {
                return Err(ErrorReason::InvalidValue);
            }
            if matches!(driver.property, AnimationTrackTarget::EntityLink) {
                let bindings = &driver.entity_bindings;
                if driver.weight != 1.0 || driver.additive || driver.reference_time != 0.0 {
                    return Err(ErrorReason::InvalidField);
                }
                if !self.state.entities.contains_key(&driver.target)
                    || bindings
                        .iter()
                        .any(|entity| !self.state.entities.contains_key(entity))
                {
                    return Err(ErrorReason::InvalidEntity);
                }
                if let Some(clip) = self
                    .source_key(driver)
                    .and_then(|key| self.clip_by_key(key))
                {
                    let track = clip
                        .typed_track::<AnimationEntityPlacementKey>(driver.track as usize)
                        .ok_or(ErrorReason::InvalidField)?;
                    if !matches!(track.target, AnimationTrackTarget::EntityLink) {
                        return Err(ErrorReason::InvalidField);
                    }
                    if !super::structural::structural_slots_valid(track, bindings) {
                        return Err(ErrorReason::InvalidField);
                    }
                    if f64::from(driver.reference_time) > clip.duration() {
                        return Err(ErrorReason::InvalidValue);
                    }
                    duration = duration.max(clip.duration());
                } else {
                    all_ready = false;
                }
                incarnations.push((0, driver.property.clone()));
                continue;
            }
            if !driver.entity_bindings.is_empty() {
                return Err(ErrorReason::InvalidField);
            }
            let record = self
                .state
                .entities
                .get(&driver.target)
                .ok_or(ErrorReason::InvalidEntity)?;
            let input = record
                .input(driver.property.component_target())
                .ok_or(ErrorReason::MissingComponent)?;
            let value = self
                .state
                .input_value(
                    &self.world.components,
                    driver.target,
                    driver.property.component_target(),
                )
                .ok_or(ErrorReason::MissingComponent)?;
            // Every offset must be a field the component lets animation write,
            // on the numeric and the general path alike, and a row-region offset
            // must address a row property of one of its rows fields.
            if let Some(property) = driver.property.property()
                && property.offsets.iter().any(|offset| {
                    !ComponentValue::animatable_field(property.component, *offset)
                        || crate::components::rows::row_region(*offset).is_some()
                            && ComponentValue::validate_field(
                                property.component,
                                *offset,
                                crate::components::schema::FieldKind::Dynamic,
                            )
                            .is_err()
                })
            {
                return Err(ErrorReason::InvalidField);
            }
            let current = match self.read_animation_target(&driver.property, &value) {
                Ok(value) => Some(value),
                Err(ErrorReason::InvalidAsset)
                    if matches!(driver.property, AnimationTrackTarget::Joints(_)) =>
                {
                    None
                }
                Err(reason) => return Err(reason),
            };
            if driver.additive
                && current
                    .as_ref()
                    .is_some_and(|current| !contribution::contributes(current))
            {
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

    pub(in crate::world::systems::animation) fn animation_target_alive(
        &self,
        identity: &AnimationTargetIdentity,
        state: &crate::world::WorldEntityState,
    ) -> bool {
        if matches!(identity.property, AnimationTrackTarget::EntityLink) {
            return state.entities.contains_key(&identity.entity);
        }
        if let [offset] = identity.property.indices()
            && removable_field(*offset)
        {
            return state
                .entities
                .get(&identity.entity)
                .and_then(|record| record.input(identity.property.component_target()))
                .is_some_and(|input| input.incarnation == identity.incarnation)
                && present_field(state.input_field(
                    &self.world.components,
                    identity.entity,
                    identity.property.component_target(),
                    *offset,
                ));
        }
        if identity
            .property
            .indices()
            .iter()
            .any(|key| removable_field(*key))
            && !state
                .input_value(
                    &self.world.components,
                    identity.entity,
                    identity.property.component_target(),
                )
                .is_some_and(|v| self.read_animation_target(&identity.property, &v).is_ok())
        {
            return false;
        }
        state
            .entities
            .get(&identity.entity)
            .and_then(|record| record.input(identity.property.component_target()))
            .is_some_and(|input| input.incarnation == identity.incarnation)
    }

    pub(in crate::world::systems::animation) fn animation_binding_alive(
        &self,
        driver: &dyn super::driver::AnimationDriverBinding,
        state: &WorldEntityState,
    ) -> bool {
        if !self.animation_target_alive(driver.identity(), state) {
            return false;
        }
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
            return skeleton.joint_overrides_fit(asset.joints().len());
        }
        true
    }

    pub(in crate::world::systems::animation) fn binding_uses_asset(
        &self,
        driver: &dyn super::driver::AnimationDriverBinding,
        key: AssetKey,
    ) -> bool {
        if driver.clip() == key {
            return true;
        }
        if driver.identity().property.component_target() == ComponentValue::PARTICLE_PLAYBACK
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

    pub(in crate::world::systems::animation) fn bind_controller(
        &self,
        controller: &AnimationController,
    ) -> Result<Option<Vec<Box<dyn super::driver::AnimationDriverBinding>>>, ErrorReason> {
        let mut drivers = Vec::new();
        for (description, (incarnation, property)) in controller
            .snapshot
            .description
            .drivers
            .iter()
            .zip(&controller.incarnations)
        {
            validate_target_support(&self.world.manifest, property)?;
            if matches!(property, AnimationTrackTarget::EntityLink) {
                continue;
            }
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
            let current = match self
                .state
                .input_value(
                    &self.world.components,
                    identity.entity,
                    identity.property.component_target(),
                )
                .ok_or(ErrorReason::MissingComponent)
                .and_then(|value| self.read_animation_target(&identity.property, &value))
            {
                Ok(current) => current,
                // Joint targets wait for the Skeleton's assets.
                Err(ErrorReason::InvalidAsset)
                    if matches!(description.property, AnimationTrackTarget::Joints(_)) =>
                {
                    return Ok(None);
                }
                Err(reason) => return Err(reason),
            };
            let template = track.sample_value(0.0);
            if !current.same_type(&template)
                || description.additive && !contribution::contributes(&template)
                || description.property.indices().len() != track.target().indices().len()
            {
                return Err(ErrorReason::InvalidField);
            }
            if f64::from(description.reference_time) > clip.duration() {
                return Err(ErrorReason::InvalidValue);
            }
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
                template,
                skeleton_source,
            )?);
        }
        Ok(Some(drivers))
    }

    pub(in crate::world::systems::animation) fn bound_skeleton_matches(
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
            crate::components::schema::same_text(&source.uri, &value.source)
        };
        matches && source.kind == crate::SKELETON_TYPE && source.variant == value.variant
    }

    pub(in crate::world::systems::animation) fn bound_skeleton_data(
        &self,
        key: AssetKey,
        value: &crate::components::Skeleton,
    ) -> Option<&'a crate::SkeletonAsset> {
        self.bound_skeleton_matches(key, value)
            .then(|| self.asset_resources().get_typed(key))
            .flatten()
    }

    pub(in crate::world::systems::animation) fn read_bound_animation_target(
        &self,
        driver: &dyn super::driver::AnimationDriverBinding,
        value: &ComponentValue,
    ) -> Result<AnimationValue, ErrorReason> {
        self.read_animation_target_from_source(
            &driver.identity().property,
            value,
            driver.skeleton_source(),
        )
    }

    pub(in crate::world) fn read_animation_target(
        &self,
        target: &AnimationTrackTarget,
        value: &ComponentValue,
    ) -> Result<AnimationValue, ErrorReason> {
        self.read_animation_target_from_source(target, value, None)
    }

    pub(in crate::world::systems::animation) fn read_animation_target_from_source(
        &self,
        target: &AnimationTrackTarget,
        value: &ComponentValue,
        skeleton_source: Option<AssetKey>,
    ) -> Result<AnimationValue, ErrorReason> {
        match target {
            AnimationTrackTarget::EntityLink => Err(ErrorReason::InvalidField),
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
                skeleton.apply_selected_joint_overrides(
                    asset.joints().len(),
                    joints,
                    &mut selected,
                )?;
                Ok(AnimationValue::Pose(selected))
            }
        }
    }

    /// Fixed public fields cannot change type or disappear within an incarnation.
    /// Dynamic fields, row properties and joint bindings still require their
    /// full lifetime checks.
    pub(in crate::world::systems::animation) fn unchanged_static_target(
        &self,
        key: (EntityId, u16),
        staged: &WorldMutationState,
    ) -> bool {
        // Dynamic properties and rows may depart within an incarnation. Every
        // rows-capable component addresses region zero's first property.
        if ComponentValue::supports_dynamic_properties(key.1)
            || ComponentValue::has_field(key.1, crate::components::rows::row_region_base(0))
        {
            return false;
        }
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
        #[cfg(feature = "instrumentation")]
        let _allocation_scope = crate::profiling::AllocationScope::new(195, "animation.validate");
        #[cfg(feature = "instrumentation")]
        let _measurement =
            crate::profiling::Stage::fixed(crate::profiling::FixedStage::AnimationValidate);

        for key in staged.changed.keys() {
            if self.unchanged_static_target(*key, staged) {
                continue;
            }
            if let Some(ids) = self.animation.target_controllers.get(key) {
                for id in ids {
                    for driver in self.animation.controllers[id].drivers_for(*key) {
                        let identity = driver.identity();
                        if (identity.entity, identity.property.component_target()) == *key {
                            self.validate_animation_driver(driver, staged)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub(in crate::world::systems::animation) fn validate_animation_driver(
        &self,
        driver: &dyn super::driver::AnimationDriverBinding,
        staged: &WorldMutationState,
    ) -> Result<(), ErrorReason> {
        let identity = driver.identity();
        if !staged
            .changed
            .contains_key(&(identity.entity, identity.property.component_target()))
        {
            return Ok(());
        }
        if !self.animation_binding_alive(driver, staged) {
            return Ok(());
        }
        if let Some(property) = identity.property.property() {
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
                identity.property.component_target(),
            )
            .ok_or(ErrorReason::MissingComponent)?;
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
