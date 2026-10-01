//! Pure operation impact discovery and scoped, fallible delivery admission.

use super::*;

/// Conservative operation-local releases, discovered before any mutation callback.
#[derive(Default)]
pub struct OperationImpact {
    /// Ordinary and subsystem-owned entities this operation can delete.
    pub deleted_entities: BTreeSet<EntityId>,
    /// Producer components this operation can remove without deleting their entity.
    pub removed_components: BTreeSet<(EntityId, u16)>,
}

/// Immutable authoring state at the next ordered operation boundary.
pub struct SystemOperationPreparationContext<'a> {
    pub(in crate::world) world_data: &'a WorldSimulationState,
    pub(in crate::world) aliases: &'a EntityAliases,
    pub(in crate::world) topology: &'a crate::host::topology::HostTopology,
    pub(in crate::world) command: &'a Command,
}

impl SystemOperationPreparationContext<'_> {
    /// The resolved command whose possible effects are being reserved.
    pub fn command(&self) -> &Command {
        self.command
    }

    /// Read the current applied prefix without staging values or invalidating bindings.
    pub fn world(&self) -> systems::SystemWorldView<'_> {
        systems::SystemWorldView {
            world: self.world_data,
            authored: &self.world_data.state,
        }
    }

    /// Resolve an entity against this batch's applied prefix.
    pub fn resolve_entity(&self, reference: &EntityRef) -> Result<EntityId, ErrorReason> {
        let entity = self
            .aliases
            .identity(reference, &self.world_data.state.symbols)?;
        self.world_data
            .state
            .allocator
            .contains(entity)
            .then_some(entity)
            .ok_or(ErrorReason::InvalidEntity)
    }
}

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
