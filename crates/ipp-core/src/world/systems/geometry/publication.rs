//! Owned visual, conservative and interaction results from completed evaluation.

use super::{CompoundGeometryShape, GeometryBounds, GeometryRay, GeometryRayHit, GeometrySystem};
use crate::{ComponentValue, EntityId, ErrorReason, WorldContext};

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

#[derive(Clone, Debug, Default, PartialEq)]
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
        output.update_chunk::<GeometryPublication>(Self::ID, |publication, output| {
            let entities = &mut publication.entities;
            let mut count = 0;
            let mut presentation = crate::systems::hierarchy::PresentationEligibility::default();
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
                if !presentation.open(world.world, entity) {
                    continue;
                }
                let geometry = world.render_geometry(entity);
                let picking = world
                    .world
                    .components
                    .picking_geometry(entity.index() as usize)
                    .and_then(|value| value.runtime.evaluation.as_ref())
                    .map(|evaluation| evaluation.evaluated());
                if entities.len() == count {
                    entities.push(PublishedGeometry {
                        entity,
                        bounding_incarnation,
                        picking_incarnation,
                        visual_bounds: geometry.mesh_bounds,
                        culling: None,
                        picking: None,
                    });
                }
                let entry = &mut entities[count];
                entry.entity = entity;
                entry.bounding_incarnation = bounding_incarnation;
                entry.picking_incarnation = picking_incarnation;
                entry.visual_bounds = geometry.mesh_bounds;
                if entry.culling.as_ref() != geometry.culling {
                    entry.culling = geometry.culling.cloned();
                }
                if entry
                    .picking
                    .as_ref()
                    .map(|value| value.as_ref().map_err(|error| *error))
                    != picking
                {
                    entry.picking = picking.map(|value| value.cloned());
                }
                count += 1;
                for component in [
                    ComponentValue::BOUNDING_GEOMETRY,
                    ComponentValue::PICKING_GEOMETRY,
                    ComponentValue::MESH_INSTANCE,
                    ComponentValue::SKELETON,
                    ComponentValue::SKIN,
                    ComponentValue::MESH_POSE,
                ] {
                    for selection in world.component_asset_sources(entity, component) {
                        if let Some(key) = world.asset_source_key(
                            selection.kind,
                            &selection.source,
                            selection.variant,
                        ) && world
                            .asset_resources()
                            .get(key)
                            .is_some_and(|provider| provider.data().is_some())
                        {
                            output.retain(key);
                        }
                    }
                }
            }
            entities.truncate(count);
            Ok(())
        })
    }
}
