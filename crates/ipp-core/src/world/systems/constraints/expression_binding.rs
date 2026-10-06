//! Declaration pinning and asset-bound prepared execution; no persistent scratch.

use super::dependency_graph::{DriverDependencies, DriverKey, PropertyIdentity};
use super::*;
use crate::components::schema::FieldValue;
use crate::expressions::{ExpressionScratch, PreparedExpression};
use crate::services::asset_management::{
    AssetKey, AssetLoadStatus,
    formats::expression::{EXPRESSION_TYPE, ExpressionAsset},
};
use crate::world::direct_bindings::property_binding::PropertyBinding;

pub(super) struct ExpressionConstraintBinding {
    pub(super) incarnation: u64,
    pub(super) target: Option<PropertyIdentity>,
    pub(super) inputs: Vec<(String, Option<PropertyIdentity>)>,
    pub(super) key: Option<AssetKey>,
    pub(super) removed_asset: bool,
    pub(super) runtime: Option<PreparedExpressionDriver>,
    pub(super) status: ExpressionDriverStatus,
}

pub(super) struct PreparedExpressionDriver {
    pub(super) plan: PreparedExpression,
    pub(super) scratch: ExpressionScratch,
    pub(super) sources: Vec<Option<PropertyBinding>>,
    pub(super) values: Vec<Option<crate::DynamicValue>>,
    pub(super) destination: PropertyBinding,
    #[cfg(test)]
    pub(super) preparation: std::sync::Arc<()>,
}

fn field(
    world: &WorldSimulationState,
    staged: &WorldEntityState,
    entity: EntityId,
    property: DriverProperty,
) -> Option<FieldValue> {
    staged.input_field(
        &world.components,
        entity,
        property.component,
        property.offset,
    )
}

fn pin(
    world: &WorldSimulationState,
    staged: &WorldEntityState,
    entity: EntityId,
    property: DriverProperty,
) -> Option<PropertyIdentity> {
    let value = field(world, staged, entity, property)?;
    let kind = match value {
        FieldValue::F32(_) => crate::DynamicPropertyKind::F32,
        FieldValue::U32(_) => crate::DynamicPropertyKind::U32,
        FieldValue::Bool(_) => crate::DynamicPropertyKind::Bool,
        FieldValue::String(_) => crate::DynamicPropertyKind::Text,
        FieldValue::Dynamic(value) if value.kind() != crate::DynamicPropertyKind::Asset => {
            value.kind()
        }
        _ => return None,
    };
    Some(PropertyIdentity {
        entity,
        property,
        kind,
        incarnation: staged
            .entities
            .get(&entity)?
            .input(property.component)?
            .incarnation,
    })
}

fn live(
    world: &WorldSimulationState,
    staged: &WorldEntityState,
    identity: PropertyIdentity,
) -> bool {
    pin(world, staged, identity.entity, identity.property) == Some(identity)
}

fn retains_layout(
    staged: &WorldEntityState,
    identity: PropertyIdentity,
    access: PropertyBinding,
) -> bool {
    let key = (identity.entity, identity.property.component);
    let prepared = (!staged.dirty.contains(&key))
        .then(|| staged.prepared.get(&key))
        .flatten();
    let value = prepared.or_else(|| {
        staged
            .entities
            .get(&identity.entity)?
            .components
            .get(&identity.property.component)?
            .staged
            .as_deref()
    });
    // No staged replacement means the compiled layout is unchanged. Cell and
    // property liveness are checked separately before this comparison.
    value.is_none_or(|value| access.retains_layout(value))
}

pub(super) fn expression_declaration_candidates(
    staged: &WorldMutationState,
    operation: bool,
) -> impl Iterator<Item = &(EntityId, u16)> {
    // Operation hooks must not revisit the cumulative batch. Commit also sees
    // changes installed by private restoration, which may defer these hooks.
    staged.operation_components.iter().chain(
        (!operation)
            .then_some(staged)
            .into_iter()
            .flat_map(|staged| staged.changed.keys()),
    )
}

