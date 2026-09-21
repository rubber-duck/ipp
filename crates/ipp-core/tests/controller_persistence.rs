//! Controller snapshot budgets and bounded durable-target lookup.

use ipp_core::services::world_serialization::{
    WorldPersistenceLimits, WorldSerializedEntity, WorldSnapshot,
};
use ipp_core::systems::animation::*;
use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityMetadata, EntityPersistentId, EntityRef,
    HostRuntime,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct AllocationCounter;

thread_local! {
    static ALLOCATED: Cell<Option<usize>> = const { Cell::new(None) };
}

fn record_allocation(bytes: usize) {
    let _ = ALLOCATED.try_with(|allocated| {
        if let Some(previous) = allocated.get() {
            allocated.set(Some(previous.saturating_add(bytes)));
        }
    });
}

// SAFETY: Every allocation delegates to System with the caller's layout. The counter
// stores sizes only, never retains pointers, and does not alter ownership or aliasing.
unsafe impl GlobalAlloc for AllocationCounter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation(layout.size());
        // SAFETY: Forward the valid allocation layout without retaining its pointer.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: This is the live System allocation and original layout supplied by
        // the caller; forwarding ends its lifetime without retaining an alias.
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation(size);
        // SAFETY: Forward the uniquely owned live allocation and valid new size.
        // System handles invalidation of the old pointer when the allocation moves.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: AllocationCounter = AllocationCounter;

fn driver(target: EntityId, source: String) -> AnimationDriverDescription {
    AnimationDriverDescription {
        source,
        variant: 0,
        track: 0,
        target,
        property: AnimationTrackTarget::AnimationProperty(AnimationProperty {
            component: ComponentValue::SCALAR,
            offsets: vec![0],
        }),
        weight: 1.0,
        additive: false,
        reference_time: 0.0,
        repeat: false,
    }
}

#[test]
fn rejected_capture_does_not_allocate_the_controller_graph() {
    let mut host = HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 0,
                    metadata: EntityMetadata::default(),
                },
                Command::InsertComponent {
                    entity: EntityRef::Alias(0),
                    component: ComponentValue::SCALAR,
                    fields: Vec::new(),
                },
            ],
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    let target = world.entities()[0].id;
    for _ in 0..200 {
        world
            .create_animation_controller(AnimationControllerDescription {
                drivers: vec![driver(target, "x".repeat(2048))],
                ..Default::default()
            })
            .unwrap();
    }

    ALLOCATED.set(Some(0));
    let capture = world.capture_world(WorldPersistenceLimits {
        max_bytes: 4096,
    });
    let allocated = ALLOCATED.replace(None).unwrap();
    assert!(capture.unwrap_err().contains("Snapshot byte budget"));
    assert!(
        allocated < 32 << 10,
        "rejected capture allocated {allocated} bytes for a 4 KiB snapshot budget"
    );

    let snapshot = world.capture_world(Default::default()).unwrap();
    let animation = AnimationPersistentState::decode(
        &snapshot.systems[AnimationSystem::ID.0],
        WorldPersistenceLimits::default().max_bytes,
    )
    .unwrap();
    assert_eq!(animation.controllers.len(), 200);
    assert!(animation.controllers.iter().all(|controller| {
        controller.description.drivers[0].target.to_bits() == snapshot.entities[0].persistent_id.0
    }));
    assert_eq!(world.animation_controllers().len(), 200);
}

#[test]
fn many_controllers_resolve_late_durable_targets_and_reject_excluded_references() {
    let mut host = HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut snapshot = host
        .world_mut(id)
        .unwrap()
        .capture_world(Default::default())
        .unwrap();
    const ENTITIES: u64 = 100_000;
    snapshot.next_entity_id = ENTITIES;
    snapshot.entities = (1..=ENTITIES)
        .map(|id| WorldSerializedEntity {
            persistent_id: EntityPersistentId(id),
            metadata: EntityMetadata::default(),
            components: Vec::new(),
        })
        .collect();
    snapshot.entities.last_mut().unwrap().components =
        vec![ComponentValue::create(ComponentValue::SCALAR).unwrap()];
    let mut animation = AnimationPersistentState {
        next_id: 65,
        controllers: (1..65)
            .map(|id| AnimationControllerSnapshot {
                id: AnimationControllerId::from_bits(id),
                state: AnimationPlaybackStatus::Paused,
                time: 0.0,
                transition: None,
                description: AnimationControllerDescription {
                    drivers: (0..256)
                        .map(|_| driver(EntityId::from_bits(ENTITIES), "missing:clip".into()))
                        .collect(),
                    ..Default::default()
                },
            })
            .collect(),
        transitions: Vec::new(),
        directional_starts: Vec::new(),
    };

    let limits = WorldPersistenceLimits::default();
    snapshot.systems.insert(
        AnimationSystem::ID.0.to_owned(),
        animation.encode(limits.max_bytes).unwrap(),
    );
    let bytes = snapshot.encode(123, limits).unwrap();
    let mut decoded = WorldSnapshot::decode(&bytes, 123, limits).unwrap();
    assert_eq!(decoded, snapshot);

    animation.controllers[0].description.drivers[0].target = EntityId::from_bits(ENTITIES + 1);
    assert!(
        animation
            .validate_entities(&decoded.entities)
            .unwrap_err()
            .contains("excluded or missing entity")
    );
    animation.controllers[0].description.drivers[0].target = EntityId::from_bits(ENTITIES);
    decoded.entities.last_mut().unwrap().components.clear();
    assert!(
        animation
            .validate_entities(&decoded.entities)
            .unwrap_err()
            .contains("excluded or missing component")
    );
}
