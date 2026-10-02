use super::traversal::{QueryTask, QueryView, QueryWalk};
use super::*;
use crate::WorldAttachmentMode;
use crate::systems::camera::CameraPublication;
use crate::systems::geometry::{
    GeometryBounds, GeometryPublication, GeometryShapeTransform, GeometrySystem,
};

struct Candidate<'a> {
    distance: f64,
    blocker: bool,
    world: WorldRef,
    entity: EntityId,
    task: QueryTask<'a>,
}

impl<'a> QueryWalk<'a, '_> {
    pub(super) fn camera(&mut self, view: QueryView) -> Result<(), GuiQueryUnavailable> {
        let camera = self
            .host
            .output(view.publication, view.output)
            .and_then(|chunk| chunk.data::<CameraPublication>())
            .ok_or(GuiQueryUnavailable::Data(ErrorReason::InvalidEntity))?;
        let ray = camera
            .ray_for_extent(view.point, view.extent)
            .map_err(GuiQueryUnavailable::Data)?;
        let mut pending = vec![(
            view.publication,
            GeometryShapeTransform::default(),
            view.path,
            None,
        )];
        let mut candidates = Vec::new();
        while let Some((id, placement, path, inherited_block)) = pending.pop() {
            let publication = self
                .host
                .publication(id)
                .ok_or(GuiQueryUnavailable::SpatialBranch)?;
            if !self.resources_available(publication) {
                return Err(GuiQueryUnavailable::SpatialBranch);
            }
            let branch_block = inherited_block.or_else(|| self.branch_block(publication.world));
            let local = placement.inverse_ray(&ray.ray);
            if let Some(geometry) = publication
                .chunk(GeometrySystem::ID)
                .and_then(|chunk| chunk.data::<GeometryPublication>())
            {
                // Only explicit blockers can occlude, so look each one up in the
                // entity-ordered publication instead of scanning its geometry.
                for blocker in self
                    .options
                    .blockers
                    .iter()
                    .filter(|blocker| blocker.world == publication.world)
                {
                    let Ok(index) = geometry
                        .entities
                        .binary_search_by(|geometry| geometry.entity.cmp(&blocker.entity))
                    else {
                        continue;
                    };
                    let geometry = &geometry.entities[index];
                    if Some(blocker.incarnation) != geometry.picking_incarnation {
                        continue;
                    }
                    let picking = geometry
                        .picking
                        .as_ref()
                        .ok_or(GuiQueryUnavailable::Data(ErrorReason::InvalidGeometry))?
                        .as_ref()
                        .map_err(|error| GuiQueryUnavailable::Data(*error))?;
                    if let Some(hit) = picking.ray_intersection(&local, ray.near, ray.far) {
                        candidates.push(Candidate {
                            distance: hit.distance,
                            blocker: true,
                            world: publication.world,
                            entity: geometry.entity,
                            task: QueryTask::Outcome(GuiQueryOutcome::Blocked {
                                reason: GuiQueryBlockReason::PickingGeometry,
                                path: path.clone(),
                            }),
                        });
                    }
                }
            }
            for edge in &publication.attachments {
                if edge
                    .placement_output
                    .is_some_and(|owner| owner != view.output)
                {
                    continue;
                }
                let affine = GeometryShapeTransform::new(edge.placement)
                    .and_then(|affine| affine.then(&placement))
                    .map_err(GuiQueryUnavailable::Data)?;
                let mut child_path = path.clone();
                child_path.push(GuiQueryStep {
                    token: edge.token.clone(),
                    publication: id,
                    distance: None,
                });
                if edge.mode == WorldAttachmentMode::Spatial {
                    let child = self
                        .host
                        .attached_publication(edge)
                        .ok_or(GuiQueryUnavailable::SpatialBranch)?;
                    self.worlds.enter(child.world)?;
                    pending.push((child.id, affine, child_path, branch_block));
                    continue;
                }
                let Some(extent) = edge.surface_extent else {
                    continue;
                };
                if !extent.iter().all(|value| value.is_finite() && *value > 0.0) {
                    return Err(GuiQueryUnavailable::Data(ErrorReason::InvalidGeometry));
                }
                let local = affine.inverse_ray(&ray.ray);
                if local.direction[2] >= 0.0 {
                    continue;
                }
                let child = self.host.attached_publication(edge);
                // A Surface separating its canvas's layers offers one plane per
                // layer in use, at its id times the spacing, each holding only
                // that layer's targets; candidates then meet the planes nearest
                // first and fall through empty ones.
                let layers = layer_planes(self.host, edge, child);
                for &layer in layers {
                    let plane = f64::from(layer) * f64::from(edge.layer_spacing);
                    let distance = (plane - local.origin[2]) / local.direction[2];
                    if !distance.is_finite() || distance < ray.near || distance > ray.far {
                        continue;
                    }
                    let point = [
                        (local.origin[0] + distance * local.direction[0]) / extent[0] + 0.5,
                        0.5 - (local.origin[1] + distance * local.direction[1]) / extent[1],
                    ];
                    if !point.iter().all(|value| (0.0..1.0).contains(value)) {
                        continue;
                    }
                    let mut path = child_path.clone();
                    path.last_mut().expect("Surface step").distance = Some(distance);
                    let task = match (branch_block, child, edge.output) {
                        (None, Some(child), Some(output)) => QueryTask::View(QueryView {
                            output,
                            publication: child.id,
                            point: point.map(|value| value as f32),
                            extent,
                            path,
                            layer: (layers.len() > 1).then_some(layer),
                        }),
                        (reason, _, _) => QueryTask::Outcome(GuiQueryOutcome::Blocked {
                            reason: reason.unwrap_or(GuiQueryBlockReason::Unavailable),
                            path,
                        }),
                    };
                    candidates.push(Candidate {
                        distance,
                        blocker: false,
                        world: publication.world,
                        entity: edge.anchor,
                        task,
                    });
                }
            }
        }
        candidates.sort_by(|left, right| {
            left.distance
                .total_cmp(&right.distance)
                .then(right.blocker.cmp(&left.blocker))
                .then(left.world.cmp(&right.world))
                .then(left.entity.cmp(&right.entity))
        });
        self.pending
            .extend(candidates.into_iter().rev().map(|candidate| candidate.task));
        Ok(())
    }
}

/// Layers whose planes a Surface edge in a camera's domain presents: those its
/// canvas uses when its layer spacing separates them, otherwise the base plane.
pub(super) fn layer_planes<'a>(
    host: &'a crate::HostRuntime,
    edge: &crate::PublishedWorldAttachment,
    child: Option<&crate::WorldPublication>,
) -> &'a [u32] {
    const BASE: &[u32] = &[0];
    if edge.layer_spacing == 0.0 {
        return BASE;
    }
    child
        .zip(edge.output)
        .and_then(|(child, output)| host.output(child.id, output))
        .and_then(|chunk| chunk.data::<crate::systems::canvas::CanvasPublication>())
        .map_or(BASE, |canvas| &canvas.layers)
}
