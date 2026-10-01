//! Exact-target membership pages and borrowed, singly owned lifecycle delivery.

use crate::{
    MAX_MESSAGE_BYTES, ProtocolError,
    codec::{Reader, Writer},
    references::WorldReference,
    wire::*,
};
use ipp_core::{EntityId, components::schema::FieldValue, systems::lifecycle_publisher::*};

/// Fixed encoded ACK allowance, excluding individual baselines and Host metadata.
pub const LIFECYCLE_ACK_BYTES: usize = 128;

/// Maximum encoded storage for one baseline, independent of target-native layout.
pub const LIFECYCLE_BASELINE_BYTES: usize = 48;

/// One request/ACK page's framing bound, not a limit on the live target set.
pub const MAX_LIFECYCLE_MEMBERS: usize =
    (MAX_MESSAGE_BYTES - LIFECYCLE_ACK_BYTES) / LIFECYCLE_BASELINE_BYTES;

const _: () = assert!(2728 == (128 * 1024 - LIFECYCLE_ACK_BYTES) / LIFECYCLE_BASELINE_BYTES);

/// Most schema fields one value target observes.
pub const MAX_LIFECYCLE_VALUE_FIELDS: usize = 64;

/// Encoded bytes each value-target field offset adds to a baseline.
pub const LIFECYCLE_BASELINE_FIELD_BYTES: usize = 4;

/// Peak wire capacities supplied to Core before retaining any typed output.
pub const LIFECYCLE_WATCH_ENCODING: LifecycleWatchEncoding = LifecycleWatchEncoding {
    acknowledgement_bytes: LIFECYCLE_ACK_BYTES,
    baseline_bytes: LIFECYCLE_BASELINE_BYTES,
    event_bytes: 128,
    baseline_field_bytes: LIFECYCLE_BASELINE_FIELD_BYTES,
    // Envelope, generation, tick, presence and count: 71 bytes.
    value_bytes: 96,
    // Offset, kind and the largest fixed payload, an output reference: 39 bytes.
    value_field_bytes: 40,
};

/// A bounded membership page for the exact World selected by the outer session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleWatchRequest {
    /// Untrusted World identity; the Host validates the session match at admission.
    pub world: WorldReference,
    /// Ordered add or removal page, with one sole typed ACK.
    pub change: LifecycleWatchChange,
}

/// Wire identities only; Core member handles never escape their owning adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecycleWatchChange {
    /// Mint fresh registrations in selection order, including duplicate targets.
    Add(Vec<(LifecycleWatchTarget, LifecycleWatchKinds)>),
    /// Remove only the sorted unique acknowledged generations in this page.
    Remove {
        /// Exact endpoint lifetime, not a reusable World-local index.
        output: u64,
        /// Strictly increasing nonzero member identities.
        generations: Vec<u64>,
    },
}

