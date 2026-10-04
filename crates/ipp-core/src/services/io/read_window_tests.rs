use super::super::*;
use std::{
    future::Future,
    num::NonZeroUsize,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};

fn minimum(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).unwrap()
}

fn ready<F: Future + Unpin>(mut future: F) -> F::Output {
    match Pin::new(&mut future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("expected immediate readiness"),
    }
}

fn options() -> IoReadOptions {
    IoReadOptions {
        max_bytes: None,
        recovery: false,
    }
}

#[test]
fn buffer_lends_original_backing_with_independent_cursors_and_exact_consumption() {
    let bytes = Arc::new(vec![1, 2, 3, 4]);
    let pointer = bytes.as_ptr();
    let mut first = BufferIoReader::from_backing(bytes.clone(), 0..4).unwrap();
    let mut second = BufferIoReader::from_backing(bytes, 0..4).unwrap();
    let reader: &mut dyn IoReader = &mut first;
    let window = ready(reader.read(minimum(9))).unwrap();
    assert_eq!(window.bytes().as_ptr(), pointer);
    assert!(window.is_final());
    drop(window);
    assert!(ready(reader.read(minimum(1))).unwrap().consume(5).is_err());
    assert_eq!(
        ready(reader.read(minimum(1))).unwrap().bytes(),
        &[1, 2, 3, 4]
    );
    ready(reader.read(minimum(1))).unwrap().consume(3).unwrap();
    assert_eq!(ready(reader.read(minimum(2))).unwrap().bytes(), &[4]);
    assert_eq!(
        ready(second.read(minimum(1))).unwrap().bytes(),
        &[1, 2, 3, 4]
    );
    ready(reader.read(minimum(1))).unwrap().consume(1).unwrap();
    let eof = ready(reader.read(minimum(1))).unwrap();
    assert!(eof.bytes().is_empty());
    assert!(eof.is_final());
}

#[test]
fn native_readers_futures_and_windows_are_transferable() {
    fn send(_: impl Send) {}
    let mut reader: Box<dyn IoReader> = Box::new(BufferIoReader::new(vec![1]));
    send(reader.read(minimum(1)));
    send(ready(reader.read(minimum(1))).unwrap());
    send(reader);
}

#[test]
fn lookahead_larger_than_transport_capacity_does_not_deadlock_and_cancel_does_not_consume() {
    let (mut reader, input) = StreamIoReader::new(options());
    let mut cx = Context::from_waker(Waker::noop());
    let large = STREAM_CAPACITY + 17;
    {
        let mut pending = reader.read(minimum(large));
        assert!(Pin::new(&mut pending).poll(&mut cx).is_pending());
        assert!(input.push(&vec![3; STREAM_CAPACITY]).unwrap());
        assert!(Pin::new(&mut pending).poll(&mut cx).is_pending());
    }
    assert!(input.push(&[7; 17]).unwrap());
    let window = ready(reader.read(minimum(large))).unwrap();
    assert_eq!(window.bytes().len(), large);
    assert_eq!(&window.bytes()[STREAM_CAPACITY..], &[7; 17]);
    assert!(!window.is_final());
    drop(window);
    let window = ready(reader.read(minimum(large))).unwrap();
    window.consume(STREAM_CAPACITY).unwrap();
    input.finish(Ok(()));
    let window = ready(reader.read(minimum(large))).unwrap();
    assert_eq!(window.bytes(), &[7; 17]);
    assert!(window.is_final());
}

#[test]
fn stream_fill_lease_lends_same_storage_and_uncommitted_bytes_stay_private() {
    let (mut reader, input) = StreamIoReader::new(options());
    let mut reservation = input.reserve(4).unwrap().unwrap();
    let pointer = reservation.as_mut_ptr();
    reservation.bytes_mut().copy_from_slice(&[1, 2, 3, 4]);
    assert!(reader.read(minimum(1)).poll_unpin_for_test().is_pending());
    reservation.commit(4).unwrap();
    let window = ready(reader.read(minimum(4))).unwrap();
    assert_eq!(window.bytes().as_ptr(), pointer);
    assert!(input.reserve(1).unwrap().is_none());
    window.consume(2).unwrap();
    let mut abandoned = input.reserve(2).unwrap().unwrap();
    abandoned.bytes_mut().copy_from_slice(&[8, 9]);
    drop(abandoned);
    input.finish(Ok(()));
    let window = ready(reader.read(minimum(4))).unwrap();
    assert_eq!(window.bytes(), &[3, 4]);
    assert!(window.is_final());
}

trait PollForTest: Future + Unpin {
    fn poll_unpin_for_test(&mut self) -> Poll<Self::Output> {
        Pin::new(self).poll(&mut Context::from_waker(Waker::noop()))
    }
}

impl<T: Future + Unpin> PollForTest for T {}

