//! Opt-in preparation/layout diagnostics supplement the real native benchmark.
//! Run this exact ignored test alone, in release, without concurrent workloads.
//! Normal builds time work; instrumentation builds count allocation calls/bytes.

use super::*;
use ipp_core::expressions::ExpressionResult;
use ipp_core::services::data::DataRowId;
use ipp_core::systems::data_bindings::DataBindingColumnView;
use ipp_core::systems::plot::*;
use ipp_core::{DynamicPropertyKind, DynamicValue};
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "instrumentation")]
#[global_allocator]
static ALLOCATOR: ipp_core::profiling::CountingAllocator = ipp_core::profiling::CountingAllocator;

const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

fn measure(name: &str, scale: usize, mut work: impl FnMut()) {
    for _ in 0..3 {
        work();
    }
    let allocations = std::env::var("IPP_CHART_PROFILE_MODE").as_deref() == Ok("allocations");
    let iterations = if allocations {
        5
    } else {
        20
    };
    #[cfg(feature = "instrumentation")]
    ipp_core::profiling::reset(allocations);
    #[cfg(not(feature = "instrumentation"))]
    assert!(!allocations, "allocation mode requires instrumentation");
    let mut samples = [0_u64; 20];
    let start = Instant::now();
    for sample in &mut samples[..iterations] {
        let start = Instant::now();
        work();
        *sample = start.elapsed().as_nanos() as u64;
    }
    let elapsed = start.elapsed().as_nanos();
    #[cfg(feature = "instrumentation")]
    let counts = {
        ipp_core::profiling::pause();
        ipp_core::profiling::allocations()
    };
    #[cfg(not(feature = "instrumentation"))]
    let counts = (0_u64, 0_u64);
    samples[..iterations].sort_unstable();
    // Allocation instrumentation is count-only evidence. Normal timing uses
    // nearest-rank percentiles, matching the maintained native/data harness.
    let percentile = |percent: usize| {
        if allocations {
            "null".to_owned()
        } else {
            samples[(iterations * percent).div_ceil(100) - 1].to_string()
        }
    };
    let p50 = percentile(50);
    let p95 = percentile(95);
    println!(
        "{{\"case\":\"{name}\",\"scale\":{scale},\"mode\":\"{}\",\"iterations\":{iterations},\"elapsedNs\":{elapsed},\"p50Ns\":{p50},\"p95Ns\":{p95},\"allocations\":{},\"requestedBytes\":{}}}",
        if allocations {
            "allocations"
        } else {
            "timing"
        },
        counts.0,
        counts.1,
    );
}

fn layout(count: usize, radial: bool) {
    let mut host = ipp_core::HostRuntime::new();
    let id = host.create_world(Default::default(), &[]).unwrap();
    host.frame(0.0).unwrap();
    let world = host.world_ref(id).unwrap();
    let publication = host.latest_publication(id).unwrap();
    let entity = ipp_core::EntityId::from_bits((1 << 32) | 1);
    let target = CanvasTarget {
        entity,
        component: ipp_core::ComponentValue::PLOT_PIE3D,
        incarnation: 1,
    };
    let source = Arc::new(PlotPreparedGeometry::default());
    let mut retained = Vec::new();
    for index in 0..count {
        let angle = index as f32 / count as f32 * std::f32::consts::TAU;
        let mut model = IDENTITY;
        model[5] = -1.0;
        model[12] = angle.cos() * 0.35;
        model[13] = angle.sin() * 0.35;
        let panel = (index * 2) as u32;
        retained.push(PlotPublishedPlane {
            part: panel,
            model,
            facing: PlotPlaneFacing::Camera,
            layout: PlotPlaneLayout::Callout,
            placement: if radial {
                PlotPlanePlacement::Radial {
                    center: [0.0; 3],
                    rim: [angle.cos() * 0.6, angle.sin() * 0.6, 0.0],
                    spacing: 0.0,
                }
            } else {
                PlotPlanePlacement::Fixed
            },
            bounds: Some([-0.07, -0.025, 0.07, 0.025]),
            clip: [-1.0, -1.0, 1.0, 1.0],
            primitives: Arc::from([]),
        });
        retained.push(PlotPublishedPlane {
            part: panel + 1,
            layout: PlotPlaneLayout::Connector {
                panel,
                endpoint: [0.0; 2],
                width: 0.001,
            },
            bounds: None,
            ..retained.last().unwrap().clone()
        });
    }
    let planes: Vec<_> = retained
        .iter()
        .map(|plane| ScenePlotPlane {
            entity: crate::services::render::outputs::scene::RenderEntity {
                world,
                entity,
                incarnation: 1,
            },
            target,
            publication,
            model: plane.model,
            chart_model: IDENTITY,
            geometry: &source,
            plane,
        })
        .collect();
    let viewport = WorldViewport {
        width: 732,
        height: 348,
        device_pixel_ratio: 1.0,
    };
    let mut state = PlotLabelLayoutState::default();
    let case = if radial {
        "layout_radial_idle"
    } else {
        "layout_callout_idle"
    };
    measure(case, count, || {
        let arranged = state.arrange(&planes, IDENTITY, viewport);
        assert_eq!(arranged.len(), count * 2);
        black_box(arranged);
    });
    let mut phase = 0.0_f32;
    measure(
        if radial {
            "layout_radial_camera"
        } else {
            "layout_callout_camera"
        },
        count,
        || {
            phase += 0.0001;
            let mut projection = IDENTITY;
            projection[12] = phase.sin() * 0.01;
            black_box(state.arrange(&planes, projection, viewport));
        },
    );
}

