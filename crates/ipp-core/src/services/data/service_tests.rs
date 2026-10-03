use super::*;
use crate::{
    EntityId, HostRuntime, WorldLimits, WorldRef,
    components::{DynamicPropertyKind as Kind, DynamicValue as Value},
};

fn world(host: &mut HostRuntime) -> WorldRef {
    let id = host.create_world(WorldLimits::default(), &[]).unwrap();
    host.world_ref(id).unwrap()
}

fn identity(world: WorldRef, binding: u64) -> DataConsumerIdentity {
    DataConsumerIdentity {
        world,
        entity: EntityId::from_bits(binding),
        binding_incarnation: binding,
    }
}

fn schema() -> DataSchema {
    DataSchema {
        columns: vec![
            DataColumn::new("time", Kind::U32),
            DataColumn::new("value", Kind::I32),
        ],
    }
}

fn request(kind: DataSourceKind, windows: Vec<DataWindow>) -> DataConsumerRequest {
    DataConsumerRequest {
        name: "dataset:measurements".into(),
        kind,
        windows,
    }
}

fn create(service: &mut DataService, kind: DataSourceKind) -> DataProducerHandle {
    service
        .create_source("dataset:measurements".into(), kind, schema())
        .unwrap()
}

fn append(
    service: &mut DataService,
    producer: DataProducerHandle,
    times: &[u32],
) -> DataBatchOutcome {
    let rows = times
        .iter()
        .map(|&time| vec![Value::U32(time), Value::I32(-(time as i32))])
        .collect();
    service
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows,
            }],
        )
        .unwrap()
}

fn ids(view: DataReadView<'_>) -> Vec<u64> {
    view.rows().map(|row| row.id.0).collect()
}

fn times(view: DataReadView<'_>) -> Vec<u32> {
    view.rows()
        .map(|row| match row.values[0] {
            Value::U32(value) => value,
            _ => panic!("fixture type"),
        })
        .collect()
}

fn range(width: f64, anchor: DataWindowAnchor) -> DataWindow {
    DataWindow::Range {
        column: "time".into(),
        width,
        anchor,
    }
}

#[test]
fn buffer_insert_edit_remove_keep_commit_order_identity_and_owned_payload() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Buffer);
    let consumer = service
        .register_consumer(identity(world, 1), request(DataSourceKind::Buffer, vec![]))
        .unwrap();
    append(service, producer, &[10, 30]);
    let outcome = service
        .apply_batch(
            producer,
            [
                DataDelta::Insert {
                    index: 1,
                    rows: vec![vec![Value::U32(20), Value::I32(-20)]],
                },
                DataDelta::Edit {
                    row: DataRowId(2),
                    values: vec![Value::U32(31), Value::I32(-31)],
                },
                DataDelta::Remove {
                    row: DataRowId(1),
                },
            ],
        )
        .unwrap();
    assert_eq!(
        outcome,
        DataBatchOutcome {
            committed_deltas: 3,
            assigned_rows: 1,
            last_assigned_row: Some(DataRowId(3))
        }
    );
    let view = service.read_consumer(consumer).unwrap();
    assert_eq!(ids(view), [3, 2]);
    assert_eq!(times(view), [20, 31]);
    assert_eq!(
        view.row(DataRowId(2)).unwrap().value(1),
        Some(&Value::I32(-31))
    );
    assert_eq!(view.schema().column_index("value"), Some(1));
    service.release_consumer(consumer).unwrap();
    assert_eq!(
        times(service.read_source(producer.source()).unwrap()),
        [20, 31]
    );
}

#[test]
fn malformed_delta_rejects_it_wholly_and_reports_committed_prefix() {
    let mut service = DataService::new();
    let producer = create(&mut service, DataSourceKind::Buffer);
    let error = service
        .apply_batch(
            producer,
            [
                DataDelta::Append {
                    rows: vec![vec![Value::U32(10), Value::I32(4)]],
                },
                DataDelta::Append {
                    rows: vec![
                        vec![Value::U32(20), Value::I32(5)],
                        vec![Value::I32(30), Value::I32(6)],
                    ],
                },
                DataDelta::Remove {
                    row: DataRowId(1),
                },
            ],
        )
        .unwrap_err();
    assert_eq!(error.delta_index, 1);
    assert_eq!(error.reason, DataError::InvalidRow);
    assert_eq!(
        error.committed,
        DataBatchOutcome {
            committed_deltas: 1,
            assigned_rows: 1,
            last_assigned_row: Some(DataRowId(1))
        }
    );
    assert!(error.to_string().contains("1 earlier deltas committed"));
    assert_eq!(times(service.read_source(producer.source()).unwrap()), [10]);
    assert_eq!(
        append(&mut service, producer, &[40]).last_assigned_row,
        Some(DataRowId(2))
    );
}

