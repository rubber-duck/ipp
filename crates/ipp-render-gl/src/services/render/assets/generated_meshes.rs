//! Retained runtime geometry through the ordinary MeshAsset/device draw path.

use super::loaders::{GlMeshData, SharedRenderDevice};
use super::triangle_order::TriangleOrder;
use crate::{RenderDevice, RenderError};
use ipp_core::systems::canvas::CanvasTarget;
use ipp_core::systems::plot::{PlotMesh, PlotMeshVariant, PlotPreparedGeometry};
use ipp_core::{HostRuntime, WorldRef};
use std::{
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Weak},
};

pub(in crate::services::render) type GeneratedMeshKey =
    (WorldRef, CanvasTarget, u32, PlotMeshVariant, bool);

struct Entry<D: RenderDevice> {
    source: Weak<PlotPreparedGeometry>,
    mesh_index: usize,
    data: Rc<GlMeshData<D>>,
    order: TriangleOrder,
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
        self.entries.retain(|(world, _, _, _, _), entry| {
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
        #[cfg(feature = "instrumentation")]
        let _trace = ipp_core::profiling::CpuTraceScope::new("RenderService.mesh-upload");
        let bytes = encode_variant(mesh, key.3, key.4)
            .map_err(|error| RenderError::RenderDevice(error.to_string()))?;
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
                order: TriangleOrder::default(),
            },
        );
        Ok((data, uploaded))
    }

    /// Sort only private generated mesh indices. View changes never rebuild Plot
    /// geometry or vertex streams; equal order performs no device upload.
    pub fn order(
        &mut self,
        key: GeneratedMeshKey,
        projection: Option<[f32; 16]>,
    ) -> Result<usize, RenderError> {
        let entry = self.entries.get_mut(&key).ok_or(RenderError::MissingMesh)?;
        let source = entry.source.upgrade().ok_or(RenderError::MissingMesh)?;
        let mesh = &source.meshes[entry.mesh_index];
        let indices =
            variant_indices(mesh, key.3).map_err(|e| RenderError::RenderDevice(e.to_string()))?;
        let changed = entry.order.update(&mesh.positions, indices, projection);
        if !changed {
            return Ok(0);
        }
        let gpu = entry.data.gpu()?.ok_or(RenderError::MissingMesh)?;
        let uploaded = std::mem::size_of_val(entry.order.indices.as_slice());
        let result = self
            .device
            .borrow_mut()
            .reorder_mesh_indices(&gpu, &entry.order.indices);
        drop(gpu);
        if let Err(error) = result {
            // A failed upload must not leave the cached order claiming success.
            self.entries.remove(&key);
            return Err(error);
        }
        Ok(uploaded)
    }
}

#[cfg(test)]
fn encode(mesh: &PlotMesh) -> Result<Vec<u8>, ipp_core::ErrorReason> {
    encode_variant(mesh, PlotMeshVariant::Default, false)
}

fn encode_variant(
    mesh: &PlotMesh,
    variant: PlotMeshVariant,
    custom: bool,
) -> Result<Vec<u8>, ipp_core::ErrorReason> {
    let material = mesh.material.as_ref();
    let (colors, normals, uvs) = if variant == PlotMeshVariant::Data && custom {
        let material = material.ok_or(ipp_core::ErrorReason::InvalidValue)?;
        (&material.colors, &material.normals, material.uvs.as_slice())
    } else {
        (&mesh.colors, &mesh.normals, &[][..])
    };
    let indices = variant_indices(mesh, variant)?;
    if mesh.positions.is_empty()
        || mesh.positions.len() > 65535
        || indices.is_empty()
        || !indices.len().is_multiple_of(3)
        || !colors.is_empty() && colors.len() != mesh.positions.len()
        || !normals.is_empty() && normals.len() != mesh.positions.len()
        || !uvs.is_empty() && uvs.len() != mesh.positions.len()
    {
        return Err(ipp_core::ErrorReason::InvalidValue);
    }
    let streams = 1
        + u32::from(!colors.is_empty())
        + u32::from(!normals.is_empty())
        + u32::from(!uvs.is_empty());
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"IPPM");
    for value in [
        3,
        mesh.positions.len() as u32,
        indices.len() as u32,
        streams,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for (semantic, values) in [(0u8, &mesh.positions), (1, colors)] {
        if !values.is_empty() {
            bytes.extend_from_slice(&[semantic, 1, 0, 0]);
            bytes.extend_from_slice(&((values.len() * 12) as u32).to_le_bytes());
        }
    }
    if !uvs.is_empty() {
        bytes.extend_from_slice(&[2, 2, 0, 0]);
        bytes.extend_from_slice(&((uvs.len() * 8) as u32).to_le_bytes());
    }
    if !normals.is_empty() {
        bytes.extend_from_slice(&[4, 1, 0, 0]);
        bytes.extend_from_slice(&((normals.len() * 12) as u32).to_le_bytes());
    }
    for values in [&mesh.positions, colors] {
        for value in values {
            for lane in value {
                bytes.extend_from_slice(&lane.to_le_bytes());
            }
        }
    }
    for value in uvs {
        for lane in value {
            bytes.extend_from_slice(&lane.to_le_bytes());
        }
    }
    for value in normals {
        for lane in value {
            bytes.extend_from_slice(&lane.to_le_bytes());
        }
    }
    for index in indices {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    Ok(bytes)
}

fn variant_indices(
    mesh: &PlotMesh,
    variant: PlotMeshVariant,
) -> Result<&[u16], ipp_core::ErrorReason> {
    Ok(match variant {
        PlotMeshVariant::Default => &mesh.indices,
        PlotMeshVariant::Data => {
            &mesh
                .material
                .as_ref()
                .ok_or(ipp_core::ErrorReason::InvalidValue)?
                .data_indices
        }
        PlotMeshVariant::Decoration => {
            &mesh
                .material
                .as_ref()
                .ok_or(ipp_core::ErrorReason::InvalidValue)?
                .decoration_indices
        }
    })
}

#[cfg(test)]
#[path = "generated_meshes_tests.rs"]
mod tests;
