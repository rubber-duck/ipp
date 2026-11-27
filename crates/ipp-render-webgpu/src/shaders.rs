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

use crate::{canvas_device::CanvasProgram, device::Graphics as Device};

pub(super) const BACKEND: &str = "glsl-es-300";
const MAX_PAINT_PARAMETERS: usize = 16;

pub(super) enum Program {
    Canvas(Rc<CanvasProgram>),
    Mesh(crate::custom_mesh::MaterialProgram),
}

pub(super) struct WebGpuShader {
    pub definition: ShaderDefinition,
    pub program: Option<Program>,
}

impl Asset for WebGpuShader {
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

pub(super) fn loader(device: Rc<Device>) -> impl AssetLoader<Data = WebGpuShader> {
    AsyncAssetLoader::new(move |mut source| async move {
        let definition = ShaderDefinition::decode_reader(&mut *source).await?;
        let result = if definition.is_paint() {
            compile_paint(&device, &definition)
                .await
                .map(Program::Canvas)
        } else {
            crate::custom_mesh::compile(&device, &definition)
                .await
                .map(Program::Mesh)
        };
        match result {
            Ok(program) => Ok(WebGpuShader {
                definition,
                program: Some(program),
            }),
            Err(error) => Err(AssetLoadFailure::with_decoded(
                error,
                WebGpuShader {
                    definition,
                    program: None,
                },
            )),
        }
    })
}

pub(super) fn inverse_functions(source: &str) -> &'static str {
    if source
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|token| token == "inverse")
    {
        include_str!("inverse.glsl")
    } else {
        ""
    }
}

async fn compile_paint(
    device: &Device,
    definition: &ShaderDefinition,
) -> Result<Rc<CanvasProgram>, String> {
    let body = &definition
        .backend_source(BACKEND)
        .ok_or("Canvas paint requires an glsl-es-300 or ipp stage")?
        .paint;
    validate_paint(body)?;
    let body = if definition.selects_portable(BACKEND) {
        lower(body, IppShaderTarget::GlslEs300)?
    } else {
        body.clone()
    };
    if definition.parameters.len() > MAX_PAINT_PARAMETERS {
        return Err("WebGPU Canvas paint exceeds its parameter capacity".into());
    }

    let presentation = definition.recipe.features & 64 != 0;
    let lighting = presentation && definition.recipe.features & 8 != 0;
    let view = if presentation {
        "canvas_view()"
    } else {
        "vec3(0.0,0.0,1.0)"
    };
    let count = if lighting {
        "canvas_light_count()"
    } else {
        "0"
    };
    let inverse = inverse_functions(&body);
    let mut source = format!(
        "{inverse}vec4 ipp_paint(vec2 position, vec2 size, vec4 color, float edge, vec4 data) {{\nvec3 viewDirection={view};\nint sceneLightCount={count};\n"
    );
    for (index, (name, kind)) in definition.parameters.iter().enumerate() {
        let (ty, lanes) = match kind {
            ShaderParameterKind::F32 => ("float", ".x"),
            ShaderParameterKind::Vec2 => ("vec2", ".xy"),
            ShaderParameterKind::Vec3 => ("vec3", ".xyz"),
            ShaderParameterKind::Vec4 => ("vec4", ""),
            _ => return Err("WebGPU Canvas paint parameters must be numeric vectors".into()),
        };
        writeln!(
            source,
            "{ty} p_{name} = paint_at(slot + IPP_PARAMETER_OFFSET + {index}u){lanes};"
        )
        .expect("writing to string");
    }
    source.push_str(&body);
    source.push_str("\n}\n");
    device.canvas.paint(&device.device, &source).await
}

pub(super) struct AuthoredPaint<'a> {
    program: &'a Rc<CanvasProgram>,
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
            .downcast_ref::<WebGpuShader>()?;
        if !shader.definition.is_paint() {
            return None;
        }
        let Program::Canvas(program) = shader.program.as_ref()? else {
            return None;
        };
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

pub(super) struct PaintParameters<'a> {
    pub program: &'a Rc<CanvasProgram>,
    pub values: &'a [[f32; 4]],
}

/// Lower the GLSL fragment variants with their per-primitive derivative policy.
/// Canvas paint selection is flat across a primitive but WGSL uniformity analysis
/// cannot infer that from the paint index and storage-buffer lookup.
pub(super) fn shader_module(
    device: &wgpu::Device,
    descriptor: wgpu::ShaderModuleDescriptor<'_>,
) -> wgpu::ShaderModule {
    #[cfg(target_arch = "wasm32")]
    if let wgpu::ShaderSource::Glsl {
        shader,
        stage: wgpu::naga::ShaderStage::Fragment,
        defines,
    } = &descriptor.source
    {
        use wgpu::naga::{back::wgsl, front::glsl, valid};
        let options = glsl::Options {
            stage: wgpu::naga::ShaderStage::Fragment,
            defines: defines
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        };
        if let Ok(module) = glsl::Frontend::default().parse(&options, shader)
            && let Ok(info) =
                valid::Validator::new(valid::ValidationFlags::all(), valid::Capabilities::all())
                    .validate(&module)
            && let Ok(code) = wgsl::write_string(&module, &info, wgsl::WriterFlags::empty())
        {
            return device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: descriptor.label,
                source: wgpu::ShaderSource::Wgsl(
                    format!("diagnostic(off, derivative_uniformity);\n{code}").into(),
                ),
            });
        }
    }
    device.create_shader_module(descriptor)
}