#[test]
fn raw_intersection_and_cross_world_union_keep_independent_views() {
    let mut host = HostRuntime::new();
    let first = world(&mut host);
    let second = world(&mut host);
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    let a = service
        .register_consumer(
            identity(first, 1),
            request(
                DataSourceKind::Streaming,
                vec![
                    DataWindow::Count(4),
                    range(4.0, DataWindowAnchor::Supplied(Value::U32(8))),
                ],
            ),
        )
        .unwrap();
    let b = service
        .register_consumer(
            identity(second, 1),
            request(
                DataSourceKind::Streaming,
                vec![range(2.0, DataWindowAnchor::Supplied(Value::U32(4)))],
            ),
        )
        .unwrap();
    // Commit order differs from raw time order: last four arrivals are 9,6,7,8.
    append(service, producer, &[1, 2, 3, 4, 5, 9, 6, 7, 8]);
    assert_eq!(times(service.read_consumer(a).unwrap()), [6, 7, 8]);
    assert_eq!(times(service.read_consumer(b).unwrap()), [2, 3, 4]);
    assert_eq!(
        times(service.read_source(producer.source()).unwrap()),
        [2, 3, 4, 6, 7, 8]
    );
    assert!(host.destroy_world(first.id()));
    assert_eq!(
        times(host.data_sources().read_source(producer.source()).unwrap()),
        [2, 3, 4]
    );
    assert_eq!(
        host.data_sources().consumer_state(a),
        Err(DataError::StaleConsumer)
    );
    assert_eq!(
        times(host.data_sources().read_consumer(b).unwrap()),
        [2, 3, 4]
    );
}

#[test]
fn latest_anchor_never_rewinds_and_widening_does_not_resurrect_history() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    let consumer = service
        .register_consumer(
            identity(world, 1),
            request(
                DataSourceKind::Streaming,
                vec![range(2.0, DataWindowAnchor::Latest)],
            ),
        )
        .unwrap();
    append(service, producer, &[3, 10, 8, 9]);
    assert_eq!(times(service.read_consumer(consumer).unwrap()), [10, 8, 9]);
    assert_eq!(
        append(service, producer, &[1]).last_assigned_row,
        Some(DataRowId(5))
    );
    assert_eq!(times(service.read_consumer(consumer).unwrap()), [10, 8, 9]);
    service
        .update_consumer(
            consumer,
            request(
                DataSourceKind::Streaming,
                vec![range(20.0, DataWindowAnchor::Latest)],
            ),
        )
        .unwrap();
    assert_eq!(times(service.read_consumer(consumer).unwrap()), [10, 8, 9]);
    append(service, producer, &[2]);
    assert_eq!(ids(service.read_consumer(consumer).unwrap()), [2, 3, 4, 6]);
}

#[test]
fn count_window_uses_all_committed_arrivals_including_immediately_expired_rows() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    assert_eq!(append(service, producer, &[1, 2, 3]).assigned_rows, 3);
    assert!(service.read_source(producer.source()).unwrap().is_empty());
    let consumer = service
        .register_consumer(
            identity(world, 1),
            request(DataSourceKind::Streaming, vec![DataWindow::Count(2)]),
        )
        .unwrap();
    assert!(service.read_consumer(consumer).unwrap().is_empty());
    append(service, producer, &[10, 9, 8]);
    assert_eq!(ids(service.read_consumer(consumer).unwrap()), [5, 6]);
    service.release_consumer(consumer).unwrap();
    assert_eq!(
        service
            .read_source(producer.source())
            .unwrap()
            .memory()
            .allocated_bytes,
        0
    );
    assert_eq!(
        append(service, producer, &[7]).last_assigned_row,
        Some(DataRowId(7))
    );
}

#[test]
fn host_time_expires_idle_streams_and_reading_does_not_progress_time() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    host.frame(5.0).unwrap();
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    let consumer = service
        .register_consumer(
            identity(world, 1),
            request(
                DataSourceKind::Streaming,
                vec![range(
                    2.0,
                    DataWindowAnchor::HostTime {
                        units_per_second: 10.0,
                    },
                )],
            ),
        )
        .unwrap();
    append(service, producer, &[47, 48, 49, 50, 51]);
    assert_eq!(
        times(service.read_consumer(consumer).unwrap()),
        [48, 49, 50]
    );
    service.take_notification(consumer).unwrap();
    assert_eq!(service.time(), 5.0);
    assert_eq!(
        times(service.read_consumer(consumer).unwrap()),
        [48, 49, 50]
    );
    assert!(service.take_notification(consumer).unwrap().is_none());
    host.frame(0.2).unwrap();
    assert_eq!(
        times(host.data_sources().read_consumer(consumer).unwrap()),
        [50]
    );
    let notification = host
        .data_sources_mut()
        .take_notification(consumer)
        .unwrap()
        .unwrap();
    assert!(notification.changed);
    assert!(!notification.availability_changed);
    host.frame(1.0).unwrap();
    assert!(
        host.data_sources()
            .read_source(producer.source())
            .unwrap()
            .is_empty()
    );
    assert!(host.frame(f64::NAN).is_err());
    assert_eq!(host.data_sources().time(), 6.2);
}

