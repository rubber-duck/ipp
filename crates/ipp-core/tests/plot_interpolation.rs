//! Column-local binding interpolation precedes Plot geometry and picking.

mod support;
use support::task_scheduler::HostTaskTestDriver;

use ipp_core::{
    Batch, Command, ComponentValue as C, DynamicPropertyKind as K, DynamicValue as V, EntityId,
    EntityMetadata, EntityRef, HostRuntime, WorldId,
    expressions::{ExpressionDeclaration, ExpressionInput, ExpressionNode, ExpressionResult},
    services::{
        asset_management::{AssetSource, formats::expression::EXPRESSION_TYPE},
        data::*,
    },
    systems::{SystemId, canvas::CanvasStyle, data_bindings::*, plot::*},
};

struct Fixture {
    host: HostRuntime,
    world: WorldId,
    entity: EntityId,
    producer: DataProducerHandle,
}

impl Fixture {
    fn new(initial: [f32; 2], percent: f32) -> Self {
        let mut host = support::task_scheduler::host();
        let world = host
            .create_world(
                Default::default(),
                &[
                    SystemId("ipp.asset-dependencies"),
                    DataBindingSystem::ID,
                    PlotSystem::ID,
                    SystemId("ipp.canvas"),
                ],
            )
            .unwrap();
        let producer = host
            .data_sources_mut()
            .create_source(
                "datasets://plot-percent".into(),
                DataSourceKind::Buffer,
                DataSchema {
                    columns: vec![
                        DataColumn::new("position", K::F32),
                        DataColumn::new("temperature", K::F32),
                        DataColumn::new("offset", K::F32),
                    ],
                },
            )
            .unwrap();
        host.data_sources_mut()
            .apply_batch(
                producer,
                [DataDelta::Append {
                    rows: initial
                        .into_iter()
                        .enumerate()
                        .map(|(index, value)| {
                            vec![V::F32(index as f32), V::F32(value), V::F32(1.0)]
                        })
                        .collect(),
                }],
            )
            .unwrap();
        let mut binding = BufferDataSourceBinding {
            source: "datasets://plot-percent".into(),
            ..Default::default()
        };
        for (id, name) in ["position", "temperature", "offset"]
            .into_iter()
            .enumerate()
        {
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
            host.asset_resources_mut()
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
        binding
            .properties
            .set("temperature_interp_percent", V::F32(percent))
            .unwrap();
        binding
            .properties
            .set("offset_interp", V::F32(4.0))
            .unwrap();
        let mut bars = PlotBars2d::default();
        bars.series
            .push(PlotSeriesRow {
                x: "position".into(),
                y: "temperature".into(),
                z: "".into(),
                ..Default::default()
            })
            .unwrap();
        host.world_mut(world)
            .unwrap()
            .enqueue(Batch {
                id: 1,
                operations: vec![
                    Command::Create {
                        alias: 0,
                        metadata: EntityMetadata {
                            symbolic_id: Some("chart".into()),
                            ..Default::default()
                        },
                        adopt: false,
                    },
                    Command::insert_value(
                        EntityRef::Alias(0),
                        C::CanvasStyle(CanvasStyle::default()),
                    ),
                    Command::insert_value(
                        EntityRef::Alias(0),
                        C::PlotFrame2d(PlotFrame2d {
                            min_y: -999.0,
                            max_y: 999.0,
                            ..Default::default()
                        }),
                    ),
                    Command::insert_value(EntityRef::Alias(0), C::PlotBars2d(bars)),
                    Command::insert_value(EntityRef::Alias(0), C::BufferDataSourceBinding(binding)),
                ],
            })
            .unwrap();
        let report = host.frame_for_test(0.0).unwrap();
        report.worlds[&world].as_ref().unwrap().outcomes[0]
            .result
            .as_ref()
            .unwrap();
        let entity = host.world_mut(world).unwrap().lookup_id("chart").unwrap();
        let mut fixture = Self {
            host,
            world,
            entity,
            producer,
        };
        for _ in 0..8 {
            fixture.step(0.0);
            if !fixture.geometry().charts.is_empty() {
                return fixture;
            }
        }
        panic!("initial Plot did not prepare");
    }

    fn step(&mut self, dt: f64) {
        let report = self.host.frame_for_test(dt).unwrap();
        assert!(report.worlds.values().all(Result::is_ok));
        assert!(report.publication_errors.is_empty());
    }

    fn targets(&mut self, values: [f32; 2]) {
        self.host
            .data_sources_mut()
            .apply_batch(
                self.producer,
                values
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| DataDelta::Edit {
                        row: DataRowId(index as u64 + 1),
                        values: vec![V::F32(index as f32), V::F32(value), V::F32(100.0)],
                    }),
            )
            .unwrap();
    }

