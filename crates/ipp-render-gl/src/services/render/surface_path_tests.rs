use super::*;

fn contour(start: [f32; 2], segments: Vec<QuadraticSegment>) -> QuadraticContour {
    QuadraticContour {
        start,
        segments,
    }
}

fn line(to: [f32; 2]) -> QuadraticSegment {
    QuadraticSegment::Line {
        to,
    }
}

fn quadratic(control: [f32; 2], to: [f32; 2]) -> QuadraticSegment {
    QuadraticSegment::Quadratic {
        control,
        to,
    }
}

fn int16(atlas: &SurfacePathAtlas) -> &[[i16; 4]] {
    match &atlas.texels.curves {
        SurfaceCurveTexels::Int16(texels) => texels,
        SurfaceCurveTexels::Int32(_) => panic!("expected 16-bit curve texels"),
    }
}

fn bands(atlas: &SurfacePathAtlas) -> Vec<u32> {
    match &atlas.texels.bands {
        SurfaceBandTexels::Uint16(texels) => texels.iter().copied().map(u32::from).collect(),
        SurfaceBandTexels::Uint32(texels) => texels.clone(),
    }
}

#[test]
fn dense_atlas_respects_device_limit_without_losing_paths_or_curves() {
    use ipp_core::expressions::ExpressionResult;
    use ipp_core::services::data::DataRowId;
    use ipp_core::systems::data_bindings::DataBindingColumnView;
    use ipp_core::systems::plot::*;
    use ipp_core::{DynamicPropertyKind, DynamicValue};

    let rows: Vec<_> = (1..=1000).map(DataRowId).collect();
    let x: Vec<_> = (0..1000)
        .map(|i| ExpressionResult::Valid(DynamicValue::F32(i as f32 / 999.0)))
        .collect();
    let y: Vec<_> = (0..1000)
        .map(|i| {
            let x = i as f32 / 999.0;
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
            PlotPrimitiveKind::Path(path) => Some((path.bounds, path.contours.as_ref())),
            _ => None,
        })
        .collect();
    assert!(
        paths.len() > 1,
        "exercise subsequent descriptor offsets too"
    );
    let tiled = pack_surface_paths_with_limit(paths.iter().copied(), 1024);
    let fallback = pack_surface_paths_with_limit(paths.iter().copied(), 256);
    let unknown = pack_surface_paths_with_limit(paths.iter().copied(), 0);
    let original = pack_surface_paths_inner(paths.iter().copied(), false);
    assert!(tiled.texels.bands.len() > 256 * 256);
    assert!(fallback.texels.bands.len() <= 256 * 256);
    assert_eq!(tiled.texels.curves, fallback.texels.curves);
    assert_eq!(fallback.texels.curves, original.texels.curves);
    assert_eq!(tiled.texels.curve_scale, fallback.texels.curve_scale);
    assert_eq!(fallback.texels.curve_scale, original.texels.curve_scale);
    assert_eq!(bands(&fallback), bands(&original));
    assert_eq!(bands(&unknown), bands(&original));
    assert_eq!(fallback.descriptors, original.descriptors);
    assert_eq!(unknown.descriptors, original.descriptors);
    assert_eq!(fallback.descriptors.len(), paths.len());
    assert!(
        tiled
            .descriptors
            .iter()
            .any(|d| bands(&tiled)[d.band_offset as usize + TILE_METADATA] != 0)
    );
    for descriptor in &fallback.descriptors {
        assert_eq!(
            bands(&fallback)[descriptor.band_offset as usize + TILE_METADATA],
            0
        );
    }
}

