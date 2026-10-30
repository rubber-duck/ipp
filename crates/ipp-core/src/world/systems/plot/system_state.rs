use super::*;
use crate::EntityId;
use crate::systems::canvas::{CanvasPrimitive, CanvasTarget};
use crate::systems::data_bindings::DataBindingPresentationConsumer;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Transient derived state only; authored chart values are never copied here.
#[derive(Default)]
pub(super) struct PlotSystemState {
    pub members: Vec<PlotMember>,
    pub visible_members: Vec<usize>,
    pub visibility_dirty: bool,
    pub charts: BTreeMap<(EntityId, u16), PlotRetainedChart>,
    pub revision: u64,
}

/// Membership changes only at committed component lifecycle boundaries.
/// Sorted by entity/component; dirty work survives a closed presentation gate.
#[derive(Clone, Copy)]
pub(super) struct PlotMember {
    pub key: (EntityId, u16),
    pub dirty: bool,
}

pub(super) struct PlotRetainedChart {
    pub target: CanvasTarget,
    pub consumer: DataBindingPresentationConsumer,
    pub geometry: Arc<PlotPreparedGeometry>,
    pub canvas: Arc<[CanvasPrimitive]>,
    pub planes: Arc<[PlotPublishedPlane]>,
}

impl PlotSystemState {
    pub fn membership(&mut self, key: (EntityId, u16), present: bool) {
        self.visibility_dirty = true;
        match (
            self.members.binary_search_by_key(&key, |member| member.key),
            present,
        ) {
            (Ok(index), true) => self.members[index].dirty = true,
            (Ok(index), false) => {
                self.members.remove(index);
            }
            (Err(index), true) => self.members.insert(
                index,
                PlotMember {
                    key,
                    dirty: true,
                },
            ),
            (Err(_), false) => {}
        }
    }

    pub fn dirty(&mut self, entity: EntityId) {
        let start = self.members.partition_point(|member| member.key.0 < entity);
        for member in &mut self.members[start..] {
            if member.key.0 != entity {
                break;
            }
            member.dirty = true;
        }
    }

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
