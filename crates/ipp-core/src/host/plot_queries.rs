//! Plot row queries through completed Canvas placements and exact attachment joins.

use super::*;
use crate::systems::canvas::{CanvasPaintEntry, CanvasPublication};

impl HostRuntime {
    pub(super) fn pick_canvas_plot(
        &self,
        publication: WorldPublicationId,
        output: OutputRef,
        point: [f32; 2],
        path: Vec<(WorldRef, crate::EntityId)>,
        layer: Option<u32>,
    ) -> Result<Option<PublishedSceneHit>, ErrorReason> {
        let source = self
            .publication(publication)
            .ok_or(ErrorReason::InvalidEntity)?;
        let canvas = self
            .output(publication, output)
            .and_then(|chunk| chunk.data::<CanvasPublication>())
            .ok_or(ErrorReason::InvalidEntity)?;
        let mut candidates: Vec<_> = canvas
            .plot_hits
            .iter()
            .enumerate()
            .filter(|(_, hit)| layer.is_none_or(|layer| hit.layer == layer))
            .map(|(index, hit)| (hit.layer, hit.order, true, index))
            .collect();
        candidates.extend(
            canvas
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| match entry.as_ref() {
                    CanvasPaintEntry::Attachment(slot)
                        if layer.is_none_or(|layer| slot.layer == layer) =>
                    {
                        Some((slot.layer, index as u32, false, index))
                    }
                    _ => None,
                }),
        );
        candidates.sort_by_key(|candidate| (candidate.0, candidate.1));
        for (_, _, plot, index) in candidates.into_iter().rev() {
            if plot {
                let mark = &canvas.plot_hits[index];
                if let Some(row) = mark.pick(point) {
                    return Ok(Some(PublishedSceneHit {
                        publication,
                        world: source.world,
                        entity: mark.target.entity,
                        component: mark.target.component,
                        incarnation: mark.target.incarnation,
                        row: Some(row),
                        hit: crate::systems::geometry::GeometryRayHit {
                            distance: 0.0,
                            part: 0,
                        },
                        path,
                    }));
                }
                continue;
            }
            let CanvasPaintEntry::Attachment(slot) = canvas.entries[index].as_ref() else {
                continue;
            };
            if slot.opacity <= 0.0
                || point[0] < slot.clip[0]
                || point[1] < slot.clip[1]
                || point[0] >= slot.clip[2]
                || point[1] >= slot.clip[3]
            {
                continue;
            }
            let Some(physical) = slot.from_canvas(point.map(f64::from)) else {
                continue;
            };
            if physical[0].abs() > slot.physical_extent[0] * 0.5
                || physical[1].abs() > slot.physical_extent[1] * 0.5
            {
                continue;
            }
            let Some(edge) = source.attachments.iter().find(|edge| {
                edge.anchor == slot.anchor
                    && edge.token == slot.token
                    && edge.placement_output == Some(output)
            }) else {
                continue;
            };
            let Some(child) = self.attached_publication(edge) else {
                continue;
            };
            let Some(child_output) = edge.output else {
                continue;
            };
            if child_output.kind() == OutputKind::Camera {
                let camera = self
                    .output(child.id, child_output)
                    .and_then(|chunk| chunk.data::<crate::systems::camera::CameraPublication>())
                    .ok_or(ErrorReason::InvalidEntity)?;
                let normalized = [
                    physical[0] / slot.physical_extent[0] + 0.5,
                    0.5 - physical[1] / slot.physical_extent[1],
                ];
                let ray = camera
                    .ray_for_extent(normalized.map(|value| value as f32), slot.physical_extent)?;
                if let Some(mut hit) =
                    self.pick_publication(child.id, child_output, &ray.ray, ray.near, ray.far)?
                {
                    let mut combined = path.clone();
                    combined.push((source.world, slot.anchor));
                    combined.append(&mut hit.path);
                    hit.path = combined;
                    hit.hit.distance = 0.0;
                    return Ok(Some(hit));
                }
                continue;
            }
            let Some(child_canvas) = self
                .output(child.id, child_output)
                .and_then(|chunk| chunk.data::<CanvasPublication>())
            else {
                continue;
            };
            let normalized = [
                physical[0] / slot.physical_extent[0] + 0.5,
                0.5 - physical[1] / slot.physical_extent[1],
            ];
            let child_point = [
                normalized[0] as f32 * child_canvas.logical_extent[0],
                normalized[1] as f32 * child_canvas.logical_extent[1],
            ];
            let mut child_path = path.clone();
            child_path.push((source.world, slot.anchor));
            if let Some(hit) =
                self.pick_canvas_plot(child.id, child_output, child_point, child_path, None)?
            {
                return Ok(Some(hit));
            }
        }
        Ok(None)
    }
}

impl HostRuntime {
    pub(super) fn pick_plot_surface(
        &self,
        contribution: &PublishedSceneContribution<'_>,
        edge: &PublishedWorldAttachment,
        ray: &crate::systems::geometry::GeometryRay,
        near: f64,
        far: f64,
    ) -> Result<Option<PublishedSceneHit>, ErrorReason> {
        let Some(child) = self.attached_publication(edge) else {
            return Ok(None);
        };
        let Some(output) = edge.output else {
            return Ok(None);
        };
        let Some(extent) = edge.surface_extent else {
            return Ok(None);
        };
        let placement = crate::systems::geometry::GeometryShapeTransform::new(edge.placement)?
            .then(&contribution.placement)?;
        let local = placement.inverse_ray(ray);
        if local.direction[2] >= 0.0 {
            return Ok(None);
        }
        let canvas = self
            .output(child.id, output)
            .and_then(|chunk| chunk.data::<CanvasPublication>());
        let layers = canvas.map_or(&[0][..], |canvas| canvas.layers.as_ref());
        let mut nearest: Option<PublishedSceneHit> = None;
        for &layer in layers.iter().rev() {
            let z = f64::from(layer) * f64::from(edge.layer_spacing);
            let distance = (z - local.origin[2]) / local.direction[2];
            if distance < near
                || distance > far
                || nearest
                    .as_ref()
                    .is_some_and(|hit| hit.hit.distance <= distance)
            {
                continue;
            }
            let physical = [
                local.origin[0] + distance * local.direction[0],
                local.origin[1] + distance * local.direction[1],
            ];
            if physical[0].abs() > extent[0] * 0.5 || physical[1].abs() > extent[1] * 0.5 {
                continue;
            }
            let point = [
                (physical[0] / extent[0] + 0.5) as f32,
                (0.5 - physical[1] / extent[1]) as f32,
            ];
            let mut path = contribution.path.clone();
            path.push((contribution.publication.world, edge.anchor));
            let hit = if let Some(canvas) = canvas {
                self.pick_canvas_plot(
                    child.id,
                    output,
                    [
                        point[0] * canvas.logical_extent[0],
                        point[1] * canvas.logical_extent[1],
                    ],
                    path,
                    (edge.layer_spacing != 0.0).then_some(layer),
                )?
            } else if let Some(camera) = self
                .output(child.id, output)
                .and_then(|chunk| chunk.data::<crate::systems::camera::CameraPublication>())
            {
                let child_ray = camera.ray_for_extent(point, extent)?;
                self.pick_publication(
                    child.id,
                    output,
                    &child_ray.ray,
                    child_ray.near,
                    child_ray.far,
                )?
                .map(|mut hit| {
                    path.append(&mut hit.path);
                    hit.path = path;
                    hit
                })
            } else {
                None
            };
            if let Some(mut hit) = hit {
                hit.hit.distance = distance;
                nearest = Some(hit);
            }
        }
        Ok(nearest)
    }
}
