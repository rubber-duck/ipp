//! Production-library capture ownership and allocator isolation across native threads.
#![cfg(feature = "instrumentation")]

use ipp_core::{HostRuntime, profiling};

#[global_allocator]
static ALLOCATOR: profiling::CountingAllocator = profiling::CountingAllocator;

#[test]
fn native_hosts_on_distinct_threads_exclude_foreign_phases_and_allocations() {
    let mut owner = HostRuntime::new();
    let selected: Vec<_> = owner.system_ids().collect();
    owner.create_world(Default::default(), &selected).unwrap();
    let owner_id = owner.identity();
    let (sender, receiver) = std::sync::mpsc::channel();
    let (continue_sender, continue_receiver) = std::sync::mpsc::channel();
    let foreign = std::thread::spawn(move || {
        let mut host = HostRuntime::new();
        let selected: Vec<_> = host.system_ids().collect();
        host.create_world(Default::default(), &selected).unwrap();
        sender.send(host.identity()).unwrap();
        continue_receiver.recv().unwrap();
        for _ in 0..10 {
            host.frame(0.0).unwrap();
        }
        host.create_world(Default::default(), &selected).unwrap();
        host.frame(0.0).unwrap();
        std::hint::black_box(vec![1_u8; 32768]);
    });
    let foreign_id = receiver.recv().unwrap();
    profiling::reset_for_host(true, owner_id);
    continue_sender.send(()).unwrap();
    for _ in 0..10 {
        owner.frame(0.0).unwrap();
    }
    foreign.join().unwrap();
    // A measured allocation runs actual production GlobalAlloc recording logic.
    {
        let _scope = owner.profile_scope();
        let _category = profiling::AllocationScope::new(193, "profile.native.owner");
        std::hint::black_box(vec![1_u8; 4096]);
    }
    profiling::pause();
    let allocations = profiling::allocations();
    assert!(allocations.0 > 0 && allocations.1 >= 4096);
    let mut hosts = Vec::new();
    let mut categories = Vec::new();
    profiling::visit_capture(
        |stage| {
            hosts.push(stage.context.host);
            true
        },
        |_, context, values| {
            categories.push((context, values));
            true
        },
    );
    assert!(hosts.contains(&owner_id));
    assert!(!hosts.contains(&foreign_id));
    assert!(hosts.iter().all(|host| *host == owner_id));
    assert!(
        categories
            .iter()
            .all(|(context, _)| context.host == 0 || context.host == owner_id)
    );
    assert!(
        categories
            .iter()
            .any(|(context, values)| context.host == owner_id && values[1] >= 4096)
    );
    assert_eq!(
        categories.iter().fold((0, 0), |sum, (_, values)| (
            sum.0 + values[0],
            sum.1 + values[1]
        )),
        allocations
    );
    let before = profiling::allocations();
    std::thread::spawn(|| std::hint::black_box(vec![1_u8; 1 << 20]))
        .join()
        .unwrap();
    assert_eq!(profiling::allocations(), before);
    drop(owner);
    profiling::release_capture();
}
