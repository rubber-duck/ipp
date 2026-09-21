//! Shared object-transform access, separate from compact authored TRS values.

use super::*;
use crate::{components::registry::ComponentStorage, world::component_binding::ComponentBinding};
use std::cell::OnceCell;

/// Derived data attached to an occupied Transform slot, absent from copied values.
#[derive(Default)]
pub(crate) struct ObjectTransformRuntime {
    pub(in crate::world) affine: OnceCell<Result<GeometryShapeTransform, ErrorReason>>,
}

#[derive(Clone, Copy)]
pub(in crate::world) struct ObjectTransformBinding {
    transform: Option<ComponentBinding<Transform>>,
    runtime: Option<ComponentBinding<ObjectTransformRuntime>>,
    hierarchy: Option<ComponentBinding<Hierarchy>>,
    aim: Option<ComponentBinding<crate::components::LookAt>>,
}

impl ObjectTransformBinding {
    /// The caller discards this binding before any selected component incarnation
    /// ends, and binds again when optional component membership changes.
    pub(in crate::world) unsafe fn bind(storage: &ComponentStorage, entity: EntityId) -> Self {
        let index = entity.index() as usize;
        // SAFETY: The caller owns synchronous incarnation invalidation. Every
        // pointer originates from the same World's stable typed storage cells.
        unsafe {
            Self {
                transform: storage
                    .transform_ptr(index)
                    .map(|p| ComponentBinding::new(p)),
                runtime: storage
                    .transform_runtime_ptr(index)
                    .map(|p| ComponentBinding::new(p)),
                hierarchy: storage
                    .hierarchy_ptr(index)
                    .map(|p| ComponentBinding::new(p)),
                aim: storage.look_at_ptr(index).map(|p| ComponentBinding::new(p)),
            }
        }
    }

    #[inline]
    pub(in crate::world) fn borrow<'a>(
        &self,
        storage: &'a ComponentStorage,
    ) -> Option<Result<&'a GeometryShapeTransform, ErrorReason>> {
        if let Some(hierarchy) = self.hierarchy {
            return Some(
                hierarchy
                    .get(storage)
                    .runtime
                    .world
                    .as_ref()
                    .ok_or(ErrorReason::InvalidValue),
            );
        }
        let runtime = self.runtime?;
        Some(
            runtime
                .get(storage)
                .affine
                .get_or_init(|| self.evaluate_root(storage))
                .as_ref()
                .map_err(|error| *error),
        )
    }

    pub(in crate::world) fn evaluate(
        &self,
        storage: &ComponentStorage,
    ) -> Result<GeometryShapeTransform, ErrorReason> {
        match self.borrow(storage) {
            Some(result) => result.copied(),
            None => self.evaluate_root(storage),
        }
    }

    fn evaluate_root(
        &self,
        storage: &ComponentStorage,
    ) -> Result<GeometryShapeTransform, ErrorReason> {
        let mut transform = self
            .transform
            .map_or_else(Transform::default, |t| *t.get(storage));
        if let Some(aim) = self.aim {
            let aim = aim.get(storage);
            if aim.runtime.invalid {
                return Err(ErrorReason::UnsupportedDependency);
            }
            if let Some(q) = aim.runtime.rotation {
                [transform.qx, transform.qy, transform.qz, transform.qw] = q;
            }
        }
        affine(&transform)
    }
}
