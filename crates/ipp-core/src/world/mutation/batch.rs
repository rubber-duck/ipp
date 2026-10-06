//! Ordered batch application: symbol resolution, effect admission, commit and
//! the batch outcome, and ordered System command application.

use std::collections::BTreeSet;

use super::EntityAliases;
use crate::world::WorldContext;
use crate::world::systems;
use crate::{Batch, BatchError, BatchOutcome, Command, EntityId, EntityRef, ErrorReason};

impl WorldContext<'_> {
    pub(in crate::world) fn apply_batch(
        &mut self,
        mut batch: Batch,
        tick: u64,
        mut effect_sink: Option<&mut dyn crate::OperationEffectSink>,
    ) -> BatchOutcome {
        if let Some(reason) = self.world.fault {
            return BatchOutcome {
                batch_id: batch.id,
                tick,
                result: Err(BatchError {
                    scope: crate::BatchErrorScope::Commit,
                    operation: None,
                    reason,
                    aliases: Vec::new(),
                }),
                symbols: Vec::new(),
                effects: Vec::new(),
            };
        }
        if !batch.operations.is_empty() {
            crate::diagnostic!(
                Debug,
                "[IPP core] batch.begin batch={} operations={}",
                batch.id,
                batch.operations.len()
            );
        }
        self.instances
            .visit_scoped(self.world.identity, None, |system, _| system.begin_batch());
        let mut aliases = EntityAliases::default();
        let mut created = Vec::new();
        let mut symbols = Vec::new();
        let mut resolved_symbols = BTreeSet::new();
        let mut result = Ok(());
        let mut effects = Vec::new();
        let mut operation_effects = Vec::new();
        for (operation, command) in batch.operations.iter_mut().enumerate() {
            let applied = (|| {
                self.runtime_access().prepare_operation_boundary(None)?;
                if let Command::DetachWorldAttachmentReceipt {
                    receipt,
                } = command
                {
                    let expected = effect_sink
                        .as_ref()
                        .ok_or(ErrorReason::InvalidEntity)?
                        .attachment_receipt(*receipt)?;
                    *command = Command::DetachWorldAttachmentIf {
                        expected,
                    };
                }
                self.resolve_operation_references(command)?;
                self.resolve_symbol_references(command, &mut symbols, &mut resolved_symbols)?;
                if let Some(sink) = effect_sink.as_mut() {
                    self.apply_admitted_operation(
                        command,
                        &mut aliases,
                        &mut created,
                        &mut operation_effects,
                        &mut **sink,
                    )
                } else {
                    self.runtime_access().apply_prepared_operation(
                        None,
                        command,
                        &mut aliases,
                        &mut created,
                        &mut operation_effects,
                    )
                }
            })();
            effects.extend(operation_effects.drain(..).map(|effect| {
                crate::AppliedOperationEffect {
                    operation,
                    effect,
                }
            }));
            if let Err(reason) = applied {
                result = Err(BatchError {
                    aliases: Vec::new(),
                    scope: crate::BatchErrorScope::Operation,
                    operation: Some(operation),
                    reason,
                });
                break;
            }
        }
        let validation = self.commit_pending_changes();
        for (event, entity) in std::mem::take(&mut self.world.state.entity_effects) {
            crate::diagnostic!(
                Debug,
                "[IPP core] {} batch={} entity={}",
                event,
                batch.id,
                entity.to_bits()
            );
        }
        if let Err(reason) = validation
            && (result.is_ok() || reason == ErrorReason::NonConvergentCommit)
        {
            result = Err(BatchError {
                aliases: Vec::new(),
                scope: crate::BatchErrorScope::Commit,
                operation: None,
                reason,
            });
        }
        let result = match result {
            Ok(()) => Ok(created),
            Err(mut error) => {
                error.aliases = created;
                Err(error)
            }
        };
        if !batch.operations.is_empty() {
            match &result {
                Ok(_) => crate::diagnostic!(
                    Debug,
                    "[IPP core] batch.commit batch={} operations={}",
                    batch.id,
                    batch.operations.len()
                ),
                Err(error) => crate::diagnostic!(
                    Warn,
                    "[IPP core] batch.reject batch={} operations={} operation={:?} reason={}",
                    batch.id,
                    batch.operations.len(),
                    error.operation,
                    error.reason
                ),
            }
        }
        let mut outcome = BatchOutcome {
            batch_id: batch.id,
            tick,
            result,
            symbols,
            effects,
        };
        self.instances
            .visit_scoped(self.world.identity, None, |system, _| {
                system.finish_batch(&mut outcome)
            });
        self.recycle_command_buffer(batch.operations);
        outcome
    }

    /// Replace each symbolic reference of `command` with the handle it names at
    /// this operation boundary, recording each distinct symbol and handle once,
    /// in first-resolution order, for the batch outcome.
    fn resolve_symbol_references(
        &self,
        command: &mut Command,
        symbols: &mut Vec<(std::sync::Arc<str>, EntityId)>,
        resolved: &mut BTreeSet<(std::sync::Arc<str>, EntityId)>,
    ) -> Result<(), ErrorReason> {
        let index = &self.world.state.symbols;
        command.visit_entity_refs_mut(&mut |reference| {
            let EntityRef::Symbol(symbol) = reference else {
                return Ok(());
            };
            let id = index
                .get(&**symbol)
                .copied()
                .ok_or(ErrorReason::MissingSymbolicId)?;
            if resolved.insert((symbol.clone(), id)) {
                symbols.push((symbol.clone(), id));
            }
            *reference = EntityRef::Handle(id);
            Ok(())
        })
    }

    pub(in crate::world) fn apply_system_command(
        &mut self,
        system: systems::SystemId,
        session: u64,
        command: &dyn std::any::Any,
    ) -> Result<(), ErrorReason> {
        if let Some(reason) = self.world.fault {
            return Err(reason);
        }
        let publications = self.publications;
        let local_outputs = self.outputs();
        self.with_system_dyn(system, |current, world| {
            let mut declared_worlds = BTreeSet::new();
            current.command_world_references(command, &mut |world| {
                declared_worlds.insert(world);
            });
            current.command(
                &mut systems::SystemCommandContext {
                    world,
                    publications,
                    declared_worlds,
                    local_outputs,
                },
                session,
                command,
            )
        })
        .unwrap_or(Err(ErrorReason::InvalidValue))
    }
}
