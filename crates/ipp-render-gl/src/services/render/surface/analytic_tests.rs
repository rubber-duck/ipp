use super::*;

#[test]
fn unprojection_inverts_perspective_with_nonuniform_placement() {
    let matrix = [
        1.7, 0.0, 0.3, 0.2, 0.0, -2.1, 0.0, 0.0, 0.4, 0.0, -1.05, -1.0, 0.2, -0.3, 2.0, 2.2,
    ];
    let identity = math::multiply(matrix, inverse(matrix).unwrap());
    for (index, value) in identity.into_iter().enumerate() {
        assert!(
            (value
                - if index % 5 == 0 {
                    1.0
                } else {
                    0.0
                })
            .abs()
                < 2e-6
        );
    }
    assert!(inverse([0.0; 16]).is_err());
}

#[test]
fn only_inside_forward_perspective_views_reject_the_near_branch() {
    // Inverse perspective with near 0.1, far 10, translated to the local eye.
    let context = |kind, curvature, eye: [f32; 3]| {
        let mut unproject = [0.0; 16];
        unproject[0] = 1.0;
        unproject[5] = 1.0;
        unproject[10] = 0.0;
        unproject[11] = -4.95;
        unproject[14] = -1.0;
        unproject[15] = 5.05;
        for row in 0..3 {
            unproject[8 + row] += eye[row] * unproject[11];
            unproject[12 + row] += eye[row] * unproject[15];
        }
        AnalyticSurface {
            projection: inverse(unproject).unwrap(),
            inverse: unproject,
            parameters: [kind, curvature, 0.0, 1.0],
        }
    };
    for k in [-0.8, 0.8] {
        for kind in [1.0, 2.0] {
            let inside = context(kind, k, [0.0, 0.0, -1.0 / k]);
            assert!(!near_branch_required(&inside));
            assert!(near_branch_required(&context(kind, k, [0.0, 0.0, 0.0])));
            assert!(near_branch_required(&context(kind, k, [2.0 / k, 0.0, 0.0])));

            let mut orthographic = inside;
            orthographic.inverse[11] = 0.0;
            assert!(near_branch_required(&orthographic));
            let mut reversed = inside;
            reversed.inverse[11] *= -1.0;
            assert!(near_branch_required(&reversed));
            let mut eye_crossing = inside;
            eye_crossing.inverse[3] = 6.0;
            assert!(near_branch_required(&eye_crossing));
        }
        assert!(!near_branch_required(&context(
            1.0,
            k,
            [0.0, 100.0, -1.0 / k]
        )));
        assert!(near_branch_required(&context(
            2.0,
            k,
            [0.0, 100.0, -1.0 / k]
        )));
    }
}
