//! Retained derived path resources using the shared analytic contour packing/device.

use super::loaders::SharedRenderDevice;
use super::paths::pack_surface_paths_with_limit;
use crate::{RenderDevice, RenderError, SurfacePathDescriptor};
use ipp_core::systems::canvas::CanvasPrimitiveId;
use ipp_core::systems::plot::PlotPath;
use ipp_core::{HostRuntime, WorldRef};
use std::{collections::BTreeMap, rc::Rc, sync::Arc};

/// Plane identity is absent for ordinary Canvas content.
pub(in crate::services::render) type GeneratedPathKey = (WorldRef, CanvasPrimitiveId, Option<u32>);

pub(in crate::services::render) struct GeneratedPath<D: RenderDevice> {
    pub source: Arc<PlotPath>,
    pub gpu: Option<D::SurfacePath>,
    pub descriptor: SurfacePathDescriptor,
    device: SharedRenderDevice<D>,
}

pub(in crate::services::render) struct GeneratedPathCache<D: RenderDevice> {
    entries: BTreeMap<GeneratedPathKey, Rc<GeneratedPath<D>>>,
    device: SharedRenderDevice<D>,
}

impl<D: RenderDevice> GeneratedPathCache<D> {
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
            host.world_fault(*world).is_ok() && Arc::strong_count(&entry.source) > 1
        });
    }

    pub fn get(
        &mut self,
        key: GeneratedPathKey,
        source: &Arc<PlotPath>,
    ) -> Result<(Rc<GeneratedPath<D>>, usize), RenderError> {
        if let Some(entry) = self.entries.get_mut(&key) {
            if Arc::ptr_eq(&entry.source, source) {
                return Ok((entry.clone(), 0));
            }
            if entry.source.as_ref() == source.as_ref()
                && let Some(entry) = Rc::get_mut(entry)
            {
                entry.source = source.clone();
                return Ok((self.entries[&key].clone(), 0));
            }
        }
        // Derived contours already use the Canvas +Y-down coordinates. The shared
        // analytic shader evaluates supplied coordinates directly; no source SVG/font
        // normalization or second Y flip applies to these runtime paths.
        #[cfg(feature = "instrumentation")]
        let _trace = ipp_core::profiling::CpuTraceScope::new("RenderService.path-upload");
        let atlas = pack_surface_paths_with_limit(
            [(source.bounds, source.contours.as_ref())],
            self.device.borrow().surface_path_texture_limit(),
        );
        let uploaded = atlas.texels.byte_len();
        if let Some(mut previous) = self.entries.remove(&key)
            && let Some(entry) = Rc::get_mut(&mut previous)
            && let Some(gpu) = entry.gpu.as_mut()
            && self
                .device
                .borrow_mut()
                .update_surface_path(gpu, &atlas.texels)?
        {
            entry.source = source.clone();
            entry.descriptor = atlas.descriptors[0];
            self.entries.insert(key, previous.clone());
            return Ok((previous, uploaded));
        }
        let gpu = self
            .device
            .borrow_mut()
            .create_surface_path(&atlas.texels)?;
        let entry = Rc::new(GeneratedPath {
            source: source.clone(),
            gpu: Some(gpu),
            descriptor: atlas.descriptors[0],
            device: self.device.clone(),
        });
        self.entries.insert(key, entry.clone());
        Ok((entry, uploaded))
    }
}

impl<D: RenderDevice> Drop for GeneratedPath<D> {
    fn drop(&mut self) {
        // GL deletion retires storage without invalidating already queued users;
        // no elapsed-frame assumption permits reuse.
        if let Some(gpu) = self.gpu.take() {
            self.device.borrow_mut().delete_surface_path(gpu);
        }
    }
}
