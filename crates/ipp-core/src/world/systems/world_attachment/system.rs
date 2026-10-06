//! Producer-only attachment admission through ordinary component operations.

use crate::components::{registry, schema::ComponentLifecycle};
use crate::host::attachments::topology::AttachmentAnchor;
use crate::systems::{
    System, SystemCapabilities, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemOperationContext,
};
use crate::{
    Command, ComponentValue, ErrorReason, OperationEffect, WorldAttachment, WorldAttachmentEffect,
};

#[derive(Default)]
/// Owns attachment admission and graph bookkeeping through scoped Host access.
pub struct WorldAttachmentSystem;

impl WorldAttachmentSystem {
    /// Stable factory identity for producer attachment support.
    pub const ID: SystemId = SystemId("ipp.world-attachment");
}

/// Stateless reusable factory with no per-World graph ownership.
pub struct WorldAttachmentSystemFactory;

impl SystemFactory for WorldAttachmentSystemFactory {
    fn id(&self) -> SystemId {
        WorldAttachmentSystem::ID
    }

    fn capabilities(&self) -> SystemCapabilities {
        SystemCapabilities::new([ComponentValue::WORLD_ATTACHMENT], [])
    }

    fn dependencies(&self) -> &[crate::systems::SystemDependency] {
        &[]
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(WorldAttachmentSystem))
    }
}

impl System for WorldAttachmentSystem {
    fn completed_attachments(
        &self,
        world: &crate::WorldContext<'_>,
        attachments: &mut Vec<crate::PublishedWorldAttachment>,
    ) {
        self.collect_attachments(world, attachments);
    }

    fn update(&mut self, _: &mut crate::systems::SystemUpdateContext<'_, '_>) {}

    fn operation_effect_demand(
        &self,
        context: &crate::systems::SystemOperationPreparationContext<'_>,
        impact: &crate::systems::OperationImpact,
        demand: &mut crate::OperationEffectDemand,
    ) -> Result<(), ErrorReason> {
        let writes = matches!(
            context.command(),
            Command::InsertComponent {
                component: ComponentValue::WORLD_ATTACHMENT,
                ..
            } | Command::SetField {
                component: ComponentValue::WORLD_ATTACHMENT,
                ..
            } | Command::SetFieldIf {
                component: ComponentValue::WORLD_ATTACHMENT,
                ..
            }
        ) || matches!(context.command(), Command::InsertComponentValue { value, .. } if value.type_id() == ComponentValue::WORLD_ATTACHMENT);
        let mut existing = std::collections::BTreeMap::new();
        if let Command::DetachWorldAttachmentIf {
            expected,
        } = context.command()
        {
            existing.insert(expected.identity(), expected.clone());
        }
        for entity in impact.deleted_entities.iter().copied().chain(
            impact
                .removed_components
                .iter()
                .filter_map(|&(entity, component)| {
                    (component == ComponentValue::WORLD_ATTACHMENT).then_some(entity)
                }),
        ) {
            if let Some(token) = context.topology.tokens.get(&AttachmentAnchor {
                world: context.world_data.id,
                entity,
            }) {
                existing.insert(token.identity(), token.clone());
            }
        }
        demand.fresh_attachment_tokens = demand
            .fresh_attachment_tokens
            .checked_add(usize::from(writes))
            .ok_or(ErrorReason::Capacity)?;
        demand.max_records = demand
            .max_records
            .checked_add(usize::from(writes))
            .and_then(|count| count.checked_add(existing.len()))
            .ok_or(ErrorReason::Capacity)?;
        demand
            .existing_attachment_tokens
            .extend(existing.into_values());
        Ok(())
    }

    fn before_operation(
        &mut self,
        context: &mut SystemOperationContext<'_>,
    ) -> Result<(), ErrorReason> {
        let (entity, candidate) = match context.command {
            Command::DetachWorldAttachmentIf {
                expected,
            } => {
                return context.topology.validate_token(
                    crate::WorldRef {
                        id: context.world_data.id,
                        incarnation: context.world_data.identity,
                    },
                    expected,
                );
            }
            Command::InsertComponentValue {
                entity,
                value,
            } if value.type_id() == ComponentValue::WORLD_ATTACHMENT => (
                context.resolve_entity(entity)?,
                ComponentValue::clone(value),
            ),
            Command::InsertComponent {
                entity,
                component: ComponentValue::WORLD_ATTACHMENT,
                fields,
                adopt,
            } => {
                let entity = context.resolve_entity(entity)?;
                // Adoption writes the listed fields over an existing attachment.
                let mut candidate = adopt
                    .then(|| {
                        context.staged.input_value(
                            &context.world_data.components,
                            entity,
                            ComponentValue::WORLD_ATTACHMENT,
                        )
                    })
                    .flatten()
                    .unwrap_or_else(|| ComponentValue::WorldAttachment(WorldAttachment::default()));
                for field in fields {
                    registry::assign(&mut candidate, field)?;
                }
                (entity, candidate)
            }
            Command::SetField {
                entity,
                component: ComponentValue::WORLD_ATTACHMENT,
                field,
            }
            | Command::SetFieldIf {
                entity,
                component: ComponentValue::WORLD_ATTACHMENT,
                field,
                ..
            } => {
                let entity = context.resolve_entity(entity)?;
                let mut candidate = context
                    .staged
                    .input_value(
                        &context.world_data.components,
                        entity,
                        ComponentValue::WORLD_ATTACHMENT,
                    )
                    .ok_or(ErrorReason::MissingComponent)?;
                registry::assign(&mut candidate, field)?;
                (entity, candidate)
            }
            _ => return Ok(()),
        };
        let ComponentValue::WorldAttachment(value) = candidate else {
            unreachable!()
        };
        if context.frame_context.is_some() {
            return Err(ErrorReason::InvalidValue);
        }
        value.validate()?;
        context.topology.preflight_token()?;
        if value.mode != 0
            && !crate::systems::surface::SURFACE_PROVIDERS
                .iter()
                .any(|&component| {
                    context
                        .staged
                        .input_value(&context.world_data.components, entity, component)
                        .is_some()
                })
        {
            return Err(ErrorReason::MissingComponent);
        }
        let anchor = AttachmentAnchor {
            world: context.world_data.id,
            entity,
        };
        context.topology.validate(anchor, &value)
    }

