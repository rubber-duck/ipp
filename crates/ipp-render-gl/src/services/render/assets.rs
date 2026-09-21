//! RenderService-owned concrete resource factories and GPU allocation lifetimes.

use crate::RenderDevice;
use ipp_core::{MeshAsset, services::asset_management::*};
use std::{
    any::Any,
    cell::{Cell, Ref, RefCell},
    rc::Rc,
};

pub(crate) type SharedRenderDevice<D> = Rc<RefCell<D>>;

pub(crate) struct GlMeshData<D: RenderDevice> {
    pub(crate) mesh: ipp_core::services::asset_management::mesh_metadata::MeshMetadata,
    gpu_bytes: usize,
    gpu: RefCell<Option<D::Mesh>>,
    device: SharedRenderDevice<D>,
}

impl<D: RenderDevice> GlMeshData<D> {
    #[cfg(feature = "particles")]
    pub(super) fn private_mesh(
        device: SharedRenderDevice<D>,
        mesh: MeshAsset,
    ) -> Result<Self, crate::RenderError> {
        let bytes = mesh.vertex_bytes() + std::mem::size_of_val(mesh.indices());
        let gpu = device.borrow_mut().create_mesh(&mesh)?;
        Ok(Self {
            mesh:
                ipp_core::services::asset_management::mesh_metadata::MeshMetadata::from_owned_mesh(
                    mesh,
                ),
            gpu_bytes: bytes,
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
        if let Some(gpu) = self.gpu.get_mut().take() {
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
        if let Some(gpu) = self.gpu.get_mut().take() {
            self.device.borrow_mut().delete_mesh(gpu);
        }
    }
}

pub(crate) fn mesh_asset_loader<D: RenderDevice>(
    device: SharedRenderDevice<D>,
    uploaded: Rc<Cell<u32>>,
    context_active: Rc<Cell<bool>>,
) -> impl AssetLoader<Data = GlMeshData<D>> {
    GlMeshLoader {
        decoder: BufferedAssetLoader::new(|bytes| {
            MeshAsset::decode(bytes)
                .map(|(mesh, _)| mesh)
                .map_err(|error| error.to_string())
        }),
        device,
        uploaded,
        failed_data: None,
        pending_mesh: None,
        context_active,
    }
}

struct GlMeshLoader<D: RenderDevice> {
    decoder: BufferedAssetLoader<MeshAsset>,
    device: SharedRenderDevice<D>,
    uploaded: Rc<Cell<u32>>,
    failed_data: Option<GlMeshData<D>>,
    pending_mesh: Option<MeshAsset>,
    context_active: Rc<Cell<bool>>,
}

impl<D: RenderDevice> AssetLoader for GlMeshLoader<D> {
    type Data = GlMeshData<D>;

    fn take_failed_data(&mut self) -> Option<Self::Data> {
        self.failed_data.take()
    }

    fn poll_load(
        &mut self,
        reader: &mut dyn DataReader,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<Self::Data, String>> {
        use std::task::Poll;
        let mesh = match self.pending_mesh.take() {
            Some(mesh) => mesh,
            None => match self.decoder.poll_load(reader, cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Ready(Ok(mesh)) => mesh,
            },
        };
        let gpu_bytes = mesh.vertex_bytes() + std::mem::size_of_val(mesh.indices());
        if !self.context_active.get() {
            return Poll::Ready(Ok(GlMeshData {
                mesh: ipp_core::services::asset_management::mesh_metadata::MeshMetadata::from_owned_mesh(
                    mesh,
                ),
                gpu_bytes: 0,
                gpu: RefCell::new(None),
                device: self.device.clone(),
            }));
        }
        let gpu = self.device.borrow_mut().create_mesh(&mesh);
        if matches!(gpu, Err(crate::RenderError::ContextLost)) {
            // Detach cancels this loader and keeps any previously decoded asset.
            // A transient context loss must not become a permanent resource failure.
            self.pending_mesh = Some(mesh);
            return Poll::Pending;
        }
        let mut data = GlMeshData {
            mesh:
                ipp_core::services::asset_management::mesh_metadata::MeshMetadata::from_owned_mesh(
                    mesh,
                ),
            gpu_bytes: 0,
            gpu: RefCell::new(None),
            device: self.device.clone(),
        };
        match gpu {
            Ok(gpu) => {
                self.uploaded
                    .set(self.uploaded.get().saturating_add(gpu_bytes as u32));
                data.gpu_bytes = gpu_bytes;
                *data.gpu.get_mut() = Some(gpu);
                Poll::Ready(Ok(data))
            }
            Err(error) => {
                self.failed_data = Some(data);
                Poll::Ready(Err(error.to_string()))
            }
        }
    }
}

pub(crate) struct GlTextureData<D: RenderDevice> {
    pub(crate) info: ipp_core::TextureHeader,
    pub(crate) gpu: Option<D::Texture>,
    device: SharedRenderDevice<D>,
}

impl<D: RenderDevice> Asset for GlTextureData<D> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        &self.info
    }

    fn invalidate_graphics(&mut self) {
        if let Some(gpu) = self.gpu.take() {
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
        if let Some(gpu) = self.gpu.take() {
            self.device.borrow_mut().delete_texture(gpu);
        }
    }
}

pub(crate) fn texture_asset_loader<D: RenderDevice>(
    device: SharedRenderDevice<D>,
    uploaded: Rc<Cell<u32>>,
) -> impl AssetLoader<Data = GlTextureData<D>> {
    GlTextureLoader {
        device,
        uploaded,
        decoder: ipp_core::TextureDecoder::new(),
        data: None,
        row: Vec::new(),
        filled: 0,
        row_index: 0,
    }
}

struct GlTextureLoader<D: RenderDevice> {
    device: SharedRenderDevice<D>,
    uploaded: Rc<Cell<u32>>,
    decoder: ipp_core::TextureDecoder,
    data: Option<GlTextureData<D>>,
    row: Vec<u8>,
    filled: usize,
    row_index: u32,
}

impl<D: RenderDevice> AssetLoader for GlTextureLoader<D> {
    type Data = GlTextureData<D>;

    fn poll_load(
        &mut self,
        reader: &mut dyn DataReader,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<Self::Data, String>> {
        use std::task::Poll;
        if self.data.is_none() {
            let info = match self.decoder.poll_header(reader, cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Ready(Ok(info)) => info,
            };
            let row_bytes = info.width as usize * 4;
            if let Err(error) = self.row.try_reserve_exact(row_bytes) {
                return Poll::Ready(Err(error.to_string()));
            }
            let gpu = match self
                .device
                .borrow_mut()
                .allocate_texture(info.width, info.height)
            {
                Ok(gpu) => gpu,
                Err(error) => return Poll::Ready(Err(error.to_string())),
            };
            self.data = Some(GlTextureData {
                info,
                gpu: Some(gpu),
                device: self.device.clone(),
            });
            self.row.resize(row_bytes, 0);
        }

        // Private storage receives at most 64 KiB of uploads per poll. Data only
        // becomes usable after the complete payload and EOF have been validated.
        let mut consumed = 0;
        while consumed < STREAM_CAPACITY {
            let end = self.row.len().min(self.filled + STREAM_CAPACITY - consumed);
            match self
                .decoder
                .poll_pixels(reader, cx, &mut self.row[self.filled..end])
            {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Ok(self.data.take().expect("validated texture")));
                }
                Poll::Ready(Ok(n)) => {
                    self.filled += n;
                    consumed += n;
                }
            }
            if self.filled == self.row.len() {
                let data = self.data.as_ref().expect("provisional allocation");
                if let Err(error) = self.device.borrow_mut().upload_texture_rows(
                    data.gpu.as_ref().expect("owned GPU texture"),
                    data.info.width,
                    self.row_index,
                    1,
                    &self.row,
                ) {
                    return Poll::Ready(Err(error.to_string()));
                }
                self.uploaded
                    .set(self.uploaded.get().saturating_add(self.row.len() as u32));
                self.row_index += 1;
                self.filled = 0;
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
