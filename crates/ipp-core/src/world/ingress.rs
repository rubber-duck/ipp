//! Ordered World ingress: explicit queue bounds, batch and System command
//! admission, and the recycled command-buffer pool.

use crate::world::systems;
use crate::world::{World, WorldContext};
use crate::{Batch, Command, ErrorReason};

/// Explicit upper bounds for ingress.
#[derive(Clone, Copy, Debug)]
pub struct WorldLimits {
    /// Maximum operations in one indivisible batch; defaults to no count quota.
    pub max_operations: usize,
    /// Maximum owned batch allocation estimate, including spare capacities.
    /// Defaults to no byte quota; allocation and representability still apply.
    pub max_batch_bytes: usize,
    /// Maximum queued batches, system commands and queries per frame.
    pub max_queued_batches: usize,
}

impl Default for WorldLimits {
    fn default() -> Self {
        Self {
            max_operations: usize::MAX,
            max_batch_bytes: usize::MAX,
            max_queued_batches: 64,
        }
    }
}

pub(in crate::world) enum Ingress {
    System {
        system: systems::SystemId,
        session: u64,
        request_id: u64,
        command: Box<dyn std::any::Any>,
    },
    SystemBatch {
        system: systems::SystemId,
        session: u64,
        request_id: u64,
        commands: Vec<Box<dyn std::any::Any>>,
    },
    Batch {
        batch: Batch,
        effect_sink: Option<Box<dyn crate::OperationEffectSink>>,
    },
}

/// Largest command capacity a World keeps for reuse: one full batch page, so
/// hosts can decode any page into a recycled buffer. Larger assembled batches
/// are released instead of cached.
pub const RECYCLED_COMMAND_BUFFER_COMMANDS: usize = 1024;

/// Recycled buffers a World keeps, at most 128 KiB of command slots each. Hosts
/// that decode pages away from the World return applied batches without taking
/// buffers, so the pool must not grow with the number of applied batches.
const RECYCLED_COMMAND_BUFFERS: usize = 2;

impl WorldContext<'_> {
    /// Reuse capacity from a consumed batch. The buffer contains no live commands.
    /// Hosts may grow it for larger input; queued batches retain exclusive ownership.
    pub fn take_command_buffer(&mut self) -> Vec<Command> {
        self.world.command_buffers.pop().unwrap_or_default()
    }

    /// Return unused decode capacity without retaining command payloads or identities.
    pub fn recycle_command_buffer(&mut self, mut commands: Vec<Command>) {
        commands.clear();
        if (1..=RECYCLED_COMMAND_BUFFER_COMMANDS).contains(&commands.capacity())
            && self.world.command_buffers.len() < RECYCLED_COMMAND_BUFFERS
        {
            self.world.command_buffers.push(commands);
        }
    }
}

impl WorldContext<'_> {
    /// Queue a complete batch, or explicitly reject it without changing the world.
    pub fn enqueue(&mut self, batch: Batch) -> Result<(), ErrorReason> {
        self.enqueue_admitted(batch, None)
    }

    /// Queue ingress with operation-time receipt validation and reliable delivery admission.
    pub fn enqueue_with_effect_sink(
        &mut self,
        batch: Batch,
        effect_sink: Box<dyn crate::OperationEffectSink>,
    ) -> Result<(), ErrorReason> {
        self.enqueue_admitted(batch, Some(effect_sink))
    }

    fn enqueue_admitted(
        &mut self,
        batch: Batch,
        effect_sink: Option<Box<dyn crate::OperationEffectSink>>,
    ) -> Result<(), ErrorReason> {
        if let Some(reason) = self.world.fault {
            return Err(reason);
        }
        // An unlimited byte quota admits every size, so skip the walk over every
        // operation and inserted value.
        let max_bytes = self.world.limits.max_batch_bytes;
        if self.world.queue.len() >= self.world.limits.max_queued_batches
            || batch.operations.len() > self.world.limits.max_operations
            || (max_bytes != usize::MAX
                && batch_bytes(&batch).is_none_or(|bytes| bytes > max_bytes))
        {
            if !batch.operations.is_empty() {
                crate::diagnostic!(
                    Warn,
                    "[IPP core] batch.reject batch={} operations={} reason=enqueue_budget",
                    batch.id,
                    batch.operations.len()
                );
            }
            return Err(ErrorReason::Capacity);
        }
        self.world.queue.push_back(Ingress::Batch {
            batch,
            effect_sink,
        });
        Ok(())
    }
}

