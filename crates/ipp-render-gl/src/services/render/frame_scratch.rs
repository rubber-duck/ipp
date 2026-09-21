//! Reusable CPU preparation storage shared by sequential World submissions.
//! Entries are frame-local indices; no World references or GPU borrows survive a call.

use ipp_core::{DebugRenderItem, RenderItem};

#[derive(Clone, Copy)]
pub(super) enum RenderDrawIndex {
    Visual(usize),
    Debug(usize),
    #[cfg(feature = "surfaces")]
    Surface(usize),
}

pub(super) enum RenderDrawItem<'a> {
    Visual(&'a RenderItem),
    Debug(&'a DebugRenderItem),
    #[cfg(feature = "surfaces")]
    Surface(&'a ipp_core::SurfaceRenderItem),
}

impl RenderDrawIndex {
    pub fn resolve<'a>(
        self,
        items: &'a [RenderItem],
        debug: &'a [DebugRenderItem],
        #[cfg(feature = "surfaces")] surfaces: &'a [ipp_core::SurfaceRenderItem],
    ) -> RenderDrawItem<'a> {
        match self {
            Self::Visual(index) => RenderDrawItem::Visual(&items[index]),
            Self::Debug(index) => RenderDrawItem::Debug(&debug[index]),
            #[cfg(feature = "surfaces")]
            Self::Surface(index) => RenderDrawItem::Surface(&surfaces[index]),
        }
    }

    pub fn order(self) -> usize {
        match self {
            Self::Visual(index) | Self::Debug(index) => index,
            #[cfg(feature = "surfaces")]
            Self::Surface(index) => index,
        }
    }
}

pub(super) struct RenderDraw {
    pub index: RenderDrawIndex,
    pub key: (ipp_core::EntityId, u8),
    pub depth: f64,
    pub material: super::draw_order::RenderMaterialKey,
    pub phase: u8,
}

impl RenderDraw {
    pub fn compare(&self, other: &Self) -> std::cmp::Ordering {
        self.phase
            .cmp(&other.phase)
            .then_with(|| match self.phase {
                0 => self
                    .material
                    .cmp(&other.material)
                    .then_with(|| self.depth.total_cmp(&other.depth)),
                2 => other.depth.total_cmp(&self.depth),
                _ => std::cmp::Ordering::Equal,
            })
            .then_with(|| self.key.cmp(&other.key))
            .then_with(|| self.index.order().cmp(&other.index.order()))
    }
}

#[cfg(feature = "shadows")]
pub(super) struct RenderShadowCaster {
    pub item: usize,
    pub config: super::shader::RenderShaderConfig,
}

#[derive(Default)]
pub(super) struct RenderFrameScratch {
    pub draws: Vec<RenderDraw>,
    #[cfg(feature = "shadows")]
    pub casters: Vec<RenderShadowCaster>,
    #[cfg(feature = "shadows")]
    pub shadow_frusta: Vec<[ipp_core::systems::geometry::GeometryPlane; 6]>,
    #[cfg(feature = "shadows")]
    pub shadow_visibility: Vec<bool>,
    #[cfg(feature = "shadows")]
    pub shadow_queries: Vec<usize>,
    #[cfg(feature = "particles")]
    pub instances: Vec<[f32; 20]>,
    #[cfg(feature = "surfaces")]
    pub surface_instances: Vec<super::device::SurfacePathInstance>,
}

impl RenderFrameScratch {
    pub fn clear(&mut self) {
        self.draws.clear();
        #[cfg(feature = "shadows")]
        {
            self.casters.clear();
            self.shadow_frusta.clear();
            self.shadow_visibility.clear();
        }
        #[cfg(feature = "particles")]
        self.instances.clear();
        #[cfg(feature = "surfaces")]
        {
            self.surface_instances.clear();
        }
    }
}

#[cfg(all(test, feature = "surfaces"))]
mod tests {
    use super::*;
    use crate::services::render::device::{SurfacePathDescriptor, SurfacePathInstance};

    #[test]
    fn clear_preserves_surface_buffer_capacity() {
        let mut scratch = RenderFrameScratch::default();
        let instance = SurfacePathInstance {
            bounds: [0.0; 4],
            placement: [0.0; 4],
            color: [1.0; 4],
            descriptor: SurfacePathDescriptor::new([0, 1], 0),
        };
        scratch.surface_instances.extend([instance; 256]);
        let instance_capacity = scratch.surface_instances.capacity();
        let pointer = scratch.surface_instances.as_ptr();

        scratch.clear();
        scratch.surface_instances.extend([instance; 200]);

        assert_eq!(scratch.surface_instances.capacity(), instance_capacity);
        assert_eq!(scratch.surface_instances.as_ptr(), pointer);
    }
}
