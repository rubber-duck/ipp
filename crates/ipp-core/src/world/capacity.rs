//! Serializable reservation hints applied through the owning storage allocator.

use std::collections::BTreeMap;

use crate::ErrorReason;

/// Named reservations declared and implemented by one selected World system.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorldSystemCapacityHints(pub BTreeMap<String, usize>);

impl WorldSystemCapacityHints {
    /// Construct a system's default reservations without runtime registration elsewhere.
    pub fn new(values: impl IntoIterator<Item = (&'static str, usize)>) -> Self {
        Self(
            values
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    /// Read a declared reservation. Unknown keys are rejected before application.
    pub fn get(&self, key: &str) -> usize {
        self.0.get(key).copied().unwrap_or(0)
    }
}

/// World-owned reservation configuration. Omitted system entries use factory defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldCapacityHints {
    /// Initial entity identities and stable component slots, never a live-count ceiling.
    pub entities: usize,
    /// System ID to the owning module's named reservations.
    pub systems: BTreeMap<String, WorldSystemCapacityHints>,
}

impl Default for WorldCapacityHints {
    fn default() -> Self {
        Self {
            entities: 256,
            systems: BTreeMap::new(),
        }
    }
}

impl WorldCapacityHints {
    pub(crate) fn resolve(&self, defaults: &Self) -> Result<Self, ErrorReason> {
        if self.entities > u32::MAX as usize {
            return Err(ErrorReason::Capacity);
        }
        let mut resolved = defaults.clone();
        resolved.entities = self.entities;
        for (system, hints) in &self.systems {
            let target = resolved
                .systems
                .get_mut(system)
                .ok_or(ErrorReason::InvalidValue)?;
            for (key, &value) in &hints.0 {
                let slot = target.0.get_mut(key).ok_or(ErrorReason::InvalidValue)?;
                if value > u32::MAX as usize {
                    return Err(ErrorReason::Capacity);
                }
                *slot = value;
            }
        }
        Ok(resolved)
    }
}

impl crate::WorldContext<'_> {
    /// Current configured reservations, including selected systems' resolved defaults.
    pub fn capacity_hints(&self) -> &WorldCapacityHints {
        &self.world.capacity_hints
    }

    /// Apply reservation configuration at a Host mutation boundary. Failed reservations
    /// preserve configuration and live values; already allocated spare capacity may remain.
    pub fn set_capacity_hints(&mut self, hints: WorldCapacityHints) -> Result<(), ErrorReason> {
        if self.world.updating || self.instances.current.is_some() {
            return Err(ErrorReason::InvalidValue);
        }
        let resolved = hints.resolve(&self.world.capacity_hints)?;
        self.world.state.allocator.reserve(resolved.entities)?;
        self.world.components.try_reserve(resolved.entities)?;
        for instance in self
            .instances
            .before
            .iter_mut()
            .chain(self.instances.after.iter_mut())
        {
            instance
                .system
                .reserve_capacity(&resolved.systems[instance.id.0])?;
        }
        self.world.capacity_hints = resolved;
        Ok(())
    }
}

/// Partial construction/load/runtime overrides. Missing values preserve prior hints.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorldCapacityHintsPatch {
    /// Optional entity reservation override.
    pub entities: Option<usize>,
    /// Sparse overrides for selected systems' named reservations.
    pub systems: BTreeMap<String, WorldSystemCapacityHints>,
}

impl WorldCapacityHintsPatch {
    /// Compose request overrides with saved values or construction defaults.
    pub fn apply(&self, previous: &WorldCapacityHints) -> WorldCapacityHints {
        let mut result = previous.clone();
        if let Some(entities) = self.entities {
            result.entities = entities;
        }
        for (system, values) in &self.systems {
            result
                .systems
                .entry(system.clone())
                .or_default()
                .0
                .extend(values.0.clone());
        }
        result
    }
}

impl From<WorldCapacityHints> for WorldCapacityHintsPatch {
    fn from(hints: WorldCapacityHints) -> Self {
        Self {
            entities: Some(hints.entities),
            systems: hints.systems,
        }
    }
}
