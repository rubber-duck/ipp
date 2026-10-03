//! Opt-in, single-threaded attribution fixture; native transport coverage is separate.
//! Build without instrumentation for timing, with instrumentation for allocation traffic.

use std::{hint::black_box, time::Instant};

use ipp_core::{
    Batch, Command, ComponentValue, DynamicPropertyKind as Kind, DynamicValue as Value, EntityId,
    EntityMetadata, EntityRef, HostRuntime, WorldLimits,
    expressions::{BinaryOperator, ExpressionDeclaration, ExpressionInput, ExpressionNode},
    services::{
        asset_management::{AssetSource, expression::EXPRESSION_TYPE},
        data::*,
    },
    systems::{SystemId, data_bindings::*},
};

#[cfg(feature = "instrumentation")]
#[global_allocator]
static ALLOCATOR: ipp_core::profiling::CountingAllocator = ipp_core::profiling::CountingAllocator;

struct Options {
    case: String,
    rows: usize,
    bindings: usize,
    iterations: usize,
    allocations: bool,
}

impl Options {
    fn read() -> Self {
        let mut options = Self {
            case: "stream-idle".into(),
            rows: 10_000,
            bindings: 1,
            iterations: 100,
            allocations: false,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--case" => options.case = args.next().expect("case"),
                "--rows" => options.rows = args.next().expect("rows").parse().unwrap(),
                "--bindings" => {
                    options.bindings = args.next().expect("bindings").parse().unwrap();
                }
                "--iterations" => {
                    options.iterations = args.next().expect("iterations").parse().unwrap();
                }
                "--allocations" => options.allocations = true,
                _ => panic!("unknown argument {arg}"),
            }
        }
        assert!((1..=1_000_000).contains(&options.rows));
        assert!((1..=32).contains(&options.bindings));
        assert!((1..=100_000).contains(&options.iterations));
        assert_eq!(options.allocations, cfg!(feature = "instrumentation"));
        assert!(matches!(
            options.case.as_str(),
            "stream-idle"
                | "stream-clock"
                | "stream-range-idle"
                | "buffer-append"
                | "buffer-edit"
                | "binding-edit"
                | "binding-stream-edit"
                | "binding-idle"
        ));
        options
    }
}

fn row(index: usize) -> Vec<Value> {
    vec![Value::F32(index as f32), Value::F32(index as f32 * 0.5)]
}

fn append(host: &mut HostRuntime, producer: DataProducerHandle, rows: Vec<Vec<Value>>) {
    let outcome = host
        .data_sources_mut()
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows,
            }],
        )
        .unwrap();
    assert_eq!(outcome.committed_deltas, 1);
}

