use super::*;

fn declaration(
    components: &ComponentStorage,
    state: &WorldEntityState,
    target: EntityId,
) -> Option<ConstraintDriverIdentity> {
    let incarnation = state
        .entities
        .get(&target)?
        .input(ComponentValue::LINEAR_DRIVER)?
        .incarnation;
    let ComponentValue::LinearDriver(value) =
        state.input_value(components, target, ComponentValue::LINEAR_DRIVER)?
    else {
        return None;
    };
    Some(ConstraintDriverIdentity {
        incarnation,
        source: value.source,
    })
}

fn binding(
    state: &WorldEntityState,
    manifest: &crate::systems::WorldManifest,
    target: EntityId,
    source: EntityId,
) -> Result<ScalarConstraintBinding, ErrorReason> {
    if !manifest.supports_operation(crate::systems::WorldOperation::Constraints)
        || !manifest.supports_component(ComponentValue::LINEAR_DRIVER)
        || !manifest.supports_component(ComponentValue::SCALAR)
    {
        return Err(ErrorReason::UnsupportedDependency);
    }
    let target_incarnation = state
        .entities
        .get(&target)
        .and_then(|record| record.input(ComponentValue::SCALAR))
        .ok_or(ErrorReason::MissingComponent)?
        .incarnation;
    let source_incarnation = state
        .entities
        .get(&source)
        .ok_or(ErrorReason::InvalidEntity)?
        .input(ComponentValue::SCALAR)
        .ok_or(ErrorReason::MissingComponent)?
        .incarnation;
    Ok(ScalarConstraintBinding {
        source,
        source_incarnation,
        target_incarnation,
    })
}

fn valid(state: &WorldEntityState, target: EntityId, binding: ScalarConstraintBinding) -> bool {
    state
        .entities
        .get(&binding.source)
        .and_then(|record| record.input(ComponentValue::SCALAR))
        .is_some_and(|value| value.incarnation == binding.source_incarnation)
        && state
            .entities
            .get(&target)
            .and_then(|record| record.input(ComponentValue::SCALAR))
            .is_some_and(|value| value.incarnation == binding.target_incarnation)
        && state
            .entities
            .get(&target)
            .is_some_and(|record| record.input(ComponentValue::LINEAR_DRIVER).is_some())
}

impl ConstraintSystem {
    pub(super) fn reconcile(
        &mut self,
        world: &WorldSimulationState,
        staged: &WorldMutationState,
    ) -> Result<(), ErrorReason> {
        let mut touched = Vec::new();
        self.state.bindings.retain(|&target, &mut binding| {
            let keep = valid(staged, target, binding);
            if !keep {
                touched.push(target);
            }
            keep
        });
        let source_offset = std::mem::offset_of!(LinearDriver, source) as u32;
        let targets: BTreeSet<_> = staged
            .dirty
            .iter()
            .chain(staged.operation_components.iter())
            .filter_map(|&(entity, component)| {
                (component == ComponentValue::LINEAR_DRIVER).then_some(entity)
            })
            .chain(
                staged
                    .explicit_fields
                    .iter()
                    .filter_map(|&(entity, component, offset)| {
                        (component == ComponentValue::LINEAR_DRIVER && offset == source_offset)
                            .then_some(entity)
                    }),
            )
            .collect();
        let mut result = Ok(());
        for target in targets {
            let current = declaration(&world.components, staged, target);
            let previous = self.state.declarations.get(&target).copied();
            if current == previous
                && !staged.explicit_fields.contains(&(
                    target,
                    ComponentValue::LINEAR_DRIVER,
                    source_offset,
                ))
            {
                continue;
            }
            self.state.bindings.remove(&target);
            touched.push(target);
            if let Some(current) = current {
                self.state.declarations.insert(target, current);
                match binding(staged, &world.manifest, target, current.source) {
                    Ok(binding) => {
                        self.state.bindings.insert(target, binding);
                    }
                    Err(reason) => {
                        result = result.and(Err(reason));
                    }
                }
            } else {
                self.state.declarations.remove(&target);
            }
        }
        self.state
            .declarations
            .retain(|target, _| staged.entities.contains_key(target));
        if !touched.is_empty() {
            // A new cycle passes through a changed binding; a broken one was
            // already invalid. Reclassify both, proportionally to their chains.
            let previous: Vec<_> = self.state.invalid.iter().copied().collect();
            classify_cycles(
                &self.state.bindings,
                &mut self.state.invalid,
                touched.into_iter().chain(previous),
            );
        }
        result
    }

