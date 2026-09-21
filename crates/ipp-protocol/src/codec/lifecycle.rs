//! Lifecycle identity encoding; payload storage is never borrowed by the transport.

use super::*;
use ipp_core::systems::lifecycle_publisher::{
    ComponentLifecycleKind, EntityLifecycleKind, LifecycleFilter, LifecycleObservation,
    LifecyclePublisherOutput,
};

impl Reader<'_> {
    pub(super) fn lifecycle_subscription_id(&mut self) -> Result<u64, ProtocolError> {
        let id = self.u64()?;
        if id == 0 {
            return Err(ProtocolError::Malformed("zero subscription identity"));
        }
        Ok(id)
    }

    pub(super) fn lifecycle_filter(&mut self) -> Result<LifecycleFilter, ProtocolError> {
        let domains = self.u16()?;
        let supported = 7;
        if domains == 0 || domains & !supported != 0 {
            return Err(ProtocolError::Malformed("lifecycle domains"));
        }
        let entity = self.u64()?;
        let component = self.u16()?;
        let asset = self.u64()?;
        Ok(LifecycleFilter {
            entities: domains & 1 != 0,
            components: domains & 2 != 0,
            assets: domains & 4 != 0,
            entity: (entity != 0).then(|| EntityId::from_bits(entity)),
            component: (component != 0).then_some(component),
            asset: (asset != 0).then_some(asset),
        })
    }
}

impl Writer {
    pub(super) fn lifecycle_output(
        &mut self,
        output: &LifecyclePublisherOutput,
    ) -> Result<(), ProtocolError> {
        match output {
            LifecyclePublisherOutput::Overflow {
                dropped,
            } => {
                if *dropped == 0 {
                    return Err(ProtocolError::Malformed("empty lifecycle overflow"));
                }
                self.u8(RESPONSE_LIFECYCLE_OVERFLOW)?;
                self.u64(*dropped)?;
            }
            LifecyclePublisherOutput::Events(events) => {
                if events.is_empty() {
                    return Err(ProtocolError::Malformed("empty lifecycle event"));
                }
                self.u8(RESPONSE_LIFECYCLE_EVENTS)?;
                self.count(events.len(), 128)?;
                for event in events {
                    if event.subscription == 0 || event.sequence == 0 {
                        return Err(ProtocolError::Malformed("lifecycle event identity"));
                    }
                    self.u64(event.subscription)?;
                    self.u64(event.sequence)?;
                    self.u64(event.tick)?;
                    match &event.observation {
                        LifecycleObservation::Entity {
                            entity,
                            kind,
                        } => {
                            if entity.to_bits() == 0 {
                                return Err(ProtocolError::Malformed("lifecycle entity"));
                            }
                            self.u8(match kind {
                                EntityLifecycleKind::Created => LIFECYCLE_ENTITY_CREATED,
                                EntityLifecycleKind::MetadataChanged => {
                                    LIFECYCLE_ENTITY_METADATA_CHANGED
                                }
                                EntityLifecycleKind::Deleted => LIFECYCLE_ENTITY_DELETED,
                            })?;
                            self.u64(entity.to_bits())?;
                        }
                        LifecycleObservation::Component {
                            entity,
                            component,
                            kind,
                            previous_incarnation,
                            incarnation,
                        } => {
                            if entity.to_bits() == 0 || *component == 0 {
                                return Err(ProtocolError::Malformed(
                                    "lifecycle component identity",
                                ));
                            }
                            self.u8(match kind {
                                ComponentLifecycleKind::Inserted => LIFECYCLE_COMPONENT_INSERTED,
                                ComponentLifecycleKind::Updated => LIFECYCLE_COMPONENT_UPDATED,
                                ComponentLifecycleKind::Replaced => LIFECYCLE_COMPONENT_REPLACED,
                                ComponentLifecycleKind::Removed => LIFECYCLE_COMPONENT_REMOVED,
                            })?;
                            self.u64(entity.to_bits())?;
                            self.u16(*component)?;
                            self.u64(previous_incarnation.unwrap_or(0))?;
                            self.u64(incarnation.unwrap_or(0))?;
                        }
                        LifecycleObservation::Asset {
                            resource,
                            kind,
                        } => {
                            if resource.id == 0
                                || resource.kind.0 == 0
                                || resource.source.is_empty()
                                || resource.source.len() > 2048
                                || matches!(&resource.status, ipp_core::AssetResourceStatus::Failed(error) if error.len() > 2048)
                            {
                                return Err(ProtocolError::Malformed("lifecycle resource"));
                            }
                            self.u8(match kind {
                                ipp_core::services::asset_management::AssetLifecycleKind::GraphicsInvalidated => LIFECYCLE_ASSET_GRAPHICS_INVALIDATED,
                                ipp_core::services::asset_management::AssetLifecycleKind::StatusChanged => LIFECYCLE_ASSET_STATUS_CHANGED,
                                ipp_core::services::asset_management::AssetLifecycleKind::Removed => LIFECYCLE_ASSET_REMOVED,
                            })?;
                            self.resource(resource)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_identity_domains_and_correlations_are_validated_before_ingress() {
        let request = |request_id, subscription, domains| {
            let mut writer = Writer(Vec::new());
            writer.u64(7).unwrap();
            writer.u64(request_id).unwrap();
            writer.u8(REQUEST_LIFECYCLE_SUBSCRIBE).unwrap();
            writer.u64(subscription).unwrap();
            writer.u16(domains).unwrap();
            writer.u64(0).unwrap();
            writer.u16(0).unwrap();
            writer.u64(0).unwrap();
            writer.0
        };
        assert!(matches!(
            decode_request(&request(1, 2, 3), 7).unwrap().body,
            RequestBody::LifecycleSubscription(_)
        ));
        for bytes in [
            request(0, 2, 3),
            request(1, 0, 3),
            request(1, 2, 0),
            request(1, 2, 8),
        ] {
            assert!(decode_request(&bytes, 7).is_err());
        }
        assert!(decode_request(&request(1, 2, 3), 8).is_err());
    }
}