impl ConstraintSystem {
    pub(super) fn invalidate_expression_access(
        &mut self,
        world: &WorldSimulationState,
        staged: &WorldMutationState,
    ) {
        for (&entity, binding) in &mut self.state.expressions {
            let mut revoked = false;
            if staged
                .entities
                .get(&entity)
                .and_then(|record| record.input(ComponentValue::EXPRESSION_DRIVER))
                .is_none_or(|input| input.incarnation != binding.incarnation)
                || binding
                    .target
                    .is_some_and(|identity| !live(world, staged, identity))
            {
                binding.target = None;
                binding.status.state =
                    ExpressionDriverState::Retained(ExpressionDriverReason::TargetUnavailable);
                revoked = true;
            }
            for (_, identity) in &mut binding.inputs {
                if identity.is_some_and(|identity| !live(world, staged, identity)) {
                    *identity = None;
                    revoked = true;
                }
            }
            if let Some(runtime) = &binding.runtime {
                revoked |= binding
                    .target
                    .is_none_or(|target| !retains_layout(staged, target, runtime.destination));
                for (slot, source) in runtime.sources.iter().enumerate() {
                    let Some(source) = source else {
                        continue;
                    };
                    let name = &runtime.plan.input_slots()[slot].name;
                    let index = binding
                        .inputs
                        .binary_search_by(|(input, _)| input.cmp(name))
                        .expect("prepared input mapping");
                    revoked |= binding.inputs[index]
                        .1
                        .is_none_or(|identity| !retains_layout(staged, identity, *source));
                }
            }
            if revoked {
                // Run after each operation as well as at commit: a departure
                // followed by a same-batch reappearance cannot revive a pin.
                binding.runtime = None;
                binding.status.recovered = false;
                self.state.numeric_dirty = true;
            }
        }
    }

    pub(super) fn reconcile_expressions(
        &mut self,
        world: &WorldSimulationState,
        staged: &WorldMutationState,
        operation: bool,
    ) {
        self.invalidate_expression_access(world, staged);
        let touched: BTreeSet<_> = expression_declaration_candidates(staged, operation)
            .filter_map(|&(entity, component)| {
                if component != ComponentValue::EXPRESSION_DRIVER {
                    return None;
                }
                let incarnation = staged
                    .entities
                    .get(&entity)
                    .and_then(|record| record.input(component))
                    .map(|input| input.incarnation);
                let previous = self
                    .state
                    .expressions
                    .get(&entity)
                    .map(|binding| binding.incarnation);
                let key = (entity, component);
                // Only this operation's accepted writes rebind. Hydration for
                // a rejected write has no effect; a declaration write earlier
                // in the batch cannot revive a property that departs later.
                let written = operation
                    && (staged.operation_untracked.contains(&key)
                        || staged
                            .operation_writes
                            .iter()
                            .any(|(target, _, _)| *target == key));
                (incarnation != previous || written).then_some(entity)
            })
            .collect();
        if !touched.is_empty() {
            self.state.numeric_dirty = true;
        }
        for entity in touched {
            let Some(input) = staged
                .entities
                .get(&entity)
                .and_then(|record| record.input(ComponentValue::EXPRESSION_DRIVER))
            else {
                self.state.expressions.remove(&entity);
                continue;
            };
            // Rebinding is explicit, including equal source/selector writes. A
            // mutation to an unrelated component cannot retarget a departed pin.
            let Some(ComponentValue::ExpressionDriver(driver)) =
                staged.input_value(&world.components, entity, ComponentValue::EXPRESSION_DRIVER)
            else {
                continue;
            };
            let inputs = decode_expression_driver_inputs(&driver.inputs)
                .unwrap_or_default()
                .into_iter()
                .map(|input| {
                    (
                        input.name,
                        pin(world, staged, driver.source, input.property),
                    )
                })
                .collect();
            let target = pin(
                world,
                staged,
                entity,
                DriverProperty {
                    component: driver.target_component as u16,
                    offset: driver.target_offset,
                },
            );
            self.state.expressions.insert(
                entity,
                ExpressionConstraintBinding {
                    incarnation: input.incarnation,
                    target,
                    inputs,
                    key: None,
                    removed_asset: false,
                    runtime: None,
                    status: ExpressionDriverStatus::default(),
                },
            );
        }
        self.state.expressions.retain(|entity, _| {
            staged
                .entities
                .get(entity)
                .is_some_and(|record| record.input(ComponentValue::EXPRESSION_DRIVER).is_some())
        });
    }

