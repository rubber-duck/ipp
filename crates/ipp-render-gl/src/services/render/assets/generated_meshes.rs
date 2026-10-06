//! Retained runtime geometry through the ordinary MeshAsset/device draw path.

use super::loaders::{GlMeshData, SharedRenderDevice};
use crate::{RenderDevice, RenderError};
use ipp_core::systems::canvas::CanvasTarget;
use ipp_core::systems::plot::{PlotMesh, PlotPreparedGeometry};
use ipp_core::{HostRuntime, WorldRef};
use std::{
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Weak},
};

pub(in crate::services::render) type GeneratedMeshKey = (WorldRef, CanvasTarget, u32);

struct Entry<D: RenderDevice> {
    source: Weak<PlotPreparedGeometry>,
    mesh_index: usize,
    data: Rc<GlMeshData<D>>,
}

pub(in crate::services::render) struct GeneratedMeshCache<D: RenderDevice> {
    entries: BTreeMap<GeneratedMeshKey, Entry<D>>,
    device: SharedRenderDevice<D>,
}

impl<D: RenderDevice> GeneratedMeshCache<D> {
    pub fn new(device: SharedRenderDevice<D>) -> Self {
        Self {
            entries: BTreeMap::new(),
            device,
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn retain_live_worlds(&mut self, host: &HostRuntime) {
        self.entries.retain(|(world, _, _), entry| {
            host.world_fault(*world).is_ok() && entry.source.strong_count() != 0
        });
    }

    pub fn get(
        &mut self,
        key: GeneratedMeshKey,
        source: &Arc<PlotPreparedGeometry>,
        mesh_index: usize,
    ) -> Result<(Rc<GlMeshData<D>>, usize), RenderError> {
        if let Some(entry) = self.entries.get(&key).filter(|entry| {
            entry.mesh_index == mesh_index
                && entry
                    .source
                    .upgrade()
                    .is_some_and(|retained| Arc::ptr_eq(&retained, source))
        }) {
            return Ok((entry.data.clone(), 0));
        }
        let mesh = source
            .meshes
            .get(mesh_index)
            .ok_or(RenderError::MissingMesh)?;
        let bytes = encode(mesh).map_err(|error| RenderError::RenderDevice(error.to_string()))?;
        let cpu = ipp_core::MeshAsset::decode(&bytes)
            .map_err(|error| RenderError::RenderDevice(error.to_string()))?;
        let uploaded = cpu.vertex_bytes() + std::mem::size_of_val(cpu.indices());
        let data = Rc::new(GlMeshData::private_mesh(self.device.clone(), cpu)?);
        self.entries.insert(
            key,
            Entry {
                source: Arc::downgrade(source),
                mesh_index,
                data: data.clone(),
            },
        );
        Ok((data, uploaded))
    }
}

fn encode(mesh: &PlotMesh) -> Result<Vec<u8>, ipp_core::ErrorReason> {
    if mesh.positions.is_empty()
        || mesh.positions.len() > 65535
        || mesh.indices.is_empty()
        || !mesh.indices.len().is_multiple_of(3)
        || !mesh.colors.is_empty() && mesh.colors.len() != mesh.positions.len()
        || !mesh.normals.is_empty() && mesh.normals.len() != mesh.positions.len()
    {
        return Err(ipp_core::ErrorReason::InvalidValue);
    }
    let streams = 1 + u32::from(!mesh.colors.is_empty()) + u32::from(!mesh.normals.is_empty());
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"IPPM");
    for value in [
        3,
        mesh.positions.len() as u32,
        mesh.indices.len() as u32,
        streams,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for (semantic, values) in [
        (0u8, &mesh.positions),
        (1, &mesh.colors),
        (4, &mesh.normals),
    ] {
        if !values.is_empty() {
            bytes.extend_from_slice(&[semantic, 1, 0, 0]);
            bytes.extend_from_slice(&((values.len() * 12) as u32).to_le_bytes());
        }
    }
    for values in [&mesh.positions, &mesh.colors, &mesh.normals] {
        for value in values {
            for lane in value {
                bytes.extend_from_slice(&lane.to_le_bytes());
            }
        }
    }
    for index in &mesh.indices {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "generated_meshes_tests.rs"]
mod tests;
