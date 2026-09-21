//! Generic ordered operations and phase-scoped System dispatch.

use super::access::SystemInstanceAccess;
use super::*;
use systems::{System, SystemDependencies, SystemOperationContext, SystemRuntimeAccess};

impl SystemInstanceAccess<'_> {
    pub(in crate::world) fn visit_scoped(
        &mut self,
        identity: usize,
        mut current: Option<&mut dyn System>,
        mut callback: impl FnMut(&mut dyn System, SystemDependencies<'_>),
    ) {
        for index in 0..self.before.len() {
            let (before, tail) = self.before.split_at_mut(index);
            let (instance, _) = tail.split_first_mut().expect("schedule index");
            callback(
                instance.system.as_mut(),
                SystemDependencies {
                    identity,
                    dependent: index,
                    instances: before,
                    trailing: None,
                },
            );
        }
        if let Some(system) = current.as_deref_mut() {
            callback(
                system,
                SystemDependencies {
                    identity,
                    dependent: self.before.len(),
                    instances: self.before,
                    trailing: None,
                },
            );
        }
        for index in 0..self.after.len() {
            let (prefix, tail) = self.after.split_at_mut(index);
            let (instance, _) = tail.split_first_mut().expect("schedule index");
            callback(
                instance.system.as_mut(),
                SystemDependencies {
                    identity,
                    dependent: self.before.len() + 1 + index,
                    instances: self.before,
                    trailing: current.as_deref().map(|system| (system, &*prefix)),
                },
            );
        }
    }
}

