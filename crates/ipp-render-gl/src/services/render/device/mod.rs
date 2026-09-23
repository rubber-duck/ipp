mod error_checks;
#[cfg(feature = "gui")]
mod retained_vertices;
mod uniform_cache;

use crate::RenderError;

/// Renderer-private atlas ranges needed to draw one quadratic path.
#[cfg(feature = "surfaces")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfacePathDescriptor {
    /// First curve and curve count in the shared atlas.
    pub curve_range: [u32; 2],
    /// First of 32 horizontal/vertical band headers.
    pub band_offset: u32,
}

#[cfg(feature = "surfaces")]
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
#[cfg(feature = "surfaces")]
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

#[cfg(feature = "surfaces")]
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

#[cfg(feature = "gui")]
pub use super::glyph_atlas::GlyphVertex;
#[cfg(feature = "gui")]
pub use super::gui_batch::GuiBoxVertex;

#[cfg(feature = "surfaces")]
pub(super) fn surface_instances_exact(instances: &[SurfacePathInstance]) -> bool {
    const MAX_EXACT_F32_INTEGER: u32 = 1 << 24;
    instances.iter().all(|instance| {
        instance.descriptor.curve_range[0] <= MAX_EXACT_F32_INTEGER
            && instance.descriptor.curve_range[1] <= MAX_EXACT_F32_INTEGER
            && instance.descriptor.band_offset <= MAX_EXACT_F32_INTEGER
    })
}

#[cfg(all(test, feature = "surfaces"))]
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

