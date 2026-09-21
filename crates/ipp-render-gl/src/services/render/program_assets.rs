//! Built-in program recipes use ordinary resource loading and recovery.

use super::{assets::SharedRenderDevice, shader::RenderShaderConfig};
use crate::{RenderDevice, RenderError, RenderService};
use ipp_core::{
    WorldContext,
    services::asset_management::{
        Asset, AssetLoader, AssetSource, AssetTypeId, BufferedAssetLoader,
    },
};
use std::{any::Any, cell::Cell, collections::BTreeSet, rc::Rc};

pub(super) const PROGRAM_TYPE: AssetTypeId = AssetTypeId(14);

pub(super) struct GlProgramData<D: RenderDevice> {
    pub program: Option<D::Program>,
    device: SharedRenderDevice<D>,
    count: Rc<Cell<usize>>,
}

impl<D: RenderDevice> Asset for GlProgramData<D> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        self
    }

    fn invalidate_graphics(&mut self) {
        if let Some(program) = self.program.take() {
            self.device.borrow_mut().delete_program(program);
            self.count.set(self.count.get().saturating_sub(1));
        }
    }

    fn graphics_ready(&self) -> Option<bool> {
        Some(self.program.is_some())
    }

    fn graphics_bytes(&self) -> Option<usize> {
        Some(0)
    }

    fn resident_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

impl<D: RenderDevice> Drop for GlProgramData<D> {
    fn drop(&mut self) {
        if let Some(program) = self.program.take() {
            self.device.borrow_mut().delete_program(program);
            self.count.set(self.count.get().saturating_sub(1));
        }
    }
}

pub(super) fn loader<D: RenderDevice>(
    device: SharedRenderDevice<D>,
    count: Rc<Cell<usize>>,
) -> impl AssetLoader<Data = GlProgramData<D>> {
    BufferedAssetLoader::new(move |bytes| {
        let bits = u32::from_le_bytes(bytes.try_into().map_err(|_| "Invalid program recipe")?);
        let config = RenderShaderConfig::from_recipe_bits(bits);
        let sources = if bits & (1 << 9) != 0 {
            #[cfg(feature = "shadows")]
            {
                config.shadow_sources()
            }
            #[cfg(not(feature = "shadows"))]
            {
                return Err("Shadow programs unavailable".into());
            }
        } else {
            config.sources()
        };
        let (vertex, fragment) = sources.map_err(|error| error.to_string())?;
        let program = device
            .borrow_mut()
            .create_program(&vertex, &fragment)
            .map_err(|error| error.to_string())?;
        count.set(count.get() + 1);
        Ok(GlProgramData {
            program: Some(program),
            device: device.clone(),
            count: count.clone(),
        })
    })
}

fn source(config: RenderShaderConfig, shadow: bool) -> AssetSource {
    let bits = config.recipe_bits() | (u32::from(shadow) << 9);
    AssetSource {
        kind: PROGRAM_TYPE,
        uri: format!("ipp-render://program/{bits}"),
        variant: 0,
    }
}

struct ProgramSourceName {
    bytes: [u8; 40],
    length: usize,
}

impl ProgramSourceName {
    fn new(config: RenderShaderConfig, shadow: bool) -> Self {
        let mut bits = config.recipe_bits() | (u32::from(shadow) << 9);
        let mut digits = [0u8; 10];
        let mut start = digits.len();
        loop {
            start -= 1;
            digits[start] = b'0' + (bits % 10) as u8;
            bits /= 10;
            if bits == 0 {
                break;
            }
        }
        let prefix = b"ipp-render://program/";
        let length = prefix.len() + digits.len() - start;
        let mut bytes = [0u8; 40];
        bytes[..prefix.len()].copy_from_slice(prefix);
        bytes[prefix.len()..length].copy_from_slice(&digits[start..]);
        Self {
            bytes,
            length,
        }
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.length]).expect("program ASCII")
    }
}

