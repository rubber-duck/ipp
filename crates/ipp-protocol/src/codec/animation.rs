use super::{ProtocolError, Reader, Writer};
use crate::wire::*;
use ipp_core::{EntityId, systems::animation::*};

impl Reader<'_> {
    pub(super) fn controller_id(&mut self) -> Result<AnimationControllerId, ProtocolError> {
        let id = self.u64()?;
        if id == 0 {
            return Err(ProtocolError::Malformed("zero animation controller"));
        }
        Ok(AnimationControllerId::from_bits(id))
    }

    pub(super) fn playback_control(&mut self) -> Result<AnimationPlaybackControl, ProtocolError> {
        let action =
            u8::try_from(self.u32()?).map_err(|_| ProtocolError::Malformed("playback control"))?;
        let time = self.f64()?;
        let speed = self.f32()?;
        if !time.is_finite()
            || time < 0.0
            || action != PLAYBACK_CONTROL_SEEK && time != 0.0
            || action != PLAYBACK_CONTROL_PLAY_AT_SPEED && speed != 0.0
        {
            return Err(ProtocolError::Malformed("playback control time"));
        }
        Ok(match action {
            PLAYBACK_CONTROL_PLAY => AnimationPlaybackControl::Play,
            PLAYBACK_CONTROL_PAUSE => AnimationPlaybackControl::Pause,
            PLAYBACK_CONTROL_STOP => AnimationPlaybackControl::Stop,
            PLAYBACK_CONTROL_SEEK => AnimationPlaybackControl::Seek(time),
            PLAYBACK_CONTROL_RESTART => AnimationPlaybackControl::Restart,
            PLAYBACK_CONTROL_PLAY_AT_SPEED => AnimationPlaybackControl::PlayAtSpeed(speed),
            _ => return Err(ProtocolError::Malformed("playback control")),
        })
    }

    pub(super) fn controller_description(
        &mut self,
    ) -> Result<AnimationControllerDescription, ProtocolError> {
        let speed = self.f32()?;
        let looping = self.boolean()?;
        let n = self.u32()?;
        let mut drivers = Vec::new();
        for _ in 0..n {
            let source = self.text()?;
            let variant = self.u32()?;
            let track = self.u32()?;
            let target = EntityId::from_bits(self.u64()?);
            let kind = self.u8()?;
            let property = match kind {
                ANIMATION_TARGET_ENTITY_LINK => AnimationTrackTarget::EntityLink,
                ANIMATION_TARGET_DYNAMIC => AnimationTrackTarget::DynamicProperty {
                    component: self.u16()?,
                    name: self.string()?,
                },
                ANIMATION_TARGET_PROPERTY => {
                    let component = self.u16()?;
                    let count = self.count(crate::MAX_ANIMATION_TARGET_INDICES)?;
                    let offsets = (0..count).map(|_| self.u32()).collect::<Result<_, _>>()?;
                    AnimationTrackTarget::AnimationProperty(AnimationProperty {
                        component,
                        offsets,
                    })
                }
                ANIMATION_TARGET_JOINTS => {
                    let count = self.count(crate::MAX_ANIMATION_TARGET_INDICES)?;
                    let indices = (0..count).map(|_| self.u32()).collect::<Result<_, _>>()?;
                    AnimationTrackTarget::Joints(indices)
                }
                _ => return Err(ProtocolError::Malformed("animation target kind")),
            };
            let binding_count = self.count(MAX_ANIMATION_ENTITY_BINDINGS as usize)?;
            let entity_bindings = (0..binding_count)
                .map(|_| Ok(EntityId::from_bits(self.u64()?)))
                .collect::<Result<Vec<_>, ProtocolError>>()?;
            drivers.push(AnimationDriverDescription {
                source,
                variant,
                track,
                target,
                property,
                entity_bindings,
                weight: self.f32()?,
                additive: self.boolean()?,
                reference_time: self.f32()?,
                repeat: self.boolean()?,
            });
        }
        Ok(AnimationControllerDescription {
            drivers,
            speed,
            looping,
        })
    }

    pub(super) fn controller_transition(
        &mut self,
    ) -> Result<AnimationControllerTransition, ProtocolError> {
        let description = self.controller_description()?;
        let duration = self.f64()?;
        if !duration.is_finite() || duration < 0.0 {
            return Err(ProtocolError::Malformed("animation transition duration"));
        }
        let easing = match u8::try_from(self.u32()?)
            .map_err(|_| ProtocolError::Malformed("animation transition easing"))?
        {
            ANIMATION_TRANSITION_LINEAR => AnimationTransitionEasing::Linear,
            ANIMATION_TRANSITION_SMOOTHSTEP => AnimationTransitionEasing::Smoothstep,
            _ => return Err(ProtocolError::Malformed("animation transition easing")),
        };
        let start_time = match u8::try_from(self.u32()?)
            .map_err(|_| ProtocolError::Malformed("animation transition start time"))?
        {
            ANIMATION_TRANSITION_RESTART => AnimationTransitionStartTime::Restart,
            ANIMATION_TRANSITION_PRESERVE => AnimationTransitionStartTime::Preserve,
            ANIMATION_TRANSITION_MATCH_PHASE => AnimationTransitionStartTime::MatchPhase,
            ANIMATION_TRANSITION_SEEK => {
                let time = self.f64()?;
                if !time.is_finite() || time < 0.0 {
                    return Err(ProtocolError::Malformed("animation transition seek time"));
                }
                AnimationTransitionStartTime::Seek(time)
            }
            _ => return Err(ProtocolError::Malformed("animation transition start time")),
        };
        if !matches!(&start_time, AnimationTransitionStartTime::Seek(_)) && self.f64()? != 0.0 {
            return Err(ProtocolError::Malformed("animation transition seek time"));
        }
        Ok(AnimationControllerTransition {
            description,
            duration,
            easing,
            start_time,
        })
    }
}

