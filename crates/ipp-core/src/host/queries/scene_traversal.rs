//! Reusable depth-first traversal over immutable spatial publications.

use crate::systems::geometry::GeometryShapeTransform;
use crate::{
    EntityId, ErrorReason, HostRuntime, OutputKind, OutputRef, WorldAttachmentMode,
    WorldPublication, WorldPublicationId, WorldRef,
};

/// One borrowed visit; the path is valid until traversal advances.
pub struct PublishedSceneVisit<'host, 'path> {
    /// Exact completed source, independent of the traversal's scratch lifetime.
    pub publication: &'host WorldPublication,
    /// World-local to containing-domain placement.
    pub placement: GeometryShapeTransform,
    /// Ancestor attachment path, excluding the visited World.
    pub path: &'path [(WorldRef, EntityId)],
}

struct SceneTraversalFrame {
    publication: WorldPublicationId,
    placement: GeometryShapeTransform,
    child: usize,
    entered: bool,
}

/// Caller-owned traversal buffers; no component or publication borrows survive a visit.
#[derive(Default)]
pub struct PublishedSceneTraversal {
    selection: Option<OutputRef>,
    stack: Vec<SceneTraversalFrame>,
    path: Vec<(WorldRef, EntityId)>,
}

impl PublishedSceneTraversal {
    /// Begin an unscoped spatial domain, or one exact historical Camera output.
    pub fn reset(
        &mut self,
        host: &HostRuntime,
        publication: WorldPublicationId,
        selection: Option<OutputRef>,
    ) -> Result<(), ErrorReason> {
        self.stack.clear();
        self.path.clear();
        self.selection = selection;
        if selection.is_some_and(|selection| {
            selection.kind() != OutputKind::Camera || host.output(publication, selection).is_none()
        }) {
            return Err(ErrorReason::InvalidEntity);
        }
        self.stack.push(SceneTraversalFrame {
            publication,
            placement: Default::default(),
            child: 0,
            entered: false,
        });
        Ok(())
    }

    /// Visit in parent-first attachment order, revalidating each exact edge.
    pub fn next<'host, 'path>(
        &'path mut self,
        host: &'host HostRuntime,
    ) -> Option<PublishedSceneVisit<'host, 'path>> {
        loop {
            let frame = self.stack.last_mut()?;
            let Some(publication) = host.publication(frame.publication) else {
                self.pop();
                continue;
            };
            if !frame.entered {
                frame.entered = true;
                return Some(PublishedSceneVisit {
                    publication,
                    placement: frame.placement,
                    path: &self.path,
                });
            }
            let Some(edge) = publication.attachments.get(frame.child) else {
                self.pop();
                continue;
            };
            frame.child += 1;
            if edge.mode != WorldAttachmentMode::Spatial
                || edge
                    .placement_output
                    .is_some_and(|owner| Some(owner) != self.selection)
            {
                continue;
            }
            let Some(child) = host.attached_publication(edge) else {
                continue;
            };
            let Ok(placement) = GeometryShapeTransform::new(super::super::frame::multiply(
                frame.placement.matrix(),
                edge.placement,
            )) else {
                continue;
            };
            self.path.push((publication.world, edge.anchor));
            self.stack.push(SceneTraversalFrame {
                publication: child.id,
                placement,
                child: 0,
                entered: false,
            });
        }
    }

    fn pop(&mut self) {
        self.stack.pop();
        self.path.truncate(self.stack.len().saturating_sub(1));
    }
}
