use super::*;
use ipp_core::services::io::IoWriteJob;
use ipp_host_session::services::task_scheduler::TaskSchedulerService;

fn finish(mut job: IoWriteJob<NativeFileIoWriter>) -> Result<(), String> {
    futures_lite::future::block_on(std::future::poll_fn(|cx| job.poll(cx)))
}

#[test]
fn staged_file_publication_replaces_only_completed_output() {
    let tasks = TaskSchedulerService::new();
    let destination = std::env::temp_dir().join(format!("ipp-output-{}.ippw", std::process::id()));
    std::fs::write(&destination, b"previous").unwrap();
    let bytes = vec![42; 180_000];
    finish(IoWriteJob::new(
        bytes.clone(),
        futures_lite::future::block_on(NativeFileIoWriter::new(
            &destination,
            tasks.schedulers().io(),
        ))
        .unwrap(),
    ))
    .unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), bytes);
    let mut cancelled = futures_lite::future::block_on(NativeFileIoWriter::new(
        &destination,
        tasks.schedulers().io(),
    ))
    .unwrap();
    let mut cx = Context::from_waker(std::task::Waker::noop());
    let _ = cancelled.poll_write(&mut cx, b"partial");
    cancelled.abort();
    drop(cancelled);
    assert_eq!(std::fs::read(&destination).unwrap(), bytes);
    std::fs::remove_file(destination).unwrap();
}

#[test]
fn failed_publication_preserves_destination() {
    let tasks = TaskSchedulerService::new();
    let directory =
        std::env::temp_dir().join(format!("ipp-output-directory-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    assert!(
        finish(IoWriteJob::new(
            vec![1, 2, 3],
            futures_lite::future::block_on(NativeFileIoWriter::new(
                &directory,
                tasks.schedulers().io()
            ))
            .unwrap()
        ))
        .is_err()
    );
    assert!(directory.is_dir());
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn authored_world_state_round_trips_through_native_file_publication() {
    let tasks = TaskSchedulerService::new();
    use ipp_core::services::world_serialization::WorldPersistenceLimits;
    use ipp_core::{Batch, Command, ComponentValue, EntityRef, HostRuntime, WorldCreateOptions};
    let limits = WorldPersistenceLimits::default();
    let mut host = HostRuntime::new();
    let world = host
        .create_world_with_options(
            Default::default(),
            WorldCreateOptions {
                symbolic_id: "file-world".into(),
                ..WorldCreateOptions::new([ipp_core::systems::constraints::ConstraintSystem::ID])
            },
        )
        .unwrap();
    let mut context = host.world_mut(world).unwrap();
    context
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::InsertComponent {
                    entity: EntityRef::Alias(1),
                    component: ComponentValue::SCALAR,
                    fields: vec![],
                    adopt: false,
                },
            ],
        })
        .unwrap();
    assert!(context.step(0.0).unwrap().outcomes[0].result.is_ok());
    let original = context.capture_world(limits).unwrap();
    drop(context);
    let captured = host.save_world(world, 123, limits).unwrap();
    let destination =
        std::env::temp_dir().join(format!("ipp-world-round-trip-{}.ippw", std::process::id()));
    finish(IoWriteJob::new(
        captured,
        futures_lite::future::block_on(NativeFileIoWriter::new(
            &destination,
            tasks.schedulers().io(),
        ))
        .unwrap(),
    ))
    .unwrap();
    let bytes = std::fs::read(&destination).unwrap();
    let mut restored_host = HostRuntime::new();
    let restored = restored_host
        .load_world(&bytes, 123, Default::default(), Default::default(), limits)
        .unwrap();
    assert_eq!(
        restored_host
            .world_mut(restored.root.id())
            .unwrap()
            .capture_world(limits)
            .unwrap(),
        original
    );
    std::fs::remove_file(destination).unwrap();
}
