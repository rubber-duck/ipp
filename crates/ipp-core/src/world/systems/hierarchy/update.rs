use super::*;

pub(crate) fn affine(transform: &Transform) -> Result<GeometryShapeTransform, ErrorReason> {
    let affine = CameraAffineTransform::new(transform)?;
    Ok(GeometryShapeTransform::from_inverse_pair(
        affine.matrix(),
        affine.inverse_matrix(),
    ))
}

/// Effective attachment frame shared by propagation and terminal LookAt.
/// Joint matrices are copied from component-owned pose data; no binding survives a call.
pub(in crate::world) fn parent_affine(
    world: &WorldSimulationState,
    entity: EntityId,
    parent: EntityId,
) -> Result<GeometryShapeTransform, ErrorReason> {
    let object = evaluated_affine(world, parent)?;
    let bone = world
        .components
        .parent_joint(entity.index() as usize)
        .map_or(u32::MAX, |value| value.ordinal);
    if bone == u32::MAX {
        return Ok(object);
    }
    let pose = world
        .components
        .skeleton(parent.index() as usize)
        .and_then(|skeleton| skeleton.runtime.pose.as_ref())
        .filter(|pose| pose.valid)
        .ok_or(ErrorReason::InvalidAsset)?;
    let joint = pose
        .global
        .get(bone as usize)
        .ok_or(ErrorReason::InvalidValue)?;
    GeometryShapeTransform::from_matrix(*joint)?.then(&object)
}

/// Read the entity's final result from stable, lazily prepared spatial storage.
pub(in crate::world) fn evaluated_affine(
    world: &WorldSimulationState,
    entity: EntityId,
) -> Result<GeometryShapeTransform, ErrorReason> {
    if !world.state.entities.contains_key(&entity) {
        return Err(ErrorReason::InvalidEntity);
    }
    world
        .state
        .links
        .transform(entity)
        .and_then(|runtime| runtime.value())
        .ok_or(ErrorReason::InvalidValue)
}

impl crate::WorldContext<'_> {
    /// Final object placement, preserving nonuniform scale and shear.
    pub fn world_matrix(&self, entity: EntityId) -> Result<[f32; 16], ErrorReason> {
        evaluated_affine(self.world, entity)?.render_matrix()
    }
}
