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
