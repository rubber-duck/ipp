//! Exact component lifetimes fence the one permitted presentation consumer.

use super::{queries::binding_identity, system::runtime_mut};
use crate::{EntityId, ErrorReason};

/// Authority for a single entity-local presentation consumer. It carries no pointers
/// and becomes stale when its World, binding or consumer component is replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataBindingPresentationConsumer {
    world: crate::WorldRef,
    pub(super) entity: EntityId,
    pub(super) binding_component: u16,
    binding_incarnation: u64,
    consumer_component: u16,
    consumer_incarnation: u64,
}

macro_rules! presentation_access {
    ($context:ty) => {
        impl $context {
            /// Register one existing presentation component on the binding's entity.
            /// Re-registering the same lifetime is idempotent; a competing lifetime fails.
            /// Headless queries require no registration and never acknowledge dirty state.
            pub fn register_data_binding_presentation_consumer(
                &mut self,
                entity: EntityId,
                component: u16,
            ) -> Result<DataBindingPresentationConsumer, ErrorReason> {
                if super::system::is_binding(component) {
                    return Err(ErrorReason::InvalidValue);
                }
                let (binding_component, binding_incarnation) =
                    binding_identity(self.world, entity)?;
                let consumer_incarnation = self
                    .world
                    .state
                    .entities
                    .get(&entity)
                    .and_then(|record| record.input(component))
                    .ok_or(ErrorReason::MissingComponent)?
                    .incarnation;
                let handle = DataBindingPresentationConsumer {
                    world: crate::WorldRef {
                        id: self.world.id,
                        incarnation: self.world.identity,
                    },
                    entity,
                    binding_component,
                    binding_incarnation,
                    consumer_component: component,
                    consumer_incarnation,
                };
                let runtime = runtime_mut(&mut self.world.components, entity, binding_component)
                    .ok_or(ErrorReason::MissingComponent)?;
                let identity = (entity, component, consumer_incarnation);
                if runtime
                    .presentation
                    .is_some_and(|previous| previous != identity)
                {
                    return Err(ErrorReason::InvalidValue);
                }
                if runtime.presentation.is_none() {
                    runtime.presentation = Some(identity);
                    runtime.dirty = true;
                }
                Ok(handle)
            }

            /// Clear dirty only after this consumer successfully updates its presentation.
            /// Failed or skipped preparation must leave dirty set. This never evaluates data.
            pub fn finish_data_binding_presentation(
                &mut self,
                consumer: DataBindingPresentationConsumer,
            ) -> Result<(), ErrorReason> {
                self.validate_data_binding_consumer(consumer)?;
                runtime_mut(
                    &mut self.world.components,
                    consumer.entity,
                    consumer.binding_component,
                )
                .ok_or(ErrorReason::MissingComponent)?
                .dirty = false;
                Ok(())
            }

            /// Release only the registered lifetime; its eventual replacement starts dirty.
            pub fn release_data_binding_presentation_consumer(
                &mut self,
                consumer: DataBindingPresentationConsumer,
            ) -> Result<(), ErrorReason> {
                self.validate_data_binding_consumer(consumer)?;
                let runtime = runtime_mut(
                    &mut self.world.components,
                    consumer.entity,
                    consumer.binding_component,
                )
                .ok_or(ErrorReason::MissingComponent)?;
                runtime.presentation = None;
                runtime.dirty = true;
                Ok(())
            }

            pub(super) fn validate_data_binding_consumer(
                &self,
                consumer: DataBindingPresentationConsumer,
            ) -> Result<(), ErrorReason> {
                if consumer.world
                    != (crate::WorldRef {
                        id: self.world.id,
                        incarnation: self.world.identity,
                    })
                    || binding_identity(self.world, consumer.entity)?
                        != (consumer.binding_component, consumer.binding_incarnation)
                    || self
                        .world
                        .state
                        .entities
                        .get(&consumer.entity)
                        .and_then(|record| record.input(consumer.consumer_component))
                        .is_none_or(|input| input.incarnation != consumer.consumer_incarnation)
                {
                    return Err(ErrorReason::InvalidEntity);
                }
                let runtime = if consumer.binding_component
                    == crate::ComponentValue::BUFFER_DATA_SOURCE_BINDING
                {
                    &self
                        .world
                        .components
                        .buffer_data_source_binding(consumer.entity.index() as usize)
                        .ok_or(ErrorReason::MissingComponent)?
                        .runtime
                } else {
                    &self
                        .world
                        .components
                        .streaming_data_source_binding(consumer.entity.index() as usize)
                        .ok_or(ErrorReason::MissingComponent)?
                        .runtime
                };
                if runtime.presentation
                    != Some((
                        consumer.entity,
                        consumer.consumer_component,
                        consumer.consumer_incarnation,
                    ))
                {
                    return Err(ErrorReason::InvalidEntity);
                }
                Ok(())
            }
        }
    };
}

presentation_access!(crate::WorldContext<'_>);
presentation_access!(crate::systems::SystemRuntimeAccess<'_>);