    pub(super) fn prepare_numeric(&mut self, components: &ComponentStorage) {
        self.state.numeric.clear();
        self.state.targets.clear();
        for entity in evaluation_order(&self.state.bindings, &self.state.invalid) {
            let binding = self.state.bindings[&entity];
            let Some(source) = components.scalar_ptr(binding.source.index() as usize) else {
                continue;
            };
            let Some(target) = components.scalar_ptr(entity.index() as usize) else {
                continue;
            };
            let Some(driver) = components.linear_driver_ptr(entity.index() as usize) else {
                continue;
            };
            self.state.targets.push((
                entity,
                ComponentValue::SCALAR,
                std::mem::offset_of!(crate::components::Scalar, value) as u32,
            ));
            // SAFETY: Reconciliation established exact source/target incarnations.
            // before_commit clears these cell-origin bindings before any referenced
            // component changes. Evaluation borrows the same World's storage.
            self.state.numeric.push(unsafe {
                super::system_state::ScalarNumericBinding {
                    source: crate::world::component_binding::ComponentBinding::new(source),
                    target: crate::world::component_binding::ComponentBinding::new(target),
                    driver: crate::world::component_binding::ComponentBinding::new(driver),
                }
            });
        }
        self.state.numeric_dirty = false;
    }

    pub(super) fn evaluate(&mut self, world: &mut WorldSimulationState) {
        for binding in &self.state.numeric {
            let driver = *binding.driver.get(&world.components);
            let source = binding.source.get(&world.components).value;
            binding.target.get_mut(&mut world.components).value =
                source * driver.scale + driver.bias;
        }
    }
}

/// Classify the drivers reachable from `affected` along their source chains.
/// Every driver has one source, so a walk either ends, reaches a chain that is
/// already classified, or closes a cycle; only the cycle's members are invalid.
/// Drivers that read an invalid driver's target still evaluate from its current value.
fn classify_cycles(
    bindings: &BTreeMap<EntityId, ScalarConstraintBinding>,
    invalid: &mut BTreeSet<EntityId>,
    affected: impl IntoIterator<Item = EntityId>,
) {
    let mut done = BTreeSet::new();
    let mut positions = BTreeMap::new();
    for start in affected {
        positions.clear();
        let mut path = Vec::new();
        let mut current = start;
        let cycle = loop {
            if done.contains(&current) {
                break None;
            }
            if let Some(&position) = positions.get(&current) {
                break Some(position);
            }
            let Some(binding) = bindings.get(&current) else {
                break None;
            };
            positions.insert(current, path.len());
            path.push(current);
            current = binding.source;
        };
        if !bindings.contains_key(&start) {
            invalid.remove(&start);
        }
        for (position, entity) in path.into_iter().enumerate() {
            done.insert(entity);
            if cycle.is_some_and(|first| position >= first) {
                if invalid.insert(entity) {
                    crate::diagnostic!(
                        Warn,
                        "[IPP core] constraint.invalid target={} reason=dependency-cycle",
                        entity.to_bits()
                    );
                }
            } else if invalid.remove(&entity) {
                crate::diagnostic!(
                    Debug,
                    "[IPP core] constraint.valid target={}",
                    entity.to_bits()
                );
            }
        }
    }
}

/// Valid drivers with every source evaluated before its target. Invalid cycle
/// members are omitted, so the remaining chains are acyclic.
fn evaluation_order(
    bindings: &BTreeMap<EntityId, ScalarConstraintBinding>,
    invalid: &BTreeSet<EntityId>,
) -> Vec<EntityId> {
    let mut order = Vec::with_capacity(bindings.len());
    let mut done = BTreeSet::new();
    for &target in bindings.keys() {
        let mut path = Vec::new();
        let mut current = target;
        while !invalid.contains(&current) && done.insert(current) {
            let Some(binding) = bindings.get(&current) else {
                break;
            };
            path.push(current);
            current = binding.source;
        }
        order.extend(path.into_iter().rev());
    }
    order
}
