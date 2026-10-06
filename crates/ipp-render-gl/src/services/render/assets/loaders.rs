//! RenderService-owned concrete resource factories and GPU allocation lifetimes.

use super::super::statistics::RenderUploadCounter;
use super::context::{RenderAssetContext, RenderAssetLease};
use crate::RenderDevice;
use ipp_core::{MeshAsset, services::asset_management::*};
use std::{
    any::Any,
    cell::{Ref, RefCell},
    rc::Rc,
};

pub(crate) type SharedRenderDevice<D> = Rc<RefCell<D>>;

pub(crate) struct GlMeshData<D: RenderDevice> {
    pub(crate) mesh: ipp_core::services::asset_management::formats::mesh_metadata::MeshMetadata,
    gpu_bytes: usize,
    asset_lease: Option<RenderAssetLease>,
    gpu: RefCell<Option<D::Mesh>>,
    device: SharedRenderDevice<D>,
}

impl<D: RenderDevice> GlMeshData<D> {
    pub(in crate::services::render) fn private_mesh(
        device: SharedRenderDevice<D>,
        mesh: MeshAsset,
    ) -> Result<Self, crate::RenderError> {
        let bytes = mesh.vertex_bytes() + std::mem::size_of_val(mesh.indices());
        let gpu = device.borrow_mut().create_mesh(&mesh)?;
        Ok(Self {
            mesh:
                ipp_core::services::asset_management::formats::mesh_metadata::MeshMetadata::from_owned_mesh(
                    mesh,
                ),
            gpu_bytes: bytes,
            asset_lease: None,
            gpu: RefCell::new(Some(gpu)),
            device,
        })
    }

    /// Loading owns allocation; failed uploads can retain only CPU metadata.
    pub(crate) fn gpu(&self) -> Result<Option<Ref<'_, D::Mesh>>, crate::RenderError> {
        Ok(Ref::filter_map(self.gpu.borrow(), |gpu| gpu.as_ref()).ok())
    }
}

impl<D: RenderDevice> Asset for GlMeshData<D> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        &self.mesh
    }

    fn invalidate_graphics(&mut self) {
        if let Some(gpu) = self.gpu.get_mut().take()
            && self
                .asset_lease
                .as_ref()
                .is_none_or(RenderAssetLease::is_current)
        {
            self.device.borrow_mut().delete_mesh(gpu);
        }
        self.gpu_bytes = 0;
    }

    fn graphics_ready(&self) -> Option<bool> {
        Some(self.gpu.borrow().is_some())
    }

    fn graphics_bytes(&self) -> Option<usize> {
        Some(self.gpu_bytes)
    }

    fn resident_bytes(&self) -> usize {
        self.mesh.resident_bytes() + self.gpu_bytes
    }
}

impl<D: RenderDevice> Drop for GlMeshData<D> {
    fn drop(&mut self) {
        if let Some(gpu) = self.gpu.get_mut().take()
            && self
                .asset_lease
                .as_ref()
                .is_none_or(RenderAssetLease::is_current)
        {
            self.device.borrow_mut().delete_mesh(gpu);
        }
    }
}

