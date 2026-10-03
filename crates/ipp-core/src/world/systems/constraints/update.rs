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
            self.state.numeric_dirty = true;
        }
        result
    }

    pub(super) fn prepare_drivers(
        &mut self,
        world: &WorldSimulationState,
        assets: &crate::services::asset_management::AssetManagementService,
    ) {
        use super::dependency_graph::{DriverDependencies, DriverKey, PropertyIdentity};
        use super::system_state::{PreparedConstraint, ScalarNumericBinding};
        let components = &world.components;
        self.state.numeric.clear();
        self.state.order.clear();
        let mut dependencies = self.prepare_expressions(world, assets);
        let mut indices = BTreeMap::new();
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
            let key = DriverKey(entity, ComponentValue::LINEAR_DRIVER);
            let scalar = DriverProperty {
                component: ComponentValue::SCALAR,
                offset: std::mem::offset_of!(crate::components::Scalar, value) as u32,
            };
            let target_property = PropertyIdentity {
                entity,
                incarnation: binding.target_incarnation,
                kind: crate::DynamicPropertyKind::F32,
                property: scalar,
            };
            let mut sources = vec![PropertyIdentity {
                entity: binding.source,
                incarnation: binding.source_incarnation,
                kind: crate::DynamicPropertyKind::F32,
                property: scalar,
            }];
            if let Some(input) = world
                .state
                .entities
                .get(&entity)
                .and_then(|record| record.input(ComponentValue::LINEAR_DRIVER))
            {
                for offset in [
                    std::mem::offset_of!(LinearDriver, scale),
                    std::mem::offset_of!(LinearDriver, bias),
                ] {
                    sources.push(PropertyIdentity {
                        entity,
                        incarnation: input.incarnation,
                        kind: crate::DynamicPropertyKind::F32,
                        property: DriverProperty {
                            component: ComponentValue::LINEAR_DRIVER,
                            offset: offset as u32,
                        },
                    });
                }
            }
            dependencies.push(DriverDependencies {
                key,
                target: target_property,
                sources,
            });
            indices.insert(key, self.state.numeric.len());
            // SAFETY: Reconciliation pins exact source/target incarnations.
            // before_commit revokes every copy before occupied cell replacement;
            // accesses borrow the same World's storage in its sequential phase.
            self.state.numeric.push(unsafe {
                ScalarNumericBinding {
                    source: crate::world::component_binding::ComponentBinding::new(source),
                    target: crate::world::component_binding::ComponentBinding::new(target),
                    driver: crate::world::component_binding::ComponentBinding::new(driver),
                }
            });
        }
        let (order, invalid) = super::dependency_graph::order(&dependencies);
        for key in invalid.difference(&self.state.invalid) {
            crate::diagnostic!(
                Warn,
                "[IPP core] constraint.invalid target={} component={} reason=dependency-cycle",
                key.0.to_bits(),
                key.1
            );
        }
        for key in self.state.invalid.difference(&invalid) {
            crate::diagnostic!(
                Debug,
                "[IPP core] constraint.valid target={} component={}",
                key.0.to_bits(),
                key.1
            );
        }
        for key in &invalid {
            if key.1 == ComponentValue::EXPRESSION_DRIVER {
                let binding = self.state.expressions.get_mut(&key.0).unwrap();
                binding.status.state =
                    ExpressionDriverState::Retained(ExpressionDriverReason::Cycle);
                binding.status.recovered = false;
            }
        }
        self.state.invalid = invalid;
        for key in order {
            self.state
                .order
                .push(if key.1 == ComponentValue::LINEAR_DRIVER {
                    PreparedConstraint::Linear {
                        entity: key.0,
                        index: indices[&key],
                    }
                } else {
                    PreparedConstraint::Expression(key.0)
                });
        }
        self.state.numeric_dirty = false;
    }

    pub(super) fn evaluate(&mut self, context: &mut crate::systems::SystemRuntimeAccess<'_>) {
        use super::system_state::PreparedConstraint;
        use crate::expressions::{ExpressionInvalid, ExpressionResult};
        for prepared in &self.state.order {
            match *prepared {
                PreparedConstraint::Linear {
                    entity,
                    index,
                } => {
                    let binding = &self.state.numeric[index];
                    let driver = *binding.driver.get(&context.world.components);
                    let result = binding.source.get(&context.world.components).value * driver.scale
                        + driver.bias;
                    let offset = std::mem::offset_of!(crate::components::Scalar, value) as u32;
                    context.before_absolute_writes(&[(entity, ComponentValue::SCALAR, offset)]);
                    context.before_numeric_update(&[(entity, ComponentValue::SCALAR)]);
                    binding.target.get_mut(&mut context.world.components).value = result;
                }
                PreparedConstraint::Expression(entity) => {
                    let binding = self.state.expressions.get_mut(&entity).unwrap();
                    let Some(runtime) = &mut binding.runtime else {
                        continue;
                    };
                    for (value, source) in runtime.values.iter_mut().zip(&runtime.sources) {
                        *value = source.and_then(|source| source.read(&context.world.components));
                    }
                    let mut inputs = [None; EXPRESSION_DRIVER_MAX_INPUTS];
                    for (input, value) in inputs.iter_mut().zip(&runtime.values) {
                        *input = value.as_ref();
                    }
                    let result = runtime
                        .plan
                        .evaluate(&mut runtime.scratch, &inputs[..runtime.values.len()])
                        .expect("prepared input kinds and scratch identity")
                        .clone();
                    let value = match result {
                        ExpressionResult::Valid(value) => value,
                        ExpressionResult::Invalid(reason) => {
                            binding.status.state = ExpressionDriverState::Retained(match reason {
                                ExpressionInvalid::MissingInput {
                                    slot,
                                } => ExpressionDriverReason::MissingInput {
                                    slot: slot as u32,
                                },
                                ExpressionInvalid::InvalidInput {
                                    slot,
                                } => ExpressionDriverReason::InvalidInput {
                                    slot: slot as u32,
                                },
                                ExpressionInvalid::Calculation => {
                                    ExpressionDriverReason::Calculation
                                }
                            });
                            binding.status.recovered = false;
                            continue;
                        }
                    };
                    if let Err(reason) = runtime
                        .destination
                        .validate(&context.world.components, &value)
                    {
                        binding.status.state = ExpressionDriverState::Retained(
                            ExpressionDriverReason::TargetRejected(reason),
                        );
                        binding.status.recovered = false;
                        continue;
                    }
                    let target = binding.target.expect("prepared target identity");
                    // Validation precedes notification: a skipped/rejected result
                    // must preserve the animation contribution already in storage.
                    context.before_absolute_writes(&[(
                        entity,
                        target.property.component,
                        target.property.offset,
                    )]);
                    context.before_numeric_update(&[(entity, target.property.component)]);
                    runtime
                        .destination
                        .write_validated(&mut context.world.components, value);
                    binding.status.recovered =
                        matches!(binding.status.state, ExpressionDriverState::Retained(_));
                    binding.status.state = ExpressionDriverState::Written;
                }
            }
        }
    }
}
