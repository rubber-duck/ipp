//! Caches owned by GeometrySystem; instance geometry lives on its components.

/// Numeric asset selections reused across this World; evaluated shapes belong to components.
#[derive(Default)]
pub struct GeometrySystemState {
    pub(super) spatial_bounds: super::GeometrySpatialIndex,
    pub(super) spatial_picking: super::GeometrySpatialIndex,
    pub(super) bounds: crate::world::component_query::ComponentQuery<super::BoundingGeometry>,
    pub(super) picking: crate::world::component_query::ComponentQuery<super::PickingGeometry>,
    pub(in crate::world) mesh_keys: super::super::asset_dependencies::AssetSourceKeyCache,
    #[cfg(feature = "mesh-poses")]
    pub(in crate::world) pose_mesh_keys: super::super::asset_dependencies::AssetSourceKeyCache,
}