impl SystemRuntimeAccess<'_> {
    #[cfg(feature = "surfaces")]
    pub(in crate::world) fn can_apply_authored_in_place(
        &self,
        entity: EntityId,
        component: u16,
    ) -> bool {
        crate::allocation_optimizations_enabled()
            && !self.world.state.dirty.contains(&(entity, component))
            && !self.world.state.prepared.contains_key(&(entity, component))
            && self
                .world
                .state
                .entities
                .get(&entity)
                .and_then(|record| record.layers.get(&component))
                .is_some_and(|layer| layer.inputs.supports_in_place_authored_mutation())
    }

    pub(in crate::world) fn apply_operation(
        &mut self,
        mut current: Option<&mut dyn System>,
        command: &Command,
        aliases: &mut BTreeMap<u32, EntityId>,
        created: &mut Vec<(u32, EntityId)>,
    ) -> Result<(), ErrorReason> {
        // A subsequent operation may reuse a deleted entity slot only after all
        // handlers have completed against the old occupied component storage.
        if !self.world.state.retired_entities.is_empty() {
            super::access::commit_components(
                self.world,
                &mut self.instances,
                current
                    .as_deref_mut()
                    .map(|system| system as &mut dyn System),
                self.asset_acquisition,
                false,
            )?;
        }
        let mut staged = WorldMutationState {
            entities_state: std::mem::take(&mut self.world.state),
        };
        staged.explicit_fields.clear();
        staged.operation_components.clear();
        staged.operation_created.clear();
        staged.operation_deleted.clear();
        let mut result = Ok(());
        self.instances.visit_scoped(
            self.world.identity,
            current
                .as_deref_mut()
                .map(|system| system as &mut dyn System),
            |system, dependencies| {
                if result.is_ok() {
                    result = system.before_operation(&mut SystemOperationContext {
                        world_data: self.world,
                        staged: &mut staged,
                        command,
                        aliases,
                        dependencies,
                        assets: self.asset_acquisition,
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
                            world_data: self.world,
                            staged: &mut staged,
                            command,
                            aliases,
                            dependencies,
                            assets: self.asset_acquisition,
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
        self.instances
            .visit_scoped(self.world.identity, current, |system, dependencies| {
                let resolved = system.after_operation(&mut SystemOperationContext {
                    world_data: self.world,
                    staged: &mut staged,
                    command,
                    aliases,
                    dependencies,
                    assets: self.asset_acquisition,
                });
                result = std::mem::replace(&mut result, Ok(())).and(resolved);
            });
        result = result.and(staged.prepare_changes(&self.world.components, self.world.limits));
        staged.record_component_observations(&self.world.components);
        self.world.state = staged.entities_state;
        result
    }

    /// Apply a prevalidated authored value edit after synchronous commit
    /// invalidation. Identity and resource declaration changes use staged commands.
    #[cfg(feature = "surfaces")]
    pub(in crate::world) fn apply_authored_in_place(
        &mut self,
        current: &mut dyn System,
        entity: EntityId,
        component: u16,
        fields: impl IntoIterator<Item = u32>,
        apply: impl FnOnce(&mut registry::ComponentStorage) -> Result<(), ErrorReason> + 'static,
    ) -> Result<(), ErrorReason> {
        if !self.can_apply_authored_in_place(entity, component) {
            return Err(ErrorReason::InvalidValue);
        }
        self.world.state.explicit_fields.clear();
        let incarnation = self
            .world
            .state
            .entities
            .get(&entity)
            .and_then(|record| record.input(component))
            .ok_or(ErrorReason::MissingComponent)?
            .incarnation;
        self.world
            .state
            .changed
            .insert((entity, component), Some(incarnation));
        for field in fields {
            self.world
                .state
                .explicit_fields
                .insert((entity, component, field));
        }
        self.world
            .state
            .deferred_mutations
            .push(DeferredComponentMutation {
                entity,
                component,
                apply: Box::new(apply),
            });
        super::access::commit_components(
            self.world,
            &mut self.instances,
            Some(current),
            self.asset_acquisition,
            false,
        )
    }

    pub(in crate::world) fn apply_authored_commands(
        &mut self,
        mut current: Option<&mut dyn System>,
        commands: &[Command],
    ) -> Result<(), ErrorReason> {
        let mut aliases = BTreeMap::new();
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
            );
            if result.is_err() {
                break;
            }
        }
        let commit = super::access::commit_components(
            self.world,
            &mut self.instances,
            current,
            self.asset_acquisition,
            false,
        );
        result.and(commit)
    }
}

impl WorldContext<'_> {
    pub(in crate::world) fn runtime_access(&mut self) -> SystemRuntimeAccess<'_> {
        SystemRuntimeAccess {
            world: self.world,
            instances: SystemInstanceAccess {
                before: self.instances.before,
                current: self.instances.current,
                after: self.instances.after,
            },
            asset_acquisition: self.asset_acquisition,
            data_sources: self.data_sources,
        }
    }

    /// Stage only affected pre-evaluation inputs; unrelated evaluated storage stays live.
    pub(in crate::world) fn stage_underlying_components(
        &mut self,
        targets: impl IntoIterator<Item = (EntityId, u16)>,
    ) {
        for (entity, component) in targets {
            let value = self.read().underlying_input_component(entity, component);
            if let Some(layer) = self
                .world
                .state
                .entities
                .get_mut(&entity)
                .and_then(|record| record.layers.get_mut(&component))
            {
                layer.inputs.stage(value);
            }
        }
    }

    pub(in crate::world) fn apply_cleanup_commands(&mut self, commands: &[Command]) {
        assert!(!self.world.updating, "cleanup requires a World boundary");
        self.world.forced_cleanup = true;
        if self.world.command_stream.is_none() {
            self.instances
                .visit_scoped(self.world.identity, None, |system, _| system.begin_batch());
        }
        let mut aliases = BTreeMap::new();
        let mut created = Vec::new();
        for command in commands {
            let _ =
                self.runtime_access()
                    .apply_operation(None, command, &mut aliases, &mut created);
        }
        let _ = self.commit_pending_changes();
        self.world.forced_cleanup = false;
    }

    fn apply_batch(&mut self, batch: Batch, tick: u64) -> BatchOutcome {
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
                state_overlays: Vec::new(),
            };
        }
        #[cfg(feature = "diagnostics")]
        if !batch.operations.is_empty() {
            crate::diagnostic!(
                Debug,
                "[IPP core] batch.begin batch={} operations={}",
                batch.id,
                batch.operations.len()
            );
        }
        if self.world.command_stream.is_none() {
            self.instances
                .visit_scoped(self.world.identity, None, |system, _| system.begin_batch());
        }
        let mut aliases = self
            .world
            .command_stream
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default();
        let mut created = Vec::new();
        let mut result = Ok(());
        for (operation, command) in batch.operations.iter().enumerate() {
            if let Err(reason) =
                self.runtime_access()
                    .apply_operation(None, command, &mut aliases, &mut created)
            {
                result = Err(BatchError {
                    aliases: Vec::new(),
                    scope: crate::BatchErrorScope::Operation,
                    operation: Some(operation),
                    reason,
                });
                break;
            }
        }
        if let Some(retained) = &mut self.world.command_stream {
            *retained = aliases;
        }
        let validation = self.commit_pending_changes();
        #[cfg(feature = "diagnostics")]
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
        #[cfg(feature = "diagnostics")]
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
            state_overlays: Vec::new(),
        };
        self.instances
            .visit_scoped(self.world.identity, None, |system, _| {
                system.finish_batch(&mut outcome)
            });
        if crate::allocation_optimizations_enabled() {
            self.recycle_command_buffer(batch.operations);
        }
        outcome
    }

    fn dispatch_phase(
        &mut self,
        dt: f64,
        phase: SystemFramePhase,
        report: &mut WorldUpdateReport,
    ) -> Result<(), ErrorReason> {
        for position in 0..self.instances.before.len() {
            let index = if matches!(phase, SystemFramePhase::Restore) {
                self.instances.before.len() - 1 - position
            } else {
                position
            };
            let (systems_before_current, tail) = self.instances.before.split_at_mut(index);
            let (current_system, systems_after_current) =
                tail.split_first_mut().expect("schedule index");
            let mut context = systems::SystemUpdateContext {
                world: SystemRuntimeAccess {
                    world: self.world,
                    instances: SystemInstanceAccess {
                        before: systems_before_current,
                        current: Some(current_system.id),
                        after: systems_after_current,
                    },
                    asset_acquisition: self.asset_acquisition,
                    data_sources: self.data_sources,
                },
                dt: &dt,
                dependent: index,
            };
            #[cfg(feature = "profiling")]
            let _measurement =
                crate::profiling::Stage::new(index * 6 + phase as usize, current_system.id.0);
            match phase {
                SystemFramePhase::Check => current_system.system.prepare_frame(&mut context)?,
                SystemFramePhase::Accept => current_system.system.accept_ingress(&mut context),
                SystemFramePhase::Restore => current_system.system.prepare_mutation(&mut context),
                SystemFramePhase::Prepare => current_system.system.prepare_evaluation(&mut context),
                SystemFramePhase::Evaluate => current_system.system.update(&mut context),
                SystemFramePhase::Finish => {
                    current_system.system.finish_update(&mut context, report)
                }
            }
        }
        Ok(())
    }

    /// Whether ordered world ingress or subsystem-deferred input must drain
    /// before a later command chunk may be admitted. The Host checks this
    /// before scoping a chunk so deferred chunks can wait without losing
    /// their scoped state; `apply_command_chunk` enforces the same gate.
    pub fn has_deferred_world_input(&self) -> bool {
        !self.world.queue.is_empty()
            || self
                .instances
                .before
                .iter()
                .any(|instance| instance.system.has_deferred_input())
    }

    /// Apply one buffer at a Host-controlled mutation boundary without evaluation.
    /// The Host must finish or abort the stream before calling `step` again.
    /// Aliases span the stream; outcomes contain only identities from this buffer.
    /// A nonempty queue or subsystem-deferred input is transient backpressure
    /// (`Capacity`), never a terminal rejection: the Host defers the chunk
    /// until earlier input drains. Only calling while updating is a
    /// programming error (`InvalidValue`).
    pub fn apply_command_chunk(&mut self, batch: Batch) -> Result<BatchOutcome, ErrorReason> {
        if self.world.updating {
            return Err(ErrorReason::InvalidValue);
        }
        if self.has_deferred_world_input() {
            return Err(ErrorReason::Capacity);
        }

        self.enqueue(batch)?;
        if let Err(error) = self.prepare_command_stream() {
            self.discard_command_chunk();
            return Err(error);
        }

        let Some(Ingress::Batch(batch)) = self.world.queue.pop_front() else {
            unreachable!("exclusive command buffer ingress");
        };
        Ok(self.apply_batch(batch, self.world.tick))
    }

    /// Apply one ordered subsystem-command buffer under the Host's existing
    /// logical stream gate. The Host must finish or abort the stream before
    /// calling `step`; a failed command stops the buffer without rollback.
    pub fn apply_system_command_chunk<T: 'static>(
        &mut self,
        system: systems::SystemId,
        session: u64,
        request_id: u64,
        commands: Vec<T>,
    ) -> Result<systems::SystemCommandOutcome, ErrorReason> {
        if self.world.updating {
            return Err(ErrorReason::InvalidValue);
        }
        if self.has_deferred_world_input() {
            return Err(ErrorReason::Capacity);
        }
        self.prepare_command_stream()?;

        let mut applied = 0;
        let mut result = Ok(());
        for command in &commands {
            result = self.apply_system_command(system, session, command);
            if result.is_err() {
                break;
            }
            applied += 1;
        }
        Ok(systems::SystemCommandOutcome {
            session,
            request_id,
            applied,
            result,
        })
    }

    fn prepare_command_stream(&mut self) -> Result<(), ErrorReason> {
        self.prepare_update(0.0)?;
        if !self.world.mutation_prepared {
            self.dispatch_phase(
                0.0,
                SystemFramePhase::Restore,
                &mut WorldUpdateReport::default(),
            )?;
            self.world.mutation_prepared = true;
        }

        if self.world.command_stream.is_none() {
            self.instances
                .visit_scoped(self.world.identity, None, |system, _| system.begin_batch());
            self.world.command_stream = Some(BTreeMap::new());
        }
        Ok(())
    }

    fn apply_system_command(
        &mut self,
        system: systems::SystemId,
        session: u64,
        command: &dyn std::any::Any,
    ) -> Result<(), ErrorReason> {
        if let Some(reason) = self.world.fault {
            return Err(reason);
        }
        self.with_system_dyn(system, |current, world| {
            current.command(
                &mut systems::SystemCommandContext {
                    world,
                },
                session,
                command,
            )
        })
        .unwrap_or(Err(ErrorReason::InvalidValue))
    }

    fn discard_command_chunk(&mut self) {
        let batch = match self
            .world
            .queue
            .pop_front()
            .expect("command chunk ingress must remain at the queue head")
        {
            Ingress::Batch(batch) => batch,
            Ingress::System {
                ..
            }
            | Ingress::SystemBatch {
                ..
            } => unreachable!("exclusive command chunk ingress"),
        };
        self.recycle_command_buffer(batch.operations);
    }

    /// Release the evaluation gate, retaining every applied effect on all exits.
    pub fn finish_command_stream(&mut self) {
        self.world.command_stream = None;
        self.release_state_overlay_owners([]);
    }

    /// Admit queued inputs through subsystem hooks before shared service progression.
    pub fn prepare_update(&mut self, dt: f64) -> Result<(), ErrorReason> {
        if self.world.updating || !dt.is_finite() || dt < 0.0 || !(self.world.time + dt).is_finite()
        {
            return Err(ErrorReason::InvalidValue);
        }
        self.world
            .tick
            .checked_add(1)
            .ok_or(ErrorReason::Capacity)?;
        if self.world.prepared_frame {
            return Ok(());
        }
        let mut report = WorldUpdateReport::default();
        self.dispatch_phase(dt, SystemFramePhase::Check, &mut report)?;
        self.dispatch_phase(dt, SystemFramePhase::Accept, &mut report)?;
        self.world.prepared_frame = true;
        Ok(())
    }

    /// Commit ordered ingress, evaluate the fixed schedule, then detach subsystem observations.
    pub fn step(&mut self, dt: f64) -> Result<WorldUpdateReport, ErrorReason> {
        if self.world.command_stream.is_some() {
            return Err(ErrorReason::InvalidValue);
        }
        self.prepare_update(dt)?;
        self.owns_update = true;
        self.world.updating = true;
        let mut report = WorldUpdateReport {
            tick: self.world.tick + 1,
            time: self.world.time + dt,
            ..Default::default()
        };
        if self.world.fault.is_none() && !self.world.mutation_prepared {
            self.dispatch_phase(dt, SystemFramePhase::Restore, &mut report)?;
        }
        while let Some(ingress) = self.world.queue.pop_front() {
            match ingress {
                Ingress::Batch(batch) => report.outcomes.push(self.apply_batch(batch, report.tick)),
                Ingress::System {
                    system,
                    session,
                    request_id,
                    command,
                } => {
                    let result = self.apply_system_command(system, session, command.as_ref());
                    if request_id != 0 {
                        report
                            .system_command_outcomes
                            .push(systems::SystemCommandOutcome {
                                session,
                                request_id,
                                applied: usize::from(result.is_ok()),
                                result,
                            });
                    }
                }
                Ingress::SystemBatch {
                    system,
                    session,
                    request_id,
                    commands,
                } => {
                    let mut applied = 0;
                    let mut result = Ok(());
                    for command in commands {
                        result = self.apply_system_command(system, session, command.as_ref());
                        if result.is_err() {
                            break;
                        }
                        applied += 1;
                    }
                    if request_id != 0 {
                        report
                            .system_command_outcomes
                            .push(systems::SystemCommandOutcome {
                                session,
                                request_id,
                                applied,
                                result,
                            });
                    }
                }
            }
        }
        if self.world.fault.is_none() {
            self.dispatch_phase(dt, SystemFramePhase::Prepare, &mut report)?;
            self.world.accepting_removals = true;
            self.dispatch_phase(dt, SystemFramePhase::Evaluate, &mut report)?;
            self.world.accepting_removals = false;
            self.drain_deferred_removals();
        } else {
            report.time = self.world.time;
        }
        self.world.tick = report.tick;
        self.world.time = report.time;
        if self.world.fault.is_none() {
            self.dispatch_phase(dt, SystemFramePhase::Finish, &mut report)?;
        }
        self.world.prepared_frame = false;
        self.world.mutation_prepared = false;
        self.world.updating = false;
        self.owns_update = false;
        Ok(report)
    }

    pub(in crate::world) fn with_system_dyn<R>(
        &mut self,
        id: systems::SystemId,
        operation: impl FnOnce(&mut dyn System, SystemRuntimeAccess<'_>) -> R,
    ) -> Option<R> {
        let index = self
            .instances
            .before
            .iter()
            .position(|instance| instance.id == id)?;
        let (before, tail) = self.instances.before.split_at_mut(index);
        let (instance, after) = tail.split_first_mut()?;
        Some(operation(
            instance.system.as_mut(),
            SystemRuntimeAccess {
                world: self.world,
                instances: SystemInstanceAccess {
                    before,
                    current: Some(id),
                    after,
                },
                asset_acquisition: self.asset_acquisition,
                data_sources: self.data_sources,
            },
        ))
    }
}

#[derive(Clone, Copy)]
enum SystemFramePhase {
    Check,
    Accept,
    Restore,
    Prepare,
    Evaluate,
    Finish,
}
