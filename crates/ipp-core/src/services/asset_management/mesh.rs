//! Owned, immutable CPU meshes. No renderer resources or borrowed input buffers.

use crate::ErrorReason;

/// Exact logical mesh identity; variants never mutate a shared active selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MeshKey {
    /// Logical mesh identity (nonzero for activation).
    pub asset: u64,
    /// Per-entity mesh variant.
    pub variant: u32,
}

/// Exclusive payload handoff to the host-owned mutation boundary.
#[derive(Debug)]
pub struct MeshUpload {
    /// Caller-supplied upload correlation identity.
    pub id: u64,
    /// Exact mesh identity and variant.
    pub key: MeshKey,
    /// Owned IPPM source payload (UVs and texture weights require textures).
    pub bytes: Vec<u8>,
}

/// Accepted source and decoded allocation sizes, excluding container overhead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshStats {
    /// Encoded source payload byte count.
    pub source_bytes: u32,
    /// Decoded vertex byte count.
    pub vertex_bytes: u32,
    /// Decoded index byte count.
    pub index_bytes: u32,
    /// Decoded vertex count.
    pub vertices: u32,
    /// Decoded triangle index count.
    pub indices: u32,
}

/// Immutable decoded CPU data retained for rendering and context recovery.
#[derive(Debug)]
pub struct MeshAsset {
    metadata: super::mesh_metadata::MeshMetadata,
    positions: Vec<[f32; 3]>,
    #[cfg(feature = "skeletal-animation")]
    joint_indices: Option<Vec<[u8; 4]>>,
    #[cfg(feature = "skeletal-animation")]
    max_joint_index: Option<u8>,
    #[cfg(feature = "skeletal-animation")]
    joint_weights: Option<Vec<[f32; 4]>>,
    #[cfg(feature = "skeletal-animation")]
    joint_bounds: Vec<Option<[[f32; 3]; 2]>>,
    bounds: [[f32; 3]; 2],
    colors: Option<Vec<[f32; 3]>>,
    normals: Option<Vec<[f32; 3]>>,
    indices: Vec<u16>,
    uvs: Option<Vec<[f32; 2]>>,
    texture_weights: Option<Vec<u8>>,
}

impl MeshAsset {
    pub(super) fn into_metadata(self) -> super::mesh_metadata::MeshMetadata {
        self.metadata
    }