#[test]
fn producer_disconnect_replacement_destroy_and_stale_consumer_are_fenced() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let consumer = service
        .register_consumer(identity(world, 1), request(DataSourceKind::Buffer, vec![]))
        .unwrap();
    assert_eq!(
        service.consumer_state(consumer).unwrap().availability,
        DataAvailability::Unavailable(DataError::MissingSource)
    );
    let old = create(service, DataSourceKind::Buffer);
    append(service, old, &[1]);
    service.take_notification(consumer).unwrap();
    assert_eq!(
        service.create_source(
            "dataset:measurements".into(),
            DataSourceKind::Buffer,
            schema()
        ),
        Err(DataError::ProducerExists)
    );
    service.detach_producer(old).unwrap();
    assert_eq!(times(service.read_consumer(consumer).unwrap()), [1]);
    assert_eq!(
        service
            .apply_batch(
                old,
                [DataDelta::Remove {
                    row: DataRowId(1)
                }]
            )
            .unwrap_err()
            .reason,
        DataError::StaleProducer
    );
    let fresh = create(service, DataSourceKind::Buffer);
    assert_ne!(fresh.source(), old.source());
    assert_eq!(
        service.read_source(old.source()).err(),
        Some(DataError::StaleSource)
    );
    let notification = service.take_notification(consumer).unwrap().unwrap();
    assert!(notification.availability_changed && notification.changed);
    assert_eq!(notification.state.source, Some(fresh.source()));
    assert_eq!(
        append(service, fresh, &[2]).last_assigned_row,
        Some(DataRowId(1))
    );
    assert_eq!(service.destroy_source(old), Err(DataError::StaleProducer));
    service.destroy_source(fresh).unwrap();
    assert_eq!(
        service.read_consumer(consumer).err(),
        Some(DataError::MissingSource)
    );
    service.release_consumer(consumer).unwrap();
    let replacement = service
        .register_consumer(identity(world, 1), request(DataSourceKind::Buffer, vec![]))
        .unwrap();
    assert_ne!(replacement, consumer);
    assert_eq!(
        service.release_consumer(consumer),
        Err(DataError::StaleConsumer)
    );
    assert_eq!(
        service.update_consumer(consumer, request(DataSourceKind::Buffer, vec![])),
        Err(DataError::StaleConsumer)
    );
}

#[test]
fn missing_kind_and_schema_resolution_remain_observably_unavailable() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let consumer = service
        .register_consumer(
            identity(world, 1),
            request(
                DataSourceKind::Streaming,
                vec![range(1.0, DataWindowAnchor::Latest)],
            ),
        )
        .unwrap();
    let producer = create(service, DataSourceKind::Buffer);
    assert_eq!(
        service.read_consumer(consumer).err(),
        Some(DataError::KindMismatch)
    );
    service.detach_producer(producer).unwrap();
    let wrong_schema = DataSchema {
        columns: vec![DataColumn::new("other", Kind::U32)],
    };
    let producer = service
        .create_source(
            "dataset:measurements".into(),
            DataSourceKind::Streaming,
            wrong_schema,
        )
        .unwrap();
    assert_eq!(
        service.read_consumer(consumer).err(),
        Some(DataError::InvalidColumn)
    );
    service.detach_producer(producer).unwrap();
    let producer = create(service, DataSourceKind::Streaming);
    append(service, producer, &[3]);
    assert_eq!(times(service.read_consumer(consumer).unwrap()), [3]);
    assert_eq!(
        service.register_consumer(
            identity(world, 1),
            request(DataSourceKind::Streaming, vec![])
        ),
        Err(DataError::ConsumerExists)
    );
}

#[test]
fn stream_only_accepts_append_and_buffer_windows_are_rejected() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    for delta in [
        DataDelta::Insert {
            index: 0,
            rows: vec![],
        },
        DataDelta::Edit {
            row: DataRowId(1),
            values: vec![],
        },
        DataDelta::Remove {
            row: DataRowId(1),
        },
    ] {
        assert_eq!(
            service.apply_batch(producer, [delta]).unwrap_err().reason,
            DataError::UnsupportedDelta
        );
    }
    assert_eq!(
        service.register_consumer(
            identity(world, 1),
            request(DataSourceKind::Buffer, vec![DataWindow::Count(1)])
        ),
        Err(DataError::InvalidWindow)
    );
}

#[test]
fn large_integer_range_endpoints_and_negative_values_are_exact() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    let consumer = service
        .register_consumer(
            identity(world, 1),
            request(
                DataSourceKind::Streaming,
                vec![range(1.0, DataWindowAnchor::Supplied(Value::U32(u32::MAX)))],
            ),
        )
        .unwrap();
    append(service, producer, &[u32::MAX - 2, u32::MAX - 1, u32::MAX]);
    assert_eq!(
        times(service.read_consumer(consumer).unwrap()),
        [u32::MAX - 1, u32::MAX]
    );
    let columns = DataSchema {
        columns: vec![DataColumn::new("signed", Kind::I32)],
    };
    let producer = service
        .create_source("dataset:signed".into(), DataSourceKind::Streaming, columns)
        .unwrap();
    let consumer = service
        .register_consumer(
            identity(world, 2),
            DataConsumerRequest {
                name: "dataset:signed".into(),
                kind: DataSourceKind::Streaming,
                windows: vec![DataWindow::Range {
                    column: "signed".into(),
                    width: 1.0,
                    anchor: DataWindowAnchor::Supplied(Value::I32(i32::MIN + 1)),
                }],
            },
        )
        .unwrap();
    service
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows: [i32::MIN, i32::MIN + 1, i32::MIN + 2]
                    .into_iter()
                    .map(|value| vec![Value::I32(value)])
                    .collect(),
            }],
        )
        .unwrap();
    assert_eq!(ids(service.read_consumer(consumer).unwrap()), [1, 2]);
}