impl Reader<'_> {
    pub(crate) fn lifecycle_watch_request(
        &mut self,
    ) -> Result<LifecycleWatchRequest, ProtocolError> {
        let world = self.world_reference()?;
        if world.id == 0 || world.incarnation == 0 {
            return Err(ProtocolError::Malformed("lifecycle World"));
        }
        let change = match self.u8()? {
            LIFECYCLE_WATCH_ADD => {
                let count = self.count(MAX_LIFECYCLE_MEMBERS)?;
                if count == 0 || count > (self.bytes.len() - self.at) / 10 {
                    return Err(ProtocolError::Malformed("lifecycle add page"));
                }
                // The sole ACK echoes every target, so value fields must fit its page too.
                let mut acknowledgement = LIFECYCLE_ACK_BYTES;
                let mut targets = Vec::with_capacity(count);
                for _ in 0..count {
                    let tag = self.u8()?;
                    let entity = EntityId::from_bits(self.u64()?);
                    if entity.to_bits() == 0 {
                        return Err(ProtocolError::Malformed("lifecycle target"));
                    }
                    let (target, mask) = match tag {
                        LIFECYCLE_WATCH_ENTITY => (
                            LifecycleWatchTarget::Entity(entity),
                            LIFECYCLE_WATCH_ENTITY_CREATED
                                | LIFECYCLE_WATCH_ENTITY_METADATA_CHANGED
                                | LIFECYCLE_WATCH_ENTITY_DELETED,
                        ),
                        LIFECYCLE_WATCH_COMPONENT => {
                            let component = self.u16()?;
                            if component == 0 {
                                return Err(ProtocolError::Malformed("lifecycle component"));
                            }
                            (
                                LifecycleWatchTarget::Component(entity, component),
                                LIFECYCLE_WATCH_COMPONENT_INSERTED
                                    | LIFECYCLE_WATCH_COMPONENT_UPDATED
                                    | LIFECYCLE_WATCH_COMPONENT_REPLACED
                                    | LIFECYCLE_WATCH_COMPONENT_REMOVED,
                            )
                        }
                        LIFECYCLE_WATCH_VALUE => {
                            let component = self.u16()?;
                            if component == 0 {
                                return Err(ProtocolError::Malformed("lifecycle component"));
                            }
                            let fields = self.count(MAX_LIFECYCLE_VALUE_FIELDS)?;
                            if fields == 0 {
                                return Err(ProtocolError::Malformed("lifecycle value fields"));
                            }
                            let mut offsets = Vec::with_capacity(fields);
                            for _ in 0..fields {
                                let offset = self.u32()?;
                                if offsets.last().is_some_and(|previous| *previous >= offset) {
                                    return Err(ProtocolError::Malformed(
                                        "unsorted lifecycle value fields",
                                    ));
                                }
                                offsets.push(offset);
                            }
                            (
                                LifecycleWatchTarget::Value(entity, component, offsets.into()),
                                LIFECYCLE_WATCH_VALUE_CHANGED,
                            )
                        }
                        _ => return Err(ProtocolError::Malformed("lifecycle target kind")),
                    };
                    acknowledgement = LIFECYCLE_WATCH_ENCODING
                        .baseline_capacity(&target)
                        .and_then(|bytes| acknowledgement.checked_add(bytes))
                        .filter(|bytes| *bytes <= MAX_MESSAGE_BYTES)
                        .ok_or(ProtocolError::Limit("lifecycle add page"))?;
                    let bits = self.u8()?;
                    let kinds = LifecycleWatchKinds::from_bits(bits)
                        .filter(|_| bits & !mask == 0)
                        .ok_or(ProtocolError::Malformed("lifecycle kinds"))?;
                    targets.push((target, kinds));
                }
                LifecycleWatchChange::Add(targets)
            }
            LIFECYCLE_WATCH_REMOVE => {
                let output = self.lifecycle_subscription_id()?;
                let count = self.count(MAX_LIFECYCLE_MEMBERS)?;
                if count == 0 || count > (self.bytes.len() - self.at) / 8 {
                    return Err(ProtocolError::Malformed("lifecycle remove page"));
                }
                let mut generations = Vec::with_capacity(count);
                for _ in 0..count {
                    let generation = self.lifecycle_subscription_id()?;
                    if generations
                        .last()
                        .is_some_and(|previous| *previous >= generation)
                    {
                        return Err(ProtocolError::Malformed("unsorted lifecycle members"));
                    }
                    generations.push(generation);
                }
                LifecycleWatchChange::Remove {
                    output,
                    generations,
                }
            }
            _ => return Err(ProtocolError::Malformed("lifecycle membership action")),
        };
        Ok(LifecycleWatchRequest {
            world,
            change,
        })
    }
}