    fn apply_operation(
        &mut self,
        context: &mut SystemOperationContext<'_>,
    ) -> Option<Result<(), ErrorReason>> {
        if let Command::DetachWorldAttachmentIf {
            expected,
        } = context.command
        {
            let anchor = AttachmentAnchor {
                world: context.world_data.id,
                entity: expected.anchor(),
            };
            if context.topology.tokens.get(&anchor) != Some(expected) {
                context.emit_effect(OperationEffect::WorldAttachment(
                    WorldAttachmentEffect::Superseded(expected.clone()),
                ));
                return Some(Ok(()));
            }
            let result = context.staged.apply(
                &context.world_data.components,
                &Command::RemoveComponent {
                    entity: crate::EntityRef::Handle(expected.anchor()),
                    component: ComponentValue::WORLD_ATTACHMENT,
                },
                context.aliases,
                &mut Vec::new(),
                context.world_data.limits,
            );
            if result.is_ok() {
                context.topology.detach_token(anchor);
                context.emit_effect(OperationEffect::WorldAttachment(
                    WorldAttachmentEffect::Detached(expected.clone()),
                ));
            }
            return Some(result);
        }
        let entity = match context.command {
            Command::InsertComponent {
                entity,
                component: ComponentValue::WORLD_ATTACHMENT,
                ..
            }
            | Command::SetField {
                entity,
                component: ComponentValue::WORLD_ATTACHMENT,
                ..
            }
            | Command::SetFieldIf {
                entity,
                component: ComponentValue::WORLD_ATTACHMENT,
                ..
            } => entity.clone(),
            Command::InsertComponentValue {
                entity,
                value,
            } if value.type_id() == ComponentValue::WORLD_ATTACHMENT => entity.clone(),
            _ => return None,
        };
        Some((|| {
            let entity = context.resolve_entity(&entity)?;
            context.staged.apply(
                &context.world_data.components,
                context.command,
                context.aliases,
                &mut Vec::new(),
                context.world_data.limits,
            )?;
            let Some(ComponentValue::WorldAttachment(value)) = context.staged.input_value(
                &context.world_data.components,
                entity,
                ComponentValue::WORLD_ATTACHMENT,
            ) else {
                unreachable!("applied attachment");
            };
            let incarnation = context.staged.entities[&entity]
                .input(ComponentValue::WORLD_ATTACHMENT)
                .expect("applied attachment incarnation")
                .incarnation;
            let token = context.topology.write_token(
                AttachmentAnchor {
                    world: context.world_data.id,
                    entity,
                },
                incarnation,
                value.child(),
            );
            context.emit_effect(OperationEffect::WorldAttachment(
                WorldAttachmentEffect::Written(token),
            ));
            Ok(())
        })())
    }

    fn after_operation(
        &mut self,
        context: &mut SystemOperationContext<'_>,
    ) -> Result<(), ErrorReason> {
        let mut effects = Vec::new();
        for &(entity, component) in &context.staged.operation_components {
            if component == ComponentValue::WORLD_ATTACHMENT {
                let value =
                    context
                        .staged
                        .input_value(&context.world_data.components, entity, component);
                if value.is_none()
                    && let Some(token) = context.topology.detach_token(AttachmentAnchor {
                        world: context.world_data.id,
                        entity,
                    })
                {
                    effects.push(OperationEffect::WorldAttachment(
                        WorldAttachmentEffect::Detached(token),
                    ));
                }
            }
        }
        for &entity in &context.staged.operation_deleted {
            if let Some(token) = context.topology.detach_token(AttachmentAnchor {
                world: context.world_data.id,
                entity,
            }) {
                effects.push(OperationEffect::WorldAttachment(
                    WorldAttachmentEffect::Detached(token),
                ));
            }
        }
        context.effects.extend(effects);
        Ok(())
    }
}
