use super::*;
use crate::lighting::MAX_DOMAIN_LIGHTS;
use objc2_foundation::NSString;
use std::{ffi::c_void, ptr::NonNull};

pub(super) const OPAQUE: usize = 0;
pub(super) const SPRITE_ADDITIVE: usize = 2;
pub(super) const ANALYTIC: usize = 3;
pub(super) const LAYERS: usize = 4;
pub(super) const AFFINE: usize = 5;
pub(super) const PATH: usize = 6;
pub(super) const IMAGE: usize = 7;
pub(super) const COLUMN_SHAPE: usize = 8;
pub(super) const COLUMN_PATH: usize = 9;
pub(super) const CYLINDER_SHAPE: usize = 10;
pub(super) const CYLINDER_PATH: usize = 11;
pub(super) const PLOT_OPAQUE: usize = 12;
pub(super) const PLOT_ALPHA: usize = 13;
pub(super) const MASKED_PATH: usize = 14;
pub(super) const SCREEN_PAINT: usize = 15;
pub(super) const SPRITE_ALPHA: usize = 16;
pub(super) const COLUMN_SCREEN_PAINT: usize = 17;
pub(super) const ROOT: MTLPixelFormat = MTLPixelFormat::RGBA8Unorm_sRGB;
pub(super) const LINEAR: MTLPixelFormat = MTLPixelFormat::RGBA16Float;

pub(crate) struct Programs {
    variants: Vec<[Option<Pipeline>; 2]>,
    pub(super) shadow: Option<Pipeline>,
}

impl Programs {
    pub(super) fn get(&self, mode: usize, root: bool) -> &Pipeline {
        self.variants[mode][usize::from(!root)]
            .as_ref()
            .expect("prepared pipeline variant")
    }

    pub(super) fn new(
        gpu: &ProtocolObject<dyn MTLDevice>,
        library: &ProtocolObject<dyn MTLLibrary>,
        modes: &[usize],
        mesh: bool,
    ) -> Result<Self, String> {
        let mut variants = vec![[None, None]; COLUMN_SCREEN_PAINT + 1];
        for &mode in modes {
            for (i, format) in [ROOT, LINEAR].into_iter().enumerate() {
                if mode == LAYERS && i == 0 {
                    continue;
                }
                let desc = MTLRenderPipelineDescriptor::new();
                let canvas = (ANALYTIC..=CYLINDER_PATH).contains(&mode)
                    || matches!(mode, MASKED_PATH | SCREEN_PAINT | COLUMN_SCREEN_PAINT);
                #[cfg(feature = "instrumentation")]
                desc.setLabel(Some(if canvas {
                    objc2_foundation::ns_string!("IPP Canvas")
                } else {
                    objc2_foundation::ns_string!("IPP meshes")
                }));
                let (vertex, fragment) = match mode {
                    ANALYTIC | COLUMN_SHAPE | CYLINDER_SHAPE => {
                        ("canvas_intersection_vertex", "canvas_fragment")
                    }
                    COLUMN_PATH | CYLINDER_PATH | PATH => {
                        ("canvas_intersection_vertex", "canvas_path_fragment")
                    }
                    LAYERS => ("canvas_vertex", "canvas_layer_fragment"),
                    AFFINE => ("canvas_vertex", "canvas_affine_fragment"),
                    SPRITE_ALPHA | SPRITE_ADDITIVE => ("sprite_vertex", "sprite_fragment"),
                    SCREEN_PAINT | COLUMN_SCREEN_PAINT => {
                        ("canvas_intersection_vertex", "canvas_screen_fragment")
                    }
                    MASKED_PATH => ("canvas_vertex", "canvas_read_path_mask"),
                    IMAGE => ("canvas_vertex", "canvas_image_fragment"),
                    PLOT_OPAQUE | PLOT_ALPHA => ("plot_color_vertex", "plot_color_fragment"),
                    _ if mesh => ("custom_vertex", "custom_fragment"),
                    _ => ("mesh_vertex", "mesh_fragment"),
                };
                desc.setVertexFunction(Some(&*function(library, vertex)?));
                let frag = if canvas && mode != MASKED_PATH {
                    let constants = MTLFunctionConstantValues::new();
                    for (i, v) in [
                        matches!(mode, COLUMN_SHAPE | COLUMN_PATH | COLUMN_SCREEN_PAINT),
                        matches!(mode, COLUMN_SHAPE | CYLINDER_SHAPE),
                        matches!(mode, CYLINDER_SHAPE | CYLINDER_PATH),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        // SAFETY: Metal copies one live MSL bool for each declared function constant.
                        unsafe {
                            constants.setConstantValue_type_atIndex(
                                NonNull::from_ref(&v).cast::<c_void>(),
                                MTLDataType::Bool,
                                i,
                            )
                        };
                    }
                    library
                        .newFunctionWithName_constantValues_error(
                            &NSString::from_str(fragment),
                            &constants,
                        )
                        .map_err(|e| e.to_string())?
                } else {
                    function(library, fragment)?
                };
                desc.setFragmentFunction(Some(&frag));
                // SAFETY: Both attachment indices are valid Metal color attachment slots.
                let color = unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) };
                color.setPixelFormat(format);
                if mode == LAYERS {
                    // SAFETY: The second attachment of a layered pass holds the owner mask.
                    unsafe { desc.colorAttachments().objectAtIndexedSubscript(1) }
                        .setPixelFormat(MTLPixelFormat::R32Uint);
                }
                color.setBlendingEnabled(!matches!(mode, OPAQUE | PLOT_OPAQUE));
                color.setSourceRGBBlendFactor(if canvas {
                    MTLBlendFactor::One
                } else {
                    MTLBlendFactor::SourceAlpha
                });
                color.setSourceAlphaBlendFactor(MTLBlendFactor::One);
                let dest = if mode == SPRITE_ADDITIVE {
                    MTLBlendFactor::One
                } else {
                    MTLBlendFactor::OneMinusSourceAlpha
                };
                color.setDestinationRGBBlendFactor(dest);
                color.setDestinationAlphaBlendFactor(dest);
                desc.setDepthAttachmentPixelFormat(MTLPixelFormat::Depth32Float);
                variants[mode][i] = Some(
                    gpu.newRenderPipelineStateWithDescriptor_error(&desc)
                        .map_err(|e| e.to_string())?,
                );
            }
        }
        Ok(Self {
            variants,
            shadow: None,
        })
    }
}