impl Writer {
    pub(super) fn controller_state(
        &mut self,
        controller: &AnimationControllerState,
    ) -> Result<(), ProtocolError> {
        self.u64(controller.id.to_bits())?;
        self.u32(controller.state as u32)?;
        self.f64(controller.time)
    }

    pub(super) fn controller(
        &mut self,
        controller: &AnimationControllerSnapshot,
    ) -> Result<(), ProtocolError> {
        self.controller_state(&AnimationControllerState {
            id: controller.id,
            state: controller.state,
            time: controller.time,
        })?;
        self.controller_description(&controller.description)?;
        let Some(transition) = &controller.transition else {
            return self.u8(OPTION_NONE);
        };
        self.u8(OPTION_SOME)?;
        self.f64(transition.duration)?;
        self.f64(transition.elapsed)?;
        self.u32(u32::from(match transition.easing {
            AnimationTransitionEasing::Linear => ANIMATION_TRANSITION_LINEAR,
            AnimationTransitionEasing::Smoothstep => ANIMATION_TRANSITION_SMOOTHSTEP,
        }))?;
        self.u8(u8::from(transition.pending))
    }

    fn controller_description(
        &mut self,
        description: &AnimationControllerDescription,
    ) -> Result<(), ProtocolError> {
        self.f32(description.speed)?;
        self.u8(u8::from(description.looping))?;
        self.count(description.drivers.len(), u32::MAX as usize)?;
        for driver in &description.drivers {
            self.string(&driver.source)?;
            self.u32(driver.variant)?;
            self.u32(driver.track)?;
            self.u64(driver.target.to_bits())?;
            match &driver.property {
                AnimationTrackTarget::EntityLink => self.u8(ANIMATION_TARGET_ENTITY_LINK)?,
                AnimationTrackTarget::DynamicProperty {
                    component,
                    name,
                } => {
                    self.u8(ANIMATION_TARGET_DYNAMIC)?;
                    self.u16(*component)?;
                    self.string(name)?;
                }
                AnimationTrackTarget::AnimationProperty(property) => {
                    self.u8(ANIMATION_TARGET_PROPERTY)?;
                    self.u16(property.component)?;
                    self.count(property.offsets.len(), crate::MAX_ANIMATION_TARGET_INDICES)?;
                    for index in &property.offsets {
                        self.u32(*index)?;
                    }
                }
                AnimationTrackTarget::Joints(indices) => {
                    self.u8(ANIMATION_TARGET_JOINTS)?;
                    self.count(indices.len(), crate::MAX_ANIMATION_TARGET_INDICES)?;
                    for index in indices {
                        self.u32(*index)?;
                    }
                }
            }
            self.count(
                driver.entity_bindings.len(),
                MAX_ANIMATION_ENTITY_BINDINGS as usize,
            )?;
            for entity in &driver.entity_bindings {
                self.u64(entity.to_bits())?;
            }
            self.f32(driver.weight)?;
            self.u8(u8::from(driver.additive))?;
            self.f32(driver.reference_time)?;
            self.u8(u8::from(driver.repeat))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_descriptions_round_trip_more_than_256_drivers() {
        let description = AnimationControllerDescription {
            drivers: (0..300)
                .map(|track| AnimationDriverDescription {
                    source: "memory:animation".into(),
                    variant: 0,
                    track,
                    target: EntityId::from_bits(u64::from(track) + 1),
                    property: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                        component: 1,
                        offsets: vec![0],
                    }),
                    entity_bindings: Vec::new(),
                    weight: 1.0,
                    additive: false,
                    reference_time: 0.0,
                    repeat: false,
                })
                .collect(),
            speed: -1.0,
            looping: false,
        };
        let mut writer = Writer::new(Vec::new());
        writer.controller_description(&description).unwrap();
        let mut reader = Reader {
            bytes: &writer.0,
            at: 0,
        };
        let decoded = reader.controller_description().unwrap();
        assert_eq!(decoded.drivers.len(), 300);
        assert_eq!(decoded.speed, -1.0);
        assert_eq!(decoded.drivers[299].track, 299);
        assert_eq!(reader.at, writer.0.len());
    }

