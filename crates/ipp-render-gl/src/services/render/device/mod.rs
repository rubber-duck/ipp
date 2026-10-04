#[cfg(feature = "instrumentation")]
mod gl_call_counts;
#[cfg(feature = "instrumentation")]
mod gpu_queries;
#[cfg(feature = "instrumentation")]
pub use gl_call_counts::RenderGlCallCounts;
#[cfg(all(feature = "instrumentation", target_arch = "wasm32"))]
mod webgl_gpu_queries;
#[cfg(feature = "instrumentation")]
pub use gpu_queries::{RenderGpuAvailability, RenderGpuCapability, RenderGpuQueryToken};

mod error_checks;
mod retained_records;
mod uniform_cache;

use crate::RenderError;

/// Renderer-private atlas ranges needed to draw one quadratic path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfacePathDescriptor {
    /// First curve texel and texel count, including contour terminators, in the shared atlas.
    pub curve_range: [u32; 2],
    /// First of the path's band header texels.
    pub band_offset: u32,
}

impl SurfacePathDescriptor {
    /// Construct one validated-at-draw descriptor.
    pub fn new(curve_range: [u32; 2], band_offset: u32) -> Self {
        Self {
            curve_range,
            band_offset,
        }
    }
}

/// One glyph/path instance packed for a contiguous Surface draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfacePathInstance {
    /// Local path bounds.
    pub bounds: [f32; 4],
    /// Local translation and scale.
    pub placement: [f32; 4],
    /// Straight linear RGBA.
    pub color: [f32; 4],
    /// Atlas curve/band lookup.
    pub descriptor: SurfacePathDescriptor,
}

pub(super) fn pack_surface_instances(
    instances: &[SurfacePathInstance],
    packed: &mut Vec<[f32; 16]>,
) {
    packed.clear();
    packed.extend(instances.iter().map(|instance| {
        [
            instance.bounds[0],
            instance.bounds[1],
            instance.bounds[2],
            instance.bounds[3],
            instance.placement[0],
            instance.placement[1],
            instance.placement[2],
            instance.placement[3],
            instance.color[0],
            instance.color[1],
            instance.color[2],
            instance.color[3],
            instance.descriptor.curve_range[0] as f32,
            instance.descriptor.curve_range[1] as f32,
            instance.descriptor.band_offset as f32,
            0.0,
        ]
    }));
}

pub use super::gui_records::{GuiRecord, GuiRecordKind};

pub(super) fn surface_instances_exact(instances: &[SurfacePathInstance]) -> bool {
    const MAX_EXACT_F32_INTEGER: u32 = 1 << 24;
    instances.iter().all(|instance| {
        instance.descriptor.curve_range[0] <= MAX_EXACT_F32_INTEGER
            && instance.descriptor.curve_range[1] <= MAX_EXACT_F32_INTEGER
            && instance.descriptor.band_offset <= MAX_EXACT_F32_INTEGER
    })
}

#[cfg(test)]
mod surface_instance_tests {
    use super::*;

    #[test]
    fn floating_instance_descriptors_reject_the_first_inexact_integer() {
        let instance = |band_offset| SurfacePathInstance {
            bounds: [0.0; 4],
            placement: [0.0; 4],
            color: [0.0; 4],
            descriptor: SurfacePathDescriptor::new([1 << 24, 1], band_offset),
        };
        assert!(surface_instances_exact(&[instance(1 << 24)]));
        assert!(!surface_instances_exact(&[instance((1 << 24) + 1)]));
    }

