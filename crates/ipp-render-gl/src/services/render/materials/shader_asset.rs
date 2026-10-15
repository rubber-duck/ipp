//! Renderer-owned shader provider: complete GPU programs are the loaded payload.
//!
//! A material definition loads as its linked programs. A paint definition has no
//! program of its own: its body is compiled and linked alone, then retained with
//! its parameters for the canvas program, which the
//! [paint slots](super::super::canvas::paint) rebuild whenever their paints change.

use super::super::assets::context::{RenderAssetContext, RenderAssetLease};
use super::super::assets::loaders::SharedRenderDevice;
use super::super::canvas::paint::{self, CanvasPaintSource};
use super::{custom_shader, shader::RenderShaderConfig};
use crate::{RenderDevice, RenderError};
use ipp_core::{
    DynamicProperties, DynamicValue,
    services::asset_management::{
        Asset, AssetLoader, AsyncAssetLoader,
        formats::shader::{ShaderDefinition, ShaderParameterKind as Kind},
    },
};
use std::any::Any;

pub(in crate::services::render) struct GlShaderData<D: RenderDevice> {
    pub definition: ShaderDefinition,
    /// The program was lowered from the portable `ipp` entry, so its texture
    /// sampling uses the IPP sampler instead of texture-owned sampler state.
    pub portable: bool,
    asset_lease: RenderAssetLease,
    pub surface: Option<D::Program>,
    pub shadow: Option<D::Program>,
    pub custom_vertex: bool,
    /// A paint definition's validated body and parameters.
    pub paint: Option<CanvasPaintSource>,
    /// The paint compiled alone in the current context; graphics invalidation
    /// clears it until the provider validates the paint again.
    pub paint_ready: bool,
    device: SharedRenderDevice<D>,
}

impl<D: RenderDevice> Asset for GlShaderData<D> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        &self.definition
    }

    fn invalidate_graphics(&mut self) {
        self.paint_ready = false;
        let mut device = self.device.borrow_mut();
        if let Some(program) = self.surface.take()
            && self.asset_lease.is_current()
        {
            device.delete_program(program);
        }
        if let Some(program) = self.shadow.take()
            && self.asset_lease.is_current()
        {
            device.delete_program(program);
        }
    }

    fn graphics_ready(&self) -> Option<bool> {
        Some(self.surface.is_some() || self.paint_ready)
    }

    fn graphics_bytes(&self) -> Option<usize> {
        Some(0)
    }

    fn resident_bytes(&self) -> usize {
        self.definition.resident_bytes()
    }
}

impl<D: RenderDevice> Drop for GlShaderData<D> {
    fn drop(&mut self) {
        self.invalidate_graphics();
    }
}

pub(super) fn config(definition: &ShaderDefinition) -> Result<RenderShaderConfig, RenderError> {
    let flags = definition.recipe.features;
    if definition.recipe.backend != "glsl-es-300"
        && definition.recipe.backend
            != ipp_core::services::asset_management::formats::shader::IPP_SHADER_BACKEND
    {
        return Err(RenderError::RenderDevice(
            "Unsupported shader backend".into(),
        ));
    }
    let config = RenderShaderConfig::default().with_lighting(false, flags & 1 != 0);
    let config = config.with_skinning(flags & 2 != 0);
    let config = config.with_mesh_pose(flags & 4 != 0);
    let config = config.with_particles(flags & 32 != 0, false);
    Ok(config)
}

fn defaults(definition: &ShaderDefinition) -> Result<DynamicProperties, String> {
    let mut properties = DynamicProperties::default();
    for (name, kind) in &definition.parameters {
        let value = match kind {
            Kind::F32 => DynamicValue::F32(0.0),
            Kind::I32 => DynamicValue::I32(0),
            Kind::U32 => DynamicValue::U32(0),
            Kind::Bool => DynamicValue::Bool(false),
            Kind::Vec2 => DynamicValue::Vec2([0.0; 2]),
            Kind::Vec3 => DynamicValue::Vec3([0.0; 3]),
            Kind::Vec4 => DynamicValue::Vec4([0.0; 4]),
            Kind::Mat2 => DynamicValue::Mat2([0.0; 4]),
            Kind::Mat3 => DynamicValue::Mat3([0.0; 9]),
            Kind::Mat4 => DynamicValue::Mat4([0.0; 16]),
            Kind::Texture2D => {
                DynamicValue::Asset(ipp_core::services::asset_management::AssetSource {
                    kind: ipp_core::TEXTURE_TYPE,
                    uri: Default::default(),
                    variant: 0,
                })
            }
        };
        properties
            .set(name, value)
            .map_err(|error| format!("{error:?}"))?;
    }
    Ok(properties)
}

