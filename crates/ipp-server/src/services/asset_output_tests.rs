use super::*;
use ipp_core::services::data_source::DataWriteJob;
use std::task::Wake;

struct WriterWake(std::thread::Thread);

impl Wake for WriterWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

fn finish(mut job: DataWriteJob<NativeFileDataWriter>) -> Result<(), String> {
    let waker = Waker::from(Arc::new(WriterWake(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        match job.poll(&mut cx) {
            Poll::Ready(result) => return result,
            Poll::Pending => std::thread::park_timeout(std::time::Duration::from_secs(2)),
        }
    }
}

#[test]
fn staged_file_publication_replaces_only_completed_output() {
    let destination = std::env::temp_dir().join(format!("ipp-output-{}.ippw", std::process::id()));
    std::fs::write(&destination, b"previous").unwrap();
    let bytes = vec![42; 180_000];
    finish(DataWriteJob::new(
        bytes.clone(),
        NativeFileDataWriter::new(&destination).unwrap(),
    ))
    .unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), bytes);
    let mut cancelled = NativeFileDataWriter::new(&destination).unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    let _ = cancelled.poll_write(&mut cx, b"partial");
    cancelled.abort();
    drop(cancelled);
    assert_eq!(std::fs::read(&destination).unwrap(), bytes);
    std::fs::remove_file(destination).unwrap();
}

#[test]
fn failed_publication_preserves_destination() {
    let directory =
        std::env::temp_dir().join(format!("ipp-output-directory-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    assert!(
        finish(DataWriteJob::new(
            vec![1, 2, 3],
            NativeFileDataWriter::new(&directory).unwrap()
        ))
        .is_err()
    );
    assert!(directory.is_dir());
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn authored_world_state_round_trips_through_native_file_publication() {
    use ipp_core::services::world_serialization::WorldPersistenceLimits;
    use ipp_core::{Batch, Command, ComponentValue, EntityRef, HostRuntime, WorldCreateOptions};
    let limits = WorldPersistenceLimits::default();
    let mut host = HostRuntime::new();
    let world = host
        .create_world_with_options(
            Default::default(),
            WorldCreateOptions {
                symbolic_id: "file-world".into(),
                ..Default::default()
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
                },
                Command::InsertComponent {
                    entity: EntityRef::Alias(1),
                    component: ComponentValue::SCALAR,
                    fields: vec![],
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
    finish(DataWriteJob::new(
        captured,
        NativeFileDataWriter::new(&destination).unwrap(),
    ))
    .unwrap();
    let bytes = std::fs::read(&destination).unwrap();
    let mut restored_host = HostRuntime::new();
    let restored = restored_host
        .load_world(&bytes, 123, Default::default(), Default::default(), limits)
        .unwrap();
    assert_eq!(
        restored_host
            .world_mut(restored)
            .unwrap()
            .capture_world(limits)
            .unwrap(),
        original
    );
    std::fs::remove_file(destination).unwrap();
}