#[test]
fn revocation_preserves_an_active_window_and_cancels_new_reads() {
    let source = MemoryIoSource::default();
    source.insert("mem:old".into(), vec![1, 2, 3]).unwrap();
    let mut service = IoService::new();
    service.register("mem:", source).unwrap();
    let mut reader = ready(service.open_read("mem:old", options())).unwrap();
    let window = ready(reader.read(minimum(1))).unwrap();
    assert!(service.unregister("mem:"));
    assert_eq!(window.bytes(), &[1, 2, 3]);
    window.consume(1).unwrap();
    assert!(ready(reader.read(minimum(1))).is_err());
}

struct GatedSource(Arc<AtomicBool>);

impl IoSource for GatedSource {
    fn list(&mut self, _: &str) -> IoListFuture {
        Box::pin(std::future::ready(Err("unsupported".into())))
    }

    fn open_read(&mut self, _: &str, _: IoReadOptions) -> IoOpenReadFuture {
        let gate = self.0.clone();
        Box::pin(std::future::poll_fn(move |_| {
            if gate.load(Ordering::Acquire) {
                Poll::Ready(Ok(
                    Box::new(BufferIoReader::new(vec![1])) as Box<dyn IoReader>
                ))
            } else {
                Poll::Pending
            }
        }))
    }
}

#[test]
fn open_captures_registration_at_call_and_replacement_fences_pending_completion() {
    let gate = Arc::new(AtomicBool::new(false));
    let mut service = IoService::new();
    service
        .register("gated:", GatedSource(gate.clone()))
        .unwrap();
    let original = service.registration_id("gated:a");
    let mut open = service.open_read("gated:a", options());
    assert!(
        open.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    service.unregister("gated:");
    service
        .register("gated:", GatedSource(gate.clone()))
        .unwrap();
    assert_ne!(original, service.registration_id("gated:a"));
    gate.store(true, Ordering::Release);
    assert!(ready(open).is_err());
    assert!(ready(service.open_read("gated:a", options())).is_ok());
}

#[test]
fn revoked_stream_fill_lease_stays_valid_but_cannot_publish_to_replacement() {
    let mut service = IoService::new();
    service.register_stream("stream:").unwrap();
    let mut reader = ready(service.open_read("stream:a", options())).unwrap();
    let request = service.take_requests().pop().unwrap();
    let mut reservation = service.reserve_input(request.id, 3).unwrap().unwrap();
    service.unregister("stream:");
    reservation.bytes_mut().copy_from_slice(&[4, 5, 6]);
    reservation.commit(3).unwrap();
    assert!(ready(reader.read(minimum(1))).is_err());
    assert_eq!(service.take_cancellations(), [request.id]);
}

#[test]
fn stream_capacity_matches_lookahead_and_prefetch_and_reuses_allocation() {
    let (mut reader, input) = StreamIoReader::new(options());
    let lookahead = STREAM_CAPACITY;
    assert!(
        reader
            .read(minimum(lookahead))
            .poll_unpin_for_test()
            .is_pending()
    );
    assert_eq!(
        reader.retained_storage().unwrap().bytes,
        2 * STREAM_CAPACITY
    );
    let mut pointer = None;
    for _ in 0..8 {
        for chunk in 0..2 {
            let mut reservation = input.reserve(STREAM_CAPACITY).unwrap().unwrap();
            if chunk == 0 {
                if let Some(pointer) = pointer {
                    assert_eq!(reservation.as_mut_ptr(), pointer);
                } else {
                    pointer = Some(reservation.as_mut_ptr());
                }
            }
            reservation.bytes_mut().fill(37);
            reservation.commit(STREAM_CAPACITY).unwrap();
        }
        let window = ready(reader.read(minimum(lookahead))).unwrap();
        assert_eq!(window.bytes().as_ptr(), pointer.unwrap());
        window.consume(2 * STREAM_CAPACITY).unwrap();
        assert_eq!(
            reader.retained_storage().unwrap().bytes,
            2 * STREAM_CAPACITY
        );
    }
}

#[test]
fn granted_open_tracks_exact_registration_and_preserves_active_windows() {
    let source = MemoryIoSource::default();
    source.insert("mem:a".into(), vec![1, 2, 3]).unwrap();
    let mut service = IoService::new();
    service.register("mem:", source).unwrap();
    let id = service.registration_id("mem:a").unwrap();
    let availability = service.registration_cancellation("mem:a", id).unwrap();
    let grant = IoCancellation::default();
    let mut reader =
        ready(service.open_read_registered("mem:a", id, options(), grant.clone())).unwrap();
    let window = ready(reader.read(minimum(1))).unwrap();
    grant.cancel();
    assert_eq!(window.bytes(), &[1, 2, 3]);
    drop(window);
    assert!(ready(reader.read(minimum(1))).is_err());
    service.unregister("mem:");
    assert!(availability.is_cancelled());
    service.register("mem:", MemoryIoSource::default()).unwrap();
    assert!(service.registration_cancellation("mem:a", id).is_none());
    assert!(
        ready(service.open_read_registered("mem:a", id, options(), IoCancellation::default()))
            .is_err()
    );
}

#[test]
fn cancelled_waiter_drop_releases_task_waker_before_namespace_revocation() {
    struct WakeProbe(AtomicBool);
    impl std::task::Wake for WakeProbe {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::Release);
        }
    }
    let task = Arc::new(WakeProbe(AtomicBool::new(false)));
    let waker = Waker::from(task.clone());
    let cancellation = IoCancellation::default();
    for _ in 0..100 {
        let mut wait = cancellation.cancelled();
        assert!(
            Pin::new(&mut wait)
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        assert_eq!(Arc::strong_count(&task), 3);
        drop(wait);
        assert_eq!(Arc::strong_count(&task), 2);
    }
    cancellation.cancel();
    assert!(!task.0.load(Ordering::Acquire));
}

