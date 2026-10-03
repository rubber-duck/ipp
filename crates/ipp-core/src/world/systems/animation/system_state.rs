//! Separate controller objects exclusively owned by AnimationSystem.

use super::{driver::AnimationDriverBinding, *};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
thread_local! {
    pub(super) static STRUCTURAL_CANDIDATE_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Multi-entity playback clock and validated typed driver bindings.
#[derive(Debug)]
pub struct AnimationController {
    pub(in crate::world) snapshot: AnimationControllerSnapshot,
    pub(in crate::world) drivers: Vec<Box<dyn AnimationDriverBinding>>,
    pub(super) structural_drivers: Vec<super::structural::AnimationStructuralDriver>,
    pub(super) driver_targets: BTreeMap<(EntityId, u16), Vec<usize>>,
    pub(in crate::world) incarnations: Vec<(u64, AnimationTrackTarget)>,
    pub(in crate::world) sought: bool,
    pub(super) directional_start_pending: bool,
    pub(super) duration: f64,
    pub(super) ready: bool,
    pub(super) numeric_targets: Vec<(EntityId, u16)>,
    pub(super) discrete_drivers: Vec<usize>,
    pub(super) numeric_outputs: Vec<(
        (EntityId, u16),
        super::numeric_output::AnimationNumericOutput,
    )>,
    pub(in crate::world) failure: Option<ErrorReason>,
    pub(super) transition: Option<Box<AnimationTransitionRuntime>>,
    /// What this controller has added to its fields.
    pub(in crate::world) contributions: super::contribution::AnimationContributions,
}

#[derive(Debug)]
pub(super) struct AnimationTransitionRuntime {
    pub(super) source: AnimationTransitionSource,
    pub(super) start_time: AnimationTransitionStartTime,
    pub(super) program: Option<super::transition::AnimationTransitionProgram>,
}

#[derive(Debug)]
pub(super) enum AnimationTransitionSource {
    Live(Box<AnimationController>),
    Frozen {
        values: Vec<AnimationRuntimeFrozenTransitionValue>,
        bindings: Box<AnimationController>,
        prepared: Option<super::transition::AnimationTransitionProgram>,
        reference_time: f64,
        reference_duration: f64,
    },
}

/// A captured value of one target. A controller crossfade captures its
/// contribution and fades it out; GUI motion captures the value it fades from,
/// and `baseline` is where a new channel starts.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct AnimationRuntimeFrozenTransitionValue {
    pub(super) target: EntityId,
    pub(super) incarnation: u64,
    pub(super) property: AnimationTrackTarget,
    pub(super) value: AnimationValue,
    pub(super) baseline: AnimationValue,
}

impl AnimationController {
    pub(super) fn transition_source(&self) -> Option<&AnimationController> {
        self.transition
            .as_deref()
            .map(|transition| match &transition.source {
                AnimationTransitionSource::Live(source) => source.as_ref(),
                AnimationTransitionSource::Frozen {
                    bindings,
                    ..
                } => bindings.as_ref(),
            })
    }

    pub(super) fn transition_source_mut(&mut self) -> Option<&mut AnimationController> {
        self.transition
            .as_deref_mut()
            .map(|transition| match &mut transition.source {
                AnimationTransitionSource::Live(source) => source.as_mut(),
                AnimationTransitionSource::Frozen {
                    bindings,
                    ..
                } => bindings.as_mut(),
            })
    }

    pub(in crate::world) fn clear_drivers(&mut self) {
        self.drivers.clear();
        self.structural_drivers.clear();
        self.numeric_targets.clear();
        self.discrete_drivers.clear();
        self.numeric_outputs.clear();
        self.contributions.invalidate();
        self.ready = false;
    }