fn preparation(count: usize) {
    let side = (count as f32).sqrt().ceil() as usize;
    let rows: Vec<_> = (0..count).map(|i| DataRowId(i as u64 + 1)).collect();
    let numbers = |axis| {
        (0..count)
            .map(|i| {
                ExpressionResult::Valid(DynamicValue::F32(match axis {
                    0 => (i % side) as f32,
                    1 => 1.0 + (i as f32 * 0.1).sin(),
                    _ => (i / side) as f32,
                }))
            })
            .collect::<Vec<_>>()
    };
    let x = numbers(0);
    let y = numbers(1);
    let z = numbers(2);
    let input = PlotPreparedInput {
        row_ids: &rows,
        columns: [("x", &x), ("y", &y), ("z", &z), ("value", &y)]
            .into_iter()
            .map(|(name, values)| DataBindingColumnView {
                name,
                kind: DynamicPropertyKind::F32,
                values,
            })
            .collect(),
    };
    let frame2 = PlotFrame2d::default();
    let frame3 = PlotFrame3d::default();
    let mut line = PlotLine2d::default();
    line.series.insert(0, PlotSeriesRow::default()).unwrap();
    let mut bars2 = PlotBars2d::default();
    bars2.series.insert(0, PlotSeriesRow::default()).unwrap();
    let mut bars3 = PlotGridBars3d::default();
    bars3.series.insert(0, PlotSeriesRow::default()).unwrap();
    let mut points = PlotPoints3d::default();
    points.series.insert(0, PlotSeriesRow::default()).unwrap();
    measure("prepare_line", count, || {
        black_box(plots_2d::prepare_line(&line, &frame2, &input).unwrap());
    });
    line.interpolation = 1;
    measure("prepare_smooth", count, || {
        black_box(plots_2d::prepare_line(&line, &frame2, &input).unwrap());
    });
    measure("prepare_bars2", count, || {
        black_box(plots_2d::prepare_bars(&bars2, &frame2, &input).unwrap());
    });
    measure("prepare_bars3", count, || {
        let output = plots_3d::prepare_bars(&bars3, &frame3, &input).unwrap();
        assert_eq!(output.hits.len(), count);
        black_box(output);
    });
    measure("prepare_points", count, || {
        let output = plots_3d::prepare_points(&points, &frame3, &input).unwrap();
        assert_eq!(output.hits.len(), count);
        black_box(output);
    });
}

