use super::{BoxBounds, bvh::GeometryBvh};
use crate::{EntityId, systems::geometry::GeometryEnclosure};

/// The query contract is independent of the selected acceleration structure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GeometrySpatialBackend {
    /// Contiguous exhaustive scan with the same conservative query predicates.
    Flat,
    /// Packed, refittable bounding-volume hierarchy.
    #[default]
    Bvh,
}

/// Compact published copy of component-owned results; entity slots are generation checked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryPreparedBounds {
    /// Stable component owner, independent of index topology.
    pub entity: EntityId,
    /// Generated visual enclosure used for light influence estimation.
    pub visual: Option<GeometryEnclosure>,
    /// Proven conservative enclosure; absence keeps rendering eligible.
    pub culling: Option<GeometryEnclosure>,
}

/// GeometrySystem-owned index, immutable for the duration of consumer queries.
#[derive(Default)]
pub struct GeometrySpatialIndex {
    pub(super) rows: Vec<Option<GeometryPreparedBounds>>,
    pub(super) active: Vec<usize>,
    pub(super) unknown: Vec<usize>,
    pub(super) backend: GeometrySpatialBackend,
    pub(super) tree: GeometryBvh,
    topology_dirty: bool,
    bounds_dirty: bool,
}

impl GeometrySpatialIndex {
    /// Selected acceleration implementation.
    pub fn backend(&self) -> GeometrySpatialBackend {
        self.backend
    }

    /// Borrow compact results without component or entity-tree lookup.
    pub fn get(&self, entity: EntityId) -> Option<&GeometryPreparedBounds> {
        self.rows
            .get(entity.index() as usize)?
            .as_ref()
            .filter(|row| row.entity == entity)
    }

    pub(in crate::world) fn set_backend(&mut self, backend: GeometrySpatialBackend) {
        if self.backend != backend {
            self.backend = backend;
            self.topology_dirty = true;
        }
    }

    pub(in crate::world) fn invalidate(&mut self) {
        self.rows.fill(None);
        self.active.clear();
        self.unknown.clear();
        self.topology_dirty = true;
    }

    pub(in crate::world) fn publish(&mut self, row: GeometryPreparedBounds) {
        let slot = row.entity.index() as usize;
        if self.rows.len() <= slot {
            self.rows.resize(slot + 1, None);
        }
        let previous = self.rows[slot];
        if previous == Some(row) {
            return;
        }
        self.topology_dirty |= previous
            .is_none_or(|p| p.entity != row.entity || p.culling.is_some() != row.culling.is_some());
        self.bounds_dirty |= previous.and_then(|p| p.culling) != row.culling;
        self.rows[slot] = Some(row);
    }

    pub(in crate::world) fn finish(&mut self) {
        if self.topology_dirty {
            self.active.clear();
            self.unknown.clear();
            for (slot, row) in self.rows.iter().enumerate() {
                if let Some(row) = row {
                    if row.culling.is_some() {
                        self.active.push(slot);
                    } else {
                        self.unknown.push(slot);
                    }
                }
            }
            if self.backend == GeometrySpatialBackend::Bvh {
                self.tree.rebuild(&self.rows, &self.active);
            }
        } else if self.bounds_dirty && self.backend == GeometrySpatialBackend::Bvh {
            self.tree.refit(&self.rows, &self.active);
        }
        self.topology_dirty = false;
        self.bounds_dirty = false;
    }

    pub(super) fn box_at(&self, slot: usize) -> BoxBounds {
        self.rows[slot]
            .expect("indexed component")
            .culling
            .expect("bounded leaf")
            .bounds
    }
}
