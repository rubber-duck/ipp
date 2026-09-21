//! Headless live emission and portable cache playback; private particles are not entities.
/// CPU mesh representation used for surface emission.
pub const PARTICLE_SURFACE_TYPE: crate::services::asset_management::AssetTypeId =
    crate::services::asset_management::AssetTypeId(16);

mod cache;
mod components;
mod simulation;
mod system;

pub(crate) use cache::particle_cache_loader;
pub use cache::{PARTICLE_CACHE_TYPE, ParticleCache, ParticleCacheFrame, ParticleCacheSample};
pub use components::{ParticleEmitter, ParticleMesh, ParticlePlayback, ParticleSprite};
pub use simulation::{Particle, ParticleRuntimeState};
pub(in crate::world) use system::state as particle_state;
pub use system::{ParticleSystem, ParticleSystemFactory};

mod rendering;
pub use rendering::ParticleRenderData;
pub(in crate::world) use rendering::prepare_particles;

#[cfg(test)]
#[path = "particle_tests.rs"]
mod tests;

pub(crate) use simulation::particle_surface_loader;
