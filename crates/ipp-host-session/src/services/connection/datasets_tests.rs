use super::*;
use ipp_core::components::{DynamicPropertyKind, DynamicValue};
use ipp_core::services::data::*;

struct Services;

impl HostServices for Services {
    const NAME: &'static str = "dataset-test";

    fn initialize(
        _: &mut ipp_core::HostRuntime,
        _schedulers: &crate::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String> {
        Ok(Self)
    }

    fn service_resources(&mut self, _: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }
}

fn open(host: &mut Host<Services>, id: u64) {
    host.open_connection(id).unwrap();
    host.receive_connection(id, &ipp_protocol::contract::HELLO)
        .unwrap();
    drop(host.take_connection_response(id).unwrap());
}

fn send(
    host: &mut Host<Services>,
    connection: u64,
    id: u64,
    operation: DatasetOperation<'_>,
) -> Vec<Vec<u8>> {
    let bytes = dataset::encode_request(connection, id, &operation).unwrap();
    let prepared = HostConnectionMessage::decode(bytes);
    host.receive_connection_message(connection, prepared)
        .unwrap();
    let mut output = Vec::new();
    while let Some(reply) = host.take_connection_response(connection) {
        output.push(reply.to_vec());
    }
    output
}

fn create(host: &mut Host<Services>, connection: u64, name: &str) -> u64 {
    let replies = send(
        host,
        connection,
        1,
        DatasetOperation::Create {
            name: name.into(),
            kind: DataSourceKind::Buffer,
            schema: DataSchema {
                columns: vec![DataColumn::new("value", DynamicPropertyKind::U32)],
            },
        },
    );
    assert_eq!(replies[0][20], 1);
    u64::from_le_bytes(replies[0][21..29].try_into().unwrap())
}

#[test]
fn complete_update_reports_one_prefix_outcome_and_local_admission_observes_it() {
    let mut host = Host::<Services>::new().unwrap();
    open(&mut host, 1);
    let token = create(&mut host, 1, "literal://stable/shared");
    let bytes = dataset::encode_update(&[
        DataDelta::Append {
            rows: vec![vec![DynamicValue::U32(u32::MAX)]],
        },
        DataDelta::Edit {
            row: DataRowId(1),
            values: vec![DynamicValue::F32(f32::NAN)],
        },
        DataDelta::Remove {
            row: DataRowId(1),
        },
    ])
    .unwrap();
    send(
        &mut host,
        1,
        2,
        DatasetOperation::Begin {
            producer: token,
            length: bytes.len() as u64,
        },
    );
    send(
        &mut host,
        1,
        3,
        DatasetOperation::Chunk {
            transfer: 2,
            offset: 0,
            bytes: &bytes,
        },
    );
    let replies = send(
        &mut host,
        1,
        4,
        DatasetOperation::Finish {
            transfer: 2,
        },
    );
    assert_eq!(replies.len(), 2);
    let outcome = replies.iter().find(|reply| reply[20] == 2).unwrap();
    assert_eq!(outcome[21], 1);
    assert_eq!(u64::from_le_bytes(outcome[22..30].try_into().unwrap()), 1);
    assert_eq!(u64::from_le_bytes(outcome[46..54].try_into().unwrap()), 1);
    let data = host.runtime.data_sources();
    let source = data.resolve_source("literal://stable/shared").unwrap();
    assert_eq!(
        data.read_source(source)
            .unwrap()
            .rows()
            .next()
            .unwrap()
            .values,
        &[DynamicValue::U32(u32::MAX)]
    );
    assert_eq!(host.connections.states[&1].reply_entries(), 0);
}

#[test]
fn foreign_handles_disconnect_replacement_and_interrupted_transfers_are_fenced() {
    let mut host = Host::<Services>::new().unwrap();
    open(&mut host, 1);
    open(&mut host, 2);
    let token = create(&mut host, 1, "shared");
    assert_eq!(
        send(
            &mut host,
            2,
            1,
            DatasetOperation::Begin {
                producer: token,
                length: 4
            }
        )[0][20],
        5
    );
    send(
        &mut host,
        1,
        2,
        DatasetOperation::Begin {
            producer: token,
            length: 4,
        },
    );
    assert_eq!(host.connections.states[&1].reply_entries(), 1);
    host.close_connection(1);
    assert!(
        host.runtime
            .data_sources()
            .resolve_source("shared")
            .is_none()
    );
    let reply = send(
        &mut host,
        2,
        2,
        DatasetOperation::Create {
            name: "shared".into(),
            kind: DataSourceKind::Buffer,
            schema: DataSchema {
                columns: vec![DataColumn::new("value", DynamicPropertyKind::U32)],
            },
        },
    );
    let fresh = u64::from_le_bytes(reply[0][21..29].try_into().unwrap());
    assert_ne!(fresh, token);
    assert_eq!(
        send(
            &mut host,
            2,
            3,
            DatasetOperation::Destroy {
                producer: token
            }
        )[0][20],
        5
    );
    send(
        &mut host,
        2,
        4,
        DatasetOperation::Destroy {
            producer: fresh,
        },
    );
    assert!(
        host.runtime
            .data_sources()
            .resolve_source("shared")
            .is_none()
    );
}

#[test]
fn bounded_pressure_keeps_peer_progress_and_cancel_releases_every_reservation() {
    let mut host = Host::<Services>::new().unwrap();
    open(&mut host, 1);
    open(&mut host, 2);
    let token = create(&mut host, 1, "pressure");
    for id in 2..4 {
        assert_eq!(
            send(
                &mut host,
                1,
                id,
                DatasetOperation::Begin {
                    producer: token,
                    length: dataset::UPDATE_BYTES as u64
                }
            )[0][20],
            0
        );
    }
    assert_eq!(
        send(
            &mut host,
            1,
            4,
            DatasetOperation::Begin {
                producer: token,
                length: 1
            }
        )[0][20],
        5
    );
    assert!(host.connection_accepts_input(1));
    assert_eq!(
        send(
            &mut host,
            2,
            1,
            DatasetOperation::Read {
                name: "pressure".into(),
                incarnation: Some(token),
                offset: 0,
                limit: 1
            }
        )[0][20],
        4
    );
    for (id, transfer) in [(5, 2), (6, 3)] {
        let replies = send(
            &mut host,
            1,
            id,
            DatasetOperation::Cancel {
                transfer,
            },
        );
        assert_eq!(replies.iter().filter(|reply| reply[20] == 6).count(), 1);
    }
    assert_eq!(host.connections.states[&1].datasets.declared_bytes, 0);
    assert_eq!(host.connections.states[&1].reply_entries(), 0);
}

#[test]
fn timeout_excludes_withholding_and_incomplete_finish_never_mutates() {
    let mut host = Host::<Services>::new().unwrap();
    open(&mut host, 1);
    let token = create(&mut host, 1, "timeouts");
    send(
        &mut host,
        1,
        2,
        DatasetOperation::Begin {
            producer: token,
            length: 4,
        },
    );
    host.connections.states.get_mut(&1).unwrap().throttled = true;
    host.maintain_connections(Duration::from_secs(60));
    assert!(host.take_connection_response(1).is_none());
    host.connections.states.get_mut(&1).unwrap().throttled = false;
    host.maintain_connections(Duration::from_secs(89));
    assert!(host.take_connection_response(1).is_none());
    host.maintain_connections(Duration::from_secs(90));
    assert_eq!(host.take_connection_response(1).unwrap()[20], 6);
    send(
        &mut host,
        1,
        3,
        DatasetOperation::Begin {
            producer: token,
            length: 4,
        },
    );
    let replies = send(
        &mut host,
        1,
        4,
        DatasetOperation::Finish {
            transfer: 3,
        },
    );
    assert_eq!(replies.iter().filter(|reply| reply[20] == 6).count(), 1);
    assert_eq!(
        host.runtime
            .data_sources()
            .read_source(
                host.runtime
                    .data_sources()
                    .resolve_source("timeouts")
                    .unwrap()
            )
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn ingress_count_pressure_cannot_withhold_an_admitted_transfer_continuation() {
    let mut host = Host::<Services>::new().unwrap();
    open(&mut host, 1);
    let producer = create(&mut host, 1, "continuations");
    let held: Vec<_> = (0..crate::MAX_PENDING * 3 / 4 - 1)
        .map(|_| host.connections.states[&1].reserve_reply(16).unwrap())
        .collect();
    let bytes = dataset::encode_update(&[DataDelta::Append {
        rows: vec![vec![DynamicValue::U32(77)]],
    }])
    .unwrap();
    send(
        &mut host,
        1,
        2,
        DatasetOperation::Begin {
            producer,
            length: bytes.len() as u64,
        },
    );
    assert_eq!(
        host.connections.states[&1].reply_entries(),
        crate::MAX_PENDING * 3 / 4
    );
    assert!(host.connection_accepts_input(1));
    send(
        &mut host,
        1,
        3,
        DatasetOperation::Chunk {
            transfer: 2,
            offset: 0,
            bytes: &bytes,
        },
    );
    assert!(host.connection_accepts_input(1));
    let replies = send(
        &mut host,
        1,
        4,
        DatasetOperation::Finish {
            transfer: 2,
        },
    );
    assert_eq!(replies.iter().filter(|reply| reply[20] == 2).count(), 1);
    drop(held);
    assert!(host.connection_accepts_input(1));
}

#[test]
fn observations_fence_sessions_and_hold_output_credit_until_delivery() {
    let mut host = Host::<Services>::new().unwrap();
    open(&mut host, 1);
    open(&mut host, 2);
    host.open_session(
        50,
        &[
            ipp_core::systems::SystemId("ipp.asset-dependencies"),
            ipp_core::systems::data_bindings::DataBindingSystem::ID,
        ],
    )
    .unwrap();
    host.connections
        .states
        .get_mut(&1)
        .unwrap()
        .sessions
        .insert(50);
    let rejected = send(
        &mut host,
        2,
        1,
        DatasetOperation::BindingView {
            session: 50,
            entity: 0,
            offset: 0,
            limit: 1,
        },
    );
    assert_eq!(rejected[0][20], 5);
    assert!(String::from_utf8_lossy(&rejected[0]).contains("another connection"));

    let world_id = host.session_world(50).unwrap();
    let mut world = host.runtime.world_mut(world_id).unwrap();
    world
        .enqueue(ipp_core::Batch {
            id: 1,
            operations: vec![
                ipp_core::Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                ipp_core::Command::InsertComponent {
                    entity: ipp_core::EntityRef::Alias(1),
                    component: ipp_core::ComponentValue::BUFFER_DATA_SOURCE_BINDING,
                    fields: vec![],
                    adopt: false,
                },
            ],
        })
        .unwrap();
    drop(world);
    let frame = host.runtime.frame(0.0).unwrap();
    let entity = frame.worlds[&world_id].as_ref().unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1;
    let before = host
        .runtime
        .world_mut(world_id)
        .unwrap()
        .data_binding_view(entity, Default::default())
        .unwrap()
        .to_owned();
    let bytes = dataset::encode_request(
        1,
        1,
        &DatasetOperation::BindingView {
            session: 50,
            entity: entity.to_bits(),
            offset: 0,
            limit: 1,
        },
    )
    .unwrap();
    host.receive_connection(1, &bytes).unwrap();
    assert_eq!(host.connections.states[&1].reply_entries(), 1);
    let delivery = host.take_connection_response(1).unwrap();
    assert_eq!(delivery[20], 7);
    assert_eq!(host.connections.states[&1].reply_entries(), 1);
    drop(delivery);
    assert_eq!(host.connections.states[&1].reply_entries(), 0);
    assert_eq!(
        host.runtime
            .world_mut(world_id)
            .unwrap()
            .data_binding_view(entity, Default::default())
            .unwrap()
            .to_owned(),
        before
    );
    assert!(before.dirty);
}
