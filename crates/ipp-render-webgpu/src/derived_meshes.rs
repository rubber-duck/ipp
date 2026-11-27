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
    assets::{MeshBuffers, MeshLayout},
    device::Graphics as Device,
};

pub(super) struct DerivedMesh {
    pub(super) buffers: MeshBuffers,
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
                let vertices = ipp_render_scene::mesh::mesh_vertices(&mesh).ok()?;
                Some(DerivedMesh {
                    buffers: MeshBuffers::upload(device, &vertices, mesh.indices())?,
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
    let color = |i, colors: &[[f32; 3]]| colors.get(i).copied().unwrap_or([1.; 3]);
    let buffers = if custom {
        let material = mesh
            .material
            .as_ref()
            .filter(|_| plot.variant == PlotMeshVariant::Data);
        let vertices = mesh
            .positions
            .iter()
            .enumerate()
            .map(|(i, &[x, y, z])| {
                let [r, g, b] = color(i, material.map_or(&mesh.colors, |m| &m.colors));
                let [u, v] = material
                    .and_then(|m| m.uvs.get(i))
                    .copied()
                    .unwrap_or([0.; 2]);
                let [nx, ny, nz] = material
                    .and_then(|m| m.normals.get(i))
                    .copied()
                    .unwrap_or([0.; 3]);
                [x, y, z, r, g, b, u, v, nx, ny, nz]
            })
            .collect::<Vec<_>>();
        MeshBuffers::upload_vertices(device, &vertices, indices, MeshLayout::PaintedPlot)?
    } else {
        let vertices = mesh
            .positions
            .iter()
            .enumerate()
            .map(|(i, &[x, y, z])| {
                let [r, g, b] = color(i, &mesh.colors);
                [x, y, z, r, g, b]
            })
            .collect::<Vec<_>>();
        MeshBuffers::upload_vertices(device, &vertices, indices, MeshLayout::Plot)?
    };

    Some(DerivedMesh {
        buffers,
    })
}