    fn value(&mut self, name: &str, row: usize) -> f32 {
        let view = self
            .host
            .world_mut(self.world)
            .unwrap()
            .data_binding_view_owned(self.entity, Default::default())
            .unwrap();
        let column = view
            .columns
            .iter()
            .find(|column| column.name == name)
            .unwrap();
        let ExpressionResult::Valid(V::F32(value)) = column.values[row] else {
            panic!("scalar display")
        };
        value
    }

    fn geometry(&self) -> &PlotPublication {
        self.host
            .publication(self.host.latest_publication(self.world).unwrap())
            .unwrap()
            .chunk(PlotSystem::ID)
            .unwrap()
            .data()
            .unwrap()
    }

    fn command(&mut self, operation: Command) {
        self.host
            .world_mut(self.world)
            .unwrap()
            .enqueue(Batch {
                id: 2,
                operations: vec![operation],
            })
            .unwrap();
        let report = self.host.frame_for_test(0.0).unwrap();
        report.worlds[&self.world].as_ref().unwrap().outcomes[0]
            .result
            .as_ref()
            .unwrap();
    }

    fn reference(&mut self, value: Option<f32>) {
        let entity = EntityRef::Handle(self.entity);
        self.command(match value {
            Some(value) => Command::SetDynamicProperty {
                entity,
                component: C::BUFFER_DATA_SOURCE_BINDING,
                name: "temperature_interp_reference".into(),
                value: V::F32(value),
            },
            None => Command::RemoveDynamicProperty {
                entity,
                component: C::BUFFER_DATA_SOURCE_BINDING,
                name: "temperature_interp_reference".into(),
            },
        });
    }

    fn ambiguous_series(&mut self) {
        let mut chart = PlotBars2d::default();
        chart
            .series
            .push(PlotSeriesRow {
                x: "temperature".into(),
                y: "temperature".into(),
                z: "".into(),
                ..Default::default()
            })
            .unwrap();
        self.command(Command::insert_value(
            EntityRef::Handle(self.entity),
            C::PlotBars2d(chart),
        ));
    }
}

#[test]
fn percentage_uses_pre_step_column_range_and_steps_mixed_outputs_before_dirty_skip() {
    let mut fixture = Fixture::new([10.0, 20.0], 50.0);
    fixture.targets([50.0, 60.0]);
    fixture.step(0.5);
    assert_eq!(fixture.value("temperature", 0), 15.0);
    assert_eq!(fixture.value("temperature", 1), 25.0);
    assert_eq!(fixture.value("offset", 0), 3.0);
    assert!(
        !fixture
            .host
            .world_mut(fixture.world)
            .unwrap()
            .data_binding_view_owned(fixture.entity, Default::default())
            .unwrap()
            .dirty
    );
    // No source mutation: the previous successful presentation cleared dirty.
    fixture.step(0.5);
    assert_eq!(fixture.value("temperature", 0), 21.25);
    assert_eq!(fixture.value("temperature", 1), 31.25);
    assert_eq!(fixture.value("offset", 0), 5.0);
    let geometry = &fixture.geometry().charts[0].geometry;
    let PlotHitShape::Rect(rect) = geometry.hits[0].shape else {
        panic!("bar hit")
    };
    let frame = PlotFrame2d::default();
    let height = frame.height - frame.padding_bottom - frame.padding_top;
    let expected_top = frame.padding_top + height * (1.0 - 21.25 / 31.25);
    assert!((rect[1] - expected_top).abs() < 0.0001);
    assert!(
        geometry.hits[0]
            .shape
            .contains_canvas_point([(rect[0] + rect[2]) / 2.0, rect[1] + 1.0])
    );
}

#[test]
fn negative_percentage_uses_largest_absolute_fitted_endpoint() {
    let mut fixture = Fixture::new([-40.0, -10.0], 50.0);
    fixture.targets([-80.0, -50.0]);
    fixture.step(0.5);
    assert_eq!(fixture.value("temperature", 0), -50.0);
    assert_eq!(fixture.value("temperature", 1), -20.0);
    fixture.step(0.5);
    assert_eq!(fixture.value("temperature", 0), -62.5);
    assert_eq!(fixture.value("temperature", 1), -32.5);
}

