//! Built-in program recipes use ordinary resource loading and recovery.

use super::{assets::SharedRenderDevice, shader::RenderShaderConfig};
use crate::{RenderDevice, RenderError, RenderService};
use ipp_core::{
    HostRuntime, OutputKind, OutputRef, WorldPublicationId, WorldRef,
    services::asset_management::{
        Asset, AssetKey, AssetLoader, AssetSource, AssetTypeId, BufferedAssetLoader,
    },
};
use std::any::Any;

pub(super) const PROGRAM_TYPE: AssetTypeId = AssetTypeId(14);

pub(super) struct ProgramDemand {
    host: u64,
    world: Option<WorldRef>,
    programs: Vec<(AssetKey, AssetSource)>,
}

impl ProgramDemand {
    fn release(&mut self, host: &mut HostRuntime, keep: Option<&Self>) {
        let world = self.world.take();
        let programs = std::mem::take(&mut self.programs);
        let Some(world) = world.filter(|world| {
            host.identity() == self.host && host.world_ref(world.id()) == Some(*world)
        }) else {
            return;
        };
        for (key, source) in programs {
            if keep.is_some_and(|keep| {
                keep.world == Some(world) && keep.programs.iter().any(|entry| entry.0 == key)
            }) {
                continue;
            }
            if host.asset_resources().find(&source) == Some(key) {
                host.asset_resources_mut()
                    .release_client_source(world.id(), &source);
            }
        }
    }
}

pub(super) struct GlProgramData<D: RenderDevice> {
    pub program: Option<D::Program>,
    device: SharedRenderDevice<D>,
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
        }
    }
}

pub(super) fn loader<D: RenderDevice>(
    device: SharedRenderDevice<D>,
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
        Ok(GlProgramData {
            program: Some(program),
            device: device.clone(),
        })
    })
}

fn source(config: RenderShaderConfig, shadow: bool) -> AssetSource {
    let bits = config.recipe_bits() | (u32::from(shadow) << 9);
    AssetSource {
        kind: PROGRAM_TYPE,
        uri: format!("ipp-render://program/{bits}").into(),
        variant: 0,
    }
}

impl<D: RenderDevice> RenderService<D> {
    /// Prepare only the current authorized root and its completed nested outputs.
    /// None or failed selection withdraws this renderer's selected demand and caches.
    /// Explicit None on the owning Host releases the catalog binding, even with no
    /// Worlds left. Until then, another Host is rejected without changing either catalog.
    pub fn prepare(
        &mut self,
        host: &mut HostRuntime,
        selected: Option<(OutputRef, WorldPublicationId)>,
    ) -> Result<(), RenderError> {
        self.validate_catalog(host)?;
        let result = self.prepare_selected(host, selected);
        if result.is_err() || selected.is_none() {
            if let Some(demand) = &mut self.program_demand {
                demand.release(host, None);
            }
            if selected.is_none() {
                self.program_demand = None;
            }
            self.program_lookup.fill(None);
            self.retain_prepared_outputs(None, &Default::default());
        }
        result
    }

    pub(super) fn validate_catalog(&self, host: &HostRuntime) -> Result<(), RenderError> {
        if self
            .program_demand
            .as_ref()
            .is_some_and(|demand| demand.host != host.identity())
        {
            return Err(RenderError::HostCatalogMismatch);
        }
        Ok(())
    }

    fn prepare_selected(
        &mut self,
        host: &mut HostRuntime,
        selected: Option<(OutputRef, WorldPublicationId)>,
    ) -> Result<(), RenderError> {
        let Some((selection, publication)) = selected else {
            return Ok(());
        };
        if selection.kind() == OutputKind::Canvas && !cfg!(feature = "surfaces") {
            return Err(RenderError::UnavailableOutput);
        }
        if !host
            .root_output(selection.world().id())
            .is_some_and(|(current, _, completed)| current == selection && completed == publication)
        {
            return Err(RenderError::UnavailableOutput);
        }
        #[cfg(feature = "surfaces")]
        let presentations = super::canvas_scene::output_order(host, selection, publication)?;
        #[cfg(not(feature = "surfaces"))]
        let presentations = vec![(selection, publication)];
        let mut recipes = std::mem::take(&mut self.recipe_scratch);
        recipes.clear();
        let mut outputs = std::collections::BTreeSet::new();
        let collected = (|| {
            for (selection, publication) in presentations {
                outputs.insert(selection);
                if selection.kind() == OutputKind::Camera {
                    let scene = super::scene::RenderScene::new(host, selection, publication)?;
                    let shadows = cfg!(feature = "shadows")
                        && scene.lights.iter().any(|light| light.2.cast_shadows);
                    for item in &scene.items {
                        let config = super::draw_order::builtin_config(item, shadows);
                        recipes.push((config, false));
                        if shadows && item.pbr.is_some() {
                            recipes.push((config.with_lighting(false, item.normals), false));
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
                            recipes.push((config, true));
                        }
                    }
                    if !scene.debug.is_empty() {
                        recipes.push((
                            RenderShaderConfig::default().with_debug_geometry(true),
                            false,
                        ));
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = collected {
            self.recipe_scratch = recipes;
            return Err(error);
        }
        recipes.sort_unstable();
        recipes.dedup();
        self.program_lookup.fill(None);
        let mut demand = ProgramDemand {
            host: host.identity(),
            world: Some(selection.world()),
            programs: Vec::with_capacity(recipes.len()),
        };
        let prepared = (|| {
            for &(config, shadow) in &recipes {
                let bits = config.recipe_bits() | (u32::from(shadow) << 9);
                let source = source(config, shadow);
                let key = host
                    .asset_resources_mut()
                    .prepare_internal_source(
                        selection.world().id(),
                        source.clone(),
                        bits.to_le_bytes().to_vec(),
                    )
                    .map_err(RenderError::RenderDevice)?;
                self.program_lookup[bits as usize] = Some(key);
                demand.programs.push((key, source));
            }
            Ok(())
        })();
        self.recipe_scratch = recipes;
        if let Err(error) = prepared {
            demand.release(host, None);
            return Err(error);
        }
        if let Some(mut previous) = self.program_demand.take() {
            previous.release(host, Some(&demand));
        }
        self.program_demand = Some(demand);
        self.retain_prepared_outputs(Some(selection), &outputs);
        Ok(())
    }

    pub(super) fn builtin_program<'a>(
        &self,
        scene: &'a super::scene::RenderScene<'_>,
        config: RenderShaderConfig,
        shadow: bool,
    ) -> Option<&'a D::Program> {
        let key = self.program_lookup[(config.recipe_bits() | (u32::from(shadow) << 9)) as usize]?;
        scene
            .host
            .asset_resources()
            .get(key)?
            .data()?
            .as_any()
            .downcast_ref::<GlProgramData<D>>()?
            .program
            .as_ref()
    }
}
