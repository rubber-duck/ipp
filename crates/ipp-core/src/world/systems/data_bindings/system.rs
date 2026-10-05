use super::{components::request, runtime::DataBindingUnavailable, *};
use crate::world::component_query::ComponentQuery;
use crate::{
    ComponentValue as C, ErrorReason,
    components::registry::{self, ComponentStorage},
    services::data::*,
    systems::*,
};

/// Lifecycle-maintained typed membership; all per-binding state stays in components.
#[derive(Default)]
pub struct DataBindingSystem {
    buffers: ComponentQuery<BufferDataSourceBinding>,
    streams: ComponentQuery<StreamingDataSourceBinding>,
    // Exact outgoing demand survives only from before_commit through after_commit.
    // Incoming incarnations must register before expiry/collection can run.
    retiring_consumers: Vec<DataConsumerHandle>,
}

impl DataBindingSystem {
    /// Stable composition identity.
    pub const ID: SystemId = SystemId("ipp.data-bindings");

    fn invalidate_asset_views(
        &mut self,
        context: &mut SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
        before_release: bool,
    ) {
        use crate::services::asset_management::AssetLifecycleKind;

        // Pending graphics events have a placeholder representation; applied
        // events may instead report StatusChanged/Unloaded with decoded data intact.
        // Immutable CPU plans need no rebuild for either graphics-only transition.
        if before_release && event.kind == AssetLifecycleKind::GraphicsInvalidated
            || !before_release
                && event.kind != AssetLifecycleKind::Removed
                && event.representation.decoded
        {
            return;
        }
        self.buffers.prepare(
            context.world.world,
            ComponentStorage::buffer_data_source_binding_ptr,
        );
        self.streams.prepare(
            context.world.world,
            ComponentStorage::streaming_data_source_binding_ptr,
        );
        for &(_, binding) in self.buffers.entries() {
            invalidate_asset(
                &mut binding.get_mut(&mut context.world.world.components).runtime,
                event,
            );
        }
        for &(_, binding) in self.streams.entries() {
            invalidate_asset(
                &mut binding.get_mut(&mut context.world.world.components).runtime,
                event,
            );
        }
    }
}

/// Construct an independent binding evaluator for each selected World.
pub struct DataBindingSystemFactory;

impl SystemFactory for DataBindingSystemFactory {
    fn id(&self) -> SystemId {
        DataBindingSystem::ID
    }

    fn capabilities(&self) -> SystemCapabilities {
        SystemCapabilities::new(
            [
                C::BUFFER_DATA_SOURCE_BINDING,
                C::STREAMING_DATA_SOURCE_BINDING,
            ],
            [],
        )
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[
            SystemDependency::Required(SystemId("ipp.asset-dependencies")),
            SystemDependency::After(SystemId("ipp.animation")),
        ]
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(DataBindingSystem::default()))
    }
}