fn surface(count: usize, irregular: bool) {
    let side = (count as f32).sqrt() as usize;
    let rows: Vec<_> = (0..count).map(|i| DataRowId(i as u64 + 1)).collect();
    let column = |axis| {
        (0..count)
            .map(|i| {
                ExpressionResult::Valid(DynamicValue::F32(match axis {
                    0 => {
                        (i % side) as f32
                            + if irregular {
                                (i as f32 * 1.7).sin() * 0.1
                            } else {
                                0.0
                            }
                    }
                    1 => 1.0 + (i as f32 * 0.1).sin(),
                    _ => (i / side) as f32,
                }))
            })
            .collect::<Vec<_>>()
    };
    let x = column(0);
    let y = column(1);
    let z = column(2);
    let input = PlotPreparedInput {
        row_ids: &rows,
        columns: [("x", &x), ("y", &y), ("z", &z)]
            .into_iter()
            .map(|(name, values)| DataBindingColumnView {
                name,
                kind: DynamicPropertyKind::F32,
                values,
            })
            .collect(),
    };
    let mut chart = PlotHeightSurface3d::default();
    chart.series.insert(0, PlotSeriesRow::default()).unwrap();
    measure(
        if irregular {
            "prepare_surface_irregular"
        } else {
            "prepare_surface_grid"
        },
        count,
        || {
            let output =
                plots_3d::prepare_surface(&chart, &PlotFrame3d::default(), &input).unwrap();
            assert_eq!(output.hits.len(), count);
            black_box(output);
        },
    );
}

#[test]
#[ignore = "opt-in diagnostic; serialize release timing and allocation runs"]
fn chart_profile() {
    for count in [4, 32, 128, 512] {
        layout(count, false);
        layout(count, true);
    }
    for count in [1000, 10000] {
        preparation(count);
    }
    for count in [100, 1024] {
        surface(count, false);
    }
    for count in [128, 512] {
        surface(count, true);
    }
}

/// CPU-only packed-band inspection. Never draws the known failing100kGPU case.
#[test]
#[ignore = "opt-in bounded dense-path diagnostics; no GPU work"]
fn chart_path_scaling() {
    for count in [1000, 10000] {
        let rows: Vec<_> = (0..count).map(|i| DataRowId(i as u64 + 1)).collect();
        let x: Vec<_> = (0..count)
            .map(|i| ExpressionResult::Valid(DynamicValue::F32(i as f32 / (count - 1) as f32)))
            .collect();
        let y: Vec<_> = (0..count)
            .map(|i| {
                let x = i as f32 / (count - 1) as f32;
                ExpressionResult::Valid(DynamicValue::F32(
                    0.5 + 0.3 * (x * 16.0 + ((i * 17) % 101) as f32 / 100.0 * 3.0).sin(),
                ))
            })
            .collect();
        let input = PlotPreparedInput {
            row_ids: &rows,
            columns: [("x", &x), ("y", &y)]
                .into_iter()
                .map(|(name, values)| DataBindingColumnView {
                    name,
                    kind: DynamicPropertyKind::F32,
                    values,
                })
                .collect(),
        };
        let mut chart = PlotLine2d {
            marker_size: 0.0,
            ..PlotLine2d::default()
        };
        chart.series.insert(0, PlotSeriesRow::default()).unwrap();
        let output = plots_2d::prepare_line(
            &chart,
            &PlotFrame2d {
                width: 624.0,
                height: 384.0,
                min_x: 0.0,
                max_x: 1.0,
                min_y: 0.0,
                max_y: 2.0,
                automatic_x: false,
                automatic_y: false,
                ..PlotFrame2d::default()
            },
            &input,
        )
        .unwrap();
        let paths: Vec<_> = output
            .canvas
            .iter()
            .filter_map(|p| match &p.kind {
                PlotPrimitiveKind::Path(path) if path.contours.len() > count => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(paths.len(), 1);
        let atlas =
            crate::pack_surface_paths(paths.iter().map(|p| (p.bounds, p.contours.as_ref())));
        let band = |i| match &atlas.texels.bands {
            crate::SurfaceBandTexels::Uint16(v) => u32::from(v[i]),
            crate::SurfaceBandTexels::Uint32(v) => v[i],
        };
        let horizontal = (0..16).map(|i| band(i * 2 + 1)).max().unwrap();
        let vertical = (0..16).map(|i| band(32 + i * 2 + 1)).max().unwrap();
        println!(
            "{{\"diagnostic\":\"dense_path_bands\",\"scale\":{count},\"contours\":{},\"curveTexels\":{},\"maxHorizontalCurves\":{horizontal},\"maxVerticalCurves\":{vertical},\"atlasBytes\":{}}}",
            paths[0].contours.len(),
            atlas.texels.curves.len(),
            atlas.texels.byte_len()
        );
    }
}