    pub(super) fn reindex_drivers(&mut self) {
        self.ready = false;
        self.numeric_targets.clear();
        self.discrete_drivers.clear();
        self.numeric_outputs.clear();
        self.contributions.invalidate();
        self.duration = self
            .drivers
            .iter()
            .map(|driver| driver.duration())
            .chain(
                self.structural_drivers
                    .iter()
                    .map(|driver| driver.duration()),
            )
            .fold(0.0, f64::max);
        self.driver_targets.clear();
        // Rebuild the direct lookup whenever bindings change.
        for (index, driver) in self.drivers.iter().enumerate() {
            let identity = driver.identity();
            self.driver_targets
                .entry((identity.entity, identity.property.component_target()))
                .or_default()
                .push(index);
        }
    }

    /// Bind whole-component numeric outputs and each driver's contribution entry.
    pub(super) fn bind_numeric_targets(
        &mut self,
        storage: &crate::components::registry::ComponentStorage,
    ) {
        self.numeric_targets.clear();
        self.discrete_drivers.clear();
        self.numeric_outputs.clear();
        for &key in self.driver_targets.keys() {
            let offsets: Vec<_> = self
                .drivers_for(key)
                .flat_map(|driver| {
                    driver
                        .identity()
                        .property
                        .property()
                        .into_iter()
                        .flat_map(|property| property.offsets.iter().copied())
                })
                .collect();
            if let Some(output) =
                super::numeric_output::AnimationNumericOutput::bind(storage, key, &offsets)
            {
                self.numeric_outputs.push((key, output));
                self.numeric_targets.push(key);
            }
        }
        for (index, driver) in self.drivers.iter().enumerate() {
            if driver.discrete() {
                self.discrete_drivers.push(index);
            }
        }
        self.contributions.prepare(&self.drivers);
    }

    pub(super) fn numeric_output(
        &self,
        key: (EntityId, u16),
    ) -> Option<super::numeric_output::AnimationNumericOutput> {
        self.numeric_outputs
            .binary_search_by_key(&key, |entry| entry.0)
            .ok()
            .map(|index| self.numeric_outputs[index].1.clone())
    }

    pub(super) fn drivers_for(
        &self,
        key: (EntityId, u16),
    ) -> impl Iterator<Item = &dyn AnimationDriverBinding> {
        let selected = self
            .driver_targets
            .get(&key)
            .into_iter()
            .flatten()
            .filter_map(|&index| self.drivers.get(index));
        let transition = self
            .transition
            .as_deref()
            .into_iter()
            .flat_map(move |transition| {
                let source = match &transition.source {
                    AnimationTransitionSource::Live(source) => source.as_ref(),
                    AnimationTransitionSource::Frozen {
                        bindings,
                        ..
                    } => bindings.as_ref(),
                };
                source.drivers.iter().filter_map(move |driver| {
                    let identity = driver.identity();
                    ((identity.entity, identity.property.component_target()) == key)
                        .then_some(driver.as_ref())
                })
            });
        selected.map(|driver| driver.as_ref()).chain(transition)
    }

    pub(super) fn changed_drivers<'a>(
        &'a self,
        staged: &'a crate::world::WorldMutationState,
    ) -> impl Iterator<Item = &'a dyn AnimationDriverBinding> {
        let selected = staged
            .changed
            .keys()
            .filter_map(|key| self.driver_targets.get(key))
            .flatten()
            .filter_map(|&index| self.drivers.get(index));
        let transition = self
            .transition
            .as_deref()
            .into_iter()
            .flat_map(move |transition| {
                let source = match &transition.source {
                    AnimationTransitionSource::Live(source) => source.as_ref(),
                    AnimationTransitionSource::Frozen {
                        bindings,
                        ..
                    } => bindings.as_ref(),
                };
                source.drivers.iter().filter_map(move |driver| {
                    let identity = driver.identity();
                    staged
                        .changed
                        .contains_key(&(identity.entity, identity.property.component_target()))
                        .then_some(driver.as_ref())
                })
            });
        selected.map(|driver| driver.as_ref()).chain(transition)
    }

    /// Read persistent descriptions and clock state without runtime bindings.
    pub fn snapshot(&self) -> &AnimationControllerSnapshot {
        &self.snapshot
    }

    /// Inspect the actual concrete type of a bound driver.
    pub fn driver<T: AnimationSample>(&self, index: usize) -> Option<&AnimationDriver<T>> {
        self.drivers.get(index)?.as_any().downcast_ref()
    }
}