    #[test]
    fn structural_driver_keeps_world_bindings_outside_immutable_key_slots() {
        let description = AnimationControllerDescription {
            drivers: vec![AnimationDriverDescription {
                source: "memory:structural-v4".into(),
                variant: 2,
                track: 3,
                target: EntityId::from_bits(0x1_0000_0001),
                property: AnimationTrackTarget::EntityLink,
                entity_bindings: vec![
                    EntityId::from_bits(0x1_0000_0002),
                    EntityId::from_bits(0x1_0000_0003),
                ],
                weight: 1.0,
                additive: false,
                reference_time: 0.0,
                repeat: true,
            }],
            speed: 1.0,
            looping: true,
        };
        let mut writer = Writer::new(Vec::new());
        writer.controller_description(&description).unwrap();
        let mut reader = Reader {
            bytes: &writer.0,
            at: 0,
        };
        assert_eq!(reader.controller_description().unwrap(), description);
        assert_eq!(reader.at, writer.0.len());
    }

    #[test]
    fn structural_bindings_use_message_budget_not_a_retained_count_quota() {
        let driver = AnimationDriverDescription {
            source: "memory:structural-v4".into(),
            variant: 0,
            track: 0,
            target: EntityId::from_bits(1),
            property: AnimationTrackTarget::EntityLink,
            entity_bindings: Vec::new(),
            weight: 1.0,
            additive: false,
            reference_time: 0.0,
            repeat: false,
        };
        let description = AnimationControllerDescription {
            drivers: vec![driver.clone()],
            speed: 1.0,
            looping: false,
        };
        let mut writer = Writer::new(Vec::new());
        writer.controller_description(&description).unwrap();
        let mut reader = Reader {
            bytes: &writer.0,
            at: 0,
        };
        assert_eq!(reader.controller_description().unwrap(), description);

        let mut many = description;
        many.drivers[0].entity_bindings = (1..=4097).map(EntityId::from_bits).collect();
        let mut writer = Writer::new(Vec::new());
        writer.controller_description(&many).unwrap();
        let mut reader = Reader {
            bytes: &writer.0,
            at: 0,
        };
        assert_eq!(reader.controller_description().unwrap(), many);
        assert_eq!(reader.at, writer.0.len());

        let truncated = &writer.0[..writer.0.len() - 1];
        let mut reader = Reader {
            bytes: truncated,
            at: 0,
        };
        assert!(reader.controller_description().is_err());

        many.drivers[0].entity_bindings =
            vec![EntityId::from_bits(1); MAX_ANIMATION_ENTITY_BINDINGS as usize];
        let mut writer = Writer::new(Vec::new());
        assert!(matches!(
            writer.controller_description(&many),
            Err(ProtocolError::Limit("message"))
        ));
    }

    #[test]
    fn playback_control_rejects_nonfinite_and_negative_times() {
        for time in [f64::NAN, f64::INFINITY, -1.0] {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&(PLAYBACK_CONTROL_SEEK as u32).to_le_bytes());
            bytes.extend_from_slice(&time.to_le_bytes());
            bytes.extend_from_slice(&0.0f32.to_le_bytes());
            let mut reader = Reader {
                bytes: &bytes,
                at: 0,
            };
            assert!(reader.playback_control().is_err());
        }
    }

    #[test]
    fn controller_transition_rejects_invalid_duration_and_seek_time() {
        for (duration, seek_time) in [(f64::NAN, 0.0f64), (-1.0, 0.0), (1.0, -1.0)] {
            let mut writer = Writer::new(Vec::new());
            writer
                .controller_description(&AnimationControllerDescription::default())
                .unwrap();
            writer.0.extend_from_slice(&duration.to_le_bytes());
            writer
                .u32(u32::from(ANIMATION_TRANSITION_LINEAR))
                .expect("easing tag");
            writer
                .u32(u32::from(ANIMATION_TRANSITION_SEEK))
                .expect("start-time tag");
            writer.0.extend_from_slice(&seek_time.to_le_bytes());
            let mut reader = Reader {
                bytes: &writer.0,
                at: 0,
            };
            assert!(reader.controller_transition().is_err());
        }
    }
}
