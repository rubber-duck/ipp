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
    target: EntityId,
    source: EntityId,
) -> Result<ScalarConstraintBinding, ErrorReason> {
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
        components: &ComponentStorage,
        staged: &WorldMutationState,
    ) -> Result<(), ErrorReason> {
        self.state
            .bindings
            .retain(|&target, &mut binding| valid(staged, target, binding));
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
            let current = declaration(components, staged, target);
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
            if let Some(current) = current {
                self.state.declarations.insert(target, current);
                match binding(staged, target, current.source) {
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
        result
    }

    pub(super) fn prepare_numeric(&mut self, components: &ComponentStorage) {
        self.state.numeric.clear();
        for (&entity, binding) in &self.state.bindings {
            let Some(source) = components.scalar_ptr(binding.source.index() as usize) else {
                continue;
            };
            let Some(target) = components.scalar_ptr(entity.index() as usize) else {
                continue;
            };
            let Some(driver) = components.linear_driver_ptr(entity.index() as usize) else {
                continue;
            };
            // SAFETY: Reconciliation established exact source/target incarnations.
            // before_commit clears these cell-origin bindings before any referenced
            // component changes. Evaluation borrows the same World's storage.
            self.state.numeric.push(unsafe {
                super::system_state::ScalarNumericBinding {
                    entity,
                    incarnation: binding.target_incarnation,
                    source: crate::world::component_binding::ComponentBinding::new(source),
                    target: crate::world::component_binding::ComponentBinding::new(target),
                    driver: crate::world::component_binding::ComponentBinding::new(driver),
                }
            });
        }
        self.state
            .numeric
            .sort_unstable_by_key(|binding| binding.entity.index());
        self.state.numeric_dirty = false;
    }

    pub(super) fn restore_inputs(&mut self, world: &mut WorldSimulationState) {
        if !self.state.restores_active {
            return;
        }
        for binding in &self.state.numeric {
            if let Some(original) = self.state.restores.get(&binding.entity) {
                *binding.target.get_mut(&mut world.components) = original.base;
            }
        }
        // Retain map nodes for the next frame, but expose no inactive original.
        self.state.restores_active = false;
    }

    pub(super) fn evaluate(&mut self, world: &mut WorldSimulationState) {
        for binding in &self.state.numeric {
            let driver = *binding.driver.get(&world.components);
            let source = binding.source.get(&world.components).value;
            let value = binding.target.get_mut(&mut world.components);
            self.state.restores.insert(
                binding.entity,
                crate::world::ComponentStateInstance {
                    base: *value,
                    incarnation: binding.incarnation,
                },
            );
            value.value = source * driver.scale + driver.bias;
        }
        self.state.restores_active = true;
    }

    #[cfg(debug_assertions)]
    fn validation_binding(
        &self,
        components: &ComponentStorage,
        staged: &WorldMutationState,
        target: EntityId,
    ) -> Option<ScalarConstraintBinding> {
        let current = declaration(components, staged, target)?;
        let source_offset = std::mem::offset_of!(LinearDriver, source) as u32;
        let candidate = if self.state.declarations.get(&target) != Some(&current)
            || staged.explicit_fields.contains(&(
                target,
                ComponentValue::LINEAR_DRIVER,
                source_offset,
            )) {
            binding(staged, target, current.source).ok()?
        } else {
            *self.state.bindings.get(&target)?
        };
        valid(staged, target, candidate).then_some(candidate)
    }

    #[cfg(debug_assertions)]
    pub(super) fn validate(
        &self,
        components: &ComponentStorage,
        staged: &WorldMutationState,
    ) -> Result<(), ErrorReason> {
        let mut values: BTreeMap<_, _> = staged
            .entities
            .keys()
            .filter_map(|&entity| {
                let ComponentValue::Scalar(value) =
                    staged.input_value(components, entity, ComponentValue::SCALAR)?
                else {
                    return None;
                };
                Some((entity.index(), value.value))
            })
            .collect();
        // Validation runs before lifecycle reconciliation. Inspect the binding
        // that the prepared declaration will establish, including sampled sources.
        let mut targets: Vec<_> = staged.entities.keys().copied().collect();
        targets.sort_unstable_by_key(|target| target.index());
        for target in targets {
            let Some(binding) = self.validation_binding(components, staged, target) else {
                continue;
            };
            if binding.source == target
                || (self
                    .validation_binding(components, staged, binding.source)
                    .is_some()
                    && binding.source.index() >= target.index())
            {
                return Err(ErrorReason::UnsupportedDependency);
            }
            let ComponentValue::LinearDriver(driver) = staged
                .input_value(components, target, ComponentValue::LINEAR_DRIVER)
                .unwrap()
            else {
                unreachable!()
            };
            let value = values[&binding.source.index()] * driver.scale + driver.bias;
            if !value.is_finite() {
                return Err(ErrorReason::InvalidValue);
            }
            values.insert(target.index(), value);
        }
        Ok(())
    }
}

impl crate::WorldContext<'_> {
    /// Whether the retained driver currently has a valid incarnation binding.
    /// Returns `None` when the entity or driver does not exist.
    pub fn driver_bound(&self, entity: EntityId) -> Option<bool> {
        self.world
            .state
            .entities
            .get(&entity)?
            .input(ComponentValue::LINEAR_DRIVER)?;
        let system = self.system::<ConstraintSystem>(ConstraintSystem::ID)?;
        Some(
            system
                .state
                .bindings
                .get(&entity)
                .is_some_and(|&binding| valid(&self.world.state, entity, binding)),
        )
    }
}
