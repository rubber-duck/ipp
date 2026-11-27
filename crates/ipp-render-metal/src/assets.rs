use std::{any::Any, mem::size_of, rc::Rc};

use ipp_core::{
    MeshAsset, TextureAsset,
    services::asset_management::{
        Asset, AssetLoadFailure, AssetLoader, AsyncAssetLoader,
        formats::mesh_metadata::MeshMetadata,
    },
};

use ipp_render_scene::mesh::mesh_vertices;

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
