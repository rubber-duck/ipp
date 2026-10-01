//! Material candidate preparation, program caching and evaluated parameter uploads.
use super::scene::{RenderEntity as EntityId, RenderScene, SceneItem};
use super::{
    assets::GlTextureData, custom_shader, shader::RenderShaderConfig, shader_asset::GlShaderData,
};
use crate::{RenderDevice, RenderError, RenderService};
use ipp_core::{
    DynamicProperties, DynamicValue, services::asset_management::AssetKey,
    systems::render::PublishedMaterialProperty,
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CustomProgramKey {
    pub asset: u64,
    pub variant: u32,
    pub config: RenderShaderConfig,
    pub lit: bool,
    pub shadow: bool,
}

#[derive(Clone, Copy, Default)]
pub(super) struct CustomDrawState {
    pub alpha_mode: u32,
    pub alpha_cutoff: f32,
    pub receives_light: bool,
    pub receives_shadows: bool,
    pub casts_shadows: bool,
    pub conservative_bounds: bool,
}

/// Why an entity's custom material falls back to default drawing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomMaterialFallback {
    /// Authored shader source of the material.
    pub source: String,
    /// Preparation failure; formatted only when logged.
    pub error: RenderError,
}

#[derive(Clone, Default)]
pub(super) struct PreparedCustomMaterial {
    pub material: CustomDrawState,
    pub key: CustomProgramKey,
    pub words: Vec<u32>,
    pub textures: Vec<(String, AssetKey)>,
    pub custom_vertex: bool,
}

