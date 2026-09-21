//! Retain sources hidden by other overlay declarations.

use super::StateOverlayEntry;
use super::registry::ComponentStateOverlay;
use crate::{
    services::asset_management::service::AssetDemandSelection, world::WorldComponentState,
};
use std::collections::BTreeSet;

impl super::StateOverlaySystem {
    pub(in crate::world) fn component_resource_demand(
        &self,
        layer: &WorldComponentState,
        demand: &mut BTreeSet<AssetDemandSelection>,
    ) {
        for id in &layer.inputs.overlay_handles {
            if let Some(StateOverlayEntry::Component(ComponentStateOverlay {
                active: true,
                fields,
                ..
            })) = self.state.registry.borrow(*id)
                && let Some(input) = layer
                    .inputs
                    .base_value
                    .as_ref()
                    .or(layer.inputs.fallback_value.as_ref())
            {
                let mut value = input.as_ref().clone();
                for field in fields {
                    let _ = crate::components::registry::write(&mut value, field);
                }
                value.resource_demand(demand);
            }
        }
    }
}
