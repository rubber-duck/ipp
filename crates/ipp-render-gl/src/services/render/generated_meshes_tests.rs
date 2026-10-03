use super::*;

#[test]
fn derived_streams_decode_into_the_ordinary_mesh_contract() {
    let mesh = PlotMesh {
        positions: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]],
        colors: vec![[0.2, 0.6, 0.9]; 3],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        indices: vec![0, 1, 2],
        ..Default::default()
    };
    let bytes = encode(&mesh).unwrap();
    let decoded = ipp_core::MeshAsset::decode(&bytes).unwrap();
    assert_eq!(decoded.positions(), mesh.positions);
    assert_eq!(decoded.colors().unwrap(), mesh.colors);
    assert_eq!(decoded.normals().unwrap(), mesh.normals);
    assert_eq!(decoded.indices(), mesh.indices);
}

#[test]
fn incomplete_or_out_of_range_streams_fail_before_draw() {
    let mut mesh = PlotMesh {
        positions: vec![[0.0; 3]; 3],
        indices: vec![0, 1, 2],
        ..Default::default()
    };
    mesh.colors = vec![[1.0; 3]; 2];
    assert!(encode(&mesh).is_err());
    mesh.colors.clear();
    mesh.indices = vec![0, 1, 4];
    assert!(ipp_core::MeshAsset::decode(&encode(&mesh).unwrap()).is_err());
}