    pub(super) fn prepare_expressions(
        &mut self,
        world: &WorldSimulationState,
        assets: &crate::services::asset_management::AssetManagementService,
    ) -> Vec<DriverDependencies> {
        let mut dependencies = Vec::new();
        for (&entity, binding) in &mut self.state.expressions {
            if binding.removed_asset {
                continue;
            }
            let Some(driver) = world.components.expression_driver(entity.index() as usize) else {
                continue;
            };
            binding.key = crate::world::systems::asset_dependencies::source_key_from_fields(
                assets,
                world.id,
                binding.key,
                EXPRESSION_TYPE,
                &driver.expression_source,
                driver.expression_variant,
            );
            let Some(asset) = binding
                .key
                .and_then(|key| assets.get_typed::<ExpressionAsset>(key))
            else {
                binding.runtime = None;
                binding.status.availability = if binding
                    .key
                    .and_then(|key| assets.get(key))
                    .is_some_and(|provider| {
                        matches!(
                            provider.status(),
                            AssetLoadStatus::Unloaded | AssetLoadStatus::Failed(_)
                        )
                    }) {
                    ExpressionDriverAvailability::Unavailable
                } else {
                    ExpressionDriverAvailability::Pending
                };
                binding.status.state = ExpressionDriverState::Retained(
                    if binding
                        .key
                        .and_then(|key| assets.get(key))
                        .is_some_and(|provider| {
                            matches!(provider.status(), AssetLoadStatus::Failed(_))
                        })
                    {
                        ExpressionDriverReason::AssetFailed
                    } else {
                        ExpressionDriverReason::AssetUnavailable
                    },
                );
                continue;
            };
            binding.status.availability = ExpressionDriverAvailability::Ready;
            if binding.runtime.is_some() {
                dependencies.push(DriverDependencies {
                    key: DriverKey(entity, ComponentValue::EXPRESSION_DRIVER),
                    target: binding.target.expect("live prepared destination"),
                    sources: binding
                        .inputs
                        .iter()
                        .filter_map(|(_, identity)| *identity)
                        .collect(),
                });
                continue;
            }
            let plan = asset.prepared().clone();
            if plan.input_slots().len() != binding.inputs.len()
                || plan.input_slots().len() > EXPRESSION_DRIVER_MAX_INPUTS
            {
                binding.status.state =
                    ExpressionDriverState::Retained(ExpressionDriverReason::InputMapping);
                continue;
            }
            let Some(target) = binding.target else {
                binding.status.state =
                    ExpressionDriverState::Retained(ExpressionDriverReason::TargetUnavailable);
                continue;
            };
            // SAFETY: Exact pins were validated at mutation boundaries. Runtime
            // access is cleared synchronously before affected cell/descriptor or
            // asset release, and evaluation borrows only this World's storage.
            let destination = unsafe {
                PropertyBinding::bind(
                    &world.components,
                    entity,
                    target.property.component,
                    target.property.offset,
                )
            };
            let Some(destination) = destination
                .filter(|target| target.writable() && target.kind() == plan.output_kind())
            else {
                binding.status.state =
                    ExpressionDriverState::Retained(ExpressionDriverReason::TargetUnavailable);
                continue;
            };
            let mut sources = Vec::with_capacity(plan.input_slots().len());
            let mut selected = Vec::new();
            let mut refusal = None;
            for (slot, input) in plan.input_slots().iter().enumerate() {
                let Ok(index) = binding
                    .inputs
                    .binary_search_by(|(name, _)| name.as_str().cmp(&input.name))
                else {
                    refusal = Some(ExpressionDriverReason::InputMapping);
                    break;
                };
                let identity = binding.inputs[index].1;
                // SAFETY: Same pin/invalidation contract as the destination above.
                let source = identity.and_then(|identity| unsafe {
                    PropertyBinding::bind(
                        &world.components,
                        identity.entity,
                        identity.property.component,
                        identity.property.offset,
                    )
                });
                if source.is_some_and(|source| source.kind() != input.kind) {
                    refusal = Some(ExpressionDriverReason::InputType {
                        slot: slot as u32,
                    });
                    break;
                }
                if let Some(identity) = identity {
                    selected.push(identity);
                }
                sources.push(source);
            }
            if let Some(reason) = refusal {
                binding.status.state = ExpressionDriverState::Retained(reason);
                continue;
            }
            dependencies.push(DriverDependencies {
                key: DriverKey(entity, ComponentValue::EXPRESSION_DRIVER),
                target,
                sources: selected,
            });
            binding.runtime = Some(PreparedExpressionDriver {
                scratch: plan.scratch(),
                values: vec![None; sources.len()],
                plan,
                sources,
                destination,
                #[cfg(test)]
                preparation: std::sync::Arc::new(()),
            });
        }
        dependencies
    }
}