impl WorldContext<'_> {
    /// Queue typed subsystem input alongside ordinary entity/component batches.
    pub fn enqueue_system_command<T: 'static>(
        &mut self,
        system: systems::SystemId,
        session: u64,
        command: T,
    ) -> Result<(), ErrorReason> {
        self.enqueue_system_command_with_reply(system, session, 0, command)
    }

    /// Queue a correlated subsystem command without depending on its event queue.
    pub fn enqueue_system_command_with_reply<T: 'static>(
        &mut self,
        system: systems::SystemId,
        session: u64,
        request_id: u64,
        command: T,
    ) -> Result<(), ErrorReason> {
        if !self.system_ids().any(|id| id == system) {
            return Err(ErrorReason::UnsupportedDependency);
        }
        if self.world.queue.len() >= self.world.limits.max_queued_batches {
            return Err(ErrorReason::Capacity);
        }
        self.world.queue.push_back(Ingress::System {
            system,
            session,
            request_id,
            command: Box::new(command),
        });
        Ok(())
    }

    /// Queue one ordered subsystem command group. Execution stops at the first
    /// failure and publishes one outcome with the successful applied prefix.
    pub fn enqueue_system_command_batch_with_reply<T: 'static>(
        &mut self,
        system: systems::SystemId,
        session: u64,
        request_id: u64,
        commands: Vec<T>,
    ) -> Result<(), ErrorReason> {
        if !self.system_ids().any(|id| id == system) {
            return Err(ErrorReason::UnsupportedDependency);
        }
        if commands.is_empty() || self.world.queue.len() >= self.world.limits.max_queued_batches {
            return Err(ErrorReason::Capacity);
        }
        self.world.queue.push_back(Ingress::SystemBatch {
            system,
            session,
            request_id,
            commands: commands
                .into_iter()
                .map(|command| Box::new(command) as Box<dyn std::any::Any>)
                .collect(),
        });
        Ok(())
    }

    /// Move owned subsystem events into the transport's temporary delivery phase.
    pub fn drain_system_events<T: 'static>(
        &mut self,
        system: systems::SystemId,
        session: u64,
    ) -> Vec<T> {
        self.instances
            .before
            .iter_mut()
            .chain(self.instances.after.iter_mut())
            .find(|instance| instance.id == system)
            .map(|instance| {
                instance
                    .system
                    .drain_events(session)
                    .into_iter()
                    .filter_map(|value| value.downcast::<T>().ok().map(|value| *value))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Fence pending commands and release every subsystem's state for one attachment.
    pub fn release_system_session(&mut self, session: u64) {
        self.world.queue.retain(|input| {
            !matches!(
                input,
                Ingress::System {session: origin, ..}
                    | Ingress::SystemBatch {session: origin, ..}
                    if *origin == session
            )
        });
        for instance in self
            .instances
            .before
            .iter_mut()
            .chain(self.instances.after.iter_mut())
        {
            instance.system.release_session(session);
        }
    }
}

impl World {
    pub(crate) fn queued_reference_worlds(
        &self,
    ) -> crate::host::reference_resolution::WorldReferenceRequests {
        let mut references = crate::host::reference_resolution::referenced_worlds(
            self.data.queue.iter().flat_map(|ingress| match ingress {
                Ingress::Batch {
                    batch,
                    ..
                } => batch.operations.as_slice(),
                _ => &[],
            }),
        );
        for ingress in &self.data.queue {
            let (system, commands) = match ingress {
                Ingress::System {
                    system,
                    command,
                    ..
                } => (*system, std::slice::from_ref(command)),
                Ingress::SystemBatch {
                    system,
                    commands,
                    ..
                } => (*system, commands.as_slice()),
                Ingress::Batch {
                    ..
                } => continue,
            };
            if let Some(instance) = self
                .schedule
                .instances
                .iter()
                .find(|instance| instance.id == system)
            {
                for command in commands {
                    instance
                        .system
                        .command_world_references(command.as_ref(), &mut |world| {
                            references.include(world)
                        });
                }
            }
        }
        references
    }
}

pub(super) fn batch_bytes(batch: &Batch) -> Option<usize> {
    batch.operations.iter().try_fold(
        batch
            .operations
            .capacity()
            .checked_mul(std::mem::size_of::<Command>())?,
        |bytes, operation| bytes.checked_add(operation.retained_heap_bytes()?),
    )
}

#[cfg(test)]
#[path = "ingress_tests.rs"]
mod tests;
