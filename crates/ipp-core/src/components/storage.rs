//! Stable typed component slots.

use std::{
    cell::{Cell, UnsafeCell},
    mem::MaybeUninit,
};

const PAGE_SIZE: usize = 64;

pub(crate) struct Paged<T, R = ()> {
    pages: Vec<Box<[Slot<T, R>]>>,
}

struct Slot<T, R> {
    runtime: UnsafeCell<R>,
    value: UnsafeCell<MaybeUninit<T>>,
    occupied: Cell<bool>,
}

impl<T, R> Drop for Slot<T, R> {
    fn drop(&mut self) {
        if self.occupied.get() {
            // SAFETY: Only occupied slots contain initialized T. Teardown owns the
            // storage exclusively and no binding is dereferenced during destruction.
            unsafe { self.value.get_mut().assume_init_drop() };
        }
    }
}

impl<T, R> Default for Paged<T, R> {
    fn default() -> Self {
        Self {
            pages: Vec::new(),
        }
    }
}

impl<T, R: Default> Paged<T, R> {
    pub(crate) fn reserve(&mut self, slots: usize) {
        self.try_reserve(slots)
            .expect("component storage allocation failed");
    }

    pub(crate) fn try_reserve(&mut self, slots: usize) -> Result<(), crate::ErrorReason> {
        let count = slots.div_ceil(PAGE_SIZE);
        self.pages
            .try_reserve(count.saturating_sub(self.pages.len()))
            .map_err(|_| crate::ErrorReason::Capacity)?;
        while self.pages.len() < count {
            let mut page = Vec::new();
            page.try_reserve_exact(PAGE_SIZE)
                .map_err(|_| crate::ErrorReason::Capacity)?;
            page.resize_with(PAGE_SIZE, || Slot {
                runtime: UnsafeCell::new(R::default()),
                value: UnsafeCell::new(MaybeUninit::uninit()),
                occupied: Cell::new(false),
            });
            self.pages.push(page.into_boxed_slice());
        }
        Ok(())
    }

    /// Bind from the cell allocation, never from a temporary exclusive T borrow.
    /// Dereferencing requires the caller's World phase and incarnation barrier.
    pub(crate) fn bound_ptr(&self, index: usize) -> Option<std::ptr::NonNull<T>> {
        let slot = self.pages.get(index / PAGE_SIZE)?.get(index % PAGE_SIZE)?;
        slot.occupied.get().then(|| {
            // SAFETY: The cell is allocated and aligned for T. Its boxed page does
            // not move during growth. Occupancy is checked before publishing access.
            unsafe { std::ptr::NonNull::new_unchecked(slot.value.get().cast::<T>()) }
        })
    }

    /// Private derived state shares the occupied slot lifetime, but is not part
    /// of authored values, animation samples or serialization.
    pub(crate) fn runtime_ptr(&self, index: usize) -> Option<std::ptr::NonNull<R>> {
        let slot = self.pages.get(index / PAGE_SIZE)?.get(index % PAGE_SIZE)?;
        slot.occupied.get().then(|| {
            // SAFETY: R is initialized with its stable boxed cell. The owning
            // component incarnation and phase borrow bound every use of this pointer.
            unsafe { std::ptr::NonNull::new_unchecked(slot.runtime.get()) }
        })
    }

    #[allow(dead_code)] // Some generated runtime accessors use only pointers.
    pub(crate) fn runtime(&self, index: usize) -> Option<&R> {
        let pointer = self.runtime_ptr(index)?;
        // SAFETY: Runtime shares this storage borrow and occupied slot lifetime.
        Some(unsafe { pointer.as_ref() })
    }

    pub(crate) fn runtime_mut(&mut self, index: usize) -> Option<&mut R> {
        let mut pointer = self.runtime_ptr(index)?;
        // SAFETY: Occupied runtime cells stay stable until removal. This exclusive
        // storage borrow prevents other cell references or invalidation while the
        // returned reference is live.
        Some(unsafe { pointer.as_mut() })
    }

    pub(crate) fn get(&self, index: usize) -> Option<&T> {
        let pointer = self.bound_ptr(index)?;
        // SAFETY: The occupied slot is initialized and shared for this borrow.
        // Compiled writes require exclusive World access, excluding this reference.
        Some(unsafe { pointer.as_ref() })
    }

