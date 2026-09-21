use super::*;

pub(in crate::world) fn local_affine(
    world: &WorldSimulationState,
    entity: EntityId,
    aimed: bool,
) -> Result<GeometryShapeTransform, ErrorReason> {
    if !world.state.entities.contains_key(&entity) {
        return Err(ErrorReason::InvalidEntity);
    }
    let index = entity.index() as usize;
    let mut transform = world
        .components
        .transform(index)
        .copied()
        .unwrap_or_default();
    if aimed
        && world
            .components
            .look_at(index)
            .is_some_and(|value| value.runtime.invalid)
    {
        return Err(ErrorReason::UnsupportedDependency);
    }
    if aimed
        && let Some(rotation) = world
            .components
            .look_at(index)
            .and_then(|v| v.runtime.rotation)
    {
        [transform.qx, transform.qy, transform.qz, transform.qw] = rotation;
    }
    affine(&transform)
}

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
        .hierarchy(entity.index() as usize)
        .map_or(u32::MAX, |value| value.parent_bone);
    if bone == u32::MAX {
        return Ok(object);
    }
    #[cfg(feature = "skeletal-animation")]
    {
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
    #[cfg(not(feature = "skeletal-animation"))]
    Err(ErrorReason::UnsupportedDependency)
}

/// Read the component-owned final result; roots without a relationship need no cache.
pub(in crate::world) fn evaluated_affine(
    world: &WorldSimulationState,
    entity: EntityId,
) -> Result<GeometryShapeTransform, ErrorReason> {
    if !world.state.entities.contains_key(&entity) {
        return Err(ErrorReason::InvalidEntity);
    }
    if let Some(hierarchy) = world.components.hierarchy(entity.index() as usize) {
        hierarchy.runtime.world.ok_or(ErrorReason::InvalidValue)
    } else {
        // SAFETY: This binding is phase-local and cannot escape the shared World
        // borrow; none of the selected component incarnations can end during it.
        unsafe { ObjectTransformBinding::bind(&world.components, entity) }
            .evaluate(&world.components)
    }
}

impl crate::WorldContext<'_> {
    /// Final object placement, preserving nonuniform scale and shear.
    pub fn world_matrix(&self, entity: EntityId) -> Result<[f32; 16], ErrorReason> {
        evaluated_affine(self.world, entity)?.render_matrix()
    }
}
