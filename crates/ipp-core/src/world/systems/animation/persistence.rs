//! Animation-owned persistent descriptions and entity remapping.

use super::*;

impl AnimationSystem {
    /// Snapshot only controller declarations and clocks, never resolved runtime targets.
    pub fn persistent_state(&self) -> AnimationPersistentState {
        let transitions = self
            .state
            .controllers
            .iter()
            .filter_map(|(&id, controller)| {
                if self.is_derived_skin_controller(id) {
                    return None;
                }
                let transition = controller.transition.as_deref()?;
                let source = match &transition.source {
                    super::system_state::AnimationTransitionSource::Live(source) => {
                        AnimationPersistentTransitionSource::Live(source.snapshot.clone())
                    }
                    super::system_state::AnimationTransitionSource::Frozen {
                        values,
                        bindings,
                        reference_time,
                        reference_duration,
                        ..
                    } => AnimationPersistentTransitionSource::Frozen {
                        values: transition
                            .program
                            .as_ref()
                            .map(|program| program.persistent_frozen_values())
                            .unwrap_or_else(|| values.clone())
                            .iter()
                            .map(|value| AnimationFrozenTransitionValue {
                                target: value.target,
                                property: value.property.clone(),
                                value: value.value.clone(),
                                baseline: value.baseline.clone(),
                            })
                            .collect(),
                        bindings: bindings.snapshot.clone(),
                        reference_time: *reference_time,
                        reference_duration: *reference_duration,
                    },
                };
                Some(AnimationPersistentTransition {
                    id,
                    source,
                    start_time: if transition.program.is_some() {
                        AnimationTransitionStartTime::Seek(controller.snapshot.time)
                    } else {
                        transition.start_time
                    },
                })
            })
            .collect();
        AnimationPersistentState {
            next_id: self.state.next_id,
            controllers: self
                .state
                .controllers
                .iter()
                .filter(|(id, _)| !self.is_derived_skin_controller(**id))
                .map(|(_, controller)| controller.snapshot.clone())
                .collect(),
            transitions,
            directional_starts: self
                .state
                .controllers
                .iter()
                .filter_map(|(&id, controller)| {
                    (!self.is_derived_skin_controller(id) && controller.directional_start_pending)
                        .then_some(id)
                })
                .collect(),
        }
    }

    pub(in crate::world) fn save_persistent_state(
        &self,
        ids: &std::collections::BTreeMap<EntityId, crate::EntityPersistentId>,
        bytes: &mut usize,
        max_bytes: usize,
    ) -> Result<AnimationPersistentState, String> {
        // Validate the borrowed graph and budget before allocating owned descriptions.
        for (&id, controller) in &self.state.controllers {
            if self.is_derived_skin_controller(id) {
                continue;
            }
            *bytes = bytes.checked_add(128).ok_or("Snapshot size overflow")?;
            if *bytes > max_bytes {
                return Err("Snapshot byte budget exhausted".into());
            }
            for driver in &controller.snapshot.description.drivers {
                *bytes = bytes
                    .checked_add(
                        128 + driver.source.len()
                            + driver.property.indices().len() * 4
                            + match &driver.property {
                                AnimationTrackTarget::DynamicProperty {
                                    name,
                                    ..
                                } => name.len(),
                                _ => 0,
                            },
                    )
                    .ok_or("Snapshot size overflow")?;
                if *bytes > max_bytes {
                    return Err("Snapshot byte budget exhausted".into());
                }
                if !ids.contains_key(&driver.target) {
                    return Err("Animation references excluded or missing entity".into());
                }
            }
        }
        let directional_count = self
            .state
            .controllers
            .iter()
            .filter(|(id, controller)| {
                !self.is_derived_skin_controller(**id) && controller.directional_start_pending
            })
            .count();
        *bytes = bytes
            .checked_add(directional_count.saturating_mul(8))
            .ok_or("Snapshot size overflow")?;
        if *bytes > max_bytes {
            return Err("Snapshot byte budget exhausted".into());
        }
        for (&id, controller) in &self.state.controllers {
            if self.is_derived_skin_controller(id) {
                continue;
            }
            let Some(transition) = controller.transition.as_deref() else {
                continue;
            };
            let source = match &transition.source {
                super::system_state::AnimationTransitionSource::Live(source) => source.as_ref(),
                super::system_state::AnimationTransitionSource::Frozen {
                    values,
                    bindings,
                    ..
                } => {
                    for value in values {
                        *bytes = bytes
                            .checked_add(160 + value.property.owned_bytes())
                            .ok_or("Snapshot size overflow")?;
                        if *bytes > max_bytes {
                            return Err("Snapshot byte budget exhausted".into());
                        }
                        if !ids.contains_key(&value.target) {
                            return Err(
                                "Animation transition references excluded or missing entity".into(),
                            );
                        }
                    }
                    bindings.as_ref()
                }
            };
            for driver in &source.snapshot.description.drivers {
                *bytes = bytes
                    .checked_add(128 + driver.source.len() + driver.property.owned_bytes())
                    .ok_or("Snapshot size overflow")?;
                if *bytes > max_bytes {
                    return Err("Snapshot byte budget exhausted".into());
                }
                if !ids.contains_key(&driver.target) {
                    return Err("Animation transition references excluded or missing entity".into());
                }
            }
        }
        let mut state = self.persistent_state();
        for controller in &mut state.controllers {
            for driver in &mut controller.description.drivers {
                driver.target = EntityId::from_bits(ids[&driver.target].0);
            }
        }
        for transition in &mut state.transitions {
            match &mut transition.source {
                AnimationPersistentTransitionSource::Live(source) => {
                    for driver in &mut source.description.drivers {
                        driver.target = EntityId::from_bits(ids[&driver.target].0);
                    }
                }
                AnimationPersistentTransitionSource::Frozen {
                    values,
                    bindings,
                    ..
                } => {
                    for value in values {
                        value.target = EntityId::from_bits(ids[&value.target].0);
                    }
                    for driver in &mut bindings.description.drivers {
                        driver.target = EntityId::from_bits(ids[&driver.target].0);
                    }
                }
            }
        }
        Ok(state)
    }
}

