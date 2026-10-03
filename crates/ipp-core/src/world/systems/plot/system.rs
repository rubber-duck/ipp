//! Plot capability registration. Retained evaluation is added behind the pure seams.

use crate::ComponentValue as C;
use crate::systems::*;

/// One World's data-bound Plot preparation owner.
#[derive(Default)]
pub struct PlotSystem {
    pub(super) state: super::system_state::PlotSystemState,
}

impl PlotSystem {
    /// Stable compiled composition identity.
    pub const ID: SystemId = SystemId("ipp.plot");
}

/// Reusable factory; each World owns independent derived results.
pub struct PlotSystemFactory;

impl SystemFactory for PlotSystemFactory {
    fn id(&self) -> SystemId {
        PlotSystem::ID
    }

    fn capabilities(&self) -> SystemCapabilities {
        let mut capabilities = SystemCapabilities::new([], []);
        capabilities.components.extend(
            [
                C::PLOT_FRAME2D,
                C::PLOT_LINE2D,
                C::PLOT_BARS2D,
                C::PLOT_PIE2D,
            ]
            .map(|component| {
                SystemCapability::requiring(component, [crate::systems::canvas::CanvasSystem::ID])
            }),
        );
        capabilities.components.extend(
            [
                C::PLOT_FRAME3D,
                C::PLOT_GRID_BARS3D,
                C::PLOT_HEIGHT_SURFACE3D,
                C::PLOT_POINTS3D,
                C::PLOT_PIE3D,
            ]
            .map(|component| {
                SystemCapability::requiring(
                    component,
                    [crate::systems::geometry::GeometrySystem::ID],
                )
            }),
        );
        capabilities
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[
            SystemDependency::Required(crate::systems::data_bindings::DataBindingSystem::ID),
            SystemDependency::After(crate::systems::hierarchy::FinalPropagationSystem::ID),
            SystemDependency::After(crate::systems::gui::GuiLayoutSystem::ID),
        ]
    }

    fn create(
        &self,
        _context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(PlotSystem::default()))
    }
}

impl System for PlotSystem {
    fn before_operation(
        &mut self,
        context: &mut SystemOperationContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        let (reference, component) = match context.command() {
            crate::Command::InsertComponent {
                entity,
                component,
                ..
            } => (entity, *component),
            crate::Command::InsertComponentValue {
                entity,
                value,
            } => (entity, value.type_id()),
            _ => return Ok(()),
        };
        if super::update::is_chart(component) {
            let entity = context.resolve_entity(reference)?;
            if super::update::CHARTS.iter().any(|other| {
                *other != component
                    && context
                        .world()
                        .component_incarnation(entity, *other)
                        .is_some()
            }) {
                return Err(crate::ErrorReason::InvalidValue);
            }
        }
        Ok(())
    }

    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        for (entity, component) in context.changed_components() {
            if super::update::is_chart(component)
                || super::update::is_frame(component)
                || matches!(
                    component,
                    C::BUFFER_DATA_SOURCE_BINDING | C::STREAMING_DATA_SOURCE_BINDING
                )
            {
                self.state.dirty.insert(entity);
                self.state.membership_dirty = true;
                if super::update::is_chart(component)
                    && !context.retains_component(entity, component)
                {
                    self.state.remove((entity, component));
                }
            }
        }
    }

    fn before_numeric_update(&mut self, context: &mut SystemNumericContext<'_>) {
        for &(entity, component) in context.changed_components() {
            if super::update::is_chart(component) || super::update::is_frame(component) {
                self.state.dirty.insert(entity);
            }
        }
    }

    fn before_asset_release(
        &mut self,
        _context: &mut SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        if event.kind != crate::services::asset_management::AssetLifecycleKind::GraphicsInvalidated
        {
            self.invalidate_fonts(event.key);
        }
    }

    fn asset_lifecycle(
        &mut self,
        _context: &mut SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        if event.kind != crate::services::asset_management::AssetLifecycleKind::GraphicsInvalidated
        {
            self.state
                .dirty
                .extend(self.state.members.iter().map(|(entity, _)| *entity));
        }
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        self.evaluate(context);
    }

    fn publish_output(
        &self,
        world: &crate::WorldContext<'_>,
        output: &mut crate::host::WorldOutputBuilder<'_>,
    ) -> Result<(), crate::ErrorReason> {
        self.publish(world, output)
    }
}

impl PlotSystem {
    /// Local presentation revision, excluding camera and spatial placement changes.
    pub fn presentation_revision(&self) -> u64 {
        self.state.revision
    }

    /// Completed 2D local primitives for the ordinary Canvas walk.
    pub(crate) fn canvas_charts(
        &self,
        entity: crate::EntityId,
    ) -> impl Iterator<
        Item = (
            crate::systems::canvas::CanvasTarget,
            &[crate::systems::canvas::CanvasPrimitive],
            &std::sync::Arc<super::PlotPreparedGeometry>,
        ),
    > {
        self.state
            .charts
            .range((entity, 0)..=(entity, u16::MAX))
            .filter(|(_, chart)| !chart.canvas.is_empty())
            .map(|(_, chart)| (chart.target, chart.canvas.as_ref(), &chart.geometry))
    }

    /// Retained chart-local scene enclosure; placement is evaluated by GeometrySystem.
    pub(crate) fn local_bounds(&self, entity: crate::EntityId) -> Option<[[f32; 3]; 2]> {
        self.state
            .charts
            .range((entity, 0)..=(entity, u16::MAX))
            .find_map(|(_, chart)| chart.geometry.bounds)
    }

    fn invalidate_fonts(&mut self, key: crate::services::asset_management::AssetKey) {
        let affected: Vec<_> = self
            .state
            .charts
            .iter()
            .filter(|(_, chart)| {
                chart
                    .canvas
                    .iter()
                    .chain(
                        chart
                            .planes
                            .iter()
                            .flat_map(|plane| plane.primitives.iter()),
                    )
                    .any(|primitive| primitive.resource() == Some(key))
            })
            .map(|(key, _)| *key)
            .collect();
        for key in affected {
            self.state.dirty.insert(key.0);
            self.state.remove(key);
        }
    }
}