    /// Shared exact local enclosure computed once from complete decoded positions.
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        (self.bounds[0], self.bounds[1])
    }

    /// Mesh bind-space enclosure for every nonzero influence in palette order.
    #[cfg(feature = "skeletal-animation")]
    pub fn joint_bounds(&self) -> &[Option<[[f32; 3]; 2]>] {
        &self.joint_bounds
    }

    /// Position xyz in the authored vertex order.
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// Optional linear RGB; absent colors use white without retained vertex data.
    pub fn colors(&self) -> Option<&[[f32; 3]]> {
        self.colors.as_deref()
    }

    /// Optional finite, nonzero object-space normals; shaders normalize them.
    pub fn normals(&self) -> Option<&[[f32; 3]]> {
        self.normals.as_deref()
    }

    /// Number of authored vertices.
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// CPU metadata for conservative deformation bounds, excluded from GPU streams.
    pub fn bounds_bytes(&self) -> usize {
        #[cfg(feature = "skeletal-animation")]
        {
            std::mem::size_of_val(self.joint_bounds.as_slice())
        }
        #[cfg(not(feature = "skeletal-animation"))]
        {
            0
        }
    }

    /// Retained vertex bytes, excluding metadata and absent attributes.
    pub fn vertex_bytes(&self) -> usize {
        let bytes = std::mem::size_of_val(self.positions())
            + self.colors().map_or(0, std::mem::size_of_val)
            + self.normals().map_or(0, std::mem::size_of_val);
        let bytes = bytes
            + self.uvs().map_or(0, std::mem::size_of_val)
            + self.texture_weights().map_or(0, std::mem::size_of_val);
        #[cfg(feature = "skeletal-animation")]
        let bytes = bytes
            + self.joint_indices().map_or(0, std::mem::size_of_val)
            + self.joint_weights().map_or(0, std::mem::size_of_val);
        bytes
    }

    /// Four palette indices per vertex, absent on rigid geometry.
    #[cfg(feature = "skeletal-animation")]
    pub fn joint_indices(&self) -> Option<&[[u8; 4]]> {
        self.joint_indices.as_deref()
    }

    /// Largest palette index, validated once without scanning vertices each frame.
    #[cfg(feature = "skeletal-animation")]
    pub fn max_joint_index(&self) -> Option<u8> {
        self.max_joint_index
    }

    /// Four nonnegative weights normalized once at decode, absent on rigid geometry.
    #[cfg(feature = "skeletal-animation")]
    pub fn joint_weights(&self) -> Option<&[[f32; 4]]> {
        self.joint_weights.as_deref()
    }

    /// Counter-clockwise triangle indices.
    pub fn indices(&self) -> &[u16] {
        &self.indices
    }

    /// Optional UV coordinates; (0, 0) samples the top-left source texel.
    pub fn uvs(&self) -> Option<&[[f32; 2]]> {
        self.uvs.as_deref()
    }

    /// Optional normalized texture contributions: 0 is solid, 255 fully textured.
    /// Missing weights use 1.0 without a retained stream.
    pub fn texture_weights(&self) -> Option<&[u8]> {
        self.texture_weights.as_deref()
    }

    /// Validate the complete encoded payload and decode its present attributes.
    pub fn decode(bytes: &[u8]) -> Result<(Self, MeshStats), ErrorReason> {
        if bytes.len() < 16 || &bytes[..4] != b"IPPM" {
            return Err(ErrorReason::InvalidAsset);
        }

        let vertices = read_u32(bytes, 8);
        let indices = read_u32(bytes, 12);
        if vertices == 0 || vertices > 65536 || indices == 0 || !indices.is_multiple_of(3) {
            return Err(ErrorReason::InvalidAsset);
        }
        let index_bytes = indices.checked_mul(2).ok_or(ErrorReason::InvalidAsset)?;
        let mut mesh = Self {
            metadata: Default::default(),
            positions: Vec::new(),
            #[cfg(feature = "skeletal-animation")]
            joint_indices: None,
            #[cfg(feature = "skeletal-animation")]
            max_joint_index: None,
            #[cfg(feature = "skeletal-animation")]
            joint_weights: None,
            #[cfg(feature = "skeletal-animation")]
            joint_bounds: Vec::new(),
            bounds: [[0.0; 3]; 2],
            colors: None,
            normals: None,
            indices: Vec::new(),
            uvs: None,
            texture_weights: None,
        };
        let index_start = match read_u32(bytes, 4) {
            1 => mesh.decode_interleaved(bytes, vertices, index_bytes, 24)?,
            2 => mesh.decode_interleaved(bytes, vertices, index_bytes, 32)?,
            3 => mesh.decode_streams(bytes, vertices, index_bytes)?,
            _ => return Err(ErrorReason::InvalidAsset),
        };

        {
            mesh.bounds = [mesh.positions[0]; 2];
            for position in &mesh.positions {
                for (axis, &value) in position.iter().enumerate() {
                    mesh.bounds[0][axis] = mesh.bounds[0][axis].min(value);
                    mesh.bounds[1][axis] = mesh.bounds[1][axis].max(value);
                }
            }
        }

        #[cfg(feature = "skeletal-animation")]
        if let (Some(joints), Some(weights)) = (&mesh.joint_indices, &mesh.joint_weights) {
            mesh.joint_bounds = vec![None; usize::from(mesh.max_joint_index.unwrap()) + 1];
            for ((position, joints), weights) in mesh.positions.iter().zip(joints).zip(weights) {
                for (&joint, &weight) in joints.iter().zip(weights) {
                    if weight > 0.0 {
                        let bounds =
                            mesh.joint_bounds[joint as usize].get_or_insert([*position; 2]);
                        for axis in 0..3 {
                            bounds[0][axis] = bounds[0][axis].min(position[axis]);
                            bounds[1][axis] = bounds[1][axis].max(position[axis]);
                        }
                    }
                }
            }
        }

        mesh.indices
            .try_reserve_exact(indices as usize)
            .map_err(|_| ErrorReason::Capacity)?;
        for index in bytes[index_start..].as_chunks::<2>().0 {
            let index = u16::from_le_bytes(*index);
            if u32::from(index) >= vertices {
                return Err(ErrorReason::InvalidAsset);
            }
            mesh.indices.push(index);
        }

        // Preserve the legacy rule: at least one triangle has area. Use f64
        // so finite f32 extremes cannot overflow the degeneracy calculation.
        let has_area = mesh.indices.as_chunks::<3>().0.iter().any(|t| {
            let [a, b, c] = t.map(|index| mesh.positions[index as usize]);
            let u = std::array::from_fn::<_, 3, _>(|i| f64::from(b[i]) - f64::from(a[i]));
            let v = std::array::from_fn::<_, 3, _>(|i| f64::from(c[i]) - f64::from(a[i]));
            [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ]
            .iter()
            .any(|&area| area != 0.0)
        });
        if !has_area {
            return Err(ErrorReason::InvalidAsset);
        }

        let stats = MeshStats {
            source_bytes: bytes.len() as u32,
            vertex_bytes: mesh.vertex_bytes() as u32,
            index_bytes,
            vertices,
            indices,
        };
        mesh.metadata = super::mesh_metadata::MeshMetadata::from_mesh(&mesh);
        Ok((mesh, stats))
    }

    fn decode_interleaved(
        &mut self,
        bytes: &[u8],
        vertices: u32,
        index_bytes: u32,
        stride: u32,
    ) -> Result<usize, ErrorReason> {
        let index_start = vertices
            .checked_mul(stride)
            .and_then(|n| n.checked_add(16))
            .ok_or(ErrorReason::InvalidAsset)?;
        if index_start.checked_add(index_bytes).map(|n| n as usize) != Some(bytes.len()) {
            return Err(ErrorReason::InvalidAsset);
        }

        self.positions = Vec::with_capacity(vertices as usize);
        let mut colors = Vec::with_capacity(vertices as usize);
        let mut uvs = (stride == 32).then(|| Vec::with_capacity(vertices as usize));
        for vertex in bytes[16..index_start as usize].chunks_exact(stride as usize) {
            self.positions.push(read_floats(&vertex[..12], false)?);
            colors.push(read_floats(&vertex[12..24], true)?);
            if let Some(uvs) = &mut uvs {
                uvs.push(read_floats(&vertex[24..32], false)?);
            }
        }
        self.colors = Some(colors);
        {
            self.uvs = uvs;
        }
        Ok(index_start as usize)
    }

    fn decode_streams(
        &mut self,
        bytes: &[u8],
        vertices: u32,
        index_bytes: u32,
    ) -> Result<usize, ErrorReason> {
        if bytes.len() < 20 {
            return Err(ErrorReason::InvalidAsset);
        }
        let count = read_u32(bytes, 16) as usize;
        if !(1..=7).contains(&count) || bytes.len() < 20 + count * 8 {
            return Err(ErrorReason::InvalidAsset);
        }

        // Validate all descriptors and extents before allocating retained streams.
        let mut streams = [None; 7];
        let mut cursor = 20 + count * 8;
        let mut previous = None;
        for descriptor in bytes[20..cursor].as_chunks::<8>().0 {
            let semantic = descriptor[0];
            let (format, width) = match semantic {
                0 | 1 | 4 => (1, 12),
                2 => (2, 8),
                3 => (3, 1),
                #[cfg(feature = "skeletal-animation")]
                5 => (4, 4),
                #[cfg(feature = "skeletal-animation")]
                6 => (5, 16),
                _ => return Err(ErrorReason::InvalidAsset),
            };
            let length = read_u32(descriptor, 4);
            if previous.is_some_and(|value| value >= semantic)
                || descriptor[1] != format
                || descriptor[2..4] != [0, 0]
                || vertices.checked_mul(width) != Some(length)
            {
                return Err(ErrorReason::InvalidAsset);
            }
            previous = Some(semantic);
            let end = cursor
                .checked_add(length as usize)
                .ok_or(ErrorReason::InvalidAsset)?;
            streams[semantic as usize] =
                Some(bytes.get(cursor..end).ok_or(ErrorReason::InvalidAsset)?);
            cursor = end;
        }
        if streams[0].is_none()
            || (streams[3].is_some() && streams[2].is_none())
            || streams[5].is_some() != streams[6].is_some()
            || cursor.checked_add(index_bytes as usize) != Some(bytes.len())
        {
            return Err(ErrorReason::InvalidAsset);
        }

        self.positions = decode_floats(streams[0].expect("validated position stream"), false)?;
        self.colors = streams[1]
            .map(|bytes| decode_floats(bytes, true))
            .transpose()?;
        self.normals = streams[4]
            .map(|bytes| decode_floats(bytes, false))
            .transpose()?;
        if self
            .normals()
            .is_some_and(|normals| normals.contains(&[0.0; 3]))
        {
            return Err(ErrorReason::InvalidAsset);
        }

        {
            self.uvs = streams[2]
                .map(|bytes| decode_floats(bytes, false))
                .transpose()?;
            self.texture_weights = streams[3].map(<[u8]>::to_vec);
        }
        #[cfg(feature = "skeletal-animation")]
        if let (Some(indices), Some(weights)) = (streams[5], streams[6]) {
            let indices: Vec<_> = indices.as_chunks::<4>().0.to_vec();
            if indices
                .iter()
                .flatten()
                .any(|&v| v as usize >= crate::MAX_JOINTS)
            {
                return Err(ErrorReason::InvalidAsset);
            }
            let mut weights = decode_floats::<4>(weights, true)?;
            for weights in &mut weights {
                let sum: f64 = weights.iter().map(|&v| f64::from(v)).sum();
                if sum <= 0.0 {
                    return Err(ErrorReason::InvalidAsset);
                }
                for weight in weights {
                    *weight = (f64::from(*weight) / sum) as f32;
                }
            }
            self.max_joint_index = indices.iter().flatten().copied().max();
            self.joint_indices = Some(indices);
            self.joint_weights = Some(weights);
        }
        Ok(cursor)
    }
}

