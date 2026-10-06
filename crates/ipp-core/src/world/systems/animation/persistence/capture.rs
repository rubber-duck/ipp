//! Animation-owned persistent descriptions and entity remapping.

use crate::EntityId;
use crate::systems::animation::{
    AnimationControllerId, AnimationControllerSnapshot, AnimationDriverDescription,
    AnimationSystem, AnimationTrackTarget, AnimationTransitionStartTime, AnimationValue,
    system_state::AnimationTransitionSource,
};

/// Format version of the AnimationSystem's saved state. Version 8 saves each
/// controller's contributions instead of the originals its drivers kept.
const STATE_VERSION: u32 = 8;

/// Complete persistent controller state, including deleted identity high-water.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationPersistentState {
    /// Next fresh identity; never less than any existing or previously used ID.
    pub next_id: u64,
    /// Controllers in identity order.
    pub controllers: Vec<AnimationControllerSnapshot>,
    /// Semantic state for active crossfades, keyed by controller identity.
    pub transitions: Vec<AnimationPersistentTransition>,
    /// Controllers awaiting a negative-speed directional start after clip readiness.
    pub directional_starts: Vec<AnimationControllerId>,
    /// What each controller has added to its fields. Component storage holds the
    /// fields with these contributions in them; stopping subtracts them.
    pub contributions: Vec<AnimationPersistentContribution>,
}

impl Default for AnimationPersistentState {
    fn default() -> Self {
        Self {
            next_id: 1,
            controllers: Vec::new(),
            transitions: Vec::new(),
            directional_starts: Vec::new(),
            contributions: Vec::new(),
        }
    }
}

/// A controller's contribution to one field.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationPersistentContribution {
    /// Contributing controller.
    pub controller: AnimationControllerId,
    /// Driven entity.
    pub target: EntityId,
    /// Driven property, with dynamic properties resolved to their keys.
    pub property: AnimationTrackTarget,
    /// What the controller has added: a float delta, or a rotation composed on
    /// the right of the field.
    pub value: AnimationValue,
}

/// Contribution captured when a crossfade is interrupted; it fades out.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationFrozenTransitionValue {
    /// Entity whose property was captured.
    pub target: EntityId,
    /// Property or single joint represented by this sparse value.
    pub property: AnimationTrackTarget,
    /// Frozen outgoing contribution at the interruption boundary.
    pub value: AnimationValue,
}

/// Durable outgoing side of an active crossfade.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationPersistentTransitionSource {
    /// Independently advancing outgoing controller.
    Live(AnimationControllerSnapshot),
    /// Sparse composite captured from an interrupted transition.
    Frozen {
        /// Captured contributions.
        values: Vec<AnimationFrozenTransitionValue>,
        /// Declaration metadata used only to rebuild stable output bindings.
        bindings: AnimationControllerSnapshot,
        /// Prior destination time used by deferred preserve and phase matching.
        reference_time: f64,
        /// Prior destination duration used by deferred phase matching.
        reference_duration: f64,
    },
}

/// Persistent crossfade state; the destination remains the ordinary controller snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationPersistentTransition {
    /// Controller identity shared with the destination snapshot.
    pub id: AnimationControllerId,
    /// Durable outgoing side.
    pub source: AnimationPersistentTransitionSource,
    /// Deferred destination clock policy.
    pub start_time: AnimationTransitionStartTime,
}