impl AnimationPersistentState {
    /// Encode this subsystem's versioned controller descriptions and clocks.
    pub fn encode(&self, max_bytes: usize) -> Result<Vec<u8>, String> {
        super::codec::validate_values(self)?;
        let mut writer =
            crate::services::world_serialization::binary::WorldBinaryWriter::new(max_bytes);
        writer.u32(5)?;
        super::codec::encode(&mut writer, self)?;
        Ok(writer.bytes)
    }

    /// Decode bounded controller state; World restoration resolves its durable targets.
    pub fn decode(bytes: &[u8], max_bytes: usize) -> Result<Self, String> {
        if bytes.len() > max_bytes {
            return Err("Animation state byte budget exhausted".into());
        }
        let mut reader =
            crate::services::world_serialization::binary::WorldBinaryReader::new(bytes, max_bytes);
        let version = reader.u32()?;
        if !matches!(version, 1..=5) {
            return Err("Unsupported animation state version".into());
        }
        let state = super::codec::decode(&mut reader, version)?;
        reader.end()?;
        super::codec::validate_values(&state)?;
        Ok(state)
    }

    /// Validate durable targets against the captured authored entity/component set.
    pub fn validate_entities(
        &self,
        entities: &[crate::services::world_serialization::WorldSerializedEntity],
    ) -> Result<(), String> {
        let entities: std::collections::BTreeMap<_, _> = entities
            .iter()
            .map(|entity| (entity.persistent_id.0, entity))
            .collect();
        for controller in &self.controllers {
            validate_persistent_drivers(&entities, &controller.description.drivers)?;
        }
        for transition in &self.transitions {
            match &transition.source {
                AnimationPersistentTransitionSource::Live(source) => {
                    validate_persistent_drivers(&entities, &source.description.drivers)?;
                }
                AnimationPersistentTransitionSource::Frozen {
                    values,
                    bindings,
                    ..
                } => {
                    validate_persistent_drivers(&entities, &bindings.description.drivers)?;
                    for value in values {
                        let entity = entities.get(&value.target.to_bits()).ok_or(
                            "Animation transition references an excluded or missing entity",
                        )?;
                        if !entity
                            .components
                            .iter()
                            .any(|component| component.type_id() == value.property.component())
                        {
                            return Err(
                                "Animation transition references an excluded or missing component"
                                    .into(),
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub(in crate::world) fn remap_entities(
        mut self,
        ids: &std::collections::BTreeMap<crate::EntityPersistentId, EntityId>,
    ) -> Result<Self, String> {
        for controller in &self.controllers {
            for driver in &controller.description.drivers {
                if !ids.contains_key(&crate::EntityPersistentId(driver.target.to_bits())) {
                    return Err("Missing persistent animation target".into());
                }
            }
        }
        for transition in &self.transitions {
            match &transition.source {
                AnimationPersistentTransitionSource::Live(source) => {
                    validate_remap_targets(&source.description.drivers, ids)?;
                }
                AnimationPersistentTransitionSource::Frozen {
                    values,
                    bindings,
                    ..
                } => {
                    validate_remap_targets(&bindings.description.drivers, ids)?;
                    for value in values {
                        if !ids.contains_key(&crate::EntityPersistentId(value.target.to_bits())) {
                            return Err("Missing persistent animation transition target".into());
                        }
                    }
                }
            }
        }
        for controller in &mut self.controllers {
            for driver in &mut controller.description.drivers {
                driver.target = ids[&crate::EntityPersistentId(driver.target.to_bits())];
            }
        }
        for transition in &mut self.transitions {
            match &mut transition.source {
                AnimationPersistentTransitionSource::Live(source) => {
                    for driver in &mut source.description.drivers {
                        driver.target = ids[&crate::EntityPersistentId(driver.target.to_bits())];
                    }
                }
                AnimationPersistentTransitionSource::Frozen {
                    values,
                    bindings,
                    ..
                } => {
                    for value in values {
                        value.target = ids[&crate::EntityPersistentId(value.target.to_bits())];
                    }
                    for driver in &mut bindings.description.drivers {
                        driver.target = ids[&crate::EntityPersistentId(driver.target.to_bits())];
                    }
                }
            }
        }
        Ok(self)
    }
}

fn validate_persistent_drivers(
    entities: &std::collections::BTreeMap<
        u64,
        &crate::services::world_serialization::WorldSerializedEntity,
    >,
    drivers: &[AnimationDriverDescription],
) -> Result<(), String> {
    for driver in drivers {
        let entity = entities
            .get(&driver.target.to_bits())
            .ok_or("Animation driver references an excluded or missing entity")?;
        if !entity
            .components
            .iter()
            .any(|component| component.type_id() == driver.property.component())
        {
            return Err("Animation driver references an excluded or missing component".into());
        }
    }
    Ok(())
}

fn validate_remap_targets(
    drivers: &[AnimationDriverDescription],
    ids: &std::collections::BTreeMap<crate::EntityPersistentId, EntityId>,
) -> Result<(), String> {
    for driver in drivers {
        if !ids.contains_key(&crate::EntityPersistentId(driver.target.to_bits())) {
            return Err("Missing persistent animation transition target".into());
        }
    }
    Ok(())
}
