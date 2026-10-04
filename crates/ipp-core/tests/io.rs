//! Generic source routing and I/O without an asset object or World.
use ipp_core::services::io::*;
use std::{
    cell::RefCell,
    future::Future,
    num::NonZeroUsize,
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

fn options(max_bytes: usize) -> IoReadOptions {
    IoReadOptions {
        max_bytes: Some(max_bytes),
        recovery: false,
    }
}

fn ready<F: Future>(future: F) -> F::Output {
    match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("in-memory operation should be immediately ready"),
    }
}

fn read(mut reader: Box<dyn IoReader>) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    loop {
        let window = ready(reader.read(NonZeroUsize::new(7).unwrap()))?;
        let count = window.bytes().len();
        let finished = window.is_final();
        bytes.extend_from_slice(window.bytes());
        window.consume(count)?;
        if finished {
            return Ok(bytes);
        }
    }
}

fn list(mut entries: Box<dyn IoListing>) -> Vec<String> {
    let mut values = Vec::new();
    while let Some(value) = ready(entries.next()).unwrap() {
        values.push(value);
    }
    values
}

struct RecordingSource(Rc<RefCell<Vec<String>>>);

impl IoSource for RecordingSource {
    fn list(&mut self, identifier: &str) -> IoListFuture {
        self.0.borrow_mut().push(identifier.into());
        Box::pin(std::future::ready(Ok(
            Box::new(MemoryIoListing::new(vec![identifier.into()])) as Box<dyn IoListing>,
        )))
    }

    fn open_read(&mut self, identifier: &str, _: IoReadOptions) -> IoOpenReadFuture {
        self.0.borrow_mut().push(identifier.into());
        Box::pin(std::future::ready(Ok(
            Box::new(BufferIoReader::new(identifier.as_bytes())) as Box<dyn IoReader>,
        )))
    }
}

#[test]
fn disjoint_literal_prefixes_forward_identifiers_without_normalizing() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut data = IoService::new();
    data.register("opaque", RecordingSource(seen.clone()))
        .unwrap();
    for overlap in ["opaque", "opa", "opaque-child", ""] {
        assert!(data.register(overlap, MemoryIoSource::default()).is_err());
    }
    data.register("neighbor", MemoryIoSource::default())
        .unwrap();
    let identifier = "opaque//../a%2fb?x=1&x=2#literal";
    assert_eq!(
        read(ready(data.open_read(identifier, options(128))).unwrap()).unwrap(),
        identifier.as_bytes()
    );
    assert_eq!(list(ready(data.list(identifier)).unwrap()), [identifier]);
    assert_eq!(*seen.borrow(), [identifier, identifier]);
    assert!(!data.can_write(identifier));
    assert!(ready(data.open_write(identifier, 100)).is_err());
}

#[test]
fn generic_memory_output_publishes_atomically_and_preserves_cancelled_destination() {
    let memory = MemoryIoSource::new(true);
    memory
        .insert("mem:world".into(), b"previous".to_vec())
        .unwrap();
    let mut data = IoService::new();
    data.register("mem:", memory).unwrap();
    assert!(data.can_write("mem:world"));
    let mut writer = ready(data.open_write("mem:world", 10)).unwrap();
    assert_eq!(ready(writer.write(b"cancel")).unwrap(), 6);
    writer.abort();
    assert_eq!(
        read(ready(data.open_read("mem:world", options(10))).unwrap()).unwrap(),
        b"previous"
    );
    let mut job = IoWriteJob::new(
        b"replaced".to_vec(),
        ready(data.open_write("mem:world", 10)).unwrap(),
    );
    assert!(matches!(
        job.poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(
        read(ready(data.open_read("mem:world", options(10))).unwrap()).unwrap(),
        b"replaced"
    );
    assert!(
        ready(data.open_read(
            "mem:world",
            IoReadOptions {
                max_bytes: Some(10),
                recovery: true
            }
        ))
        .is_err()
    );
    assert_eq!(list(ready(data.list("mem:")).unwrap()), ["mem:world"]);
    assert!(ready(data.open_read("mem:world", options(2))).is_err());
}

#[test]
fn replacement_registration_cancels_old_readers_and_rejects_stale_completion() {
    let mut data = IoService::new();
    data.register_stream("https:").unwrap();
    let mut first = ready(data.open_read("https://example.test/a?opaque=%2F", options(8))).unwrap();
    let request = data.take_requests().pop().unwrap();
    assert!(data.unregister("https:"));
    assert_eq!(data.take_cancellations(), [request.id]);
    assert!(ready(first.read(NonZeroUsize::new(8).unwrap())).is_err());
    data.register_stream("https:").unwrap();
    let replacement =
        ready(data.open_read("https://example.test/a?opaque=%2F", options(8))).unwrap();
    let next = data.take_requests().pop().unwrap();
    assert_ne!(next.id, request.id);
    assert!(data.input_chunk(request.id, b"stale").unwrap());
    data.input_end(request.id, Ok(()));
    data.input_chunk(next.id, b"fresh").unwrap();
    data.input_end(next.id, Ok(()));
    assert_eq!(read(replacement).unwrap(), b"fresh");
}

#[test]
fn asset_request_drain_preserves_independent_generic_reads() {
    let mut host = ipp_core::HostRuntime::new();
    host.io_mut().register_stream("world-data:").unwrap();
    let _reader = ready(host.io_mut().open_read("world-data:saved", options(128))).unwrap();
    assert!(host.take_resource_requests().is_empty());
    let requests = host.io().take_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].identifier, "world-data:saved");
}
