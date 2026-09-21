//! Indexed immutable assets, resource-owned loading and explicit consumer accounting.
//! Hosts choose execution phases; concrete loaders own decoding and allocation.

use std::{
    any::Any,
    task::{Context, Poll},
};

mod catalog;
mod source_lookup;

mod lifecycle;
pub use lifecycle::{AssetLifecycleEvent, AssetLifecycleKind, AssetReleaseKind};

mod resource;

mod buffered;

pub mod writer;

/// Shared private geometry builders and optional procedural source recipes.
pub mod builtin;

pub use buffered::BufferedAssetLoader;

pub use resource::AssetProvider;

pub use crate::services::data_source::{DataReader, STREAM_CAPACITY};

/// Compiled asset type identity; registration never assigns IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AssetTypeId(pub u16);

/// Runtime slot handle. Copying a key does not retain its resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetKey {
    /// Index in the Host asset slot array.
    pub slot: u32,
    /// Incarnation validated before every payload borrow.
    pub generation: u32,
}

impl AssetKey {
    /// Pack a runtime key for existing Host event and renderer identities.
    pub fn to_u64(self) -> u64 {
        (1 << 63) | (u64::from(self.slot) << 32) | u64::from(self.generation)
    }

    /// Unpack a runtime identity; service lookup still validates the incarnation.
    pub fn from_u64(value: u64) -> Self {
        Self {
            slot: (value >> 32) as u32 & 0x7fff_ffff,
            generation: value as u32,
        }
    }
}

/// Producer-selected identity, separate from a runtime slot handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AssetUploadIdentity {
    /// Compiled payload type.
    pub kind: AssetTypeId,
    /// Nonzero producer-local identity.
    pub asset: u64,
    /// Immutable selected variant.
    pub variant: u32,
}

/// Named immutable input. Source descriptors perform no I/O.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AssetSource {
    /// Expected compiled payload type.
    pub kind: AssetTypeId,
    /// Provider URI naming immutable content.
    pub uri: String,
    /// Immutable selected variant.
    pub variant: u32,
}

/// AssetProvider-owned lifecycle observations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetLoadStatus {
    /// The complete representation is unavailable; retained CPU data may remain usable.
    Unloaded,
    /// Loading has started, including source opening.
    Start,
    /// Input consumed so far; unknown totals remain unknown.
    Progress {
        /// Consumed bytes.
        completed: u64,
        /// Expected bytes, when known.
        total: Option<u64>,
    },
    /// The resource can be used.
    Loaded,
    /// Loading failed without changing identity or content.
    Failed(String),
}

/// Observation forwarded by the manager to the host's event phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetLoadProgress {
    /// Representations at this observation boundary.
    pub representation: AssetRepresentationStatus,
    /// Named input associated with this resource.
    pub source: AssetSource,
    /// Stable resource identity.
    pub key: AssetKey,
    /// Lifecycle observation.
    pub status: AssetLoadStatus,
}

/// Independently observable decoded availability and graphics residency.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AssetRepresentationStatus {
    /// A usable decoded payload is retained, including during graphics recovery.
    pub decoded: bool,
    /// None for CPU-only assets; otherwise whether graphics can be used.
    pub graphics_ready: Option<bool>,
    /// Original encoded bytes consumed by the provider.
    pub source_bytes: u64,
    /// Current retained CPU and measurable GPU storage.
    pub resident_bytes: u64,
    /// Measurable graphics storage, or None for CPU-only assets.
    pub graphics_bytes: Option<u64>,
}

/// Memory accounting independent of payload type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AssetStats {
    /// Encoded input bytes consumed.
    pub source_bytes: u64,
    /// Loaded CPU/GPU data bytes reported by the implementation.
    pub resident_bytes: usize,
}

/// Exclusively owned producer input queued at the mutation boundary.
#[derive(Debug)]
pub struct AssetUpload {
    /// Caller correlation.
    pub id: u64,
    /// Typed immutable identity.
    pub key: AssetUploadIdentity,
    /// Encoded bytes retained as an immutable recovery source.
    pub bytes: Vec<u8>,
}

/// Correlated result when a resource becomes usable or loading fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetUploadOutcome {
    /// Caller correlation.
    pub id: u64,
    /// Stable resource identity.
    pub key: AssetUploadIdentity,
    /// Host frame at completion.
    pub tick: u64,
    /// Loaded sizes or a failure diagnostic.
    pub result: Result<AssetStats, String>,
}

/// Immutable loaded data; graphics implementations free GPU data on drop.
pub trait Asset: Any {
    /// Concrete representation for the consuming subsystem.
    fn as_any(&self) -> &dyn Any;

    /// Decoded representation for headless validation and CPU consumers.
    fn decoded(&self) -> &dyn Any;

    /// Compact CPU information available independently of bulk decoded streams.
    fn metadata(&self) -> &dyn Any {
        self.decoded()
    }

    /// Release graphics allocations while retaining usable immutable CPU data.
    fn invalidate_graphics(&mut self) {}

    /// Graphics availability, independent of retained CPU metadata.
    fn graphics_ready(&self) -> Option<bool> {
        None
    }

    /// Known graphics allocation estimate; excludes opaque driver program storage.
    fn graphics_bytes(&self) -> Option<usize> {
        None
    }

    /// Loaded allocation estimate, excluding the stable object.
    fn resident_bytes(&self) -> usize;
}

/// Incremental decoder/allocation operation owned by one resource.
pub trait AssetLoader: 'static {
    /// Immutable result owned by the resource.
    type Data: Asset;

    /// Retain fully validated CPU data after a representation-allocation failure.
    /// This never changes failed readiness or exposes partial GPU allocations.
    fn take_failed_data(&mut self) -> Option<Self::Data> {
        None
    }

    /// Consume available input without blocking or retaining borrowed buffers.
    fn poll_load(
        &mut self,
        reader: &mut dyn DataReader,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Data, String>>;
}

/// Maximum UTF-8 byte length of an asset failure on all producer and protocol paths.
pub const MAX_ASSET_ERROR_BYTES: usize = 2048;

fn bounded_error(mut error: String) -> String {
    if error.len() > MAX_ASSET_ERROR_BYTES {
        let mut end = MAX_ASSET_ERROR_BYTES;
        while !error.is_char_boundary(end) {
            end -= 1;
        }
        error.truncate(end);
    }
    error
}

pub mod mesh;

pub mod quadratic;

pub mod font;

pub mod drawing;

pub mod mesh_metadata;

pub mod texture;

#[cfg(feature = "skeletal-animation")]
pub mod skeleton;

#[cfg(feature = "skeletal-animation")]
pub mod skin_binding;

pub mod service;

pub use service::AssetManagementService;

/// Immutable backend-specific shader definitions.
pub mod shader;