/// Controller identities, sparse bindings, events and retained source demand.
pub struct AnimationSystemState {
    pub(in crate::world) controllers: BTreeMap<AnimationControllerId, AnimationController>,
    // Descriptions index stopped and active controllers. Dynamic-property pruning
    // may leave a conservative superset until the next explicit description update.
    pub(super) target_controllers: BTreeMap<(EntityId, u16), BTreeSet<AnimationControllerId>>,
    target_keys: BTreeMap<AnimationControllerId, Vec<(EntityId, u16)>>,
    pub(super) structural_target_controllers: BTreeMap<EntityId, BTreeSet<AnimationControllerId>>,
    structural_target_keys: BTreeMap<AnimationControllerId, Vec<EntityId>>,
    pub(super) description_demand_clean: bool,
    pub(in crate::world) demand_revision: u64,
    pub(super) affected_controllers: Vec<AnimationControllerId>,
    pub(super) component_scratch: super::component_values::ComponentScratch,
    pub(super) property_scratch: super::component_values::PropertyScratch,
    pub(super) controller_ids: Vec<AnimationControllerId>,
    pub(super) ready_controllers: Vec<AnimationControllerId>,
    pub(in crate::world) next_id: u64,
    pub(in crate::world) playback_events: Vec<AnimationPlaybackEvent>,
    pub(in crate::world) controller_outcomes: Vec<AnimationControllerOutcome>,
    pub(in crate::world) animation_sources:
        BTreeSet<crate::services::asset_management::service::AssetDemandSelection>,
}

impl Default for AnimationSystemState {
    fn default() -> Self {
        Self {
            controllers: BTreeMap::new(),
            target_controllers: BTreeMap::new(),
            target_keys: BTreeMap::new(),
            structural_target_controllers: BTreeMap::new(),
            structural_target_keys: BTreeMap::new(),
            description_demand_clean: false,
            demand_revision: 0,
            affected_controllers: Vec::new(),
            component_scratch: Vec::new(),
            property_scratch: Vec::new(),
            controller_ids: Vec::new(),
            ready_controllers: Vec::new(),
            next_id: 1,
            playback_events: Vec::new(),
            controller_outcomes: Vec::new(),
            animation_sources: BTreeSet::new(),
        }
    }
}

impl AnimationSystemState {
    pub(super) fn unindex_controller(&mut self, id: AnimationControllerId) {
        self.description_demand_clean = false;
        if let Some(keys) = self.target_keys.remove(&id) {
            for key in keys {
                if let Some(ids) = self.target_controllers.get_mut(&key) {
                    ids.remove(&id);
                    for id in ids.iter() {
                        if let Some(controller) = self.controllers.get_mut(id) {
                            controller.ready = false;
                        }
                    }
                    if ids.is_empty() {
                        self.target_controllers.remove(&key);
                    }
                }
            }
        }
        if let Some(keys) = self.structural_target_keys.remove(&id) {
            for key in keys {
                if let Some(ids) = self.structural_target_controllers.get_mut(&key) {
                    ids.remove(&id);
                    if ids.is_empty() {
                        self.structural_target_controllers.remove(&key);
                    }
                }
            }
        }
    }