impl AnimationSystem {
    /// Snapshot controller declarations, clocks and what each controller has
    /// added to its fields, never resolved runtime bindings.
    pub fn persistent_state(&self) -> AnimationPersistentState {
        let transitions = self
            .state
            .controllers
            .iter()
            .filter_map(|(&id, controller)| {
                let transition = controller.transition.as_deref()?;
                let source = match &transition.source {
                    AnimationTransitionSource::Live(source) => {
                        AnimationPersistentTransitionSource::Live(source.snapshot.clone())
                    }
                    AnimationTransitionSource::Frozen {
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
                .values()
                .map(|controller| controller.snapshot.clone())
                .collect(),
            transitions,
            directional_starts: self
                .state
                .controllers
                .iter()
                .filter_map(|(&id, controller)| controller.directional_start_pending.then_some(id))
                .collect(),
            contributions: self
                .state
                .controllers
                .iter()
                .flat_map(|(&id, controller)| {
                    controller
                        .contributions
                        .entries()
                        .iter()
                        .filter(|(_, value)| !value.is_empty())
                        .map(move |(identity, value)| AnimationPersistentContribution {
                            controller: id,
                            target: identity.entity,
                            property: identity.property.clone(),
                            value: value.value(),
                        })
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
        for controller in self.state.controllers.values() {
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
                                AnimationTrackTarget::EntityLink => {
                                    driver.entity_bindings.len() * 8
                                }
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
                validate_binding_ids(driver, ids)?;
            }
        }
        let directional_count = self
            .state
            .controllers
            .values()
            .filter(|controller| controller.directional_start_pending)
            .count();
        *bytes = bytes
            .checked_add(directional_count.saturating_mul(8))
            .ok_or("Snapshot size overflow")?;
        if *bytes > max_bytes {
            return Err("Snapshot byte budget exhausted".into());
        }
        for controller in self.state.controllers.values() {
            let Some(transition) = controller.transition.as_deref() else {
                continue;
            };
            let source = match &transition.source {
                AnimationTransitionSource::Live(source) => source.as_ref(),
                AnimationTransitionSource::Frozen {
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
                validate_binding_ids(driver, ids)?;
            }
        }
        let mut state = self.persistent_state();
        for contribution in &mut state.contributions {
            *bytes = bytes
                .checked_add(64 + contribution.property.owned_bytes())
                .ok_or("Snapshot size overflow")?;
            if *bytes > max_bytes {
                return Err("Snapshot byte budget exhausted".into());
            }
            contribution.target = ids
                .get(&contribution.target)
                .map(|id| EntityId::from_bits(id.0))
                .ok_or("Animation contribution references excluded or missing entity")?;
        }
        for controller in &mut state.controllers {
            for driver in &mut controller.description.drivers {
                persist_driver(driver, ids);
            }
        }
        for transition in &mut state.transitions {
            match &mut transition.source {
                AnimationPersistentTransitionSource::Live(source) => {
                    for driver in &mut source.description.drivers {
                        persist_driver(driver, ids);
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
                        persist_driver(driver, ids);
                    }
                }
            }
        }
        Ok(state)
    }
}

impl AnimationPersistentState {
    /// Encode this subsystem's versioned controller descriptions, clocks and contributions.
    pub fn encode(&self, max_bytes: usize) -> Result<Vec<u8>, String> {
        super::codec::validate_values(self)?;
        let mut writer =
            crate::services::world_serialization::binary::WorldBinaryWriter::new(max_bytes);
        writer.u32(STATE_VERSION)?;
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
        if version != STATE_VERSION {
            return Err("Unsupported animation state version".into());
        }
        let state = super::codec::decode(&mut reader)?;
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
                        if !entity.components.iter().any(|component| {
                            component.type_id() == value.property.component_target()
                        }) {
                            return Err(
                                "Animation transition references an excluded or missing component"
                                    .into(),
                            );
                        }
                    }
                }
            }
        }
        for contribution in &self.contributions {
            let entity = entities
                .get(&contribution.target.to_bits())
                .ok_or("Animation contribution references an excluded or missing entity")?;
            if !entity
                .components
                .iter()
                .any(|value| value.type_id() == contribution.property.component_target())
            {
                return Err(
                    "Animation contribution references an excluded or missing component".into(),
                );
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
                validate_remap_targets(std::slice::from_ref(driver), ids)?;
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
                remap_driver(driver, ids);
            }
        }
        for transition in &mut self.transitions {
            match &mut transition.source {
                AnimationPersistentTransitionSource::Live(source) => {
                    for driver in &mut source.description.drivers {
                        remap_driver(driver, ids);
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
                        remap_driver(driver, ids);
                    }
                }
            }
        }
        for contribution in &mut self.contributions {
            contribution.target = ids
                .get(&crate::EntityPersistentId(contribution.target.to_bits()))
                .copied()
                .ok_or("Missing persistent animation contribution target")?;
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
        if !matches!(driver.property, AnimationTrackTarget::EntityLink)
            && !entity
                .components
                .iter()
                .any(|component| component.type_id() == driver.property.component_target())
        {
            return Err("Animation driver references an excluded or missing component".into());
        }
        if driver
            .entity_bindings
            .iter()
            .any(|entity| !entities.contains_key(&entity.to_bits()))
        {
            return Err("Animation structural key references an excluded or missing entity".into());
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
        if driver
            .entity_bindings
            .iter()
            .any(|entity| !ids.contains_key(&crate::EntityPersistentId(entity.to_bits())))
        {
            return Err("Missing persistent structural binding".into());
        }
    }
    Ok(())
}

fn validate_binding_ids(
    driver: &AnimationDriverDescription,
    ids: &std::collections::BTreeMap<EntityId, crate::EntityPersistentId>,
) -> Result<(), String> {
    if driver
        .entity_bindings
        .iter()
        .any(|entity| !ids.contains_key(entity))
    {
        return Err("Animation structural key references excluded or missing entity".into());
    }
    Ok(())
}

fn persist_driver(
    driver: &mut AnimationDriverDescription,
    ids: &std::collections::BTreeMap<EntityId, crate::EntityPersistentId>,
) {
    driver.target = EntityId::from_bits(ids[&driver.target].0);
    for entity in &mut driver.entity_bindings {
        *entity = EntityId::from_bits(ids[entity].0);
    }
}

fn remap_driver(
    driver: &mut AnimationDriverDescription,
    ids: &std::collections::BTreeMap<crate::EntityPersistentId, EntityId>,
) {
    driver.target = ids[&crate::EntityPersistentId(driver.target.to_bits())];
    for entity in &mut driver.entity_bindings {
        *entity = ids[&crate::EntityPersistentId(entity.to_bits())];
    }
}
