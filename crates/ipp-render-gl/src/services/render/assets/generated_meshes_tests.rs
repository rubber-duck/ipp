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

#[test]
fn custom_stream_selection_preserves_default_bytes_and_baked_fallback() {
    let mut mesh = PlotMesh {
        positions: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]],
        colors: vec![[0.1, 0.2, 0.3]; 3],
        indices: vec![0, 1, 2, 2, 1, 0],
        ..Default::default()
    };
    let original = encode(&mesh).unwrap();
    mesh.material = Some(ipp_core::systems::plot::PlotMeshMaterial {
        colors: vec![[0.2, 0.6, 0.9]; 3],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
        data_indices: vec![0, 1, 2],
        decoration_indices: vec![2, 1, 0],
    });
    assert_eq!(encode(&mesh).unwrap(), original);
    let raw =
        ipp_core::MeshAsset::decode(&encode_variant(&mesh, PlotMeshVariant::Data, true).unwrap())
            .unwrap();
    assert_eq!(raw.colors().unwrap(), [[0.2, 0.6, 0.9]; 3]);
    assert_eq!(raw.normals().unwrap(), [[0.0, 0.0, 1.0]; 3]);
    assert_eq!(raw.uvs().unwrap(), [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
    assert_eq!(raw.indices(), [0, 1, 2]);
    let fallback =
        ipp_core::MeshAsset::decode(&encode_variant(&mesh, PlotMeshVariant::Data, false).unwrap())
            .unwrap();
    assert_eq!(fallback.colors().unwrap(), mesh.colors);
    assert!(fallback.normals().is_none());
    let edges = ipp_core::MeshAsset::decode(
        &encode_variant(&mesh, PlotMeshVariant::Decoration, false).unwrap(),
    )
    .unwrap();
    assert_eq!(edges.colors().unwrap(), mesh.colors);
    assert_eq!(edges.indices(), [2, 1, 0]);
}
