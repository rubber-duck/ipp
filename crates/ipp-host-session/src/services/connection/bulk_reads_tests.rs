use super::*;
use ipp_protocol::bulk_read::{BulkReadDescriptor, CHUNK_BYTES, PIPELINE_CHUNKS};
use std::sync::Arc;

fn wire(connection: u64, id: u64, read: u64, operation: u8, position: u64, eof: bool) -> Vec<u8> {
    let mut bytes = b"IPDR".to_vec();
    bytes.extend(connection.to_le_bytes());
    bytes.extend(id.to_le_bytes());
    bytes.extend(read.to_le_bytes());
    bytes.push(operation);
    if operation != 2 {
        bytes.extend(position.to_le_bytes());
    }
    if operation == 1 {
        bytes.push(u8::from(eof));
    }
    bytes
}

fn receive(
    host: &mut Host<TestHostServices>,
    descriptor: BulkReadDescriptor,
    id: u64,
    operation: u8,
    position: u64,
    eof: bool,
) {
    host.receive_connection(
        descriptor.reference.connection,
        &wire(
            descriptor.reference.connection,
            id,
            descriptor.reference.read,
            operation,
            position,
            eof,
        ),
    )
    .unwrap();
}

#[test]
fn independent_recipient_leases_retain_final_bytes_until_consumed_eof_and_physical_delivery() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    open(&mut host, 2);
    let backing = Arc::new(vec![17; 4096]);
    let weak = Arc::downgrade(&backing);
    let first = host.publish_connection_bytes(1, backing.clone()).unwrap();
    let second = host.publish_connection_bytes(2, backing.clone()).unwrap();
    let usage = host.bulk_read_usage();
    assert_eq!(usage.backing_bytes, backing.capacity() + 2 * 2048);
    drop(backing);
    receive(&mut host, first, 1, 0, 0, false);
    host.progress_resources().unwrap();
    let delivery = host.take_connection_response(1).unwrap();
    assert_eq!(&delivery[42..], &[17; 4096]);
    host.maintain_connections(std::time::Duration::from_secs(3600));
    assert_eq!(host.bulk_read_usage().leases, 2);
    receive(&mut host, first, 2, 1, 4097, true);
    assert_eq!(host.take_connection_response(1).unwrap()[28], 2);
    receive(&mut host, first, 3, 1, 4096, false);
    assert_eq!(host.take_connection_response(1).unwrap()[28], 1);
    assert_eq!(
        host.bulk_read_usage().leases,
        2,
        "prefix alone does not consume EOF"
    );
    receive(&mut host, first, 4, 1, 4096, true);
    host.take_connection_response(1).unwrap();
    host.progress_resources().unwrap();
    assert!(
        weak.upgrade().is_some(),
        "peer lease retains shared backing"
    );
    let foreign = wire(1, 5, second.reference.read, 0, 0, false);
    host.receive_connection(1, &foreign).unwrap();
    assert_eq!(host.take_connection_response(1).unwrap()[28], 2);
    host.close_connection(2);
    host.progress_resources().unwrap();
    assert!(weak.upgrade().is_none());
    assert_eq!(host.bulk_read_usage().backing_bytes, 0);
    assert!(host.bulk_read_usage().delivery_bytes >= delivery.bytes.capacity());
    assert_eq!(
        &delivery[42..],
        &[17; 4096],
        "ack/disconnect never invalidates delivery memory"
    );
    drop(delivery);
    assert_eq!(host.bulk_read_usage().delivery_bytes, 0);
}

#[test]
fn bounded_pipeline_preserves_control_and_requires_cumulative_consumption() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let descriptor = host
        .publish_connection_bytes(1, Arc::new(vec![11; CHUNK_BYTES * 12]))
        .unwrap();
    for index in 0..PIPELINE_CHUNKS {
        receive(
            &mut host,
            descriptor,
            index as u64 + 1,
            0,
            (index * CHUNK_BYTES) as u64,
            false,
        );
    }
    receive(
        &mut host,
        descriptor,
        99,
        0,
        (PIPELINE_CHUNKS * CHUNK_BYTES) as u64,
        false,
    );
    assert_eq!(host.take_connection_response(1).unwrap()[28], 2);
    host.receive_connection(
        1,
        &host::encode_host_request(&HostRequest {
            connection: 1,
            request_id: 1000,
            body: HostRequestBody::ListWorlds {
                after: 0,
            },
        })
        .unwrap(),
    )
    .unwrap();
    host.tick_worlds(0.0).unwrap();
    let mut control_seen = false;
    while let Some(response) = host.take_connection_response(1) {
        if response.starts_with(host::HOST_RESPONSE_MAGIC) {
            assert!(matches!(
                host::decode_host_response(&response, 1).unwrap().body,
                HostResponseBody::Worlds { .. }
            ));
            control_seen = true;
        }
    }
    assert!(control_seen, "bulk window leaves control reply capacity");
    receive(
        &mut host,
        descriptor,
        100,
        1,
        (PIPELINE_CHUNKS * CHUNK_BYTES) as u64,
        false,
    );
    assert_eq!(host.take_connection_response(1).unwrap()[28], 1);
    receive(
        &mut host,
        descriptor,
        101,
        0,
        (PIPELINE_CHUNKS * CHUNK_BYTES) as u64,
        false,
    );
    host.progress_resources().unwrap();
    let chunk = host.take_connection_response(1).unwrap();
    assert_eq!(&chunk[42..], vec![11; CHUNK_BYTES]);
}

