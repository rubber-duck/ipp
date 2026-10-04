use super::*;
use ipp_core::services::world_serialization::WorldGraphNodeId;
use ipp_core::{Batch, Command, ComponentValue, EntityRef, WorldAttachment};
use ipp_protocol::host::{MAX_GRAPH_BINDING_PAGE, MAX_GRAPH_METADATA_PAGE};
use std::{collections::BTreeMap, time::Duration};

fn graph(host: &mut Host<TestHostServices>, children: usize) -> Vec<u8> {
    let runtime = host.runtime_mut();
    let root = runtime
        .create_world(
            Default::default(),
            &[ipp_core::systems::world_attachment::WorldAttachmentSystem::ID],
        )
        .unwrap();
    let mut operations = Vec::new();
    for index in 0..children {
        let child = runtime.create_world(Default::default(), &[]).unwrap();
        operations.push(Command::Create {
            alias: index as u32,
            metadata: Default::default(),
            adopt: false,
        });
        operations.push(Command::insert_value(
            EntityRef::Alias(index as u32),
            ComponentValue::WorldAttachment(WorldAttachment {
                child: runtime.world_ref(child),
                ..Default::default()
            }),
        ));
    }
    runtime
        .world_mut(root)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();

    runtime.frame(0.0).unwrap().worlds[&root]
        .as_ref()
        .unwrap()
        .outcomes[0]
        .result
        .as_ref()
        .unwrap();

    runtime
        .save_world(root, ipp_protocol::schema_hash(), Default::default())
        .unwrap()
}

fn upload(host: &mut Host<TestHostServices>, bytes: &[u8], suffix: &str) -> (u64, u32) {
    let HostResponseBody::Transfer {
        job,
    } = control(
        host,
        1,
        HostRequestBody::BeginWorldLoad {
            bytes: bytes.len() as u64,
        },
    )
    else {
        panic!("transfer")
    };
    for (index, chunk) in bytes.chunks(65536).enumerate() {
        assert_eq!(
            control(
                host,
                1,
                HostRequestBody::WriteWorldLoad {
                    job,
                    offset: (index * 65536) as u64,
                    bytes: chunk.to_vec()
                }
            ),
            HostResponseBody::Complete
        );
    }
    let mut offset = 0;
    let mut replacements = Vec::new();
    let total = loop {
        let HostResponseBody::WorldGraphPage {
            job: found,
            total,
            offset: found_offset,
            nodes,
            ..
        } = control(
            host,
            1,
            HostRequestBody::InspectWorldLoad {
                job,
                offset,
            },
        )
        else {
            panic!("preview")
        };
        assert_eq!(found, job);
        assert_eq!(found_offset, offset);
        assert!(!nodes.is_empty() && nodes.len() <= MAX_GRAPH_METADATA_PAGE);
        offset += nodes.len() as u32;
        replacements.extend(
            nodes
                .into_iter()
                .map(|node| (node.id, format!("{}-{suffix}", node.metadata.symbolic_id))),
        );
        if offset == total {
            break total;
        }
    };
    for page in replacements.chunks(MAX_GRAPH_METADATA_PAGE) {
        assert_eq!(
            control(
                host,
                1,
                HostRequestBody::SetWorldLoadNames {
                    job,
                    names: page.iter().cloned().collect()
                }
            ),
            HostResponseBody::Complete
        );
    }
    let HostResponseBody::WorldGraphLoaded {
        job: found,
        total: loaded,
        ..
    } = control(
        host,
        1,
        HostRequestBody::FinishWorldLoad {
            job,
            symbolic_id: None,
            capacity_hints: Default::default(),
        },
    )
    else {
        panic!("publication")
    };
    assert_eq!(found, job);
    assert_eq!(loaded, total);
    (job, total)
}

fn bindings(
    host: &mut Host<TestHostServices>,
    job: u64,
    total: u32,
) -> BTreeMap<WorldGraphNodeId, ipp_protocol::references::WorldReference> {
    let mut created = BTreeMap::new();
    while created.len() < total as usize {
        let offset = created.len() as u32;
        let HostResponseBody::WorldGraphBindings {
            job: found,
            offset: found_offset,
            bindings,
        } = control(
            host,
            1,
            HostRequestBody::ReadWorldLoadBindings {
                job,
                offset,
            },
        )
        else {
            panic!("bindings")
        };
        assert_eq!(found, job);
        assert_eq!(found_offset, offset);
        assert!(!bindings.is_empty() && bindings.len() <= MAX_GRAPH_BINDING_PAGE);
        for (node, world) in bindings {
            assert!(created.insert(node, world).is_none());
        }
    }
    created
}

