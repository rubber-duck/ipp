use super::worlds::read_manifest;
use super::*;
use crate::codec::Reader;
use ipp_core::{WorldCapacityHints, WorldId, WorldMetadata, WorldPersistentId};

fn descriptor() -> WorldDescriptor {
    WorldDescriptor {
        id: WorldId(9),
        metadata: WorldMetadata {
            symbolic_id: "workshop".into(),
            persistent_id: WorldPersistentId((123u128 << 64) | 45),
        },
        capacity_hints: WorldCapacityHints {
            entities: 4096,
            ..Default::default()
        },
    }
}

#[test]
fn world_hello_carries_selected_manifest_but_connection_hello_does_not() {
    let mut runtime = ipp_core::HostRuntime::new();
    let world = runtime.create_world(Default::default(), &[]).unwrap();
    let selected = runtime.world_manifest(world).unwrap();
    let connection = crate::contract::accept_hello(&crate::contract::HELLO, 7).unwrap();
    assert_eq!(connection.len(), 24);
    let reply = accept_world_hello(&crate::contract::HELLO, 7, selected).unwrap();
    assert_eq!(reply[..24], connection);
    let mut reader = Reader {
        bytes: &reply,
        at: 24,
    };
    assert_eq!(
        read_manifest(&mut reader).unwrap(),
        WorldManifest::from_core(selected)
    );
    assert_eq!(reader.at, reply.len());
}

