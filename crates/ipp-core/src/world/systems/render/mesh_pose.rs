//! Mesh pose compatibility is per use; shared endpoint assets stay immutable.

use super::RenderReadAccess;
use crate::{
    ComponentValue, EntityId, ErrorReason, MeshKey,
    components::schema::ComponentLifecycle,
    services::asset_management::{AssetManagementService, service::AssetDemandSelection},
    systems::asset_dependencies::{AssetSourceKeyCache, cached_source_key, source_key_from_fields},
    world::{WorldMutationState, WorldSimulationState},
};
use ipp_schema_derive::SchemaComponent;
use std::collections::BTreeSet;

/// Interpolate the entity's MeshInstance toward a corresponding immutable mesh.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct MeshPose {
    /// Target mesh URI; empty disables interpolation.
    pub source: String,
    /// Target mesh variant. Vertex order and triangle indices must match the base.
    pub variant: u32,
    /// Linear blend in 0..=1: zero selects base positions, one target positions.
    pub weight: f32,
}

impl ComponentLifecycle for MeshPose {
    fn asset_references() -> &'static [crate::components::schema::ComponentAssetReference] {
        &[crate::components::schema::ComponentAssetReference {
            kind: 1,
            source_offset: std::mem::offset_of!(Self, source) as u32,
            variant_offset: std::mem::offset_of!(Self, variant) as u32,
        }]
    }

    fn validate_field(&self, offset: u32) -> Result<(), ErrorReason> {
        if offset == std::mem::offset_of!(Self, source) as u32 {
            crate::services::asset_management::service::validate_source(&self.source)?;
        }

        if offset == std::mem::offset_of!(Self, weight) as u32
            && (!self.weight.is_finite() || !(0.0..=1.0).contains(&self.weight))
        {
            return Err(ErrorReason::InvalidValue);
        }

        Ok(())
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        crate::services::asset_management::service::validate_source(&self.source)?;
        if !self.weight.is_finite() || !(0.0..=1.0).contains(&self.weight) {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(())
    }

    fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
        if !self.source.is_empty() {
            demand.insert(AssetDemandSelection::new(
                crate::MESH_TYPE,
                &self.source,
                self.variant,
            ));
        }
    }
}

pub(in crate::world) fn mesh_pose(
    world: &WorldSimulationState,
    assets: &AssetManagementService,
    mesh_keys: &AssetSourceKeyCache,
    pose_keys: &AssetSourceKeyCache,
    entity: EntityId,
) -> Result<Option<(MeshKey, f32)>, ErrorReason> {
    if !world.state.entities.contains_key(&entity) {
        return Err(ErrorReason::InvalidEntity);
    }
    let index = entity.index() as usize;
    let Some(pose) = world
        .components
        .mesh_pose(index)
        .filter(|pose| !pose.source.is_empty())
    else {
        return Ok(None);
    };
    if !pose.weight.is_finite() || !(0.0..=1.0).contains(&pose.weight) {
        return Err(ErrorReason::InvalidValue);
    }
    let base = world
        .components
        .mesh_instance(index)
        .ok_or(ErrorReason::GeometryUnavailable)?;
    let base_key = cached_source_key(
        assets,
        world.id,
        mesh_keys,
        entity,
        crate::MESH_TYPE,
        &base.source,
        base.variant,
    )
    .ok_or(ErrorReason::GeometryUnavailable)?;
    let target_key = cached_source_key(
        assets,
        world.id,
        pose_keys,
        entity,
        crate::MESH_TYPE,
        &pose.source,
        pose.variant,
    )
    .ok_or(ErrorReason::GeometryUnavailable)?;
    let base = assets
        .get_typed::<crate::services::asset_management::mesh_metadata::MeshMetadata>(base_key)
        .ok_or(ErrorReason::GeometryUnavailable)?;
    let target = assets
        .get_typed::<crate::services::asset_management::mesh_metadata::MeshMetadata>(target_key)
        .ok_or(ErrorReason::GeometryUnavailable)?;
    compatible_pose(base, target)?;
    Ok(Some((
        MeshKey {
            asset: target_key.to_u64(),
            variant: pose.variant,
        },
        pose.weight,
    )))
}

fn compatible_pose(
    base: &crate::services::asset_management::mesh_metadata::MeshMetadata,
    target: &crate::services::asset_management::mesh_metadata::MeshMetadata,
) -> Result<(), ErrorReason> {
    if base.vertex_count() != target.vertex_count() || base.topology() != target.topology() {
        return Err(ErrorReason::InvalidAsset);
    }
    Ok(())
}

impl RenderReadAccess<'_> {
    pub fn mesh_pose(&self, entity: EntityId) -> Result<Option<(MeshKey, f32)>, ErrorReason> {
        mesh_pose(
            self.world,
            self.assets,
            &self.render.mesh_keys,
            &self.render.pose_mesh_keys,
            entity,
        )
    }

    pub(in crate::world) fn validate_mesh_pose_changes(
        &self,
        state: &WorldMutationState,
    ) -> Result<(), ErrorReason> {
        for &(entity, component) in state.changed.keys() {
            if component != ComponentValue::MESH_POSE && component != ComponentValue::MESH_INSTANCE
            {
                continue;
            }
            let base = state.input_value(
                &self.world.components,
                entity,
                ComponentValue::MESH_INSTANCE,
            );
            let pose = state.input_value(&self.world.components, entity, ComponentValue::MESH_POSE);
            if let (Some(ComponentValue::MeshInstance(base)), Some(ComponentValue::MeshPose(pose))) =
                (base, pose)
                && !pose.source.is_empty()
            {
                let data = |source: &str, variant| {
                    let key = source_key_from_fields(
                        self.assets,
                        self.world.id,
                        None,
                        crate::MESH_TYPE,
                        source,
                        variant,
                    )?;
                    self.assets
                        .get(key)?
                        .data()?
                        .metadata()
                        .downcast_ref::<crate::services::asset_management::mesh_metadata::MeshMetadata>()
                };
                if let (Some(base), Some(target)) = (
                    data(&base.source, base.variant),
                    data(&pose.source, pose.variant),
                ) {
                    compatible_pose(base, target)?;
                }
            }
        }
        Ok(())
    }
}

impl crate::WorldContext<'_> {
    /// Resolve corresponding immutable mesh endpoints from evaluated state.
    pub fn mesh_pose(&self, entity: EntityId) -> Result<Option<(MeshKey, f32)>, ErrorReason> {
        self.render_read().mesh_pose(entity)
    }
}
