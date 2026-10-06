use super::*;
use ipp_core::services::asset_management::AssetProvider;
use ipp_core::services::io::MemoryIoSource;
use ipp_protocol::bulk_read::{self, BulkReadDescriptor};

struct Services;

impl HostServices for Services {
    const NAME: &'static str = "asset-export-test";

    fn initialize(
        _host: &mut ipp_core::HostRuntime,
        _schedulers: &crate::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String> {
        Ok(Self)
    }

    fn asset_gpu_formats(&self, kind: AssetTypeId) -> Vec<AssetExportFormat> {
        if kind == ipp_core::TEXTURE_TYPE {
            vec![AssetExportFormat::TextureV3]
        } else {
            vec![]
        }
    }

    fn asset_gpu_export(
        &mut self,
        _provider: &AssetProvider,
        _format: AssetExportFormat,
        observer: Rc<dyn AssetOutputObserver>,
    ) -> Result<AssetExportFuture, String> {
        Ok(Box::pin(async move {
            observer.check()?;
            Ok(texture())
        }))
    }

    fn service_resources(&mut self, _host: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }
}

fn open(host: &mut Host<Services>, connection: u64) {
    host.open_connection(connection).unwrap();
    host.receive_connection(connection, &ipp_protocol::contract::HELLO)
        .unwrap();
    host.take_connection_response(connection).unwrap();
}

fn enqueue(host: &mut Host<Services>, connection: u64, id: u64, operation: AssetExportRequest) {
    let bytes = host::encode_host_request(&HostRequest {
        connection,
        request_id: id,
        body: HostRequestBody::AssetExport(operation),
    })
    .unwrap();
    host.receive_connection(connection, &bytes).unwrap();
    assert!(host.process_host_requests().is_empty());
}

fn await_reply(host: &mut Host<Services>, connection: u64) -> HostResponseBody {
    for _ in 0..256 {
        host.progress_resources().unwrap();
        if let Some(reply) = host.take_connection_response(connection) {
            return host::decode_host_response(&reply, connection).unwrap().body;
        }
    }
    panic!("asset operation did not complete through service-only scheduling");
}

fn capability(body: HostResponseBody) -> AssetReadCapability {
    let HostResponseBody::AssetExport(AssetExportResponse::Capability {
        capability,
        ..
    }) = body
    else {
        panic!("expected asset authority: {body:?}");
    };
    capability
}

fn descriptor(body: HostResponseBody) -> BulkReadDescriptor {
    let HostResponseBody::AssetExport(AssetExportResponse::Read {
        read,
        ..
    }) = body
    else {
        panic!("expected asset read: {body:?}");
    };
    read
}

fn original(capability: AssetReadCapability) -> AssetExportRequest {
    AssetExportRequest::Read {
        capability,
        representation: AssetReadRepresentation::Original,
        format: None,
    }
}

fn texture() -> Vec<u8> {
    let mut bytes = b"IPPT\x03\0\0\0\x02\0\0\0\x02\0\0\0".to_vec();
    bytes.extend([
        7, 20, 51, 64, 127, 144, 173, 128, 191, 209, 223, 192, 241, 250, 255, 255,
    ]);
    bytes
}

fn source(uri: &str) -> AssetSource {
    AssetSource {
        kind: ipp_core::TEXTURE_TYPE,
        uri: uri.into(),
        variant: 3,
    }
}

fn read_payload(host: &mut Host<Services>, descriptor: BulkReadDescriptor) -> Vec<u8> {
    let mut request = bulk_read::REQUEST_MAGIC.to_vec();
    request.extend(descriptor.reference.connection.to_le_bytes());
    request.extend(1000u64.to_le_bytes());
    request.extend(descriptor.reference.read.to_le_bytes());
    request.push(0);
    request.extend(0u64.to_le_bytes());
    host.receive_connection(descriptor.reference.connection, &request)
        .unwrap();
    for _ in 0..256 {
        host.progress_resources().unwrap();
        if let Some(reply) = host.take_connection_response(descriptor.reference.connection) {
            assert_eq!(&reply[..4], bulk_read::RESPONSE_MAGIC);
            assert_eq!(reply[28], 0, "bulk error tag");
            assert_eq!(reply[37], 1, "small fixture must reach explicit EOF");
            let length = u32::from_le_bytes(reply[38..42].try_into().unwrap()) as usize;
            assert_eq!(reply.len(), 42 + length);
            return reply[42..].to_vec();
        }
    }
    panic!("original reader did not wake service-only Host");
}

#[test]
fn uri_and_foreign_capability_never_authorize_but_explicit_public_policy_does() {
    let mut host = Host::<Services>::new().unwrap();
    open(&mut host, 1);
    open(&mut host, 2);
    let memory = MemoryIoSource::new(false);
    let source = source("fixture:secret");
    memory.insert(source.uri.to_string(), texture()).unwrap();
    host.runtime.io_mut().register("fixture:", memory).unwrap();
    enqueue(&mut host, 2, 1, AssetExportRequest::Find(source.clone()));
    assert!(
        matches!(await_reply(&mut host, 2), HostResponseBody::Error(error) if error.contains("denied"))
    );
    let granted = host
        .grant_asset_source(
            1,
            source.clone(),
            AssetReadAccess {
                original: true,
                ..Default::default()
            },
        )
        .unwrap();
    enqueue(&mut host, 2, 2, original(granted));
    assert!(
        matches!(await_reply(&mut host, 2), HostResponseBody::Error(error) if error.contains("denied"))
    );
    let policy = host
        .expose_asset_source(
            source.clone(),
            AssetReadAccess {
                original: true,
                ..Default::default()
            },
        )
        .unwrap();
    enqueue(&mut host, 2, 3, AssetExportRequest::Find(source));
    let public = capability(await_reply(&mut host, 2));
    assert_ne!(public, granted);
    enqueue(&mut host, 2, 4, original(public));
    let read = descriptor(await_reply(&mut host, 2));
    assert_eq!(read_payload(&mut host, read), texture());
    assert!(host.revoke_public_asset_source(policy));
    enqueue(&mut host, 2, 5, original(public));
    assert!(matches!(
        await_reply(&mut host, 2),
        HostResponseBody::Error(_)
    ));
    enqueue(&mut host, 1, 6, original(granted));
    let independent = descriptor(await_reply(&mut host, 1));
    assert_eq!(read_payload(&mut host, independent), texture());
}

#[test]
fn admitted_original_read_survives_producer_release_but_unused_grant_does_not_pin_source() {
    let mut host = Host::<Services>::new().unwrap();
    open(&mut host, 1);
    let world = host.open_session(9, &[]).unwrap();
    let source = source("client://9/texture#immutable");
    host.runtime
        .asset_resources_mut()
        .register_client_source(world, source.clone(), texture())
        .unwrap();
    let grant = host
        .grant_asset_source(
            1,
            source.clone(),
            AssetReadAccess {
                original: true,
                ..Default::default()
            },
        )
        .unwrap();
    enqueue(&mut host, 1, 1, original(grant));
    assert!(
        host.take_connection_response(1).is_none(),
        "no reply before scheduled open completion"
    );
    assert_eq!(
        host.connections.states[&1].reply_entries(),
        1,
        "pending operation retains correlated delivery credit"
    );
    host.runtime
        .asset_resources_mut()
        .release_client_source(world, &source);
    host.runtime.flush_resource_lifecycle();
    assert!(host.runtime.asset_resources().find(&source).is_none());
    let read = descriptor(await_reply(&mut host, 1));
    assert_eq!(read_payload(&mut host, read), texture());
    enqueue(&mut host, 1, 2, original(grant));
    assert!(
        matches!(await_reply(&mut host, 1), HostResponseBody::Error(error) if error.contains("unavailable"))
    );
}

#[test]
fn registration_replacement_and_grant_revocation_fence_accepted_async_opens() {
    for revoke_grant in [true, false] {
        let mut host = Host::<Services>::new().unwrap();
        open(&mut host, 1);
        let memory = MemoryIoSource::new(false);
        let source = source("fixture:replacement");
        memory.insert(source.uri.to_string(), texture()).unwrap();
        host.runtime.io_mut().register("fixture:", memory).unwrap();
        let grant = host
            .grant_asset_source(
                1,
                source.clone(),
                AssetReadAccess {
                    original: true,
                    ..Default::default()
                },
            )
            .unwrap();
        enqueue(&mut host, 1, 1, original(grant));
        if revoke_grant {
            assert!(host.revoke_asset_grant(grant));
        } else {
            assert!(host.runtime.io_mut().unregister("fixture:"));
            let replacement = MemoryIoSource::new(false);
            replacement
                .insert(source.uri.to_string(), vec![99; 32])
                .unwrap();
            host.runtime
                .io_mut()
                .register("fixture:", replacement)
                .unwrap();
        }
        assert!(matches!(
            await_reply(&mut host, 1),
            HostResponseBody::Error(_)
        ));
        assert_eq!(host.bulk_read_usage().leases, 0);
        assert!(host.connections.exports.pending.is_empty());
    }
}

#[test]
fn published_cpu_export_detaches_from_asset_source_world_and_grant_teardown() {
    let mut host = Host::<Services>::new().unwrap();
    open(&mut host, 1);
    let world = host.open_session(9, &[]).unwrap();
    let source = source("client://9/texture#typed");
    host.runtime
        .asset_resources_mut()
        .register_client_source(world, source.clone(), texture())
        .unwrap();
    for _ in 0..8 {
        host.progress_resources().unwrap();
    }
    let access = host.own_asset_access(source.kind);
    let grant = host.grant_asset_source(1, source.clone(), access).unwrap();
    enqueue(
        &mut host,
        1,
        1,
        AssetExportRequest::Read {
            capability: grant,
            representation: AssetReadRepresentation::Cpu,
            format: Some(AssetExportFormat::TextureV3),
        },
    );
    assert_eq!(host.bulk_read_usage().leases, 0);
    let read = descriptor(await_reply(&mut host, 1));
    assert_eq!(read.length, Some(texture().len() as u64));
    assert!(host.revoke_asset_grant(grant));
    host.runtime
        .asset_resources_mut()
        .release_client_source(world, &source);
    host.close_session(9);
    host.runtime.flush_resource_lifecycle();
    assert_eq!(read_payload(&mut host, read), texture());
}

#[test]
fn completed_private_gpu_output_fails_if_graphics_disappear_before_publication() {
    let mut host = Host::<Services>::new().unwrap();
    open(&mut host, 1);
    let world = host.open_session(9, &[]).unwrap();
    let source = source("client://9/texture#completion-fence");
    host.runtime
        .asset_resources_mut()
        .register_client_source(world, source.clone(), texture())
        .unwrap();
    for _ in 0..8 {
        host.progress_resources().unwrap();
    }
    let key = host.runtime.asset_resources().find(&source).unwrap();
    let cpu = host
        .runtime
        .asset_resources()
        .get(key)
        .unwrap()
        .cpu_export_snapshot()
        .unwrap();
    let grant = host
        .grant_asset_source(1, source.clone(), host.own_asset_access(source.kind))
        .unwrap();
    enqueue(
        &mut host,
        1,
        1,
        AssetExportRequest::Read {
            capability: grant,
            representation: AssetReadRepresentation::Gpu,
            format: Some(AssetExportFormat::TextureV3),
        },
    );
    host.scheduler.poll_ready();
    assert_eq!(host.connections.exports.completed.borrow().len(), 1);
    assert_eq!(host.bulk_read_usage().leases, 0);
    // Matches native preparation failure: invalidate GPU authority, flush the
    // ordinary barrier, then let the Host admit queued private completions.
    host.runtime.asset_resources_mut().invalidate_graphics(key);
    host.runtime.flush_resource_lifecycle();
    assert!(!cpu.available.is_cancelled());
    host.progress_asset_exports();
    let bytes = host.take_connection_response(1).unwrap();
    assert!(
        matches!(host::decode_host_response(&bytes,1).unwrap().body,HostResponseBody::Error(error) if error.contains("working representation"))
    );
    assert_eq!(host.bulk_read_usage().leases, 0);
    enqueue(
        &mut host,
        1,
        2,
        AssetExportRequest::Read {
            capability: grant,
            representation: AssetReadRepresentation::Cpu,
            format: Some(AssetExportFormat::TextureV3),
        },
    );
    let read = descriptor(await_reply(&mut host, 1));
    assert_eq!(read_payload(&mut host, read), texture());
}

#[test]
fn completed_private_output_fails_on_unload_or_pressure_before_publication() {
    for pressure in [false, true] {
        let mut host = Host::<Services>::new().unwrap();
        open(&mut host, 1);
        let world = host.open_session(9, &[]).unwrap();
        let source = source("client://9/texture#late-revocation");
        host.runtime
            .asset_resources_mut()
            .register_client_source(world, source.clone(), texture())
            .unwrap();
        for _ in 0..8 {
            host.progress_resources().unwrap();
        }
        let grant = host
            .grant_asset_source(1, source.clone(), host.own_asset_access(source.kind))
            .unwrap();
        enqueue(
            &mut host,
            1,
            1,
            AssetExportRequest::Read {
                capability: grant,
                representation: AssetReadRepresentation::Cpu,
                format: Some(AssetExportFormat::TextureV3),
            },
        );
        host.scheduler.poll_ready();
        assert_eq!(host.connections.exports.completed.borrow().len(), 1);
        if pressure {
            host.signal_severe_memory_pressure();
        } else {
            let key = host.runtime.asset_resources().find(&source).unwrap();
            host.runtime.asset_resources_mut().unload(key);
            host.runtime.flush_resource_lifecycle();
        }
        host.progress_asset_exports();
        let bytes = host.take_connection_response(1).unwrap();
        assert!(matches!(
            host::decode_host_response(&bytes, 1).unwrap().body,
            HostResponseBody::Error(_)
        ));
        assert_eq!(host.bulk_read_usage().leases, 0);
    }
}

struct GatedExportSource {
    memory: MemoryIoSource,
    released: Arc<std::sync::atomic::AtomicBool>,
    waiter: Arc<std::sync::Mutex<Option<std::task::Waker>>>,
}

impl ipp_core::services::io::IoSource for GatedExportSource {
    fn list(&mut self, identifier: &str) -> ipp_core::services::io::IoListFuture {
        self.memory.list(identifier)
    }