#[test]
fn schema_and_rows_reject_unsupported_types_and_invalid_values() {
    let mut service = DataService::new();
    for schema in [
        DataSchema {
            columns: vec![],
        },
        DataSchema {
            columns: vec![
                DataColumn::new("same", Kind::F32),
                DataColumn::new("same", Kind::U32),
            ],
        },
        DataSchema {
            columns: vec![DataColumn::new("", Kind::Bool)],
        },
        DataSchema {
            columns: vec![DataColumn::new("text", Kind::Text)],
        },
    ] {
        assert_eq!(
            service.create_source("dataset:invalid".into(), DataSourceKind::Buffer, schema),
            Err(DataError::InvalidSchema)
        );
    }
    assert_eq!(
        service.create_source(
            "dataset:assets".into(),
            DataSourceKind::Buffer,
            DataSchema {
                columns: vec![DataColumn::new("asset", Kind::Asset)]
            }
        ),
        Err(DataError::UnsupportedKind)
    );
    let producer = service
        .create_source(
            "dataset:text".into(),
            DataSourceKind::Buffer,
            DataSchema {
                columns: vec![
                    DataColumn::text("label", 3),
                    DataColumn::new("x", Kind::F32),
                ],
            },
        )
        .unwrap();
    for values in [
        vec![Value::Text("four".into()), Value::F32(0.0)],
        vec![Value::Text("ok".into()), Value::F32(f32::NAN)],
    ] {
        assert_eq!(
            service
                .apply_batch(
                    producer,
                    [DataDelta::Append {
                        rows: vec![values]
                    }]
                )
                .unwrap_err()
                .reason,
            DataError::InvalidRow
        );
    }
    service
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows: vec![vec![Value::Text("hi".into()), Value::F32(1.5)]],
            }],
        )
        .unwrap();
    assert_eq!(service.read_source(producer.source()).unwrap().len(), 1);
}

#[test]
fn invalid_window_updates_leave_previous_demand_and_pending_state_unchanged() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    let consumer = service
        .register_consumer(
            identity(world, 1),
            request(
                DataSourceKind::Streaming,
                vec![range(2.0, DataWindowAnchor::Supplied(Value::U32(10)))],
            ),
        )
        .unwrap();
    append(service, producer, &[8, 9, 10]);
    service.take_notification(consumer).unwrap();
    assert_eq!(
        service.update_consumer(
            consumer,
            request(
                DataSourceKind::Streaming,
                vec![range(2.0, DataWindowAnchor::Supplied(Value::U32(9)))]
            )
        ),
        Err(DataError::AnchorMovedBackwards)
    );
    assert_eq!(
        service.update_consumer(
            consumer,
            request(
                DataSourceKind::Streaming,
                vec![range(f64::INFINITY, DataWindowAnchor::Latest)]
            )
        ),
        Err(DataError::InvalidWindow)
    );
    assert!(service.take_notification(consumer).unwrap().is_none());
    assert_eq!(times(service.read_consumer(consumer).unwrap()), [8, 9, 10]);
}

#[test]
fn default_cap_is_per_consumer_and_explicit_demand_bypasses_it() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    let background = service
        .register_consumer(
            identity(world, 1),
            request(DataSourceKind::Streaming, vec![DataWindow::Count(10)]),
        )
        .unwrap();
    append(service, producer, &[1]);
    let row_bytes = service
        .read_source(producer.source())
        .unwrap()
        .memory()
        .retained_bytes;
    service.configure(DataServiceConfig {
        default_stream_bytes: row_bytes * 2,
    });
    let capped = service
        .register_consumer(
            identity(world, 2),
            request(DataSourceKind::Streaming, vec![]),
        )
        .unwrap();
    append(service, producer, &[2, 3, 4, 5]);
    assert_eq!(times(service.read_consumer(capped).unwrap()), [4, 5]);
    assert_eq!(
        times(service.read_consumer(background).unwrap()),
        [1, 2, 3, 4, 5]
    );
    service.release_consumer(background).unwrap();
    assert_eq!(
        times(service.read_source(producer.source()).unwrap()),
        [4, 5]
    );
    service.configure(DataServiceConfig {
        default_stream_bytes: row_bytes,
    });
    assert_eq!(times(service.read_source(producer.source()).unwrap()), [5]);
    append(service, producer, &[6]);
    assert_eq!(times(service.read_consumer(capped).unwrap()), [6]);
    let full = service
        .register_consumer(
            identity(world, 3),
            request(DataSourceKind::Streaming, vec![DataWindow::Count(10)]),
        )
        .unwrap();
    append(service, producer, &[7, 8]);
    assert_eq!(times(service.read_consumer(full).unwrap()), [6, 7, 8]);
    assert_eq!(times(service.read_consumer(capped).unwrap()), [8]);
}

#[test]
fn oversized_latest_arrival_cannot_make_default_cap_retain_older_samples() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = service
        .create_source(
            "dataset:measurements".into(),
            DataSourceKind::Streaming,
            DataSchema {
                columns: vec![DataColumn::text("text", 1000)],
            },
        )
        .unwrap();
    let consumer = service
        .register_consumer(
            identity(world, 1),
            request(DataSourceKind::Streaming, vec![]),
        )
        .unwrap();
    service
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows: vec![vec![Value::Text("x".into())]],
            }],
        )
        .unwrap();
    let bytes = service
        .read_source(producer.source())
        .unwrap()
        .memory()
        .retained_bytes;
    service.configure(DataServiceConfig {
        default_stream_bytes: bytes,
    });
    service
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows: vec![vec![Value::Text("longer".into())]],
            }],
        )
        .unwrap();
    assert!(service.read_consumer(consumer).unwrap().is_empty());
    assert_eq!(
        service
            .read_source(producer.source())
            .unwrap()
            .memory()
            .allocated_bytes,
        0
    );
}

#[test]
fn independent_services_reject_each_others_handles_and_detached_unused_sources_die() {
    let mut first = DataService::new();
    let mut second = DataService::new();
    let producer = create(&mut first, DataSourceKind::Buffer);
    let other = create(&mut second, DataSourceKind::Buffer);
    assert_ne!(producer.source(), other.source());
    assert_eq!(
        second.apply_batch(producer, []).unwrap_err().reason,
        DataError::StaleProducer
    );
    assert_eq!(
        second.read_source(producer.source()).err(),
        Some(DataError::StaleSource)
    );
    first.detach_producer(producer).unwrap();
    assert!(first.resolve_source("dataset:measurements").is_none());
}