impl<D: RenderDevice> RenderService<D> {
    /// Collect demand only. Host asset progression performs compilation on a later phase.
    pub(super) fn prepare_programs(
        &mut self,
        world: &mut WorldContext<'_>,
    ) -> Result<(), RenderError> {
        #[cfg(feature = "profiling")]
        let _allocation_scope = ipp_core::profiling::AllocationScope::new(227, "gl.program-demand");

        self.program_lookup.fill(None);
        let items = world.render_items();
        let reuse = ipp_core::allocation_optimizations_enabled();
        let mut recipes = BTreeSet::new();
        let mut recipe_scratch = std::mem::take(&mut self.recipe_scratch);
        recipe_scratch.clear();
        let mut insert_recipe = |recipe| {
            if reuse {
                recipe_scratch.push(recipe);
            } else {
                recipes.insert(recipe);
            }
        };
        let shadows = if world.active_camera().is_some()
            && items.iter().any(|item| {
                item.pbr.is_some()
                    || world
                        .custom_material(item.entity)
                        .is_some_and(|material| material.receives_light || material.casts_shadows)
            }) {
            cfg!(feature = "shadows")
                && if ipp_core::render_buffer_reuse_enabled() {
                    world.light_items().any(|(_, _, light)| light.cast_shadows)
                } else {
                    world
                        .light_items()
                        .collect::<Vec<_>>()
                        .iter()
                        .any(|(_, _, light)| light.cast_shadows)
                }
        } else {
            false
        };
        let mut seen = BTreeSet::new();
        let mut previous_entity = None;
        for item in items {
            if reuse {
                // Core render inputs are grouped in deterministic entity order.
                if previous_entity == Some(item.entity) {
                    continue;
                }
                previous_entity = Some(item.entity);
            } else if !seen.insert(item.entity) {
                continue;
            }
            #[cfg(feature = "particles")]
            let quad = item.particle.filter(|p| p.sprite).map(|_| {
                if !ipp_core::render_buffer_reuse_enabled() { self.particle_quad_metadata = None; }
                &*self.particle_quad_metadata.get_or_insert_with(|| {
                    ipp_core::services::asset_management::mesh_metadata::MeshMetadata::from_owned_mesh(super::particles::quad_asset())
                })
            });
            #[cfg(not(feature = "particles"))]
            let mesh = world.mesh_metadata(item.mesh);
            #[cfg(feature = "particles")]
            let mesh = if ipp_core::render_buffer_reuse_enabled() {
                // Sprite quads are private renderer assets, with no World mesh key.
                quad.or_else(|| world.mesh_metadata(item.mesh))
            } else {
                quad.or(world.mesh_metadata(item.mesh))
            };
            let Some(mesh) = mesh else {
                continue;
            };
            let normals = mesh.has_normals();
            #[cfg(feature = "mesh-poses")]
            let normals = normals
                && item.pose.is_none_or(|(key, _)| {
                    world
                        .mesh_metadata(key)
                        .is_some_and(|mesh| mesh.has_normals())
                });
            let config =
                RenderShaderConfig::new(item.texture.is_some(), mesh.has_texture_weights())
                    .with_solid_fallback(item.solid_fallback);
            let config = if item.pbr.is_some() {
                config.with_lighting(shadows, normals)
            } else {
                config
            };
            #[cfg(feature = "skeletal-animation")]
            let config = config.with_skinning(item.skinned);
            #[cfg(feature = "mesh-poses")]
            let config = config.with_mesh_pose(item.pose.is_some());
            #[cfg(feature = "particles")]
            let config = config.with_particles(
                item.particle.is_some(),
                item.particle.is_some_and(|p| p.sprite),
            );
            insert_recipe((config, false));
            if shadows && item.pbr.is_some() {
                insert_recipe((config.with_lighting(false, normals), false));
            }
            #[cfg(feature = "shadows")]
            if shadows && item.pbr.is_some_and(|material| material.cast_shadows) {
                let config = RenderShaderConfig::default();
                #[cfg(feature = "skeletal-animation")]
                let config = config.with_skinning(item.skinned);
                #[cfg(feature = "mesh-poses")]
                let config = config.with_mesh_pose(item.pose.is_some());
                #[cfg(feature = "particles")]
                let config = config.with_particles(item.particle.is_some(), false);
                insert_recipe((config, true));
            }
        }
        if !world.debug_render_items().is_empty() {
            insert_recipe((
                RenderShaderConfig::default().with_debug_geometry(true),
                false,
            ));
        }
        if reuse {
            recipe_scratch.sort_unstable();
            recipe_scratch.dedup();
            let mut keys = std::mem::take(&mut self.program_keys);
            keys.clear();
            let world_id = world.id();
            let result = (|| {
                for &(config, shadow) in &recipe_scratch {
                    let name = ProgramSourceName::new(config, shadow);
                    let key = match world.asset_source_key(PROGRAM_TYPE, name.as_str(), 0) {
                        Some(key) => key,
                        None => {
                            let bits = config.recipe_bits() | (u32::from(shadow) << 9);
                            world.asset_resources_mut().prepare_internal_source(
                                world_id,
                                source(config, shadow),
                                bits.to_le_bytes().to_vec(),
                            )?
                        }
                    };
                    self.program_lookup
                        [(config.recipe_bits() | (u32::from(shadow) << 9)) as usize] = Some(key);
                    keys.push(key);
                }
                world
                    .asset_resources_mut()
                    .retain_internal_sources(world_id, &keys)
            })();
            self.recipe_scratch = recipe_scratch;
            self.program_keys = keys;
            return result.map_err(RenderError::RenderDevice);
        }
        let sources = recipes.into_iter().map(|(config, shadow)| {
            let bits = config.recipe_bits() | (u32::from(shadow) << 9);
            (source(config, shadow), bits.to_le_bytes().to_vec())
        });
        let world_id = world.id();
        world
            .asset_resources_mut()
            .prepare_internal_sources(world_id, sources)
            .map_err(RenderError::RenderDevice)?;
        Ok(())
    }

    pub(super) fn builtin_program<'a>(
        &self,
        world: &'a WorldContext<'_>,
        config: RenderShaderConfig,
        shadow: bool,
    ) -> Option<&'a D::Program> {
        let resources = world.asset_resources();
        let key = if ipp_core::allocation_optimizations_enabled() {
            self.program_lookup[(config.recipe_bits() | (u32::from(shadow) << 9)) as usize]?
        } else {
            resources.find(&source(config, shadow))?
        };
        resources
            .get(key)?
            .data()?
            .as_any()
            .downcast_ref::<GlProgramData<D>>()?
            .program
            .as_ref()
    }
}
