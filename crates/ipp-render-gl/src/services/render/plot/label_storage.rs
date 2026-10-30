//! Borrowed Plot presentation buffers, cleared before returning their capacity.

use super::super::outputs::scene::ScenePlotPlane;
use std::{cell::RefCell, rc::Rc};

pub(super) type PlotPlanePool = Rc<RefCell<PlotPlaneStorage<'static>>>;

#[derive(Default)]
pub(super) struct PlotPlaneStorage<'a> {
    placed: Vec<ScenePlotPlane<'a>>,
    arranged: Vec<ScenePlotPlane<'a>>,
}

pub(in crate::services::render) struct PlotPlaneFrame<'a> {
    pub(super) placed: Vec<ScenePlotPlane<'a>>,
    pub(super) arranged: Vec<ScenePlotPlane<'a>>,
    pool: PlotPlanePool,
}

impl<'a> PlotPlaneFrame<'a> {
    pub(super) fn new(pool: PlotPlanePool) -> Self {
        let storage = std::mem::take(&mut *pool.borrow_mut());
        Self {
            placed: empty(storage.placed),
            arranged: empty(storage.arranged),
            pool,
        }
    }
}

impl<'a> std::ops::Deref for PlotPlaneFrame<'a> {
    type Target = [ScenePlotPlane<'a>];

    fn deref(&self) -> &Self::Target {
        &self.arranged
    }
}

impl Drop for PlotPlaneFrame<'_> {
    fn drop(&mut self) {
        *self.pool.borrow_mut() = PlotPlaneStorage {
            placed: empty(std::mem::take(&mut self.placed)),
            arranged: empty(std::mem::take(&mut self.arranged)),
        };
    }
}

// Drop every publication borrow before rebinding the empty equal-layout buffer.
// No lifetime is extended and correctness does not depend on Vec's reuse.
fn empty<'a, 'b>(mut planes: Vec<ScenePlotPlane<'a>>) -> Vec<ScenePlotPlane<'b>> {
    planes.clear();
    planes
        .into_iter()
        .map(|_| unreachable!("empty Plot buffer"))
        .collect()
}