impl<D: RenderDevice> RenderService<D> {
    pub(super) fn prepare_custom_materials(
        &mut self,
        world: &RenderScene<'_>,
        items: &[SceneItem<'_>],
    ) -> Result<BTreeMap<EntityId, PreparedCustomMaterial>, RenderError> {
        #[cfg(feature = "instrumentation")]
        let _allocation_scope = ipp_core::profiling::AllocationScope::new(224, "gl.custom-prepare");

        // Records own only copied draw flags, asset identities and retained buffers.
        // They never keep component/asset references across a World phase.
        let mut ready = std::mem::take(&mut self.custom_materials);
        ready.retain(|entity, _| {
            items
                .binary_search_by_key(entity, |item| item.entity)
                .is_ok()
                && items
                    .iter()
                    .any(|item| item.entity == *entity && item.published.custom.is_some())
        });
        self.custom_fallbacks.retain(|entity, _| {
            items
                .binary_search_by_key(entity, |item| item.entity)
                .is_ok()
        });
        let mut previous = None;
        for item in items {
            if previous == Some(item.entity) {
                continue;
            }
            previous = Some(item.entity);
            if item.particle.is_some_and(|p| p.sprite) {
                ready.remove(&item.entity);
                self.custom_fallbacks.remove(&item.entity);
                continue;
            }
            let material = item
                .custom_material
                .then_some(item.published.custom.as_ref())
                .flatten();
            let Some(material) = material else {
                self.custom_fallbacks.remove(&item.entity);
                continue;
            };
            let prepared = ready.entry(item.entity).or_default();
            let result = (|| {
                let asset = material.shader.ok_or_else(|| {
                    RenderError::RenderDevice("shader definition pending or unset".into())
                })?;
                let shader = world
                    .resource(asset)
                    .and_then(|resource| {
                        resource.data()?.as_any().downcast_ref::<GlShaderData<D>>()
                    })
                    .ok_or_else(|| {
                        RenderError::RenderDevice("compiled shader unavailable".into())
                    })?;
                let definition = &shader.definition;
                let mesh = world
                    .mesh_metadata(item.mesh)
                    .ok_or(RenderError::MissingMesh)?;
                let present = mesh.attributes();
                if definition.required_attributes & !present != 0 {
                    return Err(RenderError::RenderDevice(
                        "required custom vertex attribute missing".into(),
                    ));
                }
                let mut properties = DynamicProperties::default();
                for name in definition.parameters.keys() {
                    let property = material.properties.get(name.as_str()).ok_or_else(|| {
                        RenderError::RenderDevice("missing published material property".into())
                    })?;
                    let value = match property {
                        PublishedMaterialProperty::Value(value) => value.clone(),
                        PublishedMaterialProperty::Resource(key) => DynamicValue::Asset(
                            world
                                .resource(key.ok_or(RenderError::MissingTexture)?)
                                .ok_or(RenderError::MissingTexture)?
                                .source()
                                .clone(),
                        ),
                    };
                    properties.set(name, value).map_err(|_| {
                        RenderError::RenderDevice("invalid published material property".into())
                    })?;
                }
                custom_shader::parameter_words(definition, &properties, &mut prepared.words)?;
                let mut texture_index = 0;
                for (name, kind) in &definition.parameters {
                    if *kind != ipp_core::services::asset_management::shader::ShaderParameterKind::Texture2D {
                        continue;
                    }
                    let key = match material.properties.get(name.as_str()) {
                        Some(PublishedMaterialProperty::Resource(Some(key))) => *key,
                        _ => return Err(RenderError::MissingTexture),
                    };
                    if world
                        .resource(key)
                        .and_then(|r| r.data()?.as_any().downcast_ref::<GlTextureData<D>>())
                        .and_then(|d| d.gpu.as_ref())
                        .is_none()
                    {
                        return Err(RenderError::MissingTexture);
                    }
                    if let Some(slot) = prepared.textures.get_mut(texture_index) {
                        if slot.0 != *name {
                            slot.0.clone_from(name);
                        }
                        slot.1 = key;
                    } else {
                        prepared.textures.push((name.clone(), key));
                    }
                    texture_index += 1;
                }
                prepared.textures.truncate(texture_index);
                let normals = mesh.has_normals();
                let normals = normals
                    && item.pose.is_none_or(|(key, _)| {
                        world.mesh_metadata(key).is_some_and(|m| m.has_normals())
                    });
                if definition.recipe.features & 1 != 0 && !normals {
                    return Err(RenderError::RenderDevice(
                        "Shader recipe requires normals".into(),
                    ));
                }
                let config = RenderShaderConfig::default()
                    .with_lighting(false, definition.recipe.features & 1 != 0);
                let config = config.with_skinning(item.skinned);
                let config = config.with_mesh_pose(item.pose.is_some());
                let config = config.with_particles(item.particle.is_some(), false);
                let key = CustomProgramKey {
                    asset: asset.to_u64(),
                    variant: world
                        .resource(asset)
                        .ok_or(RenderError::MissingTexture)?
                        .source()
                        .variant,
                    config,
                    lit: material.receives_light,
                    shadow: false,
                };
                if config != super::shader_asset::config(definition)?
                    || material.receives_light != (definition.recipe.features & 8 != 0)
                    || (material.casts_shadows
                        && material.alpha_mode != 2
                        && shader.shadow.is_none())
                {
                    return Err(RenderError::RenderDevice(
                        "Material usage does not match explicit shader recipe".into(),
                    ));
                }
                prepared.material = CustomDrawState {
                    alpha_mode: material.alpha_mode,
                    alpha_cutoff: material.alpha_cutoff,
                    receives_light: material.receives_light,
                    receives_shadows: material.receives_shadows,
                    casts_shadows: material.casts_shadows,
                    conservative_bounds: material.conservative_bounds,
                };
                prepared.key = key;
                prepared.custom_vertex = shader.custom_vertex;
                // Validate resource/unit/UBO limits before selecting the candidate for any pass.
                self.prepare_custom_material(world, prepared, false)?;
                if material.casts_shadows && material.alpha_mode != 2 {
                    self.prepare_custom_material(world, prepared, true)?;
                }
                Ok(())
            })();
            match result {
                Ok(()) => {
                    self.custom_fallbacks.remove(&item.entity);
                }
                Err(RenderError::ContextLost) => return Err(RenderError::ContextLost),
                Err(error) => {
                    ready.remove(&item.entity);
                    self.record_custom_fallback(
                        item.entity,
                        material
                            .shader
                            .and_then(|key| world.resource(key))
                            .map_or("unavailable shader", |resource| &*resource.source().uri),
                        error,
                    );
                }
            }
        }

        Ok(ready)
    }

    /// Retain a fallback reason, logging it only when it changes.
    fn record_custom_fallback(&mut self, entity: EntityId, source: &str, error: RenderError) {
        if self
            .custom_fallbacks
            .get(&entity)
            .is_some_and(|fallback| fallback.source == source && fallback.error == error)
        {
            return;
        }

        ipp_core::diagnostic!(
            Warn,
            "custom material fallback entity={entity:?}: {source}: {error}"
        );
        self.custom_fallbacks.insert(
            entity,
            CustomMaterialFallback {
                source: source.to_owned(),
                error,
            },
        );
    }

    pub(super) fn custom_program<'a>(
        world: &'a RenderScene<'_>,
        key: CustomProgramKey,
        shadow: bool,
    ) -> Result<&'a D::Program, RenderError> {
        let shader = world
            .resource(AssetKey::from_u64(key.asset))
            .and_then(|resource| resource.data()?.as_any().downcast_ref::<GlShaderData<D>>())
            .ok_or_else(|| RenderError::RenderDevice("compiled shader unavailable".into()))?;
        (if shadow {
            shader.shadow.as_ref()
        } else {
            shader.surface.as_ref()
        })
        .ok_or_else(|| RenderError::RenderDevice("shader pass unavailable".into()))
    }

    fn prepare_custom_material(
        &self,
        world: &RenderScene<'_>,
        material: &PreparedCustomMaterial,
        shadow: bool,
    ) -> Result<(), RenderError> {
        self.device.borrow_mut().prepare_custom_parameters(
            Self::custom_program(world, material.key, shadow)?,
            material.words.len(),
            material.textures.len(),
        )
    }

    pub(super) fn upload_custom_material(
        &self,
        world: &RenderScene<'_>,
        material: &PreparedCustomMaterial,
        shadow: bool,
    ) -> Result<(), RenderError> {
        let program = Self::custom_program(world, material.key, shadow)?;
        let textures = material.textures.iter().map(|(name, key)| {
            let texture = world
                .resource(*key)
                .and_then(|r| r.data()?.as_any().downcast_ref::<GlTextureData<D>>())
                .and_then(|d| d.gpu.as_ref())
                .ok_or(RenderError::MissingTexture)?;
            Ok((name.as_str(), texture))
        });
        self.device.borrow_mut().set_custom_parameters(
            program,
            &material.words,
            textures,
            material.material.alpha_mode,
            material.material.alpha_cutoff,
        )
    }

    /// Most recent material fallback reasons, separate from semantic World outcomes.
    pub fn custom_material_diagnostics(&self) -> &BTreeMap<EntityId, CustomMaterialFallback> {
        &self.custom_fallbacks
    }
}
