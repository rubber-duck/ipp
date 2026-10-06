//! Asset lifecycle dispatch to every selected System in schedule order.

use crate::world::WorldContext;
use crate::world::context::SystemInstanceAccess;
use crate::world::systems;

impl WorldContext<'_> {
    pub(crate) fn dispatch_asset_lifecycle(
        &mut self,
        event: &crate::services::asset_management::AssetLifecycleEvent,
        before_release: bool,
    ) {
        let tick = self.world.tick;
        for index in 0..self.instances.before.len() {
            let (before, rest) = self.instances.before.split_at_mut(index);
            let (current, after) = rest.split_first_mut().expect("schedule index");
            let mut context = systems::SystemAssetContext {
                world: systems::SystemRuntimeAccess {
                    world: self.world,
                    instances: SystemInstanceAccess {
                        before,
                        current: Some(current.id),
                        after,
                    },
                    asset_acquisition: self.asset_acquisition,
                    io: self.io,
                    data: self.data,
                    topology: self.topology,
                    frame_context: self.frame_context,
                    reference_worlds: self.reference_worlds.as_ref(),
                },
                tick,
            };
            if before_release {
                current.system.before_asset_release(&mut context, event);
            } else {
                current.system.asset_lifecycle(&mut context, event);
            }
        }
    }
}

#[cfg(test)]
#[path = "asset_lifecycle_tests.rs"]
mod tests;
