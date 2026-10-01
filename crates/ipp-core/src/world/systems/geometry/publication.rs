//! Owned visual, conservative and interaction results from completed evaluation.

use super::{CompoundGeometryShape, GeometryBounds, GeometryRay, GeometryRayHit, GeometrySystem};
use crate::{ComponentValue, EntityId, ErrorReason, WorldContext};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq)]
/// Independent final visual, culling and picking data for one entity.
pub struct PublishedGeometry {
    /// World-local generational owner.
    pub entity: EntityId,
    /// Captured BoundingGeometry lifetime, including unknown candidates.
    pub bounding_incarnation: Option<u64>,
    /// Captured independent PickingGeometry lifetime.
    pub picking_incarnation: Option<u64>,
    /// Generated final visual enclosure for light influence and diagnostics.
    pub visual_bounds: Option<[[f64; 3]; 2]>,
    /// Proven conservative geometry; absence must not reject a render candidate.
    pub culling: Option<CompoundGeometryShape>,
    /// Absent/unevaluated, evaluated failure, or ready interaction union; never visual bounds.
    pub picking: Option<Result<CompoundGeometryShape, ErrorReason>>,
}

#[derive(Clone, Debug, PartialEq)]
/// Immutable geometry candidate domain, preserving explicit unknown bounds.
pub struct GeometryPublication {
    /// Stable entity-ordered owned geometry results.
    pub entities: Vec<PublishedGeometry>,
}

impl GeometryPublication {
    /// Conservative candidates without reading mutable spatial indexes.
    pub fn visible(
        &self,
        planes: &[super::GeometryPlane; 6],
    ) -> impl Iterator<Item = &PublishedGeometry> {
        self.entities.iter().filter(move |geometry| {
            geometry.bounding_incarnation.is_some()
                && geometry
                    .culling
                    .as_ref()
                    .is_none_or(|shape| shape.intersects_frustum(planes))
        })
    }

    /// Nearest completed interaction shape with deterministic entity and part ties.
    pub fn pick(
        &self,
        ray: &GeometryRay,
        near: f64,
        far: f64,
    ) -> Result<Option<(EntityId, GeometryRayHit)>, ErrorReason> {
        let mut nearest: Option<(EntityId, GeometryRayHit)> = None;
        for geometry in &self.entities {
            let Some(picking) = &geometry.picking else {
                continue;
            };
            let shape = picking.as_ref().map_err(|error| *error)?;
            let Some(hit) = shape.ray_intersection(ray, near, far) else {
                continue;
            };
            if nearest.as_ref().is_none_or(|previous| {
                hit.distance
                    .total_cmp(&previous.1.distance)
                    .then(geometry.entity.cmp(&previous.0))
                    .then(hit.part.cmp(&previous.1.part))
                    .is_lt()
            }) {
                nearest = Some((geometry.entity, hit));
            }
        }
        Ok(nearest)
    }
}

impl GeometrySystem {
    pub(super) fn publish(
        &self,
        world: &WorldContext<'_>,
        output: &mut crate::host::WorldOutputBuilder<'_>,
    ) -> Result<(), ErrorReason> {
        let mut entities = Vec::new();
        for (&entity, record) in &world.world.state.entities {
            let bounding_incarnation = record
                .input(ComponentValue::BOUNDING_GEOMETRY)
                .map(|input| input.incarnation);
            let picking_incarnation = record
                .input(ComponentValue::PICKING_GEOMETRY)
                .map(|input| input.incarnation);
            if bounding_incarnation.is_none() && picking_incarnation.is_none() {
                continue;
            }
            let geometry = world.render_geometry(entity);
            entities.push(PublishedGeometry {
                entity,
                bounding_incarnation,
                picking_incarnation,
                visual_bounds: geometry.mesh_bounds,
                culling: geometry.culling.cloned(),
                picking: world
                    .world
                    .components
                    .picking_geometry(entity.index() as usize)
                    .and_then(|value| value.runtime.evaluation.as_ref())
                    .map(|evaluation| evaluation.evaluated().cloned()),
            });
            let mut demand = BTreeSet::new();
            for component in [
                ComponentValue::BOUNDING_GEOMETRY,
                ComponentValue::PICKING_GEOMETRY,
                ComponentValue::MESH_INSTANCE,
                #[cfg(feature = "skeletal-animation")]
                ComponentValue::SKELETON,
                #[cfg(feature = "skeletal-animation")]
                ComponentValue::SKIN,
                #[cfg(feature = "mesh-poses")]
                ComponentValue::MESH_POSE,
            ] {
                if let Some(value) = world
                    .world
                    .components
                    .get(component, entity.index() as usize)
                {
                    value.resource_demand(&mut demand);
                }
            }
            for selection in demand {
                if let Some(key) =
                    world.asset_source_key(selection.kind, &selection.source, selection.variant)
                    && world
                        .asset_resources()
                        .get(key)
                        .is_some_and(|provider| provider.data().is_some())
                {
                    output.retain(key);
                }
            }
        }
        output.chunk(
            Self::ID,
            GeometryPublication {
                entities,
            },
        );
        Ok(())
    }
}
