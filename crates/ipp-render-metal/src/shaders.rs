//! Backend selection, authored paint compilation and immutable paint binding.

use std::{any::Any, fmt::Write, rc::Rc};

use ipp_core::{
    HostRuntime, WorldPublicationId,
    services::asset_management::{
        Asset, AssetLoadFailure, AssetLoader, AsyncAssetLoader,
        formats::{
            ipp_shader::{IppShaderTarget, lower, validate_paint},
            shader::{ShaderDefinition, ShaderParameterKind},
        },
    },
    systems::canvas::CanvasPaintInstance,
};

use crate::{
    custom_mesh,
    device::{Device, Object, PaintParameters},
};

pub(super) const BACKEND: &str = "msl";
const MAX_PAINT_PARAMETERS: usize = 16;

pub(super) struct MetalShader {
    definition: ShaderDefinition,
    program: Option<Object>,
}

impl MetalShader {
    pub(super) fn definition(&self) -> &ShaderDefinition {
        &self.definition
    }

    pub(super) fn program(&self) -> Option<&Object> {
        self.program.as_ref()
    }
}

impl Asset for MetalShader {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        &self.definition
    }

    fn invalidate_graphics(&mut self) {
        self.program = None;
    }

    fn graphics_ready(&self) -> Option<bool> {
        Some(self.program.is_some())
    }

    fn resident_bytes(&self) -> usize {
        self.definition.resident_bytes()
    }
}

pub(super) fn loader(device: Rc<Device>) -> impl AssetLoader<Data = MetalShader> {
    AsyncAssetLoader::new(move |mut source| async move {
        let definition = ShaderDefinition::decode_reader(&mut *source).await?;
        let result = if definition.is_paint() {
            compile_paint(&device, &definition)
        } else {
            custom_mesh::compile(&device, &definition)
        };
        match result {
            Ok(program) => Ok(MetalShader {
                definition,
                program: Some(program),
            }),
            Err(error) => Err(AssetLoadFailure::with_decoded(
                error,
                MetalShader {
                    definition,
                    program: None,
                },
            )),
        }
    })
}

fn compile_paint(device: &Device, definition: &ShaderDefinition) -> Result<Object, String> {
    let body = &definition
        .backend_source(BACKEND)
        .ok_or("Canvas paint requires an msl or ipp stage")?
        .paint;
    validate_paint(body)?;
    let body = if definition.selects_portable(BACKEND) {
        lower(body, IppShaderTarget::Msl)?
    } else {
        body.clone()
    };
    if definition.parameters.len() > MAX_PAINT_PARAMETERS {
        return Err("Metal Canvas paint exceeds its parameter capacity".into());
    }

    let presentation = definition.recipe.features & 64 != 0;
    let lighting = presentation && definition.recipe.features & 8 != 0;
    let direction = if lighting {
        "canvas_light_direction(raster, scene_lighting, index)"
    } else {
        "float3(0.0f, 0.0f, 1.0f)"
    };
    let radiance = if lighting {
        "canvas_light_radiance(raster, scene_lighting, index)"
    } else {
        "float3(0.0f)"
    };
    let view = if presentation {
        "canvas_view(raster, scene_lighting)"
    } else {
        "float3(0.0f, 0.0f, 1.0f)"
    };
    let count = if lighting {
        "raster.world.w != 0.0 ? int(scene_lighting.ambient_count.w) : 0"
    } else {
        "0"
    };
    let mut source = format!(
        "#define IPP_AUTHORED_PAINT 1\n\
         #define sceneLightDirection(index) {direction}\n\
         #define sceneLightRadiance(index) {radiance}\n\
         float4 ipp_paint(float2 position, float2 size, float4 color, float edge, float4 data,\n\
                          constant float4 *parameters, CanvasRaster raster, constant Lighting &scene_lighting) {{\n\
         float2 ipp_fragment_coord = raster.position.xy;\n\
         float3 viewDirection = {view};\n\
         int sceneLightCount = {count};\n",
    );
    for (index, (name, kind)) in definition.parameters.iter().enumerate() {
        let (ty, lanes) = match kind {
            ShaderParameterKind::F32 => ("float", ".x"),
            ShaderParameterKind::Vec2 => ("float2", ".xy"),
            ShaderParameterKind::Vec3 => ("float3", ".xyz"),
            ShaderParameterKind::Vec4 => ("float4", ""),
            _ => return Err("Metal Canvas paint parameters must be numeric vectors".into()),
        };
        writeln!(source, "{ty} p_{name} = parameters[{index}]{lanes};").expect("writing to string");
    }
    source.push_str(&body);
    source.push_str("\n}\n#undef sceneLightDirection\n#undef sceneLightRadiance\n");
    device.canvas_program(&source)
}

pub(super) struct AuthoredPaint<'a> {
    program: &'a Object,
    values: [[f32; 4]; MAX_PAINT_PARAMETERS],
    count: usize,
}

impl<'a> AuthoredPaint<'a> {
    pub(super) fn resolve(
        host: &'a HostRuntime,
        publication: WorldPublicationId,
        paint: &CanvasPaintInstance,
    ) -> Option<Self> {
        let shader = host
            .publication_resource(publication, paint.shader?)?
            .data()?
            .as_any()
            .downcast_ref::<MetalShader>()?;
        if !shader.definition.is_paint() {
            return None;
        }
        let program = shader.program.as_ref()?;
        let count = shader.definition.parameters.len();
        let mut values = [[0.0; 4]; MAX_PAINT_PARAMETERS];
        for (index, (name, kind)) in shader.definition.parameters.iter().enumerate() {
            let (_, value) = paint
                .properties
                .iter()
                .find(|(parameter, _)| parameter.as_ref() == name)?;
            if ShaderParameterKind::from_value(value).ok()? != *kind || value.validate().is_err() {
                return None;
            }
            let floats = value.floats()?;
            values[index][..floats.len()].copy_from_slice(floats);
        }
        Some(Self {
            program,
            values,
            count,
        })
    }

    pub(super) fn parameters(&self) -> PaintParameters<'_> {
        PaintParameters {
            program: self.program,
            values: &self.values[..self.count],
        }
    }
}