pub(crate) fn mesh_asset_loader<D: RenderDevice>(
    device: SharedRenderDevice<D>,
    uploads: RenderUploadCounter,
    context: RenderAssetContext,
) -> impl AssetLoader<Data = GlMeshData<D>> {
    AsyncAssetLoader::new(move |mut reader| async move {
        let mesh = MeshAsset::decode_reader(&mut *reader).await?;
        let gpu_bytes = mesh.vertex_bytes() + std::mem::size_of_val(mesh.indices());
        if !context.is_active() {
            return Ok(GlMeshData { mesh: ipp_core::services::asset_management::formats::mesh_metadata::MeshMetadata::from_owned_mesh(mesh), gpu_bytes: 0, asset_lease: None, gpu: RefCell::new(None), device });
        }
        loop {
            let asset_lease = context.wait().await;
            let gpu = device.borrow_mut().create_mesh(&mesh);
            if matches!(gpu, Err(crate::RenderError::ContextLost)) {
                context.set_active(false);
                continue;
            }
            let mut data = GlMeshData { mesh: ipp_core::services::asset_management::formats::mesh_metadata::MeshMetadata::from_owned_mesh(mesh), gpu_bytes: 0, asset_lease: Some(asset_lease), gpu: RefCell::new(None), device };
            match gpu {
                Ok(gpu) => {
                    uploads.add(gpu_bytes);
                    data.gpu_bytes = gpu_bytes;
                    *data.gpu.get_mut() = Some(gpu);
                    return Ok(data);
                }
                Err(error) => return Err(AssetLoadFailure::with_decoded(error.to_string(), data)),
            }
        }
    })
}

pub(crate) struct GlTextureData<D: RenderDevice> {
    pub(crate) info: ipp_core::TextureHeader,
    pub(crate) gpu: Option<D::Texture>,
    pub(crate) asset_lease: RenderAssetLease,
    pub(crate) device: SharedRenderDevice<D>,
}

impl<D: RenderDevice> Asset for GlTextureData<D> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        &self.info
    }

    fn invalidate_graphics(&mut self) {
        if let Some(gpu) = self.gpu.take()
            && self.asset_lease.is_current()
        {
            self.device.borrow_mut().delete_texture(gpu);
        }
    }

    fn graphics_ready(&self) -> Option<bool> {
        Some(self.gpu.is_some())
    }

    fn graphics_bytes(&self) -> Option<usize> {
        Some(if self.gpu.is_some() {
            self.info.pixel_bytes as usize
        } else {
            0
        })
    }

    fn resident_bytes(&self) -> usize {
        std::mem::size_of_val(&self.info) + self.graphics_bytes().unwrap_or(0)
    }
}

impl<D: RenderDevice> Drop for GlTextureData<D> {
    fn drop(&mut self) {
        if let Some(gpu) = self.gpu.take()
            && self.asset_lease.is_current()
        {
            self.device.borrow_mut().delete_texture(gpu);
        }
    }
}

pub(crate) fn texture_asset_loader<D: RenderDevice>(
    device: SharedRenderDevice<D>,
    uploads: RenderUploadCounter,
    context: RenderAssetContext,
) -> impl AssetLoader<Data = GlTextureData<D>> {
    AsyncAssetLoader::decode(move |mut reader| async move {
        let mut decoder = ipp_core::TextureDecoder::new();
        let info = decoder.read_header(&mut *reader).await?;
        let asset_lease = context.wait().await;
        let gpu = device
            .borrow_mut()
            .allocate_texture(info.width, info.height)
            .map_err(|error| error.to_string())?;
        let data = GlTextureData {
            info,
            gpu: Some(gpu),
            asset_lease,
            device: device.clone(),
        };
        let row_bytes = info.width as usize * 4;
        let mut row = Vec::new();
        row.try_reserve_exact(row_bytes)
            .map_err(|error| error.to_string())?;
        row.resize(row_bytes, 0);
        let mut budget = decode::DecodeBudget::default();
        for index in 0..info.height {
            let mut filled = 0;
            while filled < row.len() {
                filled += decoder
                    .read_pixels(&mut *reader, &mut row[filled..])
                    .await?;
                budget.advance(0).await;
            }
            context.wait().await;
            if !data.asset_lease.is_current() {
                return Err("Texture creating context was lost".into());
            }
            device
                .borrow_mut()
                .upload_texture_rows(data.gpu.as_ref().unwrap(), info.width, index, 1, &row)
                .map_err(|error| error.to_string())?;
            uploads.add(row.len());
            budget.advance(row.len()).await;
        }
        decoder.read_pixels(&mut *reader, &mut []).await?;
        Ok(data)
    })
}
