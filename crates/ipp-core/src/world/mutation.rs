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
            super::access::commit_components(
                self.world,
                &mut self.instances,
                current,
                self.asset_acquisition,
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
            topology: self.topology,
            frame_context: self.frame_context,
            reference_worlds: self.reference_worlds.as_ref(),
        }
    }

    fn apply_batch(
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

    fn dispatch_phase(
        &mut self,
        dt: f64,
        phase: SystemFramePhase,
        report: &mut WorldUpdateReport,
    ) -> Result<(), ErrorReason> {
        for index in 0..self.instances.before.len() {
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
                    topology: self.topology,
                    frame_context: self.frame_context,
                    reference_worlds: self.reference_worlds.as_ref(),
                },
                dt: &dt,
                dependent: index,
            };
            #[cfg(feature = "instrumentation")]
            let _measurement = crate::profiling::Stage::system(
                current_system.profile_slot,
                phase as usize,
                current_system.id.0,
            );
            match phase {
                SystemFramePhase::Check => current_system.system.prepare_frame(&mut context)?,
                SystemFramePhase::Accept => current_system.system.accept_ingress(&mut context),
                SystemFramePhase::Prepare => current_system.system.prepare_evaluation(&mut context),
                SystemFramePhase::Evaluate => current_system.system.update(&mut context),
                SystemFramePhase::Finish => {
                    current_system.system.finish_update(&mut context, report)
                }
                SystemFramePhase::Observe => current_system
                    .system
                    .observe_frame(context.world.view(), report.tick),
            }
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
        let report = self.admit_frame(dt)?;
        self.evaluate_frame(dt, report)
    }

    pub(crate) fn admit_frame(&mut self, dt: f64) -> Result<WorldUpdateReport, ErrorReason> {
        self.prepare_update(dt)?;
        self.owns_update = true;
        self.world.updating = true;
        let mut report = WorldUpdateReport {
            tick: self.world.tick + 1,
            time: self.world.time + dt,
            ..Default::default()
        };
        while let Some(front) = self.world.queue.front() {
            if let Ingress::System {
                system,
                command,
                ..
            } = front
                && self
                    .instances
                    .before
                    .iter()
                    .find(|instance| instance.id == *system)
                    .is_some_and(|instance| !instance.system.command_ready(command.as_ref()))
            {
                break;
            }
            let ingress = self.world.queue.pop_front().expect("observed ingress head");
            match ingress {
                Ingress::Batch {
                    batch,
                    mut effect_sink,
                } => {
                    let sink = effect_sink
                        .as_mut()
                        .map(|sink| &mut **sink as &mut dyn crate::OperationEffectSink);
                    report
                        .outcomes
                        .push(self.apply_batch(batch, report.tick, sink));
                }
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
        self.world.updating = false;
        self.owns_update = false;
        Ok(report)
    }

    pub(crate) fn evaluate_frame(
        &mut self,
        dt: f64,
        mut report: WorldUpdateReport,
    ) -> Result<WorldUpdateReport, ErrorReason> {
        self.owns_update = true;
        self.world.updating = true;
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
            self.dispatch_phase(dt, SystemFramePhase::Observe, &mut report)?;
        }
        self.world.prepared_frame = false;
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
                topology: self.topology,
                frame_context: self.frame_context,
                reference_worlds: self.reference_worlds.as_ref(),
            },
        ))
    }
}

/// Frame phases in dispatch order; profiling numbers them in this order.
#[derive(Clone, Copy)]
enum SystemFramePhase {
    Check,
    Accept,
    Prepare,
    Evaluate,
    Finish,
    /// Read-only comparison of final stored values, after every System's Finish.
    Observe,
}