#[test]
fn exhaustion_and_allocation_capacity_errors_do_not_consume_identity() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    let consumer = service
        .register_consumer(
            identity(world, 1),
            request(DataSourceKind::Streaming, vec![]),
        )
        .unwrap();
    service.configure(DataServiceConfig {
        default_stream_bytes: 0,
    });
    service.sources[0].next_row = u64::MAX - 1;
    assert_eq!(
        append(service, producer, &[1]).last_assigned_row,
        Some(DataRowId(u64::MAX))
    );
    assert!(service.read_consumer(consumer).unwrap().is_empty());
    assert_eq!(
        service
            .apply_batch(
                producer,
                [DataDelta::Append {
                    rows: vec![vec![Value::U32(2), Value::I32(2)]]
                }]
            )
            .unwrap_err()
            .reason,
        DataError::Capacity
    );
    assert_eq!(service.sources[0].next_row, u64::MAX);
    // Capacity overflow exercises std's fallible allocation boundary without exhausting the machine.
    assert!(service.sources[0].rows.try_reserve(usize::MAX).is_err());
}

#[test]
fn all_supported_fixed_width_kinds_use_existing_dynamic_values() {
    let values = vec![
        Value::F32(1.0),
        Value::I32(-1),
        Value::U32(1),
        Value::Bool(true),
        Value::Vec2([1.0; 2]),
        Value::Vec3([1.0; 3]),
        Value::Vec4([1.0; 4]),
        Value::Mat2([1.0; 4]),
        Value::Mat3([1.0; 9]),
        Value::Mat4([1.0; 16]),
    ];
    let schema = DataSchema {
        columns: values
            .iter()
            .enumerate()
            .map(|(index, value)| DataColumn::new(index.to_string(), value.kind()))
            .collect(),
    };
    let mut service = DataService::new();
    let producer = service
        .create_source("dataset:fixed".into(), DataSourceKind::Buffer, schema)
        .unwrap();
    service
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows: vec![values.clone()],
            }],
        )
        .unwrap();
    assert_eq!(
        service
            .read_source(producer.source())
            .unwrap()
            .rows()
            .next()
            .unwrap()
            .values,
        values
    );
}

#[test]
#[ignore = "narrow measurements; run explicitly in release mode with --nocapture"]
fn measure_bulk_ingestion_and_retained_memory() {
    for count in [1_000, 10_000, 100_000] {
        let mut host = HostRuntime::new();
        let world = world(&mut host);
        let service = host.data_sources_mut();
        let producer = create(service, DataSourceKind::Streaming);
        let consumer = service
            .register_consumer(
                identity(world, 1),
                request(DataSourceKind::Streaming, vec![DataWindow::Count(count)]),
            )
            .unwrap();
        let rows = (0..count)
            .map(|value| vec![Value::U32(value as u32), Value::I32(-(value as i32))])
            .collect();
        let begin = std::time::Instant::now();
        let outcome = service
            .apply_batch(
                producer,
                [DataDelta::Append {
                    rows,
                }],
            )
            .unwrap();
        let elapsed = begin.elapsed();
        let begin = std::time::Instant::now();
        let sum: u64 = service
            .read_consumer(consumer)
            .unwrap()
            .rows()
            .map(|row| match row.values[0] {
                Value::U32(value) => u64::from(value),
                _ => unreachable!(),
            })
            .sum();
        let read_elapsed = begin.elapsed();
        assert_eq!(sum, (count as u64) * (count as u64 - 1) / 2);
        assert_eq!(outcome.assigned_rows, count);
        let memory = service.read_source(producer.source()).unwrap().memory();
        println!(
            "rows={count} ingest_us={} read_us={} retained_bytes={} allocated_bytes={} schema_bytes={}",
            elapsed.as_micros(),
            read_elapsed.as_micros(),
            memory.retained_bytes,
            memory.allocated_bytes,
            memory.schema_bytes
        );
    }
}

#[test]
fn changing_anchor_modes_cannot_rewind_same_column() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    host.frame(10.0).unwrap();
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    let consumer = service
        .register_consumer(
            identity(world, 1),
            request(
                DataSourceKind::Streaming,
                vec![range(10.0, DataWindowAnchor::Latest)],
            ),
        )
        .unwrap();
    append(service, producer, &[10]);
    assert_eq!(
        service.update_consumer(
            consumer,
            request(
                DataSourceKind::Streaming,
                vec![range(10.0, DataWindowAnchor::Supplied(Value::U32(9)))]
            )
        ),
        Err(DataError::AnchorMovedBackwards)
    );
    service
        .update_consumer(
            consumer,
            request(
                DataSourceKind::Streaming,
                vec![range(
                    10.0,
                    DataWindowAnchor::HostTime {
                        units_per_second: 1.0,
                    },
                )],
            ),
        )
        .unwrap();
    assert_eq!(
        service.update_consumer(
            consumer,
            request(
                DataSourceKind::Streaming,
                vec![range(
                    10.0,
                    DataWindowAnchor::HostTime {
                        units_per_second: 0.5
                    }
                )]
            )
        ),
        Err(DataError::AnchorMovedBackwards)
    );
    service
        .update_consumer(
            consumer,
            request(
                DataSourceKind::Streaming,
                vec![range(10.0, DataWindowAnchor::Supplied(Value::U32(20)))],
            ),
        )
        .unwrap();
    assert_eq!(
        service.update_consumer(
            consumer,
            request(
                DataSourceKind::Streaming,
                vec![range(10.0, DataWindowAnchor::Latest)]
            )
        ),
        Err(DataError::AnchorMovedBackwards)
    );
}

