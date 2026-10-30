//! Reusable scene buffers. Only empty buffers cross a publication borrow boundary.

use super::scene::{
    RenderEntity, SceneDebug, SceneGeometry, SceneItem, SceneOutputSurface, ScenePlotPlane,
};
use ipp_core::services::asset_management::AssetKey;
use ipp_core::{EntityId, WorldPublicationId, WorldRef};
use std::{cell::RefCell, rc::Rc};

pub(in crate::services::render) type SceneStoragePool = Rc<RefCell<Vec<SceneStorage<'static>>>>;

#[derive(Default)]
pub(in crate::services::render) struct SceneStorage<'a> {
    pub traversal: ipp_core::PublishedSceneTraversal,
    pub items: Vec<SceneItem<'a>>,
    pub debug: Vec<SceneDebug>,
    pub lights: Vec<(RenderEntity, [f32; 16], ipp_core::components::Light)>,
    pub geometry: Vec<((WorldRef, EntityId), SceneGeometry<'a>)>,
    pub surfaces: Vec<SceneOutputSurface>,
    pub plot_planes: Vec<ScenePlotPlane<'a>>,
    pub resources: Vec<(AssetKey, WorldPublicationId)>,
}

impl SceneStorage<'_> {
    pub fn empty<'a>(mut self) -> SceneStorage<'a> {
        self.debug.clear();
        self.lights.clear();
        self.surfaces.clear();
        self.resources.clear();
        SceneStorage {
            traversal: self.traversal,
            items: empty_buffer(self.items),
            geometry: empty_buffer(self.geometry),
            plot_planes: empty_buffer(self.plot_planes),
            debug: self.debug,
            lights: self.lights,
            surfaces: self.surfaces,
            resources: self.resources,
        }
    }
}

// Drop every borrowed value before changing the buffer's element lifetime. The
// empty in-place Vec collection reuses its allocation for equal-layout types;
// correctness does not depend on that optimization and no lifetime is extended.
fn empty_buffer<T, U>(mut values: Vec<T>) -> Vec<U> {
    debug_assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<U>());
    debug_assert_eq!(std::mem::align_of::<T>(), std::mem::align_of::<U>());
    values.clear();
    values
        .into_iter()
        .map(|_| unreachable!("cleared buffer"))
        .collect()
}

#[cfg(test)]
#[path = "scene_storage_tests.rs"]
mod tests;
