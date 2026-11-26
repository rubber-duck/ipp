use std::{any::Any, mem::size_of, rc::Rc};

use ipp_core::{
    MeshAsset, TextureAsset,
    services::asset_management::{
        Asset, AssetLoadFailure, AssetLoader, AsyncAssetLoader,
        formats::mesh_metadata::MeshMetadata,
    },
};

use crate::device::{Device, MeshBuffers, MeshVertex, Object};

pub(super) struct MetalMesh {
    pub(super) metadata: MeshMetadata,
    pub(super) buffers: Option<MeshBuffers>,
    bytes: usize,
}

impl Asset for MetalMesh {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        &self.metadata
    }

    fn invalidate_graphics(&mut self) {
        self.buffers = None;
    }

    fn graphics_ready(&self) -> Option<bool> {
        Some(self.buffers.is_some())
    }

    fn graphics_bytes(&self) -> Option<usize> {
        Some(if self.buffers.is_some() {
            self.bytes
        } else {
            0
        })
    }

    fn resident_bytes(&self) -> usize {
        self.metadata.resident_bytes() + self.graphics_bytes().unwrap_or(0)
    }
}

pub(super) fn mesh_loader(device: Rc<Device>) -> impl AssetLoader<Data = MetalMesh> {
    AsyncAssetLoader::new(move |mut source| async move {
        let mesh = MeshAsset::decode_reader(&mut *source).await?;
        let vertices = mesh_vertices(&mesh)?;
        let buffers = device.mesh(&vertices, mesh.indices());
        let bytes =
            vertices.len() * size_of::<MeshVertex>() + std::mem::size_of_val(mesh.indices());
        let data = MetalMesh {
            metadata: MeshMetadata::from_owned_mesh(mesh),
            buffers,
            bytes,
        };
        if data.buffers.is_none() {
            return Err(AssetLoadFailure::with_decoded(
                "Metal mesh allocation failed".into(),
                data,
            ));
        }
        Ok(data)
    })
}

pub(super) fn mesh_vertices(mesh: &MeshAsset) -> Result<Vec<MeshVertex>, String> {
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

pub(super) struct MetalTexture {
    pub(super) texture: Option<Object>,
    asset: TextureAsset,
}

impl Asset for MetalTexture {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        &self.asset
    }

    fn invalidate_graphics(&mut self) {
        self.texture = None;
    }

    fn graphics_ready(&self) -> Option<bool> {
        Some(self.texture.is_some())
    }

    fn graphics_bytes(&self) -> Option<usize> {
        Some(if self.texture.is_some() {
            self.asset.pixels().len()
        } else {
            0
        })
    }

    fn resident_bytes(&self) -> usize {
        self.asset.pixels().len() + self.graphics_bytes().unwrap_or(0)
    }
}

pub(super) fn texture_loader(device: Rc<Device>) -> impl AssetLoader<Data = MetalTexture> {
    AsyncAssetLoader::new(move |mut source| async move {
        let asset = TextureAsset::decode_reader(&mut *source).await?;
        let texture = device.texture(asset.width(), asset.height(), asset.pixels());
        let data = MetalTexture {
            asset,
            texture,
        };
        if data.texture.is_none() {
            return Err(AssetLoadFailure::with_decoded(
                "Metal texture allocation failed".into(),
                data,
            ));
        }
        Ok(data)
    })
}
