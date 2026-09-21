//! Cached component access for the already validated parent order.

use super::*;
use crate::{
    components::LookAt, components::registry::ComponentStorage,
    world::component_binding::ComponentBinding,
};

#[derive(Default)]
pub(super) struct HierarchyPropagation {
    pub(super) valid: bool,
    nodes: Vec<HierarchyNode>,
    aims: Vec<ComponentBinding<LookAt>>,
}

struct HierarchyNode {
    output: ComponentBinding<Hierarchy>,
    local: LocalTransformBinding,
    parent: Option<ParentTransformBinding>,
    invalid: bool,
}

struct LocalTransformBinding {
    transform: Option<ComponentBinding<Transform>>,
    aim: Option<ComponentBinding<LookAt>>,
}

struct ParentTransformBinding {
    hierarchy: Option<ComponentBinding<Hierarchy>>,
    local: LocalTransformBinding,
    bone: u32,
    #[cfg(feature = "skeletal-animation")]
    skeleton: Option<ComponentBinding<crate::components::Skeleton>>,
}

impl LocalTransformBinding {
    fn bind(storage: &ComponentStorage, entity: EntityId) -> Self {
        let index = entity.index() as usize;
        // SAFETY: Pointers originate from stable cells in the owning World.
        // Hierarchy's lifecycle hook clears the entire compiled access list before
        // any referenced component incarnation ends. Reads borrow this same storage.
        unsafe {
            Self {
                transform: storage
                    .transform_ptr(index)
                    .map(|p| ComponentBinding::new(p)),
                aim: storage.look_at_ptr(index).map(|p| ComponentBinding::new(p)),
            }
        }
    }

    fn evaluate(
        &self,
        storage: &ComponentStorage,
        aimed: bool,
    ) -> Result<GeometryShapeTransform, ErrorReason> {
        let mut transform = self
            .transform
            .map_or_else(Transform::default, |p| *p.get(storage));
        if aimed && let Some(aim) = self.aim {
            let aim = aim.get(storage);
            if aim.runtime.invalid {
                return Err(ErrorReason::UnsupportedDependency);
            }
            if let Some(rotation) = aim.runtime.rotation {
                [transform.qx, transform.qy, transform.qz, transform.qw] = rotation;
            }
        }
        affine(&transform)
    }
}

impl ParentTransformBinding {
    fn evaluate(&self, storage: &ComponentStorage) -> Result<GeometryShapeTransform, ErrorReason> {
        let object = match self.hierarchy {
            Some(h) => h
                .get(storage)
                .runtime
                .world
                .ok_or(ErrorReason::InvalidValue)?,
            None => self.local.evaluate(storage, true)?,
        };
        if self.bone == u32::MAX {
            return Ok(object);
        }
        #[cfg(feature = "skeletal-animation")]
        {
            // Pose payloads may suspend/reallocate independently of the stable
            // Skeleton component. Borrow their current buffer only for this read.
            let pose = self
                .skeleton
                .and_then(|s| s.get(storage).runtime.pose.as_ref())
                .filter(|p| p.valid)
                .ok_or(ErrorReason::InvalidAsset)?;
            let joint = pose
                .global
                .get(self.bone as usize)
                .ok_or(ErrorReason::InvalidValue)?;
            GeometryShapeTransform::from_matrix(*joint)?.then(&object)
        }
        #[cfg(not(feature = "skeletal-animation"))]
        Err(ErrorReason::UnsupportedDependency)
    }
}

impl HierarchyPropagation {
    pub(super) fn invalidate(&mut self) {
        self.valid = false;
        self.nodes.clear();
        self.aims.clear();
    }

    pub(super) fn prepare(&mut self, graph: &HierarchyGraph, storage: &ComponentStorage) {
        self.invalidate();
        for &entity in &graph.order {
            let index = entity.index() as usize;
            if let Some(pointer) = storage.look_at_ptr(index) {
                // SAFETY: Same stable-cell/lifecycle invariant as LocalTransformBinding.
                self.aims.push(unsafe { ComponentBinding::new(pointer) });
            }
            let Some(output) = storage.hierarchy_ptr(index) else {
                continue;
            };
            let parent = graph.parents.get(&entity).map(|&parent| {
                let parent_index = parent.index() as usize;
                // SAFETY: Before-commit invalidation clears cached readers before
                // component removal/replacement; numeric writes preserve these slots.
                unsafe {
                    ParentTransformBinding {
                        hierarchy: storage
                            .hierarchy_ptr(parent_index)
                            .map(|p| ComponentBinding::new(p)),
                        local: LocalTransformBinding::bind(storage, parent),
                        bone: storage.hierarchy(index).unwrap().parent_bone,
                        #[cfg(feature = "skeletal-animation")]
                        skeleton: storage
                            .skeleton_ptr(parent_index)
                            .map(|p| ComponentBinding::new(p)),
                    }
                }
            });
            self.nodes.push(HierarchyNode {
                // SAFETY: Cached output is discarded before incarnation destruction;
                // propagation writes through an exclusive owning-storage borrow.
                output: unsafe { ComponentBinding::new(output) },
                local: LocalTransformBinding::bind(storage, entity),
                parent,
                invalid: graph.invalid.contains(&entity),
            });
        }
        self.valid = true;
    }

    pub(super) fn reset_aims(&self, storage: &mut ComponentStorage) {
        for aim in &self.aims {
            let aim = aim.get_mut(storage);
            aim.runtime.rotation = None;
            aim.runtime.invalid = false;
        }
    }

    pub(super) fn propagate(&self, storage: &mut ComponentStorage, aimed: bool) {
        for node in &self.nodes {
            let result = if node.invalid {
                Err(ErrorReason::UnsupportedDependency)
            } else {
                node.local
                    .evaluate(storage, aimed)
                    .and_then(|local| match &node.parent {
                        Some(parent) => local.then(&parent.evaluate(storage)?),
                        None => Ok(local),
                    })
            };
            node.output.get_mut(storage).runtime.world = result.ok();
        }
    }
}
