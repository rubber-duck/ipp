//! Generic ordered operations and phase-scoped System dispatch.

use std::collections::BTreeSet;

use super::EntityAliases;
use super::commit_components;
use crate::world::WorldMutationState;
use crate::world::systems::{System, SystemOperationContext, SystemRuntimeAccess};
use crate::{Command, ComponentValue, EntityId, ErrorReason};

impl SystemRuntimeAccess<'_> {
    pub(in crate::world) fn apply_operation(
        &mut self,
        mut current: Option<&mut dyn System>,
        command: &Command,
        aliases: &mut EntityAliases,
        created: &mut Vec<(u32, EntityId)>,
        effects: &mut Vec<crate::OperationEffect>,
    ) -> Result<(), ErrorReason> {
        self.prepare_operation_boundary(
            current
                .as_deref_mut()
                .map(|system| system as &mut dyn System),
        )?;
        self.apply_prepared_operation(current, command, aliases, created, effects)
    }

    pub(in crate::world) fn prepare_operation_boundary(
        &mut self,
        current: Option<&mut dyn System>,
    ) -> Result<(), ErrorReason> {
        // A subsequent operation may reuse a deleted entity slot only after all
        // handlers have completed against the old occupied component storage.
        if !self.world.state.retired_entities.is_empty() {
            commit_components(
                self.world,
                &mut self.instances,
                current,
                self.asset_acquisition,
                self.data,
                false,
            )?;
        }
        Ok(())
    }

    pub(in crate::world) fn apply_prepared_operation(
        &mut self,
        mut current: Option<&mut dyn System>,
        command: &Command,
        aliases: &mut EntityAliases,
        created: &mut Vec<(u32, EntityId)>,
        effects: &mut Vec<crate::OperationEffect>,
    ) -> Result<(), ErrorReason> {
        debug_assert!(self.world.state.retired_entities.is_empty());
        self.world.manifest.admit_command(command)?;
        let requested_component = match command {
            Command::InsertComponent {
                component,
                ..
            } => Some(*component),
            Command::InsertComponentValue {
                value,
                ..
            } => Some(ComponentValue::type_id(value)),
            _ => None,
        };
        if let Some(component) = requested_component {
            let slots = self.world.state.allocator.slots();
            let mut pending = vec![component];
            let mut visited = BTreeSet::new();
            while let Some(component) = pending.pop() {
                if visited.insert(component) {
                    self.world
                        .components
                        .try_reserve_component(component, slots)?;
                    pending.extend_from_slice(ComponentValue::required_components(component));
                }
            }
        }
        let mut staged = WorldMutationState {
            entities_state: std::mem::take(&mut self.world.state),
        };
        staged.explicit_fields.clear();
        staged.operation_components.clear();
        staged.operation_untracked.clear();
        staged.operation_writes.clear();
        staged.operation_created.clear();
        staged.operation_deleted.clear();
        staged.operation_adopted = false;
        staged.links.operation_changed.clear();
        let mut result = Ok(());
        self.instances.visit_scoped(
            self.world.identity,
            current
                .as_deref_mut()
                .map(|system| system as &mut dyn System),
            |system, dependencies| {
                if result.is_ok() {
                    result = system.before_operation(&mut SystemOperationContext {
                        effects,
                        world_data: self.world,
                        staged: &mut staged,
                        command,
                        aliases,
                        dependencies,
                        assets: self.asset_acquisition,
                        data: self.data,
                        topology: self.topology,
                        frame_context: self.frame_context,
                    });
                }
            },
        );
        if result.is_ok() {
            let mut handled = None;
            self.instances.visit_scoped(
                self.world.identity,
                current
                    .as_deref_mut()
                    .map(|system| system as &mut dyn System),
                |system, dependencies| {
                    if handled.is_none() {
                        handled = system.apply_operation(&mut SystemOperationContext {
                            effects,
                            world_data: self.world,
                            staged: &mut staged,
                            command,
                            aliases,
                            dependencies,
                            assets: self.asset_acquisition,
                            data: self.data,
                            topology: self.topology,
                            frame_context: self.frame_context,
                        });
                    }
                },
            );
            result = handled.unwrap_or_else(|| {
                staged.apply(
                    &self.world.components,
                    command,
                    aliases,
                    created,
                    self.world.limits,
                )
            });
        }
        // Applied effects are retained on failure, so the requirement rule
        // completes every operation, including a failed one.
        result = result.and(staged.insert_required_components(&self.world.components));
        let link_result = staged.links.reconcile();
        result = result.and(link_result);
        self.instances
            .visit_scoped(self.world.identity, current, |system, dependencies| {
                let resolved = system.after_operation(&mut SystemOperationContext {
                    effects,
                    world_data: self.world,
                    staged: &mut staged,
                    command,
                    aliases,
                    dependencies,
                    assets: self.asset_acquisition,
                    data: self.data,
                    topology: self.topology,
                    frame_context: self.frame_context,
                });
                result = std::mem::replace(&mut result, Ok(())).and(resolved);
            });
        staged.prepare_changes();
        staged.record_component_observations(&self.world.components);
        if staged.operation_adopted {
            effects.push(crate::OperationEffect::Adopted);
        }
        self.world.state = staged.entities_state;
        result
    }

    pub(in crate::world) fn apply_authored_commands(
        &mut self,
        mut current: Option<&mut dyn System>,
        commands: &[Command],
    ) -> Result<(), ErrorReason> {
        let mut aliases = EntityAliases::default();
        let mut created = Vec::new();
        let mut result = Ok(());
        for command in commands {
            result = self.apply_operation(
                current
                    .as_deref_mut()
                    .map(|system| system as &mut dyn System),
                command,
                &mut aliases,
                &mut created,
                &mut Vec::new(),
            );
            if result.is_err() {
                break;
            }
        }
        let commit = commit_components(
            self.world,
            &mut self.instances,
            current,
            self.asset_acquisition,
            self.data,
            false,
        );
        result.and(commit)
    }
}