#[test]
fn explicit_zero_holds_without_catch_up_and_explicit_scale_bypasses_axis_inference() {
    let mut fixture = Fixture::new([10.0, 20.0], 50.0);
    fixture.targets([50.0, 60.0]);
    fixture.reference(Some(0.0));
    fixture.step(5.0);
    assert_eq!(fixture.value("temperature", 0), 10.0);
    fixture.reference(None);
    fixture.step(0.5);
    assert_eq!(fixture.value("temperature", 0), 15.0);
    fixture.ambiguous_series();
    fixture.reference(Some(40.0));
    fixture.step(0.5);
    assert_eq!(fixture.value("temperature", 0), 25.0);
    assert_eq!(fixture.geometry().charts.len(), 1);
}

#[test]
fn percentage_is_independent_of_shared_axes_and_fixed_chart_bounds() {
    let mut fixture = Fixture::new([10.0, 20.0], 50.0);
    fixture.ambiguous_series();
    fixture.command(Command::insert_value(
        EntityRef::Handle(fixture.entity),
        C::PlotFrame2d(PlotFrame2d {
            automatic_x: false,
            automatic_y: false,
            min_x: 0.0,
            max_x: 1000.0,
            min_y: 0.0,
            max_y: 1000.0,
            ..Default::default()
        }),
    ));
    fixture.targets([50.0, 60.0]);
    fixture.step(0.5);
    assert_eq!(fixture.value("temperature", 0), 15.0);
    assert_eq!(fixture.value("temperature", 1), 25.0);
    assert_eq!(fixture.geometry().charts.len(), 1);
    // Removing the presentation consumer leaves ordinary binding progress active.
    fixture.command(Command::RemoveComponent {
        entity: EntityRef::Handle(fixture.entity),
        component: C::PLOT_BARS2D,
    });
    fixture.step(0.5);
    assert_eq!(fixture.value("temperature", 0), 21.25);
    assert_eq!(fixture.value("offset", 0), 5.0);
}

#[test]
fn sibling_paint_updates_and_replacement_keep_data_geometry_and_decoration() {
    use ipp_core::systems::canvas::{
        CanvasPaint, CanvasPaintEntry, CanvasPrimitive, CanvasPublication, CanvasShapeFill,
    };
    use std::sync::Arc;
    use support::CanvasTestHost;

    let mut fixture = Fixture::new([20.0, 40.0], 1.0);
    let output = fixture.host.canvas_output(
        fixture.host.world_ref(fixture.world).unwrap(),
        [640.0, 480.0],
        100.0,
    );
    fixture.step(0.0);
    let geometry = fixture.geometry().charts[0].geometry.clone();
    let canvas = |fixture: &Fixture| {
        fixture
            .host
            .output(
                fixture.host.latest_publication(fixture.world).unwrap(),
                output,
            )
            .unwrap()
            .data::<CanvasPublication>()
            .unwrap()
            .clone()
    };
    let flat = canvas(&fixture);
    assert!(flat.paints.is_empty());
    fixture.command(Command::insert_value(
        EntityRef::Handle(fixture.entity),
        C::CanvasPaint(CanvasPaint::default()),
    ));
    fixture.step(0.0);
    let painted = canvas(&fixture);
    assert_eq!(painted.paints.len(), 1);
    let target = painted.paints[0].target;
    let marks = painted.entries.iter().filter(|entry| matches!(entry.as_ref(), CanvasPaintEntry::Primitive {primitive: CanvasPrimitive::Box {fill: CanvasShapeFill::Paint {paint,..},..},..} if *paint == target)).count();
    assert_eq!(marks, 2);
    assert_eq!(painted.entries.len(), flat.entries.len());
    assert!(Arc::ptr_eq(
        &geometry,
        &fixture.geometry().charts[0].geometry
    ));

    fixture.command(Command::SetDynamicProperty {
        entity: EntityRef::Handle(fixture.entity),
        component: C::CANVAS_PAINT,
        name: "light".into(),
        value: V::Vec4([0.1, 0.2, 0.75, 1.0]),
    });
    fixture.step(0.0);
    let updated = canvas(&fixture);
    assert_eq!(updated.entries, painted.entries);
    assert_ne!(updated.paints, painted.paints);
    assert!(Arc::ptr_eq(
        &geometry,
        &fixture.geometry().charts[0].geometry
    ));

    fixture.command(Command::RemoveComponent {
        entity: EntityRef::Handle(fixture.entity),
        component: C::CANVAS_PAINT,
    });
    fixture.step(0.0);
    let removed = canvas(&fixture);
    assert!(removed.paints.is_empty());
    for (removed, original) in removed.entries.iter().zip(flat.entries.iter()) {
        let (
            CanvasPaintEntry::Primitive {
                primitive: removed,
                ..
            },
            CanvasPaintEntry::Primitive {
                primitive: original,
                ..
            },
        ) = (removed.as_ref(), original.as_ref())
        else {
            panic!("primitive chart")
        };
        assert_eq!(removed, original);
    }
    fixture.command(Command::insert_value(
        EntityRef::Handle(fixture.entity),
        C::CanvasPaint(CanvasPaint::default()),
    ));
    fixture.step(0.0);
    let replaced = canvas(&fixture);
    assert_eq!(replaced.paints.len(), 1);
    assert_ne!(replaced.paints[0].target.incarnation, target.incarnation);
    assert!(Arc::ptr_eq(
        &geometry,
        &fixture.geometry().charts[0].geometry
    ));
}

