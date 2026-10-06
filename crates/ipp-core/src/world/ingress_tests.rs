use super::WorldLimits;
use crate::{Command, EntityMetadata};

#[test]
fn recycled_command_buffers_bound_retained_capacity_and_release_payloads() {
    let mut host = crate::HostRuntime::new();
    let id = host.create_world(WorldLimits::default(), &[]).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let mut bounded = Vec::with_capacity(256);
    bounded.push(Command::Create {
        alias: 1,
        metadata: EntityMetadata::default(),
        adopt: false,
    });
    let pointer = bounded.as_ptr();
    world.recycle_command_buffer(bounded);
    let reused = world.take_command_buffer();
    assert!(reused.is_empty());
    assert_eq!(reused.capacity(), 256);
    assert_eq!(reused.as_ptr(), pointer);
    world.recycle_command_buffer(reused);

    // An unusually large direct-core batch must not enlarge the reusable pool.
    world.recycle_command_buffer(Vec::with_capacity(100_000));
    let reused = world.take_command_buffer();
    assert_eq!(reused.as_ptr(), pointer);
    assert_eq!(world.take_command_buffer().capacity(), 0);

    // Hosts that decode pages elsewhere only return buffers; the pool stays bounded.
    for _ in 0..16 {
        world.recycle_command_buffer(Vec::with_capacity(crate::RECYCLED_COMMAND_BUFFER_COMMANDS));
    }
    let mut kept = 0;
    while world.take_command_buffer().capacity() > 0 {
        kept += 1;
    }
    assert_eq!(kept, 2);
}
