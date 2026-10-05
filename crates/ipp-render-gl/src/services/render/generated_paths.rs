//! Retained derived path resources using the shared analytic contour packing/device.

use super::assets::SharedRenderDevice;
use super::surface_path::pack_surface_paths_with_limit;
use crate::{RenderDevice, RenderError, SurfacePathDescriptor};
use ipp_core::systems::canvas::CanvasPrimitiveId;
use ipp_core::systems::plot::PlotPath;
use ipp_core::{HostRuntime, WorldRef};
use std::{
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Weak},
};

/// Plane identity is absent for ordinary Canvas content.
pub(super) type GeneratedPathKey = (WorldRef, CanvasPrimitiveId, Option<u32>);

pub(super) struct GeneratedPath<D: RenderDevice> {
    pub source: Weak<PlotPath>,
    pub gpu: Option<D::SurfacePath>,
    pub descriptor: SurfacePathDescriptor,
    device: SharedRenderDevice<D>,
}

pub(super) struct GeneratedPathCache<D: RenderDevice> {
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
            host.world_fault(*world).is_ok() && entry.source.strong_count() != 0
        });
    }

    pub fn get(
        &mut self,
        key: GeneratedPathKey,
        source: &Arc<PlotPath>,
    ) -> Result<(Rc<GeneratedPath<D>>, usize), RenderError> {
        if let Some(entry) = self.entries.get(&key).filter(|entry| {
            entry
                .source
                .upgrade()
                .is_some_and(|retained| Arc::ptr_eq(&retained, source))
        }) {
            return Ok((entry.clone(), 0));
        }
        // Derived contours already use the Canvas +Y-down coordinates. The shared
        // analytic shader evaluates supplied coordinates directly; no source SVG/font
        // normalization or second Y flip applies to these runtime paths.
        let atlas = pack_surface_paths_with_limit(
            [(source.bounds, source.contours.as_ref())],
            self.device.borrow().surface_path_texture_limit(),
        );
        let uploaded = atlas.texels.byte_len();
        let gpu = self
            .device
            .borrow_mut()
            .create_surface_path(&atlas.texels)?;
        let entry = Rc::new(GeneratedPath {
            source: Arc::downgrade(source),
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
