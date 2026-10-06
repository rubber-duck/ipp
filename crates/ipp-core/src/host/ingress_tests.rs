use super::*;
use crate::systems::*;
use std::sync::Arc;

struct LocalFactory;

struct LocalSystem;

impl SystemFactory for LocalFactory {
    fn id(&self) -> SystemId {
        SystemId("fixture.local-read")
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(LocalSystem))
    }
}

impl System for LocalSystem {
    fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}

    fn command(
        &mut self,
        context: &mut SystemCommandContext<'_>,
        _: u64,
        _: &dyn std::any::Any,
    ) -> Result<(), ErrorReason> {
        let world = context.world.id();
        let view = context.host_ingress().unwrap();
        assert_eq!(view.local.world_ref().id(), world);
        assert!(view.world(view.local.world_ref()).is_some());
        Ok(())
    }
}

#[test]
fn ordinary_local_commands_and_groups_never_scan_foreign_worlds() {
    for count in [1, 128, 512] {
        let mut host = HostRuntime::with_system_factories(vec![Arc::new(LocalFactory)]).unwrap();
        for _ in 0..count {
            let world = host
                .create_world(Default::default(), &[SystemId("fixture.local-read")])
                .unwrap();
            let mut context = host.world_mut(world).unwrap();
            context
                .enqueue_system_command(SystemId("fixture.local-read"), 1, ())
                .unwrap();
            context
                .enqueue_system_command_batch_with_reply(
                    SystemId("fixture.local-read"),
                    1,
                    1,
                    vec![(), ()],
                )
                .unwrap();
        }
        let frame = host.frame(0.0).unwrap();
        assert_eq!(frame.worlds.len(), count);
        assert!(
            frame
                .worlds
                .values()
                .all(|report| report.as_ref().unwrap().system_command_outcomes[0].applied == 2)
        );
        assert_eq!(host.topology.foreign_world_scans, 0);
    }
}
