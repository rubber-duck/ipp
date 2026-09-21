//! Generic source routing and I/O without an asset object or World.
use ipp_core::services::data_source::*;
use std::{
    cell::RefCell,
    rc::Rc,
    task::{Context, Poll, Waker},
};

fn options(max_bytes: usize) -> DataReadOptions {
    DataReadOptions {
        max_bytes: Some(max_bytes),
        recovery: false,
    }
}

fn read(mut reader: Box<dyn DataReader>) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        let mut chunk = [0; 7];
        match reader.poll_read(&mut cx, &mut chunk) {
            Poll::Ready(Ok(0)) => return Ok(bytes),
            Poll::Ready(Ok(count)) => bytes.extend_from_slice(&chunk[..count]),
            Poll::Ready(Err(error)) => return Err(error),
            Poll::Pending => panic!("in-memory input should be available"),
        }
    }
}

struct RecordingSource(Rc<RefCell<Vec<String>>>);

impl DataSource for RecordingSource {
    fn list(&mut self, identifier: &str) -> Result<Vec<String>, String> {
        self.0.borrow_mut().push(identifier.into());
        Ok(vec![identifier.into()])
    }

    fn open_read(
        &mut self,
        identifier: &str,
        _: DataReadOptions,
    ) -> Result<Box<dyn DataReader>, String> {
        self.0.borrow_mut().push(identifier.into());
        Ok(Box::new(MemoryDataReader::new(identifier.as_bytes())))
    }
}

#[test]
fn disjoint_literal_prefixes_forward_identifiers_without_normalizing() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut data = DataSourceManagementService::new();
    data.register("opaque", RecordingSource(seen.clone()))
        .unwrap();
    for overlap in ["opaque", "opa", "opaque-child", ""] {
        assert!(data.register(overlap, MemoryDataSource::default()).is_err());
    }
    data.register("neighbor", MemoryDataSource::default())
        .unwrap();
    let identifier = "opaque//../a%2fb?x=1&x=2#literal";
    assert_eq!(
        read(data.open_read(identifier, options(128)).unwrap()).unwrap(),
        identifier.as_bytes()
    );
    assert_eq!(data.list(identifier).unwrap(), [identifier]);
    assert_eq!(*seen.borrow(), [identifier, identifier]);
    assert!(!data.can_write(identifier));
    assert!(data.open_write(identifier, 100).is_err());
}

#[test]
fn generic_memory_output_publishes_atomically_and_preserves_cancelled_destination() {
    let memory = MemoryDataSource::new(true);
    memory
        .insert("mem:world".into(), b"previous".to_vec())
        .unwrap();
    let mut data = DataSourceManagementService::new();
    data.register("mem:", memory).unwrap();
    assert!(data.can_write("mem:world"));
    let mut writer = data.open_write("mem:world", 10).unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    assert!(matches!(
        writer.poll_write(&mut cx, b"cancel"),
        Poll::Ready(Ok(6))
    ));
    writer.abort();
    assert_eq!(
        read(data.open_read("mem:world", options(10)).unwrap()).unwrap(),
        b"previous"
    );
    let mut job = DataWriteJob::new(
        b"replaced".to_vec(),
        data.open_write("mem:world", 10).unwrap(),
    );
    assert!(matches!(job.poll(&mut cx), Poll::Ready(Ok(()))));
    assert_eq!(
        read(data.open_read("mem:world", options(10)).unwrap()).unwrap(),
        b"replaced"
    );
    assert!(
        data.open_read(
            "mem:world",
            DataReadOptions {
                max_bytes: Some(10),
                recovery: true
            }
        )
        .is_err()
    );
    assert_eq!(data.list("mem:").unwrap(), ["mem:world"]);
    assert!(data.open_read("mem:world", options(2)).is_err());
}

#[test]
fn replacement_registration_cancels_old_readers_and_rejects_stale_completion() {
    let mut data = DataSourceManagementService::new();
    data.register_stream("https:").unwrap();
    let mut first = data
        .open_read("https://example.test/a?opaque=%2F", options(8))
        .unwrap();
    let request = data.take_requests().pop().unwrap();
    assert!(data.unregister("https:"));
    assert_eq!(data.take_cancellations(), [request.id]);
    assert!(matches!(
        first.poll_read(&mut Context::from_waker(Waker::noop()), &mut [0; 8]),
        Poll::Ready(Err(_))
    ));
    data.register_stream("https:").unwrap();
    let replacement = data
        .open_read("https://example.test/a?opaque=%2F", options(8))
        .unwrap();
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
    host.data_sources_mut()
        .register_stream("world-data:")
        .unwrap();
    let _reader = host
        .data_sources_mut()
        .open_read("world-data:saved", options(128))
        .unwrap();
    assert!(host.take_resource_requests().is_empty());
    let requests = host.data_sources().take_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].identifier, "world-data:saved");
}

#[cfg(feature = "zip-data-source")]
#[test]
fn zip_stored_and_deflated_entries_validate_and_obey_output_budgets() {
    let bytes = include_bytes!("fixtures/data-source.zip").to_vec();
    let mut invalid_offset = bytes.clone();
    let directory = invalid_offset
        .windows(4)
        .position(|value| value == b"PK\x01\x02")
        .unwrap();
    invalid_offset[directory + 42..directory + 46].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(ZipDataSource::new("bad:", invalid_offset).is_err());
    let zip = ZipDataSource::new("zip:", bytes.clone()).unwrap();
    let mut data = DataSourceManagementService::new();
    data.register("zip:", zip).unwrap();
    assert_eq!(
        data.list("zip:").unwrap(),
        ["zip:deflated.txt", "zip:stored.txt"]
    );
    for identifier in ["zip:deflated.txt", "zip:stored.txt"] {
        assert_eq!(
            read(data.open_read(identifier, options(128)).unwrap()).unwrap(),
            b"generic data source\n"
        );
        assert!(data.open_read(identifier, options(1)).is_err());
        assert!(!data.can_write(identifier));
    }
    let mut corrupt = bytes;
    // Corrupt the first stored entry; the valid directory still parses.
    let payload = corrupt
        .windows(20)
        .position(|value| value == b"generic data source\n")
        .unwrap();
    corrupt[payload] ^= 1;
    let mut source = ZipDataSource::new("bad:", corrupt).unwrap();
    assert!(source.open_read("bad:stored.txt", options(128)).is_err());
}
