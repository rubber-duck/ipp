//! Allocation accounting against production core (without checked-invariants).
#![cfg(feature = "instrumentation")]

use ipp_core::components::{
    BoundingGeometry, Camera, FlatSurface, Light, PickingGeometry, Transform,
};
use ipp_core::systems::canvas::{CanvasBox, CanvasStyle};
use ipp_core::{
    Batch, Command, ComponentValue as C, EntityRef, HostRuntime, WorldAttachment, profiling,
};

#[global_allocator]
static ALLOCATOR: profiling::CountingAllocator = profiling::CountingAllocator;

static CAPTURE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn unchanged_world_frames_reuse_storage() {
    let _capture = CAPTURE.lock().unwrap();
    let mut host = HostRuntime::new();
    let systems: Vec<_> = host.system_ids().collect();
    let parent = host.create_world(Default::default(), &systems).unwrap();
    let child = host.create_world(Default::default(), &systems).unwrap();
    let attachment = WorldAttachment::spatial(host.world_ref(child).unwrap());
    for world in [parent, child] {
        let mut operations = vec![Command::Create {
            alias: 0,
            metadata: Default::default(),
            adopt: false,
        }];
        for component in [
            C::Transform(Transform::default()),
            C::Camera(Camera::default()),
            C::BoundingGeometry(BoundingGeometry::default()),
            C::PickingGeometry(PickingGeometry::default()),
            C::CanvasStyle(CanvasStyle::default()),
            C::CanvasBox(CanvasBox::default()),
            C::FlatSurface(FlatSurface::default()),
            C::Light(Light::default()),
        ] {
            operations.push(Command::insert_value(EntityRef::Alias(0), component));
        }
        if world == parent {
            operations.push(Command::insert_value(
                EntityRef::Alias(0),
                C::WorldAttachment(attachment.clone()),
            ));
        }
        host.world_mut(world)
            .unwrap()
            .enqueue(Batch {
                id: 1,
                operations,
            })
            .unwrap();
    }
    let mut report = ipp_core::HostFrameReport::default();
    host.frame_into(0.0, &mut report).unwrap();
    for result in report.worlds.values() {
        result.as_ref().unwrap().outcomes[0]
            .result
            .as_ref()
            .unwrap();
    }
    assert!(report.publication_errors.is_empty());
    for _ in 0..10 {
        host.frame_into(1.0 / 60.0, &mut report).unwrap();
    }

    let mut previous = host.latest_publication(parent).unwrap();
    profiling::reset_for_host(true, host.identity());
    for _ in 0..20 {
        host.frame_into(1.0 / 60.0, &mut report).unwrap();
        assert!(report.publication_errors.is_empty());
        let current = host.latest_publication(parent).unwrap();
        assert_ne!(current, previous);
        previous = current;
    }
    profiling::pause();
    let allocations = profiling::allocations();
    profiling::visit_capture(
        |stage| {
            if stage.values[2] > 0 {
                eprintln!("{stage:?}");
            }
            true
        },
        |name, _, values| {
            if values[0] > 0 {
                eprintln!("{name}: {values:?}");
            }
            true
        },
    );
    profiling::release_capture();
    assert_eq!(allocations, (0, 0));
}

struct Platform;

impl ipp_host_session::HostServices for Platform {
    const NAME: &'static str = "steady-frame-test";

