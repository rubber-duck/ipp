//! Compiled typed access under the World's exclusive evaluation schedule.

use crate::components::registry::ComponentStorage;
use std::ptr::NonNull;

/// A zero-allocation typed location. Construction establishes the lifetime and
/// World identity contract; subsequent access borrows that World's storage.
#[derive(Debug)]
pub(in crate::world) struct ComponentBinding<T>(NonNull<T>);

impl<T> Copy for ComponentBinding<T> {}

impl<T> Clone for ComponentBinding<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> ComponentBinding<T> {
    /// # Safety
    /// The pointer must originate from the stable storage cell, be aligned and
    /// initialized as T. Every copy must be discarded before its incarnation ends.
    /// All accesses must borrow its owning World's storage; no resource/buffer
    /// ownership may be changed through a numeric binding.
    pub(in crate::world) unsafe fn new(pointer: NonNull<T>) -> Self {
        Self(pointer)
    }

    pub(in crate::world) unsafe fn cast<U>(self) -> ComponentBinding<U> {
        ComponentBinding(self.0.cast())
    }

    #[inline]
    pub(in crate::world) fn get<'a>(&self, _storage: &'a ComponentStorage) -> &'a T {
        // SAFETY: Construction establishes the owning storage and invalidation
        // contract. The shared phase borrow excludes writes until this read ends.
        unsafe { self.0.as_ref() }
    }

    #[inline]
    pub(in crate::world) fn get_mut<'a>(&self, _storage: &'a mut ComponentStorage) -> &'a mut T {
        // SAFETY: The binding remains live by its construction contract. Exclusive
        // access to the owning storage excludes every other component reference.
        unsafe { &mut *self.0.as_ptr() }
    }
}
