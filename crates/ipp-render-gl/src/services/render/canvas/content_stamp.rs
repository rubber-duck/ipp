//! Reusable candidates for image dependency comparisons. Cached stamps stay immutable.

use super::{presentation::PaintContextStamp, scene::OutputContentStamp};
use crate::RenderError;
use ipp_core::{HostRuntime, OutputRef, WorldPublicationId};
use std::{cell::RefCell, rc::Rc};

pub(in crate::services::render) type ContentStampPool = Rc<RefCell<Vec<ContentStampScratch>>>;

#[derive(Default)]
pub(in crate::services::render) struct ContentStampScratch {
    pub stamp: OutputContentStamp,
    pub paint_context: PaintContextStamp,
    pub raster_dependencies: Vec<(OutputRef, u64)>,
    traversal: ipp_core::PublishedSceneTraversal,
}

pub(in crate::services::render) struct ContentStampRead {
    storage: ContentStampScratch,
    pool: ContentStampPool,
}

impl std::ops::Deref for ContentStampRead {
    type Target = ContentStampScratch;

    fn deref(&self) -> &Self::Target {
        &self.storage
    }
}

impl std::ops::DerefMut for ContentStampRead {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.storage
    }
}

impl Drop for ContentStampRead {
    fn drop(&mut self) {
        self.pool
            .borrow_mut()
            .push(std::mem::take(&mut self.storage));
    }
}

pub(in crate::services::render) fn read(
    host: &HostRuntime,
    selection: OutputRef,
    publication: WorldPublicationId,
    order: super::super::outputs::order::OutputOrderPool,
    pool: ContentStampPool,
) -> Result<ContentStampRead, RenderError> {
    let storage = pool.borrow_mut().pop().unwrap_or_default();
    let mut result = ContentStampRead {
        storage,
        pool,
    };
    let ContentStampScratch {
        stamp,
        traversal,
        ..
    } = &mut result.storage;
    stamp.update(host, selection, publication, order, traversal)?;
    Ok(result)
}
