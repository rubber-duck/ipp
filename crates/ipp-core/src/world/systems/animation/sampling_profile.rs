//! Read-only profiling of actual bound numeric curves and phase-local typed access.
//! Probe setup resolves immutable tracks before timing and pins their World borrow.

use super::{AnimationClip, AnimationSample, AnimationSystem};
use crate::{ErrorReason, WorldContext};

impl WorldContext<'_> {
    /// Rebuild only prepared track access for controlled locality profiling.
    /// Runs outside frame timing and preserves target bindings, originals and clocks.
    #[doc(hidden)]
    pub fn profile_rebind_animation_tracks(
        &mut self,
    ) -> Result<(usize, usize, usize), ErrorReason> {
        self.with_system::<AnimationSystem, _>(AnimationSystem::ID, |system, context| {
            let mut count = 0;
            let mut copied_bytes = 0;
            let mut segment_bytes = 0;
            for controller in system
                .state
                .controllers
                .values_mut()
                .filter(|controller| controller.ready)
            {
                for driver in &mut controller.drivers {
                    let clip = context
                        .asset_acquisition
                        .get_typed::<AnimationClip>(driver.clip())
                        .ok_or(ErrorReason::InvalidAsset)?;
                    driver.suspend_track();
                    driver.resolve_track(clip)?;
                    count += 1;
                    copied_bytes += driver.copied_track_bytes();
                    segment_bytes += driver.segment_bytes();
                }
            }
            Ok((count, copied_bytes, segment_bytes))
        })
        .ok_or(ErrorReason::InvalidValue)?
    }

    /// Experimental cost floor: actual bound numeric curves and clip lookups,
    /// without component reads/staging/commits or joint-pose reconstruction.
    /// Outputs are consumed, never applied; Host/controller clocks are unchanged.
    #[doc(hidden)]
    pub fn profile_animation_curve_samples(&self, time_offset: f64) -> Result<usize, ErrorReason> {
        let animation = self
            .system::<AnimationSystem>(AnimationSystem::ID)
            .expect("compiled animation system");
        let mut count = 0;
        for controller in animation.state.controllers.values() {
            for driver in &controller.drivers {
                #[cfg(feature = "skeletal-animation")]
                if matches!(
                    driver.runtime_target(),
                    super::driver::AnimationRuntimeTarget::JointLocal { .. }
                ) {
                    continue;
                }
                let sample = driver
                    .sample_bound(controller.snapshot.time + time_offset, driver.original())?;
                std::hint::black_box(sample);
                count += 1;
            }
        }
        Ok(count)
    }

    /// Experimental phase-local typed references to the same unit-weight numeric
    /// curves. Resolution/allocation happens before timing. The returned closure
    /// borrows this World, so mutation, asset release and reuse cannot intervene.
    #[doc(hidden)]
    pub fn profile_resolved_curve_sampler(
        &self,
        time_offset: f64,
    ) -> Result<impl Fn() -> usize + '_, ErrorReason> {
        let animation = self
            .system::<AnimationSystem>(AnimationSystem::ID)
            .expect("compiled animation system");
        let mut scalars = Vec::new();
        let mut rotations = Vec::new();
        for controller in animation.state.controllers.values() {
            for driver in &controller.drivers {
                let description = driver.description();
                if description.additive || description.weight != 1.0 {
                    return Err(ErrorReason::InvalidValue);
                }
                #[cfg(feature = "skeletal-animation")]
                if matches!(
                    driver.runtime_target(),
                    super::driver::AnimationRuntimeTarget::JointLocal { .. }
                ) {
                    continue;
                }
                let clip = self
                    .asset_acquisition
                    .get_typed::<AnimationClip>(driver.clip())
                    .ok_or(ErrorReason::InvalidAsset)?;
                let time = controller.snapshot.time + time_offset;
                let time = if description.repeat {
                    time.rem_euclid(clip.duration())
                } else {
                    time
                };
                let expected = driver.sample(
                    clip,
                    controller.snapshot.time + time_offset,
                    driver.original(),
                )?;
                if let Some(track) = clip.typed_track::<f32>(description.track as usize) {
                    if track.sample(time).into_value() != expected {
                        return Err(ErrorReason::InvalidValue);
                    }
                    scalars.push((track, time));
                } else if let Some(track) = clip.typed_track::<[f32; 4]>(description.track as usize)
                {
                    if track.sample(time).into_value() != expected {
                        return Err(ErrorReason::InvalidValue);
                    }
                    rotations.push((track, time));
                } else {
                    return Err(ErrorReason::InvalidField);
                }
            }
        }
        Ok(move || {
            for (track, time) in &scalars {
                std::hint::black_box(track.sample(*time));
            }
            for (track, time) in &rotations {
                std::hint::black_box(track.sample(*time));
            }
            scalars.len() + rotations.len()
        })
    }
}