    fn initialize(
        _: &mut HostRuntime,
        _: &ipp_host_session::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String> {
        Ok(Self)
    }

    fn service_resources(&mut self, _: &mut HostRuntime) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn steady_session_progress_uses_inline_storage() {
    let _capture = CAPTURE.lock().unwrap();
    let mut host = ipp_host_session::Host::<Platform>::new().unwrap();
    host.open_session(1, &[]).unwrap();
    host.session_mut(1)
        .unwrap()
        .receive(&ipp_protocol::contract::HELLO)
        .unwrap();
    drop(host.session_mut(1).unwrap().take_response().unwrap());
    for _ in 0..10 {
        host.tick(1.0 / 60.0).unwrap();
        drop(host.session_mut(1).unwrap().take_response().unwrap());
    }
    profiling::reset_for_host(true, host.runtime().identity());
    for _ in 0..20 {
        host.tick(1.0 / 60.0).unwrap();
        let (bytes, completion) = host
            .session_mut(1)
            .unwrap()
            .take_response()
            .unwrap()
            .into_parts();
        assert!(matches!(bytes, ipp_host_session::ResponseBytes::Frame(_)));
        drop(completion);
    }
    profiling::pause();
    let allocations = profiling::allocations();
    profiling::release_capture();
    assert_eq!(allocations, (0, 0));
}

#[test]
fn unchanged_filled_multiline_plot_reuses_publication_and_geometry() {
    use ipp_core::expressions::{ExpressionDeclaration, ExpressionInput, ExpressionNode};
    use ipp_core::services::{
        asset_management::{AssetSource, formats::expression::EXPRESSION_TYPE},
        data::*,
    };
    use ipp_core::systems::{SystemId, data_bindings::BufferDataSourceBinding, plot::*};
    use ipp_core::{DynamicPropertyKind as K, DynamicValue as V, EntityMetadata};

    let _capture = CAPTURE.lock().unwrap();
    let mut host = ipp_host_session::Host::<Platform>::new().unwrap();
    let runtime = host.runtime_mut();
    let world = runtime
        .create_world(
            Default::default(),
            &[
                SystemId("ipp.hierarchy"),
                SystemId("ipp.asset-dependencies"),
                SystemId("ipp.data-bindings"),
                PlotSystem::ID,
                SystemId("ipp.canvas"),
            ],
        )
        .unwrap();
    let source = "datasets://filled-plot-allocation";
    let producer = runtime
        .data_sources_mut()
        .create_source(
            source.into(),
            DataSourceKind::Buffer,
            DataSchema {
                columns: vec![DataColumn::new("x", K::F32), DataColumn::new("y", K::F32)],
            },
        )
        .unwrap();
    runtime
        .data_sources_mut()
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows: (0..600)
                    .map(|i| {
                        vec![
                            V::F32(i as f32 / 599.0),
                            V::F32(0.5 + (i as f32 * 0.1).sin() * 0.4),
                        ]
                    })
                    .collect(),
            }],
        )
        .unwrap();
    let mut binding = BufferDataSourceBinding {
        source: source.into(),
        ..Default::default()
    };
    for (id, name) in ["x", "y"].into_iter().enumerate() {
        let bytes = ExpressionDeclaration {
            inputs: vec![ExpressionInput {
                name: format!("column:{name}"),
                kind: K::F32,
            }],
            nodes: vec![ExpressionNode::Input(0)],
            output: 0,
        }
        .encode()
        .unwrap();
        runtime
            .asset_resources_mut()
            .register_client_source(
                world,
                AssetSource {
                    kind: EXPRESSION_TYPE,
                    uri: format!("producer://{}/19/{}", world.0, id + 1).into(),
                    variant: 0,
                },
                bytes,
            )
            .unwrap();
        binding
            .properties
            .set(
                name,
                V::Asset(AssetSource {
                    kind: EXPRESSION_TYPE,
                    uri: format!("asset://19/{}", id + 1).into(),
                    variant: 0,
                }),
            )
            .unwrap();
    }
    let mut line = PlotLine2d {
        interpolation: 1,
        fill_opacity: 0.35,
        marker_size: 4.0,
        ..Default::default()
    };
    for color in [[0.0, 0.8, 1.0, 1.0], [1.0, 0.2, 0.4, 1.0]] {
        line.series
            .push(PlotSeriesRow {
                color,
                ..Default::default()
            })
            .unwrap();
    }
    runtime
        .world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 0,
                    metadata: EntityMetadata {
                        symbolic_id: Some("filled-plot".into()),
                        ..Default::default()
                    },
                    adopt: false,
                },
                Command::insert_value(EntityRef::Alias(0), C::CanvasStyle(CanvasStyle::default())),
                Command::insert_value(EntityRef::Alias(0), C::PlotFrame2d(PlotFrame2d::default())),
                Command::insert_value(EntityRef::Alias(0), C::PlotLine2d(line)),
                Command::insert_value(EntityRef::Alias(0), C::BufferDataSourceBinding(binding)),
            ],
        })
        .unwrap();
    let geometry = |host: &ipp_host_session::Host<Platform>| {
        let runtime = host.runtime();
        let publication = runtime
            .publication(runtime.latest_publication(world).unwrap())
            .unwrap();
        let plot: &PlotPublication = publication.chunk(PlotSystem::ID).unwrap().data().unwrap();
        plot.charts.first().map(|chart| chart.geometry.clone())
    };
    for _ in 0..100 {
        host.tick(1.0 / 60.0).unwrap();
        if geometry(&host).is_some() {
            break;
        }
        std::thread::yield_now();
    }
    let original = geometry(&host).expect("real binding and filled Plot prepared");
    assert_eq!(
        original
            .canvas
            .iter()
            .filter(|p| matches!(&p.kind,PlotPrimitiveKind::Path(path) if path.is_fill()))
            .count(),
        2
    );
    assert!(
        original
            .canvas
            .iter()
            .any(|p| matches!(p.kind, PlotPrimitiveKind::Shape { .. }))
    );
    for _ in 0..10 {
        host.tick(1.0 / 60.0).unwrap();
    }
    profiling::reset_for_host(true, host.runtime().identity());
    for _ in 0..30 {
        host.tick(1.0 / 60.0).unwrap();
        assert!(std::sync::Arc::ptr_eq(&original, &geometry(&host).unwrap()));
    }
    profiling::pause();
    let allocations = profiling::allocations();
    profiling::release_capture();
    assert_eq!(allocations, (0, 0));
}