fn read_floats<const N: usize>(bytes: &[u8], color: bool) -> Result<[f32; N], ErrorReason> {
    let values = std::array::from_fn(|i| f32::from_bits(read_u32(bytes, i * 4)));
    if values
        .iter()
        .any(|v| !v.is_finite() || (color && !(0.0..=1.0).contains(v)))
    {
        return Err(ErrorReason::InvalidAsset);
    }
    Ok(values)
}

// Stable Rust cannot express the generic N * 4 as an as_chunks const argument.
#[allow(clippy::chunks_exact_to_as_chunks)]
fn decode_floats<const N: usize>(bytes: &[u8], color: bool) -> Result<Vec<[f32; N]>, ErrorReason> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(bytes.len() / (N * 4))
        .map_err(|_| ErrorReason::Capacity)?;
    for bytes in bytes.chunks_exact(N * 4) {
        values.push(read_floats(bytes, color)?);
    }
    Ok(values)
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated payload extent"),
    )
}

/// Compiled mesh type identity, independent of factory and source scheme.
pub const MESH_TYPE: crate::services::asset_management::AssetTypeId =
    crate::services::asset_management::AssetTypeId(1);

impl crate::services::asset_management::Asset for MeshAsset {
    fn metadata(&self) -> &dyn std::any::Any {
        &self.metadata
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn decoded(&self) -> &dyn std::any::Any {
        self
    }

    fn resident_bytes(&self) -> usize {
        self.vertex_bytes()
            + std::mem::size_of_val(self.indices())
            + self.bounds_bytes()
            + self.metadata.resident_bytes()
    }
}

/// Construct a headless decoder; graphics Hosts may register their own loader.
pub fn cpu_mesh_loader() -> impl super::AssetLoader<Data = MeshAsset> {
    super::BufferedAssetLoader::new(move |bytes| {
        MeshAsset::decode(bytes)
            .map(|(data, _)| data)
            .map_err(|error| error.to_string())
    })
}

impl super::writer::AssetEncoder for MeshAsset {
    fn encode_asset(&self, max_bytes: usize) -> Result<Vec<u8>, String> {
        let mut streams: Vec<(u8, u8, Vec<u8>)> = Vec::new();
        let expected = self
            .vertex_bytes()
            .checked_add(self.indices.len() * 2)
            .and_then(|bytes| bytes.checked_add(76))
            .ok_or("Mesh output overflow")?;
        if expected > max_bytes {
            return Err("Mesh output byte budget exhausted".into());
        }
        let floats = |values: &[f32]| {
            values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>()
        };
        streams.push((0, 1, floats(self.positions.as_flattened())));
        if let Some(values) = &self.colors {
            streams.push((1, 1, floats(values.as_flattened())));
        }
        {
            if let Some(values) = &self.uvs {
                streams.push((2, 2, floats(values.as_flattened())));
            }
            if let Some(values) = &self.texture_weights {
                streams.push((3, 3, values.clone()));
            }
        }
        if let Some(values) = &self.normals {
            streams.push((4, 1, floats(values.as_flattened())));
        }
        #[cfg(feature = "skeletal-animation")]
        {
            if let Some(values) = &self.joint_indices {
                streams.push((5, 4, values.as_flattened().to_vec()));
            }
            if let Some(values) = &self.joint_weights {
                streams.push((6, 5, floats(values.as_flattened())));
            }
        }
        let mut bytes = Vec::with_capacity(expected);
        bytes.extend_from_slice(b"IPPM");
        for value in [
            3,
            self.positions.len() as u32,
            self.indices.len() as u32,
            streams.len() as u32,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for (semantic, format, data) in &streams {
            bytes.extend_from_slice(&[*semantic, *format, 0, 0]);
            bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        }
        for (_, _, data) in streams {
            bytes.extend_from_slice(&data);
        }
        for index in &self.indices {
            bytes.extend_from_slice(&index.to_le_bytes());
        }
        Ok(bytes)
    }
}
