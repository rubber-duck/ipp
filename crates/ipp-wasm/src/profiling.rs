//! Host-only profiling exports, omitted by ordinary builds.

#[global_allocator]
static ALLOCATOR: ipp_core::profiling::CountingAllocator = ipp_core::profiling::CountingAllocator;

// SAFETY: Unique diagnostic export; immutable static name remains live forever.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_name_ptr(index: usize) -> *const u8 {
    if index < 32 {
        ipp_core::profiling::system_name(index).as_ptr()
    } else {
        std::ptr::null()
    }
}

// SAFETY: Unique diagnostic export; only the length of an immutable static name.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_name_len(index: usize) -> usize {
    if index < 32 {
        ipp_core::profiling::system_name(index).len()
    } else {
        0
    }
}

// SAFETY: Unique diagnostic export; resets owned counters, no World mutation.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_reset(enabled: u32) {
    ipp_core::profiling::reset(enabled != 0);
}

// SAFETY: Unique diagnostic export; bounded read of atomic counters only.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_counter(index: usize) -> u64 {
    if index < 768 {
        ipp_core::profiling::counter(index)
    } else {
        0
    }
}

// SAFETY: Unique diagnostic export; observes counters without exposing memory.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_allocations(bytes: u32) -> u64 {
    let values = ipp_core::profiling::allocations();
    if bytes == 0 {
        values.0
    } else {
        values.1
    }
}

// SAFETY: Unique diagnostic export; bounds-checked immutable static name access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_category_name_ptr(index: usize) -> *const u8 {
    if index < 256 {
        ipp_core::profiling::category_name(index).as_ptr()
    } else {
        std::ptr::null()
    }
}

// SAFETY: Unique diagnostic export; scalar length of an immutable static name.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_category_name_len(index: usize) -> usize {
    if index < 256 {
        ipp_core::profiling::category_name(index).len()
    } else {
        0
    }
}

// SAFETY: Unique diagnostic export; bounds-checked atomic counter read only.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_category_counter(index: usize) -> u64 {
    if index < 512 {
        ipp_core::profiling::category_counter(index)
    } else {
        0
    }
}

// SAFETY: Unique diagnostic export; changes only measurement counters' enabled flag.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_pause() {
    ipp_core::profiling::pause();
}