#[test]
fn memory_open_captures_backing_at_operation_begin_before_producer_release() {
    let source = MemoryIoSource::default();
    source.insert("mem:original".into(), vec![1, 2, 3]).unwrap();
    let mut service = IoService::new();
    service.register("mem:", source.clone()).unwrap();
    let registration = service.registration_id("mem:original").unwrap();
    let operation = service.open_read_registered(
        "mem:original",
        registration,
        options(),
        IoCancellation::default(),
    );
    source.remove("mem:original");
    let mut reader = ready(operation).unwrap();
    let window = ready(reader.read(minimum(1))).unwrap();
    assert_eq!(window.bytes(), &[1, 2, 3]);
    assert!(
        ready(service.open_read_registered(
            "mem:original",
            registration,
            options(),
            IoCancellation::default()
        ))
        .is_err()
    );
}

#[test]
fn detached_fill_lease_keeps_allocation_accounted_until_worker_release() {
    let (reader, input) = StreamIoReader::new(options());
    let lease = input.reserve(16).unwrap().unwrap();
    let retained = input.buffered_bytes();
    assert!(retained >= 16);
    drop(reader);
    assert!(!input.is_open());
    assert_eq!(input.buffered_bytes(), retained);
    drop(lease);
    assert_eq!(input.buffered_bytes(), 0);
}

#[test]
fn fully_consumed_request_is_not_cancelled_after_reader_backing_is_released() {
    let mut service = IoService::new();
    service.register_stream("complete:").unwrap();
    let mut reader = ready(service.open_read("complete:item", options())).unwrap();
    let request = service.take_requests().pop().unwrap();
    assert!(service.input_chunk(request.id, &[1, 2, 3]).unwrap());
    service.input_end(request.id, Ok(()));
    let window = ready(reader.read(minimum(1))).unwrap();
    assert!(window.is_final());
    window.consume(3).unwrap();
    drop(reader);
    assert_eq!(service.input_bytes(), 0);
    assert!(service.take_cancellations().is_empty());
}

struct StorageWake(std::sync::atomic::AtomicUsize);

impl std::task::Wake for StorageWake {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
}

#[test]
fn idle_storage_observer_wakes_independently_and_detaches_from_surviving_fill_lease() {
    let (mut reader, input) = StreamIoReader::new(options());
    let observer = Arc::new(StorageWake(std::sync::atomic::AtomicUsize::new(0)));
    let waker = Waker::from(observer.clone());
    reader.register_storage_waker(&waker);
    assert_eq!(reader.retained_storage().unwrap().bytes, 0);
    let lease = input.reserve(16).unwrap().unwrap();
    assert_eq!(observer.0.load(Ordering::Acquire), 1);
    assert!(reader.retained_storage().unwrap().bytes >= 16);
    assert_eq!(Arc::strong_count(&observer), 3);
    drop(reader);
    assert_eq!(Arc::strong_count(&observer), 2);
    assert!(input.buffered_bytes() >= 16);
    drop(lease);
    assert_eq!(input.buffered_bytes(), 0);
}

#[test]
fn registered_and_granted_readers_forward_storage_wakes_without_stealing_read_readiness() {
    let mut service = IoService::new();
    service.register_stream("observe:").unwrap();
    let registration = service.registration_id("observe:item").unwrap();
    let mut reader = ready(service.open_read_registered(
        "observe:item",
        registration,
        options(),
        IoCancellation::default(),
    ))
    .unwrap();
    let request = service.take_requests().pop().unwrap();
    let observer = Arc::new(StorageWake(std::sync::atomic::AtomicUsize::new(0)));
    let decoder = Arc::new(StorageWake(std::sync::atomic::AtomicUsize::new(0)));
    reader.register_storage_waker(&Waker::from(observer.clone()));
    let decoder_waker = Waker::from(decoder.clone());
    let mut cx = Context::from_waker(&decoder_waker);
    assert!(
        reader
            .poll_ready(&mut cx, minimum(STREAM_CAPACITY))
            .is_pending()
    );
    assert_eq!(observer.0.load(Ordering::Acquire), 1);
    assert_eq!(decoder.0.load(Ordering::Acquire), 0);
    assert!(
        service
            .input_chunk(request.id, &vec![1; STREAM_CAPACITY])
            .unwrap()
    );
    assert_eq!(decoder.0.load(Ordering::Acquire), 1);
    assert_eq!(observer.0.load(Ordering::Acquire), 1);
    drop(reader);
    assert_eq!(Arc::strong_count(&observer), 1);
}