impl System for DataBindingSystem {
    fn before_operation(
        &mut self,
        context: &mut SystemOperationContext<'_>,
    ) -> Result<(), ErrorReason> {
        // Preflight the owned candidate before generic mutation. Commit validation is
        // too late: applied effects survive errors, including a commit-level error.
        use crate::Command;
        let (entity, component, insertion) = match context.command() {
            Command::InsertComponent {
                entity,
                component,
                ..
            } => (entity, *component, true),
            Command::InsertComponentValue {
                entity,
                value,
            } => (entity, value.type_id(), true),
            Command::SetField {
                entity,
                component,
                ..
            }
            | Command::SetFieldIf {
                entity,
                component,
                ..
            } => (entity, *component, false),
            _ => return Ok(()),
        };
        let other = match component {
            C::BUFFER_DATA_SOURCE_BINDING => C::STREAMING_DATA_SOURCE_BINDING,
            C::STREAMING_DATA_SOURCE_BINDING => C::BUFFER_DATA_SOURCE_BINDING,
            _ => return Ok(()),
        };
        let entity = context.resolve_entity(entity)?;
        if insertion
            && context
                .world()
                .component_incarnation(entity, other)
                .is_some()
        {
            return Err(ErrorReason::InvalidValue);
        }
        let previous =
            context
                .staged
                .input_value(&context.world_data.components, entity, component);
        let mut candidate = match context.command() {
            Command::InsertComponentValue {
                value,
                ..
            } => value.as_ref().clone(),
            Command::InsertComponent {
                adopt: false,
                ..
            } => registry::create(component)?,
            Command::InsertComponent {
                ..
            } => previous.clone().unwrap_or(registry::create(component)?),
            _ => previous.clone().ok_or(ErrorReason::MissingComponent)?,
        };
        match context.command() {
            Command::InsertComponent {
                fields,
                ..
            } => {
                for field in fields {
                    registry::assign(&mut candidate, field)?;
                }
            }
            Command::SetField {
                field,
                ..
            }
            | Command::SetFieldIf {
                field,
                ..
            } => {
                // Only source/window changes need service preflight; other fields use
                // the ordinary lifecycle/descriptor validators in generic mutation.
                if crate::components::dynamic_properties::is_dynamic_field(field.offset) {
                    return Ok(());
                }
                registry::assign(&mut candidate, field)?;
            }
            _ => {}
        }
        candidate.validate_lifecycle()?;
        let next = request(&candidate).expect("binding")?;
        if !next.name.is_empty() {
            if let Some(previous) = previous.as_ref().and_then(request).filter(|_| {
                !insertion
                    || matches!(
                        context.command(),
                        Command::InsertComponent {
                            adopt: true,
                            ..
                        }
                    )
            }) {
                context
                    .data_sources()
                    .validate_window_update(&previous?, &next)
            } else {
                context.data_sources().validate_consumer_request(&next)
            }
            .map_err(|_| ErrorReason::InvalidValue)?;
        }
        Ok(())
    }

    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        let changed: Vec<_> = context.changed_components().collect();
        for (entity, component) in changed {
            if !is_binding(component) {
                if !context.retains_component(entity, component) {
                    for binding in [
                        C::BUFFER_DATA_SOURCE_BINDING,
                        C::STREAMING_DATA_SOURCE_BINDING,
                    ] {
                        if let Some(runtime) =
                            runtime_mut(&mut context.world_data.components, entity, binding)
                            && runtime
                                .presentation
                                .is_some_and(|(_, owner, _)| owner == component)
                        {
                            runtime.presentation = None;
                            runtime.dirty = true;
                        }
                    }
                }
                continue;
            }
            let retained = context.retains_component(entity, component);
            let runtime = runtime_mut(&mut context.world_data.components, entity, component);
            if let Some(runtime) = runtime {
                if !retained {
                    if let Some(handle) = runtime.consumer.take() {
                        self.retiring_consumers.push(handle);
                    }
                } else {
                    runtime.invalidate();
                }
            }
        }
        self.buffers
            .before_commit(context, C::BUFFER_DATA_SOURCE_BINDING);
        self.streams
            .before_commit(context, C::STREAMING_DATA_SOURCE_BINDING);
    }

    fn after_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        let changed: Vec<_> = context
            .changed_components()
            .filter(|&(_, component)| is_binding(component))
            .filter_map(|(entity, component)| {
                context
                    .world()
                    .component_incarnation(entity, component)
                    .map(|incarnation| (entity, component, incarnation))
            })
            .collect();
        let world = context.world().reference();
        {
            // Read final committed component values, including retained edits and
            // partial effects of failed batches. No service deferral spans hooks.
            let mut data = DataConsumerAccess {
                service: context.data,
            }
            .batch();
            for (entity, component, incarnation) in changed {
                let identity = DataConsumerIdentity {
                    world,
                    entity,
                    binding_incarnation: incarnation,
                };
                let Some(value) = context
                    .world_data
                    .components
                    .get(component, entity.index() as usize)
                else {
                    continue;
                };
                let Some(Ok(request)) = request(&value) else {
                    continue;
                };
                let runtime = runtime_mut(&mut context.world_data.components, entity, component)
                    .expect("occupied binding");
                if request.name.is_empty() {
                    if let Some(handle) = runtime.consumer.take() {
                        let _ = data.release_consumer(handle);
                    }
                    continue;
                }
                if let Some(handle) = runtime.consumer {
                    if !data
                        .consumer_request(handle)
                        .is_ok_and(|old| old == &request)
                        && let Err(error) = data.update_consumer(handle, request)
                    {
                        runtime.unavailable(DataBindingUnavailable::Source(error));
                    }
                } else {
                    match data.register_consumer(identity, request) {
                        Ok(handle) => runtime.consumer = Some(handle),
                        Err(error) => runtime.unavailable(DataBindingUnavailable::Source(error)),
                    }
                }
            }
            for handle in self.retiring_consumers.drain(..) {
                let _ = data.release_consumer(handle);
            }
        }
        self.buffers.after_commit(
            context,
            C::BUFFER_DATA_SOURCE_BINDING,
            ComponentStorage::buffer_data_source_binding_ptr,
        );
        self.streams.after_commit(
            context,
            C::STREAMING_DATA_SOURCE_BINDING,
            ComponentStorage::streaming_data_source_binding_ptr,
        );
    }

    fn before_numeric_update(&mut self, context: &mut SystemNumericContext<'_>) {
        for &(entity, component) in context.changed {
            if let Some(runtime) =
                runtime_mut(&mut context.world_data.components, entity, component)
            {
                runtime.needs_evaluate = true;
            }
        }
    }

    fn prepare_evaluation(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        self.buffers.prepare(
            context.world.world,
            ComponentStorage::buffer_data_source_binding_ptr,
        );
        self.streams.prepare(
            context.world.world,
            ComponentStorage::streaming_data_source_binding_ptr,
        );
        for &(_, binding) in self.buffers.entries() {
            let runtime = &mut binding.get_mut(&mut context.world.world.components).runtime;
            notify(runtime, context.world.data);
        }
        for &(_, binding) in self.streams.entries() {
            let runtime = &mut binding.get_mut(&mut context.world.world.components).runtime;
            notify(runtime, context.world.data);
        }
    }

    fn before_asset_release(
        &mut self,
        context: &mut SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        self.invalidate_asset_views(context, event, true);
    }

    fn asset_lifecycle(
        &mut self,
        context: &mut SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        self.invalidate_asset_views(context, event, false);
    }

    fn teardown(&mut self, context: &mut SystemTeardownContext<'_>) {
        for handle in self.retiring_consumers.drain(..) {
            let _ = context.data.release_consumer(handle);
        }
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        let tick = context.world.world.tick.saturating_add(1);
        let world = context.world.world.id;
        let dt = context.dt();
        for &(_, binding) in self.buffers.entries() {
            let value = binding.get_mut(&mut context.world.world.components);
            super::update::evaluate(
                &value.properties,
                &mut value.runtime,
                context.world.data,
                context.world.asset_acquisition,
                world,
                tick,
                dt,
            );
        }
        for &(_, binding) in self.streams.entries() {
            let value = binding.get_mut(&mut context.world.world.components);
            super::update::evaluate(
                &value.properties,
                &mut value.runtime,
                context.world.data,
                context.world.asset_acquisition,
                world,
                tick,
                dt,
            );
        }
    }
}