pub(in crate::services::render) fn loader<D: RenderDevice>(
    device: SharedRenderDevice<D>,
    context: RenderAssetContext,
) -> impl AssetLoader<Data = GlShaderData<D>> {
    AsyncAssetLoader::decode(move |mut reader| async move {
        let mut definition = ShaderDefinition::decode_reader(&mut *reader).await?;
        loop {
            let asset_lease = context.wait().await;
            if definition.is_paint() {
                match validate_paint(&device, &definition) {
                    Ok(paint) => {
                        definition.backends.clear();
                        return Ok(GlShaderData {
                            definition,
                            portable: false,
                            asset_lease,
                            surface: None,
                            shadow: None,
                            custom_vertex: false,
                            paint: Some(paint),
                            paint_ready: true,
                            device,
                        });
                    }
                    Err(RenderError::ContextLost) => {
                        context.set_active(false);
                        continue;
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
            let config = config(&definition).map_err(|error| error.to_string())?;
            let properties = defaults(&definition)?;
            let (declarations, _, _) = custom_shader::parameter_layout(&definition, &properties)
                .map_err(|error| error.to_string())?;
            let flags = definition.recipe.features;
            let compile = |shadow| {
                let (vertex, fragment) = custom_shader::sources(
                    config,
                    &definition,
                    &declarations,
                    flags & 8 != 0,
                    shadow,
                )?;
                device.borrow_mut().create_program(&vertex, &fragment)
            };
            let surface = match compile(false) {
                Ok(surface) => surface,
                Err(RenderError::ContextLost) => {
                    context.set_active(false);
                    continue;
                }
                Err(error) => return Err(error.to_string()),
            };
            let shadow = if flags & 16 != 0 {
                match compile(true) {
                    Ok(program) => Some(program),
                    Err(RenderError::ContextLost) => {
                        context.set_active(false);
                        continue;
                    }
                    Err(error) => {
                        device.borrow_mut().delete_program(surface);
                        return Err(error.to_string());
                    }
                }
            } else {
                None
            };
            let custom_vertex = definition
                .backend_source("glsl-es-300")
                .is_some_and(|source| !source.vertex.trim().is_empty());
            let portable = definition.selects_portable("glsl-es-300");
            definition.backends.clear();
            return Ok(GlShaderData {
                definition,
                portable,
                asset_lease,
                surface: Some(surface),
                shadow,
                custom_vertex,
                paint: None,
                paint_ready: false,
                device,
            });
        }
    })
}

/// Check a paint definition's GLSL ES body as the statements of one function, then
/// compile and link it alone, so a paint that cannot build never reaches the canvas
/// program. The validation program is released at once.
fn validate_paint<D: RenderDevice>(
    device: &SharedRenderDevice<D>,
    definition: &ShaderDefinition,
) -> Result<CanvasPaintSource, RenderError> {
    if definition.recipe.backend != paint::CANVAS_PAINT_BACKEND
        && definition.recipe.backend
            != ipp_core::services::asset_management::formats::shader::IPP_SHADER_BACKEND
    {
        return Err(RenderError::RenderDevice(
            "Unsupported shader backend".into(),
        ));
    }
    if definition
        .parameters
        .values()
        .any(|kind| *kind == Kind::Texture2D)
    {
        return Err(RenderError::RenderDevice(
            "paint textures are not yet supported by the canvas program".into(),
        ));
    }
    // A renderer-specific entry, even with a blank body, takes precedence:
    // selection is by entry presence, exactly as for material stages, so a
    // blank native paint fails here instead of silently using the portable one.
    let native_entry = definition
        .backends
        .contains_key(paint::CANVAS_PAINT_BACKEND);
    let lowered;
    let body = if native_entry {
        definition
            .paint_body(paint::CANVAS_PAINT_BACKEND)
            .ok_or_else(|| {
                RenderError::RenderDevice("Paint definition has no GLSL ES paint body".into())
            })?
    } else {
        let portable = definition
            .paint_body(ipp_core::services::asset_management::formats::shader::IPP_SHADER_BACKEND)
            .ok_or_else(|| {
                RenderError::RenderDevice(
                    "Paint definition has no GLSL ES or portable ipp paint body".into(),
                )
            })?;
        lowered = super::ipp_shader::lower(portable, super::ipp_shader::IppShaderTarget::GlslEs300)
            .map_err(RenderError::RenderDevice)?;
        &lowered
    };
    paint::check_paint_body(body).map_err(RenderError::RenderDevice)?;
    let source = CanvasPaintSource {
        body: body.into(),
        features: definition.recipe.features,
        parameters: definition
            .parameters
            .iter()
            .map(|(name, kind)| (name.clone(), *kind))
            .collect(),
    };
    let (vertex, fragment) = paint::validation_sources(&source);
    let mut device = device.borrow_mut();
    let program = device.create_program(&vertex, &fragment)?;
    device.delete_program(program);
    Ok(source)
}
