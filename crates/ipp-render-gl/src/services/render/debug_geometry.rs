//! Private debug meshes: no source registrations, uploads, or world asset events.

use std::collections::{BTreeMap, BTreeSet};

use crate::{RenderDevice, RenderError, services::render::assets::SharedRenderDevice};
use ipp_core::{DebugRenderItem, systems::geometry::GeometryPrimitiveVisual};

type DebugGeometryRenderKey = [u32; 6];

pub(crate) struct DebugGeometryRenderMesh<D: RenderDevice> {
    pub(crate) gpu: Option<D::Mesh>,
    pub(crate) triangles: u32,
    bytes: usize,
    device: SharedRenderDevice<D>,
}

enum DebugGeometryRenderEntry<D: RenderDevice> {
    Ready(DebugGeometryRenderMesh<D>),
    Failed,
}

pub(crate) struct DebugGeometryRenderCache<D: RenderDevice> {
    entries: BTreeMap<DebugGeometryRenderKey, DebugGeometryRenderEntry<D>>,
    used: Vec<DebugGeometryRenderKey>,
    device: SharedRenderDevice<D>,
}

impl<D: RenderDevice> DebugGeometryRenderCache<D> {
    pub(crate) fn new(device: SharedRenderDevice<D>) -> Self {
        Self {
            entries: BTreeMap::new(),
            used: Vec::new(),
            device,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(crate) fn resident_bytes(&self) -> usize {
        self.entries
            .values()
            .map(|entry| match entry {
                DebugGeometryRenderEntry::Ready(mesh) => mesh.bytes,
                _ => 0,
            })
            .sum()
    }

    pub(crate) fn retain(&mut self, items: &[DebugRenderItem]) {
        if ipp_core::render_buffer_reuse_enabled() {
            self.used.clear();
            self.used
                .extend(items.iter().map(|item| item.geometry.mesh_key()));
            self.used.sort_unstable();
            self.used.dedup();
            self.entries
                .retain(|key, _| self.used.binary_search(key).is_ok());
        } else {
            let used: BTreeSet<_> = items.iter().map(|item| item.geometry.mesh_key()).collect();
            self.entries.retain(|key, _| used.contains(key));
        }
    }

    pub(crate) fn get(
        &mut self,
        shape: &GeometryPrimitiveVisual,
    ) -> Result<(Option<&DebugGeometryRenderMesh<D>>, u32), RenderError> {
        let key = shape.mesh_key();
        let mut uploaded = 0;
        if !self.entries.contains_key(&key) {
            let result = (|| {
                let mesh = ipp_core::services::asset_management::builtin::debug_mesh(shape)
                    .map_err(|error| RenderError::RenderDevice(error.to_string()))?;
                let bytes = mesh.vertex_bytes() + std::mem::size_of_val(mesh.indices());
                let gpu = self.device.borrow_mut().create_mesh(&mesh)?;
                uploaded = bytes as u32;
                Ok(DebugGeometryRenderEntry::Ready(DebugGeometryRenderMesh {
                    gpu: Some(gpu),
                    triangles: (mesh.indices().len() / 3) as u32,
                    bytes,
                    device: self.device.clone(),
                }))
            })();
            let entry = match result {
                Ok(entry) => entry,
                Err(RenderError::ContextLost) => return Err(RenderError::ContextLost),
                Err(_error) => {
                    ipp_core::diagnostic!(Warn, "[IPP renderer] debug.mesh_failed error={_error}");
                    DebugGeometryRenderEntry::Failed
                }
            };
            self.entries.insert(key, entry);
        }
        let mesh = match self.entries.get(&key) {
            Some(DebugGeometryRenderEntry::Ready(mesh)) => Some(mesh),
            _ => None,
        };
        Ok((mesh, uploaded))
    }
}

impl<D: RenderDevice> Drop for DebugGeometryRenderMesh<D> {
    fn drop(&mut self) {
        if let Some(gpu) = self.gpu.take() {
            self.device.borrow_mut().delete_mesh(gpu);
        }
    }
}
