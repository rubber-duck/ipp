use super::*;

impl SkinningSystem {
    pub(in crate::world) fn evaluate(
        &mut self,
        context: &mut crate::systems::SystemRuntimeAccess<'_>,
    ) {
        self.state.diagnostics.clear();
        self.state.components.prepare(
            context.world,
            crate::components::registry::ComponentStorage::skin_ptr,
        );
        let mut palette = std::mem::take(&mut self.state.palette_scratch);
        for &(entity, binding) in self.state.components.entries() {
            let index = entity.index() as usize;
            let skin = binding.get(&context.world.components);
            let result = (|| {
                let Some(pose) = skeleton::pose(context.world, &context.world.state, skin.skeleton)
                else {
                    return Ok(None);
                };
                let Some((_, binding)) = context.asset_acquisition.source_data::<crate::SkinAsset>(
                    context.world.id,
                    crate::SKIN_TYPE,
                    &skin.source,
                    skin.variant,
                ) else {
                    return Ok(None);
                };
                let Some(instance) = context.world.components.mesh_instance(index) else {
                    return Ok(None);
                };
                let Some((_, mesh)) = context.asset_acquisition.source_data::<crate::services::asset_management::mesh_metadata::MeshMetadata>(
                    context.world.id,
                    crate::MESH_TYPE,
                    &instance.source,
                    instance.variant,
                ) else {
                    return Ok(None);
                };
                let maximum = mesh.max_joint_index().ok_or(ErrorReason::InvalidAsset)?;
                if maximum as usize >= binding.joints().len() {
                    return Err(ErrorReason::InvalidAsset);
                }
                let mesh_transform =
                    crate::systems::hierarchy::evaluated_affine(context.world, entity)?;
                let skeleton_transform =
                    crate::systems::hierarchy::evaluated_affine(context.world, skin.skeleton)?;
                let relative = camera::multiply(
                    mesh_transform.inverse_matrix().map(|v| v as f32),
                    skeleton_transform.render_matrix()?,
                );
                palette.clear();
                palette.reserve(binding.joints().len());
                for entry in binding.joints() {
                    let joint = pose
                        .global
                        .get(entry.joint)
                        .ok_or(ErrorReason::InvalidAsset)?;
                    let matrix =
                        camera::multiply(relative, camera::multiply(*joint, entry.inverse_bind));
                    if !matrix.iter().all(|value| value.is_finite()) {
                        return Err(ErrorReason::InvalidValue);
                    }
                    palette.push(matrix);
                }
                Ok(Some(()))
            })();
            let skin = binding.get_mut(&mut context.world.components);
            match result {
                Ok(Some(())) => {
                    skin.runtime.valid = true;
                    if let Some(previous) = skin
                        .runtime
                        .palette
                        .as_mut()
                        .filter(|previous| previous.len() == palette.len())
                    {
                        previous.copy_from_slice(&palette);
                    } else {
                        skin.runtime.palette =
                            Some(std::mem::take(&mut palette).into_boxed_slice());
                    }
                }
                Ok(None) => skin.runtime.valid = false,
                Err(reason) => {
                    skin.runtime.valid = false;
                    self.state.diagnostics.push(crate::RenderDiagnostic {
                        entity,
                        reason,
                    });
                }
            }
            if !crate::allocation_optimizations_enabled() {
                palette = Vec::new();
            }
        }
        if crate::allocation_optimizations_enabled() {
            self.state.palette_scratch = palette;
        }
    }
}

pub(in crate::world) fn palette(
    world: &crate::world::WorldSimulationState,
    entity: EntityId,
) -> Option<&[[f32; 16]]> {
    world.state.entities.get(&entity)?;
    let runtime = &world.components.skin(entity.index() as usize)?.runtime;
    runtime
        .valid
        .then_some(runtime.palette.as_deref())
        .flatten()
}

impl WorldContext<'_> {
    /// Mesh-local matrices in the immutable skin binding's palette order.
    pub fn skin_palette(&self, entity: EntityId) -> Option<&[[f32; 16]]> {
        palette(self.world, entity)
    }
}