/// The GL operations used by the shared GL renderer, statically dispatched.
///
/// Implementations copy uploads synchronously and retain no borrowed CPU views.
/// A device belongs to one context. Hosts keep that context current and discard
/// its renderer on context loss before creating a replacement after restoration.
pub trait RenderDevice: 'static {
    /// Context-owned linked program and uniform locations.
    type Program;
    /// Context-owned vertex array, vertex/index buffers and draw count.
    type Mesh;

    /// Context-owned sRGB color texture.
    type Texture;

    /// Context-owned immutable quadratic path acceleration data.
    #[cfg(feature = "surfaces")]
    type SurfacePath;

    /// Context-owned SRGB8_ALPHA8 texture and framebuffer holding one
    /// premultiplied whole-Surface image.
    #[cfg(feature = "surfaces")]
    type SurfaceCacheTarget;

    /// Context-owned depth texture and framebuffer for the bounded spot shadow pass.
    #[cfg(feature = "shadows")]
    type ShadowMap;

    /// Context-owned retained GUI batch buffer and allocation metadata.
    #[cfg(feature = "gui")]
    type GuiBatch;

    /// Context-owned retained glyph batch buffer and allocation metadata.
    #[cfg(feature = "gui")]
    type GlyphBatch;

    /// Context-owned glyph atlas page texture and framebuffer target.
    #[cfg(feature = "gui")]
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
    #[cfg(feature = "shadows")]
    fn shadow_map_limit(&self) -> u32 {
        0
    }

    /// Allocate a depth-only 2D map, checking limits and framebuffer completeness.
    #[cfg(feature = "shadows")]
    fn create_shadow_map(&mut self, size: u32) -> Result<Self::ShadowMap, RenderError>;

    /// Save the host target and render an atlas tile. Slot zero clears the atlas.
    #[cfg(feature = "shadows")]
    fn begin_shadow(
        &mut self,
        map: &Self::ShadowMap,
        slot: u32,
        grid: u32,
    ) -> Result<(), RenderError>;

    /// Restore the host framebuffer/viewport even after an unsuccessful depth draw.
    #[cfg(feature = "shadows")]
    fn end_shadow(&mut self) -> Result<(), RenderError>;

    /// Bind the completed map for sampling in a lit forward program.
    #[cfg(feature = "shadows")]
    fn bind_shadow(
        &mut self,
        program: &Self::Program,
        map: &Self::ShadowMap,
        frame: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError>;

    /// Release both map objects, tolerating invalid handles after context loss.
    #[cfg(feature = "shadows")]
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
    #[cfg(feature = "surfaces")]
    fn set_surface_double_sided(&mut self, _enabled: bool) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "double-sided Surface rendering unavailable".into(),
        ))
    }

    /// Upload renderer-packed quadratic path data. Each segment is
    /// `[start.x,start.y,control.x,control.y]`, `[end.x,end.y,kind,0]`.
    #[cfg(feature = "surfaces")]
    fn create_surface_path(
        &mut self,
        _bounds: &[f32; 4],
        _segments: &[[f32; 8]],
        _bands: &[[u32; 2]],
    ) -> Result<Self::SurfacePath, RenderError> {
        Err(RenderError::RenderDevice(
            "surface paths unavailable".into(),
        ))
    }

    /// Draw one path in painter order with scene depth testing and no depth writes.
    #[cfg(feature = "surfaces")]
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
    #[cfg(feature = "surfaces")]
    fn delete_surface_path(&mut self, _path: Self::SurfacePath) {}

    /// Draw compatible contiguous path instances in one submission.
    #[cfg(feature = "surfaces")]
    fn draw_surface_path_instances(
        &mut self,
        _program: &Self::Program,
        _path: &Self::SurfacePath,
        _instances: &[SurfacePathInstance],
        _mvp: &[f32; 16],
        _clip: &[f32; 4],
        _fill_rule: u32,
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "surface instancing unavailable".into(),
        ))
    }

    /// Draw one straight-alpha bitmap quad in painter order.
    #[cfg(feature = "surfaces")]
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
    #[cfg(feature = "surfaces")]
    fn surface_cache_limit(&self) -> u32 {
        0
    }

    /// Allocate a target cleared to transparent black, with linear filtering
    /// and edge clamping, and validate framebuffer completeness. Dimensions
    /// are positive and no larger than [`Self::surface_cache_limit`].
    #[cfg(feature = "surfaces")]
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
    #[cfg(feature = "surfaces")]
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
    #[cfg(feature = "surfaces")]
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
    #[cfg(feature = "surfaces")]
    fn end_surface_cache_target(&mut self) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "Surface cache targets unavailable".into(),
        ))
    }

    /// Composite a premultiplied image over Surface content `[0, 0, size]` at
    /// `mvp`, scaled by root-clip coverage, with premultiplied blending, scene
    /// depth testing and no depth writes. Callers select double-sided
    /// rasterization as for direct Surface draws.
    #[cfg(feature = "surfaces")]
    fn draw_surface_cache(
        &mut self,
        _program: &Self::Program,
        _target: &Self::SurfaceCacheTarget,
        _mvp: &[f32; 16],
        _size: &[f32; 2],
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "Surface cache targets unavailable".into(),
        ))
    }

    /// Release a target, tolerating invalid handles after context loss.
    #[cfg(feature = "surfaces")]
    fn delete_surface_cache_target(&mut self, _target: Self::SurfaceCacheTarget) {}

    /// Allocate and upload a retained non-indexed GUI triangle batch.
    #[cfg(feature = "gui")]
    fn create_gui_batch(
        &mut self,
        _vertices: &[GuiBoxVertex],
    ) -> Result<Self::GuiBatch, RenderError> {
        Err(RenderError::RenderDevice("gui batches unavailable".into()))
    }

    /// Replace complete batch contents through GPU storage replacement.
    ///
    /// Queued draws keep the previous storage, though allocation may still stall.
    /// On failure the contents are unknown and callers release the batch. GL
    /// errors surface here only in exhaustive mode, otherwise at this frame's end.
    #[cfg(feature = "gui")]
    fn update_gui_batch(
        &mut self,
        _batch: &mut Self::GuiBatch,
        _vertices: &[GuiBoxVertex],
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice("gui batches unavailable".into()))
    }

    /// Release one context-owned GUI batch allocation.
    #[cfg(feature = "gui")]
    fn delete_gui_batch(&mut self, _batch: Self::GuiBatch) {}

    /// Draw one retained GUI triangle batch in painter order with clipping.
    #[cfg(feature = "gui")]
    fn draw_gui_batch(
        &mut self,
        _program: &Self::Program,
        _batch: &Self::GuiBatch,
        _mvp: &[f32; 16],
        _clip: &[f32; 4],
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice("gui batches unavailable".into()))
    }

    /// Allocate and upload a retained glyph quad batch.
    #[cfg(feature = "gui")]
    fn create_glyph_batch(
        &mut self,
        _vertices: &[GlyphVertex],
    ) -> Result<Self::GlyphBatch, RenderError> {
        Err(RenderError::RenderDevice(
            "glyph batches unavailable".into(),
        ))
    }

    /// Replace complete batch contents through GPU storage replacement.
    ///
    /// Queued draws keep the previous storage, though allocation may still stall.
    /// On failure the contents are unknown and callers release the batch. GL
    /// errors surface here only in exhaustive mode, otherwise at this frame's end.
    #[cfg(feature = "gui")]
    fn update_glyph_batch(
        &mut self,
        _batch: &mut Self::GlyphBatch,
        _vertices: &[GlyphVertex],
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "glyph batches unavailable".into(),
        ))
    }

    /// Release one context-owned glyph batch allocation.
    #[cfg(feature = "gui")]
    fn delete_glyph_batch(&mut self, _batch: Self::GlyphBatch) {}

    /// Draw one retained glyph batch in painter order with atlas sampling and clipping.
    #[cfg(feature = "gui")]
    fn draw_glyph_batch(
        &mut self,
        _program: &Self::Program,
        _batch: &Self::GlyphBatch,
        _atlas: &Self::Texture,
        _mvp: &[f32; 16],
        _clip: &[f32; 4],
    ) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice(
            "glyph batches unavailable".into(),
        ))
    }

    /// Allocate an atlas page texture with linear filtering and render target.
    #[cfg(feature = "gui")]
    fn create_glyph_atlas_page(
        &mut self,
        _width: u32,
        _height: u32,
    ) -> Result<Self::GlyphAtlasPage, RenderError> {
        Err(RenderError::RenderDevice("glyph atlas unavailable".into()))
    }

    /// Release an atlas page allocation.
    #[cfg(feature = "gui")]
    fn delete_glyph_atlas_page(&mut self, _page: Self::GlyphAtlasPage) {}

    /// Bind the atlas page framebuffer and viewport.
    ///
    /// The first begin saves the host draw target. Further begins before
    /// [`Self::end_glyph_atlas_page`] switch pages and keep that saved target.
    #[cfg(feature = "gui")]
    fn begin_glyph_atlas_page(&mut self, _page: &Self::GlyphAtlasPage) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice("glyph atlas unavailable".into()))
    }

    /// Restore the host draw target and viewport saved by the first begin.
    #[cfg(feature = "gui")]
    fn end_glyph_atlas_page(&mut self) -> Result<(), RenderError> {
        Err(RenderError::RenderDevice("glyph atlas unavailable".into()))
    }

    /// Borrow the atlas page's underlying color texture for sampling.
    #[cfg(feature = "gui")]
    fn glyph_atlas_texture(page: &Self::GlyphAtlasPage) -> &Self::Texture;

    /// Replace the transient instance stream; empty restores ordinary draws.
    #[cfg(feature = "particles")]
    fn set_instances(&mut self, instances: &[[f32; 20]]) -> Result<(), RenderError> {
        if instances.is_empty() {
            Ok(())
        } else {
            Err(RenderError::RenderDevice("Instancing unavailable".into()))
        }
    }

    /// Select additive sprite blending after enabling transparency.
    #[cfg(feature = "particles")]
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

    /// Set viewport, opaque depth/culling state and clear the host target.
    fn begin_frame(&mut self, width: u32, height: u32, clear: &[f32; 4])
    -> Result<(), RenderError>;

    /// Upload one validated mesh-local joint palette before drawing its instance.
    #[cfg(feature = "skeletal-animation")]
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
        #[cfg(feature = "mesh-poses")] pose: Option<(&Self::Mesh, f32)>,
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
