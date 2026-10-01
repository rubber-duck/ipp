use crate::{Batch, Command, HostRuntime, services::asset_management::*, systems::*};
use std::{
    any::Any,
    sync::{Arc, Mutex},
};

type Trace = Arc<Mutex<Vec<String>>>;

struct Payload(Trace);

impl Asset for Payload {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        self
    }

    fn resident_bytes(&self) -> usize {
        1
    }
}

impl Drop for Payload {
    fn drop(&mut self) {
        self.0.lock().unwrap().push("drop".into());
    }
}

struct ObserverFactory(Trace);

struct Observer(Trace);

impl SystemFactory for ObserverFactory {
    fn id(&self) -> SystemId {
        SystemId("fixture.strengthening")
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(Observer(self.0.clone())))
    }
}

impl System for Observer {
    fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}

    fn before_asset_release(
        &mut self,
        context: &mut SystemAssetContext<'_>,
        event: &AssetLifecycleEvent,
    ) {
        assert!(
            context
                .world
                .asset_resources()
                .get_typed::<Payload>(event.key)
                .is_some()
        );
        let world = context.world.id();
        self.0
            .lock()
            .unwrap()
            .push(format!("before:{}:{:?}", world.0, event.kind));
        if world.0 == 2 && event.kind == AssetLifecycleKind::StatusChanged {
            context.world.asset_acquisition.revoke_resource(event.key);
        }
    }

    fn asset_lifecycle(
        &mut self,
        context: &mut SystemAssetContext<'_>,
        event: &AssetLifecycleEvent,
    ) {
        if event.kind == AssetLifecycleKind::Removed {
            assert!(context.world.asset_resources().get(event.key).is_none());
            self.0
                .lock()
                .unwrap()
                .push(format!("removed:{}", context.world.id().0));
        }
    }
}

#[test]
fn strengthening_redrains_every_world_while_prepared_or_queued() {
    let trace = Trace::default();
    let mut host =
        HostRuntime::with_system_factories(vec![Arc::new(ObserverFactory(trace.clone()))]).unwrap();
    let first = host
        .create_world(Default::default(), &[SystemId("fixture.strengthening")])
        .unwrap();
    let second = host
        .create_world(Default::default(), &[SystemId("fixture.strengthening")])
        .unwrap();
    let trace_for_loader = trace.clone();
    let kind = AssetTypeId(65000);
    host.asset_resources_mut()
        .register_loader(kind, move || {
            let trace = trace_for_loader.clone();
            BufferedAssetLoader::new(move |bytes| {
                assert_eq!(bytes, [1]);
                Ok(Payload(trace.clone()))
            })
        })
        .unwrap();
    let upload = AssetUploadIdentity {
        kind,
        asset: 1,
        variant: 0,
    };
    let key = host.asset_resources_mut().upload(upload, vec![1]).unwrap();
    host.progress_assets();
    assert!(host.asset_resources().get_typed::<Payload>(key).is_some());
    let lease = host
        .asset_resources_mut()
        .retain_publication([key])
        .unwrap();

    host.world_mut(first).unwrap().prepare_update(0.0).unwrap();
    host.world_mut(second)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations: vec![Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: false,
            }],
        })
        .unwrap();
    assert!(host.has_pending_world_updates());

    host.asset_resources_mut().unload(key);
    assert!(host.asset_resources().get_typed::<Payload>(key).is_some());
    host.flush_resource_lifecycle();
    assert_eq!(
        *trace.lock().unwrap(),
        [
            "before:1:StatusChanged",
            "before:2:StatusChanged",
            "before:1:Removed",
            "before:2:Removed",
            "drop",
            "removed:1",
            "removed:2",
        ]
    );
    assert!(host.asset_resources().get(key).is_none());
    assert!(
        host.asset_resources()
            .publication_resource(lease, key)
            .is_none()
    );
    assert!(host.has_pending_world_updates());
    host.world_mut(first).unwrap().step(0.0).unwrap();
    let report = host.world_mut(second).unwrap().step(0.0).unwrap();
    assert!(report.outcomes[0].result.is_ok());
    let replacement = host.asset_resources_mut().upload(upload, vec![1]).unwrap();
    assert_eq!(replacement.slot, key.slot);
    assert_ne!(replacement, key);
    host.progress_assets();
    assert!(
        host.asset_resources()
            .publication_resource(lease, replacement)
            .is_none()
    );
    assert!(host.asset_resources_mut().release_publication(lease));
}
