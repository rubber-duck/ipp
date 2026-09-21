//! Material candidate preparation, program caching and evaluated parameter uploads.
use super::{
    assets::GlTextureData, custom_shader, shader::RenderShaderConfig, shader_asset::GlShaderData,
};
use crate::{RenderDevice, RenderError, RenderService};
use ipp_core::{
    EntityId, WorldContext,
    services::asset_management::{AssetKey, shader::SHADER_TYPE},
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
    #[cfg(feature = "shadows")]
    pub casts_shadows: bool,
    pub conservative_bounds: bool,
}

#[derive(Default)]
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
        world: &WorldContext<'_>,
        items: &[ipp_core::RenderItem],
    ) -> Result<BTreeMap<EntityId, PreparedCustomMaterial>, RenderError> {
        #[cfg(feature = "profiling")]
        let _allocation_scope = ipp_core::profiling::AllocationScope::new(224, "gl.custom-prepare");

        // Records own only copied draw flags, asset identities and retained buffers.
        // They never keep component/asset references across a World phase.
        let mut ready = if ipp_core::allocation_optimizations_enabled() {
            std::mem::take(&mut self.custom_materials)
        } else {
            BTreeMap::new()
        };
        ready.retain(|entity, _| {
            items
                .binary_search_by_key(entity, |item| item.entity)
                .is_ok()
                && world.custom_material(*entity).is_some()
        });
        let mut previous = None;
        let mut diagnostics = BTreeMap::new();
        for item in items {
            if previous == Some(item.entity) {
                continue;
            }
            previous = Some(item.entity);
            #[cfg(feature = "particles")]
            if item.particle.is_some_and(|p| p.sprite) {
                ready.remove(&item.entity);
                continue;
            }
            if !item.custom_material {
                continue;
            }
            let Some(material) = world.custom_material(item.entity) else {
                continue;
            };
            let prepared = ready.entry(item.entity).or_default();
            let result = (|| {
                let asset = world
                    .asset_source_key(SHADER_TYPE, &material.source, material.variant)
                    .ok_or_else(|| {
                        RenderError::RenderDevice("shader definition pending or unset".into())
                    })?;
                let shader = world
                    .asset_resources()
                    .get(asset)
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
                custom_shader::parameter_words(
                    definition,
                    &material.properties,
                    &mut prepared.words,
                )?;
                let mut texture_index = 0;
                for (name, kind) in &definition.parameters {
                    if *kind != ipp_core::services::asset_management::shader::ShaderParameterKind::Texture2D {
                        continue;
                    }
                    let texture = material
                        .properties
                        .asset(name)
                        .expect("validated texture parameter");
                    let key = world
                        .asset_source_key(texture.kind, &texture.uri, texture.variant)
                        .ok_or(RenderError::MissingTexture)?;
                    if world
                        .asset_resources()
                        .get(key)
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
                #[cfg(feature = "mesh-poses")]
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
                #[cfg(feature = "skeletal-animation")]
                let config = config.with_skinning(item.skinned);
                #[cfg(feature = "mesh-poses")]
                let config = config.with_mesh_pose(item.pose.is_some());
                #[cfg(feature = "particles")]
                let config = config.with_particles(item.particle.is_some(), false);
                let key = CustomProgramKey {
                    asset: asset.to_u64(),
                    variant: material.variant,
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
                    #[cfg(feature = "shadows")]
                    casts_shadows: material.casts_shadows,
                    conservative_bounds: material.conservative_bounds,
                };
                prepared.key = key;
                prepared.custom_vertex = shader.custom_vertex;
                // Validate resource/unit/UBO limits before selecting the candidate for any pass.
                self.prepare_custom_material(world, prepared, false)?;
                #[cfg(feature = "shadows")]
                if material.casts_shadows && material.alpha_mode != 2 {
                    self.prepare_custom_material(world, prepared, true)?;
                }
                Ok(())
            })();
            match result {
                Ok(()) => {}
                Err(RenderError::ContextLost) => return Err(RenderError::ContextLost),
                Err(error) => {
                    ready.remove(&item.entity);
                    diagnostics.insert(item.entity, format!("{}: {error}", material.source));
                }
            }
        }
        for (entity, reason) in &diagnostics {
            if self.custom_diagnostics.get(entity) != Some(reason) {
                ipp_core::diagnostic!(Warn, "custom material fallback entity={entity:?}: {reason}");
            }
        }
        self.custom_diagnostics = diagnostics;
        Ok(ready)
    }

    pub(super) fn custom_program<'a>(
        world: &'a WorldContext<'_>,
        key: CustomProgramKey,
        shadow: bool,
    ) -> Result<&'a D::Program, RenderError> {
        let shader = world
            .asset_resources()
            .get(AssetKey::from_u64(key.asset))
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
        world: &WorldContext<'_>,
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
        world: &WorldContext<'_>,
        material: &PreparedCustomMaterial,
        shadow: bool,
    ) -> Result<(), RenderError> {
        let program = Self::custom_program(world, material.key, shadow)?;
        let textures = material.textures.iter().map(|(name, key)| {
            let texture = world
                .asset_resources()
                .get(*key)
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
    pub fn custom_material_diagnostics(&self) -> &BTreeMap<EntityId, String> {
        &self.custom_diagnostics
    }
}
