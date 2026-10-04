//! Thread-transfer bounds for native operations and browser-local adapters.

/// Native operations can move between execution contexts.
#[cfg(not(target_arch = "wasm32"))]
pub trait IoPlatformSend: Send {}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + ?Sized> IoPlatformSend for T {}

/// Browser operations may retain worker-local state.
#[cfg(target_arch = "wasm32")]
pub trait IoPlatformSend {}

#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> IoPlatformSend for T {}

/// Native immutable backing can be shared across independent readers.
#[cfg(not(target_arch = "wasm32"))]
pub trait IoPlatformSync: Sync {}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync + ?Sized> IoPlatformSync for T {}

/// Browser backing is shared within its owning worker.
#[cfg(target_arch = "wasm32")]
pub trait IoPlatformSync {}

#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> IoPlatformSync for T {}
