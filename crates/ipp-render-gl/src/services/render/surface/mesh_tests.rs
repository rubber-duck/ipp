use super::*;
use ipp_core::{CylinderSurface, SphereSurface};

fn assert_geometry(
    mesh: &SurfaceMeshData,
    width: f64,
    height: f64,
    k: f64,
    offset: f64,
    sphere: bool,
) {
    let uvs = mesh.asset.uvs().unwrap();
    for (point, uv) in mesh.asset.positions().iter().zip(uvs) {
        let x = f64::from(uv[0]) * width - width * 0.5;
        let y = height * 0.5 - f64::from(uv[1]) * height;
        let radius = if sphere {
            x.hypot(y)
        } else {
            x.abs()
        };
        let angle = k * radius;
        let sinc = if angle == 0.0 {
            1.0
        } else {
            angle.sin() / angle
        };
        let expected = if sphere {
            [
                x * sinc * (1.0 + k * offset),
                y * sinc * (1.0 + k * offset),
                (angle.cos() - 1.0) / k + offset * angle.cos(),
            ]
        } else {
            [
                (k * x).sin() * (1.0 / k + offset),
                y,
                ((k * x).cos() - 1.0) / k + offset * (k * x).cos(),
            ]
        };
        for (actual, expected) in point.iter().zip(expected) {
            assert!((f64::from(*actual) - expected).abs() < 2e-6);
        }
    }
    assert!(mesh.asset.positions().len() <= (MAX_CELLS + 1).pow(2));
    assert!(mesh.patches.len() <= 256);
    assert_eq!(mesh.patches.first().unwrap().indices.start, 0);
    assert_eq!(
        mesh.patches.last().unwrap().indices.end as usize,
        mesh.asset.indices().len()
    );
    assert!(
        mesh.patches
            .windows(2)
            .all(|p| p[0].indices.end == p[1].indices.start)
    );
}

#[test]
fn moderate_sphere_reduces_image_quality_at_the_subdivision_cap() {
    let shape = SphereSurface {
        width: 4.0,
        height: 3.0,
        curvature: 0.5,
        layer_spacing: 0.0,
    };
    assert!(tessellate(&shape, 0.0, [1024, 768]).is_err());
    let (size, meshes) = select_quality(&shape, &[0.0, 0.2], [1024, 768]).unwrap();
    assert!(size[0] < 1024 && size[0] >= 128);
    assert_geometry(&meshes[0], 4.0, 3.0, 0.5, 0.0, true);
    assert_geometry(&meshes[1], 4.0, 3.0, 0.5, 0.2, true);
    // Independently compare actual interpolated triangle centroids with the
    // analytic sphere; the selected half-texel bound includes shell offsets.
    let tolerance = (4.0 / f64::from(size[0])).min(3.0 / f64::from(size[1])) * 0.5;
    for (offset, mesh) in [0.0, 0.2].into_iter().zip(meshes) {
        for ids in mesh.asset.indices().as_chunks::<3>().0.iter().step_by(37) {
            let mut uv = [0.0; 2];
            let mut interpolated = [0.0; 3];
            for id in ids {
                for (axis, value) in uv.iter_mut().enumerate() {
                    *value += f64::from(mesh.asset.uvs().unwrap()[*id as usize][axis]) / 3.0;
                }
                for (axis, value) in interpolated.iter_mut().enumerate() {
                    *value += f64::from(mesh.asset.positions()[*id as usize][axis]) / 3.0;
                }
            }
            let x = uv[0] * 4.0 - 2.0;
            let y = 1.5 - uv[1] * 3.0;
            let r = x.hypot(y);
            let a = 0.5 * r;
            let s = if a == 0.0 {
                1.0
            } else {
                a.sin() / a
            };
            let exact = [
                x * s * (1.0 + 0.5 * offset),
                y * s * (1.0 + 0.5 * offset),
                (a.cos() - 1.0) / 0.5 + offset * a.cos(),
            ];
            let error = (0..3)
                .map(|i| (interpolated[i] - exact[i]).powi(2))
                .sum::<f64>()
                .sqrt();
            assert!(error <= tolerance + 1e-6, "error {error} > {tolerance}");
        }
    }
}

#[test]
fn strong_signed_curvature_keeps_geometry_and_bounds_work() {
    for k in [-1.2, 1.2] {
        let sphere = SphereSurface {
            width: 3.0,
            height: 2.0,
            curvature: k as f32,
            layer_spacing: 0.1,
        };
        let cylinder = CylinderSurface {
            width: 3.0,
            height: 2.0,
            curvature: k as f32,
            layer_spacing: 0.1,
        };
        for (shape, is_sphere) in [
            (&sphere as &dyn Surface, true),
            (&cylinder as &dyn Surface, false),
        ] {
            let (size, meshes) = select_quality(shape, &[0.1], [2048, 1365]).unwrap();
            assert!(size[0] > 1);
            assert_geometry(&meshes[0], 3.0, 2.0, f64::from(k as f32), 0.1, is_sphere);
        }
    }
}

#[test]
fn invalid_shell_is_unavailable_even_at_minimum_quality() {
    let shape = SphereSurface {
        curvature: -1.0,
        ..Default::default()
    };
    assert!(select_quality(&shape, &[1.0], [2048, 2048]).is_err());
}