fn main() {
    let options = Options::read();
    let mut host = HostRuntime::new();
    let binding_case = options.case.starts_with("binding-");
    let streaming = options.case.starts_with("stream-") || options.case == "binding-stream-edit";
    let systems = if binding_case {
        &[DataBindingSystem::ID, SystemId("ipp.asset-dependencies")][..]
    } else {
        &[][..]
    };
    let world = host.create_world(WorldLimits::default(), systems).unwrap();
    let kind = if streaming {
        DataSourceKind::Streaming
    } else {
        DataSourceKind::Buffer
    };
    let producer = host
        .data_sources_mut()
        .create_source(
            "dataset:profile".into(),
            kind,
            DataSchema {
                columns: vec![
                    DataColumn::new("x", Kind::F32),
                    DataColumn::new("y", Kind::F32),
                ],
            },
        )
        .unwrap();
    let windows = if options.case == "stream-range-idle" {
        vec![DataWindow::Range {
            column: "x".into(),
            width: options.rows as f64,
            anchor: DataWindowAnchor::Latest,
        }]
    } else {
        vec![DataWindow::Count(options.rows)]
    };
    if binding_case {
        let identity = ExpressionDeclaration {
            inputs: vec![ExpressionInput {
                name: "column:x".into(),
                kind: Kind::F32,
            }],
            nodes: vec![ExpressionNode::Input(0)],
            output: 0,
        };
        let computed = ExpressionDeclaration {
            inputs: vec![
                ExpressionInput {
                    name: "column:y".into(),
                    kind: Kind::F32,
                },
                ExpressionInput {
                    name: "parameter".into(),
                    kind: Kind::F32,
                },
            ],
            nodes: vec![
                ExpressionNode::Input(0),
                ExpressionNode::Input(1),
                ExpressionNode::Binary {
                    operator: BinaryOperator::Multiply,
                    left: 0,
                    right: 1,
                },
            ],
            output: 2,
        };
        for (id, definition) in [(1, identity), (2, computed)] {
            host.asset_resources_mut()
                .register_client_source(
                    world,
                    AssetSource {
                        kind: EXPRESSION_TYPE,
                        uri: format!("producer://{}/19/{id}", world.0).into(),
                        variant: 0,
                    },
                    definition.encode().unwrap(),
                )
                .unwrap();
        }
        let mut operations = Vec::new();
        for index in 0..options.bindings {
            let mut properties = ipp_core::DynamicProperties::default();
            for (name, id) in [("x", 1), ("scaled", 2)] {
                properties
                    .set(
                        name,
                        Value::Asset(AssetSource {
                            kind: EXPRESSION_TYPE,
                            uri: format!("asset://19/{id}").into(),
                            variant: 0,
                        }),
                    )
                    .unwrap();
            }
            properties.set("scaled_parameter", Value::F32(2.0)).unwrap();
            let value = if streaming {
                let mut binding = StreamingDataSourceBinding {
                    source: "dataset:profile".into(),
                    properties,
                    ..Default::default()
                };
                binding.set_windows(&windows).unwrap();
                ComponentValue::StreamingDataSourceBinding(binding)
            } else {
                ComponentValue::BufferDataSourceBinding(BufferDataSourceBinding {
                    source: "dataset:profile".into(),
                    properties,
                    ..Default::default()
                })
            };
            operations.push(Command::Create {
                alias: index as u32,
                metadata: EntityMetadata {
                    symbolic_id: Some(format!("binding-{index}")),
                    ..Default::default()
                },
                adopt: false,
            });
            operations.push(Command::insert_value(EntityRef::Alias(index as u32), value));
        }
        host.world_mut(world)
            .unwrap()
            .enqueue(Batch {
                id: 1,
                operations,
            })
            .unwrap();
        host.frame(0.0).unwrap();
    } else if streaming {
        let world_ref = host.world_ref(world).unwrap();
        for index in 0..options.bindings {
            host.data_sources_mut()
                .register_consumer(
                    DataConsumerIdentity {
                        world: world_ref,
                        entity: EntityId::from_bits(index as u64 + 1),
                        binding_incarnation: index as u64 + 1,
                    },
                    DataConsumerRequest {
                        name: "dataset:profile".into(),
                        kind,
                        windows: windows.clone(),
                    },
                )
                .unwrap();
        }
    }
    append(&mut host, producer, (0..options.rows).map(row).collect());
    for _ in 0..3 {
        black_box(host.frame(0.0).unwrap());
    }
    let mut deltas = (0..options.iterations)
        .map(|index| {
            if options.case == "buffer-append" || options.case == "binding-stream-edit" {
                DataDelta::Append {
                    rows: vec![row(options.rows + index)],
                }
            } else {
                DataDelta::Edit {
                    row: DataRowId(options.rows as u64),
                    values: row(options.rows + index + 1),
                }
            }
        })
        .collect::<Vec<_>>()
        .into_iter();

    #[cfg(not(feature = "instrumentation"))]
    let mut samples = Vec::with_capacity(options.iterations);
    #[cfg(feature = "instrumentation")]
    ipp_core::profiling::reset(true);
    let started = Instant::now();
    for _ in 0..options.iterations {
        #[cfg(not(feature = "instrumentation"))]
        let operation_started = Instant::now();
        match options.case.as_str() {
            "buffer-append" | "buffer-edit" | "binding-edit" | "binding-stream-edit" => {
                black_box(
                    host.data_sources_mut()
                        .apply_batch(producer, [deltas.next().unwrap()])
                        .unwrap(),
                );
                if binding_case {
                    black_box(host.frame(0.0).unwrap());
                }
            }
            _ => {
                black_box(
                    host.frame(if options.case == "stream-clock" {
                        0.001
                    } else {
                        0.0
                    })
                    .unwrap(),
                );
            }
        }
        #[cfg(not(feature = "instrumentation"))]
        samples.push(operation_started.elapsed().as_nanos());
    }
    let elapsed = started.elapsed().as_nanos();
    #[cfg(feature = "instrumentation")]
    ipp_core::profiling::pause();
    #[cfg(feature = "instrumentation")]
    let (allocation_calls, requested_bytes) = ipp_core::profiling::allocations();
    #[cfg(not(feature = "instrumentation"))]
    let (allocation_calls, requested_bytes) = (0, 0);
    #[cfg(not(feature = "instrumentation"))]
    let (p50_ns, p95_ns) = {
        samples.sort_unstable();
        (
            samples[samples.len().div_ceil(2) - 1].to_string(),
            samples[(samples.len() * 95).div_ceil(100) - 1].to_string(),
        )
    };
    #[cfg(feature = "instrumentation")]
    let (p50_ns, p95_ns) = ("null", "null");
    let view = host
        .data_sources_mut()
        .read_source(producer.source())
        .unwrap();
    let retained_rows = view.len();
    let memory = view.memory();
    assert_eq!(
        retained_rows,
        options.rows + usize::from(options.case == "buffer-append") * options.iterations
    );
    if binding_case {
        let context = host.world_mut(world).unwrap();
        for index in 0..options.bindings {
            let entity = context.lookup_id(&format!("binding-{index}")).unwrap();
            let view = context
                .data_binding_view(
                    entity,
                    DataBindingViewQuery {
                        offset: options.rows - 1,
                        limit: 1,
                    },
                )
                .unwrap();
            assert_eq!(view.total_rows, options.rows);
            assert_eq!(view.columns.len(), 2);
            assert_eq!(view.source, Some(producer.source()));
            assert_eq!(view.availability, &DataBindingAvailability::Ready);
            let expected = match options.case.as_str() {
                "binding-edit" => options.rows + options.iterations,
                "binding-stream-edit" => options.rows + options.iterations - 1,
                _ => options.rows - 1,
            } as f32;
            for column in &view.columns {
                assert_eq!(
                    column.values,
                    &[ipp_core::expressions::ExpressionResult::Valid(Value::F32(
                        expected
                    ))]
                );
            }
        }
    }
    println!(
        "{{\"case\":\"{}\",\"mode\":\"{}\",\"rows\":{},\"bindings\":{},\"iterations\":{},\"elapsedNs\":{},\"p50Ns\":{},\"p95Ns\":{},\"allocationCalls\":{},\"requestedBytes\":{},\"retainedRows\":{},\"retainedBytes\":{},\"allocatedBytes\":{}}}",
        options.case,
        if options.allocations {
            "allocations"
        } else {
            "timing"
        },
        options.rows,
        options.bindings,
        options.iterations,
        elapsed,
        p50_ns,
        p95_ns,
        allocation_calls,
        requested_bytes,
        retained_rows,
        memory.retained_bytes,
        memory.allocated_bytes
    );
    #[cfg(feature = "instrumentation")]
    for index in 0..ipp_core::profiling::system_count() {
        for phase in 0..ipp_core::profiling::SYSTEM_PHASES {
            let slot = (index * ipp_core::profiling::SYSTEM_PHASES + phase) * 4;
            let calls = ipp_core::profiling::counter(slot);
            if calls > 0 {
                println!(
                    "{{\"stage\":\"{}\",\"phase\":{},\"calls\":{},\"nanoseconds\":{},\"allocationCalls\":{},\"requestedBytes\":{}}}",
                    ipp_core::profiling::system_name(index),
                    phase,
                    calls,
                    ipp_core::profiling::counter(slot + 1),
                    ipp_core::profiling::counter(slot + 2),
                    ipp_core::profiling::counter(slot + 3)
                );
            }
        }
    }
}