#[test]
fn stale_graph_jobs_and_overlapping_begin_never_cancel_a_newer_published_graph() {
    for acknowledged in [false, true] {
        let mut host = Host::<TestHostServices>::new().unwrap();
        open(&mut host, 1);
        let bytes = graph(&mut host, MAX_GRAPH_METADATA_PAGE + 1);
        let (old, total) = upload(&mut host, &bytes, "old");
        if acknowledged {
            assert_eq!(bindings(&mut host, old, total).len(), total as usize);
            assert_eq!(
                control(
                    &mut host,
                    1,
                    HostRequestBody::AcknowledgeWorldLoad {
                        job: old
                    }
                ),
                HostResponseBody::Complete
            );
        } else {
            host.maintain_connections(Duration::from_secs(31));
        }
        let before = host.runtime().world_ids().len();
        let (current, total) = upload(&mut host, &bytes, "current");
        let reservation = host.connections.persistence.reserved;
        for request in [
            HostRequestBody::CancelWorldTransfer {
                job: old,
            },
            HostRequestBody::InspectWorldLoad {
                job: old,
                offset: 0,
            },
            HostRequestBody::ReadWorldLoadBindings {
                job: old,
                offset: 0,
            },
            HostRequestBody::AcknowledgeWorldLoad {
                job: old,
            },
            HostRequestBody::BeginWorldLoad {
                bytes: bytes.len() as u64,
            },
        ] {
            assert!(matches!(
                control(&mut host, 1, request),
                HostResponseBody::Error(_)
            ));
            assert_eq!(host.runtime().world_ids().len(), before + total as usize);
            assert_eq!(host.connections.persistence.reserved, reservation);
        }
        let created = bindings(&mut host, current, total);
        assert_eq!(
            control(
                &mut host,
                1,
                HostRequestBody::CancelWorldTransfer {
                    job: current
                }
            ),
            HostResponseBody::Complete
        );
        assert_eq!(host.runtime().world_ids().len(), before);
        for world in created.values() {
            assert!(world.resolve(host.runtime()).is_err());
        }
        assert_eq!(host.connections.persistence.reserved, 0);
        host.close_connection(1);
    }
}

#[test]
fn graph_binding_pages_account_every_world_and_ack_controls_disconnect_cleanup() {
    for acknowledge in [false, true] {
        let mut host = Host::<TestHostServices>::new().unwrap();
        open(&mut host, 1);
        let bytes = graph(&mut host, MAX_GRAPH_BINDING_PAGE);
        let before = host.runtime().world_ids().len();
        let (job, total) = upload(&mut host, &bytes, "copy");
        assert!(total as usize > MAX_GRAPH_BINDING_PAGE);
        let created = bindings(&mut host, job, total);
        if acknowledge {
            assert_eq!(
                control(
                    &mut host,
                    1,
                    HostRequestBody::AcknowledgeWorldLoad {
                        job
                    }
                ),
                HostResponseBody::Complete
            );
        }
        host.close_connection(1);
        assert_eq!(host.connections.persistence.reserved, 0);
        assert_eq!(
            host.runtime().world_ids().len(),
            before
                + if acknowledge {
                    total as usize
                } else {
                    0
                }
        );
        for world in created.values() {
            assert_eq!(world.resolve(host.runtime()).is_ok(), acknowledge);
        }
    }
}

#[test]
fn premature_graph_ack_cleans_only_its_own_created_worlds() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let bytes = graph(&mut host, 2);
    let before = host.runtime().world_ids().len();
    let (job, _) = upload(&mut host, &bytes, "copy");
    assert!(matches!(
        control(
            &mut host,
            1,
            HostRequestBody::AcknowledgeWorldLoad {
                job
            }
        ),
        HostResponseBody::Error(_)
    ));
    assert_eq!(host.runtime().world_ids().len(), before);
    assert_eq!(host.connections.persistence.reserved, 0);
}