#[test]
fn unknown_length_owned_reader_establishes_eof_without_a_total_source_limit() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let descriptor = host
        .publish_connection_reader(
            1,
            Box::new(ipp_core::services::io::BufferIoReader::new(vec![3; 97])),
            None,
        )
        .unwrap();
    receive(&mut host, descriptor, 1, 0, 0, false);
    receive(&mut host, descriptor, 2, 0, CHUNK_BYTES as u64, false);
    assert_eq!(host.take_connection_response(1).unwrap()[28], 2);
    host.progress_resources().unwrap();
    let chunk = host.take_connection_response(1).unwrap();
    assert_eq!(chunk[37], 1);
    assert_eq!(&chunk[42..], &[3; 97]);
    receive(&mut host, descriptor, 3, 1, 97, true);
    host.take_connection_response(1).unwrap();
    host.progress_resources().unwrap();
    assert_eq!(host.bulk_read_usage().leases, 0);
}

#[test]
fn severe_policy_revokes_explicitly_without_releasing_outstanding_delivery_storage() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let descriptor = host
        .publish_connection_bytes(1, Arc::new(vec![5; 8192]))
        .unwrap();
    receive(&mut host, descriptor, 1, 0, 0, false);
    host.progress_resources().unwrap();
    let delivery = host.take_connection_response(1).unwrap();
    host.set_bulk_read_policy(crate::services::bulk_read::BulkReadPolicy {
        severe_retained_bytes: std::num::NonZeroUsize::new(4096).unwrap(),
    });
    let revocation = host.take_connection_response(1).unwrap();
    assert_eq!(
        u64::from_le_bytes(revocation[12..20].try_into().unwrap()),
        0
    );
    assert_eq!(revocation[28], 2);
    assert_eq!(host.bulk_read_usage().leases, 0);
    host.progress_resources().unwrap();
    assert_eq!(host.bulk_read_usage().backing_bytes, 0);
    assert_eq!(&delivery[42..], &[5; 8192]);
    receive(&mut host, descriptor, 2, 0, 0, false);
    assert_eq!(host.take_connection_response(1).unwrap()[28], 2);
    assert!(
        host.publish_connection_bytes(1, Arc::new(vec![9; 8192]))
            .is_err()
    );
    drop(revocation);
    drop(delivery);
}

#[test]
fn pending_output_cancellation_retains_charge_and_publication_adopts_without_double_counting() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let allocation = host.reserve_bulk_output(8192).unwrap();
    assert_eq!(host.bulk_read_usage().backing_bytes, 8192);
    allocation.resize(4096).unwrap();
    let cancellation = allocation.cancellation();
    let descriptor = host
        .publish_connection_bytes_from_allocation(1, Arc::new(vec![1; 4096]), allocation)
        .unwrap();
    assert_eq!(host.bulk_read_usage().backing_bytes, 4096 + 2048);
    assert!(!cancellation.is_cancelled());
    let pending = host.reserve_bulk_output(8192).unwrap();
    let cancelled = pending.cancellation();
    host.signal_severe_memory_pressure();
    assert!(cancelled.is_cancelled());
    assert!(
        host.bulk_read_usage().backing_bytes >= 8192,
        "revocation never pretends pending scratch disappeared"
    );
    assert!(pending.resize(0).is_err());
    drop(pending);
    host.progress_resources().unwrap();
    assert_eq!(host.bulk_read_usage().backing_bytes, 0);
    receive(&mut host, descriptor, 1, 0, 0, false);
    let mut saw_error = false;
    while let Some(reply) = host.take_connection_response(1) {
        saw_error |= reply[28] == 2;
    }
    assert!(saw_error);
}

