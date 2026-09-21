use super::*;
use crate::{
    components::{BufferCounters, PreparedBuffer},
    systems::*,
};
use std::{
    rc::Rc,
    sync::{Arc, Mutex},
};

type Trace = Arc<Mutex<Vec<(&'static str, &'static str, usize)>>>;
type Target = Arc<Mutex<Option<EntityId>>>;

struct ProbeFactory {
    name: &'static str,
    request: bool,
    target: Target,
    trace: Trace,
}
struct Probe {
    name: &'static str,
    request: bool,
    target: Target,
    trace: Trace,
}

impl SystemFactory for ProbeFactory {
    fn id(&self) -> SystemId {
        SystemId(self.name)
    }

    fn create(
        &self,
        _context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(Probe {
            name: self.name,
            request: self.request,
            target: self.target.clone(),
            trace: self.trace.clone(),
        }))
    }
}

impl System for Probe {
    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        let Some(entity) = *self.target.lock().unwrap() else {
            return;
        };
        if self.request {
            context.world.defer_remove_entity(entity).unwrap();
            context.world.defer_remove_entity(entity).unwrap();
        }
        let buffer = context
            .world
            .world
            .components
            .prepared_buffer(entity.index() as usize)
            .unwrap();
        self.trace
            .lock()
            .unwrap()
            .push((self.name, "update", buffer.counters.released.get()));
    }

    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        let Some(entity) = *self.target.lock().unwrap() else {
            return;
        };
        if !context.changed_components().any(|(id, _)| id == entity) {
            return;
        }
        let buffer = context
            .world_data
            .components
            .prepared_buffer(entity.index() as usize)
            .unwrap();
        self.trace
            .lock()
            .unwrap()
            .push((self.name, "before", buffer.counters.released.get()));
        assert!(
            context
                .world()
                .effective_component(entity, ComponentValue::PREPARED_BUFFER)
                .is_some()
        );
        // Cleanup cannot enqueue another update-originated cascade.
        assert_eq!(
            context.world_data.defer_remove_entity(entity),
            Err(ErrorReason::InvalidValue)
        );
    }

    fn after_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        let Some(entity) = *self.target.lock().unwrap() else {
            return;
        };
        if context.changed_components().any(|(id, _)| id == entity) {
            assert!(
                context
                    .world_data
                    .components
                    .prepared_buffer(entity.index() as usize)
                    .is_none()
            );
            self.trace.lock().unwrap().push((self.name, "after", 1));
        }
    }
}

#[test]
fn queued_entity_storage_lives_through_every_update_and_invalidation_handler() {
    let target = Target::default();
    let trace = Trace::default();
    let mut factories = compiled_system_factories();
    for (name, request) in [("test.remove", true), ("test.later", false)] {
        factories.push(Arc::new(ProbeFactory {
            name,
            request,
            target: target.clone(),
            trace: trace.clone(),
        }));
    }
    let mut host = crate::HostRuntime::with_system_factories(factories).unwrap();
    let id = host.create_world(Default::default()).unwrap();
    let counters = Rc::new(BufferCounters::default());
    let mut world = host.world_mut(id).unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 0,
                    metadata: Default::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(0),
                    value: ComponentValue::PreparedBuffer(PreparedBuffer {
                        length: 16,
                        counters: counters.clone(),
                        allocation: None,
                    }),
                },
            ],
        })
        .unwrap();
    let entity = world.step(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1;
    *target.lock().unwrap() = Some(entity);
    world.step(0.0).unwrap();
    assert_eq!(
        *trace.lock().unwrap(),
        [
            ("test.remove", "update", 0),
            ("test.later", "update", 0),
            ("test.remove", "before", 0),
            ("test.later", "before", 0),
            ("test.remove", "after", 1),
            ("test.later", "after", 1),
        ]
    );
    assert_eq!(counters.released.get(), 1);
    assert!(world.inspect(entity).is_none());
    assert!(world.world.deferred_removals.is_empty());
}

#[test]
fn deferred_component_identity_and_generation_cannot_remove_replacements() {
    let mut host = crate::HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 0,
                    metadata: Default::default(),
                },
                Command::InsertComponent {
                    entity: EntityRef::Alias(0),
                    component: ComponentValue::SCALAR,
                    fields: Vec::new(),
                },
            ],
        })
        .unwrap();
    let entity = world.step(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1;
    world.world.accepting_removals = true;
    world
        .world
        .defer_remove_component(entity, ComponentValue::SCALAR)
        .unwrap();
    world
        .world
        .defer_remove_component(entity, ComponentValue::SCALAR)
        .unwrap();
    world.world.accepting_removals = false;
    assert_eq!(world.world.deferred_removals.len(), 1);
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![Command::InsertComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::SCALAR,
                fields: Vec::new(),
            }],
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    assert!(
        world
            .inspect(entity)
            .unwrap()
            .effective
            .iter()
            .any(|value| value.type_id() == ComponentValue::SCALAR)
    );

    world.world.accepting_removals = true;
    world.world.defer_remove_entity(entity).unwrap();
    world.world.accepting_removals = false;
    world
        .enqueue(Batch {
            id: 3,
            operations: vec![
                Command::Delete {
                    entity: EntityRef::Handle(entity),
                },
                Command::Create {
                    alias: 0,
                    metadata: Default::default(),
                },
            ],
        })
        .unwrap();
    let replacement = world.step(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1;
    assert_eq!(entity.index(), replacement.index());
    assert_ne!(entity, replacement);
    assert!(world.inspect(replacement).is_some());
    assert_eq!(
        world.world.defer_remove_entity(replacement),
        Err(ErrorReason::InvalidValue)
    );
}