#[test]
fn sibling_paint_selects_line_and_sector_paths_without_painting_axes() {
    use ipp_core::systems::canvas::{
        CanvasPaint, CanvasPaintEntry, CanvasPrimitive, CanvasPublication,
    };
    use support::CanvasTestHost;
    let mut fixture = Fixture::new([20.0, 40.0], 1.0);
    let output = fixture.host.canvas_output(
        fixture.host.world_ref(fixture.world).unwrap(),
        [640.0, 480.0],
        100.0,
    );
    fixture.command(Command::RemoveComponent {
        entity: EntityRef::Handle(fixture.entity),
        component: C::PLOT_BARS2D,
    });
    let series = PlotSeriesRow {
        x: "position".into(),
        y: "temperature".into(),
        value: "temperature".into(),
        ..Default::default()
    };
    let mut line = PlotLine2d {
        interpolation: 1,
        ..Default::default()
    };
    line.series.push(series.clone()).unwrap();
    fixture.command(Command::insert_value(
        EntityRef::Handle(fixture.entity),
        C::PlotLine2d(line),
    ));
    fixture.command(Command::insert_value(
        EntityRef::Handle(fixture.entity),
        C::CanvasPaint(CanvasPaint::default()),
    ));
    fixture.step(0.0);
    let canvas = fixture
        .host
        .output(
            fixture.host.latest_publication(fixture.world).unwrap(),
            output,
        )
        .unwrap()
        .data::<CanvasPublication>()
        .unwrap();
    assert_eq!(canvas.paints.len(), 1);
    let painted = canvas
        .entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.as_ref(),
                CanvasPaintEntry::Primitive {
                    primitive: CanvasPrimitive::Path {
                        paint: Some(_),
                        ..
                    },
                    ..
                }
            )
        })
        .count();
    assert_eq!(painted, 3);
    assert!(canvas.entries.iter().any(|entry| matches!(
        entry.as_ref(),
        CanvasPaintEntry::Primitive {
            primitive: CanvasPrimitive::Path {
                paint: None,
                ..
            },
            ..
        }
    )));
    let line_geometry = fixture.geometry().charts[0].geometry.clone();
    assert_eq!(line_geometry.hits.len(), 2);
    fixture.command(Command::SetDynamicProperty {
        entity: EntityRef::Handle(fixture.entity),
        component: C::CANVAS_PAINT,
        name: "profile".into(),
        value: V::Vec4([1.0, 0.0, 1.0, 0.5]),
    });
    fixture.step(0.0);
    assert!(std::sync::Arc::ptr_eq(
        &line_geometry,
        &fixture.geometry().charts[0].geometry
    ));

    fixture.command(Command::RemoveComponent {
        entity: EntityRef::Handle(fixture.entity),
        component: C::PLOT_LINE2D,
    });
    let mut pie = PlotPie2d {
        inner_radius: 0.35,
        ..Default::default()
    };
    pie.series.push(series).unwrap();
    fixture.command(Command::insert_value(
        EntityRef::Handle(fixture.entity),
        C::PlotPie2d(pie),
    ));
    fixture.step(0.0);
    let canvas = fixture
        .host
        .output(
            fixture.host.latest_publication(fixture.world).unwrap(),
            output,
        )
        .unwrap()
        .data::<CanvasPublication>()
        .unwrap();
    assert_eq!(canvas.paints.len(), 1);
    assert_eq!(
        canvas
            .entries
            .iter()
            .filter(|entry| matches!(
                entry.as_ref(),
                CanvasPaintEntry::Primitive {
                    primitive: CanvasPrimitive::Path {
                        paint: Some(_),
                        ..
                    },
                    ..
                }
            ))
            .count(),
        2
    );
    assert_eq!(fixture.geometry().charts[0].geometry.hits.len(), 2);
}

