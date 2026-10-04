//! Owned typed snapshots and explicit semantic output; font and drawing formats currently
//! have no semantic encoder. Renderer metadata is never a complete CPU payload.

mod snapshot;
pub(super) use snapshot::shared_cpu_data;
pub use snapshot::{AssetCpuSnapshot, AssetWorkingAvailability};

mod output;
pub use output::{AssetOutput, AssetOutputObserver, encode_cpu_snapshot};

mod encoding;
pub use encoding::{cpu_formats, write_texture_header};

/// Portable semantic format, never a device object or renderer packing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetExportFormat {
    /// IPPM version 3 authored streams.
    MeshV3,
    /// IPPT version 3 top-row-first sRGB RGBA8.
    TextureV3,
    /// IPPS version 1 parent/rest transforms.
    SkeletonV1,
    /// IPPP version 1 joint-local transforms.
    PoseV1,
    /// IPPB version 1 inverse-bind matrices.
    SkinV1,
    /// IPPH version 3 complete authored backend sources.
    ShaderV3,
    /// IPPA version 4 immutable typed keyframes.
    AnimationV4,
    /// IPPG version 1 shape definitions.
    GeometryV1,
    /// IPPC version 1 sampled particle frames.
    ParticleCacheV1,
    /// IPPE version 1 logical expression declaration.
    ExpressionV1,
}

/// Owned Host-local semantic export operation; graphics commands stay on the Host thread.
pub type AssetExportFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, String>> + 'static>>;