    #[test]
    fn packing_reuses_warmed_storage() {
        let instance = SurfacePathInstance {
            bounds: [1.0, 2.0, 3.0, 4.0],
            placement: [5.0, 6.0, 7.0, 8.0],
            color: [0.1, 0.2, 0.3, 0.4],
            descriptor: SurfacePathDescriptor::new([9, 10], 11),
        };
        let instances = vec![instance; 256];
        let mut packed = Vec::new();
        pack_surface_instances(&instances, &mut packed);
        let capacity = packed.capacity();
        let pointer = packed.as_ptr();

        pack_surface_instances(&instances[..8], &mut packed);
        assert_eq!(packed.as_ptr(), pointer);
        pack_surface_instances(&instances[..200], &mut packed);

        assert_eq!(packed.capacity(), capacity);
        assert_eq!(packed.as_ptr(), pointer);
        assert_eq!(packed.len(), 200);
        assert_eq!(
            packed[0][..12],
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 0.1, 0.2, 0.3, 0.4]
        );
        assert_eq!(packed[0][12..], [9.0, 10.0, 11.0, 0.0]);
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod gles;

#[cfg(target_arch = "wasm32")]
mod webgl;

#[cfg(target_arch = "wasm32")]
mod webgl_texture_readback;

#[cfg(not(target_arch = "wasm32"))]
pub use gles::GlesRenderDevice;

#[cfg(target_arch = "wasm32")]
pub use webgl::WebGlRenderDevice;

/// Compile-time GL device for the current target.
#[cfg(not(target_arch = "wasm32"))]
pub type PlatformRenderDevice = GlesRenderDevice;

/// Compile-time GL device for the current target.
#[cfg(target_arch = "wasm32")]
pub type PlatformRenderDevice = WebGlRenderDevice;

/// Largest drawing-buffer size the attached device accepts, per axis: the minimum of
/// `MAX_VIEWPORT_DIMS`, `MAX_RENDERBUFFER_SIZE` and `MAX_TEXTURE_SIZE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewportLimits {
    /// Largest drawing-buffer width in pixels.
    pub max_width: u32,
    /// Largest drawing-buffer height in pixels.
    pub max_height: u32,
}