#[test]
fn malformed_edits_positions_and_missing_rows_preserve_the_committed_prefix() {
    let mut service = DataService::new();
    let producer = create(&mut service, DataSourceKind::Buffer);
    append(&mut service, producer, &[1, 2]);
    let error = service
        .apply_batch(
            producer,
            [
                DataDelta::Edit {
                    row: DataRowId(1),
                    values: vec![Value::U32(10), Value::I32(-10)],
                },
                DataDelta::Insert {
                    index: 3,
                    rows: vec![vec![Value::U32(3), Value::I32(-3)]],
                },
                DataDelta::Remove {
                    row: DataRowId(2),
                },
            ],
        )
        .unwrap_err();
    assert_eq!(error.reason, DataError::InvalidPosition);
    assert_eq!(error.committed.committed_deltas, 1);
    assert_eq!(
        times(service.read_source(producer.source()).unwrap()),
        [10, 2]
    );
    for delta in [
        DataDelta::Edit {
            row: DataRowId(2),
            values: vec![Value::U32(99)],
        },
        DataDelta::Remove {
            row: DataRowId(99),
        },
    ] {
        assert!(service.apply_batch(producer, [delta]).is_err());
        assert_eq!(
            times(service.read_source(producer.source()).unwrap()),
            [10, 2]
        );
    }
}

#[test]
fn range_constraints_on_distinct_raw_columns_intersect_and_float_scalars_are_supported() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = service
        .create_source(
            "dataset:measurements".into(),
            DataSourceKind::Streaming,
            DataSchema {
                columns: vec![
                    DataColumn::new("time", Kind::U32),
                    DataColumn::new("value", Kind::F32),
                ],
            },
        )
        .unwrap();
    let consumer = service
        .register_consumer(
            identity(world, 1),
            request(
                DataSourceKind::Streaming,
                vec![
                    range(2.0, DataWindowAnchor::Supplied(Value::U32(4))),
                    DataWindow::Range {
                        column: "value".into(),
                        width: 1.0,
                        anchor: DataWindowAnchor::Supplied(Value::F32(2.5)),
                    },
                ],
            ),
        )
        .unwrap();
    service
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows: vec![
                    vec![Value::U32(1), Value::F32(2.0)],
                    vec![Value::U32(2), Value::F32(1.5)],
                    vec![Value::U32(3), Value::F32(3.0)],
                    vec![Value::U32(4), Value::F32(2.5)],
                ],
            }],
        )
        .unwrap();
    assert_eq!(ids(service.read_consumer(consumer).unwrap()), [2, 4]);
}

#[test]
fn detach_preserves_idle_expiry_and_all_consumers_observe_destroy_and_replacement() {
    let mut host = HostRuntime::new();
    let first = world(&mut host);
    let second = world(&mut host);
    host.frame(5.0).unwrap();
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    let windows = vec![range(
        1.0,
        DataWindowAnchor::HostTime {
            units_per_second: 1.0,
        },
    )];
    let a = service
        .register_consumer(
            identity(first, 1),
            request(DataSourceKind::Streaming, windows.clone()),
        )
        .unwrap();
    let b = service
        .register_consumer(
            identity(second, 1),
            request(DataSourceKind::Streaming, windows),
        )
        .unwrap();
    append(service, producer, &[4, 5]);
    service.detach_producer(producer).unwrap();
    host.frame(2.0).unwrap();
    assert!(host.data_sources().read_consumer(a).unwrap().is_empty());
    assert!(host.data_sources().read_consumer(b).unwrap().is_empty());
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    for handle in [a, b] {
        assert_eq!(
            service
                .take_notification(handle)
                .unwrap()
                .unwrap()
                .state
                .source,
            Some(producer.source())
        );
    }
    service.destroy_source(producer).unwrap();
    for handle in [a, b] {
        let notification = service.take_notification(handle).unwrap().unwrap();
        assert!(notification.availability_changed);
        assert_eq!(
            notification.state.availability,
            DataAvailability::Unavailable(DataError::MissingSource)
        );
    }
}

