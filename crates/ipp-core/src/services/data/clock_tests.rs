use super::*;
use crate::{DynamicPropertyKind, DynamicValue, EntityId, HostRuntime, WorldLimits};

fn row_ids(host: &HostRuntime, producer: DataProducerHandle) -> Vec<u64> {
    host.data_sources()
        .read_source(producer.source())
        .unwrap()
        .rows()
        .map(|row| row.id.0)
        .collect()
}

#[test]
fn clock_expiry_keeps_union_and_does_not_dirty_unaffected_streams() {
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default(), &[]).unwrap();
    let world_ref = host.world_ref(world).unwrap();
    host.frame(10.0).unwrap();
    let service = host.data_sources_mut();
    let mut producers = Vec::new();
    for name in ["clock", "static"] {
        producers.push(
            service
                .create_source(
                    name.into(),
                    DataSourceKind::Streaming,
                    DataSchema {
                        columns: vec![DataColumn::new("time", DynamicPropertyKind::F32)],
                    },
                )
                .unwrap(),
        );
    }
    let mut consumers = Vec::new();
    for (index, (name, windows)) in [
        ("clock", vec![DataWindow::Count(2)]),
        (
            "clock",
            vec![DataWindow::Range {
                column: "time".into(),
                width: 4.0,
                anchor: DataWindowAnchor::HostTime {
                    units_per_second: 1.0,
                },
            }],
        ),
        (
            "static",
            vec![DataWindow::Range {
                column: "time".into(),
                width: 20.0,
                anchor: DataWindowAnchor::Latest,
            }],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        consumers.push(
            service
                .register_consumer(
                    DataConsumerIdentity {
                        world: world_ref,
                        entity: EntityId::from_bits(index as u64 + 1),
                        binding_incarnation: 1,
                    },
                    DataConsumerRequest {
                        name: name.into(),
                        kind: DataSourceKind::Streaming,
                        windows,
                    },
                )
                .unwrap(),
        );
    }
    for producer in &producers {
        service
            .apply_batch(
                *producer,
                [DataDelta::Append {
                    rows: (5..=11)
                        .map(|time| vec![DynamicValue::F32(time as f32)])
                        .collect(),
                }],
            )
            .unwrap();
    }
    for consumer in &consumers {
        service.take_notification(*consumer).unwrap();
    }
    assert_eq!(row_ids(&host, producers[0]), [2, 3, 4, 5, 6, 7]);
    let static_memory = host
        .data_sources()
        .read_source(producers[1].source())
        .unwrap()
        .memory();

    host.frame(0.0).unwrap();
    for consumer in &consumers {
        assert!(
            host.data_sources_mut()
                .take_notification(*consumer)
                .unwrap()
                .is_none()
        );
    }
    host.frame(3.0).unwrap();
    assert_eq!(row_ids(&host, producers[0]), [5, 6, 7]);
    assert_eq!(row_ids(&host, producers[1]), [1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(
        host.data_sources()
            .read_source(producers[1].source())
            .unwrap()
            .memory(),
        static_memory
    );
    assert!(
        host.data_sources_mut()
            .take_notification(consumers[0])
            .unwrap()
            .is_none()
    );
    assert!(
        host.data_sources_mut()
            .take_notification(consumers[1])
            .unwrap()
            .unwrap()
            .changed
    );
    assert!(
        host.data_sources_mut()
            .take_notification(consumers[2])
            .unwrap()
            .is_none()
    );

    host.data_sources_mut()
        .release_consumer(consumers[0])
        .unwrap();
    host.data_sources_mut()
        .detach_producer(producers[0])
        .unwrap();
    host.frame(5.0).unwrap();
    assert!(row_ids(&host, producers[0]).is_empty());
    assert_eq!(row_ids(&host, producers[1]), [1, 2, 3, 4, 5, 6, 7]);
}

#[test]
fn zero_clock_preserves_deferred_demand_cleanup() {
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default(), &[]).unwrap();
    let world_ref = host.world_ref(world).unwrap();
    let service = host.data_sources_mut();
    let producer = service
        .create_source(
            "stream".into(),
            DataSourceKind::Streaming,
            DataSchema {
                columns: vec![DataColumn::new("time", DynamicPropertyKind::F32)],
            },
        )
        .unwrap();
    let mut consumers = Vec::new();
    for (index, count) in [4, 1].into_iter().enumerate() {
        consumers.push(
            service
                .register_consumer(
                    DataConsumerIdentity {
                        world: world_ref,
                        entity: EntityId::from_bits(index as u64 + 1),
                        binding_incarnation: 1,
                    },
                    DataConsumerRequest {
                        name: "stream".into(),
                        kind: DataSourceKind::Streaming,
                        windows: vec![DataWindow::Count(count)],
                    },
                )
                .unwrap(),
        );
    }
    service
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows: (1..=4)
                    .map(|time| vec![DynamicValue::F32(time as f32)])
                    .collect(),
            }],
        )
        .unwrap();
    let prior_deferred = service.begin_consumer_updates();
    assert!(!prior_deferred);
    service.release_consumer(consumers[0]).unwrap();
    service.advance_time(0.0).unwrap();
    assert_eq!(service.read_source(producer.source()).unwrap().len(), 4);
    service.end_consumer_updates(prior_deferred);
    assert_eq!(
        service
            .read_source(producer.source())
            .unwrap()
            .rows()
            .map(|row| row.id.0)
            .collect::<Vec<_>>(),
        [4]
    );
}
