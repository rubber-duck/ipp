//! Completed spatial-domain traversal without mutable World or component reads.

use super::*;
use crate::{
    EntityId,
    systems::geometry::{
        GeometryPublication, GeometryRay, GeometryRayHit, GeometryShapeTransform, GeometrySystem,
    },
};

/// One completed spatial contribution in its containing camera domain.
pub struct PublishedSceneContribution<'a> {
    /// Owned data borrowed from the immutable completed publication.
    pub publication: &'a WorldPublication,
    /// Affine transform from this World's local space to the containing domain.
    pub placement: GeometryShapeTransform,
    /// World-qualified attachment path from the containing domain root.
    pub path: Vec<(WorldRef, EntityId)>,
}

#[derive(Clone, Debug, PartialEq)]
/// World-qualified historical hit against a completed camera domain.
pub struct PublishedSceneHit {
    /// Publication containing the intersected geometry.
    pub publication: WorldPublicationId,
    /// Exact producing World lifetime.
    pub world: WorldRef,
    /// Generational picked entity within its World.
    pub entity: EntityId,
    /// Producing component identity (PickingGeometry or Plot).
    pub component: u16,
    /// Original source-row identity for a Plot mark.
    pub row: Option<crate::systems::plot::PlotRowIdentity>,
    /// Captured producing component incarnation.
    pub incarnation: u64,
    /// Containing-domain ray parameter and primitive ordinal.
    pub hit: GeometryRayHit,
    /// World-qualified attachment path from the containing domain root.
    pub path: Vec<(WorldRef, EntityId)>,
}

impl HostRuntime {
    /// Unscoped spatial descendants only; output-owned placements and Surfaces are separate.
    pub fn spatial_contributions(
        &self,
        publication: WorldPublicationId,
    ) -> Vec<PublishedSceneContribution<'_>> {
        self.spatial_contributions_in_domain(publication, None)
    }

    /// Historical Camera-domain contributions, including placements owned by this exact output.
    /// Validates output availability without granting root presentation or input authority.
    pub fn spatial_contributions_for_output(
        &self,
        publication: WorldPublicationId,
        selection: OutputRef,
    ) -> Result<Vec<PublishedSceneContribution<'_>>, ErrorReason> {
        if selection.kind() != OutputKind::Camera || self.output(publication, selection).is_none() {
            return Err(ErrorReason::InvalidEntity);
        }

        Ok(self.spatial_contributions_in_domain(publication, Some(selection)))
    }

    fn spatial_contributions_in_domain(
        &self,
        publication: WorldPublicationId,
        selection: Option<OutputRef>,
    ) -> Vec<PublishedSceneContribution<'_>> {
        let mut output = Vec::new();
        let mut pending = vec![(publication, attachment::IDENTITY, Vec::new())];
        while let Some((id, placement, path)) = pending.pop() {
            let Some(publication) = self.publication(id) else {
                continue;
            };
            let Ok(affine) = GeometryShapeTransform::new(placement) else {
                continue;
            };
            for edge in publication.attachments.iter().rev().filter(|edge| {
                edge.mode == WorldAttachmentMode::Spatial
                    && edge
                        .placement_output
                        .is_none_or(|owner| Some(owner) == selection)
            }) {
                if let Some(child) = self.attached_publication(edge) {
                    let mut path = path.clone();
                    path.push((publication.world, edge.anchor));
                    pending.push((child.id, frame::multiply(placement, edge.placement), path));
                }
            }
            output.push(PublishedSceneContribution {
                publication,
                placement: affine,
                path,
            });
        }
        output
    }

    /// Historical CPU geometry query, not input dispatch or an action admission token.
    pub fn pick_publication(
        &self,
        publication: WorldPublicationId,
        selection: OutputRef,
        ray: &GeometryRay,
        near: f64,
        far: f64,
    ) -> Result<Option<PublishedSceneHit>, ErrorReason> {
        let contributions = self.spatial_contributions_for_output(publication, selection)?;
        let mut nearest: Option<PublishedSceneHit> = None;
        for contribution in contributions {
            let local = contribution.placement.inverse_ray(ray);
            let mut candidates = Vec::new();
            if let Some(geometry) = contribution
                .publication
                .chunk(GeometrySystem::ID)
                .and_then(|chunk| chunk.data::<GeometryPublication>())
                && let Some((entity, hit)) = geometry.pick(&local, near, far)?
            {
                let incarnation = geometry
                    .entities
                    .iter()
                    .find(|value| value.entity == entity)
                    .and_then(|value| value.picking_incarnation)
                    .expect("published picking shape");
                candidates.push(PublishedSceneHit {
                    publication: contribution.publication.id,
                    world: contribution.publication.world,
                    entity,
                    component: crate::ComponentValue::PICKING_GEOMETRY,
                    incarnation,
                    row: None,
                    hit,
                    path: contribution.path.clone(),
                });
            }
            if let Some(plot) = contribution
                .publication
                .chunk(crate::systems::plot::PlotSystem::ID)
                .and_then(|chunk| chunk.data::<crate::systems::plot::PlotPublication>())
                && let Some(pick) = plot.pick(&local, near, far)
            {
                candidates.push(PublishedSceneHit {
                    publication: contribution.publication.id,
                    world: contribution.publication.world,
                    entity: pick.target.entity,
                    component: pick.target.component,
                    incarnation: pick.target.incarnation,
                    row: Some(pick.row),
                    hit: pick.hit,
                    path: contribution.path.clone(),
                });
            }
            for edge in &contribution.publication.attachments {
                if edge.mode == WorldAttachmentMode::Spatial
                    || edge
                        .placement_output
                        .is_some_and(|owner| owner != selection)
                {
                    continue;
                }
                if let Some(hit) = self.pick_plot_surface(&contribution, edge, ray, near, far)? {
                    candidates.push(hit);
                }
            }
            for candidate in candidates {
                if nearest.as_ref().is_none_or(|previous| {
                    candidate
                        .hit
                        .distance
                        .total_cmp(&previous.hit.distance)
                        .then(candidate.world.cmp(&previous.world))
                        .then(candidate.entity.cmp(&previous.entity))
                        .then(candidate.component.cmp(&previous.component))
                        .is_lt()
                }) {
                    nearest = Some(candidate);
                }
            }
        }
        Ok(nearest)
    }
}
