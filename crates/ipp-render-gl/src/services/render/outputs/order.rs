//! Child-first output traversal with bounded reusable working buffers.

use super::super::canvas::scene::CanvasScene;
use crate::RenderError;
use ipp_core::{HostRuntime, OutputKind, OutputRef, WorldAttachmentMode, WorldPublicationId};
use std::{cell::RefCell, rc::Rc};

type Entry = (OutputRef, WorldPublicationId);
pub(in crate::services::render) type OutputOrderPool = Rc<RefCell<Vec<OutputOrderStorage>>>;

#[derive(Default)]
pub(in crate::services::render) struct OutputOrderStorage {
    order: Vec<Entry>,
    pending: Vec<(OutputRef, WorldPublicationId, bool)>,
    children: Vec<Entry>,
    traversal: ipp_core::PublishedSceneTraversal,
}

pub(in crate::services::render) struct OutputOrder {
    storage: OutputOrderStorage,
    pool: OutputOrderPool,
    cursor: usize,
}

impl std::ops::Deref for OutputOrder {
    type Target = [Entry];

    fn deref(&self) -> &Self::Target {
        &self.storage.order[self.cursor..]
    }
}

impl Iterator for OutputOrder {
    type Item = Entry;

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.storage.order.get(self.cursor).copied()?;
        self.cursor += 1;
        Some(value)
    }
}

impl Drop for OutputOrder {
    fn drop(&mut self) {
        self.pool
            .borrow_mut()
            .push(std::mem::take(&mut self.storage));
    }
}

pub(in crate::services::render) fn output_order(
    host: &HostRuntime,
    selection: OutputRef,
    publication: WorldPublicationId,
    pool: OutputOrderPool,
) -> Result<OutputOrder, RenderError> {
    if host.output(publication, selection).is_none() {
        return Err(RenderError::UnavailableOutput);
    }
    let mut result = OutputOrder {
        storage: pool.borrow_mut().pop().unwrap_or_default(),
        pool: pool.clone(),
        cursor: 0,
    };
    let OutputOrderStorage {
        order,
        pending,
        children,
        traversal,
    } = &mut result.storage;
    order.clear();
    pending.clear();
    pending.push((selection, publication, false));
    while let Some((selection, publication, visited)) = pending.pop() {
        if visited {
            order.push((selection, publication));
            continue;
        }
        children.clear();
        match selection.kind() {
            OutputKind::Canvas => children.extend(
                CanvasScene::new(host, selection, publication)?
                    .attachments()
                    .filter(|attachment| attachment.slot.opacity > 0.0)
                    .filter_map(|attachment| attachment.output()),
            ),
            OutputKind::Camera => {
                traversal
                    .reset(host, publication, Some(selection))
                    .map_err(|_| RenderError::UnavailableOutput)?;
                while let Some(contribution) = traversal.next(host) {
                    children.extend(contribution.publication.attachments.iter().filter_map(
                        |edge| {
                            if edge.mode == WorldAttachmentMode::Spatial
                                || edge
                                    .placement_output
                                    .is_some_and(|owner| owner != selection)
                            {
                                return None;
                            }
                            Some((edge.output?, host.attached_publication(edge)?.id))
                        },
                    ));
                }
            }
        }
        pending.push((selection, publication, true));
        pending.extend(
            children
                .iter()
                .copied()
                .rev()
                .map(|(selection, publication)| (selection, publication, false)),
        );
    }
    Ok(result)
}
