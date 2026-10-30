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
