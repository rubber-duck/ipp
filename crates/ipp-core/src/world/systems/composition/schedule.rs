//! Validated factory order and exclusively owned World instances.

use super::{System, SystemDependency, SystemFactory, SystemId, SystemScheduleError};
use std::{collections::BTreeMap, sync::Arc};

pub(in crate::world) struct SystemFactoryRegistration {
    pub factory: Arc<dyn SystemFactory>,
    pub id: SystemId,
    pub predecessors: Vec<usize>,
}

/// Host-owned reusable factories in deterministic dependency order.
pub struct SystemFactories {
    pub(in crate::world) ordered: Vec<SystemFactoryRegistration>,
}

impl SystemFactories {
    /// Validate the complete graph without constructing a single system.
    pub fn new(factories: Vec<Arc<dyn SystemFactory>>) -> Result<Self, SystemScheduleError> {
        let ids: Vec<_> = factories.iter().map(|factory| factory.id()).collect();
        let mut indices = BTreeMap::new();
        for (index, &id) in ids.iter().enumerate() {
            if indices.insert(id, index).is_some() {
                return Err(SystemScheduleError::Duplicate(id));
            }
        }

        let mut predecessors = vec![Vec::new(); factories.len()];
        for (index, factory) in factories.iter().enumerate() {
            for &dependency in factory.dependencies() {
                let (id, required) = match dependency {
                    SystemDependency::Required(id) => (id, true),
                    SystemDependency::After(id) => (id, false),
                };
                if id == ids[index] {
                    return Err(SystemScheduleError::SelfDependency(id));
                }
                if let Some(&predecessor) = indices.get(&id) {
                    if !predecessors[index].contains(&predecessor) {
                        predecessors[index].push(predecessor);
                    }
                } else if required {
                    return Err(SystemScheduleError::MissingRequired {
                        system: ids[index],
                        required: id,
                    });
                }
            }
        }

        let mut order = Vec::with_capacity(factories.len());
        let mut emitted = vec![false; factories.len()];
        while order.len() < factories.len() {
            let Some(next) = (0..factories.len())
                .find(|&index| !emitted[index] && predecessors[index].iter().all(|&p| emitted[p]))
            else {
                return Err(SystemScheduleError::Cycle(
                    ids.iter()
                        .enumerate()
                        .filter_map(|(index, &id)| (!emitted[index]).then_some(id))
                        .collect(),
                ));
            };
            emitted[next] = true;
            order.push(next);
        }

        let mut positions = vec![0; order.len()];
        for (position, &index) in order.iter().enumerate() {
            positions[index] = position;
        }
        Ok(Self {
            ordered: order
                .into_iter()
                .map(|index| SystemFactoryRegistration {
                    factory: Arc::clone(&factories[index]),
                    id: ids[index],
                    predecessors: predecessors[index].iter().map(|&p| positions[p]).collect(),
                })
                .collect(),
        })
    }

    /// Resolved order; unconstrained ties preserve registration order.
    pub fn ids(&self) -> impl ExactSizeIterator<Item = SystemId> + '_ {
        self.ordered.iter().map(|registration| registration.id)
    }

    /// Select registered factories for one World and validate the selected graph.
    pub fn select(&self, selected: &[SystemId]) -> Result<Self, SystemScheduleError> {
        let factories = selected
            .iter()
            .map(|&id| {
                self.ordered
                    .iter()
                    .find(|registration| registration.id == id)
                    .map(|registration| Arc::clone(&registration.factory))
                    .ok_or(SystemScheduleError::Unknown(id))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(factories)
    }
}

/// Owned entry. There is no clonable instance or state allocation handle.
pub(in crate::world) struct SystemInstance {
    pub id: SystemId,
    pub system: Box<dyn System>,
    #[cfg(feature = "instrumentation")]
    pub profile_slot: Option<usize>,
}

/// World-owned instances in their construction and update order.
pub struct SystemSchedule {
    pub(in crate::world) instances: Vec<SystemInstance>,
}

impl SystemSchedule {
    /// The immutable order used for every update of this World.
    pub fn ids(&self) -> impl ExactSizeIterator<Item = SystemId> + '_ {
        self.instances.iter().map(|instance| instance.id)
    }
}

/// Correlated outcome of an ordered system command, independent of event subscriptions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemCommandOutcome {
    /// Originating World attachment/session.
    pub session: u64,
    /// Caller correlation, with zero reserved for commands without replies.
    pub request_id: u64,
    /// Successful commands before completion or the first failed command.
    pub applied: usize,
    /// Actual applied-command result; failures do not imply rollback.
    pub result: Result<(), crate::ErrorReason>,
}