#[test]
fn callout_paint_updates_and_lifetimes_do_not_rebuild_chart_geometry() {
    use ipp_core::systems::canvas::{
        CanvasPaintEntry, CanvasPrimitive, CanvasPublication, CanvasShapeFill,
    };
    use std::sync::Arc;
    use support::CanvasTestHost;
    let mut fixture = Fixture::new([20.0, 40.0], 1.0);
    let output = fixture.host.canvas_output(
        fixture.host.world_ref(fixture.world).unwrap(),
        [640.0, 480.0],
        100.0,
    );
    let mut bars = PlotBars2d::default();
    bars.series
        .push(PlotSeriesRow {
            x: "position".into(),
            y: "temperature".into(),
            ..Default::default()
        })
        .unwrap();
    bars.labels
        .push(PlotLabelRow {
            row_id: "1".into(),
            text: "Value\nSelected".into(),
            ..Default::default()
        })
        .unwrap();
    for value in [
        C::PlotBars2d(bars),
        C::PlotCalloutStyle(PlotCalloutStyle::default()),
        C::PlotCalloutPaint(PlotCalloutPaint::default()),
    ] {
        fixture.command(Command::insert_value(
            EntityRef::Handle(fixture.entity),
            value,
        ));
    }
    fixture.step(0.0);
    let geometry = fixture.geometry().charts[0].geometry.clone();
    let canvas = |fixture: &Fixture| {
        fixture
            .host
            .output(
                fixture.host.latest_publication(fixture.world).unwrap(),
                output,
            )
            .unwrap()
            .data::<CanvasPublication>()
            .unwrap()
            .clone()
    };
    let before = canvas(&fixture);
    assert_eq!(before.paints.len(), 1);
    let target = before.paints[0].target;
    assert_eq!(target.component, C::PLOT_CALLOUT_PAINT);
    let panels = before.entries.iter().filter(|entry| matches!(entry.as_ref(), CanvasPaintEntry::Primitive { primitive: CanvasPrimitive::Box {fill: CanvasShapeFill::Paint {paint,..},..},..} if *paint == target)).count();
    assert_eq!(panels, 1);
    for value in [0.2, 0.8] {
        fixture.command(Command::SetDynamicProperty {
            entity: EntityRef::Handle(fixture.entity),
            component: C::PLOT_CALLOUT_PAINT,
            name: "light".into(),
            value: V::Vec4([value, 0.2, 3.0, 1.0]),
        });
        fixture.step(0.0);
        let updated = canvas(&fixture);
        assert_eq!(updated.entries, before.entries);
        assert_ne!(updated.paints, before.paints);
        assert!(Arc::ptr_eq(
            &geometry,
            &fixture.geometry().charts[0].geometry
        ));
    }
    fixture.command(Command::RemoveComponent {
        entity: EntityRef::Handle(fixture.entity),
        component: C::PLOT_CALLOUT_PAINT,
    });
    fixture.step(0.0);
    assert!(canvas(&fixture).paints.is_empty());
    assert!(Arc::ptr_eq(
        &geometry,
        &fixture.geometry().charts[0].geometry
    ));
    fixture.command(Command::insert_value(
        EntityRef::Handle(fixture.entity),
        C::PlotCalloutPaint(PlotCalloutPaint::default()),
    ));
    fixture.step(0.0);
    assert_ne!(
        canvas(&fixture).paints[0].target.incarnation,
        target.incarnation
    );
    assert!(Arc::ptr_eq(
        &geometry,
        &fixture.geometry().charts[0].geometry
    ));
}
