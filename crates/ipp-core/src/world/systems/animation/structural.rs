//! Structural drivers place their target through ordinary link writes. They are
//! absolute writers: a driver that stops leaves its target where it placed it.

use super::*;
use crate::{EntityPlacement, services::asset_management::AssetKey};

pub(super) fn structural_slots_valid(
    track: &AnimationTrack<AnimationEntityPlacementKey>,
    bindings: &[EntityId],
) -> bool {
    track.keys.iter().all(|key| {
        !key.value
            .parent
            .is_some_and(|slot| slot as usize >= bindings.len())
            && !key
                .value
                .before
                .is_some_and(|slot| slot as usize >= bindings.len())
    })
}

fn resolved_placements(
    track: &AnimationTrack<AnimationEntityPlacementKey>,
    bindings: &[EntityId],
) -> Result<Vec<EntityPlacement>, ErrorReason> {
    if !matches!(track.target, AnimationTrackTarget::EntityLink)
        || !structural_slots_valid(track, bindings)
    {
        return Err(ErrorReason::InvalidField);
    }
    Ok(track
        .keys
        .iter()
        .map(|key| {
            let resolve = |slot: Option<u32>| slot.map(|index| bindings[index as usize]);
            EntityPlacement {
                parent: resolve(key.value.parent),
                before: resolve(key.value.before),
            }
        })
        .collect())
}

#[derive(Debug)]
pub(super) struct AnimationStructuralDriver {
    description: AnimationDriverDescription,
    clip: AssetKey,
    duration: f64,
    track: Option<std::sync::Arc<AnimationTrack<AnimationEntityPlacementKey>>>,
    placements: Vec<EntityPlacement>,
    applied_index: Option<usize>,
    last_repeat_cycle: Option<f64>,
}

impl AnimationStructuralDriver {
    pub(super) fn duration(&self) -> f64 {
        self.duration
    }

    pub(super) fn clip(&self) -> AssetKey {
        self.clip
    }

    pub(super) fn target(&self) -> EntityId {
        self.description.target
    }

    pub(super) fn suspend_track(&mut self) {
        self.track = None;
    }

    pub(super) fn reset_selection(&mut self) {
        self.applied_index = None;
        self.last_repeat_cycle = None;
    }
}

