//! World pass retaining GUI constraint evaluation across frames.
//!
//! [`GuiLayoutSystem`] runs after the GUI state and animation passes and
//! retains the evaluated geometry of ordinary `GuiLayout` entities for Canvas
//! production. The pass writes the scroll geometry, wanted range and
//! normalized position fields of scrolling controls it evaluated; it never
//! issues client commands and never advances simulation time.

use crate::ComponentValue;
use crate::systems::{
    System, SystemDependency, SystemFactory, SystemId, SystemInitContext, SystemInitError,
};

/// Retained GUI layout pass. See the module documentation for the pass
/// contract.
#[derive(Default)]
pub struct GuiLayoutSystem {
    gui: Option<crate::systems::SystemDependencyBinding<super::super::GuiSystem>>,
    pub(super) entity_layout: super::entity_layout::GuiEntityLayoutState,
}

impl GuiLayoutSystem {
    /// Stable system identity.
    pub const ID: SystemId = SystemId("ipp.gui-layout");

    pub(in crate::world::systems) fn entity_view(
        &self,
    ) -> Option<&std::sync::Arc<super::entity_layout::GuiEntityLayoutView>> {
        self.entity_layout.view.as_ref()
    }
}

/// Factory for [`GuiLayoutSystem`].
#[derive(Default)]
pub struct GuiLayoutSystemFactory;

impl SystemFactory for GuiLayoutSystemFactory {
    fn id(&self) -> SystemId {
        GuiLayoutSystem::ID
    }

    fn capabilities(&self) -> crate::systems::SystemCapabilities {
        let mut capabilities = crate::systems::SystemCapabilities::default();
        capabilities
            .components
            .push(crate::systems::SystemCapability::requiring(
                ComponentValue::GUI_LAYOUT,
                [crate::systems::canvas::CanvasSystem::ID],
            ));
        capabilities
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[
            SystemDependency::Required(super::super::GuiSystem::ID),
            SystemDependency::After(crate::systems::animation::AnimationSystem::ID),
            SystemDependency::After(crate::systems::surface::SurfaceSystem::ID),
        ]
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(GuiLayoutSystem {
            gui: Some(context.dependency::<super::super::GuiSystem>(super::super::GuiSystem::ID)?),
            entity_layout: Default::default(),
        }))
    }
}

impl System for GuiLayoutSystem {
    fn before_numeric_update(&mut self, context: &mut crate::systems::SystemNumericContext<'_>) {
        for &(entity, component) in context.changed_components() {
            self.entity_layout.dirty(entity, component);
        }
    }

    fn before_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        self.entity_layout.before_commit(context);
    }

    fn before_asset_release(
        &mut self,
        _context: &mut crate::systems::SystemAssetContext<'_>,
        _event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        self.entity_layout.resources_changed();
    }

    fn asset_lifecycle(
        &mut self,
        _context: &mut crate::systems::SystemAssetContext<'_>,
        _event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        self.entity_layout.resources_changed();
    }

    fn update(&mut self, context: &mut crate::systems::SystemUpdateContext<'_, '_>) {
        self.entity_layout.update(
            &context.world,
            self.gui
                .and_then(|binding| context.world.dependency(binding)),
        );
        let world = &mut *context.world.world;
        self.entity_layout
            .write_scroll_fields(&mut world.components, &world.state);
    }
}

crate::system_parameter!(GuiLayoutSystem);
