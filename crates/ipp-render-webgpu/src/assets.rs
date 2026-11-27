use std::{any::Any, mem::size_of, rc::Rc};

use ipp_core::{
    MeshAsset, TextureAsset,
    services::asset_management::{
        Asset, AssetLoadFailure, AssetLoader, AsyncAssetLoader,
        formats::mesh_metadata::MeshMetadata,
    },
};

use ipp_render_scene::mesh::{MeshVertex, mesh_vertices};
use wgpu::util::DeviceExt;

use crate::device::Graphics;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MeshLayout {
    Full,
    Plot,
    PaintedPlot,
}

#[derive(Clone)]
pub(super) struct MeshBuffers {
    pub layout: MeshLayout,
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub count: u32,
}

pub(super) struct WebGpuMesh {
    pub(super) metadata: MeshMetadata,
    pub(super) buffers: Option<MeshBuffers>,
    bytes: usize,
}

impl Asset for WebGpuMesh {
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

pub(super) fn mesh_loader(device: Rc<Graphics>) -> impl AssetLoader<Data = WebGpuMesh> {
    AsyncAssetLoader::new(move |mut source| async move {
        let mesh = MeshAsset::decode_reader(&mut *source).await?;
        let vertices = mesh_vertices(&mesh)?;
        let buffers = device
            .upload(|| MeshBuffers::upload(&device, &vertices, mesh.indices()))
            .await;
        let bytes =
            vertices.len() * size_of::<MeshVertex>() + std::mem::size_of_val(mesh.indices());
        let mut data = WebGpuMesh {
            metadata: MeshMetadata::from_owned_mesh(mesh),
            buffers: None,
            bytes,
        };
        match buffers {
            Ok(buffers) => data.buffers = Some(buffers),
            Err(error) => return Err(AssetLoadFailure::with_decoded(error, data)),
        }
        Ok(data)
    })
}

pub(super) struct WebGpuTexture {
    pub(super) texture: Option<(wgpu::BindGroup, wgpu::BindGroup, wgpu::TextureView)>,
    asset: TextureAsset,
}

impl Asset for WebGpuTexture {
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

pub(super) fn texture_loader(device: Rc<Graphics>) -> impl AssetLoader<Data = WebGpuTexture> {
    AsyncAssetLoader::new(move |mut source| async move {
        let asset = TextureAsset::decode_reader(&mut *source).await?;
        let texture = device
            .upload(|| device.texture(asset.width(), asset.height(), asset.pixels()))
            .await;
        let mut data = WebGpuTexture {
            asset,
            texture: None,
        };
        match texture {
            Ok(texture) => data.texture = Some(texture),
            Err(error) => return Err(AssetLoadFailure::with_decoded(error, data)),
        }
        Ok(data)
    })
}

impl MeshBuffers {
    pub fn upload(device: &Graphics, vertices: &[MeshVertex], indices: &[u16]) -> Option<Self> {
        Self::upload_vertices(device, vertices, indices, MeshLayout::Full)
    }

    pub fn upload_vertices<T: bytemuck::Pod>(
        device: &Graphics,
        vertices: &[T],
        indices: &[u16],
        layout: MeshLayout,
    ) -> Option<Self> {
        (!vertices.is_empty() && !indices.is_empty()).then(|| MeshBuffers {
            layout,
            vertices: device
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("IPP mesh vertices"),
                    contents: bytemuck::cast_slice(vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
            indices: device
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("IPP mesh indices"),
                    contents: bytemuck::cast_slice(indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
            count: indices.len() as u32,
        })
    }
}
