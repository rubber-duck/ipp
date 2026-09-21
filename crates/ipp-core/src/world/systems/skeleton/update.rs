use super::*;

pub(in crate::world) fn local_pose(
    assets: &AssetManagementService,
    world: WorldId,
    value: &Skeleton,
) -> Result<Option<(AssetKey, Vec<Transform>)>, ErrorReason> {
    local_pose_into(assets, world, value, Vec::new())
}

fn local_pose_into(
    assets: &AssetManagementService,
    world: WorldId,
    value: &Skeleton,
    mut local: Vec<Transform>,
) -> Result<Option<(AssetKey, Vec<Transform>)>, ErrorReason> {
    #[cfg(feature = "profiling")]
    let _allocation_scope = crate::profiling::AllocationScope::new(207, "skeleton.local_pose");

    let Some((key, skeleton)) = assets.source_data::<SkeletonAsset>(
        world,
        crate::SKELETON_TYPE,
        &value.source,
        value.variant,
    ) else {
        return Ok(None);
    };
    local.clear();
    if value.pose_source.is_empty() {
        local.extend(skeleton.joints().iter().map(|joint| joint.rest));
    } else {
        let Some((_, pose)) = assets.source_data::<PoseAsset>(
            world,
            crate::POSE_TYPE,
            &value.pose_source,
            value.pose_variant,
        ) else {
            return Ok(None);
        };
        if pose.joints().len() != skeleton.joints().len() {
            return Err(ErrorReason::InvalidAsset);
        }
        local.extend_from_slice(pose.joints());
    }
    for (joint, transform) in
        crate::services::asset_management::skeleton::override_iter(&value.joints)?
    {
        *local.get_mut(joint).ok_or(ErrorReason::InvalidValue)? = transform;
    }
    Ok(Some((key, local)))
}

fn assign_local_pose(
    runtime: &mut SkeletonRuntimeState,
    resolved: Option<(AssetKey, Vec<Transform>)>,
    preserved: Option<&std::collections::BTreeSet<u32>>,
) -> Vec<Transform> {
    let Some((source, mut local)) = resolved else {
        if let Some(pose) = runtime.pose.as_mut() {
            pose.valid = false;
        }
        return Vec::new();
    };
    if let Some(pose) = runtime
        .pose
        .as_mut()
        .filter(|pose| pose.source == source && pose.local.len() == local.len())
    {
        for (index, local) in local.drain(..).enumerate() {
            if !(preserved.is_some_and(|joints| joints.contains(&(index as u32)))
                || (crate::allocation_followup_enabled() && pose.sampled[index]))
            {
                pose.local[index] = local;
            }
        }
        pose.valid = true;
        local
    } else {
        runtime.pose = Some(SkeletonPoseState {
            valid: true,
            source,
            global: vec![[0.0; 16]; local.len()].into_boxed_slice(),
            evaluation: vec![Transform::default(); local.len()].into_boxed_slice(),
            sampled: vec![false; local.len()].into_boxed_slice(),
            local: local.into_boxed_slice(),
        });
        Vec::new()
    }
}

fn prepare_component(
    value: &mut Skeleton,
    assets: &AssetManagementService,
    world: WorldId,
    scratch: &mut Vec<Transform>,
) -> Result<(), ErrorReason> {
    if let Some(pose) = &mut value.runtime.pose {
        pose.sampled.fill(false);
    }
    let reuse = crate::allocation_optimizations_enabled();
    let resolved = local_pose_into(
        assets,
        world,
        value,
        if reuse {
            std::mem::take(scratch)
        } else {
            Vec::new()
        },
    )?;
    let capacity = assign_local_pose(&mut value.runtime, resolved, None);
    if reuse {
        *scratch = capacity;
    }
    Ok(())
}

/// Rebase a same-rig discrete pose input while preserving earlier sampled entries.
/// Source replacement stays on the generic invalidation path before allocation reuse.
pub(in crate::world) fn rebase_sampled_inputs(
    value: &mut Skeleton,
    sampled: &Skeleton,
    assets: &AssetManagementService,
    world: WorldId,
    preserved: Option<&std::collections::BTreeSet<u32>>,
) -> Result<(), ErrorReason> {
    if value.source != sampled.source || value.variant != sampled.variant {
        return Ok(());
    }
    let resolved = local_pose(assets, world, sampled)?;
    assign_local_pose(&mut value.runtime, resolved, preserved);
    Ok(())
}