    fn open_read(
        &mut self,
        identifier: &str,
        options: ipp_core::services::io::IoReadOptions,
    ) -> ipp_core::services::io::IoOpenReadFuture {
        let open = self.memory.open_read(identifier, options);
        let released = self.released.clone();
        let waiter = self.waiter.clone();
        Box::pin(async move {
            poll_fn(|cx| {
                *waiter.lock().unwrap() = Some(cx.waker().clone());
                if released.load(std::sync::atomic::Ordering::Acquire) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
            open.await
        })
    }
}

#[test]
fn pending_export_rejects_duplicate_host_ids_before_and_after_private_completion() {
    use std::sync::atomic::{AtomicBool, Ordering};

    for completion_queued in [false, true] {
        let mut host = Host::<Services>::new().unwrap();
        open(&mut host, 1);
        let memory = MemoryIoSource::new(false);
        let source = source("gated-export:original");
        memory.insert(source.uri.to_string(), texture()).unwrap();
        let released = Arc::new(AtomicBool::new(false));
        let waiter = Arc::new(std::sync::Mutex::new(None));
        host.runtime
            .io_mut()
            .register(
                "gated-export:",
                GatedExportSource {
                    memory,
                    released: released.clone(),
                    waiter: waiter.clone(),
                },
            )
            .unwrap();
        let grant = host
            .grant_asset_source(
                1,
                source,
                AssetReadAccess {
                    original: true,
                    ..Default::default()
                },
            )
            .unwrap();
        enqueue(&mut host, 1, 73, original(grant));
        host.scheduler.poll_ready();
        assert!(waiter.lock().unwrap().is_some());
        assert_eq!(host.connections.states[&1].reply_entries(), 1);
        assert!(host.connections.exports.completed.borrow().is_empty());

        if completion_queued {
            released.store(true, Ordering::Release);
            waiter.lock().unwrap().take().unwrap().wake();
            host.scheduler.poll_ready();
            assert_eq!(host.connections.exports.completed.borrow().len(), 1);
        }
        for body in [
            HostRequestBody::ListWorlds {
                after: 0,
            },
            HostRequestBody::AssetExport(original(grant)),
        ] {
            let bytes = host::encode_host_request(&HostRequest {
                connection: 1,
                request_id: 73,
                body,
            })
            .unwrap();
            assert_eq!(
                host.receive_connection(1, &bytes).unwrap_err(),
                "duplicate outstanding Host request"
            );
            assert_eq!(host.connections.exports.pending.len(), 1);
            assert_eq!(host.connections.states[&1].reply_entries(), 1);
            assert!(
                !host.connections.exports.pending[&(1, 73)]
                    .grant
                    .cancelled
                    .is_cancelled()
            );
        }
        released.store(true, Ordering::Release);
        if let Some(waker) = waiter.lock().unwrap().take() {
            waker.wake();
        }
        let read = descriptor(await_reply(&mut host, 1));
        assert!(host.connections.exports.pending.is_empty());
        assert!(
            host.take_connection_response(1).is_none(),
            "exactly one correlated export reply"
        );
        assert_eq!(read_payload(&mut host, read), texture());
    }
}
