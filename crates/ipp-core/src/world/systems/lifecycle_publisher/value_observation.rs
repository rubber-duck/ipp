//! Frame-end comparison of value members with the values last reported to them.

use super::output::{LifecycleMemberStatus, LifecycleOutputState};
use super::*;
use crate::components::schema::{FieldValue, same_text};
use crate::services::reliable_output::{
    OutputCharge, OutputFailure, OutputReserveError, ReliableOutputLease,
};
use crate::world::systems::SystemWorldView;
use std::mem::size_of;

/// What a value member last reported, compared with the stored values at each frame end.
enum LifecycleReportedValues {
    /// Nothing yet, so the first observation after the ACK reports the current values.
    Unreported,
    /// The entity or component was absent.
    Absent,
    /// The values in the target's field order.
    Present(Box<[FieldValue]>),
}

impl LifecycleReportedValues {
    fn retained_bytes(&self) -> usize {
        match self {
            Self::Unreported | Self::Absent => 0,
            Self::Present(values) => values
                .iter()
                .fold(values.len() * size_of::<FieldValue>(), |bytes, value| {
                    bytes.saturating_add(value_heap_bytes(value))
                }),
        }
    }
}

/// An active value member and the last values reported to it.
pub(super) struct LifecycleValueMember {
    member: LifecycleWatchMember,
    reported: LifecycleReportedValues,
    /// Charge for the retained reported values, taken with the first report.
    retained: Option<ReliableOutputLease>,
    /// Stamp of this member's undelivered record in its endpoint queue.
    pending: Option<u64>,
}

impl LifecycleValueMember {
    pub fn new(member: LifecycleWatchMember) -> Self {
        Self {
            member,
            reported: LifecycleReportedValues::Unreported,
            retained: None,
            pending: None,
        }
    }

    /// Report the final stored values of `tick` when they differ from the last report.
    /// An unchanged member reads its fields in place and allocates nothing.
    pub fn observe(&mut self, world: SystemWorldView<'_>, tick: u64) {
        if self.member.0.status.get() != LifecycleMemberStatus::Active {
            return;
        }

        let Some(output) = self.member.0.output.upgrade() else {
            return;
        };
        if !output.tracking() {
            return;
        }

        let LifecycleWatchTarget::Value(entity, component, fields) = &self.member.0.target else {
            return;
        };
        let target = StoredFields {
            world,
            entity: *entity,
            component: *component,
        };
        if self.unchanged(&target, fields) {
            return;
        }

        let values = target.read(fields);
        let reported = match &values {
            Some(values) => LifecycleReportedValues::Present(
                values.iter().map(|(_, value)| value.clone()).collect(),
            ),
            None => LifecycleReportedValues::Absent,
        };
        if output.observe_value(self.member.id(), &mut self.pending, tick, values) {
            self.remember(&output, reported);
        }
    }

    fn unchanged(&self, target: &StoredFields<'_>, fields: &[u32]) -> bool {
        match &self.reported {
            LifecycleReportedValues::Unreported => false,
            LifecycleReportedValues::Absent => target.field(fields[0]).is_none(),
            LifecycleReportedValues::Present(values) => {
                fields.iter().zip(values.iter()).all(|(&offset, reported)| {
                    target
                        .field(offset)
                        .is_some_and(|current| same_value(&current, reported))
                })
            }
        }
    }

    /// Keep the reported values for later comparison, charged to the connection's account.
    fn remember(&mut self, output: &LifecycleOutputState, reported: LifecycleReportedValues) {
        let charge = OutputCharge {
            entries: 0,
            bytes: reported.retained_bytes(),
        };
        self.reported = reported;

        let result = match &mut self.retained {
            Some(lease) => lease.resize(charge),
            None => output.account.reserve(charge).map(|lease| {
                self.retained = Some(lease);
            }),
        };
        if result == Err(OutputReserveError::Capacity) {
            output.account.fail(OutputFailure::Capacity);
        }
    }
}

/// Reads of one value target's fields from the component store.
struct StoredFields<'a> {
    world: SystemWorldView<'a>,
    entity: crate::EntityId,
    component: u16,
}

impl StoredFields<'_> {
    /// The stored value of one field, or `None` while the entity or component is absent.
    /// The generation check keeps a reused entity slot from answering for this target.
    fn field(&self, offset: u32) -> Option<FieldValue> {
        if !self.world.authored.allocator.contains(self.entity) {
            return None;
        }

        self.world
            .world
            .components
            .field(self.component, self.entity.index() as usize, offset)
    }

    /// Current values in the target's field order, or `None` while absent.
    fn read(&self, fields: &[u32]) -> Option<Vec<(u32, FieldValue)>> {
        let first = self.field(fields[0])?;
        let mut values = Vec::with_capacity(fields.len());
        values.push((fields[0], first));
        for &offset in &fields[1..] {
            values.push((offset, self.field(offset)?));
        }
        Some(values)
    }
}

/// Text compares by shared reference and then content, and floats by their bits, so a
/// stored NaN does not report a change every frame; every other kind compares by value.
fn same_value(current: &FieldValue, reported: &FieldValue) -> bool {
    match (current, reported) {
        (FieldValue::String(current), FieldValue::String(reported)) => same_text(current, reported),
        (FieldValue::F32(current), FieldValue::F32(reported)) => {
            current.to_bits() == reported.to_bits()
        }
        _ => current == reported,
    }
}

/// Heap bytes a field value retains beyond its inline size.
pub(super) fn value_heap_bytes(value: &FieldValue) -> usize {
    match value {
        FieldValue::String(text) => text.len(),
        FieldValue::Bytes(bytes) => bytes.capacity(),
        _ => 0,
    }
}