fn write_record(
    writer: &mut Writer,
    output: u64,
    record: &LifecycleWatchRecord,
) -> Result<(), ProtocolError> {
    if output == 0 || record.session == 0 {
        return Err(ProtocolError::Malformed("lifecycle endpoint"));
    }
    let request = match &record.body {
        LifecycleWatchRecordBody::Acknowledgement {
            request,
            ..
        } => {
            if *request == 0 {
                return Err(ProtocolError::Malformed("lifecycle correlation"));
            }
            *request
        }
        _ => 0,
    };
    writer.u64(record.session)?;
    writer.u64(request)?;
    writer.u64(0)?;
    writer.u8(RESPONSE_LIFECYCLE_WATCH)?;
    writer.world_reference(record.world.into())?;
    writer.u64(output)?;
    match &record.body {
        LifecycleWatchRecordBody::Acknowledgement {
            action,
            cut,
            result,
            ..
        } => {
            writer.u8(LIFECYCLE_WATCH_ACK)?;
            writer.u8(match action {
                LifecycleMembershipAction::Add => LIFECYCLE_WATCH_ADD,
                LifecycleMembershipAction::Remove => LIFECYCLE_WATCH_REMOVE,
            })?;
            writer.u8(u8::from(cut.is_some()))?;
            if let Some((sequence, tick)) = cut {
                writer.u64(*sequence)?;
                writer.u64(*tick)?;
            }
            match result {
                LifecycleMembershipResult::Applied(baselines) => {
                    writer.u8(LIFECYCLE_MEMBERSHIP_APPLIED)?;
                    writer.count(baselines.len(), MAX_LIFECYCLE_MEMBERS)?;
                    for baseline in baselines {
                        if baseline.member.output != output || baseline.member.generation == 0 {
                            return Err(ProtocolError::Malformed("lifecycle baseline endpoint"));
                        }
                        writer.u64(baseline.member.generation)?;
                        writer.lifecycle_watch_target(&baseline.target)?;
                        match baseline.lifetime {
                            LifecycleTargetLifetime::Entity {
                                live,
                            } => {
                                writer.u8(LIFECYCLE_LIFETIME_ENTITY)?;
                                writer.u8(u8::from(live))?;
                            }
                            LifecycleTargetLifetime::Component {
                                entity_live,
                                incarnation,
                            } => {
                                writer.u8(LIFECYCLE_LIFETIME_COMPONENT)?;
                                writer.u8(u8::from(entity_live))?;
                                writer.u64(incarnation.unwrap_or(0))?;
                            }
                            LifecycleTargetLifetime::Removed => {
                                writer.u8(LIFECYCLE_LIFETIME_REMOVED)?
                            }
                        }
                    }
                }
                LifecycleMembershipResult::Rejected(reason) => {
                    writer.u8(LIFECYCLE_MEMBERSHIP_REJECTED)?;
                    writer.u8(match reason {
                        LifecycleMembershipRejection::StaleWorld => {
                            LIFECYCLE_MEMBERSHIP_STALE_WORLD
                        }
                        LifecycleMembershipRejection::StaleSession => {
                            LIFECYCLE_MEMBERSHIP_STALE_SESSION
                        }
                        LifecycleMembershipRejection::StaleMember => {
                            LIFECYCLE_MEMBERSHIP_STALE_MEMBER
                        }
                        LifecycleMembershipRejection::AlreadyActive => {
                            LIFECYCLE_MEMBERSHIP_ALREADY_ACTIVE
                        }
                        LifecycleMembershipRejection::TrackingEnded => {
                            LIFECYCLE_MEMBERSHIP_TRACKING_ENDED
                        }
                        LifecycleMembershipRejection::Capacity => LIFECYCLE_MEMBERSHIP_CAPACITY,
                    })?;
                }
                LifecycleMembershipResult::Cancelled => {
                    writer.u8(LIFECYCLE_MEMBERSHIP_CANCELLED)?
                }
            }
        }
        LifecycleWatchRecordBody::Event {
            member,
            sequence,
            tick,
            observation,
        } => {
            if member.output != output || member.generation == 0 || *sequence == 0 {
                return Err(ProtocolError::Malformed("lifecycle event identity"));
            }
            writer.u8(LIFECYCLE_WATCH_EVENT)?;
            writer.u64(member.generation)?;
            writer.u64(*sequence)?;
            writer.u64(*tick)?;
            match observation {
                LifecycleObservation::Entity {
                    entity,
                    kind,
                } => {
                    writer.u8(match kind {
                        EntityLifecycleKind::Created => LIFECYCLE_ENTITY_CREATED,
                        EntityLifecycleKind::MetadataChanged => LIFECYCLE_ENTITY_METADATA_CHANGED,
                        EntityLifecycleKind::Deleted => LIFECYCLE_ENTITY_DELETED,
                    })?;
                    writer.u64(entity.to_bits())?;
                }
                LifecycleObservation::Component {
                    entity,
                    component,
                    kind,
                    previous_incarnation,
                    incarnation,
                } => {
                    writer.u8(match kind {
                        ComponentLifecycleKind::Inserted => LIFECYCLE_COMPONENT_INSERTED,
                        ComponentLifecycleKind::Updated => LIFECYCLE_COMPONENT_UPDATED,
                        ComponentLifecycleKind::Replaced => LIFECYCLE_COMPONENT_REPLACED,
                        ComponentLifecycleKind::Removed => LIFECYCLE_COMPONENT_REMOVED,
                    })?;
                    writer.u64(entity.to_bits())?;
                    writer.u16(*component)?;
                    writer.u64(previous_incarnation.unwrap_or(0))?;
                    writer.u64(incarnation.unwrap_or(0))?;
                }
                LifecycleObservation::Asset {
                    ..
                } => {
                    return Err(ProtocolError::Malformed(
                        "asset in targeted lifecycle output",
                    ));
                }
            }
        }
        LifecycleWatchRecordBody::Value {
            member,
            tick,
            values,
        } => {
            if member.output != output || member.generation == 0 {
                return Err(ProtocolError::Malformed("lifecycle value identity"));
            }
            writer.u8(LIFECYCLE_WATCH_VALUE_RECORD)?;
            writer.u64(member.generation)?;
            writer.u64(*tick)?;
            writer.u8(u8::from(values.is_some()))?;
            if let Some(values) = values {
                writer.count(values.len(), MAX_LIFECYCLE_VALUE_FIELDS)?;
                for (offset, value) in values {
                    if matches!(value, FieldValue::Rows(_) | FieldValue::Dynamic(_)) {
                        return Err(ProtocolError::Malformed("lifecycle value kind"));
                    }
                    writer.resolved_field(*offset, value.clone())?;
                }
            }
        }
    }
    Ok(())
}

