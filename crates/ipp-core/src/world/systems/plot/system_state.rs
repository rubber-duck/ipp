use super::*;
use crate::EntityId;
use crate::systems::canvas::{CanvasPrimitive, CanvasTarget};
use crate::systems::data_bindings::DataBindingPresentationConsumer;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Transient derived state only; authored chart values are never copied here.
#[derive(Default)]
pub(super) struct PlotSystemState {
    pub members: BTreeSet<(EntityId, u16)>,
    pub dirty: BTreeSet<EntityId>,
    pub charts: BTreeMap<(EntityId, u16), PlotRetainedChart>,
    pub revision: u64,
    pub membership_dirty: bool,
}

pub(super) struct PlotRetainedChart {
    pub target: CanvasTarget,
    pub consumer: DataBindingPresentationConsumer,
    pub geometry: Arc<PlotPreparedGeometry>,
    pub canvas: Arc<[CanvasPrimitive]>,
    pub planes: Arc<[PlotPublishedPlane]>,
}

impl PlotSystemState {
    pub fn changed(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("Plot revision exhausted");
    }

    pub fn remove(&mut self, key: (EntityId, u16)) {
        if self.charts.remove(&key).is_some() {
            self.changed();
        }
    }
}
