use super::*;

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
fn lifecycle_envelopes_preserve_host_identity_world_metadata_and_sparse_hints() {
    let world = descriptor();
    let requests = [
        HostRequestBody::ListWorlds {
            after: 8,
        },
        HostRequestBody::CreateWorld {
            options: WorldCreateOptions {
                symbolic_id: world.metadata.symbolic_id.clone(),
                capacity_hints: world.capacity_hints.clone(),
            },
            temporary: true,
        },
        HostRequestBody::AttachWorld(WorldSelector::SymbolicId("workshop".into())),
        HostRequestBody::RenameWorld {
            world: WorldSelector::Id(world.id),
            symbolic_id: "renamed".into(),
        },
        HostRequestBody::DestroyWorld(WorldSelector::Id(world.id)),
        HostRequestBody::DetachWorld,
        HostRequestBody::SetCapacityHints(ipp_core::WorldCapacityHintsPatch {
            entities: Some(0),
            systems: BTreeMap::new(),
        }),
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
            world: world.clone(),
            session: 1 << 63 | 1,
        },
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
    use ipp_core::services::world_serialization::WorldLoadOptions;
    for body in [
        HostRequestBody::SaveWorld,
        HostRequestBody::ReadWorldSave {
            job: 2,
            offset: 65536,
        },
        HostRequestBody::BeginWorldLoad {
            bytes: 65539,
            options: WorldLoadOptions {
                symbolic_id: Some("copy".into()),
                capacity_hints: ipp_core::WorldCapacityHintsPatch {
                    entities: Some(1234),
                    ..Default::default()
                },
            },
        },
        HostRequestBody::WriteWorldLoad {
            job: 2,
            offset: 65536,
            bytes: vec![1, 2, 3],
        },
        HostRequestBody::FinishWorldLoad {
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
        HostResponseBody::SaveChunk {
            job: 2,
            offset: 65536,
            total: 65539,
            bytes: vec![1, 2, 3],
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