impl Writer {
    fn lifecycle_watch_target(
        &mut self,
        target: &LifecycleWatchTarget,
    ) -> Result<(), ProtocolError> {
        match target {
            LifecycleWatchTarget::Entity(entity) => {
                self.u8(LIFECYCLE_WATCH_ENTITY)?;
                self.u64(entity.to_bits())?;
            }
            LifecycleWatchTarget::Component(entity, component) => {
                self.u8(LIFECYCLE_WATCH_COMPONENT)?;
                self.u64(entity.to_bits())?;
                self.u16(*component)?;
            }
            LifecycleWatchTarget::Value(entity, component, fields) => {
                self.u8(LIFECYCLE_WATCH_VALUE)?;
                self.u64(entity.to_bits())?;
                self.u16(*component)?;
                self.count(fields.len(), MAX_LIFECYCLE_VALUE_FIELDS)?;
                for offset in fields.iter() {
                    self.u32(*offset)?;
                }
            }
        }
        Ok(())
    }
}

/// Validate and measure borrowed output without copying its retained payload.
pub fn encoded_size(output: u64, record: &LifecycleWatchRecord) -> Result<usize, ProtocolError> {
    let mut writer = Writer::measuring();
    write_record(&mut writer, output, record)?;
    Ok(writer.len())
}

/// Encode into already-reserved storage; insufficient capacity never allocates.
pub fn encode_into(
    output: u64,
    record: &LifecycleWatchRecord,
    bytes: &mut Vec<u8>,
) -> Result<(), ProtocolError> {
    let size = encoded_size(output, record)?;
    if bytes.capacity() < size {
        return Err(ProtocolError::Limit("lifecycle reserved capacity"));
    }
    bytes.clear();
    let mut writer = Writer::new(std::mem::take(bytes));
    let result = write_record(&mut writer, output, record);
    *bytes = writer.0;
    if result.is_err() {
        bytes.clear();
    }
    result
}
