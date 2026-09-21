//! Renderer-owned shader provider: complete GPU programs are the loaded payload.

use super::{assets::SharedRenderDevice, custom_shader, shader::RenderShaderConfig};
use crate::{RenderDevice, RenderError};
use ipp_core::{
    DynamicProperties, DynamicValue,
    services::asset_management::{
        Asset, AssetLoader, BufferedAssetLoader,
        shader::{ShaderDefinition, ShaderParameterKind as Kind},
    },
};
use std::{any::Any, cell::Cell, rc::Rc};

pub(super) struct GlShaderData<D: RenderDevice> {
    pub definition: ShaderDefinition,
    pub surface: Option<D::Program>,
    pub shadow: Option<D::Program>,
    pub custom_vertex: bool,
    device: SharedRenderDevice<D>,
    count: Rc<Cell<usize>>,
}

impl<D: RenderDevice> Asset for GlShaderData<D> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        &self.definition
    }

    fn invalidate_graphics(&mut self) {
        let count = usize::from(self.surface.is_some()) + usize::from(self.shadow.is_some());
        self.count.set(self.count.get().saturating_sub(count));
        let mut device = self.device.borrow_mut();
        if let Some(program) = self.surface.take() {
            device.delete_program(program);
        }
        if let Some(program) = self.shadow.take() {
            device.delete_program(program);
        }
    }

    fn graphics_ready(&self) -> Option<bool> {
        Some(self.surface.is_some())
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
    if definition.recipe.backend != "glsl-es-300" {
        return Err(RenderError::RenderDevice(
            "Unsupported shader backend".into(),
        ));
    }
    #[cfg(not(feature = "skeletal-animation"))]
    if flags & 2 != 0 {
        return Err(RenderError::RenderDevice(
            "Shader requires skinning support".into(),
        ));
    }
    #[cfg(not(feature = "mesh-poses"))]
    if flags & 4 != 0 {
        return Err(RenderError::RenderDevice(
            "Shader requires mesh-pose support".into(),
        ));
    }
    #[cfg(not(feature = "shadows"))]
    if flags & 16 != 0 {
        return Err(RenderError::RenderDevice(
            "Shader requires shadow support".into(),
        ));
    }
    #[cfg(not(feature = "particles"))]
    if flags & 32 != 0 {
        return Err(RenderError::RenderDevice(
            "Shader requires instancing support".into(),
        ));
    }
    let config = RenderShaderConfig::default().with_lighting(false, flags & 1 != 0);
    #[cfg(feature = "skeletal-animation")]
    let config = config.with_skinning(flags & 2 != 0);
    #[cfg(feature = "mesh-poses")]
    let config = config.with_mesh_pose(flags & 4 != 0);
    #[cfg(feature = "particles")]
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
                    uri: String::new(),
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

pub(super) fn loader<D: RenderDevice>(
    device: SharedRenderDevice<D>,
    count: Rc<Cell<usize>>,
) -> impl AssetLoader<Data = GlShaderData<D>> {
    BufferedAssetLoader::new(move |bytes| {
        let mut definition = ShaderDefinition::decode(bytes)?;
        let config = config(&definition).map_err(|error| error.to_string())?;
        let properties = defaults(&definition)?;
        let (declarations, _, _) = custom_shader::parameter_layout(&definition, &properties)
            .map_err(|error| error.to_string())?;
        let flags = definition.recipe.features;
        let compile = |shadow| {
            let (vertex, fragment) =
                custom_shader::sources(config, &definition, &declarations, flags & 8 != 0, shadow)?;
            device.borrow_mut().create_program(&vertex, &fragment)
        };
        let surface = compile(false).map_err(|error| error.to_string())?;
        let shadow = if flags & 16 != 0 {
            match compile(true) {
                Ok(program) => Some(program),
                Err(error) => {
                    device.borrow_mut().delete_program(surface);
                    return Err(error.to_string());
                }
            }
        } else {
            None
        };
        let custom_vertex = definition
            .backends
            .get(&definition.recipe.backend)
            .is_some_and(|source| !source.vertex.trim().is_empty());
        definition.backends.clear();
        count.set(count.get() + 1 + usize::from(shadow.is_some()));
        Ok(GlShaderData {
            definition,
            surface: Some(surface),
            shadow,
            custom_vertex,
            device: device.clone(),
            count: count.clone(),
        })
    })
}
