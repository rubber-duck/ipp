//! Pure operation impact discovery and scoped, fallible delivery admission.

use super::EntityAliases;
use crate::systems::{OperationImpact, SystemOperationPreparationContext};
use crate::world::WorldContext;
use crate::{Command, EntityId, ErrorReason};

impl WorldContext<'_> {
    pub(in crate::world) fn operation_effect_demand(
        &self,
        command: &Command,
        aliases: &EntityAliases,
    ) -> Result<crate::OperationEffectDemand, ErrorReason> {
        self.world.manifest.admit_command(command)?;
        let context = SystemOperationPreparationContext {
            world_data: self.world,
            aliases,
            topology: self.topology,
            command,
        };
        let mut impact = OperationImpact::default();
        match command {
            Command::Delete {
                entity,
            } => {
                impact
                    .deleted_entities
                    .insert(context.resolve_entity(entity)?);
            }
            Command::DeleteSubtree {
                root,
            } => {
                impact.deleted_entities.extend(
                    self.world
                        .state
                        .links
                        .subtree(context.resolve_entity(root)?),
                );
            }
            Command::RemoveComponent {
                entity,
                component,
            } => {
                impact
                    .removed_components
                    .insert((context.resolve_entity(entity)?, *component));
            }
            _ => {}
        }
        for instance in self.instances.before.iter() {
            instance.system.operation_impact(&context, &mut impact)?;
        }
        // An adopting operation may report that it found its target present.
        let mut demand = crate::OperationEffectDemand {
            adopting: matches!(
                command,
                Command::Create {
                    adopt: true,
                    ..
                } | Command::InsertComponent {
                    adopt: true,
                    ..
                }
            ),
            ..Default::default()
        };
        for instance in self.instances.before.iter() {
            instance
                .system
                .operation_effect_demand(&context, &impact, &mut demand)?;
        }
        Ok(demand)
    }

    pub(in crate::world) fn apply_admitted_operation(
        &mut self,
        command: &Command,
        aliases: &mut EntityAliases,
        created: &mut Vec<(u32, EntityId)>,
        effects: &mut Vec<crate::OperationEffect>,
        sink: &mut dyn crate::OperationEffectSink,
    ) -> Result<(), ErrorReason> {
        let demand = self.operation_effect_demand(command, aliases)?;
        effects
            .try_reserve(demand.max_records + usize::from(demand.adopting))
            .map_err(|_| ErrorReason::Capacity)?;
        if let Err(error) = sink.reserve(&demand) {
            sink.settle();
            return Err(error);
        }
        let start = effects.len();
        let result = self
            .runtime_access()
            .apply_prepared_operation(None, command, aliases, created, effects);
        for effect in &effects[start..] {
            sink.emit(effect.clone());
        }
        sink.settle();
        result
    }
}
