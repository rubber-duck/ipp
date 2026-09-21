//! Mesh information required after graphics upload, without retained vertex streams.

use super::mesh::MeshAsset;

/// CPU geometry/compatibility facts shared by headless and graphics consumers.
#[derive(Debug, Default)]
pub struct MeshMetadata {
    bounds: [[f32; 3]; 2],
    vertices: usize,
    indices: usize,
    attributes: u32,
    #[cfg(feature = "mesh-poses")]
    topology: Vec<u16>,
    #[cfg(feature = "skeletal-animation")]
    joint_bounds: Vec<Option<[[f32; 3]; 2]>>,
    #[cfg(feature = "skeletal-animation")]
    maximum_joint: Option<u8>,
}

impl MeshMetadata {
    pub(super) fn from_mesh(mesh: &MeshAsset) -> Self {
        let (min, max) = mesh.bounds();
        Self {
            bounds: [min, max],
            vertices: mesh.vertex_count(),
            indices: mesh.indices().len(),
            attributes: u32::from(mesh.colors().is_some())
                | (u32::from(mesh.uvs().is_some()) << 1)
                | (u32::from(mesh.normals().is_some()) << 2)
                | (u32::from(mesh.texture_weights().is_some()) << 3),
            #[cfg(feature = "mesh-poses")]
            topology: mesh.indices().to_vec(),
            #[cfg(feature = "skeletal-animation")]
            joint_bounds: mesh.joint_bounds().to_vec(),
            #[cfg(feature = "skeletal-animation")]
            maximum_joint: mesh.max_joint_index(),
        }
    }

    /// Move compact facts out after the loader has consumed all upload streams.
    pub fn from_owned_mesh(mesh: MeshAsset) -> Self {
        mesh.into_metadata()
    }

    /// Conservative bind-space enclosure.
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        (self.bounds[0], self.bounds[1])
    }

    /// Authored vertex count.
    pub fn vertex_count(&self) -> usize {
        self.vertices
    }

    /// Authored triangle-index count.
    pub fn index_count(&self) -> usize {
        self.indices
    }

    /// Authored color/UV/normal/texture-weight stream mask.
    pub fn attributes(&self) -> u32 {
        self.attributes
    }

    /// Whether authored normals are available.
    pub fn has_normals(&self) -> bool {
        self.attributes & 4 != 0
    }

    /// Whether authored texture coordinates are available.
    pub fn has_uvs(&self) -> bool {
        self.attributes & 2 != 0
    }

    /// Whether authored texture weights are available.
    pub fn has_texture_weights(&self) -> bool {
        self.attributes & 8 != 0
    }

    #[cfg(feature = "mesh-poses")]
    /// Exact ordered topology for pose compatibility.
    pub fn topology(&self) -> &[u16] {
        &self.topology
    }

    #[cfg(feature = "skeletal-animation")]
    /// Conservative bind-space joint enclosures.
    pub fn joint_bounds(&self) -> &[Option<[[f32; 3]; 2]>] {
        &self.joint_bounds
    }

    #[cfg(feature = "skeletal-animation")]
    /// Highest palette index referenced by any vertex.
    pub fn max_joint_index(&self) -> Option<u8> {
        self.maximum_joint
    }

    /// Retained CPU metadata allocation estimate.
    pub fn resident_bytes(&self) -> usize {
        let bytes = std::mem::size_of::<Self>();
        #[cfg(feature = "mesh-poses")]
        let bytes = bytes + std::mem::size_of_val(self.topology.as_slice());
        #[cfg(feature = "skeletal-animation")]
        let bytes = bytes + std::mem::size_of_val(self.joint_bounds.as_slice());
        bytes
    }
}