pub(super) fn compile(
    gpu: &ProtocolObject<dyn MTLDevice>,
    source: &str,
) -> Result<Gpu<dyn MTLLibrary>, String> {
    let options = MTLCompileOptions::new();
    if objc2::available!(macos = 15.0) {
        options.setMathMode(MTLMathMode::Relaxed);
    }
    let text = NSString::from_str(&format!(
        "#define IPP_MAX_DOMAIN_LIGHTS {MAX_DOMAIN_LIGHTS}\n{source}"
    ));
    gpu.newLibraryWithSource_options_error(&text, Some(&options))
        .map_err(|e| e.to_string())
}

pub(super) fn function(
    lib: &ProtocolObject<dyn MTLLibrary>,
    name: &str,
) -> Result<Gpu<dyn MTLFunction>, String> {
    lib.newFunctionWithName(&NSString::from_str(name))
        .ok_or_else(|| format!("Missing Metal function {name}"))
}

pub(super) fn simple_pipeline(
    gpu: &ProtocolObject<dyn MTLDevice>,
    lib: &ProtocolObject<dyn MTLLibrary>,
    vertex: &str,
    frag: Option<&str>,
    color: Option<MTLPixelFormat>,
    depth: bool,
) -> Result<Pipeline, String> {
    let d = MTLRenderPipelineDescriptor::new();
    #[cfg(feature = "instrumentation")]
    d.setLabel(Some(&NSString::from_str(vertex)));
    d.setVertexFunction(Some(&*function(lib, vertex)?));
    if let Some(name) = frag {
        d.setFragmentFunction(Some(&*function(lib, name)?));
    }
    if let Some(format) = color {
        // SAFETY: Slot zero is the single color attachment of this pipeline.
        unsafe { d.colorAttachments().objectAtIndexedSubscript(0) }.setPixelFormat(format);
    }
    if depth {
        d.setDepthAttachmentPixelFormat(MTLPixelFormat::Depth32Float);
    }
    gpu.newRenderPipelineStateWithDescriptor_error(&d)
        .map_err(|e| e.to_string())
}