impl SkeletonSystem {
    /// Resolve authored locals before animation writes selected internal targets.
    pub(in crate::world) fn prepare(
        &mut self,
        context: &mut crate::systems::SystemRuntimeAccess<'_>,
    ) {
        self.state.skeleton_diagnostics.clear();
        self.state.components.prepare(
            context.world,
            crate::components::registry::ComponentStorage::skeleton_ptr,
        );
        for &(entity, binding) in self.state.components.entries() {
            let value = binding.get_mut(&mut context.world.components);
            if let Err(reason) = prepare_component(
                value,
                context.asset_acquisition,
                context.world.id,
                &mut self.state.local_scratch,
            ) {
                if let Some(pose) = value.runtime.pose.as_mut() {
                    pose.valid = false;
                }
                self.state
                    .skeleton_diagnostics
                    .push(crate::RenderDiagnostic {
                        entity,
                        reason,
                    });
            }
        }
    }

    /// Propagate the sampled local buffer without reconstructing authored values.
    pub(in crate::world) fn evaluate(
        &mut self,
        context: &mut crate::systems::SystemRuntimeAccess<'_>,
    ) {
        self.state.components.prepare(
            context.world,
            crate::components::registry::ComponentStorage::skeleton_ptr,
        );
        for &(entity, binding) in self.state.components.entries() {
            let value = binding.get_mut(&mut context.world.components);
            // A discrete source sample can replace a buffer after preparation.
            // Rebuild that new source now so this frame never uses the old rig.
            if value.runtime.pose.is_none()
                && let Err(reason) = prepare_component(
                    value,
                    context.asset_acquisition,
                    context.world.id,
                    &mut self.state.local_scratch,
                )
            {
                self.state
                    .skeleton_diagnostics
                    .push(crate::RenderDiagnostic {
                        entity,
                        reason,
                    });
            }
            let Some(pose) = value.runtime.pose.as_mut().filter(|pose| pose.valid) else {
                continue;
            };
            let result = (|| {
                let skeleton = context
                    .asset_acquisition
                    .get_typed::<SkeletonAsset>(pose.source)
                    .ok_or(ErrorReason::InvalidAsset)?;
                for (index, joint) in skeleton.joints().iter().enumerate() {
                    let local = camera::model_matrix(&pose.local[index])?;
                    let global = joint
                        .parent
                        .map_or(local, |parent| camera::multiply(pose.global[parent], local));
                    if !global.iter().all(|value| value.is_finite()) {
                        return Err(ErrorReason::InvalidValue);
                    }
                    pose.global[index] = global;
                }
                Ok(())
            })();
            if let Err(reason) = result {
                if let Some(pose) = value.runtime.pose.as_mut() {
                    pose.valid = false;
                }
                self.state
                    .skeleton_diagnostics
                    .push(crate::RenderDiagnostic {
                        entity,
                        reason,
                    });
            }
        }
    }
}

pub(in crate::world) fn validate_changes(
    world: &WorldSimulationState,
    assets: &AssetManagementService,
    staged: &WorldMutationState,
) -> Result<(), ErrorReason> {
    for &(entity, component) in staged.changed.keys() {
        if crate::allocation_optimizations_enabled()
            && component != ComponentValue::SKELETON
            && component != ComponentValue::SKIN
        {
            continue;
        }
        let value = staged.input_value(&world.components, entity, component);
        if let Some(ComponentValue::Skeleton(value)) = &value {
            local_pose(assets, world.id, value)?;
        }

        if let Some(ComponentValue::Skin(value)) = value
            && value.skeleton.to_bits() != 0
            && !staged
                .entities
                .get(&value.skeleton)
                .is_some_and(|record| record.input(ComponentValue::SKELETON).is_some())
        {
            return Err(ErrorReason::InvalidEntity);
        }
    }
    Ok(())
}

pub(in crate::world) fn pose<'a>(
    world: &'a WorldSimulationState,
    authored: &WorldEntityState,
    entity: EntityId,
) -> Option<&'a SkeletonPoseState> {
    authored.entities.get(&entity)?;
    world
        .components
        .skeleton(entity.index() as usize)?
        .runtime
        .pose
        .as_ref()
        .filter(|pose| pose.valid)
}

impl WorldContext<'_> {
    /// Current evaluated local pose. The borrow cannot span a mutation boundary.
    pub fn skeleton_pose(&self, entity: EntityId) -> Option<&[Transform]> {
        Some(&pose(self.world, &self.world.state, entity)?.local)
    }
}
