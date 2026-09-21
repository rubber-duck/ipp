use super::{
    PARTICLE_CACHE_TYPE, ParticleCache, ParticleRuntimeState,
    simulation::{ParticleEmissionSurface, simulate},
};
use crate::systems::{
    System, SystemDependency, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemUpdateContext,
};
use crate::world::systems::asset_dependencies::source_key_from_fields;

/// World evaluator for component-owned particle state.
#[derive(Default)]
pub struct ParticleSystem {
    emitters: crate::world::component_query::ComponentQuery<super::ParticleEmitter>,
    playbacks: crate::world::component_query::ComponentQuery<super::ParticlePlayback>,
}

impl ParticleSystem {
    /// Stable system identity.
    pub const ID: SystemId = SystemId("ipp.particles");
}

/// Construct one particle evaluator per World.
pub struct ParticleSystemFactory;

impl SystemFactory for ParticleSystemFactory {
    fn id(&self) -> SystemId {
        ParticleSystem::ID
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[
            SystemDependency::Required(SystemId("ipp.asset-dependencies")),
            SystemDependency::Required(SystemId("ipp.final-propagation")),
        ]
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(ParticleSystem::default()))
    }
}

impl System for ParticleSystem {
    fn before_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        self.emitters
            .before_commit(context, crate::ComponentValue::PARTICLE_EMITTER);
        self.playbacks
            .before_commit(context, crate::ComponentValue::PARTICLE_PLAYBACK);
    }

    fn after_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        self.emitters.after_commit(
            context,
            crate::ComponentValue::PARTICLE_EMITTER,
            crate::components::registry::ComponentStorage::particle_emitter_ptr,
        );
        self.playbacks.after_commit(
            context,
            crate::ComponentValue::PARTICLE_PLAYBACK,
            crate::components::registry::ComponentStorage::particle_playback_ptr,
        );
    }

    fn validate_commit(
        &self,
        context: &crate::systems::SystemCommitContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        use crate::ComponentValue as C;
        for &(entity, component) in context.staged.changed.keys() {
            if ![
                C::PARTICLE_EMITTER,
                C::PARTICLE_PLAYBACK,
                C::PARTICLE_SPRITE,
                C::PARTICLE_MESH,
            ]
            .contains(&component)
            {
                continue;
            }
            let present = |component| {
                if crate::allocation_followup_enabled() {
                    return context
                        .staged
                        .entities
                        .get(&entity)
                        .and_then(|record| record.input(component))
                        .is_some();
                }
                context
                    .staged
                    .input_value(&context.world_data.components, entity, component)
                    .is_some()
            };
            if (present(C::PARTICLE_EMITTER) && present(C::PARTICLE_PLAYBACK))
                || (present(C::PARTICLE_SPRITE) && present(C::PARTICLE_MESH))
            {
                return Err(crate::ErrorReason::InvalidValue);
            }
        }
        Ok(())
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        let dt = context.dt();
        let runtime = &mut context.world;
        self.emitters.prepare(
            runtime.world,
            crate::components::registry::ComponentStorage::particle_emitter_ptr,
        );
        self.playbacks.prepare(
            runtime.world,
            crate::components::registry::ComponentStorage::particle_playback_ptr,
        );
        for &(entity, binding) in self.emitters.entries() {
            let index = entity.index() as usize;
            if runtime.world.components.particle_playback(index).is_some() {
                binding
                    .get_mut(&mut runtime.world.components)
                    .runtime
                    .particles
                    .clear();
                continue;
            }
            let Ok(model) = crate::systems::hierarchy::evaluated_affine(runtime.world, entity)
                .and_then(|m| m.render_matrix())
            else {
                continue;
            };
            {
                let emitter = binding.get(&runtime.world.components);
                let surface = if emitter.shape == 3 {
                    source_key_from_fields(
                        runtime.asset_acquisition,
                        runtime.world.id,
                        None,
                        crate::systems::particles::PARTICLE_SURFACE_TYPE,
                        &emitter.source,
                        emitter.variant,
                    )
                    .and_then(|key| {
                        runtime
                            .asset_acquisition
                            .get(key)?
                            .data()?
                            .decoded()
                            .downcast_ref::<ParticleEmissionSurface>()
                    })
                } else {
                    None
                };
                let emitter = binding.get_mut(&mut runtime.world.components);
                let mut state = std::mem::take(&mut emitter.runtime);
                simulate(emitter, &mut state, dt, model, surface);
                emitter.runtime = state;
            }
        }
        for &(entity, binding) in self.playbacks.entries() {
            let index = entity.index() as usize;
            if runtime.world.components.particle_emitter(index).is_some() {
                binding
                    .get_mut(&mut runtime.world.components)
                    .runtime
                    .particles
                    .clear();
                continue;
            }
            if crate::systems::hierarchy::evaluated_affine(runtime.world, entity)
                .and_then(|m| m.render_matrix())
                .is_err()
            {
                continue;
            }
            {
                let playback = binding.get(&runtime.world.components);
                let cache = source_key_from_fields(
                    runtime.asset_acquisition,
                    runtime.world.id,
                    None,
                    PARTICLE_CACHE_TYPE,
                    &playback.source,
                    playback.variant,
                )
                .and_then(|key| {
                    runtime
                        .asset_acquisition
                        .get(key)?
                        .data()?
                        .decoded()
                        .downcast_ref::<ParticleCache>()
                });
                let playback = binding.get_mut(&mut runtime.world.components);
                if let Some(cache) = cache {
                    cache.sample(playback.time, &mut playback.runtime);
                } else {
                    playback.runtime.particles.clear();
                }
            }
        }
    }
}

pub(in crate::world) fn state(
    world: &crate::world::WorldSimulationState,
    index: usize,
) -> Option<&ParticleRuntimeState> {
    match (
        world.components.particle_emitter(index),
        world.components.particle_playback(index),
    ) {
        (Some(e), None) => Some(&e.runtime),
        (None, Some(p)) => Some(&p.runtime),
        _ => None,
    }
}

impl crate::WorldContext<'_> {
    /// Read-only completed simulation output for headless consumers and renderer preparation.
    pub fn particles(&self, entity: crate::EntityId) -> Option<&[super::Particle]> {
        self.world.state.entities.get(&entity)?;
        Some(&state(self.world, entity.index() as usize)?.particles)
    }
}