#[test]
fn pending_reader_rebinds_replacement_storage_without_resizing_a_shared_peer_charge() {
    use ipp_core::services::io::{
        BufferIoReader, IoError, IoReadBackend, IoReadWindow, IoReaderStorage,
    };
    use std::{
        num::NonZeroUsize,
        task::{Context, Poll},
    };

    struct ReplacingReader {
        current: BufferIoReader,
        replacement: Option<BufferIoReader>,
    }

    impl IoReadBackend for ReplacingReader {
        fn poll_ready(
            &mut self,
            cx: &mut Context<'_>,
            minimum: NonZeroUsize,
        ) -> Poll<Result<(), IoError>> {
            if let Some(replacement) = self.replacement.take() {
                self.current = replacement;
                // Allocated replacement remains pending, so its charge must be
                // visible before any byte readiness or consumer window exists.
                return Poll::Pending;
            }
            self.current.poll_ready(cx, minimum)
        }

        fn window(&mut self) -> IoReadWindow<'_> {
            self.current.window()
        }

        fn retained_storage(&self) -> Option<IoReaderStorage> {
            self.current.retained_storage()
        }
    }

    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    open(&mut host, 2);
    let shared = Arc::new(vec![3; 4096]);
    let first = BufferIoReader::from_backing(shared.clone(), 0..shared.len()).unwrap();
    let peer = BufferIoReader::from_backing(shared.clone(), 0..shared.len()).unwrap();
    let replacement = vec![7; 8192];
    let replacement_capacity = replacement.capacity();
    let first = host
        .publish_connection_reader(
            1,
            Box::new(ReplacingReader {
                current: first,
                replacement: Some(BufferIoReader::new(replacement)),
            }),
            None,
        )
        .unwrap();
    let peer = host
        .publish_connection_reader(2, Box::new(peer), Some(shared.len() as u64))
        .unwrap();
    assert_eq!(
        host.bulk_read_usage().backing_bytes,
        shared.capacity() + 2 * 2048
    );
    receive(&mut host, first, 1, 0, 0, false);
    host.progress_resources().unwrap();
    assert_eq!(
        host.bulk_read_usage().backing_bytes,
        shared.capacity() + replacement_capacity + 2 * 2048
    );
    // Drop the replacement while the peer still owns the original allocation.
    receive(&mut host, first, 2, 2, 0, false);
    host.progress_resources().unwrap();
    assert_eq!(
        host.bulk_read_usage().backing_bytes,
        shared.capacity() + 2048
    );
    receive(&mut host, peer, 3, 0, 0, false);
    host.progress_resources().unwrap();
    let bytes = host.take_connection_response(2).unwrap();
    assert_eq!(&bytes[42..], shared.as_slice());
}

#[test]
fn idle_stream_storage_is_charged_before_and_after_observer_registration() {
    use ipp_core::services::io::{IoReadOptions, StreamIoReader};

    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let (reader, input) = StreamIoReader::new(IoReadOptions {
        max_bytes: None,
        recovery: false,
    });
    let descriptor = host
        .publish_connection_reader(1, Box::new(reader), None)
        .unwrap();
    assert_eq!(host.bulk_read_usage().backing_bytes, 2048);

    // Growth before the task's first poll is caught by its initial snapshot.
    let filling = input.reserve(1024).unwrap().unwrap();
    host.progress_resources().unwrap();
    assert_eq!(
        host.bulk_read_usage().backing_bytes,
        input.buffered_bytes() + 2048
    );
    drop(filling);

    // The next allocation wakes storage observation despite no read request,
    // byte readiness or source consumption. The uncommitted fill is charged.
    let filling = input.reserve(8192).unwrap().unwrap();
    host.progress_resources().unwrap();
    assert!(input.buffered_bytes() >= 8192);
    assert_eq!(
        host.bulk_read_usage().backing_bytes,
        input.buffered_bytes() + 2048
    );
    assert!(host.take_connection_response(1).is_none());
    drop(filling);
    receive(&mut host, descriptor, 1, 2, 0, false);
    host.progress_resources().unwrap();
    assert_eq!(host.bulk_read_usage().backing_bytes, 0);
    assert!(!input.is_open());
}