#[test]
fn tiled_closed_contours_preserve_two_axis_winding_and_edge_weights() {
    let mut contours = Vec::new();
    for i in 0..1700 {
        let x = (i % 64) as f32;
        let y = (i / 64) as f32 * 2.0;
        contours.push(contour(
            [x, y],
            vec![
                line([x + 1.25, y]),
                line([x + 1.25, y + 2.25]),
                line([x, y + 2.25]),
            ],
        ));
    }
    // A spanning outside contour and a reversed hole must retain their winding
    // through all tiles, including where neither has a local visible edge.
    contours.push(contour(
        [-2.0, -2.0],
        vec![line([66.0, -2.0]), line([66.0, 66.0]), line([-2.0, 66.0])],
    ));
    contours.push(contour(
        [20.0, 20.0],
        vec![line([20.0, 40.0]), line([40.0, 40.0]), line([40.0, 20.0])],
    ));
    let bounds = [-2.0, -2.0, 66.0, 66.0];
    let atlas = pack_surface_paths([(bounds, contours.as_slice())]);
    let mut points = Vec::new();
    push_contour_texels(&contours, &mut points);
    let extents = curve_extents(&points, &atlas.texels.curves, atlas.texels.curve_scale);
    // Exercise the conservative lookup independently of the optional atlas
    // storage heuristic, which can choose bands for short rectangle contours.
    let tiles = fill_tile_lists(&extents, bounds, usize::MAX).unwrap();
    let curves = int16(&atlas);
    let scale = atlas.texels.curve_scale;
    let all: Vec<u32> = curves
        .iter()
        .enumerate()
        .filter_map(|(i, p)| (i + 1 < curves.len() && p[..2] != p[2..]).then_some(i as u32))
        .collect();
    // Independent oriented segment-ray integration, including the finite pixel
    // footprint and nearest-edge weight used by analytic antialiasing.
    let integrate = |indices: &[u32], point: [f32; 2], footprint: f32, axis: usize| {
        let mut winding = 0.0_f32;
        let mut weight = 0.0_f32;
        let ray = 1 - axis;
        for &index in indices {
            let index = index as usize;
            let start = [
                f32::from(curves[index][0]) * scale,
                f32::from(curves[index][1]) * scale,
            ];
            let end = [
                f32::from(curves[index + 1][0]) * scale,
                f32::from(curves[index + 1][1]) * scale,
            ];
            let sign = if start[ray] <= point[ray] && end[ray] > point[ray] {
                1.0
            } else if end[ray] <= point[ray] && start[ray] > point[ray] {
                -1.0
            } else {
                continue;
            };
            let t = (point[ray] - start[ray]) / (end[ray] - start[ray]);
            let crossing = (start[axis] + (end[axis] - start[axis]) * t - point[axis]) / footprint;
            winding += sign * (crossing + 0.5).clamp(0.0, 1.0);
            weight = weight.max((1.0 - crossing.abs() * 2.0).clamp(0.0, 1.0));
        }
        (winding, weight)
    };
    let cell = (bounds[2] - bounds[0]) / TILE_COUNT as f32;
    for y in [0, 1, 20, 40, 63] {
        for x in [0, 1, 20, 40, 63] {
            for delta in [-0.001, 0.0, 0.001, 0.5, 0.999] {
                let point = [
                    bounds[0] + cell * (x as f32 + delta),
                    bounds[1] + cell * (y as f32 + delta),
                ];
                let tile = point.map(|p| {
                    (((p - bounds[0]) / cell).floor() as i32).clamp(0, TILE_COUNT as i32 - 1)
                        as usize
                });
                for (axis, list) in tiles[tile[1] * TILE_COUNT + tile[0]].iter().enumerate() {
                    assert!(list.windows(2).all(|pair| pair[0] < pair[1]));
                    for footprint in [cell / 16.0, cell / 2.0, cell] {
                        let expected = integrate(&all, point, footprint, axis);
                        let actual = integrate(list, point, footprint, axis);
                        assert!(
                            (actual.0 - expected.0).abs() < 0.00001,
                            "winding: {point:?}/{axis}: {actual:?} != {expected:?}"
                        );
                        assert_eq!(actual.1, expected.1, "edge weight: {point:?}/{axis}");
                    }
                }
            }
        }
    }
}

#[test]
fn dense_tiles_are_optional_and_storage_bounded() {
    let contour = contour([0.0, 0.0], vec![quadratic([0.5, 1.0], [1.0, 0.0])]);
    let contours = vec![contour; 3000];
    let mut points = Vec::new();
    push_contour_texels(&contours, &mut points);
    let (curves, scale) = quantize(&points);
    let extents = curve_extents(&points, &curves, scale);
    assert!(
        fill_tile_lists(&extents, [0.0, 0.0, 1.0, 1.0], 100000).is_none(),
        "spanning contours must fall back rather than grow lookup storage without a bound"
    );
    assert!(fill_tile_lists(&extents, [0.0; 4], usize::MAX).is_none());
    let atlas = pack_surface_paths([([0.0, 0.0, 1.0, 1.0], contours.as_slice())]);
    assert_eq!(bands(&atlas)[TILE_METADATA], 0);
    assert!(
        !atlas.texels.curves.is_empty(),
        "fallback retains all contours"
    );
}

#[test]
fn contours_share_endpoints_and_end_with_a_terminator() {
    let contours = [contour(
        [-1.0, 2.0],
        vec![line([3.0, 4.0]), quadratic([5.0, 6.0], [-1.0, 2.0])],
    )];
    let atlas = pack_surface_paths([([-1.0, 2.0, 5.0, 6.0], contours.as_slice())]);

    assert_eq!(atlas.texels.curve_scale, 1.0);
    assert_eq!(
        int16(&atlas),
        [[-1, 2, 3, 4], [3, 4, 5, 6], [-1, 2, -1, 2]],
        "a line stores its end as the control; the terminator holds the closing point"
    );
    assert_eq!(atlas.descriptors[0], SurfacePathDescriptor::new([0, 3], 0));
}

#[test]
fn open_contours_close_with_a_line_before_the_terminator() {
    let atlas = pack_surface_paths([(
        [0.0, 0.0, 2.0, 1.0],
        [contour([0.0, 0.0], vec![line([2.0, 0.0])])].as_slice(),
    )]);

    assert_eq!(int16(&atlas), [[0, 0, 2, 0], [2, 0, 0, 0], [0, 0, 0, 0]]);
}

