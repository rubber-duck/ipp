use super::*;

#[derive(Default)]
pub(crate) struct ObjectTransformRuntime {
    world: std::cell::UnsafeCell<Option<GeometryShapeTransform>>,
}

impl ObjectTransformRuntime {
    pub(in crate::world) fn clear(&mut self) {
        *self.world.get_mut() = None;
    }

    pub(in crate::world) fn value(&self) -> Option<GeometryShapeTransform> {
        // SAFETY: Runtime reads borrow the World phase; propagation and lifetime
        // invalidation require its exclusive borrow and cannot overlap this read.
        unsafe { *self.world.get() }
    }
}

#[derive(Clone, Copy)]
pub(in crate::world) struct ObjectTransformBinding {
    identity: usize,
    runtime: std::ptr::NonNull<Option<GeometryShapeTransform>>,
}

impl ObjectTransformBinding {
    /// Invalidate this binding before its entity generation is released.
    pub(in crate::world) unsafe fn bind(world: &WorldSimulationState, entity: EntityId) -> Self {
        Self {
            identity: world.identity,
            runtime: std::ptr::NonNull::new(
                world
                    .state
                    .links
                    .transform(entity)
                    .expect("prepared entity transform")
                    .world
                    .get(),
            )
            .expect("occupied transform cell"),
        }
    }

    pub(in crate::world) fn borrow<'a>(
        &self,
        world: &'a WorldSimulationState,
    ) -> Option<Result<&'a GeometryShapeTransform, ErrorReason>> {
        assert_eq!(self.identity, world.identity);
        // SAFETY: The owning System invalidates before entity release. The boxed
        // runtime never moves while occupied; this shared World phase excludes
        // propagation, mutation and release for the returned borrow's lifetime.
        let runtime = unsafe { self.runtime.as_ref() };
        Some(runtime.as_ref().ok_or(ErrorReason::InvalidValue))
    }

    pub(in crate::world) fn evaluate(
        &self,
        world: &WorldSimulationState,
    ) -> Result<GeometryShapeTransform, ErrorReason> {
        self.borrow(world).expect("bound transform").copied()
    }

    pub(super) fn write(
        &self,
        world: &mut WorldSimulationState,
        value: Option<GeometryShapeTransform>,
    ) {
        assert_eq!(self.identity, world.identity);
        let mut runtime = self.runtime;
        // SAFETY: Propagation owns the exclusive World phase and retains no
        // result borrows across writes. The compiled list is invalidated before
        // this entity's stable boxed runtime is released or its slot reused.
        unsafe {
            *runtime.as_mut() = value;
        }
    }
}