#[test]
fn idle_frames_do_not_rewind_a_default_cap_window_into_another_consumers_history() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = service
        .create_source(
            "dataset:measurements".into(),
            DataSourceKind::Streaming,
            DataSchema {
                columns: vec![
                    DataColumn::new("time", Kind::U32),
                    DataColumn::text("text", 1000),
                ],
            },
        )
        .unwrap();
    let explicit = service
        .register_consumer(
            identity(world, 1),
            request(
                DataSourceKind::Streaming,
                vec![range(0.0, DataWindowAnchor::Supplied(Value::U32(0)))],
            ),
        )
        .unwrap();
    let capped = service
        .register_consumer(
            identity(world, 2),
            request(DataSourceKind::Streaming, vec![]),
        )
        .unwrap();
    service
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows: vec![vec![Value::U32(0), Value::Text("x".into())]],
            }],
        )
        .unwrap();
    let cap = service
        .read_consumer(capped)
        .unwrap()
        .memory()
        .retained_bytes;
    service.configure(DataServiceConfig {
        default_stream_bytes: cap,
    });
    service
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows: vec![vec![Value::U32(1), Value::Text("oversized".into())]],
            }],
        )
        .unwrap();
    assert!(service.read_consumer(capped).unwrap().is_empty());
    assert_eq!(ids(service.read_source(producer.source()).unwrap()), [1]);
    host.frame(1.0).unwrap();
    assert_eq!(
        ids(host.data_sources().read_consumer(explicit).unwrap()),
        [1]
    );
    assert!(
        host.data_sources()
            .read_consumer(capped)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn repeated_raw_column_constraints_keep_independent_forward_anchors() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    let initial = request(
        DataSourceKind::Streaming,
        vec![
            range(10.0, DataWindowAnchor::Supplied(Value::U32(10))),
            range(20.0, DataWindowAnchor::Supplied(Value::U32(20))),
        ],
    );
    let consumer = service
        .register_consumer(identity(world, 1), initial.clone())
        .unwrap();
    append(service, producer, &[1, 5, 10, 12]);
    service.update_consumer(consumer, initial).unwrap();
    service
        .update_consumer(
            consumer,
            request(
                DataSourceKind::Streaming,
                vec![
                    range(11.0, DataWindowAnchor::Supplied(Value::U32(10))),
                    range(21.0, DataWindowAnchor::Supplied(Value::U32(20))),
                ],
            ),
        )
        .unwrap();
    assert_eq!(times(service.read_consumer(consumer).unwrap()), [1, 5, 10]);
    assert_eq!(
        service.update_consumer(
            consumer,
            request(
                DataSourceKind::Streaming,
                vec![
                    range(11.0, DataWindowAnchor::Supplied(Value::U32(10))),
                    range(21.0, DataWindowAnchor::Supplied(Value::U32(19))),
                ]
            )
        ),
        Err(DataError::AnchorMovedBackwards)
    );
}

#[test]
fn contraction_releases_large_row_capacity_without_disabling_append_reuse() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    let consumer = service
        .register_consumer(
            identity(world, 1),
            request(DataSourceKind::Streaming, vec![DataWindow::Count(10_000)]),
        )
        .unwrap();
    append(service, producer, &(0..10_000).collect::<Vec<_>>());
    let large = service.read_source(producer.source()).unwrap().memory();
    service
        .update_consumer(
            consumer,
            request(DataSourceKind::Streaming, vec![DataWindow::Count(3)]),
        )
        .unwrap();
    let small = service.read_source(producer.source()).unwrap().memory();
    assert_eq!(
        ids(service.read_consumer(consumer).unwrap()),
        [9998, 9999, 10000]
    );
    assert!(small.allocated_bytes < large.allocated_bytes / 100);
    assert!(service.sources[0].rows.capacity() <= 6);
    append(service, producer, &[10_000]);
    assert_eq!(
        ids(service.read_consumer(consumer).unwrap()),
        [9999, 10000, 10001]
    );
    assert!(service.sources[0].rows.capacity() <= 6);
}

#[test]
fn commit_demand_release_defers_source_expiry_until_all_evaluation_readers_finish() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    let consumer = service
        .register_consumer(
            identity(world, 1),
            request(DataSourceKind::Streaming, vec![DataWindow::Count(3)]),
        )
        .unwrap();
    append(service, producer, &[1, 2, 3]);
    service.begin_evaluation();
    let first = ids(service.read_source(producer.source()).unwrap());
    let mut commit = super::DataConsumerAccess {
        service,
    };
    commit.release_consumer(consumer).unwrap();
    assert_eq!(ids(commit.read_source(producer.source()).unwrap()), first);
    service.end_evaluation();
    assert!(service.read_source(producer.source()).unwrap().is_empty());
    assert_eq!(service.sources[0].rows.capacity(), 0);
}

#[test]
fn consumer_demand_batch_finishes_on_unwind_and_respects_nested_evaluation() {
    for evaluating in [false, true] {
        let mut host = HostRuntime::new();
        let world = world(&mut host);
        let service = host.data_sources_mut();
        let producer = create(service, DataSourceKind::Streaming);
        let consumer = service
            .register_consumer(
                identity(world, 1),
                request(DataSourceKind::Streaming, vec![DataWindow::Count(3)]),
            )
            .unwrap();
        append(service, producer, &[1, 2, 3]);
        service.take_notification(consumer).unwrap();
        if evaluating {
            service.begin_evaluation();
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut outer = DataConsumerAccess {
                service,
            }
            .batch();
            outer
                .update_consumer(
                    consumer,
                    request(DataSourceKind::Streaming, vec![DataWindow::Count(1)]),
                )
                .unwrap();
            {
                let mut inner = DataConsumerAccess {
                    service: outer.service,
                }
                .batch();
                inner
                    .update_consumer(
                        consumer,
                        request(DataSourceKind::Streaming, vec![DataWindow::Count(2)]),
                    )
                    .unwrap();
            }
            assert_eq!(
                ids(outer.read_source(producer.source()).unwrap()),
                [1, 2, 3]
            );
            assert!(
                outer
                    .service
                    .take_notification(consumer)
                    .unwrap()
                    .unwrap()
                    .changed
            );
            panic!("test consumer batch unwind");
        }));
        assert!(result.is_err());
        if evaluating {
            assert_eq!(
                ids(service.read_source(producer.source()).unwrap()),
                [1, 2, 3]
            );
            service.end_evaluation();
        }
        assert_eq!(ids(service.read_source(producer.source()).unwrap()), [2, 3]);
        service
            .update_consumer(
                consumer,
                request(DataSourceKind::Streaming, vec![DataWindow::Count(1)]),
            )
            .unwrap();
        assert_eq!(ids(service.read_source(producer.source()).unwrap()), [3]);
        service.release_consumer(consumer).unwrap();
        assert!(service.read_source(producer.source()).unwrap().is_empty());
    }
}