#[test]
fn half_unit_font_coordinates_stay_exact_in_sixteen_bits() {
    let atlas = pack_surface_paths([(
        [0.0, 0.0, 1000.0, 1000.0],
        [contour(
            [0.5, -999.5],
            vec![quadratic([1000.0, 12.5], [-16.0, 0.0])],
        )]
        .as_slice(),
    )]);

    assert_eq!(atlas.texels.curve_scale, 0.5);
    let texel = int16(&atlas)[0];
    let restored = texel.map(|value| f32::from(value) * atlas.texels.curve_scale);
    assert_eq!(restored, [0.5, -999.5, 1000.0, 12.5]);
}

#[test]
fn large_or_fine_coordinates_widen_to_exact_thirty_two_bit_texels() {
    let atlas = pack_surface_paths([(
        [0.0, 0.0, 40000.0, 1.0],
        [contour(
            [0.0, 0.0],
            vec![line([40000.0, 0.0]), line([0.125, 1.0])],
        )]
        .as_slice(),
    )]);

    let SurfaceCurveTexels::Int32(texels) = &atlas.texels.curves else {
        panic!("coordinates beyond 16-bit fixed point need 32-bit texels");
    };
    let restored: Vec<f32> = texels
        .iter()
        .flatten()
        .map(|&value| value as f32 * atlas.texels.curve_scale)
        .collect();
    assert!(restored.contains(&40000.0));
    assert!(restored.contains(&0.125));
}

#[test]
fn bands_use_relative_offsets_and_curve_indices() {
    let square = [contour(
        [0.0, 0.0],
        vec![
            line([1.0, 0.0]),
            line([1.0, 1.0]),
            line([0.0, 1.0]),
            line([0.0, 0.0]),
        ],
    )];
    let bottom = [contour([0.0, 0.0], vec![line([1.0, 0.0])])];
    let atlas = pack_surface_paths([
        ([0.0, 0.0, 1.0, 1.0], square.as_slice()),
        ([0.0, 0.0, 1.0, 1.0], bottom.as_slice()),
    ]);
    let bands = bands(&atlas);

    let second = atlas.descriptors[1];
    assert_eq!(second.curve_range, [5, 3]);
    let base = second.band_offset as usize;
    let header = |band: usize| (bands[base + band * 2], bands[base + band * 2 + 1]);

    let (offset, count) = header(0);
    assert_eq!(
        offset, BAND_HEADER_TEXELS,
        "lists follow the path's headers"
    );
    let list = &bands[base + offset as usize..][..count as usize];
    assert_eq!(
        list,
        [0, 1],
        "indices are relative to the path's first curve texel"
    );
    assert!(list.iter().all(|&curve| curve < second.curve_range[1]));

    assert_eq!(
        header(BAND_COUNT - 1).1,
        0,
        "the top band misses the bottom edge"
    );
}

#[test]
fn band_texels_widen_only_when_a_value_exceeds_sixteen_bits() {
    let narrow = pack_surface_paths([(
        [0.0, 0.0, 1.0, 1.0],
        [contour([0.0, 0.0], vec![line([1.0, 1.0])])].as_slice(),
    )]);
    assert!(matches!(narrow.texels.bands, SurfaceBandTexels::Uint16(_)));

    let segments = (0..70_000)
        .map(|index| line([index as f32 % 2.0, 1.0]))
        .collect();
    let contours = [contour([0.0, 0.0], segments)];
    let wide = pack_surface_paths([([0.0, 0.0, 1.0, 1.0], contours.as_slice())]);
    assert!(matches!(wide.texels.bands, SurfaceBandTexels::Uint32(_)));
}

#[test]
fn fraction_bits_cover_exact_binary_fractions() {
    assert_eq!(f32_fraction_bits(12.0), 0);
    assert_eq!(f32_fraction_bits(-0.5), 1);
    assert_eq!(f32_fraction_bits(3.375), 3);
    assert_eq!(f32_fraction_bits(f32::MIN_POSITIVE), 126);
}

#[test]
fn async_packing_matches_exact_curve_and_band_formats_across_cooperative_yields() {
    use std::{
        future::Future,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        task::{Context, Poll, Wake, Waker},
    };

    struct Ready(AtomicBool);

    impl Wake for Ready {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    for scale in [0.5, 1.0, 32768.0] {
        let contours: Vec<_> = (0..1100)
            .map(|_| {
                contour(
                    [0., 0.],
                    vec![quadratic([scale, scale / 2.], [scale, 0.]), line([0., 0.])],
                )
            })
            .collect();
        let paths = [([0., 0., scale, scale], contours.as_slice())];
        let expected = pack_surface_paths(paths);
        let ready = Arc::new(Ready(AtomicBool::new(false)));
        let waker = Waker::from(ready.clone());
        let mut cx = Context::from_waker(&waker);
        let mut future = std::pin::pin!(pack_surface_paths_async(paths));
        let actual = loop {
            ready.0.store(false, Ordering::SeqCst);
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(result) => break result,
                Poll::Pending => assert!(ready.0.load(Ordering::SeqCst)),
            }
        };
        assert_eq!(actual.texels, expected.texels);
        assert_eq!(actual.descriptors, expected.descriptors);
        for descriptor in &actual.descriptors {
            assert_eq!(
                bands(&actual)[descriptor.band_offset as usize + TILE_METADATA],
                0
            );
        }
    }
}
