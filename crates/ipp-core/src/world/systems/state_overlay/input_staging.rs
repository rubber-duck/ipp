use super::{StateOverlayBatch, StateOverlayEntry};
use crate::{Command, ErrorReason, components::registry};

impl super::StateOverlayMutationAccess<'_> {
    pub(in crate::world) fn stage_overlay_inputs(
        &mut self,
        components: &registry::ComponentStorage,
        command: &Command,
        batch: &StateOverlayBatch,
    ) -> Result<(), ErrorReason> {
        let keys: Vec<_> = match command {
            Command::AttachComponentStateOverlay {
                binding,
                component,
                ..
            } => match self.state_overlays.registry.get(batch.resolve(*binding)?)? {
                Some(StateOverlayEntry::EntityBinding(binding)) => {
                    vec![(binding.entity, *component)]
                }
                _ => Vec::new(),
            },
            Command::UpdateComponentStateOverlay {
                overlay,
                ..
            }
            | Command::UpdateDynamicComponentStateOverlay {
                overlay,
                ..
            }
            | Command::ReleaseComponentStateOverlay {
                overlay,
                ..
            } => match self.state_overlays.registry.get(batch.resolve(*overlay)?)? {
                Some(StateOverlayEntry::Component(overlay)) => {
                    vec![(overlay.entity, overlay.component)]
                }
                _ => Vec::new(),
            },
            Command::ReleaseEntityOverlayBinding {
                binding,
                ..
            } => {
                let binding = batch.resolve(*binding)?;
                self.state_overlays
                    .registry
                    .iter()
                    .filter_map(|(_, entry)| match entry {
                        StateOverlayEntry::Component(overlay) if overlay.binding == binding => {
                            Some((overlay.entity, overlay.component))
                        }
                        _ => None,
                    })
                    .collect()
            }
            Command::ReleaseStateOverlayOwner {
                owner,
            } => {
                let owner = batch.resolve(*owner)?;
                self.state_overlays
                    .registry
                    .iter()
                    .filter_map(|(_, entry)| match entry {
                        StateOverlayEntry::Component(overlay) if overlay.owner == owner => {
                            Some((overlay.entity, overlay.component))
                        }
                        _ => None,
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        for (entity, component) in keys {
            self.stage_component(components, entity, component);
        }
        Ok(())
    }
}
