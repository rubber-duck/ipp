//! World-local generational entity identities.

/// A world-local handle. Hosts additionally fence handles by session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct EntityId(u64);

impl EntityId {
    /// Decode handle bits; liveness is checked by the world, not this constructor.
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// Return the stable wire representation.
    pub const fn to_bits(self) -> u64 {
        self.0
    }

    /// Storage slot index.
    pub const fn index(self) -> u32 {
        self.0 as u32
    }

    /// Slot generation.
    pub const fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }

    pub(crate) const fn new(index: u32, generation: u32) -> Self {
        Self(((generation as u64) << 32) | index as u64)
    }
}

#[derive(Default)]
pub(crate) struct Allocator {
    slots: Vec<(u32, bool)>,
    free: Vec<u32>,
}

impl Allocator {
    pub(crate) fn reserve(&mut self, slots: usize) -> Result<(), crate::ErrorReason> {
        self.slots
            .try_reserve(slots.saturating_sub(self.slots.len()))
            .map_err(|_| crate::ErrorReason::Capacity)?;
        self.free
            .try_reserve(slots.saturating_sub(self.free.len()))
            .map_err(|_| crate::ErrorReason::Capacity)
    }

    pub(crate) fn allocate(&mut self) -> Option<EntityId> {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.1 = true;

            return Some(EntityId::new(index, slot.0));
        }

        let index = u32::try_from(self.slots.len()).ok()?;
        self.slots.push((1, true));

        Some(EntityId::new(index, 1))
    }

    pub(crate) fn contains(&self, id: EntityId) -> bool {
        self.slots
            .get(id.index() as usize)
            .is_some_and(|&(generation, alive)| alive && generation == id.generation())
    }

    pub(crate) fn release(&mut self, id: EntityId) {
        debug_assert!(self.contains(id));
        let slot = &mut self.slots[id.index() as usize];
        slot.1 = false;
        slot.0 = slot.0.wrapping_add(1);
        self.free.push(id.index());
    }

    pub(crate) fn slots(&self) -> usize {
        self.slots.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_generation_does_not_make_a_tombstone_live() {
        let mut allocator = Allocator::default();
        let first = allocator.allocate().unwrap();
        allocator.release(first);
        assert!(!allocator.contains(first));
        assert!(!allocator.contains(EntityId::new(first.index(), first.generation() + 1)));
        assert!(!allocator.contains(EntityId::from_bits(u64::MAX)));
        let next = allocator.allocate().unwrap();
        assert!(allocator.contains(next));
        assert!(allocator.allocate().is_some());
    }
}