fn notify(runtime: &mut DataBindingRuntime, data: &mut DataService) {
    let Some(handle) = runtime.consumer else {
        return;
    };
    match data.take_notification(handle) {
        Ok(Some(notification)) => {
            if notification.state.source != runtime.source {
                runtime.reset_rows();
            }
            runtime.needs_evaluate |= notification.changed;
            runtime.needs_prepare |=
                notification.availability_changed || notification.state.source != runtime.source;
            runtime.source = notification.state.source;
        }
        Err(error) => runtime.unavailable(DataBindingUnavailable::Source(error)),
        _ => {}
    }
}

fn invalidate_asset(
    runtime: &mut DataBindingRuntime,
    event: &crate::services::asset_management::AssetLifecycleEvent,
) {
    let affected = runtime
        .columns
        .iter()
        .find(|column| column.asset == event.key)
        .map(|column| column.name.clone());
    if let Some(output) = affected {
        runtime.invalidate();
        // The pre-release hook drops shared plans/scratch even for a frozen World.
        // Missing definitions remain pending preparation, including after late load.
        runtime.unavailable(DataBindingUnavailable::MissingAsset {
            output,
        });
    }
}

pub(super) fn is_binding(component: u16) -> bool {
    matches!(
        component,
        C::BUFFER_DATA_SOURCE_BINDING | C::STREAMING_DATA_SOURCE_BINDING
    )
}

pub(super) fn runtime_mut(
    storage: &mut ComponentStorage,
    entity: crate::EntityId,
    component: u16,
) -> Option<&mut DataBindingRuntime> {
    match component {
        C::BUFFER_DATA_SOURCE_BINDING => storage
            .buffer_data_source_binding_mut(entity.index() as usize)
            .map(|v| &mut v.runtime),
        C::STREAMING_DATA_SOURCE_BINDING => storage
            .streaming_data_source_binding_mut(entity.index() as usize)
            .map(|v| &mut v.runtime),
        _ => None,
    }
}