    pub(super) fn index_controller(&mut self, id: AnimationControllerId) {
        self.unindex_controller(id);
        let mut keys: Vec<_> = self.controllers[&id]
            .snapshot
            .description
            .drivers
            .iter()
            .filter(|driver| !matches!(driver.property, AnimationTrackTarget::EntityLink))
            .map(|driver| (driver.target, driver.property.component_target()))
            .collect();
        if let Some(transition) = &self.controllers[&id].transition {
            match &transition.source {
                AnimationTransitionSource::Live(source) => keys.extend(
                    source
                        .snapshot
                        .description
                        .drivers
                        .iter()
                        .map(|driver| (driver.target, driver.property.component_target())),
                ),
                AnimationTransitionSource::Frozen {
                    values,
                    bindings,
                    ..
                } => {
                    keys.extend(
                        values
                            .iter()
                            .map(|value| (value.target, value.property.component_target())),
                    );
                    keys.extend(
                        bindings
                            .snapshot
                            .description
                            .drivers
                            .iter()
                            .map(|driver| (driver.target, driver.property.component_target())),
                    );
                }
            }
            if let Some(program) = transition.program.as_ref() {
                keys.extend(program.target_keys());
            }
        }
        keys.sort_unstable();
        keys.dedup();
        for &key in &keys {
            self.target_controllers.entry(key).or_default().insert(id);
            for other in &self.target_controllers[&key] {
                if *other == id {
                    continue;
                }
                if let Some(controller) = self.controllers.get_mut(other) {
                    controller.ready = false;
                }
            }
        }
        self.target_keys.insert(id, keys);
        let mut structural_targets: Vec<_> = self.controllers[&id]
            .snapshot
            .description
            .drivers
            .iter()
            .filter(|driver| matches!(driver.property, AnimationTrackTarget::EntityLink))
            .map(|driver| driver.target)
            .collect();
        structural_targets.sort_unstable();
        structural_targets.dedup();
        for &target in &structural_targets {
            self.structural_target_controllers
                .entry(target)
                .or_default()
                .insert(id);
        }
        self.structural_target_keys.insert(id, structural_targets);
    }

    pub(super) fn rebuild_target_index(&mut self) {
        self.description_demand_clean = false;
        self.target_controllers.clear();
        self.target_keys.clear();
        self.structural_target_controllers.clear();
        self.structural_target_keys.clear();
        let ids: Vec<_> = self.controllers.keys().copied().collect();
        for id in ids {
            self.index_controller(id);
        }
    }

    /// Detach reusable mutation scratch while callbacks borrow controller state.
    pub(super) fn affected_by(
        &mut self,
        staged: &crate::world::WorldMutationState,
    ) -> Vec<AnimationControllerId> {
        let mut ids = std::mem::take(&mut self.affected_controllers);
        ids.clear();
        for key in staged.changed.keys() {
            if let Some(targets) = self.target_controllers.get(key) {
                ids.extend(targets);
            }
        }
        for target in &staged.operation_deleted {
            #[cfg(test)]
            STRUCTURAL_CANDIDATE_VISITS.set(STRUCTURAL_CANDIDATE_VISITS.get() + 1);
            if let Some(controllers) = self.structural_target_controllers.get(target) {
                #[cfg(test)]
                STRUCTURAL_CANDIDATE_VISITS
                    .set(STRUCTURAL_CANDIDATE_VISITS.get() + controllers.len());
                ids.extend(controllers);
            }
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Borrow-checker detachment without deleting the B-tree node. The caller
    /// reinstalls the actual controller before any lifecycle/commit callbacks.
    pub(super) fn take_controller(
        &mut self,
        id: AnimationControllerId,
    ) -> Option<AnimationController> {
        let controller = self.controllers.get_mut(&id)?;
        Some(std::mem::replace(
            controller,
            AnimationController {
                snapshot: AnimationControllerSnapshot {
                    id,
                    description: AnimationControllerDescription::default(),
                    state: AnimationPlaybackStatus::Stopped,
                    time: 0.0,
                    transition: None,
                },
                drivers: Vec::new(),
                structural_drivers: Vec::new(),
                driver_targets: BTreeMap::new(),
                incarnations: Vec::new(),
                sought: false,
                directional_start_pending: false,
                duration: 0.0,
                ready: false,
                numeric_targets: Vec::new(),
                discrete_drivers: Vec::new(),
                numeric_outputs: Vec::new(),
                failure: None,
                transition: None,
                contributions: Default::default(),
            },
        ))
    }
}

#[cfg(test)]
#[path = "system_state_tests.rs"]
mod tests;