impl AnimationAccess<'_, '_> {
    pub(super) fn structural_sources_ready(
        &self,
        controller: &AnimationController,
    ) -> Result<bool, ErrorReason> {
        for description in &controller.snapshot.description.drivers {
            if !matches!(description.property, AnimationTrackTarget::EntityLink) {
                continue;
            }
            if !self
                .context
                .world
                .state
                .entities
                .contains_key(&description.target)
            {
                return Err(ErrorReason::MissingComponent);
            }
            let Some(source) = self.read().source_key(description) else {
                return Ok(false);
            };
            if !self.read().clip_ready(source)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(super) fn bind_structural_drivers(
        &mut self,
        controller: &mut AnimationController,
    ) -> Result<(), ErrorReason> {
        if !controller.structural_drivers.is_empty() {
            return Ok(());
        }
        let result = (|| {
            for description in &controller.snapshot.description.drivers {
                if !matches!(description.property, AnimationTrackTarget::EntityLink) {
                    continue;
                }
                super::binding::validate_target_support(
                    &self.context.world.manifest,
                    &description.property,
                )?;
                let source = self
                    .read()
                    .source_key(description)
                    .ok_or(ErrorReason::InvalidAsset)?;
                let clip = self
                    .read()
                    .clip_by_key(source)
                    .ok_or(ErrorReason::InvalidAsset)?;
                let track = clip
                    .shared_track::<AnimationEntityPlacementKey>(description.track as usize)
                    .ok_or(ErrorReason::InvalidField)?;
                let placements = resolved_placements(&track, &description.entity_bindings)?;
                if description
                    .entity_bindings
                    .iter()
                    .any(|entity| !self.context.world.state.entities.contains_key(entity))
                {
                    return Err(ErrorReason::InvalidEntity);
                }
                let duration = clip.duration();
                if self.context.entity_link(description.target).is_none() {
                    return Err(ErrorReason::InvalidEntity);
                }
                controller
                    .structural_drivers
                    .push(AnimationStructuralDriver {
                        description: description.clone(),
                        clip: source,
                        duration,
                        track: Some(track),
                        placements,
                        applied_index: None,
                        last_repeat_cycle: None,
                    });
                controller.duration = controller.duration.max(duration);
            }
            Ok(())
        })();
        if result.is_err() {
            controller.structural_drivers.clear();
        }
        result
    }

    pub(super) fn prepare_structural_drivers(
        &self,
        controller: &mut AnimationController,
    ) -> Result<(), ErrorReason> {
        for driver in &mut controller.structural_drivers {
            if driver.track.is_some() {
                continue;
            }
            let description = &driver.description;
            let clip = self
                .read()
                .clip_by_key(driver.clip)
                .ok_or(ErrorReason::InvalidAsset)?;
            let track = clip
                .shared_track::<AnimationEntityPlacementKey>(description.track as usize)
                .ok_or(ErrorReason::InvalidField)?;
            driver.placements = resolved_placements(&track, &description.entity_bindings)?;
            driver.track = Some(track);
        }
        Ok(())
    }

    pub(super) fn sample_structural_drivers(
        &mut self,
        controller: &mut AnimationController,
    ) -> Result<(), ErrorReason> {
        for driver in &mut controller.structural_drivers {
            let description = &driver.description;
            let track = driver.track.as_ref().ok_or(ErrorReason::InvalidAsset)?;
            let time = if description.repeat {
                controller.snapshot.time.rem_euclid(driver.duration)
            } else {
                controller.snapshot.time
            };
            let selected = track
                .keys
                .partition_point(|key| key.time <= time)
                .saturating_sub(1);
            let repeat_cycle = description
                .repeat
                .then(|| (controller.snapshot.time / driver.duration).floor());
            let repeated_boundary = repeat_cycle.is_some()
                && driver.last_repeat_cycle.is_some()
                && repeat_cycle != driver.last_repeat_cycle;
            if driver.applied_index == Some(selected) && !repeated_boundary {
                driver.last_repeat_cycle = repeat_cycle;
                continue;
            }
            self.context.place_entity_link(
                self.system,
                driver.description.target,
                driver.placements[selected],
            )?;
            driver.applied_index = Some(selected);
            driver.last_repeat_cycle = repeat_cycle;
        }
        Ok(())
    }

    /// Stopped drivers leave their targets where they placed them; other
    /// structural drivers of those targets place them again.
    pub(super) fn release_structural_drivers(&mut self, drivers: Vec<AnimationStructuralDriver>) {
        for driver in drivers {
            self.system
                .state
                .resample_structural_target(driver.target());
        }
    }

    pub(super) fn invalidate_structural_asset(&mut self, key: AssetKey) {
        let affected: Vec<_> = self
            .system
            .state
            .controllers
            .iter()
            .filter_map(|(&id, controller)| {
                controller
                    .structural_drivers
                    .iter()
                    .any(|driver| driver.clip == key)
                    .then_some(id)
            })
            .collect();
        for id in affected {
            self.withdraw_controller(id);
            let controller = self.system.state.controllers.get_mut(&id).unwrap();
            let drivers = std::mem::take(&mut controller.structural_drivers);
            self.release_structural_drivers(drivers);
            let controller = self.system.state.controllers.get_mut(&id).unwrap();
            controller.clear_drivers();
            controller.snapshot.state = AnimationPlaybackStatus::Stopped;
            controller.snapshot.transition = None;
            controller.transition = None;
            self.system
                .state
                .playback_events
                .push(AnimationPlaybackEvent {
                    controller: AnimationControllerState {
                        id,
                        state: AnimationPlaybackStatus::Stopped,
                        time: controller.snapshot.time,
                    },
                    kind: AnimationPlaybackEventKind::Invalidated,
                    reason: Some(ErrorReason::InvalidAsset),
                });
        }
    }
}

impl AnimationSystemState {
    /// Another writer placed `target`, or one of its structural drivers stopped:
    /// the drivers still placing it apply their sample again next frame.
    pub(super) fn resample_structural_target(&mut self, target: EntityId) {
        for id in self
            .structural_target_controllers
            .get(&target)
            .into_iter()
            .flatten()
        {
            if let Some(controller) = self.controllers.get_mut(id) {
                for driver in &mut controller.structural_drivers {
                    if driver.target() == target {
                        driver.reset_selection();
                    }
                }
            }
        }
    }
}