    #[allow(dead_code)] // Generated accessors depend on selected systems.
    pub(crate) fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        let mut pointer = self.bound_ptr(index)?;
        // SAFETY: Exclusive storage access prevents overlapping component borrows
        // or compiled writes. The reference cannot outlive this storage borrow.
        Some(unsafe { pointer.as_mut() })
    }

    pub(crate) fn set(&mut self, index: usize, value: Option<T>) {
        let slot = &self.pages[index / PAGE_SIZE][index % PAGE_SIZE];
        if slot.occupied.get() {
            slot.occupied.set(false);
            // SAFETY: Old T is initialized, with no active component borrow.
            // Incarnation changes invalidate bindings before this storage operation.
            unsafe { slot.value.get().cast::<T>().drop_in_place() };
        }
        // SAFETY: Exclusive storage access excludes runtime borrows. Binding
        // owners invalidate before incarnation changes; the runtime cell stays put.
        unsafe { slot.runtime.get().replace(R::default()) };
        if let Some(value) = value {
            // SAFETY: The cell is uninitialized and exclusively owned for this
            // write. Same-incarnation numeric replacement preserves its allocation.
            unsafe { slot.value.get().cast::<T>().write(value) };
            slot.occupied.set(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occupied_addresses_survive_growth_and_neighbor_reuse() {
        let mut pages = Paged::<_, ()>::default();
        pages.reserve(64);
        pages.set(0, Some(17_u64));
        let address = pages.get(0).unwrap() as *const u64;
        pages.reserve(4096);
        for index in 1..4096 {
            pages.set(index, Some(index as u64));
        }
        for index in 1..4096 {
            pages.set(index, None);
            pages.set(index, Some(9));
        }
        assert_eq!(address, pages.get(0).unwrap() as *const u64);
        assert_eq!(pages.get(0), Some(&17));
    }

    #[test]
    fn cell_origin_bindings_survive_temporary_borrows_and_numeric_replacement() {
        let mut pages = Paged::<_, ()>::default();
        pages.reserve(64);
        pages.set(0, Some([1.0_f32, 2.0, 3.0, 4.0]));
        let pointer = pages.bound_ptr(0).unwrap();
        assert_eq!(pages.get(0).unwrap()[0], 1.0);
        pages.get_mut(0).unwrap()[1] = 8.0;
        pages.reserve(4096);
        pages.set(0, Some([4.0, 5.0, 6.0, 7.0]));
        // SAFETY: The same occupied cell survives growth and numeric replacement.
        // All temporary references have ended; this test owns the storage phase.
        unsafe {
            pointer.as_ptr().write([9.0, 10.0, 11.0, 12.0]);
        }
        assert_eq!(pages.get(0), Some(&[9.0, 10.0, 11.0, 12.0]));
        pages.get_mut(0).unwrap()[2] = 15.0;
        // SAFETY: Same live cell and exclusive phase; the prior mutable borrow ended.
        assert_eq!(unsafe { pointer.as_ref() }[2], 15.0);
    }

    #[test]
    fn runtime_companion_survives_growth_and_resets_on_replacement() {
        let mut pages = Paged::<u64, Cell<usize>>::default();
        pages.reserve(64);
        pages.set(0, Some(1));
        let pointer = pages.runtime_ptr(0).unwrap();
        pages.runtime(0).unwrap().set(17);
        pages.reserve(2048);
        assert_eq!(pages.runtime_ptr(0), Some(pointer));
        // SAFETY: The occupied cell remains live across growth; the shared runtime
        // access is scoped to this storage's phase and Cell owns interior mutation.
        assert_eq!(unsafe { pointer.as_ref() }.get(), 17);
        pages.set(0, Some(2));
        assert_eq!(pages.runtime(0).unwrap().get(), 0);
        pages.set(0, None);
        assert!(pages.runtime_ptr(0).is_none());
        pages.set(0, Some(3));
        assert_eq!(pages.runtime(0).unwrap().get(), 0);
        assert_eq!(std::mem::size_of::<crate::components::Transform>(), 40);
    }

    #[test]
    fn slots_drop_owned_values_exactly_once() {
        use std::rc::Rc;
        struct Value(Rc<Cell<usize>>);
        impl Drop for Value {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let drops = Rc::new(Cell::new(0));
        let mut pages = Paged::<_, ()>::default();
        pages.reserve(64);
        pages.set(0, Some(Value(drops.clone())));
        pages.set(0, Some(Value(drops.clone())));
        assert_eq!(drops.get(), 1);
        pages.set(0, None);
        pages.set(0, None);
        assert_eq!(drops.get(), 2);
        pages.set(1, Some(Value(drops.clone())));
        pages.reserve(1024);
        drop(pages);
        assert_eq!(drops.get(), 3);
    }
}