#[test]
fn consumer_demand_batch_error_collects_detached_sources_and_keeps_exact_handles() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Buffer);
    let consumer = service
        .register_consumer(identity(world, 1), request(DataSourceKind::Buffer, vec![]))
        .unwrap();
    append(service, producer, &[7]);
    service.detach_producer(producer).unwrap();
    let replacement = {
        let mut batch = DataConsumerAccess {
            service,
        }
        .batch();
        batch.release_consumer(consumer).unwrap();
        let replacement = batch
            .register_consumer(identity(world, 2), request(DataSourceKind::Buffer, vec![]))
            .unwrap();
        assert_eq!(
            batch.consumer_state(consumer),
            Err(DataError::StaleConsumer)
        );
        replacement
    };
    assert_eq!(times(service.read_consumer(replacement).unwrap()), [7]);
    let result = (|| -> Result<(), DataError> {
        let mut batch = DataConsumerAccess {
            service,
        }
        .batch();
        batch.release_consumer(replacement)?;
        assert_eq!(times(batch.read_source(producer.source())?), [7]);
        batch.release_consumer(consumer)?;
        Ok(())
    })();
    assert_eq!(result, Err(DataError::StaleConsumer));
    assert!(service.read_source(producer.source()).is_err());
    assert_eq!(
        service.consumer_state(replacement),
        Err(DataError::StaleConsumer)
    );
    // Subsequent ordinary teardown must collect immediately, with no leaked scope.
    let next = create(service, DataSourceKind::Buffer);
    let next_consumer = service
        .register_consumer(identity(world, 3), request(DataSourceKind::Buffer, vec![]))
        .unwrap();
    service.detach_producer(next).unwrap();
    service.release_consumer(next_consumer).unwrap();
    assert!(service.read_source(next.source()).is_err());
}

#[test]
fn request_preflight_reuses_admission_validation_without_mutating_demand_or_flags() {
    let mut host = HostRuntime::new();
    let world = world(&mut host);
    let service = host.data_sources_mut();
    let producer = create(service, DataSourceKind::Streaming);
    let original = request(
        DataSourceKind::Streaming,
        vec![range(2.0, DataWindowAnchor::Supplied(Value::U32(10)))],
    );
    let consumer = service
        .register_consumer(identity(world, 1), original.clone())
        .unwrap();
    append(service, producer, &[8, 9, 10]);
    service.take_notification(consumer).unwrap();
    for (next, expected) in [
        (
            request(
                DataSourceKind::Streaming,
                vec![range(2.0, DataWindowAnchor::Supplied(Value::U32(9)))],
            ),
            DataError::AnchorMovedBackwards,
        ),
        (
            request(
                DataSourceKind::Streaming,
                vec![range(f64::NAN, DataWindowAnchor::Latest)],
            ),
            DataError::InvalidWindow,
        ),
        (
            request(DataSourceKind::Buffer, vec![DataWindow::Count(1)]),
            DataError::InvalidWindow,
        ),
    ] {
        assert_eq!(
            service.validate_window_update(&original, &next),
            Err(expected)
        );
        assert_eq!(service.update_consumer(consumer, next), Err(expected));
        assert_eq!(times(service.read_consumer(consumer).unwrap()), [8, 9, 10]);
        assert!(service.take_notification(consumer).unwrap().is_none());
    }
    let valid = request(DataSourceKind::Streaming, vec![DataWindow::Count(1)]);
    service.validate_consumer_request(&valid).unwrap();
    service.validate_window_update(&original, &valid).unwrap();
    assert_eq!(times(service.read_consumer(consumer).unwrap()), [8, 9, 10]);
    assert!(service.take_notification(consumer).unwrap().is_none());
    // Preflight does not acquire retention for a missing source.
    let mut missing = valid;
    missing.name = "dataset:missing".into();
    service.validate_consumer_request(&missing).unwrap();
    assert_eq!(service.consumers.len(), 1);
}

#[test]
fn row_memory_counts_reserved_value_storage_and_text_after_edit() {
    let mut service = DataService::new();
    let producer = service
        .create_source(
            "dataset:text".into(),
            DataSourceKind::Buffer,
            DataSchema {
                columns: vec![DataColumn::text("text", 100)],
            },
        )
        .unwrap();
    let mut values = Vec::with_capacity(128);
    values.push(Value::Text("payload".into()));
    let expected = std::mem::size_of::<super::source::DataRow>()
        + values.capacity() * std::mem::size_of::<Value>()
        + 7;
    service
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows: vec![values],
            }],
        )
        .unwrap();
    let memory = service.read_source(producer.source()).unwrap().memory();
    assert_eq!(memory.retained_bytes, expected);
    assert!(memory.allocated_bytes >= expected);
    let mut replacement = Vec::with_capacity(64);
    replacement.push(Value::Text("short".into()));
    let expected = std::mem::size_of::<super::source::DataRow>()
        + replacement.capacity() * std::mem::size_of::<Value>()
        + 5;
    service
        .apply_batch(
            producer,
            [DataDelta::Edit {
                row: DataRowId(1),
                values: replacement,
            }],
        )
        .unwrap();
    assert_eq!(
        service
            .read_source(producer.source())
            .unwrap()
            .memory()
            .retained_bytes,
        expected
    );
}
