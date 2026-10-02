use super::traversal::{QueryTask, QueryView, QueryWalk};
use super::*;
use crate::WorldAttachmentMode;
use crate::systems::canvas::{CanvasHitKind, CanvasPaintEntry, CanvasPublication, CanvasSystem};
use crate::systems::gui::presentation::GuiCanvasPublication;

impl<'a> QueryWalk<'a, '_> {
    pub(super) fn canvas(&mut self, view: QueryView) -> Result<(), GuiQueryUnavailable> {
        let publication = self
            .host
            .publication(view.publication)
            .ok_or(GuiQueryUnavailable::Data(ErrorReason::InvalidEntity))?;
        let canvas = self
            .host
            .output(view.publication, view.output)
            .and_then(|chunk| chunk.data::<CanvasPublication>())
            .ok_or(GuiQueryUnavailable::Data(ErrorReason::InvalidEntity))?;
        let point = std::array::from_fn(|axis| view.point[axis] * canvas.logical_extent[axis]);
        if !point
            .iter()
            .enumerate()
            .all(|(axis, value)| *value >= 0.0 && *value < canvas.logical_extent[axis])
        {
            return Ok(());
        }
        let controls = publication
            .chunk(CanvasSystem::ID)
            .and_then(|chunk| chunk.data::<GuiCanvasPublication>())
            .and_then(|gui| gui.views.get(&view.output));
        for hit in canvas.hits.iter() {
            // A layer plane of a Surface separating layers holds only that layer's targets.
            if view.layer.is_some_and(|layer| hit.layer != layer) || !hit.contains(point) {
                continue;
            }
            let CanvasHitKind::Attachment {
                anchor,
                token,
            } = &hit.kind
            else {
                let control = controls.and_then(|controls| controls.control(hit.target));
                let outcome = if control.is_some_and(|control| !control.available) {
                    GuiQueryOutcome::Blocked {
                        reason: GuiQueryBlockReason::Unavailable,
                        path: view.path.clone(),
                    }
                } else {
                    GuiQueryOutcome::Hit(GuiQueryHit {
                        output: view.output,
                        publication: view.publication,
                        point,
                        hit,
                        control,
                        path: view.path.clone(),
                    })
                };
                self.pending.push(QueryTask::Outcome(outcome));
                continue;
            };
            let slot = canvas
                .entries
                .iter()
                .find_map(|entry| match entry.as_ref() {
                    CanvasPaintEntry::Attachment(slot)
                        if slot.anchor == *anchor && slot.token == *token =>
                    {
                        Some(slot)
                    }
                    _ => None,
                })
                .ok_or(GuiQueryUnavailable::Data(ErrorReason::InvalidGeometry))?;
            if slot.opacity <= 0.0 {
                continue;
            }
            let local = slot
                .from_canvas(point.map(f64::from))
                .ok_or(GuiQueryUnavailable::Data(ErrorReason::InvalidGeometry))?;
            let child_point = [
                local[0] / slot.physical_extent[0] + 0.5,
                0.5 - local[1] / slot.physical_extent[1],
            ];
            if !child_point.iter().all(|value| (0.0..1.0).contains(value)) {
                continue;
            }
            let mut path = view.path.clone();
            path.push(GuiQueryStep {
                token: token.clone(),
                publication: view.publication,
                distance: None,
            });
            let edge = publication.attachments.iter().find(|edge| {
                edge.anchor == *anchor
                    && edge.token == *token
                    && edge.placement_output == Some(view.output)
                    && edge.mode != WorldAttachmentMode::Spatial
                    && edge.surface_extent == Some(slot.physical_extent)
            });
            let child = edge.and_then(|edge| self.host.attached_publication(edge));
            let task = match (child, edge.and_then(|edge| edge.output)) {
                (Some(child), Some(output)) => QueryTask::View(QueryView {
                    output,
                    publication: child.id,
                    point: child_point.map(|value| value as f32),
                    extent: slot.physical_extent,
                    path,
                    layer: None,
                }),
                _ => QueryTask::Outcome(GuiQueryOutcome::Blocked {
                    reason: GuiQueryBlockReason::Unavailable,
                    path,
                }),
            };
            self.pending.push(task);
        }
        Ok(())
    }
}
