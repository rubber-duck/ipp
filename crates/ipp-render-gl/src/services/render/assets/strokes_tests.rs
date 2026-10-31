use super::*;

fn bands(atlas: &SurfacePathAtlas) -> Vec<u32> {
    match &atlas.texels.bands {
        SurfaceBandTexels::Uint16(v) => v.iter().copied().map(u32::from).collect(),
        SurfaceBandTexels::Uint32(v) => v.clone(),
    }
}

#[test]
fn compact_endpoints_are_exact_and_lookup_preserves_every_pixel_candidate() {
    let segments: Vec<_> = (0..599)
        .map(|i| {
            let x = i as f32;
            [
                x,
                (x * 0.7).sin() * 30.0,
                x + 1.0,
                ((x + 1.0) * 0.7).sin() * 30.0,
            ]
        })
        .collect();
    let bounds = [-0.75, -30.75, 599.75, 30.75];
    let atlas = pack_stroke(bounds, &segments, 0.75, 4096);
    let SurfaceCurveTexels::Int32(points) = &atlas.texels.curves else {
        panic!("stroke bit patterns");
    };
    assert_eq!(points.len(), segments.len() + 1);
    for (packed, source) in points[1..].iter().zip(&segments) {
        assert_eq!(packed.map(|v| f32::from_bits(v as u32)), *source);
    }
    let lookup = bands(&atlas);
    assert_eq!(lookup[0], 0, "ordered time-series should index X");
    assert!(lookup.len() <= segments.len() * 32 + BAND_HEADER_TEXELS as usize);
    for level in 0..lookup[1] as usize {
        let bins = lookup[2 + level * 2] as usize;
        let header = lookup[3 + level * 2] as usize;
        let width = (f64::from(bounds[2]) - f64::from(bounds[0])) / bins as f64;
        for bin in 0..bins {
            let first = lookup[header + bin * 2] as usize;
            let count = lookup[header + bin * 2 + 1] as usize;
            let candidates = &lookup[first..first + count];
            for fraction in [0.0, 0.5, 1.0] {
                let x = f64::from(bounds[0]) + (bin as f64 + fraction) * width;
                for (i, s) in segments.iter().enumerate() {
                    if f64::from(s[0].min(s[2])) - 0.75 <= x + width * 0.5
                        && f64::from(s[0].max(s[2])) + 0.75 >= x - width * 0.5
                    {
                        assert!(
                            candidates.contains(&(i as u32 + 1)),
                            "missing segment {i} at level {level}, bin {bin}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn spanning_strokes_and_small_devices_fall_back_without_dropping_segments() {
    let segments = vec![[0.0, 0.0, 100.0, 100.0]; 1000];
    for limit in [0, 64, 4096] {
        let atlas = pack_stroke([-1.0, -1.0, 101.0, 101.0], &segments, 1.0, limit);
        let lookup = bands(&atlas);
        assert_eq!(atlas.texels.curves.len(), 1001);
        let final_level = lookup[1] as usize - 1;
        let header = lookup[3 + final_level * 2] as usize;
        assert_eq!(lookup[2 + final_level * 2], 1);
        assert_eq!(lookup[header + 1], 1000);
        assert!(lookup.len() <= 32 * segments.len() + BAND_HEADER_TEXELS as usize);
    }
}