#[test]
fn lifecycle_envelopes_preserve_host_identity_world_metadata_and_sparse_hints() {
    let world = descriptor();
    let reference = WorldReference {
        id: world.id.0,
        incarnation: 17,
    };
    let output = OutputReference {
        world: reference,
        target: crate::references::OutputTarget::Camera {
            entity: 3,
            incarnation: 19,
        },
    };
    let requests = [
        HostRequestBody::ListWorlds {
            after: 8,
        },
        HostRequestBody::CreateWorld {
            options: WorldCreateOptions {
                symbolic_id: world.metadata.symbolic_id.clone(),
                capacity_hints: world.capacity_hints.clone(),
                selected_systems: Some(vec!["ipp.animation".into()]),
                canvas: Some(ipp_core::CanvasState {
                    extent: [640.0, 360.0],
                    units_per_metre: 400.0,
                }),
            },
            temporary: true,
        },
        HostRequestBody::ResolveWorld(WorldSelector::SymbolicId("workshop".into())),
        HostRequestBody::OpenWorld(reference),
        HostRequestBody::BindOutput {
            world: reference,
            entity: 3,
            kind: output.kind(),
        },
        HostRequestBody::ResolveOutput(output),
        HostRequestBody::SetRootOutput {
            output,
            viewport: ipp_core::WorldViewport {
                width: 640,
                height: 480,
                device_pixel_ratio: 2.0,
            },
        },
        HostRequestBody::ClearRootOutput(crate::host::presentation::RootBinding {
            output,
            viewport: ipp_core::WorldViewport {
                width: 640,
                height: 480,
                device_pixel_ratio: 2.0,
            },
            generation: crate::host::presentation::PresentationIdentity {
                host: 12,
                serial: 19,
            },
        }),
        HostRequestBody::RenameWorld {
            world: WorldSelector::Id(world.id),
            symbolic_id: "renamed".into(),
        },
        HostRequestBody::DestroyWorld(reference),
        HostRequestBody::DetachWorld {
            session: 11,
        },
        HostRequestBody::SetCapacityHints {
            session: 11,
            hints: ipp_core::WorldCapacityHintsPatch {
                entities: Some(0),
                systems: BTreeMap::new(),
            },
        },
    ];
    for body in requests {
        let request = HostRequest {
            connection: 5,
            request_id: 12,
            body,
        };
        let bytes = encode_host_request(&request).unwrap();
        assert_eq!(decode_host_request(&bytes, 5).unwrap(), request);
        assert!(decode_host_request(&bytes, 6).is_err());
        for length in 0..bytes.len() {
            assert!(decode_host_request(&bytes[..length], 5).is_err());
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert!(decode_host_request(&trailing, 5).is_err());
    }
    for body in [
        HostResponseBody::Worlds {
            worlds: vec![world.clone()],
            next: 9,
        },
        HostResponseBody::Attached {
            reference,
            world: world.clone(),
            session: 1 << 63 | 1,
            manifest: WorldManifest {
                systems: vec!["ipp.animation".into()],
                components: vec![1],
                operations: vec![0, 2],
            },
        },
        HostResponseBody::Created {
            world: world.clone(),
            reference,
        },
        HostResponseBody::WorldReference(reference),
        HostResponseBody::OutputReference(output),
        HostResponseBody::World(world),
        HostResponseBody::Complete,
        HostResponseBody::Error("Name already exists".into()),
        HostResponseBody::Detached {
            session: 1 << 63 | 1,
            reason: "World destroyed".into(),
        },
    ] {
        let response = HostResponse {
            connection: 5,
            request_id: 12,
            body,
        };
        let bytes = encode_host_response(&response).unwrap();
        assert_eq!(decode_host_response(&bytes, 5).unwrap(), response);
        assert!(decode_host_response(&bytes, 6).is_err());
        for length in 0..bytes.len() {
            assert!(decode_host_response(&bytes[..length], 5).is_err());
        }
    }
}

#[test]
fn persistence_envelopes_preserve_owned_chunks_and_load_overrides() {
    for body in [
        HostRequestBody::SaveWorld {
            session: 11,
        },
        HostRequestBody::BeginWorldLoad {
            bytes: 65539,
        },
        HostRequestBody::WriteWorldLoad {
            job: 2,
            offset: 65536,
            bytes: vec![1, 2, 3],
        },
        HostRequestBody::FinishWorldLoad {
            job: 2,
            symbolic_id: Some("copy".into()),
            capacity_hints: ipp_core::WorldCapacityHintsPatch {
                entities: Some(1234),
                ..Default::default()
            },
        },
        HostRequestBody::InspectWorldLoad {
            job: 2,
            offset: 8,
        },
        HostRequestBody::SetWorldLoadNames {
            job: 2,
            names: BTreeMap::from([(WorldGraphNodeId(4), "child-copy".into())]),
        },
        HostRequestBody::ReadWorldLoadBindings {
            job: 2,
            offset: 1024,
        },
        HostRequestBody::AcknowledgeWorldLoad {
            job: 2,
        },
        HostRequestBody::CancelWorldTransfer {
            job: 2,
        },
    ] {
        let request = HostRequest {
            connection: 5,
            request_id: 12,
            body,
        };
        assert_eq!(
            decode_host_request(&encode_host_request(&request).unwrap(), 5).unwrap(),
            request
        );
    }
    for body in [
        HostResponseBody::Transfer {
            job: 2,
        },
        HostResponseBody::Read {
            reference: crate::bulk_read::BulkReadReference {
                connection: 5,
                read: 2,
            },
            length: Some(65539),
        },
    ] {
        let response = HostResponse {
            connection: 5,
            request_id: 12,
            body,
        };
        assert_eq!(
            decode_host_response(&encode_host_response(&response).unwrap(), 5).unwrap(),
            response
        );
    }
}

#[test]
fn graph_reply_pages_preserve_typed_metadata_and_complete_bindings() {
    let reference = WorldReference {
        id: 5,
        incarnation: 7,
    };
    for body in [
        HostResponseBody::WorldGraphLoaded {
            job: 3,
            root: reference,
            total: 1025,
        },
        HostResponseBody::WorldGraphPage {
            job: 3,
            root: WorldGraphNodeId(9),
            total: 20,
            offset: 8,
            nodes: (0..MAX_GRAPH_METADATA_PAGE)
                .map(|index| WorldGraphNodeDescriptor {
                    id: WorldGraphNodeId(index as u32 + 8),
                    metadata: WorldMetadata {
                        symbolic_id: format!("node-{index}"),
                        persistent_id: WorldPersistentId(u128::MAX - index as u128),
                    },
                })
                .collect(),
        },
        HostResponseBody::WorldGraphBindings {
            job: 3,
            offset: 0,
            bindings: (0..MAX_GRAPH_BINDING_PAGE)
                .map(|index| {
                    (
                        WorldGraphNodeId(index as u32),
                        WorldReference {
                            id: index as u64 + 1,
                            incarnation: index as u64 + 2,
                        },
                    )
                })
                .collect(),
        },
    ] {
        let response = HostResponse {
            connection: 2,
            request_id: 8,
            body,
        };
        let bytes = encode_host_response(&response).unwrap();
        assert_eq!(decode_host_response(&bytes, 2).unwrap(), response);
        assert!(decode_host_response(&bytes[..bytes.len() - 1], 2).is_err());
        let mut trailing = bytes;
        trailing.push(0);
        assert!(decode_host_response(&trailing, 2).is_err());
    }
    let oversized = HostResponse {
        connection: 2,
        request_id: 8,
        body: HostResponseBody::WorldGraphBindings {
            job: 3,
            offset: 0,
            bindings: vec![(WorldGraphNodeId(0), reference); MAX_GRAPH_BINDING_PAGE + 1],
        },
    };
    assert!(encode_host_response(&oversized).is_err());
}