/// The GL operations used by the shared GL renderer, statically dispatched.
///
/// Implementations copy uploads synchronously and retain no borrowed CPU views.
/// A device belongs to one context. Hosts keep that context current and discard
/// its renderer on context loss before creating a replacement after restoration.
pub trait RenderDevice: 'static {
    /// Begin physical-call counting on this device; unsupported devices remain explicit.
    #[cfg(feature = "instrumentation")]
    fn gl_calls_start(&mut self) -> bool {
        false
    }

    /// Retained context-owned physical-call snapshot; absent when unsupported.
    #[cfg(feature = "instrumentation")]
    fn gl_calls_snapshot(&self) -> Option<RenderGlCallCounts> {
        None
    }

    /// Actual bridge loss callback clock, when observation froze before Rust saw loss.
    #[cfg(feature = "instrumentation")]
    fn gl_calls_loss_end(&self) -> Option<u64> {
        None
    }

    /// Stop counting without changing ordinary rendering statistics.
    #[cfg(feature = "instrumentation")]
    fn gl_calls_stop(&mut self) {}

    /// Optional query capability; never controls ordinary rendering.
    #[cfg(feature = "instrumentation")]
    fn gpu_capability(&self) -> RenderGpuCapability {
        RenderGpuCapability::Unsupported
    }

    /// Issue an asynchronous timer scope owned by this device context.
    #[cfg(feature = "instrumentation")]
    fn gpu_start(&mut self) -> Result<RenderGpuQueryToken, RenderGpuAvailability> {
        Err(RenderGpuAvailability::Unsupported)
    }

    /// End the issued scope without waiting for its result.
    #[cfg(feature = "instrumentation")]
    fn gpu_end(&mut self, _token: RenderGpuQueryToken) {}

    /// Read one available result only after checking completion; pending never blocks.
    #[cfg(feature = "instrumentation")]
    fn gpu_poll(&mut self, _token: RenderGpuQueryToken) -> RenderGpuAvailability {
        RenderGpuAvailability::Unsupported
    }

    /// Invalidate and retire all query resources; lost contexts abandon names.
    #[cfg(feature = "instrumentation")]
    fn gpu_stop(&mut self, _reason: RenderGpuAvailability) {}

    /// Context-owned linked program and uniform locations.
    type Program;
    /// Context-owned vertex array, vertex/index buffers and draw count.
    type Mesh;

    /// Context-owned sRGB color texture.
    type Texture: Clone;

    /// Owned staging/fence state; handles may be retired only by their creating context.
    type TextureReadback;

    /// Context-owned immutable quadratic path acceleration data.
    type SurfacePath;

    /// Context-owned SRGB8_ALPHA8 texture and framebuffer holding one
    /// premultiplied whole-Surface image.
    type SurfaceCacheTarget;

    /// Context-owned retained instance stream for one analytic path run.
    type SurfaceInstances;

    /// Context-owned depth texture and framebuffer for the bounded spot shadow pass.
    type ShadowMap;

    /// Context-owned retained GUI storage of one Surface: shape or glyph records,
    /// one per instanced quad ([`GuiRecordKind`]).
    type GuiBatch;

    /// Context-owned glyph atlas page texture and framebuffer target.
    type GlyphAtlasPage;

    /// Enable exhaustive error polling for diagnostics: every draw, uniform,
    /// stream and retained-batch replacement and every frame boundary checks,
    /// attributing an error to the failing call.
    ///
    /// Otherwise allocations, compilation, resource uploads and kept passes
    /// check when they finish, and the frame end checks on a sampled subset of
    /// frames and after retained-storage replacement, so other errors may be
    /// reported at a later frame end. Context loss is still reported within the
    /// frame in which the device observes it.
    fn set_exhaustive_draw_checks(&mut self, _enabled: bool) {}

    /// Largest drawing-buffer size this context accepts; `None` while the context
    /// cannot report limits, such as after loss.
    fn viewport_limits(&self) -> Option<ViewportLimits>;

    /// Bind per-frame lights and per-draw model/material uniforms.
    fn set_lighting(
        &mut self,
        program: &Self::Program,
        model: &[f32; 16],
        normal: &[f32; 16],
        surface: &[f32; 3],
        frame: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError>;

    /// Maximum square atlas dimension supported by this device and viewport.
    fn shadow_map_limit(&self) -> u32 {
        0
    }

    /// Allocate a depth-only 2D map, checking limits and framebuffer completeness.
    fn create_shadow_map(&mut self, size: u32) -> Result<Self::ShadowMap, RenderError>;

    /// Save the host target and render an atlas tile. Slot zero clears the atlas.
    fn begin_shadow(
        &mut self,
        map: &Self::ShadowMap,
        slot: u32,
        grid: u32,
    ) -> Result<(), RenderError>;

    /// Restore the host framebuffer/viewport even after an unsuccessful depth draw.
    fn end_shadow(&mut self) -> Result<(), RenderError>;

    /// Bind the completed map for sampling in a lit forward program.
    fn bind_shadow(
        &mut self,
        program: &Self::Program,
        map: &Self::ShadowMap,
        frame: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError>;

    /// Release both map objects, tolerating invalid handles after context loss.
    fn delete_shadow_map(&mut self, map: Self::ShadowMap);

    /// Validate pass limits and reserve parameter storage without uploading draw values.
    fn prepare_custom_parameters(
        &mut self,
        _program: &Self::Program,
        _words: usize,
        _textures: usize,
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "custom shader parameters unavailable".into(),
        ))
    }

    /// Upload renderer-packed std140 words and bind named texture parameters.
    fn set_custom_parameters<'a>(
        &mut self,
        _program: &Self::Program,
        _words: &[u32],
        _textures: impl ExactSizeIterator<Item = Result<(&'a str, &'a Self::Texture), RenderError>>,
        _alpha_mode: u32,
        _alpha_cutoff: f32,
    ) -> Result<(), RenderError>
    where
        Self::Texture: 'a,
    {
        Err(RenderError::RenderDevice(
            "custom shader parameters unavailable".into(),
        ))
    }

    /// Built-in mesh opacity; ordinary authored meshes use one. Programs without
    /// this uniform ignore it. Values remain instance inputs, never geometry uploads.
    fn set_mesh_opacity(
        &mut self,
        _program: &Self::Program,
        opacity: f32,
    ) -> Result<(), RenderError> {
        if opacity == 1.0 {
            Ok(())
        } else {
            Err(RenderError::RenderDevice("mesh opacity unavailable".into()))
        }
    }

    /// Select ordinary opaque depth writes or straight-alpha blending.
    fn set_alpha_blend(&mut self, enabled: bool) -> Result<(), RenderError> {
        if enabled {
            Err(RenderError::RenderDevice(
                "alpha blending unavailable".into(),
            ))
        } else {
            Ok(())
        }
    }

    /// Select double-sided rasterization only for Surface primitive submission.
    ///
    /// Disabling this mode restores ordinary counter-clockwise front faces with
    /// back-face culling for mesh draws.
    fn set_surface_double_sided(&mut self, _enabled: bool) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "double-sided Surface rendering unavailable".into(),
        ))
    }

    /// Upload the textures of a packed path atlas; see [`crate::pack_surface_paths`].
    fn create_surface_path(
        &mut self,
        _texels: &super::surface_path::SurfacePathTexels,
    ) -> Result<Self::SurfacePath, RenderError> {
        Err(RenderError::RenderDevice(
            "surface paths unavailable".into(),
        ))
    }

    /// Draw one path in painter order with scene depth testing and no depth writes.
    #[allow(clippy::too_many_arguments)]
    fn draw_surface_path(
        &mut self,
        _program: &Self::Program,
        _path: &Self::SurfacePath,
        _bounds: &[f32; 4],
        _descriptor: SurfacePathDescriptor,
        _mvp: &[f32; 16],
        _placement: &[f32; 4],
        _clip: &[f32; 4],
        _color: &[f32; 4],
        _fill_rule: u32,
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "surface paths unavailable".into(),
        ))
    }

    /// Release one context-owned path allocation.
    fn delete_surface_path(&mut self, _path: Self::SurfacePath) {}

    /// Validate `instances` against `path` and upload them as a retained stream.
    /// `instances` is not empty.
    fn create_surface_instances(
        &mut self,
        _path: &Self::SurfacePath,
        _instances: &[SurfacePathInstance],
    ) -> Result<Self::SurfaceInstances, RenderError> {
        Err(RenderError::RenderDevice(
            "surface instancing unavailable".into(),
        ))
    }

    /// Replace a retained stream's complete contents through storage replacement.
    ///
    /// Queued draws keep the previous storage. On failure the contents are unknown
    /// and callers release the stream.
    fn update_surface_instances(
        &mut self,
        _stream: &mut Self::SurfaceInstances,
        _path: &Self::SurfacePath,
        _instances: &[SurfacePathInstance],
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "surface instancing unavailable".into(),
        ))
    }

    /// Release a retained stream, tolerating handles invalidated by context loss.
    fn delete_surface_instances(&mut self, _stream: Self::SurfaceInstances) {}

    /// Draw every instance of a retained stream against `path` in one submission.
    fn draw_surface_instances(
        &mut self,
        _program: &Self::Program,
        _path: &Self::SurfacePath,
        _stream: &Self::SurfaceInstances,
        _mvp: &[f32; 16],
        _clip: &[f32; 4],
        _fill_rule: u32,
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "surface instancing unavailable".into(),
        ))
    }

    /// Draw one straight-alpha bitmap quad in painter order.
    fn draw_surface_bitmap(
        &mut self,
        _program: &Self::Program,
        _texture: &Self::Texture,
        _mvp: &[f32; 16],
        _placement: &[f32; 4],
        _clip: &[f32; 4],
        _color: &[f32; 4],
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "surface bitmaps unavailable".into(),
        ))
    }

    /// Largest Surface cache target dimension this context supports; zero
    /// makes every opted-in Surface present directly.
    fn surface_cache_limit(&self) -> u32 {
        0
    }

    /// Bind a linear-color target with independent depth for a nested Camera.
    /// Ends through `end_surface_cache_target`, without display conversion.
    fn begin_camera_target(
        &mut self,
        _target: &mut Self::SurfaceCacheTarget,
        _clear: &[f32; 4],
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "Camera targets unavailable".into(),
        ))
    }

    /// Allocate a target cleared to transparent black, with linear filtering
    /// and edge clamping, and validate framebuffer completeness. Dimensions
    /// are positive and no larger than [`Self::surface_cache_limit`].
    fn create_surface_cache_target(
        &mut self,
        _width: u32,
        _height: u32,
    ) -> Result<Self::SurfaceCacheTarget, RenderError> {
        Err(RenderError::RenderDevice(
            "Surface cache targets unavailable".into(),
        ))
    }

    /// Reallocate the target's storage in place with undefined contents.
    ///
    /// On failure the target is unusable and callers delete it.
    fn resize_surface_cache_target(
        &mut self,
        _target: &mut Self::SurfaceCacheTarget,
        _width: u32,
        _height: u32,
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "Surface cache targets unavailable".into(),
        ))
    }

    /// Save the current draw/read targets, viewport and Surface antialiasing
    /// viewport, bind the target at its full size with scissor, stencil and
    /// depth writes disabled, and clear it to transparent black.
    ///
    /// Surface draws until [`Self::end_surface_cache_target`] use the target's
    /// dimensions for antialiasing. Glyph atlas population may nest inside: its
    /// end restores this target rather than the host target.
    fn begin_surface_cache_target(
        &mut self,
        _target: &Self::SurfaceCacheTarget,
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "Surface cache targets unavailable".into(),
        ))
    }

    /// Restore the state saved by [`Self::begin_surface_cache_target`], also
    /// after failed draws, then check errors so a failed repaint is never kept
    /// as a complete image. Context loss reports [`RenderError::ContextLost`].
    fn end_surface_cache_target(&mut self) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "Surface cache targets unavailable".into(),
        ))
    }

    /// Composite a premultiplied image over Surface content `[0, 0, size]` at
    /// `mvp`, scaled by root-clip coverage and opacity, with premultiplied blending, scene
    /// depth testing and no depth writes. Callers select double-sided
    /// rasterization as for direct Surface draws. Canvas cache opacity is already
    /// baked per primitive and supplies one; Camera images supply their slot opacity.
    fn draw_surface_cache(
        &mut self,
        _program: &Self::Program,
        _target: &Self::SurfaceCacheTarget,
        _mvp: &[f32; 16],
        _size: &[f32; 2],
        _clip: &[f32; 4],
        _opacity: f32,
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "Surface cache targets unavailable".into(),
        ))
    }

    /// Release a target, tolerating invalid handles after context loss.
    fn delete_surface_cache_target(&mut self, _target: Self::SurfaceCacheTarget) {}

    /// Allocate retained GUI storage for `capacity` records of `kind`, all zero.
    fn create_gui_batch(
        &mut self,
        _kind: GuiRecordKind,
        _capacity: usize,
    ) -> Result<Self::GuiBatch, RenderError> {
        Err(RenderError::RenderDevice("gui batches unavailable".into()))
    }

    /// Write `records` into the storage starting at record `first`. The storage
    /// was created for records of their kind.
    ///
    /// Queued draws keep reading the previous contents; GL may copy or wait to
    /// provide that. On failure the contents are unknown and callers release the
    /// storage. GL errors surface here only in exhaustive mode, otherwise at this
    /// frame's end.
    fn write_gui_batch<R: GuiRecord>(
        &mut self,
        _batch: &mut Self::GuiBatch,
        _first: usize,
        _records: &[R],
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice("gui batches unavailable".into()))
    }

    /// Release one context-owned GUI storage allocation.
    fn delete_gui_batch(&mut self, _batch: Self::GuiBatch) {}

    /// Draw `count` records from record `first` of retained GUI storage as instanced
    /// quads of six vertices each, in record order, each clipped by its own
    /// rectangle. `program` is the canvas program for shape storage and the glyph
    /// program, sampling `atlas`, for glyph storage.
    #[allow(clippy::too_many_arguments)]
    fn draw_gui_batch(
        &mut self,
        _program: &Self::Program,
        _batch: &Self::GuiBatch,
        _atlas: Option<&Self::Texture>,
        _mvp: &[f32; 16],
        _first: usize,
        _count: usize,
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice("gui batches unavailable".into()))
    }

    /// Upload `blocks` into `program`'s `u_paint_blocks` array from its first
    /// vector: the custom paint parameters of the canvas whose GUI batches it
    /// draws next. Callers pass at most
    /// [`CANVAS_PAINT_VECTORS`](crate::CANVAS_PAINT_VECTORS) vectors and only to a
    /// canvas program with paints.
    fn set_gui_paint_blocks(
        &mut self,
        _program: &Self::Program,
        blocks: &[[f32; 4]],
    ) -> Result<(), RenderError> {
        if blocks.is_empty() {
            Ok(())
        } else {
            Err(RenderError::RenderDevice(
                "canvas paints unavailable".into(),
            ))
        }
    }

    /// Allocate a single-channel R8 coverage page texture, cleared to zero, with
    /// linear filtering and a render target.
    fn create_glyph_atlas_page(
        &mut self,
        _width: u32,
        _height: u32,
    ) -> Result<Self::GlyphAtlasPage, RenderError> {
        Err(RenderError::RenderDevice("glyph atlas unavailable".into()))
    }

    /// Release an atlas page allocation.
    fn delete_glyph_atlas_page(&mut self, _page: Self::GlyphAtlasPage) {}

    /// Bind the atlas page framebuffer and viewport.
    ///
    /// The first begin saves the host draw target. Further begins before
    /// [`Self::end_glyph_atlas_page`] switch pages and keep that saved target.
    fn begin_glyph_atlas_page(&mut self, _page: &Self::GlyphAtlasPage) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice("glyph atlas unavailable".into()))
    }

    /// Restore the host draw target and viewport saved by the first begin.
    fn end_glyph_atlas_page(&mut self) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice("glyph atlas unavailable".into()))
    }

    /// Borrow the atlas page's underlying color texture for sampling.
    fn glyph_atlas_texture(page: &Self::GlyphAtlasPage) -> &Self::Texture;

    /// Replace the transient instance stream; empty restores ordinary draws.
    fn set_instances(&mut self, instances: &[[f32; 20]]) -> Result<(), RenderError> {
        if instances.is_empty() {
            Ok(())
        } else {
            Err(RenderError::RenderDevice("Instancing unavailable".into()))
        }
    }

    /// Select additive sprite blending after enabling transparency.
    fn set_additive(&mut self, enabled: bool) -> Result<(), RenderError> {
        if enabled {
            Err(RenderError::RenderDevice(
                "Additive blending unavailable".into(),
            ))
        } else {
            Ok(())
        }
    }

    /// Compile and link a program; delete partial objects on failure.
    fn create_program(
        &mut self,
        vertex: &str,
        fragment: &str,
    ) -> Result<Self::Program, RenderError>;

    /// Upload only retained attribute streams and triangle indices.
    fn create_mesh(&mut self, asset: &ipp_core::MeshAsset) -> Result<Self::Mesh, RenderError>;

    /// Upload top-left-first packed RGBA8 pixels with sRGB color and linear straight alpha.
    fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Self::Texture, RenderError>;

    /// Allocate private SRGB8_ALPHA8 storage without uploading a whole CPU image.
    fn allocate_texture(&mut self, width: u32, height: u32) -> Result<Self::Texture, RenderError>;

    /// Upload complete packed rows. The input borrow ends before returning.
    fn upload_texture_rows(
        &mut self,
        texture: &Self::Texture,
        width: u32,
        first_row: u32,
        rows: u32,
        pixels: &[u8],
    ) -> Result<(), RenderError>;

    /// Whether this device implements bounded asynchronous texture staging.
    fn texture_readback_supported(&self) -> bool {
        false
    }

    /// Submit texture storage to a pixel-pack buffer and flush its fence.
    fn begin_texture_readback(
        &mut self,
        _texture: &Self::Texture,
        _width: u32,
        _height: u32,
    ) -> Result<Self::TextureReadback, RenderError> {
        Err(RenderError::RenderDevice(
            "GPU texture export unsupported".into(),
        ))
    }

    /// Check the submitted fence with zero timeout, without waiting for drawing.
    fn poll_texture_readback(
        &mut self,
        _readback: &Self::TextureReadback,
    ) -> Result<bool, RenderError> {
        Err(RenderError::RenderDevice(
            "GPU texture export unsupported".into(),
        ))
    }

    /// Copy an already completed, checked byte range directly into final output.
    fn copy_texture_readback(
        &mut self,
        _readback: &Self::TextureReadback,
        _offset: usize,
        _destination: &mut [u8],
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "GPU texture export unsupported".into(),
        ))
    }

    /// Release staging only while its creating context is current.
    fn delete_texture_readback(&mut self, _readback: Self::TextureReadback) {}

    /// Set viewport, opaque depth/culling state and clear the host target.
    fn begin_frame(&mut self, width: u32, height: u32, clear: &[f32; 4])
    -> Result<(), RenderError>;

    /// Upload one validated mesh-local joint palette before drawing its instance.
    fn set_skin_palette(
        &mut self,
        _program: &Self::Program,
        _palette: &[[f32; 16]],
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "device does not support skinning".into(),
        ))
    }

    /// Draw one indexed instance with final effective scene values.
    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        program: &Self::Program,
        mesh: &Self::Mesh,
        mvp: &[f32; 16],
        material: &[f32; 3],
        pose: Option<(&Self::Mesh, f32)>,
        texture: Option<&Self::Texture>,
    ) -> Result<(), RenderError>;

    /// Present the frame, then check submission errors on sampled frames (see
    /// [`Self::set_exhaustive_draw_checks`]) and report context loss on every
    /// frame. GPU completion/capture belongs to the host.
    fn end_frame(&mut self) -> Result<(), RenderError>;

    /// Release the mesh, or discard its invalid handles after context loss.
    fn delete_mesh(&mut self, mesh: Self::Mesh);

    /// Release this context's texture, tolerating invalid handles after loss.
    fn delete_texture(&mut self, texture: Self::Texture);

    /// Release the program, or discard its invalid handles after context loss.
    fn delete_program(&mut self, program: Self::Program);
}
