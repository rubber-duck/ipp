use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

use ipp_core::{
    services::asset_management::builtin,
    systems::{
        geometry::GeometryPrimitiveVisual,
        plot::{PlotMeshVariant, PlotPreparedGeometry, PlotPublishedMesh},
    },
};

use crate::{
    assets,
    device::{Device, MeshBuffers},
};

pub(super) struct DerivedMesh {
    pub(super) buffers: MeshBuffers,
    pub(super) triangles: u32,
    pub(super) vertex_mode: f32,
}

type PlotKey = (usize, usize, PlotMeshVariant, bool);

#[derive(Default)]
pub(super) struct DerivedMeshes {
    debug: BTreeMap<[u32; 6], (u64, Option<DerivedMesh>)>,
    plots: BTreeMap<PlotKey, (Weak<PlotPreparedGeometry>, Option<DerivedMesh>)>,
}

impl DerivedMeshes {
    pub(super) fn retain(&mut self, frame: u64) {
        self.debug.retain(|_, (last, _)| frame - *last <= 2);
        self.plots
            .retain(|_, (source, _)| source.strong_count() > 0);
    }

    pub(super) fn debug(
        &mut self,
        device: &Device,
        visual: &GeometryPrimitiveVisual,
        frame: u64,
    ) -> Option<&DerivedMesh> {
        let (last, mesh) = self.debug.entry(visual.mesh_key()).or_insert_with(|| {
            let mesh = builtin::debug_mesh(visual).ok().and_then(|mesh| {
                let triangles = (mesh.indices().len() / 3) as u32;
                let vertices = assets::mesh_vertices(&mesh).ok()?;
                Some(DerivedMesh {
                    buffers: device.mesh(&vertices, mesh.indices())?,
                    triangles,
                    vertex_mode: 0.0,
                })
            });
            (frame, mesh)
        });
        *last = frame;
        mesh.as_ref()
    }

    pub(super) fn plot(
        &mut self,
        device: &Device,
        plot: &PlotPublishedMesh,
        custom: bool,
    ) -> Option<&DerivedMesh> {
        let geometry = &plot.geometry;
        let key = (
            Arc::as_ptr(geometry) as usize,
            plot.mesh_index,
            plot.variant,
            custom,
        );
        self.plots
            .entry(key)
            .or_insert_with(|| (Arc::downgrade(geometry), plot_buffers(device, plot, custom)))
            .1
            .as_ref()
    }
}

fn plot_buffers(device: &Device, plot: &PlotPublishedMesh, custom: bool) -> Option<DerivedMesh> {
    #[cfg(feature = "instrumentation")]
    let _trace = ipp_core::profiling::CpuTraceScope::new("RenderService.mesh-upload");
    let mesh = plot.geometry.meshes.get(plot.mesh_index)?;
    let indices = match plot.variant {
        PlotMeshVariant::Default => &mesh.indices,
        PlotMeshVariant::Data => &mesh.material.as_ref()?.data_indices,
        PlotMeshVariant::Decoration => &mesh.material.as_ref()?.decoration_indices,
    };
    // Keep triangle order for blending. Data and decoration can reference different
    // subsets of one mesh, so use indices only when they reduce the upload.
    let optical = custom && plot.variant == PlotMeshVariant::Data;
    let buffers = if optical {
        let material = mesh.material.as_ref()?;
        let vertices: Vec<_> = indices
            .iter()
            .map(|&i| {
                let i = usize::from(i);
                let [x, y, z] = mesh.positions[i];
                let [r, g, b] = material.colors.get(i).copied().unwrap_or([1.0; 3]);
                let [nx, ny, nz] = material.normals.get(i).copied().unwrap_or([0.0; 3]);
                let [u, v] = material.uvs.get(i).copied().unwrap_or([0.0; 2]);
                [x, y, z, r, g, b, u, v, nx, ny, nz]
            })
            .collect();
        device.plot_triangles(&vertices)?
    } else {
        let vertex = |i: usize| {
            let [x, y, z] = mesh.positions[i];
            let [r, g, b] = mesh.colors.get(i).copied().unwrap_or([1.0; 3]);
            [x, y, z, r, g, b]
        };
        if mesh.positions.len() * size_of::<[f32; 6]>() + size_of_val(indices.as_slice())
            < indices.len() * size_of::<[f32; 6]>()
        {
            let vertices: Vec<_> = (0..mesh.positions.len()).map(vertex).collect();
            device.mesh(&vertices, indices)?
        } else {
            let vertices: Vec<_> = indices.iter().map(|&i| vertex(usize::from(i))).collect();
            device.plot_triangles(&vertices)?
        }
    };

    Some(DerivedMesh {
        buffers,
        triangles: (indices.len() / 3) as u32,
        vertex_mode: if optical {
            -2.0
        } else {
            -1.0
        },
    })
}
