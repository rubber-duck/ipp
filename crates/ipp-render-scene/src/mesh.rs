//! Mesh input packing from decoded assets.

use ipp_core::MeshAsset;

/// Interleaved mesh input for vertex fetch or device-buffer indexing.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshVertex {
    /// Homogeneous position.
    pub position: [f32; 4],
    /// Linear RGB and texture weight.
    pub color_weight: [f32; 4],
    /// Texture coordinates.
    pub uv: [f32; 4],
    /// Skin palette indices.
    pub joints: [u32; 4],
    /// Skinning weights.
    pub joint_weights: [f32; 4],
    /// Object-space normal.
    pub normal: [f32; 4],
}

/// Pack the available streams with the standard default values.
pub fn mesh_vertices(mesh: &MeshAsset) -> Result<Vec<MeshVertex>, String> {
    let mut vertices = Vec::new();
    vertices
        .try_reserve_exact(mesh.vertex_count())
        .map_err(|error| error.to_string())?;
    for (i, &[x, y, z]) in mesh.positions().iter().enumerate() {
        let [r, g, b] = mesh.colors().map_or([1.0; 3], |colors| colors[i]);
        let weight = mesh
            .texture_weights()
            .map_or(1.0, |weights| f32::from(weights[i]) / 255.0);
        let [u, v] = mesh.uvs().map_or([0.0; 2], |uvs| uvs[i]);
        let [nx, ny, nz] = mesh.normals().map_or([0.0; 3], |normals| normals[i]);
        vertices.push(MeshVertex {
            position: [x, y, z, 1.0],
            color_weight: [r, g, b, weight],
            uv: [u, v, 0.0, 0.0],
            joints: mesh
                .joint_indices()
                .map_or([0; 4], |joints| joints[i].map(u32::from)),
            joint_weights: mesh
                .joint_weights()
                .map_or([1.0, 0.0, 0.0, 0.0], |weights| weights[i]),
            normal: [nx, ny, nz, 0.0],
        });
    }
    Ok(vertices)
}