#[test]
fn idle_generic_reader_tracks_growth_shrink_and_shared_identity_without_reading() {
    use ipp_core::services::io::{
        BufferIoReader, IoError, IoReadBackend, IoReadWindow, IoReaderStorage, IoStorageId,
    };
    use std::{
        num::NonZeroUsize,
        sync::Mutex,
        task::{Context, Poll, Waker},
    };

    struct Storage {
        backing: Arc<Vec<u8>>,
        waker: Option<Waker>,
        observations: usize,
    }

    impl Storage {
        fn replace(&mut self, backing: Arc<Vec<u8>>) {
            self.backing = backing;
            if let Some(waker) = &self.waker {
                waker.wake_by_ref();
            }
        }

        fn resize(&mut self, length: usize) {
            let backing = Arc::get_mut(&mut self.backing).unwrap();
            backing.resize(length, 9);
            backing.shrink_to_fit();
            if let Some(waker) = &self.waker {
                waker.wake_by_ref();
            }
        }
    }

    struct DynamicReader(Arc<Mutex<Storage>>);

    impl IoReadBackend for DynamicReader {
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
            _minimum: NonZeroUsize,
        ) -> Poll<Result<(), IoError>> {
            panic!("Idle storage accounting must not request source readiness");
        }

        fn window(&mut self) -> IoReadWindow<'_> {
            panic!("Idle storage accounting must not acquire a source window");
        }

        fn register_storage_waker(&mut self, waker: &Waker) {
            self.0.lock().unwrap().waker = Some(waker.clone());
        }

        fn retained_storage(&self) -> Option<IoReaderStorage> {
            let mut storage = self.0.lock().unwrap();
            storage.observations += 1;
            Some(IoReaderStorage {
                identity: IoStorageId::of_backing(&storage.backing),
                bytes: storage.backing.capacity(),
                mapped_bytes: 0,
            })
        }
    }

    impl Drop for DynamicReader {
        fn drop(&mut self) {
            self.0.lock().unwrap().waker = None;
        }
    }

    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    open(&mut host, 2);
    let shared = Arc::new(vec![3; 1024]);
    let storage = Arc::new(Mutex::new(Storage {
        backing: shared.clone(),
        waker: None,
        observations: 0,
    }));
    let first = host
        .publish_connection_reader(1, Box::new(DynamicReader(storage.clone())), None)
        .unwrap();
    let peer = host
        .publish_connection_reader(
            2,
            Box::new(BufferIoReader::from_backing(shared.clone(), 0..shared.len()).unwrap()),
            None,
        )
        .unwrap();
    host.progress_resources().unwrap();
    assert!(storage.lock().unwrap().waker.is_some());
    assert_eq!(host.bulk_read_usage().backing_bytes, 1024 + 2 * 2048);

    storage.lock().unwrap().replace(Arc::new(vec![7; 8192]));
    host.progress_resources().unwrap();
    assert_eq!(host.bulk_read_usage().backing_bytes, 1024 + 8192 + 2 * 2048);
    let identity = IoStorageId::of_backing(&storage.lock().unwrap().backing);
    storage.lock().unwrap().resize(16384);
    host.progress_resources().unwrap();
    assert_eq!(
        host.bulk_read_usage().backing_bytes,
        1024 + 16384 + 2 * 2048
    );
    storage.lock().unwrap().resize(256);
    host.progress_resources().unwrap();
    assert_eq!(host.bulk_read_usage().backing_bytes, 1024 + 256 + 2 * 2048);
    assert!(identity == IoStorageId::of_backing(&storage.lock().unwrap().backing));

    // Rejoining the peer's exact backing must charge it only once.
    storage.lock().unwrap().replace(shared);
    host.progress_resources().unwrap();
    assert_eq!(host.bulk_read_usage().backing_bytes, 1024 + 2 * 2048);
    let observations = storage.lock().unwrap().observations;
    for _ in 0..8 {
        host.progress_resources().unwrap();
    }
    assert_eq!(storage.lock().unwrap().observations, observations);
    assert!(host.take_connection_response(1).is_none());

    receive(&mut host, first, 1, 2, 0, false);
    host.progress_resources().unwrap();
    assert!(storage.lock().unwrap().waker.is_none());
    assert_eq!(host.bulk_read_usage().backing_bytes, 1024 + 2048);
    receive(&mut host, peer, 2, 2, 0, false);
    host.progress_resources().unwrap();
    assert_eq!(host.bulk_read_usage().backing_bytes, 0);
}

#[test]
fn idle_reader_growth_reaches_severe_policy_and_revokes_without_a_client_read() {
    use ipp_core::services::io::{IoReadOptions, StreamIoReader};

    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let (reader, input) = StreamIoReader::new(IoReadOptions {
        max_bytes: None,
        recovery: false,
    });
    host.publish_connection_reader(1, Box::new(reader), None)
        .unwrap();
    host.set_bulk_read_policy(crate::services::bulk_read::BulkReadPolicy {
        severe_retained_bytes: std::num::NonZeroUsize::new(8192).unwrap(),
    });
    host.progress_resources().unwrap();
    let filling = input.reserve(16384).unwrap().unwrap();
    drop(filling);
    host.progress_resources().unwrap();
    assert_eq!(host.bulk_read_usage().backing_bytes, 16384 + 2048);
    host.maintain_connections(std::time::Duration::ZERO);
    let notice = host.take_connection_response(1).unwrap();
    assert_eq!(notice[28], 2);
    assert_eq!(host.bulk_read_usage().leases, 0);
    host.progress_resources().unwrap();
    assert_eq!(host.bulk_read_usage().backing_bytes, 0);
    assert!(!input.is_open());
}
